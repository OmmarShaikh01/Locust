use super::*;

impl Model {
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
}
