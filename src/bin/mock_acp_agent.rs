//! Offline mock ACP agent for tests. Speaks NDJSON JSON-RPC over stdio.
//!
//! Methods: initialize, session/new, session/load, session/prompt, session/cancel.
//! Behavior is picked by the MOCK_ACP_MODE env var:
//!   ok          (default) answer normally
//!   slow        sleep 60s before every reply (MOCK_ACP_SLOW_SECS overrides the 60)
//!   crash       exit right after answering the first session/prompt
//!   permission  send session/request_permission and wait for its response
//!               before answering each session/prompt

use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::time::Duration;

/// Default sleep in slow mode. MOCK_ACP_SLOW_SECS overrides it (tests use a shorter value
/// so an orphaned mock does not hold its exe locked for a full minute).
const SLOW_SECS_DEFAULT: u64 = 60;
/// Id used for the agent->client permission request. High, so it never clashes with the
/// small ids the client uses for its own requests.
const PERMISSION_REQUEST_ID: i64 = 9000;
/// Session ids start with this; session/load accepts only ids with this prefix.
const SESSION_PREFIX: &str = "mock-session-";

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Ok,
    Slow,
    Crash,
    Permission,
    Think,
    /// Send one thought, then say nothing for MOCK_ACP_STALL_SECS before answering.
    Stall,
    /// Send a text chunk every second for MOCK_ACP_DRIP_SECS, then answer.
    Drip,
}

impl Mode {
    fn from_env() -> Self {
        match std::env::var("MOCK_ACP_MODE").as_deref() {
            Ok("slow") => Mode::Slow,
            Ok("crash") => Mode::Crash,
            Ok("permission") => Mode::Permission,
            Ok("think") => Mode::Think,
            Ok("stall") => Mode::Stall,
            Ok("drip") => Mode::Drip,
            _ => Mode::Ok,
        }
    }
}

fn send(value: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

fn respond(id: &Value, result: Value) {
    send(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
}

fn respond_error(id: &Value, code: i64, message: &str) {
    send(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}));
}

fn update(session_id: &str, body: Value) {
    send(&json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {"sessionId": session_id, "update": body}
    }));
}

fn thought_chunk(session_id: &str, text: &str) {
    update(
        session_id,
        json!({
            "sessionUpdate": "agent_thought_chunk",
            "content": {"type": "text", "text": text}
        }),
    );
}

fn message_chunk(session_id: &str, text: &str) {
    send(&json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": session_id,
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": text}
            }
        }
    }));
}

/// Join every text block of a session/prompt `prompt` array.
fn prompt_text(params: &Value) -> String {
    params
        .get("prompt")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

/// Ask the client for permission, then block until its response arrives.
/// Returns true if permission was granted ("allow"), false otherwise.
/// Any other line read while waiting is dropped.
fn request_permission(session_id: &str, lines: &mut impl Iterator<Item = String>) -> bool {
    send(&json!({
        "jsonrpc": "2.0",
        "id": PERMISSION_REQUEST_ID,
        "method": "session/request_permission",
        "params": {
            "sessionId": session_id,
            "toolCall": {"toolCallId": "mock-tool-1", "title": "mock tool call"},
            "options": [
                {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                {"optionId": "reject", "name": "Reject", "kind": "reject_once"}
            ]
        }
    }));
    for line in lines {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        if v.get("id").and_then(Value::as_i64) == Some(PERMISSION_REQUEST_ID)
            && v.get("method").is_none()
        {
            let granted = v
                .get("result")
                .and_then(|r| r.get("outcome"))
                .and_then(|o| o.get("optionId"))
                .and_then(Value::as_str)
                == Some("allow");
            return granted;
        }
    }
    false
}

fn main() {
    let mode = Mode::from_env();
    let pid = std::process::id();
    let slow_sleep = Duration::from_secs(
        std::env::var("MOCK_ACP_SLOW_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(SLOW_SECS_DEFAULT),
    );
    let mut session_counter = 0u32;
    let mut prompts_seen = 0u32;

    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines().map_while(Result::ok);

    while let Some(line) = lines.next() {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(method) = msg.get("method").and_then(Value::as_str) else {
            continue; // a stray response; nothing to do
        };
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        // Notifications (no id) get no reply. session/cancel is one.
        let Some(id) = id else { continue };

        if mode == Mode::Slow {
            std::thread::sleep(slow_sleep);
        }

        match method {
            "initialize" => respond(
                &id,
                json!({
                    "protocolVersion": 1,
                    "agentCapabilities": {"loadSession": true},
                    "agentInfo": {"name": "mock-acp-agent", "version": "0.0.1"},
                    "authMethods": []
                }),
            ),
            "session/new" => {
                session_counter += 1;
                // pid makes ids differ across processes, so tests can tell resume from new.
                let sid = format!("{SESSION_PREFIX}{pid}-{session_counter}");
                respond(&id, json!({"sessionId": sid}));
            }
            "session/load" => {
                let known = params
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s.starts_with(SESSION_PREFIX));
                if known {
                    respond(&id, Value::Null);
                } else {
                    respond_error(&id, -32602, "unknown session");
                }
            }
            "session/prompt" => {
                prompts_seen += 1;
                let sid = params
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let env_secs = |name: &str, default: u64| {
                    std::env::var(name)
                        .ok()
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(default)
                };
                if mode == Mode::Stall {
                    thought_chunk(&sid, "thinking, then going quiet\n");
                    std::thread::sleep(std::time::Duration::from_secs(env_secs(
                        "MOCK_ACP_STALL_SECS",
                        20,
                    )));
                }
                if mode == Mode::Drip {
                    for i in 0..env_secs("MOCK_ACP_DRIP_SECS", 6) {
                        message_chunk(&sid, &format!("{i} "));
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                }
                if mode == Mode::Think {
                    thought_chunk(&sid, "Let me look at the spec.\n");
                    thought_chunk(&sid, "Then plan the change.\n");
                    update(
                        &sid,
                        json!({
                            "sessionUpdate": "tool_call",
                            "toolCallId": "t1",
                            "title": "Read SPEC.md",
                            "status": "in_progress"
                        }),
                    );
                    update(
                        &sid,
                        json!({
                            "sessionUpdate": "tool_call_update",
                            "toolCallId": "t1",
                            "status": "completed"
                        }),
                    );
                }
                let permission_suffix = if mode == Mode::Permission {
                    let granted = request_permission(&sid, &mut lines);
                    if granted { " (access granted)" } else { "" }
                } else {
                    ""
                };
                message_chunk(
                    &sid,
                    &format!("mock: {}{permission_suffix}", prompt_text(&params)),
                );
                respond(&id, json!({"stopReason": "end_turn"}));
                if mode == Mode::Crash && prompts_seen == 1 {
                    std::process::exit(1);
                }
            }
            "session/cancel" => respond(&id, Value::Null),
            _ => respond_error(&id, -32601, "method not found"),
        }
    }
}
