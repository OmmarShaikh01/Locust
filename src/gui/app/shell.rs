use super::CockpitApp;
use crate::gui::theme::*;
use crate::ipc::protocol::ClientCommand;
use gpui::*;

impl CockpitApp {
    pub(super) fn render_header(&self, _cx: &mut Context<Self>) -> impl IntoElement {
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

    pub(super) fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
}
