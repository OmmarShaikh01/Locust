use super::CockpitApp;
use crate::gui::theme::*;
use crate::ipc::protocol::ClientCommand;
use gpui::*;

impl CockpitApp {
    pub(super) fn render_dashboard(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
}
