use crate::acp::client::{
    self, AGENT_SPECS, AcpError, AgentSpec, HandshakeInfo, SessionId, UsageInfo,
};
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

/// Where session ids get persisted across Locust restarts, so an agent's conversation can
/// be resumed instead of starting fresh every launch. One flat JSON map from
/// "{spec_id}::{key}" to the last session id seen for that identity.
const SESSION_STORE_PATH: &str = ".state/acp_sessions.json";

fn session_store_key(spec_id: &str, key: &str) -> String {
    format!("{spec_id}::{key}")
}

fn load_session_store() -> HashMap<String, String> {
    std::fs::read_to_string(SESSION_STORE_PATH)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_session_id(spec_id: &str, key: &str, session_id: &str) {
    let mut map = load_session_store();
    map.insert(session_store_key(spec_id, key), session_id.to_string());
    if let Some(parent) = std::path::Path::new(SESSION_STORE_PATH).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(&map) {
        let _ = std::fs::write(SESSION_STORE_PATH, json);
    }
}

/// Open the session identified by `key` on an agent's connection: resume whatever session
/// id was saved for it from a previous Locust run if the agent still recognizes it, else
/// start a fresh one (and persist its id for next time).
fn open_or_resume_session(
    conn: &mut client::AcpConnection,
    spec_id: &'static str,
    key: &str,
    cwd: &str,
) -> Result<SessionId, String> {
    if let Some(saved_id) = load_session_store().get(&session_store_key(spec_id, key))
        && client::load_session(conn, SessionId::from(saved_id.clone()), cwd).is_ok()
    {
        return Ok(SessionId::from(saved_id.clone()));
    }
    // Saved id is stale (agent restarted its own storage, deleted the session, etc.) --
    // fall through and start a fresh one below.

    let sid = client::new_session(conn, cwd).map_err(|e| e.to_string())?;
    save_session_id(spec_id, key, &sid.to_string());
    Ok(sid)
}

/// Fixed identity for the persistent Controller chat: the same "acp-claude" agent the ACP
/// Servers tab can also connect, and every Swarm pane assigned `agent: claude` runs against.
/// All three go through the exact same pooled connection -- see `Manager`.
pub const CONTROLLER_SPEC_ID: &str = "acp-claude";

/// Session key the Controller chat uses on its agent's connection. Swarm panes use their own
/// pane name as the key instead, so each pane gets its own session even when several panes
/// share one underlying agent connection (e.g. two `agent: claude` panes never see each
/// other's conversation).
pub const CONTROLLER_SESSION_KEY: &str = "__controller__";

/// One update a background actor reports back about its connection.
pub enum AgentOutcome {
    /// The initial handshake succeeded; the connection is ready for sessions/prompts.
    Connected(HandshakeInfo),
    /// A one-shot usage probe turn finished, with real token/cost telemetry.
    UsageProbed(UsageInfo),
    /// A probe turn failed (commonly: the agent needs interactive auth Locust can't
    /// provide) -- the connection itself is still fine, just without fresh usage numbers.
    ProbeFailed(String),
    /// The connection itself failed.
    Failed(AcpError),
}

pub struct AgentUpdate {
    pub server_id: &'static str,
    pub outcome: AgentOutcome,
}

/// One piece of work for an agent's actor thread to run against its single live connection.
enum Job {
    /// Open the session identified by `key` if it doesn't exist yet; otherwise a no-op.
    /// Used to warm every Swarm pane up with a ready session at Locust startup, without
    /// running its task -- so no agent needs the user to explicitly start it, and none of
    /// them show as anything but idle until actually run.
    EnsureSession {
        key: String,
        reply_to: Sender<Result<(), String>>,
    },
    /// Run one more turn on the session identified by `key`, opening it first if this is
    /// the first turn on it. Every Swarm pane keys by its own pane name, so repeat runs of
    /// the same pane continue that pane's own conversation; the Controller chat keys by
    /// `CONTROLLER_SESSION_KEY`.
    Turn {
        key: String,
        prompt: String,
        reply_to: Sender<Result<(String, Option<UsageInfo>), String>>,
        /// Live events while the turn runs (thinking, text, tools). Dropped when the turn
        /// ends, which is how the receiver knows no more events will come.
        progress_tx: Option<Sender<client::TurnEvent>>,
        limits: client::TurnLimits,
    },
    /// Run one real session/new + session/prompt probe turn to refresh token/cost
    /// telemetry, same as the ACP Servers tab has always done on explicit connect.
    Probe {
        reply_to: Sender<Result<UsageInfo, String>>,
    },
}

/// Incoming permission request from an agent to be shown in the UI modal.
pub struct PermissionPrompt {
    pub id: i64,
    pub agent_pane: String,
    pub tool_title: String,
    pub detail: String,
    pub expires_at: std::time::Instant,
    pub reply_to: Sender<client::PermissionDecision>,
}

/// One real, persistent ACP connection to a single agent spec, serialized through its own
/// background thread so every caller -- the ACP Servers tab, the Controller chat, and every
/// Swarm pane run for this agent -- shares the exact same spawned subprocess instead of each
/// starting its own.
struct AgentActor {
    job_tx: Sender<Job>,
}

impl AgentActor {
    fn start(
        spec: AgentSpec,
        status_tx: Sender<AgentUpdate>,
        permission_tx: Sender<PermissionPrompt>,
    ) -> Self {
        let (job_tx, job_rx) = mpsc::channel::<Job>();

        std::thread::spawn(move || {
            let (mut conn, info) = match client::connect(&spec) {
                Ok(pair) => pair,
                Err(e) => {
                    let _ = status_tx.send(AgentUpdate {
                        server_id: spec.id,
                        outcome: AgentOutcome::Failed(e),
                    });
                    return;
                }
            };

            let p_tx = permission_tx.clone();
            let agent_id = spec.id.to_string();
            conn.permission_handler = Some(std::sync::Arc::new(move |req| {
                let (reply_tx, reply_rx) = mpsc::channel();
                let timeout = std::time::Duration::from_secs(30);
                let expires_at = std::time::Instant::now() + timeout;
                let prompt = PermissionPrompt {
                    id: req.id,
                    agent_pane: agent_id.clone(),
                    tool_title: req.tool_title,
                    detail: req.detail,
                    expires_at,
                    reply_to: reply_tx,
                };
                if p_tx.send(prompt).is_err() {
                    return client::PermissionDecision::Deny;
                }
                reply_rx
                    .recv_timeout(timeout)
                    .unwrap_or(client::PermissionDecision::Deny)
            }));

            let _ = status_tx.send(AgentUpdate {
                server_id: spec.id,
                outcome: AgentOutcome::Connected(info),
            });

            let cwd = std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| ".".to_string());

            // Always probe usage immediately upon connecting so live telemetry is populated
            match client::probe_usage(&mut conn, &cwd) {
                Ok(usage) => {
                    let _ = status_tx.send(AgentUpdate {
                        server_id: spec.id,
                        outcome: AgentOutcome::UsageProbed(usage),
                    });
                }
                Err(err) => {
                    let _ = status_tx.send(AgentUpdate {
                        server_id: spec.id,
                        outcome: AgentOutcome::ProbeFailed(err.to_string()),
                    });
                }
            }

            // One session per key (pane name, or CONTROLLER_SESSION_KEY) -- this is what
            // keeps two panes sharing one agent connection from seeing each other's
            // conversation, and what lets repeat runs of the same pane stay one ongoing
            // conversation instead of starting over each time.
            let mut sessions: HashMap<String, SessionId> = HashMap::new();

            while let Ok(job) = job_rx.recv() {
                match job {
                    Job::EnsureSession { key, reply_to } => {
                        if sessions.contains_key(&key) {
                            let _ = reply_to.send(Ok(()));
                            continue;
                        }
                        match open_or_resume_session(&mut conn, spec.id, &key, &cwd) {
                            Ok(sid) => {
                                sessions.insert(key, sid);
                                let _ = reply_to.send(Ok(()));
                            }
                            Err(e) => {
                                let _ = reply_to.send(Err(e));
                            }
                        }
                    }
                    Job::Turn {
                        key,
                        prompt,
                        reply_to,
                        progress_tx,
                        limits,
                    } => {
                        if !sessions.contains_key(&key) {
                            match open_or_resume_session(&mut conn, spec.id, &key, &cwd) {
                                Ok(sid) => {
                                    sessions.insert(key.clone(), sid);
                                }
                                Err(e) => {
                                    let _ = reply_to.send(Err(e));
                                    continue;
                                }
                            }
                        }
                        let sid = sessions.get(&key).expect("just ensured above").clone();
                        let result =
                            client::send_prompt_streaming(&mut conn, sid, &prompt, limits, |ev| {
                                if let Some(tx) = &progress_tx {
                                    let _ = tx.send(ev);
                                }
                            })
                            .map_err(|e| e.to_string());
                        drop(progress_tx);
                        let _ = reply_to.send(result);
                    }
                    Job::Probe { reply_to } => {
                        let result =
                            client::probe_usage(&mut conn, &cwd).map_err(|e| e.to_string());
                        let _ = reply_to.send(result);
                    }
                }
            }
            // Every job sender (Manager handles, cloned per caller) has been dropped, so
            // nothing can ask this agent for anything else -- shut its real subprocess down.
            conn.shutdown();
        });

        Self { job_tx }
    }
}

/// Pool of live ACP connections, one actor per agent spec id, started lazily the first time
/// anything asks for that agent and kept alive for the rest of the run. This is the single
/// place every real agent launch goes through -- the ACP Servers tab, the Controller chat,
/// and every Swarm pane run -- so there is never more than one spawned subprocess per agent.
pub struct Manager {
    status_tx: Sender<AgentUpdate>,
    status_rx: Receiver<AgentUpdate>,
    permission_tx: Sender<PermissionPrompt>,
    permission_rx: Receiver<PermissionPrompt>,
    actors: HashMap<&'static str, AgentActor>,
    pending_probes: Vec<(&'static str, Receiver<Result<UsageInfo, String>>)>,
    /// Idle and total time limits applied to every turn this manager starts.
    turn_limits: client::TurnLimits,
}

impl Default for Manager {
    fn default() -> Self {
        let (status_tx, status_rx) = mpsc::channel();
        let (permission_tx, permission_rx) = mpsc::channel();
        Self {
            status_tx,
            status_rx,
            permission_tx,
            permission_rx,
            actors: HashMap::new(),
            pending_probes: Vec::new(),
            turn_limits: client::TurnLimits::default(),
        }
    }
}

impl Manager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the idle and total time limits for turns started after this call.
    pub fn set_turn_limits(&mut self, limits: client::TurnLimits) {
        self.turn_limits = limits;
    }

    /// Drain any pending agent permission requests to be shown in modal dialogs.
    pub fn poll_permissions(&self) -> Vec<PermissionPrompt> {
        let mut prompts = Vec::new();
        while let Ok(prompt) = self.permission_rx.try_recv() {
            prompts.push(prompt);
        }
        prompts
    }

    /// Ensure exactly one live connection exists for `spec`, spawning it only if nothing has
    /// asked for this agent yet. A no-op if it's already connected or connecting -- this is
    /// a plain handshake, with no usage probe and so no real LLM call, so it's safe to run
    /// for every registered agent up front (e.g. at Locust startup).
    pub fn ensure_connected(&mut self, spec: AgentSpec) {
        if !self.actors.contains_key(spec.id) {
            let status_tx = self.status_tx.clone();
            let permission_tx = self.permission_tx.clone();
            self.actors
                .insert(spec.id, AgentActor::start(spec, status_tx, permission_tx));
        }
    }

    /// Connect and always probe real token/cost telemetry via a probe turn.
    /// Non-blocking either way.
    pub fn connect(&mut self, spec: AgentSpec) {
        let is_new = !self.actors.contains_key(spec.id);
        self.ensure_connected(spec);
        if !is_new && let Some(actor) = self.actors.get(spec.id) {
            let (tx, rx) = mpsc::channel();
            let _ = actor.job_tx.send(Job::Probe { reply_to: tx });
            self.pending_probes.push((spec.id, rx));
        }
    }

    /// Always probe all registered agents.
    pub fn probe_all(&mut self) {
        for spec in AGENT_SPECS {
            self.connect(*spec);
        }
    }

    /// Ensure a connection exists for `spec` and open the session identified by `key` if it
    /// doesn't exist yet, without running anything on it. Used to warm Swarm panes up at
    /// startup so none of them need the user to explicitly start them -- they're simply
    /// ready, still idle, the first time the user does run one.
    pub fn ensure_session(&mut self, spec: AgentSpec, key: String) -> Receiver<Result<(), String>> {
        self.ensure_connected(spec);
        let (tx, rx) = mpsc::channel();
        if let Some(actor) = self.actors.get(spec.id) {
            let _ = actor.job_tx.send(Job::EnsureSession { key, reply_to: tx });
        }
        rx
    }

    /// Ensure a connection exists for `spec` and send `prompt` as the next turn on the
    /// session identified by `key` (opening it first if needed). Backs both Swarm pane runs
    /// (keyed by pane name) and the Controller chat (keyed by `CONTROLLER_SESSION_KEY`).
    pub fn turn(
        &mut self,
        spec: AgentSpec,
        key: String,
        prompt: String,
    ) -> Receiver<Result<(String, Option<UsageInfo>), String>> {
        self.turn_with_progress(spec, key, prompt, None)
    }

    /// Same as `turn`, and also returns a receiver of live `TurnEvent`s (thinking, text,
    /// tool calls) while the turn runs. The event receiver disconnects when the turn ends.
    pub fn turn_streaming(
        &mut self,
        spec: AgentSpec,
        key: String,
        prompt: String,
    ) -> (
        Receiver<Result<(String, Option<UsageInfo>), String>>,
        Receiver<client::TurnEvent>,
    ) {
        let (ptx, prx) = mpsc::channel();
        let rx = self.turn_with_progress(spec, key, prompt, Some(ptx));
        (rx, prx)
    }

    fn turn_with_progress(
        &mut self,
        spec: AgentSpec,
        key: String,
        prompt: String,
        progress_tx: Option<Sender<client::TurnEvent>>,
    ) -> Receiver<Result<(String, Option<UsageInfo>), String>> {
        self.ensure_connected(spec);
        let (tx, rx) = mpsc::channel();
        if let Some(actor) = self.actors.get(spec.id) {
            let _ = actor.job_tx.send(Job::Turn {
                key,
                prompt,
                reply_to: tx,
                progress_tx,
                limits: self.turn_limits,
            });
        }
        rx
    }

    /// Drain every status/probe update that has arrived since the last poll.
    pub fn poll(&mut self) -> Vec<AgentUpdate> {
        let mut out: Vec<AgentUpdate> = self.status_rx.try_iter().collect();

        self.pending_probes
            .retain_mut(|(id, rx)| match rx.try_recv() {
                Ok(result) => {
                    let outcome = match result {
                        Ok(usage) => AgentOutcome::UsageProbed(usage),
                        Err(e) => AgentOutcome::ProbeFailed(e),
                    };
                    out.push(AgentUpdate {
                        server_id: id,
                        outcome,
                    });
                    false
                }
                Err(TryRecvError::Empty) => true,
                Err(TryRecvError::Disconnected) => false,
            });

        out
    }

    pub fn disconnect(&mut self, server_id: &str) {
        // Dropping the actor's job_tx handle ends its thread's `job_rx.recv()` loop, which
        // then shuts the real subprocess down itself.
        self.actors.remove(server_id);
    }

    pub fn is_connected(&self, server_id: &str) -> bool {
        self.actors.contains_key(server_id)
    }
}

/// Map an agents.yaml `agent:` id to the real ACP spec that launches it. `None` for
/// unrecognized agents rather than guessing a spec that would just fail to connect.
pub fn spec_for_agent_id(agent: &str) -> Option<&'static AgentSpec> {
    let spec_id = match agent {
        "claude" | "acp-claude" => "acp-claude",
        "agy" | "antigravity" | "acp-antigravity" => "acp-antigravity",
        "opencode" | "acp-opencode" => "acp-opencode",
        "codex" | "acp-codex" => "acp-codex",
        id if AGENT_SPECS.iter().any(|s| s.id == id) => id,
        _ => return None,
    };
    AGENT_SPECS.iter().find(|s| s.id == spec_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spec_for_agent_id_known_agents() {
        assert_eq!(spec_for_agent_id("claude").unwrap().id, "acp-claude");
        assert_eq!(spec_for_agent_id("acp-claude").unwrap().id, "acp-claude");
        assert_eq!(spec_for_agent_id("agy").unwrap().id, "acp-antigravity");
        assert_eq!(
            spec_for_agent_id("antigravity").unwrap().id,
            "acp-antigravity"
        );
        assert_eq!(
            spec_for_agent_id("acp-antigravity").unwrap().id,
            "acp-antigravity"
        );
        assert_eq!(
            spec_for_agent_id("opencode").unwrap().id,
            "acp-opencode"
        );
        assert_eq!(
            spec_for_agent_id("acp-opencode").unwrap().id,
            "acp-opencode"
        );
        assert_eq!(
            spec_for_agent_id("codex").unwrap().id,
            "acp-codex"
        );
        assert_eq!(
            spec_for_agent_id("acp-codex").unwrap().id,
            "acp-codex"
        );
    }

    #[test]
    fn test_spec_for_agent_id_rejects_unknown_agent() {
        assert!(spec_for_agent_id("not-a-real-cli").is_none());
    }

    #[test]
    fn test_manager_starts_with_no_connections() {
        let manager = Manager::new();
        assert!(!manager.is_connected("acp-claude"));
    }
}
