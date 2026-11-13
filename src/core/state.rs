use crate::acp::{AGENT_SPECS, AgentOutcome, AgentUpdate, CONTROLLER_SPEC_ID};
use crate::core::telemetry::SystemTelemetry;
use crate::swarm::{AgentAssignment, PaneGridPos, RunOutcome, SwarmConfig};
use std::time::Instant;

/// Maximum number of recent log lines kept per ACP server before older ones are dropped.
const MAX_RECENT_LOGS: usize = 12;
/// Maximum number of lines kept in the cross-agent activity tracer (Dashboard Frame 2).
const MAX_ACTIVITY_LOG: usize = 200;
/// Fixed top-level tabs: Dashboard, ACP Servers, Swarm.
const TOP_LEVEL_TABS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpStatus {
    Offline,
    Connecting,
    Online,
    Error,
}

impl AcpStatus {
    pub fn label(&self) -> &'static str {
        match self {
            AcpStatus::Offline => "OFFLINE",
            AcpStatus::Connecting => "CONNECTING",
            AcpStatus::Online => "ONLINE",
            AcpStatus::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AcpServer {
    pub id: &'static str,
    pub name: &'static str,
    pub transport: &'static str,
    pub command_line: String,
    pub status: AcpStatus,
    pub protocol: String,
    pub ping_ms: u64,
    pub capabilities: Vec<String>,
    pub connected_at: Option<Instant>,
    pub recent_logs: Vec<String>,
    /// Real token/cost telemetry from the one session/new + session/prompt probe turn run at
    /// connect time. `None` until a probe has completed (or if it failed, e.g. auth required).
    pub tokens_used: Option<u64>,
    pub context_size: Option<u64>,
    pub cost: Option<(f64, String)>,
    pub stop_reason: Option<String>,
}

impl AcpServer {
    fn from_spec(spec: &crate::acp::AgentSpec) -> Self {
        Self {
            id: spec.id,
            name: spec.name,
            transport: "stdio",
            command_line: spec.command_line(),
            status: AcpStatus::Offline,
            protocol: "ACP (not connected)".to_string(),
            ping_ms: 0,
            capabilities: Vec::new(),
            connected_at: None,
            recent_logs: vec![
                "Not connected. Press [Enter] to start the agent and handshake.".to_string(),
            ],
            tokens_used: None,
            context_size: None,
            cost: None,
            stop_reason: None,
        }
    }

    fn push_log(&mut self, line: String) {
        self.recent_logs.push(line);
        if self.recent_logs.len() > MAX_RECENT_LOGS {
            let overflow = self.recent_logs.len() - MAX_RECENT_LOGS;
            self.recent_logs.drain(0..overflow);
        }
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwarmRunStatus {
    Idle,
    Running,
    Done,
    Error,
}

impl SwarmRunStatus {
    pub fn label(&self) -> &'static str {
        match self {
            SwarmRunStatus::Idle => "IDLE",
            SwarmRunStatus::Running => "RUNNING",
            SwarmRunStatus::Done => "DONE",
            SwarmRunStatus::Error => "ERROR",
        }
    }
}

/// Which part of the Swarm tab currently has keyboard focus: the agent list, or the
/// Controller chat input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwarmFocus {
    List,
    Controller,
}

/// Who said one line in the Controller pane's chat history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerRole {
    User,
    Agent,
    System,
}

#[derive(Debug, Clone)]
pub struct ControllerMessage {
    pub role: ControllerRole,
    pub text: String,
}

/// Where one dispatched agent-to-agent task stands. Set from the real outcome of the turn it
/// triggered -- never from an agent's own claim -- so this reflects what actually happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Done,
    Failed,
}

impl TaskStatus {
    pub fn label(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "PENDING",
            TaskStatus::Done => "DONE",
            TaskStatus::Failed => "FAILED",
        }
    }
}

/// One real agent-to-agent handoff: `source` addressed `target` with `description` via a
/// `@[target]: ...` directive. Completion is decided by the real outcome of the turn that
/// directive triggered on `target`'s own session, not by anything either agent says.
#[derive(Debug, Clone)]
pub struct Task {
    pub id: u64,
    /// Idempotency id of the message that created this task (see `Model::message_id`).
    pub msg_id: u64,
    pub source: String,
    pub target: String,
    pub description: String,
    pub status: TaskStatus,
}

/// What a pane's running turn is doing right now, from the agent's own streamed events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TurnPhase {
    #[default]
    Idle,
    /// Prompt sent; no event from the agent yet.
    Waiting,
    /// The model is emitting reasoning text.
    Thinking,
    /// The model is emitting its visible reply.
    Writing,
    /// The agent is running a tool call.
    Tool,
}

impl TurnPhase {
    pub fn label(&self) -> &'static str {
        match self {
            TurnPhase::Idle => "idle",
            TurnPhase::Waiting => "waiting for first token",
            TurnPhase::Thinking => "thinking",
            TurnPhase::Writing => "writing reply",
            TurnPhase::Tool => "running tool",
        }
    }
}

/// A message identical to one sent inside this many seconds is not sent again.
pub const MESSAGE_WINDOW_SECS: u64 = 600;

/// Most thinking text kept per pane. Older text is dropped from the front.
const MAX_THOUGHT_CHARS: usize = 32_000;

/// Live view of one pane's turn, fed by `TurnEvent`s while the model works.
#[derive(Debug, Clone, Default)]
pub struct PaneLive {
    pub phase: TurnPhase,
    /// The model's reasoning text for the current (or last) turn.
    pub thought: String,
    /// The visible reply streamed so far.
    pub partial: String,
    /// Title of the tool call in progress, if any.
    pub tool: Option<String>,
    pub tools_started: u32,
    pub tools_done: u32,
    pub started: Option<std::time::Instant>,
    pub last_event: Option<std::time::Instant>,
}

impl PaneLive {
    /// Seconds since the turn started, if one is running.
    pub fn elapsed_secs(&self) -> Option<u64> {
        self.started.map(|t| t.elapsed().as_secs())
    }

    /// Seconds since the agent last sent anything. A large number on a running turn means
    /// the agent is silent (stuck, or in a long model call).
    pub fn silent_secs(&self) -> Option<u64> {
        self.last_event
            .or(self.started)
            .map(|t| t.elapsed().as_secs())
    }
}

/// One pane of the full software dev stack pipeline: a role, its grid position, its real
/// agent assignment, and the outcome of the last time it was actually run.
#[derive(Debug, Clone)]
pub struct SwarmPaneState {
    /// The pane's own identity -- matches `AgentAssignment::pane` exactly, so it's also the
    /// key used to fold a finished run's outcome back into the right pane.
    pub name: String,
    /// The config tab this pane belongs to (its mosaic cell's key), e.g. "AGT-CTL". Several
    /// panes share one tab when that tab has a `names` list.
    pub tab: String,
    /// Position within the tab's `names` list (0 for a plain one-pane tab). Preserves the
    /// author's intended order for cycling, since the pane's own `name` carries none.
    pub seq: usize,
    pub grid_x: i32,
    pub grid_y: i32,
    pub assignment: AgentAssignment,
    pub status: SwarmRunStatus,
    pub output: String,
    pub tokens_used: Option<u64>,
    pub context_size: Option<u64>,
    pub cost: Option<(f64, String)>,
    pub last_prompt: Option<String>,
    /// Live turn state: phase, thinking text, streamed reply, tool in progress.
    pub live: PaneLive,
    /// How many lines the detail view is scrolled up from the bottom. 0 follows the newest
    /// line. Reset to 0 when a new turn starts.
    pub scroll_back: usize,
}

impl SwarmPaneState {
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

/// Build the pane list from a loaded config: one entry per pane that has a matching agent
/// assignment, sorted by (y, x, seq) so panes sharing a grid cell land contiguously, in the
/// order their tab's `names` list gave them -- that's what makes cycling within a cell and
/// jumping between cells simple list operations.
fn build_swarm_panes(config: &SwarmConfig) -> Vec<SwarmPaneState> {
    let mut panes: Vec<SwarmPaneState> = config
        .tabs
        .iter()
        .flat_map(|(key, pos)| expand_tab_panes(config, key, pos))
        .collect();
    panes.sort_by_key(|p| (p.grid_y, p.grid_x, p.seq));
    panes
}

/// Expand one tab into its pane(s). A plain tab (no `names`) is one pane keyed by its own
/// tab name. A tab with `names` is a mosaic cell shared by several sub-agents, tabbed between
/// rather than each getting their own cell; each sub-pane is looked up by its bare name,
/// matching the agent roster's `pane:` entries directly.
fn expand_tab_panes(config: &SwarmConfig, key: &str, pos: &PaneGridPos) -> Vec<SwarmPaneState> {
    if pos.names.is_empty() {
        return match config.assignment_for(key) {
            Some(assignment) => vec![SwarmPaneState {
                name: key.to_string(),
                tab: key.to_string(),
                seq: 0,
                grid_x: pos.grid_x,
                grid_y: pos.grid_y,
                assignment: assignment.clone(),
                status: SwarmRunStatus::Idle,
                output: String::new(),
                tokens_used: None,
                context_size: None,
                cost: None,
                last_prompt: None,
                live: PaneLive::default(),
                scroll_back: 0,
            }],
            None => Vec::new(),
        };
    }

    pos.names
        .iter()
        .enumerate()
        .filter_map(|(seq, label)| {
            let assignment = config.assignment_for(label)?.clone();
            Some(SwarmPaneState {
                name: label.clone(),
                tab: key.to_string(),
                seq,
                grid_x: pos.grid_x,
                grid_y: pos.grid_y,
                assignment,
                status: SwarmRunStatus::Idle,
                output: String::new(),
                tokens_used: None,
                context_size: None,
                cost: None,
                last_prompt: None,
                live: PaneLive::default(),
                scroll_back: 0,
            })
        })
        .collect()
}

/// The "switch project" dialog: type a path or pick a recent project.
#[derive(Debug, Clone, Default)]
pub struct ProjectModal {
    pub current: String,
    pub input: String,
    pub recent: Vec<String>,
    /// Index into `recent` of the highlighted entry, if one was picked.
    pub selected: Option<usize>,
    pub error: Option<String>,
}

/// Active agent permission request displayed in an interactive modal.
#[derive(Debug, Clone)]
pub struct PermissionModal {
    pub id: i64,
    pub agent_pane: String,
    pub tool_title: String,
    /// What the tool will do, with secrets masked.
    pub detail: String,
    pub expires_at: std::time::Instant,
    pub reply_to: std::sync::mpsc::Sender<crate::acp::client::PermissionDecision>,
}

/// Application state. Owns all data the view renders and the controller mutates.
#[derive(Debug)]
pub struct Model {
    pub selected_tab: usize,
    pub dashboard_frame: usize,
    pub acp_selected_server: usize,
    pub acp_servers: Vec<AcpServer>,
    pub telemetry: SystemTelemetry,
    pub start_time: std::time::Instant,
    /// Count of real ACP JSON-RPC calls Locust has dispatched this session (handshakes plus
    /// any session/new + session/prompt probe turns). This is live activity, not a guess.
    pub acp_calls_made: u64,
    pub swarm_panes: Vec<SwarmPaneState>,
    pub swarm_selected_pane: usize,
    /// Detail view fills the whole Swarm tab, for reading a long reply.
    pub swarm_zoom: bool,
    /// Key help overlay is open.
    pub show_help: bool,
    /// Task board (agent-to-agent handoffs) overlay is open.
    pub show_tasks: bool,
    /// Lines the Controller chat is scrolled up from the bottom. 0 follows the newest line.
    pub controller_scroll_back: usize,
    /// Open "switch project" dialog, if any.
    pub project_modal: Option<ProjectModal>,
    /// Messages already sent between agents, by id, with the unix second they were sent.
    /// Sending the same message again inside `MESSAGE_WINDOW_SECS` is suppressed.
    pub sent_messages: std::collections::HashMap<u64, u64>,
    /// Where `sent_messages` is saved so a restart does not resend. `None` keeps it in memory.
    message_store: Option<std::path::PathBuf>,
    /// Show the model's thinking text in the agent detail view.
    pub show_thinking: bool,
    /// Set when agents/full-software-dev-stack/agents.yaml failed to load or parse.
    pub swarm_config_error: Option<String>,
    /// Which roster file is actually loaded (the built-in default, or --swarm-config).
    pub swarm_config_path: String,
    /// Run limits loaded from the swarm roster config (turn cap, timeouts, token limit).
    pub limits: crate::swarm::config::RunLimits,
    /// In-flight permission request modal if one is currently awaiting operator decision.
    pub active_permission: Option<PermissionModal>,
    /// Cross-agent activity tracer: one real line per status change, across every ACP
    /// connection and every swarm pane. Feeds Dashboard Frame 2.
    pub activity_log: Vec<String>,
    /// Which part of the Swarm tab has keyboard focus: the agent list, or the Controller
    /// chat input.
    pub swarm_focus: SwarmFocus,
    /// The Controller pane's chat history: every message sent and every reply received,
    /// oldest first. Persists for the whole run, independent of which agent is selected.
    pub controller_log: Vec<ControllerMessage>,
    /// Text currently typed into the Controller input, not yet submitted.
    pub controller_input: String,
    /// Count of agent-to-agent directives auto-dispatched so far this run, against
    /// `limits.turn_cap`.
    pub auto_routed_turns: usize,
    /// Every agent-to-agent task dispatched this run, oldest first, with its real completion
    /// state. This is what lets an orchestrator (or a human) actually check whether a
    /// delegated task finished instead of just trusting an agent's own claim.
    pub tasks: Vec<Task>,
    /// Monotonic id source for `tasks`, so every dispatched task gets a stable identity.
    next_task_id: u64,
}

impl Default for Model {
    fn default() -> Self {
        // `ARGS` is only set by `main()`; fall back to the default roster when it's unset,
        // which is always the case under `cargo test` (tests never go through `main`).
        let configured_path = crate::ARGS
            .get()
            .map(|a| a.swarm_config.as_str())
            .unwrap_or("");
        let swarm_config_path = if configured_path.is_empty() {
            crate::swarm::config::DEFAULT_CONFIG_PATH.to_string()
        } else {
            configured_path.to_string()
        };
        let loaded = if configured_path.is_empty() {
            SwarmConfig::load_default()
        } else {
            SwarmConfig::load(std::path::Path::new(configured_path))
        };
        let (swarm_panes, swarm_config_error, limits) = match loaded {
            Ok(config) => (build_swarm_panes(&config), None, config.limits),
            Err(e) => (
                Vec::new(),
                Some(e.to_string()),
                crate::swarm::config::RunLimits::default(),
            ),
        };

        Self {
            selected_tab: 0,
            dashboard_frame: 0,
            acp_selected_server: 0,
            telemetry: SystemTelemetry::new(),
            start_time: std::time::Instant::now(),
            acp_servers: AGENT_SPECS.iter().map(AcpServer::from_spec).collect(),
            acp_calls_made: 0,
            swarm_panes,
            swarm_selected_pane: 0,
            swarm_zoom: false,
            show_help: false,
            show_tasks: false,
            controller_scroll_back: 0,
            project_modal: None,
            sent_messages: std::collections::HashMap::new(),
            message_store: None,
            show_thinking: true,
            swarm_config_error,
            swarm_config_path,
            limits,
            active_permission: None,
            activity_log: Vec::new(),
            swarm_focus: SwarmFocus::List,
            controller_log: Vec::new(),
            controller_input: String::new(),
            auto_routed_turns: 0,
            tasks: Vec::new(),
            next_task_id: 1,
        }
    }
}

impl Model {
    pub fn snapshot(&self, project_path: &str) -> crate::ipc::protocol::ModelSnapshot {
        use crate::ipc::protocol::*;

        let acp_servers = self
            .acp_servers
            .iter()
            .map(|s| AcpServerSnapshot {
                id: s.id.to_string(),
                name: s.name.to_string(),
                transport: s.transport.to_string(),
                command_line: s.command_line.clone(),
                status: s.status.label().to_string(),
                protocol: s.protocol.clone(),
                ping_ms: s.ping_ms,
                capabilities: s.capabilities.clone(),
                connected_at_secs: s.connected_at.map(|t| t.elapsed().as_secs()),
                recent_logs: s.recent_logs.clone(),
                tokens_used: s.tokens_used,
                context_size: s.context_size,
                cost: s.cost.clone(),
                stop_reason: s.stop_reason.clone(),
            })
            .collect();

        let swarm_panes = self
            .swarm_panes
            .iter()
            .map(|p| SwarmPaneSnapshot {
                name: p.name.clone(),
                agent: p.assignment.agent.clone(),
                model: p.assignment.model.clone(),
                task: p.assignment.task.clone().unwrap_or_default(),
                status: p.status.label().to_string(),
                output_lines: p.output.lines().map(String::from).collect(),
                thinking_lines: p.live.thought.lines().map(String::from).collect(),
                turns_used: 0,
                tokens_used: p.tokens_used.unwrap_or(0),
                phase: format!("{:?}", p.live.phase),
                elapsed_secs: p.live.started.map(|t| t.elapsed().as_secs()).unwrap_or(0),
                auto_route_budget: 0,
                unread_count: 0,
            })
            .collect();

        let controller_history = self
            .controller_log
            .iter()
            .map(|m| ControllerMessageSnapshot {
                role: match m.role {
                    ControllerRole::User => "operator".to_string(),
                    ControllerRole::Agent => "agent".to_string(),
                    ControllerRole::System => "system".to_string(),
                },
                text: m.text.clone(),
                timestamp_secs: 0,
            })
            .collect();

        let tasks = self
            .tasks
            .iter()
            .map(|t| TaskSnapshot {
                id: t.id,
                from: t.source.clone(),
                to: t.target.clone(),
                prompt: t.description.clone(),
                status: match t.status {
                    TaskStatus::Pending => "pending".to_string(),
                    TaskStatus::Done => "done".to_string(),
                    TaskStatus::Failed => "failed".to_string(),
                },
            })
            .collect();

        let active_permission = self.active_permission.as_ref().map(|p| PermissionSnapshot {
            request_id: p.id.to_string(),
            agent_id: p.agent_pane.clone(),
            tool: p.tool_title.clone(),
            params: p.detail.clone(),
            seconds_left: p
                .expires_at
                .saturating_duration_since(Instant::now())
                .as_secs(),
        });

        ModelSnapshot {
            project_path: project_path.to_string(),
            selected_tab: self.selected_tab,
            dashboard_frame: self.dashboard_frame,
            acp_selected_server: self.acp_selected_server,
            acp_servers,
            swarm_selected_pane: self.swarm_selected_pane,
            swarm_panes,
            show_thinking: self.show_thinking,
            controller_history,
            tasks,
            active_permission,
            activity_log: self.activity_log.clone(),
            total_tokens_used: self.telemetry.total_tokens,
            total_cost_usd: self.telemetry.total_cost.as_ref().map(|(c, _)| *c).unwrap_or(0.0),
            total_invocations: self.telemetry.invocations,
            turn_cap: self.limits.turn_cap as u32,
            timeout_secs: self.limits.timeout_secs,
        }
    }

    pub fn refresh_telemetry(&mut self) {
        self.telemetry.refresh(&self.start_time);
    }

    /// Append one real event to the cross-agent activity tracer, timestamped by elapsed
    /// time since Locust started (matches the "Uptime" clock already shown in Frame 3).
    pub fn push_activity(&mut self, source: &str, message: impl Into<String>) {
        let secs = self.start_time.elapsed().as_secs();
        let line = format!(
            "[{:02}:{:02}:{:02}] [{}] {}",
            secs / 3600,
            (secs % 3600) / 60,
            secs % 60,
            source,
            message.into()
        );
        crate::logging::log(crate::logging::Level::Info, &line);
        self.activity_log.push(line);
        if self.activity_log.len() > MAX_ACTIVITY_LOG {
            let overflow = self.activity_log.len() - MAX_ACTIVITY_LOG;
            self.activity_log.drain(0..overflow);
        }
    }

    pub fn select_tab(&mut self, idx: usize) {
        if idx < TOP_LEVEL_TABS {
            self.selected_tab = idx;
            self.swarm_focus = SwarmFocus::List;
        }
    }

    pub fn next_tab(&mut self) {
        self.selected_tab = (self.selected_tab + 1) % TOP_LEVEL_TABS;
        self.swarm_focus = SwarmFocus::List;
    }

    pub fn prev_tab(&mut self) {
        self.selected_tab = (self.selected_tab + TOP_LEVEL_TABS - 1) % TOP_LEVEL_TABS;
        self.swarm_focus = SwarmFocus::List;
    }

    /// Toggle keyboard focus, within the Swarm tab, between the agent list and the
    /// Controller chat input.
    pub fn toggle_swarm_focus(&mut self) {
        self.swarm_focus = match self.swarm_focus {
            SwarmFocus::List => SwarmFocus::Controller,
            SwarmFocus::Controller => SwarmFocus::List,
        };
    }

    pub fn is_controller_focused(&self) -> bool {
        self.selected_tab == 2 && self.swarm_focus == SwarmFocus::Controller
    }

    /// Pull every directive an agent addressed to another pane out of `text` that this run is
    /// actually allowed to dispatch: target can be a pane name, a tab name, or `*` for swarm-wide broadcast.
    /// Everything runs under the configured run limits (turn cap and token limit). Anything rejected
    /// is logged to the activity tracer (never silently dropped).
    fn approved_directives(&mut self, source: &str, text: &str) -> Vec<(String, String)> {
        let mut approved = Vec::new();
        for directive in crate::swarm::parse_directives(text) {
            let target_panes: Vec<String> = if directive.target == "*" {
                self.swarm_panes
                    .iter()
                    .map(|p| p.name.clone())
                    .filter(|name| name != source)
                    .collect()
            } else if self.swarm_panes.iter().any(|p| p.tab == directive.target) {
                self.swarm_panes
                    .iter()
                    .filter(|p| p.tab == directive.target && p.name != source)
                    .map(|p| p.name.clone())
                    .collect()
            } else if self.swarm_panes.iter().any(|p| p.name == directive.target) {
                vec![directive.target.clone()]
            } else {
                self.push_activity(
                    source,
                    format!("addressed unknown agent '{}', ignoring", directive.target),
                );
                continue;
            };

            for target_name in target_panes {
                let msg_id = Self::message_id(source, &target_name, &directive.message);
                if self.is_duplicate_message(msg_id) {
                    self.push_activity(
                        source,
                        format!(
                            "duplicate message to '{target_name}' suppressed (id {msg_id:016x})"
                        ),
                    );
                    continue;
                }
                if let Some(limit) = self.limits.token_limit
                    && self.telemetry.total_tokens >= limit
                {
                    self.push_activity(
                        source,
                        format!(
                            "token limit ({limit}) reached, dropping message to '{target_name}'"
                        ),
                    );
                    break;
                }
                if self.auto_routed_turns >= self.limits.turn_cap {
                    self.push_activity(
                        source,
                        format!(
                            "auto-route budget exhausted, dropping message to '{}'",
                            target_name
                        ),
                    );
                    break;
                }
                self.auto_routed_turns += 1;
                let preview: String = directive.message.chars().take(80).collect();
                self.push_activity(source, format!("-> {}: {}", target_name, preview));
                self.create_task(source, &target_name, &directive.message);
                approved.push((target_name, directive.message.clone()));
            }
        }
        approved
    }

    /// Stable id of one agent-to-agent message: same source, target and text give the same id,
    /// in every run. Receivers see it in the delivered prompt.
    pub fn message_id(source: &str, target: &str, message: &str) -> u64 {
        crate::project::fnv1a(&[source, target, message.trim()])
    }

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Keep sent-message ids in `path` so they survive a restart. Loads what is there and
    /// drops entries older than the window.
    pub fn enable_message_store(&mut self, path: std::path::PathBuf) {
        let now = Self::now_secs();
        let saved: std::collections::HashMap<String, u64> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        for (k, at) in saved {
            if let Ok(id) = u64::from_str_radix(&k, 16)
                && now.saturating_sub(at) < MESSAGE_WINDOW_SECS
            {
                self.sent_messages.insert(id, at);
            }
        }
        self.message_store = Some(path);
    }

    fn save_messages(&self) {
        let Some(path) = &self.message_store else {
            return;
        };
        let map: std::collections::HashMap<String, u64> = self
            .sent_messages
            .iter()
            .map(|(k, v)| (format!("{k:016x}"), *v))
            .collect();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(&map) {
            let _ = std::fs::write(path, json);
        }
    }

    /// True if this exact message was already sent inside the window.
    fn is_duplicate_message(&self, id: u64) -> bool {
        self.sent_messages
            .get(&id)
            .is_some_and(|at| Self::now_secs().saturating_sub(*at) < MESSAGE_WINDOW_SECS)
    }

    /// Record one dispatched agent-to-agent task as Pending. Its completion is folded in
    /// later by `complete_oldest_pending_task`, once the real turn it triggered finishes.
    fn create_task(&mut self, source: &str, target: &str, description: &str) {
        let id = self.next_task_id;
        self.next_task_id += 1;
        let msg_id = Self::message_id(source, target, description);
        self.sent_messages.insert(msg_id, Self::now_secs());
        self.save_messages();
        self.tasks.push(Task {
            id,
            msg_id,
            source: source.to_string(),
            target: target.to_string(),
            description: description.to_string(),
            status: TaskStatus::Pending,
        });
    }

    /// Mark the oldest still-Pending task addressed to `target` as finished, Done or Failed
    /// per the real outcome of the turn it triggered. Tasks for the same target complete in
    /// dispatch order since every turn into one pane's session runs on that pane's single
    /// serialized actor thread -- a no-op if `target` has no dispatched task pending (e.g. a
    /// pane's regular startup run, which was never addressed by a directive).
    fn complete_oldest_pending_task(&mut self, target: &str, status: TaskStatus) {
        if let Some(task) = self
            .tasks
            .iter_mut()
            .find(|t| t.target == target && t.status == TaskStatus::Pending)
        {
            task.status = status;
            // A failed message may be sent again: forget its id so a retry is not suppressed.
            if status == TaskStatus::Failed {
                let msg_id = task.msg_id;
                self.sent_messages.remove(&msg_id);
                self.save_messages();
            }
        }
    }

    /// Counts of dispatched tasks by status, as (pending, done, failed).
    pub fn task_counts(&self) -> (usize, usize, usize) {
        let mut counts = (0, 0, 0);
        for task in &self.tasks {
            match task.status {
                TaskStatus::Pending => counts.0 += 1,
                TaskStatus::Done => counts.1 += 1,
                TaskStatus::Failed => counts.2 += 1,
            }
        }
        counts
    }

    pub fn fail_task_for(&mut self, target: &str) {
        self.complete_oldest_pending_task(target, TaskStatus::Failed);
    }

    /// Cancel a running turn on the swarm pane at `idx`.
    pub fn cancel_swarm_pane(&mut self, idx: usize) -> Option<String> {
        if idx >= self.swarm_panes.len() {
            return None;
        }
        let pane = &mut self.swarm_panes[idx];
        if pane.status == SwarmRunStatus::Running {
            pane.status = SwarmRunStatus::Error;
            pane.live.phase = TurnPhase::Idle;
            pane.live.tool = None;
            pane.live.started = None;
            let name = pane.name.clone();
            self.complete_oldest_pending_task(&name, TaskStatus::Failed);
            self.push_activity(&name, "turn cancelled by operator");
            Some(name)
        } else {
            None
        }
    }

    /// Set an active permission modal from an agent prompt.
    pub fn set_permission_modal(&mut self, prompt: crate::acp::manager::PermissionPrompt) {
        self.active_permission = Some(PermissionModal {
            id: prompt.id,
            agent_pane: prompt.agent_pane,
            tool_title: prompt.tool_title,
            detail: crate::redact::redact(&prompt.detail),
            expires_at: prompt.expires_at,
            reply_to: prompt.reply_to,
        });
    }

    /// Check if the active permission modal has reached its deadline.
    /// If timed out, automatically denies permission and closes the modal.
    pub fn check_permission_timeout(&mut self) -> bool {
        if let Some(perm) = &self.active_permission
            && std::time::Instant::now() >= perm.expires_at
        {
            let p = self.active_permission.take().unwrap();
            let _ = p
                .reply_to
                .send(crate::acp::client::PermissionDecision::Deny);
            self.push_activity(&p.agent_pane, "permission request timed out (denied)");
            return true;
        }
        false
    }

    /// Export all pane transcripts to `.locust/runs/<timestamp>/<pane>.md`.
    /// Secrets are always redacted before writing.
    pub fn export_transcripts(
        &self,
        base_dir: Option<&std::path::Path>,
    ) -> std::io::Result<std::path::PathBuf> {
        let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
        let target_dir = match base_dir {
            Some(dir) => dir.join(".locust").join("runs").join(&timestamp),
            None => std::path::Path::new(".locust")
                .join("runs")
                .join(&timestamp),
        };
        std::fs::create_dir_all(&target_dir)?;

        for pane in &self.swarm_panes {
            let filename = format!("{}.md", sanitize_filename(&pane.name));
            let filepath = target_dir.join(filename);
            let content = format!(
                "# Transcript: {}\n\n- **Agent**: {}\n- **Model**: {}\n- **Tab**: {}\n- **Status**: {:?}\n\n## Output\n\n{}\n",
                pane.name,
                pane.assignment.agent,
                pane.assignment.model,
                pane.tab,
                pane.status,
                crate::redact::redact(&pane.output)
            );
            write_private(&filepath, &content)?;
        }

        let controller_path = target_dir.join("Controller.md");
        let mut controller_content = String::from("# Transcript: Controller\n\n## History\n\n");
        for msg in &self.controller_log {
            controller_content.push_str(&format!(
                "- **[{:?}]**: {}\n",
                msg.role,
                crate::redact::redact(&msg.text)
            ));
        }
        write_private(&controller_path, &controller_content)?;

        Ok(target_dir)
    }

    pub fn controller_type_char(&mut self, c: char) {
        self.controller_input.push(c);
    }

    pub fn controller_backspace(&mut self) {
        self.controller_input.pop();
    }

    /// Submit the typed message: push it to the chat log as the user's turn and clear the
    /// input. Returns the text to actually send, or `None` if there was nothing to send.
    pub fn controller_submit(&mut self) -> Option<String> {
        let text = self.controller_input.trim().to_string();
        self.controller_input.clear();
        if text.is_empty() {
            return None;
        }
        self.controller_log.push(ControllerMessage {
            role: ControllerRole::User,
            text: text.clone(),
        });
        // Sending a message jumps back to the newest line.
        self.controller_scroll_back = 0;
        Some(text)
    }

    pub fn controller_scroll_up(&mut self, lines: usize) {
        self.controller_scroll_back = self.controller_scroll_back.saturating_add(lines);
    }

    pub fn controller_scroll_down(&mut self, lines: usize) {
        self.controller_scroll_back = self.controller_scroll_back.saturating_sub(lines);
    }

    pub fn controller_scroll_top(&mut self) {
        self.controller_scroll_back = usize::MAX;
    }

    pub fn controller_scroll_follow(&mut self) {
        self.controller_scroll_back = 0;
    }

    /// The Controller pane's connection status is just whatever the shared ACP pool reports
    /// for "acp-claude" -- the exact same record the ACP Servers tab reads, since they're
    /// the same spawned connection. There is no separate Controller-only status to drift.
    pub fn controller_connection_status(&self) -> AcpStatus {
        self.acp_servers
            .iter()
            .find(|s| s.id == CONTROLLER_SPEC_ID)
            .map(|s| s.status)
            .unwrap_or(AcpStatus::Offline)
    }

    /// Fold the result of one Controller chat turn into its history, and hand back any
    /// agent-to-agent directives the reply addressed to a Swarm pane (approved for dispatch).
    pub fn push_controller_reply(
        &mut self,
        result: Result<String, String>,
    ) -> Vec<(String, String)> {
        match result {
            Ok(text) => {
                let directives = self.approved_directives("Controller", &text);
                self.controller_log.push(ControllerMessage {
                    role: ControllerRole::Agent,
                    text,
                });
                directives
            }
            Err(err) => {
                self.controller_log.push(ControllerMessage {
                    role: ControllerRole::System,
                    text: format!("ERROR: {err}"),
                });
                Vec::new()
            }
        }
    }

    pub fn next_dashboard_frame(&mut self) {
        if self.selected_tab == 0 {
            self.dashboard_frame = (self.dashboard_frame + 1) % 3;
        }
    }

    pub fn prev_dashboard_frame(&mut self) {
        if self.selected_tab == 0 {
            self.dashboard_frame = (self.dashboard_frame + 2) % 3;
        }
    }

    pub fn next_server(&mut self) {
        if self.selected_tab == 1 && !self.acp_servers.is_empty() {
            self.acp_selected_server = (self.acp_selected_server + 1) % self.acp_servers.len();
        } else if self.selected_tab >= 2 {
            self.next_swarm_pane();
        }
    }

    pub fn prev_server(&mut self) {
        if self.selected_tab == 1 && !self.acp_servers.is_empty() {
            self.acp_selected_server =
                (self.acp_selected_server + self.acp_servers.len() - 1) % self.acp_servers.len();
        } else if self.selected_tab >= 2 {
            self.prev_swarm_pane();
        }
    }

    pub fn selected_server(&self) -> Option<&AcpServer> {
        self.acp_servers.get(self.acp_selected_server)
    }

    /// Mark the selected ACP server as connecting and log the attempt. The real handshake
    /// runs in the background; call `apply_acp_update` once it reports back.
    pub fn begin_connect_selected(&mut self) -> Option<&'static str> {
        let server = self.acp_servers.get_mut(self.acp_selected_server)?;
        server.status = AcpStatus::Connecting;
        server.push_log(format!("--> launching: {}", server.command_line));
        self.acp_calls_made += 1; // the initialize handshake itself
        let id = server.id;
        self.push_activity(id, "connecting...");
        Some(id)
    }

    /// Mark all ACP servers as connecting and probe them.
    pub fn begin_connect_all(&mut self) {
        for server in &mut self.acp_servers {
            server.status = AcpStatus::Connecting;
            server.push_log(format!(
                "--> launching and probing: {}",
                server.command_line
            ));
            self.acp_calls_made += 1;
        }
        self.push_activity("system", "connecting and probing all agents...");
    }

    /// Sum of real tokens-in-context reported by connected agents' usage probes or swarm runs.
    pub fn acp_tokens_used(&self) -> u64 {
        let server_tokens: u64 = self.acp_servers.iter().filter_map(|s| s.tokens_used).sum();
        let swarm_tokens: u64 = self.swarm_panes.iter().filter_map(|p| p.tokens_used).sum();
        server_tokens.max(swarm_tokens)
    }

    /// Sum of remaining tokens in context across all servers/panes that reported context size.
    pub fn acp_tokens_left(&self) -> Option<u64> {
        let server_left: u64 = self
            .acp_servers
            .iter()
            .filter_map(|s| s.tokens_left())
            .sum();
        let swarm_left: u64 = self
            .swarm_panes
            .iter()
            .filter_map(|p| p.tokens_left())
            .sum();
        if server_left > 0 || swarm_left > 0 {
            Some(server_left.max(swarm_left))
        } else {
            None
        }
    }

    /// Sum of total context window sizes across all servers/panes that reported one.
    pub fn acp_context_size(&self) -> Option<u64> {
        let server_size: u64 = self.acp_servers.iter().filter_map(|s| s.context_size).sum();
        let swarm_size: u64 = self.swarm_panes.iter().filter_map(|p| p.context_size).sum();
        if server_size > 0 || swarm_size > 0 {
            Some(server_size.max(swarm_size))
        } else {
            None
        }
    }

    /// Combined cost of all probes that reported one, assuming a single shared currency
    /// (true in practice -- each probe is a single real-money API call).
    pub fn acp_total_cost(&self) -> Option<(f64, String)> {
        let mut total = 0.0;
        let mut currency = None;
        for server in &self.acp_servers {
            if let Some((amount, cur)) = &server.cost {
                total += amount;
                currency.get_or_insert_with(|| cur.clone());
            }
        }
        currency.map(|c| (total, c))
    }

    pub fn mark_disconnected(&mut self, server_id: &str) {
        let found = if let Some(server) = self.acp_servers.iter_mut().find(|s| s.id == server_id) {
            server.status = AcpStatus::Offline;
            server.connected_at = None;
            server.ping_ms = 0;
            server.push_log("[INFO] disconnected".to_string());
            true
        } else {
            false
        };
        if found {
            self.push_activity(server_id, "disconnected");
        }
    }

    /// Fold a background update from the shared ACP pool into the matching server's state.
    /// This is the single place any agent's real connection status/usage lands -- the ACP
    /// Servers tab, the Controller pane, and every Swarm pane run all read it back from here.
    pub fn apply_acp_update(&mut self, update: AgentUpdate) {
        let server_id = update.server_id;
        let server_idx = self.acp_servers.iter().position(|s| s.id == server_id);
        let Some(idx) = server_idx else { return };

        match update.outcome {
            AgentOutcome::Connected(info) => {
                let server = &mut self.acp_servers[idx];
                server.status = AcpStatus::Online;
                server.protocol = format!("ACP/{} (JSON-RPC 2.0)", info.protocol_version);
                server.ping_ms = info.ping_ms;
                server.capabilities = info.capabilities;
                server.connected_at = Some(Instant::now());
                let agent_label = match (&info.agent_name, &info.agent_version) {
                    (Some(name), Some(version)) => format!("{} v{}", name, version),
                    (Some(name), None) => name.clone(),
                    _ => "agent".to_string(),
                };
                server.push_log(format!(
                    "<-- initialize ok | {} | auth={}",
                    agent_label,
                    if info.auth_methods.is_empty() {
                        "none required".to_string()
                    } else {
                        info.auth_methods.join(",")
                    }
                ));
                self.push_activity(server_id, "online");
            }
            AgentOutcome::UsageProbed(usage) => {
                let server = &mut self.acp_servers[idx];
                server.tokens_used = usage.tokens_used;
                server.context_size = usage.context_size;
                server.cost = match (usage.cost_amount, &usage.cost_currency) {
                    (Some(amount), Some(currency)) => Some((amount, currency.clone())),
                    _ => None,
                };
                server.stop_reason = usage.stop_reason.clone();
                self.acp_calls_made += 2; // session/new + session/prompt

                crate::core::telemetry::record_tokens(
                    server_id,
                    0,
                    0,
                    usage.tokens_used,
                    usage.context_size,
                    usage.cost_amount.map(|c| {
                        (
                            c,
                            usage
                                .cost_currency
                                .clone()
                                .unwrap_or_else(|| "USD".to_string()),
                        )
                    }),
                );

                let tokens = usage
                    .tokens_used
                    .map(|u| u.to_string())
                    .unwrap_or_else(|| "?".to_string());
                let size = usage
                    .context_size
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "?".to_string());
                let remaining = match (usage.tokens_used, usage.context_size) {
                    (Some(u), Some(s)) => {
                        let left = s.saturating_sub(u);
                        let pct = if s > 0 {
                            (left as f64 / s as f64) * 100.0
                        } else {
                            0.0
                        };
                        format!(" ({} left, {:.1}%)", left, pct)
                    }
                    _ => String::new(),
                };
                let cost = match (usage.cost_amount, &usage.cost_currency) {
                    (Some(amount), Some(currency)) => format!(" | cost {:.4} {}", amount, currency),
                    _ => String::new(),
                };
                let stop_reason = usage.stop_reason.clone().unwrap_or_else(|| "?".to_string());
                server.push_log(format!(
                    "<-- session/prompt ok | stop={} | tokens {}/{}{}{}",
                    stop_reason, tokens, size, remaining, cost
                ));
                self.push_activity(
                    server_id,
                    format!(
                        "real probe ok (stop={stop_reason}, tokens {tokens}/{size}{remaining})"
                    ),
                );
            }
            AgentOutcome::ProbeFailed(err) => {
                let server = &mut self.acp_servers[idx];
                server.push_log(format!(
                    "[INFO] connected, but the real-token probe did not complete: {err} -- \
                     the agent likely needs interactive auth Locust cannot provide."
                ));
                self.push_activity(
                    server_id,
                    "online, but the real-token probe did not complete",
                );
            }
            AgentOutcome::Failed(err) => {
                let server = &mut self.acp_servers[idx];
                server.status = AcpStatus::Error;
                server.protocol = "ACP (unreachable)".to_string();
                server.ping_ms = 0;
                server.push_log(format!("[ERROR] {}", err));
                self.push_activity(server_id, format!("ERROR: {err}"));
            }
        }
    }

    pub fn selected_swarm_pane(&self) -> Option<&SwarmPaneState> {
        self.swarm_panes.get(self.swarm_selected_pane)
    }

    /// Counts of swarm panes by status, as (idle, running, done, error).
    pub fn swarm_status_counts(&self) -> (usize, usize, usize, usize) {
        let mut counts = (0, 0, 0, 0);
        for pane in &self.swarm_panes {
            match pane.status {
                SwarmRunStatus::Idle => counts.0 += 1,
                SwarmRunStatus::Running => counts.1 += 1,
                SwarmRunStatus::Done => counts.2 += 1,
                SwarmRunStatus::Error => counts.3 += 1,
            }
        }
        counts
    }

    /// Move the agent list selection to the next/prev pane, wrapping around.
    pub fn next_swarm_pane(&mut self) {
        if self.swarm_panes.is_empty() {
            return;
        }
        self.swarm_selected_pane = (self.swarm_selected_pane + 1) % self.swarm_panes.len();
    }

    pub fn prev_swarm_pane(&mut self) {
        if self.swarm_panes.is_empty() {
            return;
        }
        self.swarm_selected_pane =
            (self.swarm_selected_pane + self.swarm_panes.len() - 1) % self.swarm_panes.len();
    }

    /// Mark the selected swarm pane as running and hand back its agent assignment so the
    /// controller can actually launch it. The real process runs in the background; call
    /// `apply_swarm_run_outcome` once it reports back.
    pub fn begin_run_selected_swarm_pane(&mut self) -> Option<AgentAssignment> {
        self.begin_run_pane(self.swarm_selected_pane)
    }

    /// Fold one live event from a running turn into the pane it belongs to. Events for a
    /// pane that is no longer running (cancelled, or already finished) are ignored.
    pub fn apply_turn_event(&mut self, pane: &str, event: crate::acp::TurnEvent) {
        use crate::acp::TurnEvent;
        let Some(p) = self.swarm_panes.iter_mut().find(|p| p.name == pane) else {
            return;
        };
        if p.status != SwarmRunStatus::Running {
            return;
        }
        p.live.last_event = Some(std::time::Instant::now());
        match event {
            TurnEvent::Thought(text) => {
                p.live.phase = TurnPhase::Thinking;
                p.live.thought.push_str(&text);
                if p.live.thought.chars().count() > MAX_THOUGHT_CHARS {
                    let drop = p.live.thought.chars().count() - MAX_THOUGHT_CHARS;
                    p.live.thought = p.live.thought.chars().skip(drop).collect();
                }
            }
            TurnEvent::Message(text) => {
                p.live.phase = TurnPhase::Writing;
                p.live.partial.push_str(&text);
                p.output = p.live.partial.clone();
            }
            TurnEvent::ToolStarted(title) => {
                p.live.phase = TurnPhase::Tool;
                p.live.tool = Some(title);
                p.live.tools_started += 1;
            }
            TurnEvent::ToolUpdate { title, done } => {
                if done {
                    p.live.tools_done += 1;
                    p.live.tool = None;
                    p.live.phase = TurnPhase::Waiting;
                } else if let Some(t) = title {
                    p.live.tool = Some(t);
                    p.live.phase = TurnPhase::Tool;
                }
            }
        }
    }

    /// Scroll the selected pane's detail view up by `lines` (toward older text).
    pub fn scroll_pane_up(&mut self, lines: usize) {
        if let Some(p) = self.swarm_panes.get_mut(self.swarm_selected_pane) {
            p.scroll_back = p.scroll_back.saturating_add(lines);
        }
    }

    /// Scroll the selected pane's detail view down by `lines` (toward the newest text).
    pub fn scroll_pane_down(&mut self, lines: usize) {
        if let Some(p) = self.swarm_panes.get_mut(self.swarm_selected_pane) {
            p.scroll_back = p.scroll_back.saturating_sub(lines);
        }
    }

    /// Jump the selected pane's detail view to its oldest line.
    pub fn scroll_pane_top(&mut self) {
        if let Some(p) = self.swarm_panes.get_mut(self.swarm_selected_pane) {
            p.scroll_back = usize::MAX;
        }
    }

    /// Follow the newest line again.
    pub fn scroll_pane_follow(&mut self) {
        if let Some(p) = self.swarm_panes.get_mut(self.swarm_selected_pane) {
            p.scroll_back = 0;
        }
    }

    /// Open the switch-project dialog with the recent list.
    pub fn open_project_modal(&mut self, current: String, recent: Vec<String>) {
        self.project_modal = Some(ProjectModal {
            current,
            recent,
            ..ProjectModal::default()
        });
    }

    pub fn project_modal_type(&mut self, c: char) {
        if let Some(m) = &mut self.project_modal {
            m.input.push(c);
            m.selected = None;
            m.error = None;
        }
    }

    pub fn project_modal_backspace(&mut self) {
        if let Some(m) = &mut self.project_modal {
            m.input.pop();
            m.selected = None;
            m.error = None;
        }
    }

    /// Move the highlight through the recent list and copy that path into the input.
    pub fn project_modal_move(&mut self, down: bool) {
        let Some(m) = &mut self.project_modal else {
            return;
        };
        if m.recent.is_empty() {
            return;
        }
        let last = m.recent.len() - 1;
        let next = match (m.selected, down) {
            (None, true) => 0,
            (None, false) => last,
            (Some(i), true) => (i + 1).min(last),
            (Some(i), false) => i.saturating_sub(1),
        };
        m.selected = Some(next);
        m.input = m.recent[next].clone();
        m.error = None;
    }

    pub fn toggle_swarm_zoom(&mut self) {
        self.swarm_zoom = !self.swarm_zoom;
    }

    pub fn toggle_show_thinking(&mut self) {
        self.show_thinking = !self.show_thinking;
    }

    /// Mark the pane at `idx` as running and hand back its agent assignment, regardless of
    /// which pane (if any) is currently selected -- used both by the Enter-key run and by
    /// running every pane automatically at startup.
    pub fn begin_run_pane(&mut self, idx: usize) -> Option<AgentAssignment> {
        let pane = self.swarm_panes.get_mut(idx)?;
        pane.status = SwarmRunStatus::Running;
        pane.output = format!(
            "-> launching {} ({})...",
            pane.assignment.agent, pane.assignment.model
        );
        pane.live = PaneLive {
            phase: TurnPhase::Waiting,
            started: Some(std::time::Instant::now()),
            ..PaneLive::default()
        };
        pane.scroll_back = 0;
        self.acp_calls_made += 1;
        let name = pane.name.clone();
        let agent = pane.assignment.agent.clone();
        let model = pane.assignment.model.clone();
        let assignment = pane.assignment.clone();
        self.push_activity(&name, format!("launching {agent} ({model})..."));
        Some(assignment)
    }

    /// Fold a finished real agent run back into the matching pane's state, and hand back any
    /// agent-to-agent directives its reply addressed to another pane (approved for dispatch).
    pub fn apply_swarm_run_outcome(&mut self, outcome: RunOutcome) -> Vec<(String, String)> {
        let Some(idx) = self.swarm_panes.iter().position(|p| p.name == outcome.pane) else {
            return Vec::new();
        };
        let name = self.swarm_panes[idx].name.clone();

        if let Some(usage) = &outcome.usage {
            if let Some(u) = usage.tokens_used {
                self.swarm_panes[idx].tokens_used = Some(u);
            }
            if let Some(sz) = usage.context_size {
                self.swarm_panes[idx].context_size = Some(sz);
            }
            if let Some(cost) = usage.cost_amount {
                let curr = usage
                    .cost_currency
                    .clone()
                    .unwrap_or_else(|| "USD".to_string());
                self.swarm_panes[idx].cost = Some((cost, curr));
            }
            crate::core::telemetry::record_tokens(
                &name,
                0,
                0,
                usage.tokens_used,
                usage.context_size,
                usage.cost_amount.map(|c| {
                    (
                        c,
                        usage
                            .cost_currency
                            .clone()
                            .unwrap_or_else(|| "USD".to_string()),
                    )
                }),
            );
            self.refresh_telemetry();
        }

        {
            // The turn is over: keep the thinking text for review, drop the in-flight bits.
            let live = &mut self.swarm_panes[idx].live;
            live.phase = TurnPhase::Idle;
            live.tool = None;
            live.partial.clear();
            live.started = None;
        }
        let (message, directives) = match outcome.output {
            Ok(text) => {
                self.swarm_panes[idx].status = SwarmRunStatus::Done;
                self.swarm_panes[idx].output = text.clone();
                self.complete_oldest_pending_task(&name, TaskStatus::Done);
                let preview: String = text.chars().take(80).collect();
                let directives = self.approved_directives(&name, &text);
                (format!("done: {preview}"), directives)
            }
            Err(err) => {
                self.swarm_panes[idx].status = SwarmRunStatus::Error;
                self.swarm_panes[idx].output = err.clone();
                self.complete_oldest_pending_task(&name, TaskStatus::Failed);
                (format!("ERROR: {err}"), Vec::new())
            }
        };
        self.push_activity(&name, message);
        directives
    }
}

/// Write a file only its owner can read. Transcripts can hold code and data that redaction
/// does not recognise, so they should not be readable by other users of the machine.
fn write_private(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)?.write_all(content.as_bytes())
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_initialization() {
        let model = Model::default();
        assert_eq!(model.selected_tab, 0);
        assert_eq!(model.dashboard_frame, 0);
        assert_eq!(model.acp_selected_server, 0);
        assert!(!model.acp_servers.is_empty());
    }

    #[test]
    fn test_dashboard_frame_cycling() {
        let mut model = Model::default();
        assert_eq!(model.dashboard_frame, 0);

        model.next_dashboard_frame();
        assert_eq!(model.dashboard_frame, 1);

        model.next_dashboard_frame();
        assert_eq!(model.dashboard_frame, 2);

        model.next_dashboard_frame();
        assert_eq!(model.dashboard_frame, 0);

        model.prev_dashboard_frame();
        assert_eq!(model.dashboard_frame, 2);
    }

    #[test]
    fn test_tab_switching() {
        let mut model = Model::default();
        assert_eq!(model.selected_tab, 0);

        model.next_tab();
        assert_eq!(model.selected_tab, 1);

        model.next_tab();
        assert_eq!(model.selected_tab, 2);

        model.next_tab();
        assert_eq!(model.selected_tab, 3);

        model.next_tab();
        assert_eq!(model.selected_tab, 0);

        model.prev_tab();
        assert_eq!(model.selected_tab, 3);
    }

    #[test]
    fn test_server_selection_wraps() {
        let mut model = Model::default();
        model.select_tab(1);
        let len = model.acp_servers.len();

        model.prev_server();
        assert_eq!(model.acp_selected_server, len - 1);

        model.next_server();
        assert_eq!(model.acp_selected_server, 0);
    }

    #[test]
    fn test_swarm_panes_loaded_from_real_config() {
        let model = Model::default();
        assert!(
            model.swarm_config_error.is_none(),
            "{:?}",
            model.swarm_config_error
        );
        // 5 tier tabs (CTL 4 + SPC 3 + IMP 5 + VER 5 + OPS 4 named sub-panes) = 21 total.
        assert_eq!(model.swarm_panes.len(), 21);
    }

    #[test]
    fn test_swarm_next_pane_advances_and_wraps() {
        let mut model = Model::default();
        model.select_tab(2);
        let len = model.swarm_panes.len();

        model.next_swarm_pane();
        assert_eq!(model.swarm_selected_pane, 1 % len);

        model.prev_swarm_pane();
        assert_eq!(model.swarm_selected_pane, 0);

        model.prev_swarm_pane();
        assert_eq!(model.swarm_selected_pane, len - 1);
    }

    #[test]
    fn test_swarm_focus_toggles_and_resets_on_tab_switch() {
        let mut model = Model::default();
        model.select_tab(2);
        assert_eq!(model.swarm_focus, SwarmFocus::List);
        assert!(!model.is_controller_focused());

        model.toggle_swarm_focus();
        assert_eq!(model.swarm_focus, SwarmFocus::Controller);
        assert!(model.is_controller_focused());

        // Leaving and returning to the Swarm tab resets focus to the agent list.
        model.select_tab(0);
        model.select_tab(2);
        assert_eq!(model.swarm_focus, SwarmFocus::List);
    }

    #[test]
    fn test_controller_type_backspace_and_submit() {
        let mut model = Model::default();
        model.controller_type_char('h');
        model.controller_type_char('i');
        assert_eq!(model.controller_input, "hi");

        model.controller_backspace();
        assert_eq!(model.controller_input, "h");

        model.controller_type_char('i');
        let sent = model.controller_submit();
        assert_eq!(sent, Some("hi".to_string()));
        assert_eq!(model.controller_input, "");
        assert_eq!(model.controller_log.len(), 1);
        assert_eq!(model.controller_log[0].role, ControllerRole::User);
        assert_eq!(model.controller_log[0].text, "hi");

        // Submitting empty/whitespace-only input sends nothing and logs nothing.
        model.controller_type_char(' ');
        assert_eq!(model.controller_submit(), None);
        assert_eq!(model.controller_log.len(), 1);
    }

    #[test]
    fn test_controller_connection_status_mirrors_the_shared_acp_pool() {
        let mut model = Model::default();
        // Nobody has connected "acp-claude" yet.
        assert_eq!(model.controller_connection_status(), AcpStatus::Offline);

        // The Controller pane, the ACP Servers tab, and every Swarm pane run for this agent
        // all fold into the exact same acp_servers record -- there is only one status.
        model.apply_acp_update(AgentUpdate {
            server_id: CONTROLLER_SPEC_ID,
            outcome: AgentOutcome::Connected(crate::acp::HandshakeInfo {
                protocol_version: 1,
                agent_name: Some("Claude Code".to_string()),
                agent_version: None,
                capabilities: vec![],
                auth_methods: vec![],
                ping_ms: 10,
            }),
        });
        assert_eq!(model.controller_connection_status(), AcpStatus::Online);

        model.apply_acp_update(AgentUpdate {
            server_id: CONTROLLER_SPEC_ID,
            outcome: AgentOutcome::Failed(crate::acp::AcpError::Timeout),
        });
        assert_eq!(model.controller_connection_status(), AcpStatus::Error);
    }

    #[test]
    fn test_push_controller_reply_logs_agent_or_system_role() {
        let mut model = Model::default();

        model.push_controller_reply(Ok("hi there".to_string()));
        assert_eq!(model.controller_log.len(), 1);
        assert_eq!(model.controller_log[0].role, ControllerRole::Agent);
        assert_eq!(model.controller_log[0].text, "hi there");

        model.push_controller_reply(Err("boom".to_string()));
        assert_eq!(model.controller_log.len(), 2);
        assert_eq!(model.controller_log[1].role, ControllerRole::System);
    }

    #[test]
    fn test_directive_to_known_pane_is_approved_and_counted() {
        let mut model = Model::default();
        let target = model.swarm_panes[1].name.clone();
        let source = model.swarm_panes[0].name.clone();
        let text = format!("Done here.\n@[{target}]: please pick this up next");

        let directives = model.apply_swarm_run_outcome(RunOutcome::new(source, Ok(text)));

        assert_eq!(
            directives,
            vec![(target, "please pick this up next".to_string())]
        );
        assert_eq!(model.auto_routed_turns, 1);
    }

    #[test]
    fn test_directive_to_unknown_pane_is_rejected_and_logged() {
        let mut model = Model::default();
        let source = model.swarm_panes[0].name.clone();
        let text = "@[Nonexistent Pane]: do the thing".to_string();

        let directives = model.apply_swarm_run_outcome(RunOutcome::new(source, Ok(text)));

        assert!(directives.is_empty());
        assert_eq!(model.auto_routed_turns, 0);
        assert!(
            model
                .activity_log
                .iter()
                .any(|l| l.contains("unknown agent") && l.contains("Nonexistent Pane"))
        );
    }

    #[test]
    fn test_auto_route_budget_caps_total_dispatches() {
        let mut model = Model::default();
        let target = model.swarm_panes[1].name.clone();
        let source = model.swarm_panes[0].name.clone();
        let cap = model.limits.turn_cap;
        model.auto_routed_turns = cap;

        let directives = model.apply_swarm_run_outcome(RunOutcome::new(
            source,
            Ok(format!("@[{target}]: one more hop")),
        ));

        assert!(
            directives.is_empty(),
            "budget-exhausted directive must not be dispatched"
        );
        assert_eq!(model.auto_routed_turns, cap);
        assert!(
            model
                .activity_log
                .iter()
                .any(|l| l.contains("budget exhausted"))
        );
    }

    #[test]
    fn test_directive_creates_pending_task_and_completion_marks_it_done() {
        let mut model = Model::default();
        let target = model.swarm_panes[1].name.clone();
        let source = model.swarm_panes[0].name.clone();

        model.apply_swarm_run_outcome(RunOutcome::new(
            source.clone(),
            Ok(format!("@[{target}]: please pick this up")),
        ));

        assert_eq!(model.task_counts(), (1, 0, 0));
        assert_eq!(model.tasks[0].source, source);
        assert_eq!(model.tasks[0].target, target);
        assert_eq!(model.tasks[0].status, TaskStatus::Pending);

        // The target pane's own turn finishing (successfully) is what completes the task --
        // not anything the agent says.
        model.apply_swarm_run_outcome(RunOutcome::new(target, Ok("done, thanks".to_string())));
        assert_eq!(model.task_counts(), (0, 1, 0));
    }

    #[test]
    fn test_turn_events_update_live_state_while_running() {
        use crate::acp::TurnEvent;
        let mut model = Model::default();
        let name = model.swarm_panes[0].name.clone();
        model.begin_run_pane(0);
        assert_eq!(model.swarm_panes[0].live.phase, TurnPhase::Waiting);

        model.apply_turn_event(&name, TurnEvent::Thought("plan A\n".into()));
        assert_eq!(model.swarm_panes[0].live.phase, TurnPhase::Thinking);
        assert_eq!(model.swarm_panes[0].live.thought, "plan A\n");

        model.apply_turn_event(&name, TurnEvent::ToolStarted("Read file".into()));
        assert_eq!(model.swarm_panes[0].live.phase, TurnPhase::Tool);
        assert_eq!(model.swarm_panes[0].live.tool.as_deref(), Some("Read file"));

        model.apply_turn_event(
            &name,
            TurnEvent::ToolUpdate {
                title: None,
                done: true,
            },
        );
        assert_eq!(model.swarm_panes[0].live.tools_done, 1);
        assert!(model.swarm_panes[0].live.tool.is_none());

        model.apply_turn_event(&name, TurnEvent::Message("hel".into()));
        model.apply_turn_event(&name, TurnEvent::Message("lo".into()));
        assert_eq!(model.swarm_panes[0].live.phase, TurnPhase::Writing);
        assert_eq!(model.swarm_panes[0].output, "hello");
        assert_eq!(model.swarm_panes[0].status, SwarmRunStatus::Running);
    }

    #[test]
    fn test_finished_turn_keeps_thinking_and_goes_idle() {
        use crate::acp::TurnEvent;
        let mut model = Model::default();
        let name = model.swarm_panes[0].name.clone();
        model.begin_run_pane(0);
        model.apply_turn_event(&name, TurnEvent::Thought("deep thought".into()));
        model.apply_swarm_run_outcome(RunOutcome::new(name.clone(), Ok("final".into())));
        let pane = &model.swarm_panes[0];
        assert_eq!(pane.status, SwarmRunStatus::Done);
        assert_eq!(pane.live.phase, TurnPhase::Idle);
        assert_eq!(pane.live.thought, "deep thought");
        assert_eq!(pane.output, "final");
        // Late events after the turn ended must not flip anything back.
        model.apply_turn_event(&name, TurnEvent::Message("late".into()));
        assert_eq!(model.swarm_panes[0].output, "final");
    }

    #[test]
    fn test_thinking_text_is_capped() {
        use crate::acp::TurnEvent;
        let mut model = Model::default();
        let name = model.swarm_panes[0].name.clone();
        model.begin_run_pane(0);
        model.apply_turn_event(&name, TurnEvent::Thought("x".repeat(MAX_THOUGHT_CHARS)));
        model.apply_turn_event(&name, TurnEvent::Thought("END".into()));
        let t = &model.swarm_panes[0].live.thought;
        assert_eq!(t.chars().count(), MAX_THOUGHT_CHARS);
        assert!(t.ends_with("END"));
    }

    #[test]
    fn test_permission_detail_is_redacted() {
        let mut model = Model::default();
        let (tx, _rx) = std::sync::mpsc::channel();
        model.set_permission_modal(crate::acp::manager::PermissionPrompt {
            id: 1,
            agent_pane: "A".into(),
            tool_title: "run".into(),
            detail: "curl -H 'Authorization: Bearer abc.def.ghi123' https://x".into(),
            expires_at: std::time::Instant::now() + std::time::Duration::from_secs(30),
            reply_to: tx,
        });
        let detail = &model.active_permission.as_ref().unwrap().detail;
        assert!(detail.contains("curl"));
        assert!(!detail.contains("abc.def.ghi123"), "{detail}");
    }

    #[test]
    fn test_controller_scroll_moves_and_resets_on_submit() {
        let mut model = Model::default();
        model.controller_scroll_up(10);
        model.controller_scroll_up(5);
        assert_eq!(model.controller_scroll_back, 15);
        model.controller_scroll_down(100);
        assert_eq!(model.controller_scroll_back, 0);
        model.controller_scroll_top();
        model.controller_scroll_up(1); // must not overflow
        assert_eq!(model.controller_scroll_back, usize::MAX);
        model.controller_input = "hello".to_string();
        model.controller_submit();
        assert_eq!(model.controller_scroll_back, 0);
    }

    #[test]
    fn test_scroll_back_moves_and_resets_on_new_turn() {
        let mut model = Model::default();
        model.scroll_pane_up(10);
        model.scroll_pane_up(5);
        assert_eq!(model.swarm_panes[0].scroll_back, 15);
        model.scroll_pane_down(20);
        assert_eq!(model.swarm_panes[0].scroll_back, 0);
        model.scroll_pane_top();
        assert_eq!(model.swarm_panes[0].scroll_back, usize::MAX);
        model.scroll_pane_up(1); // must not overflow
        model.begin_run_pane(0);
        assert_eq!(model.swarm_panes[0].scroll_back, 0);
    }

    fn two_panes(model: &Model) -> (String, String) {
        (
            model.swarm_panes[0].name.clone(),
            model.swarm_panes[1].name.clone(),
        )
    }

    #[test]
    fn test_same_message_twice_is_sent_once() {
        let mut model = Model::default();
        let (a, b) = two_panes(&model);
        let text = format!("@[{b}]: review the diff");
        let first = model.apply_swarm_run_outcome(RunOutcome::new(a.clone(), Ok(text.clone())));
        let second = model.apply_swarm_run_outcome(RunOutcome::new(a, Ok(text)));
        assert_eq!(first.len(), 1);
        assert!(second.is_empty(), "duplicate must be suppressed");
        assert_eq!(
            model.auto_routed_turns, 1,
            "duplicate must not use the turn cap"
        );
        assert_eq!(
            model.task_counts(),
            (1, 0, 0),
            "duplicate must not add a task"
        );
        assert!(
            model
                .activity_log
                .iter()
                .any(|l| l.contains("duplicate message"))
        );
    }

    #[test]
    fn test_different_text_or_target_is_not_a_duplicate() {
        let mut model = Model::default();
        let (a, b) = two_panes(&model);
        let c = model.swarm_panes[2].name.clone();
        let out = model.apply_swarm_run_outcome(RunOutcome::new(
            a,
            Ok(format!("@[{b}]: one\n@[{b}]: two\n@[{c}]: one")),
        ));
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn test_failed_message_can_be_sent_again() {
        let mut model = Model::default();
        let (a, b) = two_panes(&model);
        let text = format!("@[{b}]: do the thing");
        model.apply_swarm_run_outcome(RunOutcome::new(a.clone(), Ok(text.clone())));
        model.apply_swarm_run_outcome(RunOutcome::new(b, Err("agent crashed".into())));
        let again = model.apply_swarm_run_outcome(RunOutcome::new(a, Ok(text)));
        assert_eq!(again.len(), 1, "a failed message must be retryable");
    }

    #[test]
    fn test_message_ids_survive_restart_and_expire() {
        let dir = std::env::temp_dir().join(format!("locust-idem-{}", std::process::id()));
        let path = dir.join("messages.json");
        let _ = std::fs::remove_dir_all(&dir);

        let mut model = Model::default();
        model.enable_message_store(path.clone());
        let (a, b) = two_panes(&model);
        let text = format!("@[{b}]: ship it");
        assert_eq!(
            model
                .apply_swarm_run_outcome(RunOutcome::new(a.clone(), Ok(text.clone())))
                .len(),
            1
        );

        // A new process (new Model) loading the same store must not resend.
        let mut restarted = Model::default();
        restarted.enable_message_store(path.clone());
        assert!(
            restarted
                .apply_swarm_run_outcome(RunOutcome::new(a.clone(), Ok(text.clone())))
                .is_empty()
        );

        // An entry older than the window is dropped on load, so the message may be sent again.
        let id = Model::message_id(&a, &b, "ship it");
        let old = std::collections::HashMap::from([(format!("{id:016x}"), 1u64)]);
        std::fs::write(&path, serde_json::to_string(&old).unwrap()).unwrap();
        let mut later = Model::default();
        later.enable_message_store(path);
        assert_eq!(
            later
                .apply_swarm_run_outcome(RunOutcome::new(a, Ok(text)))
                .len(),
            1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_project_modal_navigation() {
        let mut model = Model::default();
        model.open_project_modal("C:/cur".into(), vec!["C:/a".into(), "C:/b".into()]);
        model.project_modal_move(true);
        assert_eq!(model.project_modal.as_ref().unwrap().input, "C:/a");
        model.project_modal_move(true);
        model.project_modal_move(true); // clamps at the last entry
        assert_eq!(model.project_modal.as_ref().unwrap().input, "C:/b");
        model.project_modal_type('x');
        assert_eq!(model.project_modal.as_ref().unwrap().selected, None);
        model.project_modal_backspace();
        assert_eq!(model.project_modal.as_ref().unwrap().input, "C:/b");
    }

    #[test]
    fn test_failed_turn_marks_dispatched_task_failed() {
        let mut model = Model::default();
        let target = model.swarm_panes[1].name.clone();
        let source = model.swarm_panes[0].name.clone();

        model.apply_swarm_run_outcome(RunOutcome::new(
            source,
            Ok(format!("@[{target}]: please pick this up")),
        ));
        model.apply_swarm_run_outcome(RunOutcome::new(target, Err("agent crashed".to_string())));

        assert_eq!(model.task_counts(), (0, 0, 1));
    }

    #[test]
    fn test_unaddressed_pane_run_does_not_create_or_complete_a_task() {
        let mut model = Model::default();
        let pane = model.swarm_panes[0].name.clone();

        model.apply_swarm_run_outcome(RunOutcome::new(
            pane,
            Ok("just a normal startup run, no directive".to_string()),
        ));

        assert!(model.tasks.is_empty());
    }

    #[test]
    fn test_controller_reply_directive_is_approved_too() {
        let mut model = Model::default();
        let target = model.swarm_panes[0].name.clone();

        let directives = model.push_controller_reply(Ok(format!("@[{target}]: start on this")));

        assert_eq!(directives, vec![(target, "start on this".to_string())]);
        assert_eq!(model.auto_routed_turns, 1);
    }

    #[test]
    fn test_begin_run_selected_swarm_pane_marks_running() {
        let mut model = Model::default();
        model.select_tab(2);

        let assignment = model
            .begin_run_selected_swarm_pane()
            .expect("a pane is selected");
        assert_eq!(
            model.selected_swarm_pane().unwrap().status,
            SwarmRunStatus::Running
        );
        assert_eq!(assignment.pane, model.selected_swarm_pane().unwrap().name);
    }

    #[test]
    fn test_apply_swarm_run_outcome_updates_matching_pane() {
        let mut model = Model::default();
        let pane_name = model.swarm_panes[0].name.clone();
        model.swarm_panes[0].status = SwarmRunStatus::Running;

        model.apply_swarm_run_outcome(crate::swarm::RunOutcome::new(
            pane_name.clone(),
            Ok("done talking".to_string()),
        ));

        let pane = model
            .swarm_panes
            .iter()
            .find(|p| p.name == pane_name)
            .unwrap();
        assert_eq!(pane.status, SwarmRunStatus::Done);
        assert_eq!(pane.output, "done talking");
    }

    #[test]
    fn test_apply_swarm_run_outcome_records_usage_telemetry() {
        let mut model = Model::default();
        let pane_name = model.swarm_panes[0].name.clone();

        let usage = crate::acp::UsageInfo {
            tokens_used: Some(1500),
            context_size: Some(128000),
            cost_amount: Some(0.02),
            cost_currency: Some("USD".to_string()),
            stop_reason: Some("end_turn".to_string()),
        };

        model.apply_swarm_run_outcome(
            RunOutcome::new(pane_name.clone(), Ok("all done".to_string())).with_usage(Some(usage)),
        );

        let pane = model
            .swarm_panes
            .iter()
            .find(|p| p.name == pane_name)
            .unwrap();
        assert_eq!(pane.tokens_used, Some(1500));
        assert_eq!(pane.context_size, Some(128000));
        assert_eq!(pane.tokens_left(), Some(126500));
        assert_eq!(
            pane.percent_left().map(|p| (p * 10.0).round() / 10.0),
            Some(98.8)
        );
        assert_eq!(pane.cost, Some((0.02, "USD".to_string())));
        assert!(model.acp_tokens_used() >= 1500);
        assert_eq!(model.acp_tokens_left(), Some(126500));
    }

    #[test]
    fn test_acp_server_tokens_left_and_percentage() {
        let mut server = AcpServer::from_spec(&crate::acp::AGENT_SPECS[0]);
        assert_eq!(server.tokens_left(), None);
        assert_eq!(server.percent_left(), None);

        server.tokens_used = Some(5000);
        server.context_size = Some(200_000);
        assert_eq!(server.tokens_left(), Some(195_000));
        assert_eq!(server.percent_left(), Some(97.5));
    }

    #[test]
    fn test_broadcast_to_all_panes() {
        let mut model = Model::default();
        let source = model.swarm_panes[0].name.clone();
        let directives = model.apply_swarm_run_outcome(RunOutcome::new(
            source.clone(),
            Ok("@[*]: alert everyone".to_string()),
        ));

        // Must dispatch to all panes except source
        assert_eq!(directives.len(), model.swarm_panes.len() - 1);
        assert!(!directives.iter().any(|(target, _)| target == &source));
        for (target, msg) in directives {
            assert_eq!(msg, "alert everyone");
            assert!(model.swarm_panes.iter().any(|p| p.name == target));
        }
    }

    #[test]
    fn test_broadcast_to_tab() {
        let mut model = Model::default();
        let tab = model.swarm_panes[0].tab.clone();
        let tab_pane_count = model.swarm_panes.iter().filter(|p| p.tab == tab).count();

        let directives = model.push_controller_reply(Ok(format!("@[{tab}]: tab notice")));
        assert_eq!(directives.len(), tab_pane_count);
        for (target, msg) in directives {
            assert_eq!(msg, "tab notice");
            assert!(
                model
                    .swarm_panes
                    .iter()
                    .any(|p| p.name == target && p.tab == tab)
            );
        }
    }

    #[test]
    fn test_reply_loop_between_two_agents_stops_at_turn_cap() {
        let mut model = Model::default();
        model.limits.turn_cap = 10;
        let agent_a = model.swarm_panes[0].name.clone();
        let agent_b = model.swarm_panes[1].name.clone();

        let mut current_speaker = agent_a.clone();
        let mut loop_count = 0;

        loop {
            let next_target = if current_speaker == agent_a {
                &agent_b
            } else {
                &agent_a
            };
            // Each message differs, so only the turn cap can stop this loop.
            let text = format!("@[{next_target}]: ping pong {loop_count}");
            let directives =
                model.apply_swarm_run_outcome(RunOutcome::new(current_speaker.clone(), Ok(text)));
            if directives.is_empty() {
                break;
            }
            loop_count += 1;
            current_speaker = next_target.clone();
            assert!(loop_count <= 20, "loop runaway guard");
        }

        assert_eq!(model.auto_routed_turns, 10);
        assert_eq!(loop_count, 10);
    }

    #[test]
    fn test_identical_ping_pong_is_stopped_by_idempotency() {
        let mut model = Model::default();
        let a = model.swarm_panes[0].name.clone();
        let b = model.swarm_panes[1].name.clone();
        let mut speaker = a.clone();
        let mut turns = 0;
        loop {
            let target = if speaker == a { &b } else { &a };
            let text = format!("@[{target}]: ping pong");
            let out = model.apply_swarm_run_outcome(RunOutcome::new(speaker.clone(), Ok(text)));
            if out.is_empty() {
                break;
            }
            turns += 1;
            speaker = target.clone();
            assert!(turns < 10, "must stop on its own");
        }
        // a->b and b->a are different ids, then a->b repeats and is suppressed.
        assert_eq!(turns, 2);
    }

    #[test]
    fn test_token_limit_blocks_new_directives() {
        let mut model = Model::default();
        model.limits.token_limit = Some(1000);
        model.telemetry.total_tokens = 1500;
        let target = model.swarm_panes[1].name.clone();
        let source = model.swarm_panes[0].name.clone();

        let directives = model
            .apply_swarm_run_outcome(RunOutcome::new(source, Ok(format!("@[{target}]: do work"))));

        assert!(directives.is_empty());
        assert!(
            model
                .activity_log
                .iter()
                .any(|l| l.contains("token limit (1000) reached"))
        );
    }

    #[test]
    fn test_export_transcripts_and_redacts_secrets() {
        let mut model = Model::default();
        let pane_name = model.swarm_panes[0].name.clone();
        model.swarm_panes[0].output =
            "Sensitive key: AKIA1234567890ABCDEF, Bearer secrettoken123".to_string();
        model.controller_log.push(ControllerMessage {
            role: ControllerRole::User,
            text: "User secret: xoxb-1234567890-abcdef".to_string(),
        });

        let temp_dir =
            std::env::temp_dir().join(format!("locust_test_export_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let export_dir = model
            .export_transcripts(Some(&temp_dir))
            .expect("export should succeed");
        assert!(export_dir.exists());

        // Check pane markdown
        let pane_file = export_dir.join(format!("{}.md", sanitize_filename(&pane_name)));
        assert!(pane_file.exists());
        let pane_content = std::fs::read_to_string(pane_file).unwrap();
        assert!(!pane_content.contains("AKIA1234567890ABCDEF"));
        assert!(!pane_content.contains("secrettoken123"));
        assert!(pane_content.contains("[REDACTED"));

        // Check Controller markdown
        let ctl_file = export_dir.join("Controller.md");
        assert!(ctl_file.exists());
        let ctl_content = std::fs::read_to_string(ctl_file).unwrap();
        assert!(!ctl_content.contains("xoxb-1234567890-abcdef"));
        assert!(ctl_content.contains("[REDACTED"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_cancel_running_pane() {
        let mut model = Model::default();
        let target = model.swarm_panes[0].name.clone();
        model.swarm_panes[0].status = SwarmRunStatus::Running;
        model.create_task("Controller", &target, "some task");

        let cancelled = model.cancel_swarm_pane(0);
        assert_eq!(cancelled, Some(target.clone()));
        assert_eq!(model.swarm_panes[0].status, SwarmRunStatus::Error);
        assert_eq!(model.task_counts(), (0, 0, 1));
        assert!(
            model
                .activity_log
                .iter()
                .any(|l| l.contains("cancelled by operator"))
        );
    }

    #[test]
    fn test_permission_modal_timeout() {
        let mut model = Model::default();
        let (tx, rx) = std::sync::mpsc::channel();
        let prompt = crate::acp::manager::PermissionPrompt {
            id: 100,
            agent_pane: "Test Agent".to_string(),
            tool_title: "dangerous_tool".to_string(),
            detail: String::new(),
            expires_at: std::time::Instant::now() - std::time::Duration::from_secs(1),
            reply_to: tx,
        };
        model.set_permission_modal(prompt);
        assert!(model.active_permission.is_some());

        assert!(model.check_permission_timeout());
        assert!(model.active_permission.is_none());

        let decision = rx.recv().expect("must send deny decision on timeout");
        assert_eq!(decision, crate::acp::client::PermissionDecision::Deny);
    }
}
