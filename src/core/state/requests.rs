use super::*;

impl Model {
    /// Keep swarm requests in `path` so they survive a restart and resume per project.
    pub fn enable_request_store(&mut self, path: std::path::PathBuf) {
        if let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(saved) = serde_json::from_str::<Vec<SwarmRequest>>(&text)
        {
            let max_id = saved.iter().map(|r| r.id).max().unwrap_or(0);
            self.next_request_id = max_id + 1;
            self.requests = saved;
        }
        self.request_store = Some(path);
    }

    /// Save current requests to disk in JSON format.
    fn save_requests(&self) {
        let Some(path) = &self.request_store else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.requests) {
            let _ = std::fs::write(path, json);
        }
    }

    /// Raise a new issue or feature request in the swarm issue tracker.
    pub fn raise_request(
        &mut self,
        title: String,
        target_agent: String,
        description: String,
    ) -> u64 {
        let id = self.next_request_id;
        self.next_request_id += 1;
        let now = Self::now_secs();
        self.requests.push(SwarmRequest {
            id,
            title,
            target_agent,
            description,
            state: RequestState::Raised,
            created_at_secs: now,
            updated_at_secs: now,
            review_notes: Vec::new(),
        });
        self.save_requests();
        id
    }

    /// Update the state of a tracked request (e.g. Raised -> InProgress -> SelfReview -> HumanReview -> Passed).
    pub fn update_request_state(
        &mut self,
        id: u64,
        new_state: RequestState,
        review_note: Option<String>,
    ) -> bool {
        if let Some(req) = self.requests.iter_mut().find(|r| r.id == id) {
            req.state = new_state;
            req.updated_at_secs = Self::now_secs();
            if let Some(note) = review_note
                && !note.trim().is_empty()
            {
                req.review_notes.push(note);
            }
            self.save_requests();
            true
        } else {
            false
        }
    }
}
