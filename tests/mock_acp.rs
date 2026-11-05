//! Runs the real `acp::client` and `acp::manager` against the offline mock agent
//! (`src/bin/mock_acp_agent.rs`). No network, no real LLM.
//!
//! Locust is a bin-only crate, so the ACP modules are pulled in by path instead of a lib.
//! Tests share process-wide env (MOCK_ACP_MODE) and cwd (the session store is relative),
//! so every test takes `serial()` first.
#![allow(dead_code)]

#[path = "../src/acp/mod.rs"]
mod acp;

use acp::client::{self, AcpError, AgentSpec, SessionId};
use acp::manager::{AgentOutcome, Manager};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

const MOCK: AgentSpec = AgentSpec {
    id: "acp-mock",
    name: "Mock ACP",
    command: env!("CARGO_BIN_EXE_mock_acp_agent"),
    args: &[],
};

static LOCK: Mutex<()> = Mutex::new(());

/// Holds the serial lock; restores a clean env and a private cwd for the test.
struct Env {
    _guard: MutexGuard<'static, ()>,
}

fn serial(mode: &str) -> Env {
    let guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let dir = std::env::temp_dir().join(format!(
        "locust-mock-acp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: all tests hold LOCK, so nothing else touches env or cwd concurrently.
    unsafe {
        std::env::set_var("MOCK_ACP_MODE", mode);
        std::env::set_var("MOCK_ACP_SLOW_SECS", "25");
    }
    std::env::set_current_dir(&dir).unwrap();
    Env { _guard: guard }
}

const WAIT: Duration = Duration::from_secs(30);

fn read_store() -> serde_json::Value {
    let text = std::fs::read_to_string(".state/acp_sessions.json").expect("session store");
    serde_json::from_str(&text).unwrap()
}

#[test]
fn handshake_reports_mock_agent() {
    let _e = serial("ok");
    let (conn, info) = client::connect(&MOCK).expect("handshake");
    assert_eq!(info.protocol_version, 1);
    assert_eq!(info.agent_name.as_deref(), Some("mock-acp-agent"));
    assert_eq!(info.agent_version.as_deref(), Some("0.0.1"));
    assert!(info.capabilities.iter().any(|c| c == "loadSession"));
    assert!(info.auth_methods.is_empty());
    conn.shutdown();
}

#[test]
fn one_turn_through_client() {
    let _e = serial("ok");
    let (mut conn, _) = client::connect(&MOCK).unwrap();
    let sid = client::new_session(&mut conn, ".").unwrap();
    let (reply, _usage) = client::send_prompt(&mut conn, sid, "hello").unwrap();
    assert_eq!(reply, "mock: hello");
    conn.shutdown();
}

#[test]
fn one_turn_through_manager() {
    let _e = serial("ok");
    let mut manager = Manager::new();
    let rx = manager.turn(MOCK, "pane-a".into(), "hi there".into());
    let (reply, _) = rx
        .recv_timeout(WAIT)
        .expect("turn finished")
        .expect("turn ok");
    assert_eq!(reply, "mock: hi there");

    // The actor reported the handshake and the connect-time probe.
    let updates = manager.poll();
    assert!(
        updates
            .iter()
            .any(|u| matches!(u.outcome, AgentOutcome::Connected(_)))
    );
    assert!(updates.iter().any(|u| {
        matches!(
            u.outcome,
            AgentOutcome::UsageProbed(_) | AgentOutcome::ProbeFailed(_)
        )
    }));
    manager.disconnect("acp-mock");
}

#[test]
fn permission_request_is_answered_and_turn_completes() {
    let _e = serial("permission");
    let (mut conn, _) = client::connect(&MOCK).unwrap();
    let sid = client::new_session(&mut conn, ".").unwrap();
    // The client auto-rejects the agent's permission request; the mock must accept that
    // response and still finish the turn.
    let (reply, _) = client::send_prompt(&mut conn, sid, "ask").unwrap();
    assert_eq!(reply, "mock: ask");
    conn.shutdown();
}

#[test]
fn permission_denied_never_grants_access() {
    let _e = serial("permission");
    let (mut conn, _) = client::connect(&MOCK).unwrap();
    conn.permission_handler = Some(std::sync::Arc::new(|_req| client::PermissionDecision::Deny));
    let sid = client::new_session(&mut conn, ".").unwrap();
    let (reply, _) = client::send_prompt(&mut conn, sid, "write_file").unwrap();
    assert_eq!(reply, "mock: write_file");
    assert!(!reply.contains("(access granted)"));
    conn.shutdown();
}

#[test]
fn permission_allowed_grants_access() {
    let _e = serial("permission");
    let (mut conn, _) = client::connect(&MOCK).unwrap();
    conn.permission_handler = Some(std::sync::Arc::new(|_req| {
        client::PermissionDecision::Allow
    }));
    let sid = client::new_session(&mut conn, ".").unwrap();
    let (reply, _) = client::send_prompt(&mut conn, sid, "read_file").unwrap();
    assert_eq!(reply, "mock: read_file (access granted)");
    conn.shutdown();
}

#[test]
fn slow_agent_times_out_the_handshake() {
    let _e = serial("slow");
    let start = Instant::now();
    let result = client::connect(&MOCK);
    assert!(matches!(result, Err(AcpError::Timeout)));
    assert!(
        start.elapsed() >= Duration::from_secs(15),
        "returned too early"
    );
}

#[test]
fn crash_after_first_prompt_fails_next_call() {
    let _e = serial("crash");
    let (mut conn, _) = client::connect(&MOCK).unwrap();
    let sid = client::new_session(&mut conn, ".").unwrap();
    let (reply, _) = client::send_prompt(&mut conn, sid.clone(), "one").unwrap();
    assert_eq!(reply, "mock: one"); // the first prompt is still answered
    assert!(client::send_prompt(&mut conn, sid, "two").is_err());
    conn.shutdown();
}

#[test]
fn crash_surfaces_as_error_through_manager() {
    let _e = serial("crash");
    let mut manager = Manager::new();
    // Connect-time probe uses up the single allowed prompt, then the agent exits.
    let first = manager
        .turn(MOCK, "pane-a".into(), "one".into())
        .recv_timeout(WAIT);
    assert!(
        matches!(first, Ok(Err(_))),
        "turn after the crash must fail, got {first:?}"
    );
    manager.disconnect("acp-mock");
}

#[test]
fn client_loads_known_session_and_rejects_unknown() {
    let _e = serial("ok");
    let (mut conn, _) = client::connect(&MOCK).unwrap();
    let sid = client::new_session(&mut conn, ".").unwrap();
    client::load_session(&mut conn, sid, ".").expect("known id loads");
    let bad = client::load_session(&mut conn, SessionId::from("bogus".to_string()), ".");
    assert!(matches!(bad, Err(AcpError::Protocol(_))));
    conn.shutdown();
}

#[test]
fn manager_resumes_saved_session_after_restart() {
    let _e = serial("ok");
    let key = "acp-mock::pane-a";

    let mut first = Manager::new();
    first
        .ensure_session(MOCK, "pane-a".into())
        .recv_timeout(WAIT)
        .unwrap()
        .unwrap();
    let saved = read_store()[key].as_str().unwrap().to_string();
    assert!(saved.starts_with("mock-session-"));
    first.disconnect("acp-mock");

    // New Manager = new mock process (new pid). A fresh session would get a different id
    // and overwrite the store; a resumed one leaves it untouched.
    let mut second = Manager::new();
    second
        .ensure_session(MOCK, "pane-a".into())
        .recv_timeout(WAIT)
        .unwrap()
        .unwrap();
    assert_eq!(read_store()[key].as_str().unwrap(), saved);
    second.disconnect("acp-mock");
}

#[test]
fn manager_replaces_stale_saved_session() {
    let _e = serial("ok");
    std::fs::create_dir_all(".state").unwrap();
    std::fs::write(
        ".state/acp_sessions.json",
        r#"{"acp-mock::pane-b":"stale-id"}"#,
    )
    .unwrap();

    let mut manager = Manager::new();
    manager
        .ensure_session(MOCK, "pane-b".into())
        .recv_timeout(WAIT)
        .unwrap()
        .unwrap();
    let now = read_store()["acp-mock::pane-b"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(now, "stale-id");
    assert!(now.starts_with("mock-session-"));
    manager.disconnect("acp-mock");
}

#[test]
fn streaming_turn_reports_thinking_tool_and_text() {
    use acp::client::TurnEvent;
    let _e = serial("think");
    let (mut conn, _) = client::connect(&MOCK).unwrap();
    let sid = client::new_session(&mut conn, ".").unwrap();
    let mut events = Vec::new();
    let (reply, _) =
        client::send_prompt_streaming(&mut conn, sid, "go", client::TurnLimits::default(), |ev| {
            events.push(ev)
        })
        .unwrap();
    assert_eq!(reply, "mock: go");
    assert!(events.contains(&TurnEvent::Thought("Let me look at the spec.\n".into())));
    assert!(events.contains(&TurnEvent::ToolStarted("Read SPEC.md".into())));
    assert!(events.contains(&TurnEvent::ToolUpdate {
        title: None,
        done: true
    }));
    assert!(events.contains(&TurnEvent::Message("mock: go".into())));
    conn.shutdown();
}

#[test]
fn manager_streams_progress_while_turn_runs() {
    let _e = serial("think");
    let mut manager = Manager::new();
    let (rx, progress) = manager.turn_streaming(MOCK, "pane-a".into(), "go".into());
    let result = rx.recv_timeout(WAIT).expect("turn should finish");
    assert_eq!(result.unwrap().0, "mock: go");
    // The progress channel closes when the turn ends, with every event already queued.
    let events: Vec<_> = progress.try_iter().collect();
    assert!(
        events.len() >= 5,
        "expected thought, tool and text events, got {events:?}"
    );
}

fn set_env(name: &str, value: &str) {
    // SAFETY: every test holds LOCK, so nothing else touches env concurrently.
    unsafe { std::env::set_var(name, value) };
}

#[test]
fn idle_limit_cancels_a_silent_turn_and_names_the_limit() {
    let _e = serial("stall");
    set_env("MOCK_ACP_STALL_SECS", "10");
    let (mut conn, _) = client::connect(&MOCK).unwrap();
    let sid = client::new_session(&mut conn, ".").unwrap();
    let limits = client::TurnLimits::from_secs(2, 60);
    let start = Instant::now();
    let err = client::send_prompt_streaming(&mut conn, sid, "go", limits, |_| {}).unwrap_err();
    assert!(matches!(err, AcpError::Idle(2)), "got {err:?}");
    assert!(err.to_string().contains("idle limit"));
    // 2s idle plus at most a 3s wait for the cancel to be acknowledged, not the 10s stall.
    assert!(
        start.elapsed() < Duration::from_secs(9),
        "took {:?}",
        start.elapsed()
    );
    conn.shutdown();
}

#[test]
fn a_streaming_turn_is_not_cut_by_the_idle_limit() {
    let _e = serial("drip");
    set_env("MOCK_ACP_DRIP_SECS", "5");
    let (mut conn, _) = client::connect(&MOCK).unwrap();
    let sid = client::new_session(&mut conn, ".").unwrap();
    // The turn lasts 5s, longer than the 2s idle limit, but never goes silent for 2s.
    let limits = client::TurnLimits::from_secs(2, 60);
    let (reply, _) = client::send_prompt_streaming(&mut conn, sid, "go", limits, |_| {}).unwrap();
    assert!(reply.contains("4 "), "got {reply:?}");
    conn.shutdown();
}

#[test]
fn total_limit_caps_a_turn_that_keeps_streaming() {
    let _e = serial("drip");
    set_env("MOCK_ACP_DRIP_SECS", "8");
    let (mut conn, _) = client::connect(&MOCK).unwrap();
    let sid = client::new_session(&mut conn, ".").unwrap();
    let limits = client::TurnLimits::from_secs(2, 3);
    let err = client::send_prompt_streaming(&mut conn, sid, "go", limits, |_| {}).unwrap_err();
    assert!(matches!(err, AcpError::TotalLimit(3)), "got {err:?}");
    assert!(err.to_string().contains("total limit"));
    conn.shutdown();
}

#[test]
fn manager_applies_configured_turn_limits() {
    let _e = serial("stall");
    set_env("MOCK_ACP_STALL_SECS", "10");
    let mut manager = Manager::new();
    manager.set_turn_limits(client::TurnLimits::from_secs(2, 60));
    let rx = manager.turn(MOCK, "pane-a".into(), "go".into());
    let result = rx.recv_timeout(WAIT).expect("turn should end");
    let msg = result.unwrap_err();
    assert!(msg.contains("idle limit"), "got {msg}");
}
