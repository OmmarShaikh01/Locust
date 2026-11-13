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
    cmd_tx: Option<Sender<ClientCommand>>,
    focus_handle: FocusHandle,
}

impl CockpitApp {
    pub fn new(pipe_name: String, cx: &mut Context<Self>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ClientCommand>();
        let (event_tx, event_rx) = flume::unbounded::<ServerEvent>();

        let app = Self {
            snapshot: ModelSnapshot::default(),
            connected: false,
            composer_input: String::new(),
            cmd_tx: Some(cmd_tx),
            focus_handle: cx.focus_handle(),
        };

        cx.spawn(async move |entity, app| {
            while let Ok(event) = event_rx.recv_async().await {
                let _ = entity.update(app, |this: &mut CockpitApp, cx| {
                    match event {
                        ServerEvent::InitialSync {
                            state,
                            server_version: _,
                        } => {
                            this.snapshot = *state;
                            this.connected = true;
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
                    }
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

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
        let nav_tabs = ["0 Dashboard", "1 ACP Servers", "2 Swarm Cockpit"];

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .w_full()
            .h(px(52.0))
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
                            .rounded(radius_md())
                            .bg(color_surface())
                            .border_1()
                            .border_color(color_border())
                            .font_family(font_family_sans())
                            .text_size(font_size_caption())
                            .text_color(color_text_muted())
                            .child(format!("Project: {}", self.snapshot.project_path)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(gap_xs())
                    .children(nav_tabs.iter().enumerate().map(|(idx, title)| {
                        let is_active = idx == self.snapshot.selected_tab;
                        div()
                            .px(padding_md())
                            .py(padding_xs())
                            .rounded_full()
                            .bg(if is_active {
                                color_surface_hover()
                            } else {
                                color_surface()
                            })
                            .border_1()
                            .border_color(if is_active {
                                color_accent_blue()
                            } else {
                                color_border()
                            })
                            .cursor_pointer()
                            .font_family(font_family_sans())
                            .text_size(font_size_caption())
                            .font_weight(if is_active {
                                FontWeight::BOLD
                            } else {
                                FontWeight::MEDIUM
                            })
                            .text_color(if is_active {
                                color_accent_blue()
                            } else {
                                color_text_primary()
                            })
                            .child(*title)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, _| {
                                    this.snapshot.selected_tab = idx;
                                    this.send_command(ClientCommand::SwitchTab { tab_index: idx });
                                }),
                            )
                    })),
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
                            .rounded_full()
                            .bg(color_surface())
                            .border_1()
                            .border_color(color_border())
                            .child(
                                div()
                                    .w(px(8.0))
                                    .h(px(8.0))
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
                                        "Tokens: {} / {} Cap",
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
                            .rounded_full()
                            .bg(color_surface())
                            .border_1()
                            .border_color(color_border())
                            .child(
                                div()
                                    .w(px(8.0))
                                    .h(px(8.0))
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
        let tabs = ["0 Dashboard", "1 ACP Servers", "2 Swarm Cockpit"];
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
            .p(padding_md())
            .gap(gap_lg())
            .font_family(font_family_sans())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(gap_xs())
                    .child(
                        div()
                            .font_family(font_family_sans())
                            .text_size(font_size_caption())
                            .font_weight(FontWeight::BOLD)
                            .text_color(color_text_muted())
                            .px(padding_sm())
                            .py(padding_xs())
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
                            .rounded(radius_md())
                            .bg(bg)
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
                                cx.listener(move |this, _, _, _| {
                                    this.snapshot.selected_tab = idx;
                                    this.send_command(ClientCommand::SwitchTab { tab_index: idx });
                                }),
                            )
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(gap_xs())
                    .child(
                        div()
                            .font_family(font_family_sans())
                            .text_size(font_size_caption())
                            .font_weight(FontWeight::BOLD)
                            .text_color(color_text_muted())
                            .px(padding_sm())
                            .py(padding_xs())
                            .child("SWARM PANES"),
                    )
                    .children(
                        self.snapshot
                            .swarm_panes
                            .iter()
                            .enumerate()
                            .map(|(idx, pane)| {
                                let is_active = idx == selected_pane;
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
                                    .rounded(radius_md())
                                    .bg(bg)
                                    .hover(|s| s.bg(color_surface_hover()))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, _| {
                                            this.snapshot.selected_tab = 2;
                                            this.snapshot.swarm_selected_pane = idx;
                                            this.send_command(ClientCommand::SwitchTab { tab_index: 2 });
                                            this.send_command(ClientCommand::SelectPane {
                                                pane_index: idx,
                                            });
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
                                                    .w(px(8.0))
                                                    .h(px(8.0))
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
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .text_color(color_text_muted())
                                            .child(pane.agent.clone()),
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
                    .p(padding_lg())
                    .border_b_1()
                    .border_color(color_border())
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
                                    .w(px(36.0))
                                    .h(px(36.0))
                                    .rounded_full()
                                    .bg(color_surface())
                                    .border_1()
                                    .border_color(color_border())
                                    .font_family(font_family_sans())
                                    .text_size(font_size_body())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_accent_blue())
                                    .child(
                                        pane.name
                                            .chars()
                                            .next()
                                            .unwrap_or('A')
                                            .to_uppercase()
                                            .to_string(),
                                    ),
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
                                            .child(format!("Model: {}", pane.model)),
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
                                    .rounded_full()
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
                                        "Hide Thinking"
                                    } else {
                                        "Show Thinking"
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
                                    .rounded_full()
                                    .bg(color_surface())
                                    .border_1()
                                    .border_color(status_badge_color)
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(status_badge_color)
                                    .child(pane.status.clone()),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .p(padding_lg())
                    .gap(gap_lg())
                    .child(if show_thinking && !pane.thinking_lines.is_empty() {
                        div()
                            .flex()
                            .flex_col()
                            .p(padding_md())
                            .rounded(radius_md())
                            .bg(color_surface())
                            .border_1()
                            .border_color(color_border())
                            .gap(gap_xs())
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_caption())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_warning_amber())
                                    .child("> Internal Reasoning (Thinking)"),
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
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .p(padding_lg())
                            .rounded(radius_lg())
                            .bg(color_surface())
                            .border_1()
                            .border_color(color_border())
                            .gap(gap_xs())
                            .children(if pane.output_lines.is_empty() {
                                vec![div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_body())
                                    .text_color(color_text_muted())
                                    .child("No output yet. Submit a directive to begin.")]
                            } else {
                                pane.output_lines
                                    .iter()
                                    .map(|line| {
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_body())
                                            .text_color(color_text_primary())
                                            .child(line.clone())
                                    })
                                    .collect()
                            }),
                    ),
            )
    }

    fn render_composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let input_text = self.composer_input.clone();

        div()
            .flex()
            .flex_col()
            .w_full()
            .p(padding_md())
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
                    .px(padding_md())
                    .py(padding_xs())
                    .rounded_full()
                    .bg(color_surface())
                    .border_1()
                    .border_color(color_border())
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
                                "Write directive @[Pane Name]: task or prompt Controller..."
                                    .to_string()
                            } else {
                                input_text.clone()
                            }),
                    )
                    .child(
                        div()
                            .px(padding_lg())
                            .py(padding_sm())
                            .rounded_full()
                            .bg(color_accent_blue())
                            .hover(|s| s.opacity(0.85))
                            .cursor_pointer()
                            .font_family(font_family_sans())
                            .text_size(font_size_body())
                            .font_weight(FontWeight::BOLD)
                            .text_color(color_text_primary())
                            .child("Send Turn")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, _| {
                                    if !input_text.is_empty() {
                                        this.send_command(ClientCommand::SubmitControllerTurn {
                                            text: input_text.clone(),
                                        });
                                    }
                                }),
                            ),
                    ),
            )
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
            .p(padding_md())
            .gap(gap_lg())
            .font_family(font_family_sans())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p(padding_lg())
                    .rounded(radius_lg())
                    .bg(color_surface())
                    .border_1()
                    .border_color(color_border())
                    .gap(gap_md())
                    .child(
                        div()
                            .font_family(font_family_sans())
                            .text_size(font_size_caption())
                            .font_weight(FontWeight::BOLD)
                            .text_color(color_text_muted())
                            .child("TELEMETRY & LIMITS"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
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
                    .p(padding_lg())
                    .rounded(radius_lg())
                    .bg(color_surface())
                    .border_1()
                    .border_color(color_border())
                    .gap(gap_md())
                    .child(
                        div()
                            .font_family(font_family_sans())
                            .text_size(font_size_caption())
                            .font_weight(FontWeight::BOLD)
                            .text_color(color_text_muted())
                            .child("AGENT HANDOFFS"),
                    )
                    .children(if self.snapshot.tasks.is_empty() {
                        vec![div()
                            .font_family(font_family_sans())
                            .text_size(font_size_body())
                            .text_color(color_text_muted())
                            .child("No active handoff tasks.")]
                    } else {
                        self.snapshot
                            .tasks
                            .iter()
                            .map(|task| {
                                let status_col = status_color(&task.status);
                                div()
                                    .flex()
                                    .flex_col()
                                    .p(padding_sm())
                                    .rounded(radius_md())
                                    .bg(color_bg_base())
                                    .border_1()
                                    .border_color(color_border())
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
                                                    .child(format!(
                                                        "{} -> {}",
                                                        task.from, task.to
                                                    )),
                                            )
                                            .child(
                                                div()
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
            .bg(rgba(0x000000aa))
            .font_family(font_family_sans())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(480.0))
                    .p(padding_xl())
                    .rounded(radius_xl())
                    .bg(color_surface())
                    .border_1()
                    .border_color(color_warning_amber())
                    .gap(gap_lg())
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
                            .rounded(radius_md())
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
                            .gap(gap_md())
                            .child(
                                div()
                                    .px(padding_lg())
                                    .py(padding_sm())
                                    .rounded_full()
                                    .bg(color_surface_hover())
                                    .border_1()
                                    .border_color(color_border())
                                    .cursor_pointer()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_body())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_danger_red())
                                    .child("Deny [Esc]")
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
                                    .py(padding_sm())
                                    .rounded_full()
                                    .bg(color_accent_blue())
                                    .cursor_pointer()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_body())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_primary())
                                    .child("Allow Access")
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
            .p(padding_lg())
            .gap(gap_lg())
            .font_family(font_family_sans())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(gap_md())
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .p(padding_md())
                            .rounded(radius_lg())
                            .bg(color_surface())
                            .border_1()
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
                            .rounded(radius_lg())
                            .bg(color_surface())
                            .border_1()
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
                            .rounded(radius_lg())
                            .bg(color_surface())
                            .border_1()
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
                                    .child(format!("{} invocations", self.snapshot.total_invocations)),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .p(padding_md())
                            .rounded(radius_lg())
                            .bg(color_surface())
                            .border_1()
                            .border_color(color_border())
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
                                    .child(format!("{} Configured", self.snapshot.acp_servers.len())),
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
                    .gap(gap_md())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .p(padding_lg())
                            .rounded(radius_lg())
                            .bg(color_surface())
                            .border_1()
                            .border_color(color_border())
                            .gap(gap_sm())
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .justify_between()
                                    .child(
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_title())
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(color_text_primary())
                                            .child("Swarm Agent Matrix"),
                                    )
                                    .child(
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .text_color(color_text_muted())
                                            .child("Click pane to open cockpit"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(gap_xs())
                                    .children(panes.iter().enumerate().map(|(idx, p)| {
                                        let status_col = status_color(&p.status);
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .justify_between()
                                            .p(padding_sm())
                                            .rounded(radius_md())
                                            .bg(color_bg_base())
                                            .border_1()
                                            .border_color(color_border())
                                            .hover(|s| s.bg(color_surface_hover()))
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(move |this, _, _, _| {
                                                    this.snapshot.selected_tab = 2;
                                                    this.snapshot.swarm_selected_pane = idx;
                                                    this.send_command(ClientCommand::SwitchTab {
                                                        tab_index: 2,
                                                    });
                                                    this.send_command(ClientCommand::SelectPane {
                                                        pane_index: idx,
                                                    });
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
                                                            .w(px(8.0))
                                                            .h(px(8.0))
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
                                                            .child(format!("{} turns", p.turns_used)),
                                                    )
                                                    .child(
                                                        div()
                                                            .px(padding_sm())
                                                            .py(padding_xs())
                                                            .rounded_full()
                                                            .bg(color_surface())
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
                            .flex_1()
                            .p(padding_lg())
                            .rounded(radius_lg())
                            .bg(color_surface())
                            .border_1()
                            .border_color(color_border())
                            .gap(gap_sm())
                            .child(
                                div()
                                    .font_family(font_family_sans())
                                    .text_size(font_size_title())
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color_text_primary())
                                    .child(format!(
                                        "Activity Tracer ({} events)",
                                        self.snapshot.activity_log.len()
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(gap_xs())
                                    .children(if self.snapshot.activity_log.is_empty() {
                                        vec![div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_body())
                                            .text_color(color_text_muted())
                                            .child("No activity recorded yet. Swarm is idle.")]
                                    } else {
                                        self.snapshot
                                            .activity_log
                                            .iter()
                                            .map(|entry| {
                                                div()
                                                    .p(padding_xs())
                                                    .font_family(font_family_sans())
                                                    .text_size(font_size_caption())
                                                    .text_color(if entry.contains("ERROR")
                                                        || entry.contains("failed")
                                                    {
                                                        color_danger_red()
                                                    } else if entry.contains("done")
                                                        || entry.contains("approved")
                                                    {
                                                        color_success_green()
                                                    } else {
                                                        color_text_muted()
                                                    })
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
            .p(padding_lg())
            .gap(gap_lg())
            .font_family(font_family_sans())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(380.0))
                    .p(padding_lg())
                    .rounded(radius_lg())
                    .bg(color_surface())
                    .border_1()
                    .border_color(color_border())
                    .gap(gap_md())
                    .child(
                        div()
                            .font_family(font_family_sans())
                            .text_size(font_size_title())
                            .font_weight(FontWeight::BOLD)
                            .text_color(color_text_primary())
                            .child(format!("ACP Servers ({})", servers.len())),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(gap_xs())
                            .children(servers.iter().enumerate().map(|(idx, s)| {
                                let is_active = idx == selected_idx;
                                let status_col = status_color(&s.status);
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .p(padding_md())
                                    .rounded(radius_md())
                                    .bg(if is_active {
                                        color_surface_hover()
                                    } else {
                                        color_bg_base()
                                    })
                                    .border_1()
                                    .border_color(if is_active {
                                        color_accent_blue()
                                    } else {
                                        color_border()
                                    })
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
                                            .rounded_full()
                                            .bg(color_surface())
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(status_col)
                                            .child(s.status.clone()),
                                    )
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .p(padding_lg())
                    .rounded(radius_lg())
                    .bg(color_surface())
                    .border_1()
                    .border_color(color_border())
                    .gap(gap_md())
                    .child(if let Some(s) = selected_server {
                        div()
                            .flex()
                            .flex_col()
                            .gap(gap_md())
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
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
                                    .gap(gap_xs())
                                    .child(
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(color_text_muted())
                                            .child("COMMAND LINE"),
                                    )
                                    .child(
                                        div()
                                            .p(padding_md())
                                            .rounded(radius_md())
                                            .bg(color_bg_base())
                                            .border_1()
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
                                    .gap(gap_xs())
                                    .child(
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(color_text_muted())
                                            .child("CAPABILITIES"),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .gap(gap_xs())
                                            .children(if s.capabilities.is_empty() {
                                                vec![div()
                                                    .font_family(font_family_sans())
                                                    .text_size(font_size_caption())
                                                    .text_color(color_text_muted())
                                                    .child("Default tool execution")]
                                            } else {
                                                s.capabilities
                                                    .iter()
                                                    .map(|c| {
                                                        div()
                                                            .px(padding_sm())
                                                            .py(padding_xs())
                                                            .rounded_full()
                                                            .bg(color_bg_base())
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
                                    .gap(gap_xs())
                                    .child(
                                        div()
                                            .font_family(font_family_sans())
                                            .text_size(font_size_caption())
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(color_text_muted())
                                            .child("RECENT SERVER LOGS"),
                                    )
                                    .child(
                                        div()
                                            .p(padding_md())
                                            .rounded(radius_md())
                                            .bg(color_bg_base())
                                            .border_1()
                                            .border_color(color_border())
                                            .children(if s.recent_logs.is_empty() {
                                                vec![div()
                                                    .font_family(font_family_sans())
                                                    .text_size(font_size_caption())
                                                    .text_color(color_text_muted())
                                                    .child("No recent logs for this server.")]
                                            } else {
                                                s.recent_logs
                                                    .iter()
                                                    .map(|log| {
                                                        div()
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
                            .font_family(font_family_sans())
                            .text_size(font_size_body())
                            .text_color(color_text_muted())
                            .child("Select an ACP server to view configuration and status.")
                    }),
            )
    }
}

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
                match event.keystroke.key.as_str() {
                    "0" => {
                        this.snapshot.selected_tab = 0;
                        this.send_command(ClientCommand::SwitchTab { tab_index: 0 });
                        cx.notify();
                    }
                    "1" => {
                        this.snapshot.selected_tab = 1;
                        this.send_command(ClientCommand::SwitchTab { tab_index: 1 });
                        cx.notify();
                    }
                    "2" => {
                        this.snapshot.selected_tab = 2;
                        this.send_command(ClientCommand::SwitchTab { tab_index: 2 });
                        cx.notify();
                    }
                    "tab" => {
                        let next = (this.snapshot.selected_tab + 1) % 3;
                        this.snapshot.selected_tab = next;
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
                            this.send_command(ClientCommand::SelectPane { pane_index: prev });
                            cx.notify();
                        }
                    }
                    "]" => {
                        if !this.snapshot.swarm_panes.is_empty() {
                            let curr = this.snapshot.swarm_selected_pane;
                            let next = (curr + 1) % this.snapshot.swarm_panes.len();
                            this.snapshot.swarm_selected_pane = next;
                            this.send_command(ClientCommand::SelectPane { pane_index: next });
                            cx.notify();
                        }
                    }
                    "t" => {
                        this.snapshot.show_thinking = !this.snapshot.show_thinking;
                        this.send_command(ClientCommand::ToggleThinking);
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
                        _ => self.render_swarm_cockpit(cx).into_any_element(),
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
