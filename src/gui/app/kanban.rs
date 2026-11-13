use super::CockpitApp;
use crate::gui::theme::*;
use crate::ipc::protocol::ClientCommand;
use gpui::*;

impl CockpitApp {
    pub(super) fn render_empty_column_card(text: &'static str) -> AnyElement {
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

    pub(super) fn render_kanban_column(
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

    pub(super) fn render_kanban_inspector(
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

    pub(super) fn render_kanban_tracker(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
