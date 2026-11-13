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

/// Lifecycle state of a raised issue or feature request in the swarm issue tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum RequestState {
    #[default]
    Raised,
    InProgress,
    SelfReview,
    HumanReview,
    Passed,
    Rejected,
}

impl RequestState {
    pub fn as_str(&self) -> &'static str {
        match self {
            RequestState::Raised => "raised",
            RequestState::InProgress => "in_progress",
            RequestState::SelfReview => "self_review",
            RequestState::HumanReview => "human_review",
            RequestState::Passed => "passed",
            RequestState::Rejected => "rejected",
        }
    }

    /// Lenient parse that always succeeds (falls back to `Raised`), unlike `std::str::FromStr`.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "in_progress" | "in progress" | "implementing" => RequestState::InProgress,
            "self_review" | "self review" => RequestState::SelfReview,
            "human_review" | "human review" => RequestState::HumanReview,
            "passed" | "done" | "approved" => RequestState::Passed,
            "rejected" | "changes_requested" => RequestState::Rejected,
            _ => RequestState::Raised,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            RequestState::Raised => "RAISED",
            RequestState::InProgress => "IN PROGRESS",
            RequestState::SelfReview => "SELF REVIEW",
            RequestState::HumanReview => "HUMAN REVIEW",
            RequestState::Passed => "PASSED",
            RequestState::Rejected => "REJECTED",
        }
    }
}

/// A persistent swarm request or issue tracked from creation to verification and review.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct SwarmRequest {
    pub id: u64,
    pub title: String,
    pub target_agent: String,
    pub description: String,
    pub state: RequestState,
    pub created_at_secs: u64,
    pub updated_at_secs: u64,
    pub review_notes: Vec<String>,
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
    /// Persistent swarm requests / issues in the kanban tracker.
    pub requests: Vec<SwarmRequest>,
    /// Monotonic id source for `requests`.
    pub next_request_id: u64,
    /// Where `requests` is saved so state survives restart and resumes per project.
    request_store: Option<std::path::PathBuf>,
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
            requests: Vec::new(),
            next_request_id: 1,
            request_store: None,
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

        let requests = self
            .requests
            .iter()
            .map(|r| SwarmRequestSnapshot {
                id: r.id,
                title: r.title.clone(),
                target_agent: r.target_agent.clone(),
                description: r.description.clone(),
                state: r.state.as_str().to_string(),
                created_at_secs: r.created_at_secs,
                updated_at_secs: r.updated_at_secs,
                review_notes: r.review_notes.clone(),
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
            requests,
            active_permission,
            activity_log: self.activity_log.clone(),
            total_tokens_used: self.telemetry.total_tokens,
            total_cost_usd: self
                .telemetry
                .total_cost
                .as_ref()
                .map(|(c, _)| *c)
                .unwrap_or(0.0),
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
}

mod acp_events;
mod acp_servers;
mod controller;
mod directives;
mod export;
mod permissions;
mod project_modal;
mod requests;
mod swarm_panes;

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
mod tests;
