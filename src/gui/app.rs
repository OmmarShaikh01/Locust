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
}

impl Render for CockpitApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let perm_opt = self.snapshot.active_permission.clone();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(color_bg_base())
            .font_family(font_family_sans())
            .child(self.render_header(cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .size_full()
                    .child(self.render_sidebar(cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .h_full()
                            .child(self.render_timeline(cx))
                            .child(self.render_composer(cx)),
                    )
                    .child(self.render_inspector(cx)),
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
