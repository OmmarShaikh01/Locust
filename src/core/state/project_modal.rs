use super::*;

impl Model {
    pub fn open_project_modal(&mut self, current: String, recent: Vec<String>) {
        self.project_modal = Some(ProjectModal {
            current,
            recent,
            ..ProjectModal::default()
        });
    }

    pub fn project_modal_type(&mut self, c: char) {
        if let Some(m) = &mut self.project_modal {
            m.input.push(c);
            m.selected = None;
            m.error = None;
        }
    }

    pub fn project_modal_backspace(&mut self) {
        if let Some(m) = &mut self.project_modal {
            m.input.pop();
            m.selected = None;
            m.error = None;
        }
    }

    /// Move the highlight through the recent list and copy that path into the input.
    pub fn project_modal_move(&mut self, down: bool) {
        let Some(m) = &mut self.project_modal else {
            return;
        };
        if m.recent.is_empty() {
            return;
        }
        let last = m.recent.len() - 1;
        let next = match (m.selected, down) {
            (None, true) => 0,
            (None, false) => last,
            (Some(i), true) => (i + 1).min(last),
            (Some(i), false) => i.saturating_sub(1),
        };
        m.selected = Some(next);
        m.input = m.recent[next].clone();
        m.error = None;
    }
}
