use crate::acp::{
    self, AGENT_SPECS, AgentSpec, CONTROLLER_SESSION_KEY, CONTROLLER_SPEC_ID, Manager,
};
use crate::app::model::Model;
use crate::app::view;
use crate::swarm::RunOutcome;
use crossterm::event::{self, KeyCode};
use ratatui::DefaultTerminal;
use std::io;
use std::path::Path;
use std::sync::mpsc::Receiver;

type SwarmRunItem = (
    String,
    Receiver<Result<(String, Option<crate::acp::UsageInfo>), String>>,
);
type ControllerTurnItem = Receiver<Result<(String, Option<crate::acp::UsageInfo>), String>>;

/// Drives the event loop: reads input, mutates the Model, drives real ACP connections, and
/// asks the View to render the result. Every real agent launch -- the ACP Servers tab, the
/// Controller chat, and every Swarm pane run -- goes through the one shared `Manager` pool
/// below, so there is never more than one spawned subprocess per agent.
#[derive(Default)]
pub struct Controller {
    pub model: Model,
    manager: Manager,
    /// In-flight Swarm pane runs: (pane name, its one-shot result).
    swarm_runs: Vec<SwarmRunItem>,
    /// Live event streams (thinking, text, tools) for the in-flight Swarm pane runs.
    swarm_progress: Vec<(String, Receiver<crate::acp::TurnEvent>)>,
    /// In-flight Controller chat turns.
    controller_turns: Vec<ControllerTurnItem>,
    /// Whether the Controller chat's session has already been taught the agent-to-agent
    /// addressing convention. Sent once -- the persistent session remembers it afterward.
    controller_briefed: bool,
    /// The project this swarm runs in, holding its one-swarm-per-project lock.
    project: Option<crate::project::Project>,
    last_heartbeat: Option<std::time::Instant>,
}

impl Controller {
    pub fn new() -> Self {
        Self::default()
    }

    /// A controller bound to an open project (and so holding that project's swarm lock).
    pub fn with_project(project: crate::project::Project) -> Self {
        Self {
            project: Some(project),
            ..Self::default()
        }
    }

    /// Connect every agent and start every pane. Runs at launch and after a project switch.
    fn boot(&mut self) {
        self.manager
            .set_turn_limits(crate::acp::client::TurnLimits::from_secs(
                self.model.limits.timeout_secs,
                self.model.limits.max_turn_secs,
            ));
        // Remember sent messages on disk, in this project, so a restart does not resend them.
        self.model
            .enable_message_store(std::path::PathBuf::from(".state").join("messages.json"));
        // Spawn, start, and always probe every registered agent up front, so the ACP
        // Servers tab, the Controller pane, and every Swarm pane are all immediately live
        // with real token/cost telemetry rather than waiting on an explicit connect.
        self.model.begin_connect_all();
        self.manager.probe_all();

        // Run every Swarm pane's task automatically -- continuing its session from the
        // last time Locust ran (saved to disk) if there is one, else starting fresh.
        // Nobody needs to press Enter for an agent to actually start working.
        for idx in 0..self.model.swarm_panes.len() {
            self.start_run(idx);
        }
    }

    /// Switch to another project. The new project's lock is taken first, so a refusal (the
    /// path is bad, or another Locust already runs a swarm there) leaves this swarm running.
    /// On success every agent process, chat and pending task of the old project is dropped:
    /// nothing carries over between projects.
    pub fn switch_project(&mut self, path: &str) -> Result<(), String> {
        let path = path.trim();
        if path.is_empty() {
            return Err("enter a project path".to_string());
        }
        let wanted =
            std::fs::canonicalize(path).map_err(|e| format!("cannot open \"{path}\": {e}"))?;
        if let Some(cur) = &self.project
            && std::fs::canonicalize(&cur.root).ok().as_ref() == Some(&wanted)
        {
            return Err("that project is already open".to_string());
        }
        let new = crate::project::Project::open(Path::new(path))?;
        // Past this point the switch cannot fail except for the working directory.
        std::env::set_current_dir(&new.root)
            .map_err(|e| format!("cannot enter {}: {e}", new.root.display()))?;
        crate::project::remember_project(&new.root);

        // Dropping the old manager closes every agent subprocess of the old project.
        self.manager = Manager::default();
        self.swarm_runs.clear();
        self.swarm_progress.clear();
        self.controller_turns.clear();
        self.controller_briefed = false;
        self.model = Model::default();
        // Replacing the project drops the old one, which releases its lock.
        self.project = Some(new);
        self.last_heartbeat = None;
        self.boot();
        let root = self
            .project
            .as_ref()
            .map(|p| p.root.display().to_string())
            .unwrap_or_default();
        self.model
            .push_activity("Locust", format!("switched to project {root}"));
        Ok(())
    }

    fn open_project_dialog(&mut self) {
        let current = self
            .project
            .as_ref()
            .map(|p| p.root.display().to_string())
            .unwrap_or_default();
        self.model
            .open_project_modal(current, crate::project::recent_projects());
    }

    fn handle_project_modal_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => self.model.project_modal = None,
            KeyCode::Up => self.model.project_modal_move(false),
            KeyCode::Down => self.model.project_modal_move(true),
            KeyCode::Backspace => self.model.project_modal_backspace(),
            KeyCode::Char(c) => self.model.project_modal_type(c),
            KeyCode::Enter => {
                let input = self
                    .model
                    .project_modal
                    .as_ref()
                    .map(|m| m.input.clone())
                    .unwrap_or_default();
                match self.switch_project(&input) {
                    Ok(()) => self.model.project_modal = None,
                    Err(e) => {
                        if let Some(m) = &mut self.model.project_modal {
                            m.error = Some(e);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        self.boot();

        loop {
            if let Some(project) = &self.project
                && self
                    .last_heartbeat
                    .is_none_or(|t| t.elapsed() >= crate::project::HEARTBEAT_EVERY)
            {
                project.heartbeat();
                self.last_heartbeat = Some(std::time::Instant::now());
            }
            self.model.refresh_telemetry();
            for update in self.manager.poll() {
                self.model.apply_acp_update(update);
            }
            for prompt in self.manager.poll_permissions() {
                self.model.set_permission_modal(prompt);
            }
            self.model.check_permission_timeout();
            self.poll_swarm_progress();
            self.poll_swarm_runs();
            self.poll_controller_turns();
            terminal.draw(|frame| view::render(&self.model, frame))?;

            if event::poll(std::time::Duration::from_millis(250))?
                && let Some(key) = event::read()?.as_key_press_event()
                && self.handle_key(key.code)
            {
                break Ok(());
            }
        }
    }

    /// Handle one key press. Returns true if the application should quit.
    fn handle_key(&mut self, code: KeyCode) -> bool {
        if self.model.active_permission.is_some() {
            match code {
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    if let Some(p) = self.model.active_permission.take() {
                        let _ = p
                            .reply_to
                            .send(crate::acp::client::PermissionDecision::Allow);
                        self.model
                            .push_activity(&p.agent_pane, "permission granted by operator");
                    }
                }
                KeyCode::Char('d') | KeyCode::Char('D') | KeyCode::Esc => {
                    if let Some(p) = self.model.active_permission.take() {
                        let _ = p
                            .reply_to
                            .send(crate::acp::client::PermissionDecision::Deny);
                        self.model
                            .push_activity(&p.agent_pane, "permission denied by operator");
                    }
                }
                _ => {}
            }
            return false;
        }

        if self.model.show_help || self.model.show_tasks {
            if matches!(code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('b')) {
                self.model.show_help = false;
                self.model.show_tasks = false;
            }
            return false;
        }

        if self.model.project_modal.is_some() {
            self.handle_project_modal_key(code);
            return false;
        }

        if self.model.is_controller_focused() {
            self.handle_controller_input_key(code);
            return false;
        }

        if code == KeyCode::Char('o') {
            self.open_project_dialog();
            return false;
        }
        if code == KeyCode::Char('?') {
            self.model.show_help = true;
            return false;
        }
        if code == KeyCode::Char('b') {
            self.model.show_tasks = true;
            return false;
        }

        if self.model.selected_tab >= 2 {
            match code {
                KeyCode::PageUp => {
                    self.model.scroll_pane_up(10);
                    return false;
                }
                KeyCode::PageDown => {
                    self.model.scroll_pane_down(10);
                    return false;
                }
                KeyCode::Home => {
                    self.model.scroll_pane_top();
                    return false;
                }
                KeyCode::End => {
                    self.model.scroll_pane_follow();
                    return false;
                }
                KeyCode::Char('z') => {
                    self.model.toggle_swarm_zoom();
                    return false;
                }
                KeyCode::Char('t') => {
                    self.model.toggle_show_thinking();
                    return false;
                }
                KeyCode::Esc if self.model.swarm_zoom => {
                    self.model.swarm_zoom = false;
                    return false;
                }
                _ => {}
            }
        }

        match code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Char(c @ '1'..='9') => self.model.select_tab(c as usize - '1' as usize),
            KeyCode::Char('l') | KeyCode::Right => self.model.next_tab(),
            KeyCode::Char('h') | KeyCode::Left => self.model.prev_tab(),
            KeyCode::Tab => {
                if self.model.selected_tab == 2 {
                    self.model.toggle_swarm_focus();
                } else {
                    self.model.next_dashboard_frame();
                }
            }
            KeyCode::BackTab => self.model.prev_dashboard_frame(),
            KeyCode::Char('j') | KeyCode::Down => self.model.next_server(),
            KeyCode::Char('k') | KeyCode::Up => self.model.prev_server(),
            KeyCode::Char('c') | KeyCode::Enter => self.activate_selected(),
            KeyCode::Char('d') => self.disconnect_selected(),
            KeyCode::Char('p') => {
                if self.model.selected_tab == 1 {
                    self.connect_selected();
                } else {
                    self.manager.probe_all();
                }
            }
            KeyCode::Char('x') => self.cancel_selected(),
            KeyCode::Char('r') => self.retry_selected(),
            KeyCode::Char('e') => {
                if let Ok(dir) = self.model.export_transcripts(None) {
                    self.model.push_activity(
                        "Locust",
                        format!("transcripts exported to {}", dir.display()),
                    );
                }
            }
            _ => {}
        }
        false
    }

    /// Handle a key press while the Controller chat input has focus: everything typeable
    /// goes into the message buffer instead of triggering the normal shortcuts above (so
    /// typing "q" into a chat message never quits the app).
    fn handle_controller_input_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Tab => self.model.toggle_swarm_focus(),
            KeyCode::PageUp => self.model.controller_scroll_up(10),
            KeyCode::PageDown => self.model.controller_scroll_down(10),
            KeyCode::Up => self.model.controller_scroll_up(1),
            KeyCode::Down => self.model.controller_scroll_down(1),
            KeyCode::Home => self.model.controller_scroll_top(),
            KeyCode::End => self.model.controller_scroll_follow(),
            KeyCode::Enter => {
                if let Some(text) = self.model.controller_submit()
                    && let Some(spec) = find_spec(CONTROLLER_SPEC_ID)
                {
                    let prompt = if self.controller_briefed {
                        text
                    } else {
                        self.controller_briefed = true;
                        format!("{}{}", self.addressing_instructions("Controller"), text)
                    };
                    self.controller_turns.push(self.manager.turn(
                        *spec,
                        CONTROLLER_SESSION_KEY.to_string(),
                        prompt,
                    ));
                }
            }
            KeyCode::Backspace => self.model.controller_backspace(),
            KeyCode::Char(c) => self.model.controller_type_char(c),
            _ => {}
        }
    }

    /// Drain any Controller chat turns that have finished since the last poll, dispatching
    /// any agent-to-agent directive the reply addressed to a Swarm pane.
    fn poll_controller_turns(&mut self) {
        let mut i = 0;
        while i < self.controller_turns.len() {
            match self.controller_turns[i].try_recv() {
                Ok(result) => {
                    let (res, usage) = match result {
                        Ok((text, usage)) => (Ok(text), usage),
                        Err(e) => (Err(e), None),
                    };
                    if let Some(u) = &usage {
                        crate::app::telemetry::record_tokens(
                            "Controller",
                            0,
                            0,
                            u.tokens_used,
                            u.context_size,
                            u.cost_amount.map(|amt| {
                                (
                                    amt,
                                    u.cost_currency.clone().unwrap_or_else(|| "USD".to_string()),
                                )
                            }),
                        );
                        self.model.refresh_telemetry();
                    }
                    let directives = self.model.push_controller_reply(res);
                    self.controller_turns.remove(i);
                    for (target, message) in directives {
                        self.start_directed_turn("Controller", target, message);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => i += 1,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.controller_turns.remove(i);
                }
            }
        }
    }

    /// Enter/'c': connect the selected ACP server, or run the selected swarm pane's real
    /// agent, depending on which tab is active.
    fn activate_selected(&mut self) {
        match self.model.selected_tab {
            1 => self.connect_selected(),
            n if n >= 2 => self.run_selected_swarm_pane(),
            _ => {}
        }
    }

    /// Connect (or reuse the existing connection for) the selected agent.
    fn connect_selected(&mut self) {
        let Some(server_id) = self.model.begin_connect_selected() else {
            return;
        };
        if let Some(spec) = find_spec(server_id) {
            self.manager.connect(*spec);
        }
    }

    fn disconnect_selected(&mut self) {
        if self.model.selected_tab != 1 {
            return;
        }
        if let Some(server) = self.model.selected_server() {
            let id = server.id;
            self.manager.disconnect(id);
            self.model.mark_disconnected(id);
        }
    }

    /// Run the selected swarm pane's real agent.
    fn run_selected_swarm_pane(&mut self) {
        self.start_run(self.model.swarm_selected_pane);
    }

    /// Cancel the active run on the selected swarm pane or controller.
    pub fn cancel_selected(&mut self) {
        if self.model.selected_tab >= 2 {
            let idx = self.model.swarm_selected_pane;
            if let Some(pane_name) = self.model.cancel_swarm_pane(idx) {
                self.swarm_runs.retain(|(name, _)| name != &pane_name);
                self.swarm_progress.retain(|(name, _)| name != &pane_name);
            }
        } else if (self.model.selected_tab == 0 || self.model.is_controller_focused())
            && !self.controller_turns.is_empty()
        {
            self.controller_turns.clear();
            self.model
                .push_controller_reply(Err("turn cancelled by operator".to_string()));
        }
    }

    /// Retry the last turn on the selected swarm pane.
    pub fn retry_selected(&mut self) {
        if self.model.selected_tab >= 2 {
            let idx = self.model.swarm_selected_pane;
            if idx < self.model.swarm_panes.len()
                && self.model.swarm_panes[idx].status != crate::app::model::SwarmRunStatus::Running
            {
                self.start_run(idx);
            }
        }
    }

    /// Run the pane at `idx` as a turn on its own session, keyed by its pane name, on its
    /// shared ACP connection -- one real LLM call -- in the background. That session
    /// resumes the pane's last conversation (saved to disk) if one exists, else starts
    /// fresh, so every run continues rather than starting over.
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

    /// Move every live event that arrived since the last poll into its pane, so the status,
    /// thinking text and streamed reply update while the turn is still running.
    fn poll_swarm_progress(&mut self) {
        let mut i = 0;
        while i < self.swarm_progress.len() {
            let mut closed = false;
            loop {
                match self.swarm_progress[i].1.try_recv() {
                    Ok(event) => {
                        let pane = self.swarm_progress[i].0.clone();
                        self.model.apply_turn_event(&pane, event);
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
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

    /// Drain any swarm agent runs that have finished since the last poll, dispatching any
    /// agent-to-agent directive the reply addressed to another pane.
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
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => i += 1,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.swarm_runs.remove(i);
                }
            }
        }
    }

    /// One-time text taught to a pane (or the Controller chat) the first time it runs,
    /// explaining the agent-to-agent addressing convention and who else is in the swarm. The
    /// session remembers it afterward, so this is never repeated on later turns.
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

    /// Dispatch one agent-written directive as a new turn on its target pane's own session --
    /// the auto-routing half of agent-to-agent addressing. The source is framed into the
    /// prompt so the receiving agent knows who is talking to it.
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

fn find_spec(id: &str) -> Option<&'static AgentSpec> {
    AGENT_SPECS.iter().find(|spec| spec.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::model::AcpStatus;

    #[test]
    fn test_quit_keys() {
        let mut controller = Controller::new();
        assert!(controller.handle_key(KeyCode::Char('q')));
        assert!(controller.handle_key(KeyCode::Esc));
    }

    #[test]
    fn test_tab_number_keys() {
        let mut controller = Controller::new();
        controller.handle_key(KeyCode::Char('2'));
        assert_eq!(controller.model.selected_tab, 1);
        controller.handle_key(KeyCode::Char('1'));
        assert_eq!(controller.model.selected_tab, 0);
    }

    #[test]
    fn test_probe_key_triggers_probe() {
        let mut controller = Controller::new();
        controller.handle_key(KeyCode::Char('2'));
        assert_eq!(controller.model.selected_tab, 1);
        assert!(!controller.handle_key(KeyCode::Char('p')));
        assert_eq!(
            controller.model.acp_servers[0].status,
            AcpStatus::Connecting
        );
    }

    #[test]
    fn test_digit_three_reaches_the_swarm_tab() {
        let mut controller = Controller::new();
        controller.handle_key(KeyCode::Char('3'));
        assert_eq!(controller.model.selected_tab, 2);
    }

    #[test]
    fn test_up_down_cycle_the_agent_list_on_swarm_tab() {
        let mut controller = Controller::new();
        controller.handle_key(KeyCode::Char('3'));
        let first = controller.model.selected_swarm_pane().unwrap().name.clone();

        controller.handle_key(KeyCode::Char('j'));
        let second = controller.model.selected_swarm_pane().unwrap().name.clone();
        assert_ne!(first, second);

        controller.handle_key(KeyCode::Char('k'));
        assert_eq!(controller.model.selected_swarm_pane().unwrap().name, first);
    }

    #[test]
    fn test_tab_focuses_controller_and_typing_does_not_trigger_shortcuts() {
        let mut controller = Controller::new();
        controller.handle_key(KeyCode::Char('3'));
        controller.handle_key(KeyCode::Tab);
        assert!(controller.model.is_controller_focused());

        // "q" while the chat input is focused must type, not quit.
        assert!(!controller.handle_key(KeyCode::Char('q')));
        assert_eq!(controller.model.controller_input, "q");

        controller.handle_key(KeyCode::Backspace);
        assert_eq!(controller.model.controller_input, "");

        // Esc un-focuses instead of quitting while the chat input has focus.
        assert!(!controller.handle_key(KeyCode::Esc));
        assert!(!controller.model.is_controller_focused());
    }

    #[test]
    fn test_controller_submit_queues_a_chat_turn_and_clears_input() {
        let mut controller = Controller::new();
        controller.handle_key(KeyCode::Char('3'));
        controller.handle_key(KeyCode::Tab);
        for c in "hello".chars() {
            controller.handle_key(KeyCode::Char(c));
        }
        controller.handle_key(KeyCode::Enter);

        assert_eq!(controller.model.controller_input, "");
        // The user's own message is logged immediately; the reply arrives later via poll.
        assert_eq!(controller.model.controller_log.len(), 1);
        assert_eq!(controller.model.controller_log[0].text, "hello");
        assert_eq!(controller.controller_turns.len(), 1);
    }

    #[test]
    #[ignore = "spawns the real acp-claude subprocess over npx; run with `cargo test -- --ignored`"]
    fn test_connect_to_real_acp_claude_goes_through_connecting_before_online() {
        use crate::app::model::AcpStatus;

        let mut controller = Controller::new();
        controller.model.select_tab(1);
        let idx = controller
            .model
            .acp_servers
            .iter()
            .position(|s| s.id == "acp-claude")
            .expect("acp-claude is a registered spec");
        controller.model.acp_selected_server = idx;

        controller.handle_key(KeyCode::Enter);
        assert_eq!(
            controller.model.acp_servers[idx].status,
            AcpStatus::Connecting,
            "status must not jump straight to Online before the background handshake runs"
        );

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            for update in controller.manager.poll() {
                controller.model.apply_acp_update(update);
            }
            if controller.model.acp_servers[idx].status != AcpStatus::Connecting {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for a real connect result"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        assert_eq!(
            controller.model.acp_servers[idx].status,
            AcpStatus::Online,
            "{:?}",
            controller.model.acp_servers[idx].recent_logs
        );
    }

    #[test]
    #[ignore = "spawns the real acp-claude subprocess over npx; run with `cargo test -- --ignored`"]
    fn test_swarm_run_and_connect_share_one_acp_connection() {
        let mut controller = Controller::new();

        // Connect "acp-claude" explicitly via the ACP Servers tab path.
        let spec = find_spec("acp-claude").unwrap();
        controller.manager.connect(*spec);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !controller.manager.is_connected("acp-claude") {
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting to connect"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // Running a Swarm pane for the same agent must reuse that connection, not spawn a
        // second subprocess -- `is_connected` would be unaffected either way, but a real
        // turn completing at all proves it rode the existing actor.
        let rx =
            controller
                .manager
                .turn(*spec, "Test Pane".to_string(), "Reply with OK.".to_string());
        let result = rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("turn should reply");
        assert!(result.is_ok(), "{:?}", result);
    }

    #[test]
    #[ignore = "spawns the real acp-claude subprocess over npx; run with `cargo test -- --ignored`"]
    fn test_swarm_pane_session_warmed_at_startup_is_reused_on_run() {
        let spec = find_spec("acp-claude").unwrap();
        let mut manager = Manager::new();

        // Startup warms the pane's session without running anything.
        let warm_rx = manager.ensure_session(*spec, "Test Pane".to_string());
        warm_rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("ensure_session should reply")
            .unwrap();

        // Running the pane afterwards must reuse that same session, not open a second one
        // -- verified by having a real conversation turn succeed on it immediately.
        let run_rx = manager.turn(
            *spec,
            "Test Pane".to_string(),
            "Reply with the word READY.".to_string(),
        );
        let result = run_rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("turn should reply");
        assert!(result.is_ok(), "{:?}", result);
    }

    #[test]
    #[ignore = "spawns the real acp-claude subprocess twice to prove cross-restart resume; run with `cargo test -- --ignored`"]
    fn test_session_resumes_across_manager_restarts() {
        let spec = find_spec("acp-claude").unwrap();
        let key = "Cross-Restart Test Pane".to_string();

        {
            let mut manager = Manager::new();
            let rx = manager.turn(
                *spec,
                key.clone(),
                "Remember this secret code: PINEAPPLE42. Acknowledge briefly, don't repeat it yet."
                    .to_string(),
            );
            let result = rx
                .recv_timeout(std::time::Duration::from_secs(30))
                .expect("first turn should reply");
            assert!(result.is_ok(), "{:?}", result);
        } // Manager (and its actor's connection) drops here, simulating Locust exiting.

        // A fresh Manager is a fresh subprocess, simulating a Locust restart. The same key
        // must resume the previous conversation via the session id saved to
        // .state/acp_sessions.json, not start over with no memory of it.
        let mut manager = Manager::new();
        let rx = manager.turn(
            *spec,
            key,
            "What was the secret code I told you? Reply with just the code.".to_string(),
        );
        let result = rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("resumed turn should reply");
        let (reply, _) = result.expect("resumed turn should succeed");
        assert!(
            reply.contains("PINEAPPLE42"),
            "expected resumed conversation to recall the secret, got: {reply}"
        );
    }

    #[test]
    #[ignore = "spawns every registered agent's real subprocess; run with `cargo test -- --ignored`"]
    fn test_startup_connects_every_registered_agent() {
        let mut controller = Controller::new();
        for spec in AGENT_SPECS {
            controller.manager.ensure_connected(*spec);
        }

        // Every spec should report *some* outcome (Connected or Failed -- e.g. opencode/
        // codex may not be installed on this machine) without anyone having pressed Enter.
        let mut seen: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while seen.len() < AGENT_SPECS.len() {
            for update in controller.manager.poll() {
                if matches!(
                    update.outcome,
                    acp::AgentOutcome::Connected(_) | acp::AgentOutcome::Failed(_)
                ) {
                    seen.insert(update.server_id);
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for all agents to report in; saw: {seen:?}"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    #[test]
    fn test_server_navigation_keys() {
        let mut controller = Controller::new();
        controller.model.select_tab(1);
        let len = controller.model.acp_servers.len();

        controller.handle_key(KeyCode::Char('j'));
        assert_eq!(controller.model.acp_selected_server, 1 % len);

        controller.handle_key(KeyCode::Char('k'));
        assert_eq!(controller.model.acp_selected_server, 0);
    }
}
