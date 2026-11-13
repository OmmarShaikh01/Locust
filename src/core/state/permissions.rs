use super::*;

impl Model {
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
}
