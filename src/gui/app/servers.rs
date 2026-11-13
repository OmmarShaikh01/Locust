use super::CockpitApp;
use crate::gui::theme::*;
use crate::ipc::protocol::ClientCommand;
use gpui::*;

impl CockpitApp {
    pub(super) fn render_acp_servers(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
}
