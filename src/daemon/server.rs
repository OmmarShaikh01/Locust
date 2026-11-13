use crate::acp::{self, Manager, TurnEvent, UsageInfo};
use crate::core::state::Model;
use crate::ipc::framing::{read_frame, write_frame};
use crate::ipc::protocol::{ClientCommand, ServerEvent};
use crate::ipc::socket::{create_listener, pipe_name_for_project, Listener};
use crate::logging::{self, Level};
use crate::project::Project;
use crate::swarm::RunOutcome;
use interprocess::local_socket::Stream as LocalStream;
use interprocess::TryClone;
use std::io;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

type SwarmRunItem = (
    String,
    Receiver<Result<(String, Option<UsageInfo>), String>>,
);
type ControllerTurnItem = Receiver<Result<(String, Option<UsageInfo>), String>>;

pub struct DaemonServer {
    pub model: Model,
    manager: Manager,
    project: Project,
    pipe_name: String,
    swarm_runs: Vec<SwarmRunItem>,
    swarm_progress: Vec<(String, Receiver<TurnEvent>)>,
    controller_turns: Vec<ControllerTurnItem>,
    controller_briefed: bool,
    client_tx: Option<Sender<ServerEvent>>,
    cmd_rx: Receiver<ClientCommand>,
    cmd_tx: Sender<ClientCommand>,
    new_client_rx: Receiver<LocalStream>,
    last_broadcast: Instant,
    last_heartbeat: Option<Instant>,
    shutdown_requested: bool,
}

impl DaemonServer {
    pub fn new(project: Project) -> io::Result<Self> {
        let pipe_name = pipe_name_for_project(&project.root);
        let listener = create_listener(&pipe_name)?;

        let (new_client_tx, new_client_rx) = mpsc::channel();
        let pipe_clone = pipe_name.clone();

        thread::spawn(move || {
            logging::log(
                Level::Info,
                &format!("IPC listener started on {pipe_clone}"),
            );
            while let Ok(stream) = listener.accept() {
                if new_client_tx.send(stream).is_err() {
                    break;
                }
            }
        });

        let (cmd_tx, cmd_rx) = mpsc::channel();

        let mut server = Self {
            model: Model::default(),
            manager: Manager::default(),
            project,
            pipe_name,
            swarm_runs: Vec::new(),
            swarm_progress: Vec::new(),
            controller_turns: Vec::new(),
            controller_briefed: false,
            client_tx: None,
            cmd_rx,
            cmd_tx,
            new_client_rx,
            last_broadcast: Instant::now(),
            last_heartbeat: None,
            shutdown_requested: false,
        };

        server.boot();
        Ok(server)
    }

    pub fn pipe_name(&self) -> &str {
        &self.pipe_name
    }

    pub fn project_path(&self) -> String {
        self.project.root.display().to_string()
    }

    fn boot(&mut self) {
        self.manager
            .set_turn_limits(crate::acp::client::TurnLimits::from_secs(
                self.model.limits.timeout_secs,
                self.model.limits.max_turn_secs,
            ));
        self.model
            .enable_message_store(std::path::PathBuf::from(".state").join("messages.json"));
        self.model.begin_connect_all();
        self.manager.probe_all();

        for idx in 0..self.model.swarm_panes.len() {
            self.start_run(idx);
        }
    }

    pub fn run_loop(&mut self) {
        while !self.shutdown_requested {
            self.tick();
            thread::sleep(Duration::from_millis(16));
        }
        logging::log(Level::Info, "Daemon server shutting down gracefully.");
    }

    pub fn tick(&mut self) {
        self.check_new_clients();
        self.process_client_commands();
        self.drain_manager_updates();
        self.poll_swarm_progress();
        self.poll_swarm_runs();
        self.poll_controller_turns();
        self.check_permission_timeouts();
        self.check_heartbeat();
    }

    fn check_new_clients(&mut self) {
        if let Ok(stream) = self.new_client_rx.try_recv() {
            logging::log(Level::Info, "New GUI client connected to IPC server.");

            let mut write_stream = match stream.try_clone() {
                Ok(s) => s,
                Err(e) => {
                    logging::log(Level::Error, &format!("Failed to clone stream: {e}"));
                    return;
                }
            };
            let mut read_stream = stream;

            let (event_tx, event_rx) = mpsc::channel::<ServerEvent>();

            let snap = self.model.snapshot(&self.project_path());
            let initial = ServerEvent::InitialSync {
                state: Box::new(snap),
                server_version: "0.1.0".to_string(),
            };
            let _ = event_tx.send(initial);

            thread::spawn(move || {
                while let Ok(event) = event_rx.recv() {
                    if let Ok(bytes) = serde_json::to_vec(&event) {
                        if write_frame(&mut write_stream, &bytes).is_err() {
                            break;
                        }
                    }
                }
            });

            let cmd_tx = self.cmd_tx.clone();
            thread::spawn(move || {
                while let Ok(bytes) = read_frame(&mut read_stream) {
                    if let Ok(cmd) = serde_json::from_slice::<ClientCommand>(&bytes) {
                        if cmd_tx.send(cmd).is_err() {
                            break;
                        }
                    }
                }
                logging::log(Level::Info, "Client command stream closed.");
            });

            self.client_tx = Some(event_tx);
        }
    }

    fn broadcast_event(&mut self, event: ServerEvent) {
        if let Some(tx) = &self.client_tx {
            if tx.send(event).is_err() {
                self.client_tx = None;
                logging::log(
                    Level::Info,
                    "GUI client disconnected; continuing background execution.",
                );
            }
        }
    }

    fn broadcast_state_updated(&mut self) {
        let snap = self.model.snapshot(&self.project_path());
        self.broadcast_event(ServerEvent::StateUpdated(Box::new(snap)));
    }

    fn process_client_commands(&mut self) {
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            match cmd {
                ClientCommand::Hello { client_version } => {
                    logging::log(
                        Level::Info,
                        &format!("Client handshake: version {client_version}"),
                    );
                    let snap = self.model.snapshot(&self.project_path());
                    self.broadcast_event(ServerEvent::InitialSync {
                        state: Box::new(snap),
                        server_version: "0.1.0".into(),
                    });
                }
                ClientCommand::SubmitControllerTurn { text } => {
                    self.submit_controller_turn(&text);
                }
                ClientCommand::DispatchTask {
                    target_pane,
                    prompt,
                } => {
                    self.start_directed_turn("operator", target_pane, prompt);
                }
                ClientCommand::CancelTurn { pane_name } => {
                    if let Some(idx) = self
                        .model
                        .swarm_panes
                        .iter()
                        .position(|p| p.name == pane_name)
                    {
                        self.model.cancel_swarm_pane(idx);
                        self.swarm_runs.retain(|(name, _)| name != &pane_name);
                        self.swarm_progress.retain(|(name, _)| name != &pane_name);
                        self.broadcast_state_updated();
                    }
                }
                ClientCommand::RetryTurn { pane_name } => {
                    if let Some(idx) = self
                        .model
                        .swarm_panes
                        .iter()
                        .position(|p| p.name == pane_name)
                    {
                        self.start_run(idx);
                        self.broadcast_state_updated();
                    }
                }
                ClientCommand::ResolvePermission {
                    request_id: _,
                    allowed,
                } => {
                    self.resolve_permission(allowed);
                }
                ClientCommand::SwitchTab { tab_index } => {
                    self.model.select_tab(tab_index);
                    self.broadcast_state_updated();
                }
                ClientCommand::SelectPane { pane_index } => {
                    self.model.swarm_selected_pane = pane_index;
                    self.broadcast_state_updated();
                }
                ClientCommand::SelectAcpServer { server_index } => {
                    self.model.acp_selected_server = server_index;
                    self.broadcast_state_updated();
                }
                ClientCommand::ToggleThinking => {
                    self.model.show_thinking = !self.model.show_thinking;
                    self.broadcast_state_updated();
                }
                ClientCommand::SwitchProject { path: _ } => {
                    self.broadcast_event(ServerEvent::Alert {
                        title: "Switch Project".into(),
                        message: "Project switching handled via daemon restart.".into(),
                    });
                }
                ClientCommand::QueryLogs { limit, min_level } => {
                    let level = match min_level.as_deref() {
                        Some("Error") => Some(Level::Error),
                        Some("Warn") => Some(Level::Warn),
                        Some("Info") => Some(Level::Info),
                        _ => None,
                    };
                    for entry in logging::recent_logs(limit, level) {
                        self.broadcast_event(ServerEvent::LogEmitted(entry));
                    }
                }
                ClientCommand::Shutdown => {
                    self.shutdown_requested = true;
                }
            }
        }
    }

    fn submit_controller_turn(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.model.controller_input = text.to_string();
        let Some(text) = self.model.controller_submit() else {
            return;
        };

        let agent_from_roster = self
            .model
            .swarm_panes
            .iter()
            .find(|p| {
                let lower = p.name.to_lowercase();
                lower.contains("orchestrator") || lower.contains("controller")
            })
            .map(|p| p.assignment.agent.clone());

        let spec = agent_from_roster
            .as_deref()
            .and_then(acp::spec_for_agent_id)
            .or_else(|| acp::spec_for_agent_id(acp::CONTROLLER_SPEC_ID))
            .or_else(|| acp::AGENT_SPECS.first());

        let Some(spec) = spec else {
            self.model
                .push_controller_reply(Err("no controller agent registered".to_string()));
            self.broadcast_state_updated();
            return;
        };

        let mut prompt = text.to_string();
        if !self.controller_briefed {
            prompt.push_str(&self.addressing_instructions("__controller__"));
            self.controller_briefed = true;
        }

        let rx = self.manager.turn(
            *spec,
            acp::CONTROLLER_SESSION_KEY.to_string(),
            prompt,
        );
        self.controller_turns.push(rx);
        self.broadcast_state_updated();
    }

    fn resolve_permission(&mut self, allowed: bool) {
        if let Some(perm) = self.model.active_permission.take() {
            let decision = if allowed {
                crate::acp::client::PermissionDecision::Allow
            } else {
                crate::acp::client::PermissionDecision::Deny
            };
            let _ = perm.reply_to.send(decision);
            let action = if allowed { "granted" } else { "denied" };
            self.model.push_activity(
                &perm.agent_pane,
                format!("Tool permission {action} by operator"),
            );
            self.broadcast_state_updated();
        }
    }

    fn check_permission_timeouts(&mut self) {
        self.model.check_permission_timeout();
    }

    fn check_heartbeat(&mut self) {
        let now = Instant::now();
        let due = self
            .last_heartbeat
            .map(|t| now.duration_since(t) >= Duration::from_secs(1))
            .unwrap_or(true);
        if due {
            self.last_heartbeat = Some(now);
            self.model.refresh_telemetry();
            if self.last_broadcast.elapsed() >= Duration::from_millis(500) {
                self.last_broadcast = now;
                self.broadcast_state_updated();
            }
        }
    }

    fn drain_manager_updates(&mut self) {
        let mut changed = false;
        for update in self.manager.poll() {
            self.model.apply_acp_update(update);
            changed = true;
        }
        for prompt in self.manager.poll_permissions() {
            self.model.set_permission_modal(prompt);
            changed = true;
        }
        if changed {
            self.broadcast_state_updated();
        }
    }

    fn start_run(&mut self, idx: usize) {
        let Some(assignment) = self.model.begin_run_pane(idx) else {
            return;
        };
        let Some(spec) = acp::spec_for_agent_id(&assignment.agent) else {
            self.model.apply_swarm_run_outcome(RunOutcome::new(
                assignment.pane.clone(),
                Err(format!("unknown agent type: {}", assignment.agent)),
            ));
            return;
        };

        let prompt = match &self.model.swarm_panes[idx].last_prompt {
            Some(p) => p.clone(),
            None => {
                let mut p = assignment.effective_task();
                if !assignment.skills.is_empty() {
                    p.push_str(&format!(
                        " (requested skills: {})",
                        assignment.skills.join(", ")
                    ));
                }
                p.push_str(&self.addressing_instructions(&assignment.pane));
                p
            }
        };
        self.model.swarm_panes[idx].last_prompt = Some(prompt.clone());

        let (rx, progress) = self
            .manager
            .turn_streaming(*spec, assignment.pane.clone(), prompt);
        self.swarm_runs.push((assignment.pane.clone(), rx));
        self.swarm_progress
            .retain(|(name, _)| name != &assignment.pane);
        self.swarm_progress
            .push((assignment.pane.clone(), progress));
    }

    fn poll_swarm_progress(&mut self) {
        let mut i = 0;
        while i < self.swarm_progress.len() {
            let mut closed = false;
            loop {
                match self.swarm_progress[i].1.try_recv() {
                    Ok(event) => {
                        let pane = self.swarm_progress[i].0.clone();
                        match &event {
                            TurnEvent::Thought(t) => {
                                self.broadcast_event(ServerEvent::TurnProgress {
                                    pane_name: pane.clone(),
                                    delta_type: "thinking".into(),
                                    content: t.clone(),
                                });
                            }
                            TurnEvent::Message(m) => {
                                self.broadcast_event(ServerEvent::TurnProgress {
                                    pane_name: pane.clone(),
                                    delta_type: "text".into(),
                                    content: m.clone(),
                                });
                            }
                            _ => {}
                        }
                        self.model.apply_turn_event(&pane, event);
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        closed = true;
                        break;
                    }
                }
            }
            if closed {
                self.swarm_progress.remove(i);
            } else {
                i += 1;
            }
        }
    }

    fn poll_swarm_runs(&mut self) {
        let mut i = 0;
        while i < self.swarm_runs.len() {
            match self.swarm_runs[i].1.try_recv() {
                Ok(output) => {
                    let (pane, _) = self.swarm_runs.remove(i);
                    let (res, usage) = match output {
                        Ok((text, usage)) => (Ok(text), usage),
                        Err(e) => (Err(e), None),
                    };
                    let directives = self.model.apply_swarm_run_outcome(
                        RunOutcome::new(pane.clone(), res).with_usage(usage),
                    );
                    for (target, message) in directives {
                        self.start_directed_turn(&pane, target, message);
                    }
                    self.broadcast_state_updated();
                }
                Err(mpsc::TryRecvError::Empty) => i += 1,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.swarm_runs.remove(i);
                }
            }
        }
    }

    fn poll_controller_turns(&mut self) {
        let mut i = 0;
        while i < self.controller_turns.len() {
            match self.controller_turns[i].try_recv() {
                Ok(output) => {
                    self.controller_turns.remove(i);
                    let (res, _) = match output {
                        Ok((text, usage)) => (Ok(text), usage),
                        Err(e) => (Err(e), None),
                    };
                    let directives = self.model.push_controller_reply(res);
                    for (target, message) in directives {
                        self.start_directed_turn("Controller", target, message);
                    }
                    self.broadcast_state_updated();
                }
                Err(mpsc::TryRecvError::Empty) => i += 1,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.controller_turns.remove(i);
                }
            }
        }
    }

    fn addressing_instructions(&self, self_pane: &str) -> String {
        let others: Vec<&str> = self
            .model
            .swarm_panes
            .iter()
            .map(|p| p.name.as_str())
            .filter(|name| *name != self_pane)
            .collect();
        if others.is_empty() {
            return String::new();
        }
        format!(
            "\n\nYou can hand off work or send a message to another agent in this swarm by \
             writing \"@[Exact Pane Name]: your message\" on its own line in your reply -- \
             Locust delivers it to that agent automatically as its next turn. Other agents in \
             this swarm: {}.",
            others.join(", ")
        )
    }

    fn start_directed_turn(&mut self, source: &str, target: String, message: String) {
        let Some(idx) = self.model.swarm_panes.iter().position(|p| p.name == target) else {
            return;
        };
        let assignment = self.model.swarm_panes[idx].assignment.clone();
        let Some(spec) = acp::spec_for_agent_id(&assignment.agent) else {
            self.model.apply_swarm_run_outcome(RunOutcome::new(
                target,
                Err(format!("unknown agent type: {}", assignment.agent)),
            ));
            return;
        };
        self.model.begin_run_pane(idx);
        let msg_id = Model::message_id(source, &target, &message);
        let prompt = format!("[Message from {source} | id {msg_id:016x}]: {message}");
        self.model.swarm_panes[idx].last_prompt = Some(prompt.clone());
        let (rx, progress) = self.manager.turn_streaming(*spec, target.clone(), prompt);
        self.swarm_runs.push((target.clone(), rx));
        self.swarm_progress.retain(|(name, _)| name != &target);
        self.swarm_progress.push((target, progress));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::socket::connect_stream;
    use crate::project::Project;

    #[test]
    fn test_daemon_server_boots_and_syncs_over_ipc() {
        let temp_dir = std::env::temp_dir().join(format!("locust_daemon_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let project = Project::open(&temp_dir).unwrap();

        let mut server = DaemonServer::new(project).unwrap();
        let pipe_name = server.pipe_name().to_string();

        let client_thread = thread::spawn(move || {
            let mut client = connect_stream(&pipe_name).unwrap();
            let frame = read_frame(&mut client).unwrap();
            let event: ServerEvent = serde_json::from_slice(&frame).unwrap();
            match event {
                ServerEvent::InitialSync { state, server_version } => {
                    assert_eq!(server_version, "0.1.0");
                    assert_eq!(state.selected_tab, 0);
                }
                _ => panic!("Expected InitialSync event"),
            }
        });

        for _ in 0..50 {
            server.tick();
            thread::sleep(Duration::from_millis(10));
        }

        client_thread.join().unwrap();
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
