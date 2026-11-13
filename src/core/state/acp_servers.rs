use super::*;

impl Model {
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
}
