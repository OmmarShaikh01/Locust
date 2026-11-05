use agent_client_protocol::schema::ProtocolVersion;
/// Re-exported so callers keying a session cache (e.g. `Manager`'s per-pane sessions) can
/// name the type without reaching into the `agent_client_protocol` crate directly.
pub use agent_client_protocol::v1::schema::SessionId;
use agent_client_protocol::v1::schema::{
    AGENT_METHOD_NAMES, AuthMethod, CLIENT_METHOD_NAMES, ClientCapabilities, ContentBlock,
    Implementation, InitializeRequest, InitializeResponse, LoadSessionRequest, NewSessionRequest,
    NewSessionResponse, PromptRequest, PromptResponse, SessionNotification, SessionUpdate,
    TextContent, ToolCallStatus,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// How long to wait for an agent to answer the initial handshake before giving up.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
/// How long to wait for a real model turn (session/new + session/prompt) to finish.
/// Longer than the handshake timeout since this is an actual LLM call, not a protocol ping.
const PROMPT_TIMEOUT: Duration = Duration::from_secs(45);
/// Deliberately tiny so the one real LLM call Locust makes per connect costs as little as
/// possible while still exercising the real session/prompt -> usage_update path.
const PROBE_PROMPT: &str = "Reply with the single word OK and nothing else.";

/// Static description of how to launch one ACP-compatible coding agent as a stdio subprocess.
#[derive(Debug, Clone, Copy)]
pub struct AgentSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
}

impl AgentSpec {
    pub fn command_line(&self) -> String {
        if self.args.is_empty() {
            self.command.to_string()
        } else {
            format!("{} {}", self.command, self.args.join(" "))
        }
    }
}

/// Real ACP agents reachable from Locust, launched over stdio per the Agent Client Protocol.
pub const AGENT_SPECS: &[AgentSpec] = &[
    AgentSpec {
        id: "acp-claude",
        name: "Claude Code (ACP)",
        command: "npx",
        args: &["-y", "@agentclientprotocol/claude-agent-acp"],
    },
    AgentSpec {
        id: "acp-opencode",
        name: "opencode (ACP)",
        command: "opencode",
        args: &["acp"],
    },
    AgentSpec {
        id: "acp-antigravity",
        name: "Antigravity (agy ACP)",
        command: "agy",
        args: &["acp"],
    },
    AgentSpec {
        id: "acp-codex",
        name: "Codex (ACP)",
        command: "npx",
        args: &["-y", "@agentclientprotocol/codex-acp"],
    },
];

/// Result of a successful `initialize` handshake with an agent.
#[derive(Debug, Clone)]
pub struct HandshakeInfo {
    pub protocol_version: i64,
    pub agent_name: Option<String>,
    pub agent_version: Option<String>,
    pub capabilities: Vec<String>,
    pub auth_methods: Vec<String>,
    pub ping_ms: u64,
}

/// Real call/token telemetry pulled out of one session/new + session/prompt turn.
#[derive(Debug, Clone, Default)]
pub struct UsageInfo {
    pub stop_reason: Option<String>,
    pub tokens_used: Option<u64>,
    pub context_size: Option<u64>,
    pub cost_amount: Option<f64>,
    pub cost_currency: Option<String>,
}

impl UsageInfo {
    /// Calculate remaining capacity in context window.
    pub fn tokens_left(&self) -> Option<u64> {
        match (self.tokens_used, self.context_size) {
            (Some(used), Some(size)) => Some(size.saturating_sub(used)),
            _ => None,
        }
    }

    /// Calculate percentage of context capacity remaining.
    pub fn percent_left(&self) -> Option<f64> {
        match (self.tokens_used, self.context_size) {
            (Some(used), Some(size)) if size > 0 => {
                let left = size.saturating_sub(used);
                Some((left as f64 / size as f64) * 100.0)
            }
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum AcpError {
    Spawn(String),
    Io(String),
    /// A short fixed wait (handshake, opening a session) ran out.
    Timeout,
    /// A turn produced no output at all for this many seconds. The agent is stuck or silent.
    Idle(u64),
    /// A turn kept going but passed the hard cap of this many seconds in total.
    TotalLimit(u64),
    /// The agent process exited or closed its output.
    Disconnected,
    Protocol(String),
}

impl AcpError {
    /// True for the errors that mean a running turn was given up on.
    pub fn is_turn_timeout(&self) -> bool {
        matches!(self, AcpError::Idle(_) | AcpError::TotalLimit(_))
    }
}

/// Time limits for one model turn. `idle` restarts every time the agent sends anything, so a
/// turn that keeps streaming is never cut short by it. `total` is the hard cap on the whole
/// turn. Time spent waiting for the operator to answer a permission prompt counts toward
/// neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnLimits {
    pub idle: Duration,
    pub total: Duration,
}

impl TurnLimits {
    pub fn from_secs(idle: u64, total: u64) -> Self {
        Self {
            idle: Duration::from_secs(idle.max(1)),
            total: Duration::from_secs(total.max(1)),
        }
    }
}

impl Default for TurnLimits {
    fn default() -> Self {
        Self::from_secs(180, 1800)
    }
}

impl std::fmt::Display for AcpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AcpError::Spawn(msg) => write!(f, "failed to launch agent: {}", msg),
            AcpError::Io(msg) => write!(f, "stdio error: {}", msg),
            AcpError::Timeout => write!(f, "timed out waiting for a response"),
            AcpError::Idle(s) => write!(
                f,
                "no output from the agent for {s}s (idle limit). The turn was cancelled; press r to retry"
            ),
            AcpError::TotalLimit(s) => write!(
                f,
                "turn ran longer than {s}s (total limit). The turn was cancelled; press r to retry"
            ),
            AcpError::Disconnected => write!(f, "the agent process exited"),
            AcpError::Protocol(msg) => write!(f, "protocol error: {}", msg),
        }
    }
}

/// Decision made on an agent's permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny,
}

/// Details of an agent permission request.
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub id: i64,
    pub session_id: String,
    pub tool_title: String,
    /// What the tool is about to do (its raw input: command, path, ...), cut to 400 chars.
    /// Empty if the agent did not send any.
    pub detail: String,
}

pub type PermissionCallback =
    std::sync::Arc<dyn Fn(PermissionRequest) -> PermissionDecision + Send + Sync>;

/// A live ACP agent subprocess with a persistent reader thread forwarding every parsed
/// JSON-RPC message (responses and notifications alike) so multiple calls can be made
/// over the connection's lifetime, not just the initial handshake.
///
/// The wire format (request/response/notification payloads) comes from the official
/// `agent-client-protocol` crate's generated schema; only the stdio transport (spawning the
/// child, framing NDJSON lines, matching response ids) is Locust's own, since that crate's
/// transport/connection builder expects to own the whole async runtime rather than slot into
/// this app's plain worker-thread model.
pub struct AcpConnection {
    pub child: Child,
    stdin: ChildStdin,
    msg_rx: Receiver<Value>,
    next_id: i64,
    pub permission_handler: Option<PermissionCallback>,
}

impl AcpConnection {
    pub fn shutdown(mut self) {
        let _ = self.stdin.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn send(&mut self, method: &str, params: Value) -> Result<i64, AcpError> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let mut line = request.to_string();
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .map_err(|e| AcpError::Io(e.to_string()))?;
        self.stdin
            .flush()
            .map_err(|e| AcpError::Io(e.to_string()))?;
        Ok(id)
    }

    /// Wait for the response matching `id`, forwarding anything else to `on_other`.
    /// Any inbound agent->client request seen along the way is auto-rejected so the agent
    /// never blocks forever waiting on a capability (fs access, permission, etc.) Locust
    /// does not implement.
    fn recv_response(
        &mut self,
        id: i64,
        timeout: Duration,
        on_other: impl FnMut(&Value),
    ) -> Result<Value, AcpError> {
        let limits = TurnLimits {
            idle: timeout,
            total: timeout,
        };
        self.recv_response_limits(id, limits, on_other)
            .map_err(|e| {
                if e.is_turn_timeout() {
                    AcpError::Timeout
                } else {
                    e
                }
            })
    }

    /// Like `recv_response`, with separate idle and total limits (see `TurnLimits`).
    fn recv_response_limits(
        &mut self,
        id: i64,
        limits: TurnLimits,
        mut on_other: impl FnMut(&Value),
    ) -> Result<Value, AcpError> {
        let mut deadline = Instant::now() + limits.total;
        let mut last_activity = Instant::now();
        loop {
            let now = Instant::now();
            let total_left = deadline
                .checked_duration_since(now)
                .ok_or(AcpError::TotalLimit(limits.total.as_secs()))?;
            let idle_left = (last_activity + limits.idle)
                .checked_duration_since(now)
                .ok_or(AcpError::Idle(limits.idle.as_secs()))?;
            let v = match self.msg_rx.recv_timeout(total_left.min(idle_left)) {
                Ok(v) => v,
                // Loop again: the checks above name which limit ran out.
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Err(AcpError::Disconnected),
            };
            last_activity = Instant::now();

            let msg_id = v.get("id").and_then(Value::as_i64);
            let has_method = v.get("method").is_some();

            if msg_id == Some(id) && !has_method {
                return Ok(v);
            }
            if has_method && v.get("id").is_some() {
                if v.get("method").and_then(Value::as_str) == Some("session/request_permission")
                    && let Some(handler) = &self.permission_handler
                {
                    let req_id = v.get("id").and_then(Value::as_i64).unwrap_or(0);
                    let session_id = v["params"]["sessionId"].as_str().unwrap_or("").to_string();
                    let tool_title = v["params"]["toolCall"]["title"]
                        .as_str()
                        .or_else(|| v["params"]["toolCall"]["toolCallId"].as_str())
                        .unwrap_or("permission request")
                        .to_string();
                    let detail = match &v["params"]["toolCall"]["rawInput"] {
                        Value::Null => String::new(),
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    let detail: String = detail.chars().take(400).collect();
                    let asked_at = Instant::now();
                    let decision = handler(PermissionRequest {
                        id: req_id,
                        session_id,
                        tool_title,
                        detail,
                    });
                    // The operator's thinking time is not the agent's: give it back.
                    deadline += asked_at.elapsed();
                    last_activity = Instant::now();
                    let option_id = match decision {
                        PermissionDecision::Allow => "allow",
                        PermissionDecision::Deny => "reject",
                    };
                    let response = json!({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "result": {
                            "outcome": {
                                "outcome": "selected",
                                "optionId": option_id
                            }
                        }
                    });
                    let mut line = response.to_string();
                    line.push('\n');
                    let _ = self.stdin.write_all(line.as_bytes());
                    let _ = self.stdin.flush();
                } else {
                    self.reject_incoming(&v);
                }
            }
            on_other(&v);
        }
    }

    /// Send a JSON-RPC notification (no id, no reply expected).
    fn notify(&mut self, method: &str, params: Value) {
        let msg = json!({"jsonrpc": "2.0", "method": method, "params": params});
        let mut line = msg.to_string();
        line.push('\n');
        let _ = self.stdin.write_all(line.as_bytes());
        let _ = self.stdin.flush();
    }

    /// Give up on a running prompt: tell the agent to stop, then wait briefly for it to
    /// acknowledge so the session is clean for the next turn. Best effort, never fails.
    fn cancel_turn(&mut self, session: &Value, prompt_id: i64) {
        self.notify("session/cancel", json!({ "sessionId": session }));
        let grace = TurnLimits::from_secs(3, 3);
        let _ = self.recv_response_limits(prompt_id, grace, |_| {});
    }

    /// Politely decline a request the agent sent us (e.g. a permission or fs prompt) so it
    /// doesn't sit blocked waiting for a client capability Locust's probe doesn't implement.
    fn reject_incoming(&mut self, req: &Value) {
        let Some(rid) = req.get("id") else { return };
        let response = json!({
            "jsonrpc": "2.0",
            "id": rid,
            "error": {"code": -32601, "message": "locust: client capability not implemented"}
        });
        let mut line = response.to_string();
        line.push('\n');
        let _ = self.stdin.write_all(line.as_bytes());
        let _ = self.stdin.flush();
    }
}

/// Spawn an agent per `spec` and perform the ACP `initialize` handshake over NDJSON stdio.
/// The returned connection stays open with its reader thread live, ready for further calls.
pub fn connect(spec: &AgentSpec) -> Result<(AcpConnection, HandshakeInfo), AcpError> {
    let start = Instant::now();
    let mut child = spawn(spec).map_err(|e| AcpError::Spawn(format!("{}: {}", spec.command, e)))?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| AcpError::Io("child has no stdin".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AcpError::Io("child has no stdout".into()))?;

    // Persistent reader: lives for the whole connection, forwarding every NDJSON line.
    let (tx, msg_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut buf = String::new();
            match reader.read_line(&mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = buf.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Ok(v) = serde_json::from_str::<Value>(trimmed)
                        && tx.send(v).is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut conn = AcpConnection {
        child,
        stdin,
        msg_rx,
        next_id: 0,
        permission_handler: None,
    };

    let init_request = InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(ClientCapabilities::default())
        .client_info(Implementation::new("locust", env!("CARGO_PKG_VERSION")));
    let params = to_value(&init_request)?;
    let id = conn.send(AGENT_METHOD_NAMES.initialize, params)?;

    let response = match conn.recv_response(id, HANDSHAKE_TIMEOUT, |_| {}) {
        Ok(r) => r,
        Err(e) => {
            let _ = conn.child.kill();
            return Err(e);
        }
    };

    if let Some(err) = response.get("error") {
        return Err(AcpError::Protocol(err.to_string()));
    }
    let result = response
        .get("result")
        .ok_or_else(|| AcpError::Protocol("response had no result".into()))?;
    let init: InitializeResponse = serde_json::from_value(result.clone())
        .map_err(|e| AcpError::Protocol(format!("initialize response: {e}")))?;

    let agent_name = init.agent_info.as_ref().map(|i| i.name.clone());
    let agent_version = init.agent_info.as_ref().map(|i| i.version.clone());
    let capabilities = truthy_keys(&init.agent_capabilities);
    let auth_methods = init
        .auth_methods
        .iter()
        .filter_map(auth_method_id)
        .collect();
    let ping_ms = start.elapsed().as_millis() as u64;

    Ok((
        conn,
        HandshakeInfo {
            protocol_version: init.protocol_version.as_u16() as i64,
            agent_name,
            agent_version,
            capabilities,
            auth_methods,
            ping_ms,
        },
    ))
}

/// Open a new session against an already-connected agent, returning its id for reuse across
/// many later prompt turns -- the persistent Controller chat keeps one for its whole
/// lifetime instead of opening a fresh one per message.
pub fn new_session(conn: &mut AcpConnection, cwd: &str) -> Result<SessionId, AcpError> {
    open_session(conn, cwd, PROMPT_TIMEOUT, |_| {})
}

fn open_session(
    conn: &mut AcpConnection,
    cwd: &str,
    timeout: Duration,
    on_other: impl FnMut(&Value),
) -> Result<SessionId, AcpError> {
    let new_session_req = NewSessionRequest::new(cwd);
    let params = to_value(&new_session_req)?;
    let new_id = conn.send(AGENT_METHOD_NAMES.session_new, params)?;
    let new_resp = conn.recv_response(new_id, timeout, on_other)?;
    if let Some(err) = new_resp.get("error") {
        return Err(AcpError::Protocol(err.to_string()));
    }
    let new_result = new_resp
        .get("result")
        .ok_or_else(|| AcpError::Protocol("session/new had no result".into()))?;
    let new_session: NewSessionResponse = serde_json::from_value(new_result.clone())
        .map_err(|e| AcpError::Protocol(format!("session/new response: {e}")))?;
    Ok(new_session.session_id)
}

/// Resume a previously-created session by id, e.g. one saved from an earlier run of
/// Locust. Only available if the agent advertises the `loadSession` capability; fails if
/// the agent doesn't (or no longer) know this session id, in which case callers should
/// fall back to `new_session`.
pub fn load_session(
    conn: &mut AcpConnection,
    session_id: SessionId,
    cwd: &str,
) -> Result<(), AcpError> {
    let req = LoadSessionRequest::new(session_id, cwd);
    let params = to_value(&req)?;
    let id = conn.send(AGENT_METHOD_NAMES.session_load, params)?;
    let resp = conn.recv_response(id, PROMPT_TIMEOUT, |_| {})?;
    if let Some(err) = resp.get("error") {
        return Err(AcpError::Protocol(err.to_string()));
    }
    Ok(())
}

/// Send one user message as a session/prompt turn on an existing session, returning the
/// agent's streamed text reply and any reported usage telemetry. This makes one real LLM call
/// -- callers should only invoke it for an explicit user-submitted message, never speculatively.
pub fn send_prompt(
    conn: &mut AcpConnection,
    session_id: SessionId,
    text: &str,
) -> Result<(String, Option<UsageInfo>), AcpError> {
    send_prompt_streaming(conn, session_id, text, TurnLimits::default(), |_| {})
}

/// One live thing an agent did while a turn was still running. Sent to the UI as it happens,
/// so a pane can show "thinking", "writing", or "using a tool" instead of a frozen status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnEvent {
    /// A chunk of the model's reasoning text.
    Thought(String),
    /// A chunk of the visible reply text.
    Message(String),
    /// The agent started a tool call (the title is the agent's own label for it).
    ToolStarted(String),
    /// A tool call changed state; `done` is true when it finished or failed.
    ToolUpdate { title: Option<String>, done: bool },
}

/// Same as `send_prompt`, and also reports every `TurnEvent` to `on_event` as it streams in.
pub fn send_prompt_streaming(
    conn: &mut AcpConnection,
    session_id: SessionId,
    text: &str,
    limits: TurnLimits,
    mut on_event: impl FnMut(TurnEvent),
) -> Result<(String, Option<UsageInfo>), AcpError> {
    let mut reply = String::new();
    let mut usage_notifications: Vec<SessionNotification> = Vec::new();
    let mut capture = |v: &Value| {
        if let Some(notification) = parse_session_update(v) {
            match notification.update {
                SessionUpdate::AgentMessageChunk(chunk) => {
                    if let ContentBlock::Text(text_content) = chunk.content {
                        reply.push_str(&text_content.text);
                        on_event(TurnEvent::Message(text_content.text));
                    }
                }
                SessionUpdate::AgentThoughtChunk(chunk) => {
                    if let ContentBlock::Text(text_content) = chunk.content {
                        on_event(TurnEvent::Thought(text_content.text));
                    }
                }
                SessionUpdate::ToolCall(call) => {
                    on_event(TurnEvent::ToolStarted(call.title));
                }
                SessionUpdate::ToolCallUpdate(update) => {
                    let done = matches!(
                        update.fields.status,
                        Some(ToolCallStatus::Completed) | Some(ToolCallStatus::Failed)
                    );
                    on_event(TurnEvent::ToolUpdate {
                        title: update.fields.title,
                        done,
                    });
                }
                SessionUpdate::UsageUpdate(_) => {
                    usage_notifications.push(notification);
                }
                _ => {}
            }
        }
    };

    let session_value = to_value(&session_id)?;
    let prompt_req =
        PromptRequest::new(session_id, vec![ContentBlock::Text(TextContent::new(text))]);
    let params = to_value(&prompt_req)?;
    let prompt_id = conn.send(AGENT_METHOD_NAMES.session_prompt, params)?;
    let prompt_resp = match conn.recv_response_limits(prompt_id, limits, &mut capture) {
        Ok(resp) => resp,
        Err(e) => {
            if e.is_turn_timeout() {
                conn.cancel_turn(&session_value, prompt_id);
            }
            return Err(e);
        }
    };
    if let Some(err) = prompt_resp.get("error") {
        return Err(AcpError::Protocol(err.to_string()));
    }
    let prompt_result = prompt_resp
        .get("result")
        .ok_or_else(|| AcpError::Protocol("session/prompt had no result".into()))?;
    let prompt_response: PromptResponse = serde_json::from_value(prompt_result.clone())
        .map_err(|e| AcpError::Protocol(format!("session/prompt response: {e}")))?;

    let mut usage = UsageInfo {
        stop_reason: Some(format!("{:?}", prompt_response.stop_reason)),
        ..Default::default()
    };
    let last_update = usage_notifications.pop().map(|n| n.update);
    let has_usage = if let Some(SessionUpdate::UsageUpdate(update)) = last_update {
        usage.tokens_used = Some(update.used);
        usage.context_size = Some(update.size);
        if let Some(cost) = update.cost {
            usage.cost_amount = Some(cost.amount);
            usage.cost_currency = Some(cost.currency);
        }
        true
    } else {
        false
    };

    Ok((reply, if has_usage { Some(usage) } else { None }))
}

/// Run one real session/new + session/prompt turn against an already-connected agent and
/// extract whatever usage_update telemetry it reports. This makes one genuine LLM call and
/// therefore spends real tokens/credits against whatever account the agent is authenticated
/// with -- callers should only invoke this deliberately, never on a timer.
pub fn probe_usage(conn: &mut AcpConnection, cwd: &str) -> Result<UsageInfo, AcpError> {
    let mut usage_notifications: Vec<SessionNotification> = Vec::new();
    let mut capture = |v: &Value| {
        if let Some(notification) = parse_session_update(v)
            && matches!(notification.update, SessionUpdate::UsageUpdate(_))
        {
            usage_notifications.push(notification);
        }
    };

    let session_id = open_session(conn, cwd, PROMPT_TIMEOUT, &mut capture)?;

    let prompt_req = PromptRequest::new(
        session_id,
        vec![ContentBlock::Text(TextContent::new(PROBE_PROMPT))],
    );
    let params = to_value(&prompt_req)?;
    let prompt_id = conn.send(AGENT_METHOD_NAMES.session_prompt, params)?;
    let prompt_resp = conn.recv_response(prompt_id, PROMPT_TIMEOUT, &mut capture)?;
    if let Some(err) = prompt_resp.get("error") {
        return Err(AcpError::Protocol(err.to_string()));
    }
    let prompt_result = prompt_resp
        .get("result")
        .ok_or_else(|| AcpError::Protocol("session/prompt had no result".into()))?;
    let prompt_response: PromptResponse = serde_json::from_value(prompt_result.clone())
        .map_err(|e| AcpError::Protocol(format!("session/prompt response: {e}")))?;

    let mut usage = UsageInfo {
        stop_reason: Some(format!("{:?}", prompt_response.stop_reason)),
        ..Default::default()
    };
    let last_update = usage_notifications.pop().map(|n| n.update);
    if let Some(SessionUpdate::UsageUpdate(update)) = last_update {
        usage.tokens_used = Some(update.used);
        usage.context_size = Some(update.size);
        if let Some(cost) = update.cost {
            usage.cost_amount = Some(cost.amount);
            usage.cost_currency = Some(cost.currency);
        }
    }

    Ok(usage)
}

/// Parse a raw inbound line as a `session/update` notification, if that's what it is.
fn parse_session_update(v: &Value) -> Option<SessionNotification> {
    if v.get("method").and_then(Value::as_str) != Some(CLIENT_METHOD_NAMES.session_update) {
        return None;
    }
    serde_json::from_value(v.get("params")?.clone()).ok()
}

/// Serialize a typed request payload into the JSON-RPC `params` value.
fn to_value<T: Serialize>(request: &T) -> Result<Value, AcpError> {
    serde_json::to_value(request).map_err(|e| AcpError::Protocol(e.to_string()))
}

/// Collect the names of whichever top-level capability fields came back truthy/present, by
/// round-tripping the typed struct through serde rather than guessing field names by hand.
fn truthy_keys<T: Serialize>(value: &T) -> Vec<String> {
    let Ok(Value::Object(map)) = serde_json::to_value(value) else {
        return Vec::new();
    };
    map.iter()
        .filter(|(_, v)| v.as_bool().unwrap_or(!v.is_null()))
        .map(|(k, _)| k.clone())
        .collect()
}

fn auth_method_id(method: &AuthMethod) -> Option<String> {
    serde_json::to_value(method)
        .ok()?
        .get("id")
        .and_then(Value::as_str)
        .map(String::from)
}

#[cfg(target_os = "windows")]
fn spawn(spec: &AgentSpec) -> std::io::Result<Child> {
    // Windows cannot CreateProcess a .cmd/.bat shim (npx, opencode, ...) directly;
    // routing through cmd.exe resolves PATHEXT the same way an interactive shell would.
    //
    // NoDefaultCurrentDirectoryInExePath stops cmd.exe from looking in the current directory
    // first. The current directory is the project, which may hold a planted claude.cmd or
    // agy.exe that would otherwise run instead of the real tool.
    Command::new("cmd")
        .env("NoDefaultCurrentDirectoryInExePath", "1")
        .arg("/C")
        .arg(spec.command)
        .args(spec.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

#[cfg(not(target_os = "windows"))]
fn spawn(spec: &AgentSpec) -> std::io::Result<Child> {
    Command::new(spec.command)
        .args(spec.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_line_formatting() {
        let spec = AgentSpec {
            id: "x",
            name: "X",
            command: "agy",
            args: &["acp"],
        };
        assert_eq!(spec.command_line(), "agy acp");
    }

    #[test]
    fn test_usage_info_tokens_left_and_percentage() {
        let usage = UsageInfo {
            stop_reason: Some("end_turn".to_string()),
            tokens_used: Some(1500),
            context_size: Some(200_000),
            cost_amount: Some(0.0045),
            cost_currency: Some("USD".to_string()),
        };
        assert_eq!(usage.tokens_left(), Some(198_500));
        assert_eq!(usage.percent_left(), Some(99.25));

        let saturated = UsageInfo {
            stop_reason: None,
            tokens_used: Some(250_000),
            context_size: Some(200_000),
            cost_amount: None,
            cost_currency: None,
        };
        assert_eq!(saturated.tokens_left(), Some(0));
        assert_eq!(saturated.percent_left(), Some(0.0));
    }

    #[test]
    fn test_parse_session_update_matches_real_shape() {
        let v: Value = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"usage_update","used":100,"size":200}}}"#,
        )
        .unwrap();
        let notification = parse_session_update(&v).expect("usage_update should parse");
        assert!(matches!(notification.update, SessionUpdate::UsageUpdate(_)));
    }

    #[test]
    fn test_parse_session_update_rejects_other_methods() {
        let v: Value =
            serde_json::from_str(r#"{"jsonrpc":"2.0","method":"initialize","id":0,"result":{}}"#)
                .unwrap();
        assert!(parse_session_update(&v).is_none());
    }

    #[test]
    #[ignore = "requires the `agy` (Antigravity) CLI on PATH; run with `cargo test -- --ignored`"]
    fn test_connect_to_real_antigravity_agent() {
        let spec = AGENT_SPECS
            .iter()
            .find(|s| s.id == "acp-antigravity")
            .expect("antigravity spec present");
        let (conn, info) = connect(spec).expect("handshake with a real agy acp process");
        assert_eq!(info.protocol_version, 1);
        assert!(!info.agent_name.as_deref().unwrap_or_default().is_empty());
        conn.shutdown();
    }

    #[test]
    fn test_connect_to_missing_binary_fails_fast() {
        let spec = AgentSpec {
            id: "acp-does-not-exist",
            name: "Nonexistent Agent",
            command: "locust-this-binary-does-not-exist",
            args: &[],
        };
        let result = connect(&spec);
        assert!(result.is_err());
    }
}
