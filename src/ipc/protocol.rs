use crate::logging::LogEntry;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "payload")]
pub enum ClientCommand {
    Hello {
        client_version: String,
    },
    SubmitControllerTurn {
        text: String,
    },
    DispatchTask {
        target_pane: String,
        prompt: String,
    },
    CancelTurn {
        pane_name: String,
    },
    RetryTurn {
        pane_name: String,
    },
    ResolvePermission {
        request_id: String,
        allowed: bool,
    },
    SwitchTab {
        tab_index: usize,
    },
    SelectPane {
        pane_index: usize,
    },
    SelectAcpServer {
        server_index: usize,
    },
    ToggleThinking,
    SwitchProject {
        path: String,
    },
    QueryLogs {
        limit: usize,
        min_level: Option<String>,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "payload")]
pub enum ServerEvent {
    InitialSync {
        state: Box<ModelSnapshot>,
        server_version: String,
    },
    StateUpdated(Box<ModelSnapshot>),
    TurnProgress {
        pane_name: String,
        delta_type: String, // "text", "thinking", "tool_call", "tool_result"
        content: String,
    },
    PermissionRequested {
        request_id: String,
        agent_id: String,
        tool: String,
        params: String,
        timeout_secs: u64,
    },
    LogEmitted(LogEntry),
    Alert {
        title: String,
        message: String,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AcpServerSnapshot {
    pub id: String,
    pub name: String,
    pub transport: String,
    pub command_line: String,
    pub status: String,
    pub protocol: String,
    pub ping_ms: u64,
    pub capabilities: Vec<String>,
    pub connected_at_secs: Option<u64>,
    pub recent_logs: Vec<String>,
    pub tokens_used: Option<u64>,
    pub context_size: Option<u64>,
    pub cost: Option<(f64, String)>,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SwarmPaneSnapshot {
    pub name: String,
    pub agent: String,
    pub model: String,
    pub task: String,
    pub status: String,
    pub output_lines: Vec<String>,
    pub thinking_lines: Vec<String>,
    pub turns_used: u32,
    pub tokens_used: u64,
    pub phase: String,
    pub elapsed_secs: u64,
    pub auto_route_budget: u32,
    pub unread_count: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ControllerMessageSnapshot {
    pub role: String, // "operator", "agent", "system"
    pub text: String,
    pub timestamp_secs: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TaskSnapshot {
    pub id: u64,
    pub from: String,
    pub to: String,
    pub prompt: String,
    pub status: String, // "pending", "done", "failed"
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PermissionSnapshot {
    pub request_id: String,
    pub agent_id: String,
    pub tool: String,
    pub params: String,
    pub seconds_left: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ModelSnapshot {
    pub project_path: String,
    pub selected_tab: usize,
    pub dashboard_frame: usize,
    pub acp_selected_server: usize,
    pub acp_servers: Vec<AcpServerSnapshot>,
    pub swarm_selected_pane: usize,
    pub swarm_panes: Vec<SwarmPaneSnapshot>,
    pub show_thinking: bool,
    pub controller_history: Vec<ControllerMessageSnapshot>,
    pub tasks: Vec<TaskSnapshot>,
    pub active_permission: Option<PermissionSnapshot>,
    pub activity_log: Vec<String>,
    pub total_tokens_used: u64,
    pub total_cost_usd: f64,
    pub total_invocations: u64,
    pub turn_cap: u32,
    pub timeout_secs: u64,
}
