use crate::gui::theme::*;
use crate::ipc::framing::{read_frame, write_frame};
use crate::ipc::protocol::{ClientCommand, ModelSnapshot, PermissionSnapshot, ServerEvent};
use crate::ipc::socket::connect_stream;
use crate::logging::{self, Level};
use gpui::*;
use interprocess::TryClone;
use std::sync::mpsc::{self, Sender};
use std::thread;

pub struct CockpitApp {
    snapshot: ModelSnapshot,
    connected: bool,
    composer_input: String,
    composer_focused: bool,
    history: Vec<String>,
    history_index: Option<usize>,
    draft_input: String,
    cmd_tx: Option<Sender<ClientCommand>>,
    focus_handle: FocusHandle,
    timeline_scroll_handle: ScrollHandle,
    sidebar_scroll_handle: ScrollHandle,
    matrix_scroll_handle: ScrollHandle,
    activity_scroll_handle: ScrollHandle,
    kanban_col0_scroll_handle: ScrollHandle,
    kanban_col1_scroll_handle: ScrollHandle,
    kanban_col2_scroll_handle: ScrollHandle,
    kanban_col3_scroll_handle: ScrollHandle,
    kanban_filter: usize,
    kanban_selected_item: Option<String>,
}

impl CockpitApp {
    pub fn new(pipe_name: String, cx: &mut Context<Self>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ClientCommand>();
        let (event_tx, event_rx) = flume::unbounded::<ServerEvent>();

        let app = Self {
            snapshot: ModelSnapshot::default(),
            connected: false,
            composer_input: String::new(),
            composer_focused: true,
            history: Vec::new(),
            history_index: None,
            draft_input: String::new(),
            cmd_tx: Some(cmd_tx),
            focus_handle: cx.focus_handle(),
            timeline_scroll_handle: ScrollHandle::new(),
            sidebar_scroll_handle: ScrollHandle::new(),
            matrix_scroll_handle: ScrollHandle::new(),
            activity_scroll_handle: ScrollHandle::new(),
            kanban_col0_scroll_handle: ScrollHandle::new(),
            kanban_col1_scroll_handle: ScrollHandle::new(),
            kanban_col2_scroll_handle: ScrollHandle::new(),
            kanban_col3_scroll_handle: ScrollHandle::new(),
            kanban_filter: 0,
            kanban_selected_item: None,
        };

        cx.spawn(async move |entity, app| {
            while let Ok(event) = event_rx.recv_async().await {
                let _ = entity.update(app, |this: &mut CockpitApp, cx| match event {
                    ServerEvent::InitialSync {
                        state,
                        server_version: _,
                    } => {
                        this.snapshot = *state;
                        this.connected = true;
                        if this.snapshot.selected_tab == 2
                            && this.is_orchestrator_pane(this.snapshot.swarm_selected_pane)
                        {
                            this.composer_focused = true;
                        }
                        for msg in &this.snapshot.controller_history {
                            if (msg.role == "operator" || msg.role == "user")
                                && !msg.text.trim().is_empty()
                            {
                                if this.history.last().map(|s| s.as_str()) != Some(&msg.text) {
                                    this.history.push(msg.text.clone());
                                }
                            }
                        }
                        cx.notify();
                    }
                    ServerEvent::StateUpdated(state) => {
                        this.snapshot = *state;
                        cx.notify();
                    }
                    ServerEvent::TurnProgress {
                        pane_name,
                        delta_type,
                        content,
                    } => {
                        if let Some(pane) = this
                            .snapshot
                            .swarm_panes
                            .iter_mut()
                            .find(|p| p.name == pane_name)
                        {
                            if delta_type == "thinking" {
                                pane.thinking_lines.push(content);
                            } else {
                                pane.output_lines.push(content);
                            }
                            cx.notify();
                        }
                    }
                    _ => {}
                });
            }
        })
        .detach();

        thread::spawn(move || {
            let mut stream = match connect_stream(&pipe_name) {
                Ok(s) => s,
                Err(e) => {
                    logging::log(
                        Level::Error,
                        &format!("Failed to connect to daemon pipe: {e}"),
                    );
                    return;
                }
            };

            let mut write_stream = match stream.try_clone() {
                Ok(s) => s,
                Err(_) => return,
            };

            thread::spawn(move || {
                while let Ok(cmd) = cmd_rx.recv() {
                    if let Ok(bytes) = serde_json::to_vec(&cmd) {
                        if write_frame(&mut write_stream, &bytes).is_err() {
                            break;
                        }
                    }
                }
            });

            while let Ok(frame) = read_frame(&mut stream) {
                if let Ok(event) = serde_json::from_slice::<ServerEvent>(&frame) {
                    if event_tx.send(event).is_err() {
                        break;
                    }
                }
            }
        });

        app
    }

    pub fn send_command(&self, cmd: ClientCommand) {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(cmd);
        }
    }

    pub fn is_orchestrator_pane(&self, pane_idx: usize) -> bool {
        if let Some(pane) = self.snapshot.swarm_panes.get(pane_idx) {
            let lower = pane.name.to_lowercase();
            if lower.contains("orchestrator") || lower.contains("controller") {
                return true;
            }
        }
        if pane_idx == 0
            && !self.snapshot.swarm_panes.iter().any(|p| {
                let l = p.name.to_lowercase();
                l.contains("orchestrator") || l.contains("controller")
            })
        {
            return true;
        }
        false
    }

    pub fn find_pane_by_name(&self, name: &str) -> Option<usize> {
        let name_lower = name.trim().to_lowercase();
        self.snapshot
            .swarm_panes
            .iter()
            .position(|p| p.name.to_lowercase() == name_lower)
    }

    pub fn orchestrator_pane_index(&self) -> usize {
        self.snapshot
            .swarm_panes
            .iter()
            .position(|p| {
                let l = p.name.to_lowercase();
                l.contains("orchestrator") || l.contains("controller")
            })
            .unwrap_or(0)
    }

    fn render_header(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        let conn_badge_color = if self.connected {
            color_success_green()
        } else {
            color_danger_red()
        };
        let conn_text = if self.connected {
            "DAEMON LIVE"
        } else {
            "OFFLINE"
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .w_full()
            .h(px(48.0))
            .px(padding_lg())
            .bg(color_bg_base())
            .border_b_1()
            .border_color(color_border())
            .font_family(font_family_sans())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(gap_md())
                    .child(
                        div()
                            .font_family(font_family_sans())
                            .text_size(font_size_header())
                            .font_weight(FontWeight::BOLD)
                            .text_color(color_text_primary())
                            .child("LOCUST"),
                    )
                    .child(
                        div()
                            .font_family(font_family_sans())
                            .text_size(font_size_caption())
                            .text_color(color_text_muted())
                            .child("// SWISS GRID COCKPIT"),
                    )
                    .child(
                        div()
                            .px(padding_sm())
                            .py(padding_xs())
                            .rounded(radius_xs())
                            .bg(color_surface())
                            .border_1()
                            .border_color(color_border())
                            .font_family(font_family_sans())
                            .text_size(font_size_caption())
                            .text_color(color_text_muted())
                            .child(format!("PROJECT: {}", self.snapshot.project_path)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(gap_md())
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(gap_sm())
                            .px(padding_md())
                            .py(padding_xs())
                            .rounded(radius_xs())
                            .bg(color_surface())
                            .border_1()
                            .border_color(color_border())
                            .child(
                                div()
                                    .w(px(6.0))
                                    .h(px(6.0))
                                    .rounded_full()
                                    .bg(color_accent_blue()),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(color_text_primary())
                                    .child(format!(
                                        "TOKENS: {} / {} CAP",
                                        self.snapshot.total_tokens_used, self.snapshot.turn_cap
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(gap_sm())
                            .px(padding_md())
                            .py(padding_xs())
                            .rounded(radius_xs())
                            .bg(color_surface())
                            .border_1()
                            .border_color(color_border())
                            .child(
                                div()
                                    .w(px(6.0))
                                    .h(px(6.0))
                                    .rounded_full()
                                    .bg(conn_badge_color),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_primary())
                                    .child(conn_text),
                            ),
                    ),
            )
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = [
            "0 DASHBOARD",
            "1 ACP SERVERS",
            "2 SWARM COCKPIT",
            "3 KANBAN TRACKER",
        ];
        let selected_tab = self.snapshot.selected_tab;
        let selected_pane = self.snapshot.swarm_selected_pane;

        div()
            .flex()
            .flex_col()
            .w(px(256.0))
            .h_full()
            .bg(color_bg_base())
            .border_r_1()
            .border_color(color_border())
            .font_family(font_family_sans())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .font_family(font_family_sans())
                            .text_size(font_size_caption())
                            .font_weight(FontWeight::BOLD)
                            .text_color(color_text_muted())
                            .px(padding_md())
                            .py(padding_sm())
                            .border_b_1()
                            .border_color(color_border_subtle())
                            .child("NAVIGATION"),
                    )
                    .children(tabs.iter().enumerate().map(|(idx, title)| {
                        let is_active = idx == selected_tab;
                        let bg = if is_active {
                            color_surface()
                        } else {
                            color_bg_base()
                        };
                        let text_color = if is_active {
                            color_accent_blue()
                        } else {
                            color_text_primary()
                        };

                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .px(padding_md())
                            .py(padding_sm())
                            .bg(bg)
                            .border_b_1()
                            .border_color(color_border_subtle())
                            .border_l_2()
                            .border_color(if is_active {
                                color_accent_blue()
                            } else {
                                color_bg_base()
                            })
                            .hover(|s| s.bg(color_surface_hover()))
                            .cursor_pointer()
                            .font_family(font_family_sans())
                            .text_size(font_size_body())
                            .font_weight(if is_active {
                                FontWeight::BOLD
                            } else {
                                FontWeight::MEDIUM
                            })
                            .text_color(text_color)
                            .child(*title)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, window, cx| {
                                    this.snapshot.selected_tab = idx;
                                    if idx == 2
                                        && this
                                            .is_orchestrator_pane(this.snapshot.swarm_selected_pane)
                                    {
                                        this.composer_focused = true;
                                    } else {
                                        this.composer_focused = false;
                                    }
                                    window.focus(&this.focus_handle);
                                    this.send_command(ClientCommand::SwitchTab { tab_index: idx });
                                    cx.notify();
                                }),
                            )
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .id("sidebar-panes-scroll")
                    .overflow_y_scroll()
                    .track_scroll(&self.sidebar_scroll_handle)
                    .child(
                        div()
                            .font_family(font_family_sans())
                            .text_size(font_size_caption())
                            .font_weight(FontWeight::BOLD)
                            .text_color(color_text_muted())
                            .px(padding_md())
                            .py(padding_sm())
                            .border_b_1()
                            .border_color(color_border_subtle())
                            .child(format!("SWARM PANES ({})", self.snapshot.swarm_panes.len())),
                    )
                    .children(
                        self.snapshot
                            .swarm_panes
                            .iter()
                            .enumerate()
                            .map(|(idx, pane)| {
                                let is_active = selected_tab == 2 && idx == selected_pane;
                                let is_orch = self.is_orchestrator_pane(idx);
                                let dot_color = status_color(&pane.status);
                                let bg = if is_active {
                                    color_surface()
                                } else {
                                    color_bg_base()
                                };

                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .px(padding_md())
                                    .py(padding_sm())
                                    .bg(bg)
                                    .border_b_1()
                                    .border_color(color_border_subtle())
                                    .border_l_2()
                                    .border_color(if is_active {
                                        color_accent_blue()
                                    } else {
                                        color_bg_base()
                                    })
                                    .hover(|s| s.bg(color_surface_hover()))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, window, cx| {
                                            this.snapshot.selected_tab = 2;
                                            this.snapshot.swarm_selected_pane = idx;
                                            this.composer_focused = this.is_orchestrator_pane(idx);
                                            window.focus(&this.focus_handle);
                                            this.send_command(ClientCommand::SwitchTab {
                                                tab_index: 2,
                                            });
                                            this.send_command(ClientCommand::SelectPane {
                                                pane_index: idx,
                                            });
                                            cx.notify();
                                        }),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(gap_sm())
                                            .child(
                                                div()
                                                    .w(px(6.0))
                                                    .h(px(6.0))
                                                    .rounded_full()
                                                    .bg(dot_color),
                                            )
                                            .child(
                                                div()
                                                    .font_family(font_family_sans())
                                                    .text_size(font_size_body())
                                                    .font_weight(if is_active {
                                                        FontWeight::SEMIBOLD
                                                    } else {
                                                        FontWeight::NORMAL
                                                    })
                                                    .text_color(color_text_primary())
                                                    .child(pane.name.clone()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(gap_xs())
                                            .child(if is_orch {
                                                div()
                                                    .px(padding_xs())
                                                    .py(px(1.0))
                                                    .rounded(radius_xs())
                                                    .bg(color_surface_hover())
                                                    .border_1()
                                                    .border_color(color_accent_blue())
                                                    .font_family(font_family_sans())
                                                    .text_size(px(9.0))
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(color_accent_blue())
                                                    .child("ORCH")
                                            } else {
                                                div()
                                            })
                                            .child(
                                                div()
                                                    .font_family(font_family_sans())
                                                    .text_size(font_size_caption())
                                                    .text_color(color_text_muted())
                                                    .child(pane.agent.clone()),
                                            ),
                                    )
                            }),
                    ),
            )
    }

    fn render_timeline(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(pane) = self
            .snapshot
            .swarm_panes
            .get(self.snapshot.swarm_selected_pane)
        else {
            return div()
                .flex()
                .items_center()
                .justify_center()
                .size_full()
                .font_family(font_family_sans())
                .text_color(color_text_muted())
                .child("Select a pane to view transcript");
        };

        let is_orch = self.is_orchestrator_pane(self.snapshot.swarm_selected_pane);
        let status_badge_color = status_color(&pane.status);
        let show_thinking = self.snapshot.show_thinking;

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(color_bg_base())
            .font_family(font_family_sans())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px(padding_lg())
                    .py(padding_sm())
                    .border_b_1()
                    .border_color(color_border())
                    .bg(color_surface())
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(gap_md())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .w(px(32.0))
                                    .h(px(32.0))
                                    .rounded(radius_xs())
                                    .bg(if is_orch {
                                        color_surface_hover()
                                    } else {
                                        color_bg_base()
                                    })
                                    .border_1()
                                    .border_color(if is_orch {
                                        color_accent_blue()
                                    } else {
                                        color_border()
                                    })
                                    .font_family(font_family_sans())
                                    .text_size(font_size_body())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(if is_orch {
                                        color_accent_blue()
                                    } else {
                                        color_text_muted()
                                    })
                                    .child(if is_orch {
                                        "O".to_string()
                                    } else {
                                        pane.name
                                            .chars()
                                            .next()
                                            .unwrap_or('A')
                                            .to_uppercase()
                                            .to_string()
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(gap_sm())
                                            .child(
                                                div()
                                                    .font_family(font_family_sans())
                                                    .text_size(font_size_title())
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(color_text_primary())
                                                    .child(pane.name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .font_family(font_family_sans())
                                                    .text_size(font_size_body())
                                                    .text_color(color_text_muted())
                                                    .child(format!("@{}", pane.agent)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .text_color(color_text_muted())
                                            .child(if is_orch {
                                                format!(
                                                    "ROLE: ORCHESTRATOR | MODEL: {} | DIRECT HUMAN INTERACTION CHANNEL",
                                                    pane.model
                                                )
                                            } else {
                                                format!(
                                                    "ROLE: AUTONOMOUS WORKER | MODEL: {} | NON-INTERACTIVE EXECUTION STREAM",
                                                    pane.model
                                                )
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(gap_sm())
                            .child(
                                div()
                                    .px(padding_md())
                                    .py(padding_xs())
                                    .rounded(radius_xs())
                                    .bg(if is_orch {
                                        color_surface_hover()
                                    } else {
                                        color_bg_base()
                                    })
                                    .border_1()
                                    .border_color(if is_orch {
                                        color_accent_blue()
                                    } else {
                                        color_border()
                                    })
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(if is_orch {
                                        color_accent_blue()
                                    } else {
                                        color_text_muted()
                                    })
                                    .child(if is_orch {
                                        "INTERACTIVE ORCHESTRATOR"
                                    } else {
                                        "NON-INTERACTIVE AGENT"
                                    }),
                            )
                            .child(
                                div()
                                    .px(padding_md())
                                    .py(padding_xs())
                                    .rounded(radius_xs())
                                    .bg(if show_thinking {
                                        color_surface_hover()
                                    } else {
                                        color_surface()
                                    })
                                    .border_1()
                                    .border_color(color_border())
                                    .cursor_pointer()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if show_thinking {
                                        color_warning_amber()
                                    } else {
                                        color_text_muted()
                                    })
                                    .child(if show_thinking {
                                        "HIDE THINKING"
                                    } else {
                                        "SHOW THINKING"
                                    })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, _| {
                                            this.send_command(ClientCommand::ToggleThinking);
                                        }),
                                    ),
                            )
                            .child(
                                div()
                                    .px(padding_md())
                                    .py(padding_xs())
                                    .rounded(radius_xs())
                                    .bg(color_surface())
                                    .border_1()
                                    .border_color(status_badge_color)
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(status_badge_color)
                                    .child(pane.status.to_uppercase()),
                            ),
                    ),
            )
            .child(
                div()
                    .id("timeline-scroll-feed")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.timeline_scroll_handle)
                    .child(if is_orch {
                        // Orchestrator transcript: Human-to-Orchestrator conversation + thinking + dispatches
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .child(
                                div()
                                    .px(padding_md())
                                    .py(padding_xs())
                                    .bg(color_surface())
                                    .border_b_1()
                                    .border_color(color_border_subtle())
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_accent_blue())
                                    .child("OPERATOR <-> ORCHESTRATOR DIALOGUE"),
                            )
                            .children(if self.snapshot.controller_history.is_empty() {
                                vec![div()
                                    .px(padding_md())
                                    .py(padding_md())
                                    .font_family(font_family_sans())
                                    .text_size(font_size_body())
                                    .text_color(color_text_muted())
                                    .child("No dialogue recorded yet. Submit instructions in the composer below to direct the Orchestrator.")]
                            } else {
                                self.snapshot
                                    .controller_history
                                    .iter()
                                    .map(|msg| {
                                        let is_user = msg.role == "operator" || msg.role == "user";
                                        let is_agent = msg.role == "agent";
                                        let role_color = if is_user {
                                            color_accent_blue()
                                        } else if is_agent {
                                            color_success_green()
                                        } else {
                                            color_text_muted()
                                        };
                                        let role_label = if is_user {
                                            "OPERATOR"
                                        } else if is_agent {
                                            "MISSION ORCHESTRATOR"
                                        } else {
                                            "SYSTEM"
                                        };

                                        div()
                                            .flex()
                                            .flex_col()
                                            .px(padding_md())
                                            .py(padding_sm())
                                            .border_b_1()
                                            .border_color(color_border_subtle())
                                            .border_l_2()
                                            .border_color(role_color)
                                            .bg(if is_user {
                                                color_surface()
                                            } else {
                                                color_bg_base()
                                            })
                                            .gap(gap_xs())
                                            .child(
                                                div()
                                                    .font_family(font_family_sans())
                                                    .text_size(font_size_caption())
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(role_color)
                                                    .child(role_label),
                                            )
                                            .child(
                                                div()
                                                    .font_family(font_family_sans())
                                                    .text_size(font_size_body())
                                                    .text_color(color_text_primary())
                                                    .child(msg.text.clone()),
                                            )
                                    })
                                    .collect()
                            })
                            .child(if show_thinking && !pane.thinking_lines.is_empty() {
                                div()
                                    .flex()
                                    .flex_col()
                                    .p(padding_md())
                                    .bg(color_surface())
                                    .border_b_1()
                                    .border_color(color_border())
                                    .gap(gap_xs())
                                    .child(
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(color_warning_amber())
                                            .child("> ORCHESTRATOR STRATEGY & REASONING"),
                                    )
                                    .children(pane.thinking_lines.iter().map(|line| {
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_body())
                                            .text_color(color_text_muted())
                                            .child(line.clone())
                                    }))
                            } else {
                                div()
                            })
                            .child(if !pane.output_lines.is_empty() {
                                div()
                                    .flex()
                                    .flex_col()
                                    .p(padding_md())
                                    .gap(gap_xs())
                                    .child(
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(color_text_muted())
                                            .child("> ORCHESTRATOR DELEGATIONS & ACTIVITY"),
                                    )
                                    .children(pane.output_lines.iter().map(|line| {
                                        div()
                                            .p(padding_xs())
                                            .border_b_1()
                                            .border_color(color_border_subtle())
                                            .font_family(font_family_sans())
                                            .text_size(font_size_body())
                                            .text_color(color_text_primary())
                                            .child(line.clone())
                                    }))
                            } else {
                                div()
                            })
                    } else {
                        // Autonomous Worker Pane: Verbose thinking + execution state stream
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .child(
                                // Execution State Ribbon
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .px(padding_md())
                                    .py(padding_xs())
                                    .bg(color_surface())
                                    .border_b_1()
                                    .border_color(color_border())
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .gap(gap_md())
                                            .child(
                                                div()
                                                    .text_color(status_badge_color)
                                                    .font_weight(FontWeight::BOLD)
                                                    .child(format!("STATUS: {}", pane.status.to_uppercase())),
                                            )
                                            .child(
                                                div()
                                                    .text_color(color_text_muted())
                                                    .child(format!("PHASE: {}", if pane.phase.is_empty() { "WAITING" } else { &pane.phase })),
                                            )
                                            .child(
                                                div()
                                                    .text_color(color_text_muted())
                                                    .child(format!("TURNS: {}", pane.turns_used)),
                                            )
                                            .child(
                                                div()
                                                    .text_color(color_text_muted())
                                                    .child(format!("TOKENS: {}", pane.tokens_used)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_color(color_text_muted())
                                            .child(format!("RUNTIME: {}s", pane.elapsed_secs)),
                                    ),
                            )
                            .child(
                                // Verbose Thinking Stream (Always visible & prominent)
                                div()
                                    .flex()
                                    .flex_col()
                                    .p(padding_md())
                                    .bg(color_surface())
                                    .border_b_1()
                                    .border_color(color_border())
                                    .gap(gap_xs())
                                    .child(
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(color_warning_amber())
                                            .child("> VERBOSE REASONING STREAM (INTERNAL THOUGHTS & DECISIONS)"),
                                    )
                                    .children(if pane.thinking_lines.is_empty() {
                                        vec![div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .text_color(color_text_muted())
                                            .child("No internal reasoning steps logged for current task.")]
                                    } else {
                                        pane.thinking_lines
                                            .iter()
                                            .map(|line| {
                                                div()
                                                    .p(padding_xs())
                                                    .border_b_1()
                                                    .border_color(color_border_subtle())
                                                    .font_family(font_family_sans())
                                                    .text_size(font_size_body())
                                                    .text_color(color_text_muted())
                                                    .child(line.clone())
                                            })
                                            .collect()
                                    }),
                            )
                            .child(
                                // Autonomous Output / Tool Results Stream
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .p(padding_md())
                                    .gap(gap_xs())
                                    .child(
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(color_text_muted())
                                            .child("> AUTONOMOUS EXECUTION OUTPUT & TOOL RESULTS"),
                                    )
                                    .children(if pane.output_lines.is_empty() {
                                        vec![div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_body())
                                            .text_color(color_text_muted())
                                            .child("Agent is waiting for task directives from Orchestrator.")]
                                    } else {
                                        pane.output_lines
                                            .iter()
                                            .map(|line| {
                                                div()
                                                    .p(padding_xs())
                                                    .border_b_1()
                                                    .border_color(color_border_subtle())
                                                    .font_family(font_family_sans())
                                                    .text_size(font_size_body())
                                                    .text_color(color_text_primary())
                                                    .child(line.clone())
                                            })
                                            .collect()
                                    }),
                            )
                    }),
            )
    }

    fn render_composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_orch = self.is_orchestrator_pane(self.snapshot.swarm_selected_pane);
        let input_text = self.composer_input.clone();
        let focused = self.composer_focused;

        if is_orch {
            div()
                .flex()
                .flex_row()
                .items_center()
                .w_full()
                .px(padding_md())
                .py(padding_sm())
                .bg(color_bg_base())
                .border_t_1()
                .border_color(color_border())
                .gap(gap_sm())
                .font_family(font_family_sans())
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .flex_1()
                        .px(padding_md())
                        .py(padding_xs())
                        .rounded(radius_xs())
                        .bg(if focused {
                            color_surface_hover()
                        } else {
                            color_surface()
                        })
                        .border_1()
                        .border_color(if focused {
                            color_accent_blue()
                        } else {
                            color_border()
                        })
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                this.composer_focused = true;
                                window.focus(&this.focus_handle);
                                cx.notify();
                            }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .font_family(font_family_sans())
                                .text_size(font_size_body())
                                .text_color(if input_text.is_empty() {
                                    color_text_muted()
                                } else {
                                    color_text_primary()
                                })
                                .child(if input_text.is_empty() {
                                    if focused {
                                        "Type instruction for Orchestrator and press Enter (Esc to blur)...".to_string()
                                    } else {
                                        "Click or type to direct Orchestrator (0/1/2 switch tabs, [ / ] switch agents)...".to_string()
                                    }
                                } else {
                                    format!("{input_text}{}", if focused { "_" } else { "" })
                                }),
                        ),
                )
                .child(
                    div()
                        .px(padding_md())
                        .py(padding_xs())
                        .rounded(radius_xs())
                        .bg(color_accent_blue())
                        .hover(|s| s.opacity(0.85))
                        .cursor_pointer()
                        .font_family(font_family_sans())
                        .text_size(font_size_body())
                        .font_weight(FontWeight::BOLD)
                        .text_color(color_text_primary())
                        .child("TALK TO ORCHESTRATOR")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                let text = this.composer_input.trim().to_string();
                                if !text.is_empty() {
                                    this.send_command(ClientCommand::SubmitControllerTurn { text: text.clone() });
                                    if this.history.last().map(|s| s.as_str()) != Some(&text) {
                                        this.history.push(text);
                                    }
                                    this.history_index = None;
                                    this.draft_input.clear();
                                    this.composer_input.clear();
                                    this.composer_focused = true;
                                    window.focus(&this.focus_handle);
                                    cx.notify();
                                }
                            }),
                        ),
                )
        } else {
            // Non-interactive sub-agent notice bar with quick shortcut to switch to Orchestrator
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .w_full()
                .px(padding_md())
                .py(padding_sm())
                .bg(color_surface())
                .border_t_1()
                .border_color(color_border())
                .gap(gap_md())
                .font_family(font_family_sans())
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(gap_sm())
                        .child(
                            div()
                                .px(padding_sm())
                                .py(padding_xs())
                                .rounded(radius_xs())
                                .bg(color_bg_base())
                                .border_1()
                                .border_color(color_border())
                                .font_family(font_family_sans())
                                .text_size(font_size_caption())
                                .font_weight(FontWeight::BOLD)
                                .text_color(color_warning_amber())
                                .child("NON-INTERACTIVE AGENT"),
                        )
                        .child(
                            div()
                                .font_family(font_family_sans())
                                .text_size(font_size_caption())
                                .text_color(color_text_muted())
                                .child("Autonomous worker prints verbose thinking and state. Only the Orchestrator communicates with humans."),
                        ),
                )
                .child(
                    div()
                        .px(padding_md())
                        .py(padding_xs())
                        .rounded(radius_xs())
                        .bg(color_accent_blue())
                        .hover(|s| s.opacity(0.85))
                        .cursor_pointer()
                        .font_family(font_family_sans())
                        .text_size(font_size_caption())
                        .font_weight(FontWeight::BOLD)
                        .text_color(color_text_primary())
                        .child("TALK TO ORCHESTRATOR [O]")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                let orch_idx = this.orchestrator_pane_index();
                                this.snapshot.selected_tab = 2;
                                this.snapshot.swarm_selected_pane = orch_idx;
                                this.composer_focused = true;
                                window.focus(&this.focus_handle);
                                this.send_command(ClientCommand::SwitchTab { tab_index: 2 });
                                this.send_command(ClientCommand::SelectPane {
                                    pane_index: orch_idx,
                                });
                                cx.notify();
                            }),
                        ),
                )
        }
    }

    fn render_inspector(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w(px(320.0))
            .h_full()
            .bg(color_bg_base())
            .border_l_1()
            .border_color(color_border())
            .font_family(font_family_sans())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .font_family(font_family_sans())
                            .text_size(font_size_caption())
                            .font_weight(FontWeight::BOLD)
                            .text_color(color_text_muted())
                            .px(padding_md())
                            .py(padding_sm())
                            .bg(color_surface())
                            .border_b_1()
                            .border_color(color_border_subtle())
                            .child("TELEMETRY & LIMITS"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .px(padding_md())
                            .py(padding_sm())
                            .border_b_1()
                            .border_color(color_border_subtle())
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_body())
                                    .text_color(color_text_muted())
                                    .child("Total Tokens"),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_body())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_primary())
                                    .child(self.snapshot.total_tokens_used.to_string()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .px(padding_md())
                            .py(padding_sm())
                            .border_b_1()
                            .border_color(color_border_subtle())
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_body())
                                    .text_color(color_text_muted())
                                    .child("Turn Cap"),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_body())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_primary())
                                    .child(format!("{} max", self.snapshot.turn_cap)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .px(padding_md())
                            .py(padding_sm())
                            .border_b_1()
                            .border_color(color_border())
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_body())
                                    .text_color(color_text_muted())
                                    .child("Estimated Cost"),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_body())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_success_green())
                                    .child(format!("${:.4} USD", self.snapshot.total_cost_usd)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .child(
                        div()
                            .font_family(font_family_sans())
                            .text_size(font_size_caption())
                            .font_weight(FontWeight::BOLD)
                            .text_color(color_text_muted())
                            .px(padding_md())
                            .py(padding_sm())
                            .bg(color_surface())
                            .border_b_1()
                            .border_color(color_border_subtle())
                            .child("AGENT HANDOFFS"),
                    )
                    .children(if self.snapshot.tasks.is_empty() {
                        vec![
                            div()
                                .px(padding_md())
                                .py(padding_sm())
                                .font_family(font_family_sans())
                                .text_size(font_size_body())
                                .text_color(color_text_muted())
                                .child("No active handoff tasks."),
                        ]
                    } else {
                        self.snapshot
                            .tasks
                            .iter()
                            .map(|task| {
                                let status_col = status_color(&task.status);
                                div()
                                    .flex()
                                    .flex_col()
                                    .px(padding_md())
                                    .py(padding_sm())
                                    .border_b_1()
                                    .border_color(color_border_subtle())
                                    .gap(gap_xs())
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .font_family(font_family_sans())
                                                    .text_size(font_size_caption())
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(color_text_primary())
                                                    .child(format!("{} -> {}", task.from, task.to)),
                                            )
                                            .child(
                                                div()
                                                    .px(padding_sm())
                                                    .py(padding_xs())
                                                    .rounded(radius_xs())
                                                    .bg(color_surface())
                                                    .border_1()
                                                    .border_color(status_col)
                                                    .font_family(font_family_sans())
                                                    .text_size(font_size_caption())
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(status_col)
                                                    .child(task.status.to_uppercase()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .text_color(color_text_muted())
                                            .child(task.prompt.clone()),
                                    )
                            })
                            .collect()
                    }),
            )
    }

    fn render_permission_modal(
        &self,
        perm: &PermissionSnapshot,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let perm_id = perm.request_id.clone();
        let perm_id_deny = perm.request_id.clone();

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x000000cc))
            .font_family(font_family_sans())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(480.0))
                    .p(padding_lg())
                    .rounded(radius_xs())
                    .bg(color_surface())
                    .border_1()
                    .border_color(color_warning_amber())
                    .gap(gap_md())
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_title())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_warning_amber())
                                    .child("PERMISSION REQUIRED"),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .text_color(color_text_muted())
                                    .child(format!("Expires in {}s", perm.seconds_left)),
                            ),
                    )
                    .child(
                        div()
                            .font_family(font_family_sans())
                            .text_size(font_size_body())
                            .text_color(color_text_primary())
                            .child(format!(
                                "Agent @{} requests execution of tool: {}",
                                perm.agent_id, perm.tool
                            )),
                    )
                    .child(
                        div()
                            .p(padding_md())
                            .rounded(radius_xs())
                            .bg(color_bg_base())
                            .border_1()
                            .border_color(color_border())
                            .font_family(font_family_sans())
                            .text_size(font_size_caption())
                            .text_color(color_text_muted())
                            .child(perm.params.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_end()
                            .gap(gap_sm())
                            .child(
                                div()
                                    .px(padding_lg())
                                    .py(padding_xs())
                                    .rounded(radius_xs())
                                    .bg(color_surface_hover())
                                    .border_1()
                                    .border_color(color_border())
                                    .cursor_pointer()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_body())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_danger_red())
                                    .child("DENY [ESC]")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, _| {
                                            this.send_command(ClientCommand::ResolvePermission {
                                                request_id: perm_id_deny.clone(),
                                                allowed: false,
                                            });
                                        }),
                                    ),
                            )
                            .child(
                                div()
                                    .px(padding_lg())
                                    .py(padding_xs())
                                    .rounded(radius_xs())
                                    .bg(color_accent_blue())
                                    .cursor_pointer()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_body())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_primary())
                                    .child("ALLOW ACCESS")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, _| {
                                            this.send_command(ClientCommand::ResolvePermission {
                                                request_id: perm_id.clone(),
                                                allowed: true,
                                            });
                                        }),
                                    ),
                            ),
                    ),
            )
    }
    fn render_swarm_cockpit(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .flex_1()
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .child(self.render_timeline(cx))
                    .child(self.render_composer(cx)),
            )
            .child(self.render_inspector(cx))
    }

    fn render_dashboard(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let panes = &self.snapshot.swarm_panes;
        let mut idle_count = 0;
        let mut running_count = 0;
        let mut done_count = 0;
        let mut error_count = 0;
        for p in panes {
            match p.status.to_uppercase().as_str() {
                "ONLINE" | "DONE" => done_count += 1,
                "RUNNING" | "THINKING" => running_count += 1,
                "ERROR" | "FAILED" => error_count += 1,
                _ => idle_count += 1,
            }
        }

        div()
            .flex()
            .flex_col()
            .flex_1()
            .size_full()
            .bg(color_bg_base())
            .font_family(font_family_sans())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .w_full()
                    .border_b_1()
                    .border_color(color_border())
                    .child(
                        div()
                            .flex_1()
                            .p(padding_md())
                            .border_r_1()
                            .border_color(color_border())
                            .gap(gap_xs())
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_muted())
                                    .child("SWARM AGENTS"),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_header())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_primary())
                                    .child(format!("{} Total", panes.len())),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap(gap_sm())
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .child(
                                        div()
                                            .text_color(color_success_green())
                                            .child(format!("{done_count} done")),
                                    )
                                    .child(
                                        div()
                                            .text_color(color_warning_amber())
                                            .child(format!("{running_count} running")),
                                    )
                                    .child(
                                        div()
                                            .text_color(color_danger_red())
                                            .child(format!("{error_count} err")),
                                    )
                                    .child(
                                        div()
                                            .text_color(color_text_muted())
                                            .child(format!("{idle_count} idle")),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .p(padding_md())
                            .border_r_1()
                            .border_color(color_border())
                            .gap(gap_xs())
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_muted())
                                    .child("TOKEN USAGE"),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_header())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_accent_blue())
                                    .child(format!("{}", self.snapshot.total_tokens_used)),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .text_color(color_text_muted())
                                    .child(format!("Turn Cap: {} max", self.snapshot.turn_cap)),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .p(padding_md())
                            .border_r_1()
                            .border_color(color_border())
                            .gap(gap_xs())
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_muted())
                                    .child("FINANCIAL TELEMETRY"),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_header())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_success_green())
                                    .child(format!("${:.4} USD", self.snapshot.total_cost_usd)),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .text_color(color_text_muted())
                                    .child(format!(
                                        "{} invocations",
                                        self.snapshot.total_invocations
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .p(padding_md())
                            .gap(gap_xs())
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_muted())
                                    .child("ACP SERVERS"),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_header())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_primary())
                                    .child(format!(
                                        "{} Configured",
                                        self.snapshot.acp_servers.len()
                                    )),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .text_color(color_text_muted())
                                    .child("Protocol v5.0.0"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .w_full()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .border_r_1()
                            .border_color(color_border())
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .justify_between()
                                    .items_center()
                                    .px(padding_md())
                                    .py(padding_sm())
                                    .bg(color_surface())
                                    .border_b_1()
                                    .border_color(color_border())
                                    .child(
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(color_text_muted())
                                            .child("SWARM AGENT MATRIX"),
                                    )
                                    .child(
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .text_color(color_text_muted())
                                            .child("CLICK PANE TO OPEN COCKPIT"),
                                    ),
                            )
                            .child(
                                div()
                                    .id("dashboard-matrix-scroll")
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.matrix_scroll_handle)
                                    .children(panes.iter().enumerate().map(|(idx, p)| {
                                        let status_col = status_color(&p.status);
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .justify_between()
                                            .px(padding_md())
                                            .py(padding_sm())
                                            .border_b_1()
                                            .border_color(color_border_subtle())
                                            .hover(|s| s.bg(color_surface_hover()))
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(move |this, _, window, cx| {
                                                    this.snapshot.selected_tab = 2;
                                                    this.snapshot.swarm_selected_pane = idx;
                                                    this.composer_focused =
                                                        this.is_orchestrator_pane(idx);
                                                    window.focus(&this.focus_handle);
                                                    this.send_command(ClientCommand::SwitchTab {
                                                        tab_index: 2,
                                                    });
                                                    this.send_command(ClientCommand::SelectPane {
                                                        pane_index: idx,
                                                    });
                                                    cx.notify();
                                                }),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_row()
                                                    .items_center()
                                                    .gap(gap_sm())
                                                    .child(
                                                        div()
                                                            .w(px(6.0))
                                                            .h(px(6.0))
                                                            .rounded_full()
                                                            .bg(status_col),
                                                    )
                                                    .child(
                                                        div()
                                                            .font_family(font_family_sans())
                                                            .text_size(font_size_body())
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .text_color(color_text_primary())
                                                            .child(p.name.clone()),
                                                    )
                                                    .child(
                                                        div()
                                                            .font_family(font_family_sans())
                                                            .text_size(font_size_caption())
                                                            .text_color(color_text_muted())
                                                            .child(format!("@{}", p.agent)),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_row()
                                                    .items_center()
                                                    .gap(gap_md())
                                                    .child(
                                                        div()
                                                            .font_family(font_family_sans())
                                                            .text_size(font_size_caption())
                                                            .text_color(color_text_muted())
                                                            .child(format!(
                                                                "{} turns",
                                                                p.turns_used
                                                            )),
                                                    )
                                                    .child(
                                                        div()
                                                            .px(padding_sm())
                                                            .py(padding_xs())
                                                            .rounded(radius_xs())
                                                            .bg(color_surface())
                                                            .border_1()
                                                            .border_color(status_col)
                                                            .font_family(font_family_sans())
                                                            .text_size(font_size_caption())
                                                            .font_weight(FontWeight::BOLD)
                                                            .text_color(status_col)
                                                            .child(p.status.clone()),
                                                    ),
                                            )
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w(px(380.0))
                            .child(
                                div()
                                    .px(padding_md())
                                    .py(padding_sm())
                                    .bg(color_surface())
                                    .border_b_1()
                                    .border_color(color_border())
                                    .child(
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(color_text_muted())
                                            .child(format!(
                                                "ACTIVITY TRACER ({} EVENTS)",
                                                self.snapshot.activity_log.len()
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .id("dashboard-activity-scroll")
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.activity_scroll_handle)
                                    .children(if self.snapshot.activity_log.is_empty() {
                                        vec![
                                            div()
                                                .px(padding_md())
                                                .py(padding_sm())
                                                .font_family(font_family_sans())
                                                .text_size(font_size_body())
                                                .text_color(color_text_muted())
                                                .child("No activity recorded yet. Swarm is idle."),
                                        ]
                                    } else {
                                        self.snapshot
                                            .activity_log
                                            .iter()
                                            .map(|entry| {
                                                div()
                                                    .px(padding_md())
                                                    .py(padding_xs())
                                                    .border_b_1()
                                                    .border_color(color_border_subtle())
                                                    .font_family(font_family_sans())
                                                    .text_size(font_size_caption())
                                                    .text_color(
                                                        if entry.contains("ERROR")
                                                            || entry.contains("failed")
                                                        {
                                                            color_danger_red()
                                                        } else if entry.contains("done")
                                                            || entry.contains("approved")
                                                        {
                                                            color_success_green()
                                                        } else {
                                                            color_text_muted()
                                                        },
                                                    )
                                                    .child(entry.clone())
                                            })
                                            .collect()
                                    }),
                            ),
                    ),
            )
    }

    fn render_acp_servers(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let servers = &self.snapshot.acp_servers;
        let selected_idx = self.snapshot.acp_selected_server;
        let selected_server = servers.get(selected_idx);

        div()
            .flex()
            .flex_row()
            .flex_1()
            .size_full()
            .bg(color_bg_base())
            .font_family(font_family_sans())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(340.0))
                    .h_full()
                    .border_r_1()
                    .border_color(color_border())
                    .child(
                        div()
                            .px(padding_md())
                            .py(padding_sm())
                            .bg(color_surface())
                            .border_b_1()
                            .border_color(color_border())
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_muted())
                                    .child(format!("ACP SERVERS ({})", servers.len())),
                            ),
                    )
                    .child(
                        div()
                            .id("servers-pool-scroll")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .overflow_y_scroll()
                            .children(servers.iter().enumerate().map(|(idx, s)| {
                                let is_active = idx == selected_idx;
                                let status_col = status_color(&s.status);
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .px(padding_md())
                                    .py(padding_sm())
                                    .bg(if is_active {
                                        color_surface()
                                    } else {
                                        color_bg_base()
                                    })
                                    .border_b_1()
                                    .border_color(color_border_subtle())
                                    .border_l_2()
                                    .border_color(if is_active {
                                        color_accent_blue()
                                    } else {
                                        color_bg_base()
                                    })
                                    .hover(|s| s.bg(color_surface_hover()))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, _| {
                                            this.snapshot.acp_selected_server = idx;
                                            this.send_command(ClientCommand::SelectAcpServer {
                                                server_index: idx,
                                            });
                                        }),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .child(
                                                div()
                                                    .font_family(font_family_sans())
                                                    .text_size(font_size_body())
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(color_text_primary())
                                                    .child(s.name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .font_family(font_family_sans())
                                                    .text_size(font_size_caption())
                                                    .text_color(color_text_muted())
                                                    .child(s.id.clone()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .px(padding_sm())
                                            .py(padding_xs())
                                            .rounded(radius_xs())
                                            .bg(color_surface())
                                            .border_1()
                                            .border_color(status_col)
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(status_col)
                                            .child(s.status.clone()),
                                    )
                            })),
                    ),
            )
            .child(div().flex().flex_col().flex_1().h_full().child(
                if let Some(s) = selected_server {
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .px(padding_lg())
                                .py(padding_md())
                                .bg(color_surface())
                                .border_b_1()
                                .border_color(color_border())
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .font_family(font_family_sans())
                                                .text_size(font_size_header())
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(color_text_primary())
                                                .child(s.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .font_family(font_family_sans())
                                                .text_size(font_size_caption())
                                                .text_color(color_text_muted())
                                                .child(format!(
                                                    "Transport: {} | Protocol: {}",
                                                    s.transport, s.protocol
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .px(padding_md())
                                        .py(padding_xs())
                                        .rounded(radius_xs())
                                        .bg(color_bg_base())
                                        .border_1()
                                        .border_color(color_border())
                                        .font_family(font_family_sans())
                                        .text_size(font_size_caption())
                                        .text_color(color_text_muted())
                                        .child(format!("Ping: {}ms", s.ping_ms)),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .font_family(font_family_sans())
                                        .text_size(font_size_caption())
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(color_text_muted())
                                        .px(padding_md())
                                        .py(padding_sm())
                                        .bg(color_surface())
                                        .border_b_1()
                                        .border_color(color_border_subtle())
                                        .child("COMMAND LINE"),
                                )
                                .child(
                                    div()
                                        .px(padding_md())
                                        .py(padding_sm())
                                        .bg(color_bg_base())
                                        .border_b_1()
                                        .border_color(color_border())
                                        .font_family(font_family_sans())
                                        .text_size(font_size_body())
                                        .text_color(color_accent_blue())
                                        .child(if s.command_line.is_empty() {
                                            "Internal In-Memory Agent".to_string()
                                        } else {
                                            s.command_line.clone()
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .font_family(font_family_sans())
                                        .text_size(font_size_caption())
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(color_text_muted())
                                        .px(padding_md())
                                        .py(padding_sm())
                                        .bg(color_surface())
                                        .border_b_1()
                                        .border_color(color_border_subtle())
                                        .child("CAPABILITIES"),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .gap(gap_xs())
                                        .px(padding_md())
                                        .py(padding_sm())
                                        .bg(color_bg_base())
                                        .border_b_1()
                                        .border_color(color_border())
                                        .children(if s.capabilities.is_empty() {
                                            vec![
                                                div()
                                                    .font_family(font_family_sans())
                                                    .text_size(font_size_caption())
                                                    .text_color(color_text_muted())
                                                    .child("Default tool execution"),
                                            ]
                                        } else {
                                            s.capabilities
                                                .iter()
                                                .map(|c| {
                                                    div()
                                                        .px(padding_sm())
                                                        .py(padding_xs())
                                                        .rounded(radius_xs())
                                                        .bg(color_surface())
                                                        .border_1()
                                                        .border_color(color_border())
                                                        .font_family(font_family_sans())
                                                        .text_size(font_size_caption())
                                                        .text_color(color_text_primary())
                                                        .child(c.clone())
                                                })
                                                .collect()
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .child(
                                    div()
                                        .font_family(font_family_sans())
                                        .text_size(font_size_caption())
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(color_text_muted())
                                        .px(padding_md())
                                        .py(padding_sm())
                                        .bg(color_surface())
                                        .border_b_1()
                                        .border_color(color_border_subtle())
                                        .child("RECENT SERVER LOGS"),
                                )
                                .child(
                                    div()
                                        .id("server-logs-scroll")
                                        .flex()
                                        .flex_col()
                                        .flex_1()
                                        .overflow_y_scroll()
                                        .children(if s.recent_logs.is_empty() {
                                            vec![
                                                div()
                                                    .px(padding_md())
                                                    .py(padding_sm())
                                                    .font_family(font_family_sans())
                                                    .text_size(font_size_caption())
                                                    .text_color(color_text_muted())
                                                    .child("No recent logs for this server."),
                                            ]
                                        } else {
                                            s.recent_logs
                                                .iter()
                                                .map(|log| {
                                                    div()
                                                        .px(padding_md())
                                                        .py(padding_xs())
                                                        .border_b_1()
                                                        .border_color(color_border_subtle())
                                                        .font_family(font_family_sans())
                                                        .text_size(font_size_caption())
                                                        .text_color(color_text_muted())
                                                        .child(log.clone())
                                                })
                                                .collect()
                                        }),
                                ),
                        )
                } else {
                    div()
                        .p(padding_lg())
                        .font_family(font_family_sans())
                        .text_size(font_size_body())
                        .text_color(color_text_muted())
                        .child("Select an ACP server to view configuration and status.")
                },
            ))
    }

    fn render_empty_column_card(text: &'static str) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .py(padding_xl())
            .border_1()
            .border_color(color_border_subtle())
            .bg(color_surface())
            .rounded(radius_xs())
            .child(
                div()
                    .font_family(font_family_sans())
                    .text_size(font_size_caption())
                    .text_color(color_text_muted())
                    .child(text),
            )
            .into_any_element()
    }

    fn render_kanban_column(
        title: &'static str,
        count: usize,
        accent: Rgba,
        cards: Vec<AnyElement>,
        scroll_id: &'static str,
        scroll_handle: &ScrollHandle,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .border_r_1()
            .border_color(color_border())
            .bg(color_bg_base())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px(padding_md())
                    .py(padding_sm())
                    .bg(color_surface())
                    .border_b_1()
                    .border_color(color_border())
                    .border_l_2()
                    .border_color(accent)
                    .child(
                        div()
                            .font_family(font_family_sans())
                            .text_size(font_size_body())
                            .font_weight(FontWeight::BOLD)
                            .text_color(color_text_primary())
                            .child(title),
                    )
                    .child(
                        div()
                            .font_family(font_family_sans())
                            .text_size(font_size_caption())
                            .font_weight(FontWeight::BOLD)
                            .px(padding_xs())
                            .py(px(1.0))
                            .bg(color_bg_base())
                            .border_1()
                            .border_color(color_border())
                            .rounded(radius_xs())
                            .text_color(accent)
                            .child(format!("{count}")),
                    ),
            )
            .child(
                div()
                    .id(scroll_id)
                    .overflow_y_scroll()
                    .track_scroll(scroll_handle)
                    .flex()
                    .flex_col()
                    .flex_1()
                    .p(padding_sm())
                    .gap(gap_sm())
                    .children(cards),
            )
    }

    fn render_kanban_inspector(
        &self,
        selected_key: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (title, sub, full_text, agent_name) = if selected_key.starts_with("task-") {
            let task_id: u64 = selected_key
                .trim_start_matches("task-")
                .parse()
                .unwrap_or(0);
            if let Some(t) = self.snapshot.tasks.iter().find(|t| t.id == task_id) {
                (
                    format!("#TASK-{:03} Directive", t.id),
                    format!("Route: {} -> {} | Status: {}", t.from, t.to, t.status),
                    t.prompt.clone(),
                    Some(t.to.clone()),
                )
            } else {
                (
                    "Task Detail".to_string(),
                    "".to_string(),
                    "Task not found".to_string(),
                    None,
                )
            }
        } else if selected_key.starts_with("job-") {
            let name = selected_key.trim_start_matches("job-");
            if let Some(p) = self.snapshot.swarm_panes.iter().find(|p| p.name == name) {
                let thought = p.thinking_lines.last().cloned().unwrap_or_default();
                let output = p.output_lines.last().cloned().unwrap_or_default();
                let text = if !thought.is_empty() {
                    format!(
                        "Role Contract:\n{}\n\nRecent Thought Stream:\n{}",
                        p.task, thought
                    )
                } else if !output.is_empty() {
                    format!("Role Contract:\n{}\n\nLast Output:\n{}", p.task, output)
                } else {
                    format!("Role Contract:\n{}", p.task)
                };
                (
                    format!("Agent Worker: {}", p.name),
                    format!(
                        "Agent: {} | Model: {} | Status: {} | Phase: {} | Tokens: {}",
                        p.agent, p.model, p.status, p.phase, p.tokens_used
                    ),
                    text,
                    Some(p.name.clone()),
                )
            } else {
                (
                    "Job Detail".to_string(),
                    "".to_string(),
                    "Agent not found".to_string(),
                    None,
                )
            }
        } else {
            (
                "Work Item Detail".to_string(),
                "".to_string(),
                "".to_string(),
                None,
            )
        };

        div()
            .flex()
            .flex_col()
            .w_full()
            .h(px(140.0))
            .bg(color_surface())
            .border_t_1()
            .border_color(color_border())
            .p(padding_md())
            .gap(gap_xs())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(gap_sm())
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_body())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_accent_blue())
                                    .child(title),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .text_color(color_text_muted())
                                    .child(sub),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(gap_sm())
                            .children(agent_name.map(|target_name| {
                                div()
                                    .px(padding_sm())
                                    .py(px(2.0))
                                    .bg(color_accent_blue())
                                    .rounded(radius_xs())
                                    .font_family(font_family_sans())
                                    .text_size(px(10.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_bg_base())
                                    .cursor_pointer()
                                    .child("OPEN IN SWARM COCKPIT [O]")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            if let Some(target_idx) =
                                                this.find_pane_by_name(&target_name)
                                            {
                                                this.snapshot.swarm_selected_pane = target_idx;
                                                this.snapshot.selected_tab = 2;
                                                this.composer_focused =
                                                    this.is_orchestrator_pane(target_idx);
                                                this.send_command(ClientCommand::SwitchTab {
                                                    tab_index: 2,
                                                });
                                                this.send_command(ClientCommand::SelectPane {
                                                    pane_index: target_idx,
                                                });
                                                cx.notify();
                                            }
                                        }),
                                    )
                            }))
                            .child(
                                div()
                                    .px(padding_sm())
                                    .py(px(2.0))
                                    .bg(color_surface_hover())
                                    .border_1()
                                    .border_color(color_border())
                                    .rounded(radius_xs())
                                    .font_family(font_family_sans())
                                    .text_size(px(10.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_muted())
                                    .cursor_pointer()
                                    .hover(|s| s.text_color(color_text_primary()))
                                    .child("CLOSE [ESC]")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.kanban_selected_item = None;
                                            cx.notify();
                                        }),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .id("kanban-inspector-body")
                    .flex_1()
                    .overflow_y_scroll()
                    .font_family(font_family_sans())
                    .text_size(font_size_caption())
                    .text_color(color_text_primary())
                    .line_height(relative(1.4))
                    .child(full_text),
            )
    }

    fn render_kanban_tracker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let panes = &self.snapshot.swarm_panes;
        let tasks = &self.snapshot.tasks;
        let filter = self.kanban_filter;

        let mut col0_items: Vec<AnyElement> = Vec::new();
        let mut col1_items: Vec<AnyElement> = Vec::new();
        let mut col2_items: Vec<AnyElement> = Vec::new();
        let mut col3_items: Vec<AnyElement> = Vec::new();

        // 1. Process active permission modal if present (goes to Col 3)
        if let Some(perm) = &self.snapshot.active_permission {
            let req_id_approve = perm.request_id.clone();
            let req_id_deny = perm.request_id.clone();
            let seconds_left = perm.seconds_left;
            let agent_id = perm.agent_id.clone();
            let tool = perm.tool.clone();
            let params = perm.params.clone();

            col3_items.push(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .bg(color_surface())
                    .border_1()
                    .border_color(color_danger_red())
                    .border_l_2()
                    .border_color(color_danger_red())
                    .p(padding_sm())
                    .gap(gap_xs())
                    .rounded(radius_xs())
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_danger_red())
                                    .child("ACTION REQUIRED"),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(px(10.0))
                                    .text_color(color_warning_amber())
                                    .child(format!("{seconds_left}s left")),
                            ),
                    )
                    .child(
                        div()
                            .font_family(font_family_sans())
                            .text_size(font_size_caption())
                            .font_weight(FontWeight::BOLD)
                            .text_color(color_text_primary())
                            .child(format!("Agent: {agent_id}")),
                    )
                    .child(
                        div()
                            .font_family(font_family_sans())
                            .text_size(font_size_caption())
                            .text_color(color_text_muted())
                            .child(format!("Tool: {tool}")),
                    )
                    .child(
                        div()
                            .font_family(font_family_sans())
                            .text_size(px(10.0))
                            .text_color(color_text_muted())
                            .child(format!("Params: {params}")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(gap_sm())
                            .pt(px(4.0))
                            .child(
                                div()
                                    .px(padding_sm())
                                    .py(px(2.0))
                                    .bg(color_success_green())
                                    .rounded(radius_xs())
                                    .font_family(font_family_sans())
                                    .text_size(px(10.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_bg_base())
                                    .cursor_pointer()
                                    .child("APPROVE [Y]")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            this.send_command(ClientCommand::ResolvePermission {
                                                request_id: req_id_approve.clone(),
                                                allowed: true,
                                            });
                                            cx.notify();
                                        }),
                                    ),
                            )
                            .child(
                                div()
                                    .px(padding_sm())
                                    .py(px(2.0))
                                    .bg(color_danger_red())
                                    .rounded(radius_xs())
                                    .font_family(font_family_sans())
                                    .text_size(px(10.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_primary())
                                    .cursor_pointer()
                                    .child("DENY [ESC]")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            this.send_command(ClientCommand::ResolvePermission {
                                                request_id: req_id_deny.clone(),
                                                allowed: false,
                                            });
                                            cx.notify();
                                        }),
                                    ),
                            ),
                    )
                    .into_any_element(),
            );
        }

        // 2. Process Dispatched Directive Tasks
        if filter == 0 || filter == 1 {
            for t in tasks {
                let task_id = t.id;
                let from = t.from.clone();
                let to = t.to.clone();
                let prompt = t.prompt.clone();
                let status_raw = t.status.clone();
                let task_key = format!("task-{}", task_id);
                let is_selected = self.kanban_selected_item.as_deref() == Some(&task_key);

                let is_target_active = self
                    .find_pane_by_name(&to)
                    .and_then(|idx| panes.get(idx))
                    .map(|p| p.status.to_uppercase() == "RUNNING" || p.phase != "idle")
                    .unwrap_or(false);

                let col_target = match status_raw.to_lowercase().as_str() {
                    "done" => 2,
                    "failed" => 3,
                    _ => {
                        if is_target_active {
                            1
                        } else {
                            0
                        }
                    }
                };

                let col_accent = match col_target {
                    0 => color_warning_amber(),
                    1 => color_accent_blue(),
                    2 => color_success_green(),
                    _ => color_danger_red(),
                };

                let status_label = match col_target {
                    0 => "QUEUED",
                    1 => "IN PROGRESS",
                    2 => "DONE",
                    _ => "FAILED",
                };

                let target_name = to.clone();
                let prompt_preview = if prompt.len() > 140 {
                    format!("{}...", &prompt[..140])
                } else {
                    prompt.clone()
                };

                let card_key_clone = task_key.clone();
                let card = div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .bg(if is_selected {
                        color_surface_hover()
                    } else {
                        color_surface()
                    })
                    .border_1()
                    .border_color(if is_selected {
                        color_accent_blue()
                    } else {
                        color_border()
                    })
                    .border_l_2()
                    .border_color(col_accent)
                    .p(padding_sm())
                    .gap(gap_xs())
                    .rounded(radius_xs())
                    .hover(|s| s.bg(color_surface_hover()))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.kanban_selected_item = Some(card_key_clone.clone());
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(gap_xs())
                                    .child(
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(color_accent_blue())
                                            .child(format!("#TASK-{:03}", task_id)),
                                    )
                                    .child(
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(px(10.0))
                                            .px(px(4.0))
                                            .py(px(1.0))
                                            .bg(color_bg_base())
                                            .border_1()
                                            .border_color(color_border_subtle())
                                            .rounded(radius_xs())
                                            .text_color(color_text_muted())
                                            .child("DIRECTIVE"),
                                    ),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(px(10.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(col_accent)
                                    .child(status_label),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(gap_xs())
                            .font_family(font_family_sans())
                            .text_size(font_size_caption())
                            .text_color(color_text_muted())
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(color_text_primary())
                                    .child(from),
                            )
                            .child(div().child("->"))
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_accent_blue())
                                    .child(to),
                            ),
                    )
                    .child(
                        div()
                            .font_family(font_family_sans())
                            .text_size(font_size_caption())
                            .text_color(color_text_primary())
                            .line_height(relative(1.3))
                            .child(prompt_preview),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .pt(px(2.0))
                            .border_t_1()
                            .border_color(color_border_subtle())
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(px(10.0))
                                    .text_color(color_text_muted())
                                    .child(if is_target_active {
                                        "Worker executing"
                                    } else {
                                        "Standby queue"
                                    }),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(px(10.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_accent_blue())
                                    .hover(|s| s.text_color(color_text_primary()))
                                    .child("OPEN IN COCKPIT ->")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            if let Some(p_idx) =
                                                this.find_pane_by_name(&target_name)
                                            {
                                                this.snapshot.swarm_selected_pane = p_idx;
                                                this.snapshot.selected_tab = 2;
                                                this.composer_focused =
                                                    this.is_orchestrator_pane(p_idx);
                                                this.send_command(ClientCommand::SwitchTab {
                                                    tab_index: 2,
                                                });
                                                this.send_command(ClientCommand::SelectPane {
                                                    pane_index: p_idx,
                                                });
                                                cx.notify();
                                            }
                                        }),
                                    ),
                            ),
                    )
                    .into_any_element();

                match col_target {
                    0 => col0_items.push(card),
                    1 => col1_items.push(card),
                    2 => col2_items.push(card),
                    _ => col3_items.push(card),
                }
            }
        }

        // 3. Process Swarm Agent Worker Jobs
        if filter == 0 || filter == 2 {
            for (p_idx, p) in panes.iter().enumerate() {
                let pane_name = p.name.clone();
                let job_key = format!("job-{}", pane_name);
                let is_selected = self.kanban_selected_item.as_deref() == Some(&job_key);
                let status_up = p.status.to_uppercase();
                let is_running = status_up == "RUNNING" || p.phase != "idle";
                let is_err = status_up == "ERROR" || status_up == "FAILED";
                let is_done = (status_up == "ONLINE" || status_up == "DONE") && !is_running;

                let col_target = if is_running {
                    1
                } else if is_err {
                    3
                } else if is_done {
                    2
                } else {
                    0
                };

                let col_accent = match col_target {
                    0 => color_warning_amber(),
                    1 => color_accent_blue(),
                    2 => color_success_green(),
                    _ => color_danger_red(),
                };

                let task_preview = if p.task.len() > 120 {
                    format!("{}...", &p.task[..120])
                } else {
                    p.task.clone()
                };

                let card_key_clone = job_key.clone();
                let card =
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .bg(if is_selected {
                            color_surface_hover()
                        } else {
                            color_surface()
                        })
                        .border_1()
                        .border_color(if is_selected {
                            color_accent_blue()
                        } else {
                            color_border()
                        })
                        .border_l_2()
                        .border_color(col_accent)
                        .p(padding_sm())
                        .gap(gap_xs())
                        .rounded(radius_xs())
                        .hover(|s| s.bg(color_surface_hover()))
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.kanban_selected_item = Some(card_key_clone.clone());
                                cx.notify();
                            }),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(
                                    div().flex().flex_row().items_center().gap(gap_xs()).child(
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(color_text_primary())
                                            .child(p.name.clone()),
                                    ),
                                )
                                .child(
                                    div()
                                        .font_family(font_family_sans())
                                        .text_size(px(10.0))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(col_accent)
                                        .child(if is_running {
                                            "ACTIVE JOB"
                                        } else if is_err {
                                            "ERROR"
                                        } else if is_done {
                                            "READY"
                                        } else {
                                            "STANDBY"
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .font_family(font_family_sans())
                                .text_size(px(10.0))
                                .text_color(color_text_muted())
                                .child(
                                    div()
                                        .text_color(if p.phase != "idle" {
                                            color_warning_amber()
                                        } else {
                                            color_text_muted()
                                        })
                                        .child(format!("Phase: {}", p.phase)),
                                )
                                .child(div().child(format!(
                                    "{} tok | {} turns",
                                    p.tokens_used, p.turns_used
                                ))),
                        )
                        .child(
                            div()
                                .font_family(font_family_sans())
                                .text_size(font_size_caption())
                                .text_color(color_text_muted())
                                .line_height(relative(1.3))
                                .child(task_preview),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .pt(px(2.0))
                                .border_t_1()
                                .border_color(color_border_subtle())
                                .child(
                                    div()
                                        .font_family(font_family_sans())
                                        .text_size(px(10.0))
                                        .text_color(color_text_muted())
                                        .child(format!("{} | {}", p.agent, p.model)),
                                )
                                .child(
                                    div()
                                        .font_family(font_family_sans())
                                        .text_size(px(10.0))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(color_accent_blue())
                                        .hover(|s| s.text_color(color_text_primary()))
                                        .child("INSPECT [O]")
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |this, _, _, cx| {
                                                this.snapshot.swarm_selected_pane = p_idx;
                                                this.snapshot.selected_tab = 2;
                                                this.composer_focused =
                                                    this.is_orchestrator_pane(p_idx);
                                                this.send_command(ClientCommand::SwitchTab {
                                                    tab_index: 2,
                                                });
                                                this.send_command(ClientCommand::SelectPane {
                                                    pane_index: p_idx,
                                                });
                                                cx.notify();
                                            }),
                                        ),
                                ),
                        )
                        .into_any_element();

                match col_target {
                    0 => col0_items.push(card),
                    1 => col1_items.push(card),
                    2 => col2_items.push(card),
                    _ => col3_items.push(card),
                }
            }
        }

        let c0_len = col0_items.len();
        let c1_len = col1_items.len();
        let c2_len = col2_items.len();
        let c3_len = col3_items.len();
        let total_work = c0_len + c1_len + c2_len + c3_len;

        if col0_items.is_empty() {
            col0_items.push(Self::render_empty_column_card("No queued work in backlog"));
        }
        if col1_items.is_empty() {
            col1_items.push(Self::render_empty_column_card(
                "No agents currently executing",
            ));
        }
        if col2_items.is_empty() {
            col2_items.push(Self::render_empty_column_card("No completed work yet"));
        }
        if col3_items.is_empty() {
            col3_items.push(Self::render_empty_column_card(
                "Zero errors or blocked tasks",
            ));
        }

        let inspector_opt = self
            .kanban_selected_item
            .as_ref()
            .map(|sel| self.render_kanban_inspector(sel, cx).into_any_element());

        div()
            .flex()
            .flex_col()
            .flex_1()
            .size_full()
            .bg(color_bg_base())
            .font_family(font_family_sans())
            .child(
                // Header Bar with Filter controls
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px(padding_lg())
                    .py(padding_md())
                    .border_b_1()
                    .border_color(color_border())
                    .bg(color_surface())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_header())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_primary())
                                    .child("SWARM KANBAN & JOB TRACKER"),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .text_color(color_text_muted())
                                    .child("Real-time task orchestration, execution streams, and inter-agent directives"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(gap_sm())
                            .children([
                                ("ALL ITEMS", 0),
                                ("DIRECTIVE TASKS", 1),
                                ("AGENT WORKER JOBS", 2),
                            ].iter().map(|(label, fid)| {
                                let is_active = self.kanban_filter == *fid;
                                let filter_id = *fid;
                                div()
                                    .px(padding_sm())
                                    .py(padding_xs())
                                    .bg(if is_active { color_surface_hover() } else { color_bg_base() })
                                    .border_1()
                                    .border_color(if is_active { color_accent_blue() } else { color_border() })
                                    .border_l_2()
                                    .border_color(if is_active { color_accent_blue() } else { color_border() })
                                    .rounded(radius_xs())
                                    .cursor_pointer()
                                    .hover(|s| s.bg(color_surface_hover()))
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(if is_active { FontWeight::BOLD } else { FontWeight::MEDIUM })
                                    .text_color(if is_active { color_accent_blue() } else { color_text_primary() })
                                    .child(*label)
                                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                                        this.kanban_filter = filter_id;
                                        cx.notify();
                                    }))
                            })),
                    ),
            )
            .child(
                // Metrics Banner
                div()
                    .flex()
                    .flex_row()
                    .w_full()
                    .border_b_1()
                    .border_color(color_border())
                    .bg(color_bg_base())
                    .child(
                        div()
                            .flex_1()
                            .p(padding_md())
                            .border_r_1()
                            .border_color(color_border())
                            .gap(gap_xs())
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_muted())
                                    .child("TOTAL WORK ITEMS"),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_header())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_primary())
                                    .child(format!("{total_work}")),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .p(padding_md())
                            .border_r_1()
                            .border_color(color_border())
                            .gap(gap_xs())
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_muted())
                                    .child("BACKLOG & QUEUED"),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_header())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_warning_amber())
                                    .child(format!("{c0_len}")),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .p(padding_md())
                            .border_r_1()
                            .border_color(color_border())
                            .gap(gap_xs())
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_muted())
                                    .child("IN PROGRESS"),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_header())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_accent_blue())
                                    .child(format!("{c1_len}")),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .p(padding_md())
                            .border_r_1()
                            .border_color(color_border())
                            .gap(gap_xs())
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_muted())
                                    .child("COMPLETED"),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_header())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_success_green())
                                    .child(format!("{c2_len}")),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .p(padding_md())
                            .gap(gap_xs())
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_muted())
                                    .child("BLOCKED / FAILED"),
                            )
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_header())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(if c3_len > 0 { color_danger_red() } else { color_text_muted() })
                                    .child(format!("{c3_len}")),
                            ),
                    ),
            )
            .child(
                // 4 Kanban Columns
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .w_full()
                    .overflow_hidden()
                    .child(Self::render_kanban_column("BACKLOG & QUEUED", c0_len, color_warning_amber(), col0_items, "kanban-col-0", &self.kanban_col0_scroll_handle))
                    .child(Self::render_kanban_column("IN PROGRESS", c1_len, color_accent_blue(), col1_items, "kanban-col-1", &self.kanban_col1_scroll_handle))
                    .child(Self::render_kanban_column("COMPLETED", c2_len, color_success_green(), col2_items, "kanban-col-2", &self.kanban_col2_scroll_handle))
                    .child(Self::render_kanban_column("BLOCKED / FAILED", c3_len, color_danger_red(), col3_items, "kanban-col-3", &self.kanban_col3_scroll_handle)),
            )
            .children(inspector_opt)
    }
}

use crate::gui::input::key_to_char;

impl Render for CockpitApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus_handle.is_focused(window) {
            window.focus(&self.focus_handle);
        }

        let perm_opt = self.snapshot.active_permission.clone();
        let selected_tab = self.snapshot.selected_tab;

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(color_bg_base())
            .font_family(font_family_sans())
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
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

                let is_orch = this.is_orchestrator_pane(this.snapshot.swarm_selected_pane);
                let in_cockpit = this.snapshot.selected_tab == 2;
                let key_name = event.keystroke.key.as_str();

                // 2. Active Orchestrator Composer Input:
                if in_cockpit && is_orch && this.composer_focused {
                    // Clipboard paste (Ctrl+V / Cmd+V)
                    if (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
                        && (key_name == "v" || key_name == "V")
                    {
                        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                            let clean = text
                                .replace("\r\n", " ")
                                .replace('\n', " ")
                                .replace('\r', " ");
                            this.composer_input.push_str(&clean);
                            cx.notify();
                            return;
                        }
                    }

                    // Clipboard copy (Ctrl+C / Cmd+C)
                    if (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
                        && (key_name == "c" || key_name == "C")
                    {
                        if !this.composer_input.is_empty() {
                            cx.write_to_clipboard(ClipboardItem::new_string(
                                this.composer_input.clone(),
                            ));
                            return;
                        }
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
                            if event.keystroke.modifiers.control
                                || event.keystroke.modifiers.platform
                            {
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
                        if next == 2 && this.is_orchestrator_pane(this.snapshot.swarm_selected_pane)
                        {
                            this.composer_focused = true;
                        } else {
                            this.composer_focused = false;
                        }
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
            }))
            .child(self.render_header(cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .size_full()
                    .child(self.render_sidebar(cx))
                    .child(match selected_tab {
                        0 => self.render_dashboard(cx).into_any_element(),
                        1 => self.render_acp_servers(cx).into_any_element(),
                        2 => self.render_swarm_cockpit(cx).into_any_element(),
                        _ => self.render_kanban_tracker(cx).into_any_element(),
                    }),
            )
            .child(if let Some(perm) = &perm_opt {
                self.render_permission_modal(perm, cx).into_any_element()
            } else {
                div().into_any_element()
            })
    }
}

pub fn run_desktop(pipe_name: String) {
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
        let pipe_clone = pipe_name.clone();

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Locust Cockpit".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| CockpitApp::new(pipe_clone, cx)),
        )
        .unwrap();
    });
}
