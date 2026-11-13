use super::*;

impl Model {
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
