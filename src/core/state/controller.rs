use super::*;

impl Model {
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
}
