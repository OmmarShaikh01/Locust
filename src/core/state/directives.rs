use super::*;

impl Model {
    /// Pull every directive an agent addressed to another pane out of `text` that this run is
    /// actually allowed to dispatch: target can be a pane name, a tab name, or `*` for swarm-wide broadcast.
    /// Everything runs under the configured run limits (turn cap and token limit). Anything rejected
    /// is logged to the activity tracer (never silently dropped).
    pub(super) fn approved_directives(
        &mut self,
        source: &str,
        text: &str,
    ) -> Vec<(String, String)> {
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

    pub(super) fn now_secs() -> u64 {
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
    pub(super) fn create_task(&mut self, source: &str, target: &str, description: &str) {
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
    pub(super) fn complete_oldest_pending_task(&mut self, target: &str, status: TaskStatus) {
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
}
