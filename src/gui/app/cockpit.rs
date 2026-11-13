use super::CockpitApp;
use crate::gui::theme::*;
use crate::ipc::protocol::{ClientCommand, PermissionSnapshot};
use gpui::*;

impl CockpitApp {
    pub(super) fn render_timeline(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

    pub(super) fn render_composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

    pub(super) fn render_inspector(&self, _cx: &mut Context<Self>) -> impl IntoElement {
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

    pub(super) fn render_permission_modal(
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
    pub(super) fn render_swarm_cockpit(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
}
