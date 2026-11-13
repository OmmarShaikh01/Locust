use super::CockpitApp;
use crate::gui::input::key_to_char;
use crate::ipc::protocol::ClientCommand;
use gpui::*;

impl CockpitApp {
    /// Routes a raw key event to the active view: permission modal intercept,
    /// focused orchestrator composer editing, blurred orchestrator pane shortcuts,
    /// or global tab/scroll navigation. Extracted from `Render::render` so the
    /// render path stays pure layout composition.
    pub(super) fn handle_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let this = self;
        // 1. Permission request modal intercept
        if let Some(perm) = &this.snapshot.active_permission {
            let req_id = perm.request_id.clone();
            match event.keystroke.key.as_str() {
                "escape" | "n" | "N" => {
                    this.send_command(ClientCommand::ResolvePermission {
                        request_id: req_id,
                        allowed: false,
                    });
                    cx.notify();
                    return;
                }
                "enter" | "y" | "Y" => {
                    this.send_command(ClientCommand::ResolvePermission {
                        request_id: req_id,
                        allowed: true,
                    });
                    cx.notify();
                    return;
                }
                _ => return,
            }
        }

        // 1.5. Raise Request Modal Intercept
        if this.raise_modal_open {
            let key_name = event.keystroke.key.as_str();
            match key_name {
                "escape" => {
                    this.raise_modal_open = false;
                    cx.notify();
                    return;
                }
                "tab" => {
                    this.raise_focus_field = (this.raise_focus_field + 1) % 3;
                    cx.notify();
                    return;
                }
                "enter" => {
                    if event.keystroke.modifiers.control || this.raise_focus_field == 2 {
                        let title = this.raise_title.trim().to_string();
                        if !title.is_empty() {
                            let target_agent = if this.raise_target.trim().is_empty() {
                                "Mission Orchestrator".to_string()
                            } else {
                                this.raise_target.trim().to_string()
                            };
                            let description = this.raise_desc.trim().to_string();
                            this.send_command(ClientCommand::RaiseRequest {
                                title,
                                target_agent,
                                description,
                            });
                            this.raise_title.clear();
                            this.raise_desc.clear();
                            this.raise_modal_open = false;
                            cx.notify();
                            return;
                        }
                    } else {
                        this.raise_focus_field = (this.raise_focus_field + 1) % 3;
                        cx.notify();
                        return;
                    }
                }
                "backspace" => {
                    match this.raise_focus_field {
                        0 => {
                            this.raise_title.pop();
                        }
                        1 => {
                            this.raise_target.pop();
                        }
                        _ => {
                            this.raise_desc.pop();
                        }
                    }
                    cx.notify();
                    return;
                }
                _ => {
                    if !event.keystroke.modifiers.control
                        && !event.keystroke.modifiers.alt
                        && !event.keystroke.modifiers.platform
                        && let Some(s) = key_to_char(&event.keystroke)
                    {
                        match this.raise_focus_field {
                            0 => this.raise_title.push_str(&s),
                            1 => this.raise_target.push_str(&s),
                            _ => this.raise_desc.push_str(&s),
                        }
                        cx.notify();
                        return;
                    }
                }
            }
            return;
        }

        let is_orch = this.is_orchestrator_pane(this.snapshot.swarm_selected_pane);
        let in_cockpit = this.snapshot.selected_tab == 2;
        let key_name = event.keystroke.key.as_str();

        // 2. Active Orchestrator Composer Input:
        if in_cockpit && is_orch && this.composer_focused {
            // Clipboard paste (Ctrl+V / Cmd+V)
            if (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
                && (key_name == "v" || key_name == "V")
                && let Some(text) = cx.read_from_clipboard().and_then(|item| item.text())
            {
                let clean = text.replace("\r\n", " ").replace(['\n', '\r'], " ");
                this.composer_input.push_str(&clean);
                cx.notify();
                return;
            }

            // Clipboard copy (Ctrl+C / Cmd+C)
            if (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
                && (key_name == "c" || key_name == "C")
                && !this.composer_input.is_empty()
            {
                cx.write_to_clipboard(ClipboardItem::new_string(this.composer_input.clone()));
                return;
            }

            // Clear line (Ctrl+U or Ctrl+Backspace)
            if (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
                && (key_name == "u" || key_name == "U")
            {
                this.composer_input.clear();
                cx.notify();
                return;
            }

            match key_name {
                "enter" => {
                    let text = this.composer_input.trim().to_string();
                    if !text.is_empty() {
                        this.send_command(ClientCommand::SubmitControllerTurn {
                            text: text.clone(),
                        });
                        if this.history.last().map(|s| s.as_str()) != Some(&text) {
                            this.history.push(text);
                        }
                        this.history_index = None;
                        this.draft_input.clear();
                        this.composer_input.clear();
                        this.composer_focused = true;
                        cx.notify();
                    }
                    return;
                }
                "escape" => {
                    this.composer_focused = false;
                    cx.notify();
                    return;
                }
                "up" => {
                    if !this.history.is_empty() {
                        match this.history_index {
                            None => {
                                this.draft_input = this.composer_input.clone();
                                let new_idx = this.history.len() - 1;
                                this.history_index = Some(new_idx);
                                this.composer_input = this.history[new_idx].clone();
                            }
                            Some(idx) => {
                                if idx > 0 {
                                    let new_idx = idx - 1;
                                    this.history_index = Some(new_idx);
                                    this.composer_input = this.history[new_idx].clone();
                                }
                            }
                        }
                        cx.notify();
                    }
                    return;
                }
                "down" => {
                    if let Some(idx) = this.history_index {
                        if idx + 1 < this.history.len() {
                            let new_idx = idx + 1;
                            this.history_index = Some(new_idx);
                            this.composer_input = this.history[new_idx].clone();
                        } else {
                            this.history_index = None;
                            this.composer_input = this.draft_input.clone();
                        }
                        cx.notify();
                    }
                    return;
                }
                "backspace" => {
                    if event.keystroke.modifiers.control || event.keystroke.modifiers.platform {
                        this.composer_input.clear();
                    } else {
                        this.composer_input.pop();
                    }
                    cx.notify();
                    return;
                }
                "pageup" => {
                    let mut off = this.timeline_scroll_handle.offset();
                    off.y = (off.y + px(150.0)).min(px(0.0));
                    this.timeline_scroll_handle.set_offset(off);
                    cx.notify();
                    return;
                }
                "pagedown" => {
                    let mut off = this.timeline_scroll_handle.offset();
                    off.y -= px(150.0);
                    this.timeline_scroll_handle.set_offset(off);
                    cx.notify();
                    return;
                }
                "home" => {
                    this.timeline_scroll_handle.set_offset(Point::default());
                    cx.notify();
                    return;
                }
                "end" => {
                    this.timeline_scroll_handle.scroll_to_bottom();
                    cx.notify();
                    return;
                }
                _ => {
                    if let Some(ch) = key_to_char(&event.keystroke) {
                        this.composer_input.push_str(&ch);
                        cx.notify();
                        return;
                    }
                }
            }
            return;
        }

        // 3. Orchestrator Pane when composer is blurred (via Esc):
        if in_cockpit && is_orch && !this.composer_focused {
            match key_name {
                "enter" | "o" | "O" => {
                    this.composer_focused = true;
                    cx.notify();
                    return;
                }
                "0" => {
                    this.snapshot.selected_tab = 0;
                    this.send_command(ClientCommand::SwitchTab { tab_index: 0 });
                    cx.notify();
                    return;
                }
                "1" => {
                    this.snapshot.selected_tab = 1;
                    this.send_command(ClientCommand::SwitchTab { tab_index: 1 });
                    cx.notify();
                    return;
                }
                "2" => {
                    this.composer_focused = true;
                    cx.notify();
                    return;
                }
                "3" => {
                    this.snapshot.selected_tab = 3;
                    this.composer_focused = false;
                    this.send_command(ClientCommand::SwitchTab { tab_index: 3 });
                    cx.notify();
                    return;
                }
                "tab" => {
                    this.snapshot.selected_tab = 3;
                    this.send_command(ClientCommand::SwitchTab { tab_index: 3 });
                    cx.notify();
                    return;
                }
                "[" => {
                    if !this.snapshot.swarm_panes.is_empty() {
                        let curr = this.snapshot.swarm_selected_pane;
                        let prev = if curr == 0 {
                            this.snapshot.swarm_panes.len() - 1
                        } else {
                            curr - 1
                        };
                        this.snapshot.swarm_selected_pane = prev;
                        this.composer_focused = this.is_orchestrator_pane(prev);
                        this.send_command(ClientCommand::SelectPane { pane_index: prev });
                        cx.notify();
                    }
                    return;
                }
                "]" => {
                    if !this.snapshot.swarm_panes.is_empty() {
                        let curr = this.snapshot.swarm_selected_pane;
                        let next = (curr + 1) % this.snapshot.swarm_panes.len();
                        this.snapshot.swarm_selected_pane = next;
                        this.composer_focused = this.is_orchestrator_pane(next);
                        this.send_command(ClientCommand::SelectPane { pane_index: next });
                        cx.notify();
                    }
                    return;
                }
                "up" => {
                    let mut off = this.timeline_scroll_handle.offset();
                    off.y = (off.y + px(50.0)).min(px(0.0));
                    this.timeline_scroll_handle.set_offset(off);
                    cx.notify();
                    return;
                }
                "down" => {
                    let mut off = this.timeline_scroll_handle.offset();
                    off.y -= px(50.0);
                    this.timeline_scroll_handle.set_offset(off);
                    cx.notify();
                    return;
                }
                "pageup" => {
                    let mut off = this.timeline_scroll_handle.offset();
                    off.y = (off.y + px(150.0)).min(px(0.0));
                    this.timeline_scroll_handle.set_offset(off);
                    cx.notify();
                    return;
                }
                "pagedown" => {
                    let mut off = this.timeline_scroll_handle.offset();
                    off.y -= px(150.0);
                    this.timeline_scroll_handle.set_offset(off);
                    cx.notify();
                    return;
                }
                "home" => {
                    this.timeline_scroll_handle.set_offset(Point::default());
                    cx.notify();
                    return;
                }
                "end" => {
                    this.timeline_scroll_handle.scroll_to_bottom();
                    cx.notify();
                    return;
                }
                "t" => {
                    this.snapshot.show_thinking = !this.snapshot.show_thinking;
                    this.send_command(ClientCommand::ToggleThinking);
                    cx.notify();
                    return;
                }
                _ => {
                    if let Some(ch) = key_to_char(&event.keystroke) {
                        this.composer_focused = true;
                        this.composer_input.push_str(&ch);
                        cx.notify();
                        return;
                    }
                }
            }
            return;
        }

        // 4. Global Navigation & Scrolling (Tab 0, Tab 1, or non-orchestrator agent panes):
        match key_name {
            "0" => {
                this.snapshot.selected_tab = 0;
                this.composer_focused = false;
                this.send_command(ClientCommand::SwitchTab { tab_index: 0 });
                cx.notify();
            }
            "1" => {
                this.snapshot.selected_tab = 1;
                this.composer_focused = false;
                this.send_command(ClientCommand::SwitchTab { tab_index: 1 });
                cx.notify();
            }
            "2" => {
                this.snapshot.selected_tab = 2;
                this.composer_focused =
                    this.is_orchestrator_pane(this.snapshot.swarm_selected_pane);
                this.send_command(ClientCommand::SwitchTab { tab_index: 2 });
                cx.notify();
            }
            "3" => {
                this.snapshot.selected_tab = 3;
                this.composer_focused = false;
                this.send_command(ClientCommand::SwitchTab { tab_index: 3 });
                cx.notify();
            }
            "tab" => {
                let next = (this.snapshot.selected_tab + 1) % 4;
                this.snapshot.selected_tab = next;
                this.composer_focused =
                    next == 2 && this.is_orchestrator_pane(this.snapshot.swarm_selected_pane);
                this.send_command(ClientCommand::SwitchTab { tab_index: next });
                cx.notify();
            }
            "[" => {
                if !this.snapshot.swarm_panes.is_empty() {
                    let curr = this.snapshot.swarm_selected_pane;
                    let prev = if curr == 0 {
                        this.snapshot.swarm_panes.len() - 1
                    } else {
                        curr - 1
                    };
                    this.snapshot.swarm_selected_pane = prev;
                    this.composer_focused = this.is_orchestrator_pane(prev);
                    this.send_command(ClientCommand::SelectPane { pane_index: prev });
                    cx.notify();
                }
            }
            "]" => {
                if !this.snapshot.swarm_panes.is_empty() {
                    let curr = this.snapshot.swarm_selected_pane;
                    let next = (curr + 1) % this.snapshot.swarm_panes.len();
                    this.snapshot.swarm_selected_pane = next;
                    this.composer_focused = this.is_orchestrator_pane(next);
                    this.send_command(ClientCommand::SelectPane { pane_index: next });
                    cx.notify();
                }
            }
            "o" | "O" => {
                let orch = this.orchestrator_pane_index();
                this.snapshot.selected_tab = 2;
                this.snapshot.swarm_selected_pane = orch;
                this.composer_focused = true;
                this.send_command(ClientCommand::SwitchTab { tab_index: 2 });
                this.send_command(ClientCommand::SelectPane { pane_index: orch });
                cx.notify();
            }
            "enter" => {
                if is_orch && in_cockpit {
                    this.composer_focused = true;
                    cx.notify();
                }
            }
            "up" => {
                match this.snapshot.selected_tab {
                    0 => {
                        let mut off = this.matrix_scroll_handle.offset();
                        off.y = (off.y + px(50.0)).min(px(0.0));
                        this.matrix_scroll_handle.set_offset(off);
                    }
                    3 => {
                        let mut off = this.kanban_col0_scroll_handle.offset();
                        off.y = (off.y + px(50.0)).min(px(0.0));
                        this.kanban_col0_scroll_handle.set_offset(off);
                        this.kanban_col1_scroll_handle.set_offset(off);
                        this.kanban_col2_scroll_handle.set_offset(off);
                        this.kanban_col3_scroll_handle.set_offset(off);
                        this.kanban_col4_scroll_handle.set_offset(off);
                    }
                    _ => {
                        let mut off = this.timeline_scroll_handle.offset();
                        off.y = (off.y + px(50.0)).min(px(0.0));
                        this.timeline_scroll_handle.set_offset(off);
                    }
                }
                cx.notify();
            }
            "down" => {
                match this.snapshot.selected_tab {
                    0 => {
                        let mut off = this.matrix_scroll_handle.offset();
                        off.y -= px(50.0);
                        this.matrix_scroll_handle.set_offset(off);
                    }
                    3 => {
                        let mut off = this.kanban_col0_scroll_handle.offset();
                        off.y -= px(50.0);
                        this.kanban_col0_scroll_handle.set_offset(off);
                        this.kanban_col1_scroll_handle.set_offset(off);
                        this.kanban_col2_scroll_handle.set_offset(off);
                        this.kanban_col3_scroll_handle.set_offset(off);
                        this.kanban_col4_scroll_handle.set_offset(off);
                    }
                    _ => {
                        let mut off = this.timeline_scroll_handle.offset();
                        off.y -= px(50.0);
                        this.timeline_scroll_handle.set_offset(off);
                    }
                }
                cx.notify();
            }
            "pageup" => {
                match this.snapshot.selected_tab {
                    0 => {
                        let mut off = this.matrix_scroll_handle.offset();
                        off.y = (off.y + px(150.0)).min(px(0.0));
                        this.matrix_scroll_handle.set_offset(off);
                    }
                    3 => {
                        let mut off = this.kanban_col0_scroll_handle.offset();
                        off.y = (off.y + px(150.0)).min(px(0.0));
                        this.kanban_col0_scroll_handle.set_offset(off);
                        this.kanban_col1_scroll_handle.set_offset(off);
                        this.kanban_col2_scroll_handle.set_offset(off);
                        this.kanban_col3_scroll_handle.set_offset(off);
                        this.kanban_col4_scroll_handle.set_offset(off);
                    }
                    _ => {
                        let mut off = this.timeline_scroll_handle.offset();
                        off.y = (off.y + px(150.0)).min(px(0.0));
                        this.timeline_scroll_handle.set_offset(off);
                    }
                }
                cx.notify();
            }
            "pagedown" => {
                match this.snapshot.selected_tab {
                    0 => {
                        let mut off = this.matrix_scroll_handle.offset();
                        off.y -= px(150.0);
                        this.matrix_scroll_handle.set_offset(off);
                    }
                    3 => {
                        let mut off = this.kanban_col0_scroll_handle.offset();
                        off.y -= px(150.0);
                        this.kanban_col0_scroll_handle.set_offset(off);
                        this.kanban_col1_scroll_handle.set_offset(off);
                        this.kanban_col2_scroll_handle.set_offset(off);
                        this.kanban_col3_scroll_handle.set_offset(off);
                        this.kanban_col4_scroll_handle.set_offset(off);
                    }
                    _ => {
                        let mut off = this.timeline_scroll_handle.offset();
                        off.y -= px(150.0);
                        this.timeline_scroll_handle.set_offset(off);
                    }
                }
                cx.notify();
            }
            "home" => {
                this.timeline_scroll_handle.set_offset(Point::default());
                this.matrix_scroll_handle.set_offset(Point::default());
                this.kanban_col0_scroll_handle.set_offset(Point::default());
                this.kanban_col1_scroll_handle.set_offset(Point::default());
                this.kanban_col2_scroll_handle.set_offset(Point::default());
                this.kanban_col3_scroll_handle.set_offset(Point::default());
                this.kanban_col4_scroll_handle.set_offset(Point::default());
                cx.notify();
            }
            "end" => {
                this.timeline_scroll_handle.scroll_to_bottom();
                cx.notify();
            }
            "t" => {
                this.snapshot.show_thinking = !this.snapshot.show_thinking;
                this.send_command(ClientCommand::ToggleThinking);
                cx.notify();
            }
            "escape" => {
                if this.kanban_selected_item.is_some() {
                    this.kanban_selected_item = None;
                }
                this.composer_focused = false;
                cx.notify();
            }
            _ => {}
        }
    }
}
