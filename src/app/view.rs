use crate::app::model::{AcpStatus, ControllerRole, Model, SwarmFocus, SwarmRunStatus};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs, Wrap};

/// Render the full UI from the current model state.
pub fn render(model: &Model, frame: &mut Frame) {
    let area = frame.area();
    let chunks = Layout::vertical([
        Constraint::Length(0), // Top Header Title
        Constraint::Length(1), // Tab Bar
        Constraint::Fill(0),   // Main Content View
        Constraint::Length(1), // Bottom Status / Keybindings Footer
    ])
    .split(area);

    render_tab_bar(model, frame, chunks[1]);

    match model.selected_tab {
        0 => render_dashboard_tab(model, frame, chunks[2]),
        1 => render_acp_servers_window(model, frame, chunks[2]),
        _ => render_swarm_tab(model, frame, chunks[2]),
    }

    render_footer(model, frame, chunks[3]);

    if model.show_help {
        render_help_overlay(frame);
    }
    if model.show_tasks {
        render_task_board(model, frame);
    }

    if let Some(modal) = &model.project_modal {
        render_project_modal(modal, frame);
    }

    if let Some(perm) = &model.active_permission {
        render_permission_modal(perm, frame);
    }
}

fn overlay_rect(frame: &Frame, width: u16, height: u16) -> Rect {
    let area = frame.area();
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.width.saturating_sub(width) / 2,
        area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

/// Key help overlay, opened with `?`.
fn render_help_overlay(frame: &mut Frame) {
    let rect = overlay_rect(frame, 74, 26);
    frame.render_widget(ratatui::widgets::Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Keys ([?] or [Esc] closes) ")
        .title_alignment(Alignment::Center);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let key = |k: &'static str, what: &'static str| {
        Line::from(vec![
            Span::styled(
                format!("{k:<18}"),
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw(what),
        ])
    };
    let lines = vec![
        key("1-9, h/l", "switch tab"),
        key("j/k, Up/Down", "select agent"),
        key("Tab", "focus the Controller chat (Esc/Tab leaves)"),
        key("Enter / c", "run the selected agent"),
        key("x", "cancel the running turn"),
        key("r", "retry the last turn"),
        key(
            "PgUp / PgDn",
            "scroll agent output (chat when it has focus)",
        ),
        key("Home / End", "oldest line / follow newest"),
        key("z", "zoom the agent detail (Esc leaves)"),
        key("t", "show or hide the model's thinking"),
        key("b", "task board: agent-to-agent handoffs"),
        key("o", "switch project (one swarm per project)"),
        key("e", "export transcripts to .locust/runs"),
        key("p", "probe agents for usage"),
        key("a / d", "answer a permission request: allow once / deny"),
        key("q / Esc", "quit"),
        Line::from(""),
        Line::from(Span::styled(
            "Agents hand off work by writing  @[Pane Name]: message  on its own line.",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Task board, opened with `b`: every agent-to-agent handoff and how it ended.
fn render_task_board(model: &Model, frame: &mut Frame) {
    use crate::app::model::TaskStatus;
    let rect = overlay_rect(frame, 100, 22);
    frame.render_widget(ratatui::widgets::Clear, rect);
    let (pending, done, failed) = model.task_counts();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(format!(
            " Task board: {pending} pending, {done} done, {failed} failed ([b] or [Esc] closes) "
        ))
        .title_alignment(Alignment::Center);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let mut lines: Vec<Line> = Vec::new();
    if model.tasks.is_empty() {
        lines.push(Line::from(Span::styled(
            "No handoffs yet. They appear when an agent writes @[Pane Name]: message.",
            Style::default().fg(Color::DarkGray),
        )));
    }
    let rows = inner.height as usize;
    for task in model.tasks.iter().rev().take(rows) {
        let (label, color) = match task.status {
            TaskStatus::Pending => ("PENDING", Color::Yellow),
            TaskStatus::Done => ("DONE   ", Color::Green),
            TaskStatus::Failed => ("FAILED ", Color::Red),
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("#{:<3} ", task.id),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(label, Style::default().fg(color).bold()),
            Span::raw(format!(
                " {} -> {}: {}",
                truncate(&task.source, 18),
                truncate(&task.target, 18),
                truncate(&task.description, 50)
            )),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The switch-project dialog: current project, path input, recent list, any error.
fn render_project_modal(modal: &crate::app::model::ProjectModal, frame: &mut Frame) {
    let area = frame.area();
    let width = 78.min(area.width);
    let height = (10 + modal.recent.len() as u16).min(area.height);
    let rect = Rect::new(
        area.width.saturating_sub(width) / 2,
        area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(ratatui::widgets::Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Switch project (one swarm per project) ")
        .title_alignment(Alignment::Center);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let dim = Style::default().fg(Color::DarkGray);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Current: ", dim),
            Span::raw(modal.current.clone()),
        ]),
        Line::from(vec![
            Span::styled("Open:    ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{}_", modal.input)),
        ]),
        Line::from(""),
        Line::from(Span::styled("Recent:", dim)),
    ];
    if modal.recent.is_empty() {
        lines.push(Line::from(Span::styled("  (none yet)", dim)));
    }
    for (i, path) in modal.recent.iter().enumerate() {
        let picked = modal.selected == Some(i);
        lines.push(Line::from(Span::styled(
            format!("{} {}", if picked { ">" } else { " " }, path),
            if picked {
                Style::default().fg(Color::Yellow).bold()
            } else {
                Style::default()
            },
        )));
    }
    lines.push(Line::from(""));
    if let Some(err) = &modal.error {
        lines.push(Line::from(Span::styled(
            format!("Error: {err}"),
            Style::default().fg(Color::Red).bold(),
        )));
    }
    lines.push(Line::from(Span::styled(
        "[Enter] open  [Up/Down] recent  [Esc] cancel. Agents and chats of this project stop.",
        dim,
    )));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

/// Format the global usage telemetry badge displayed in the top bar.
fn format_top_usage_badge(
    total_tokens: u64,
    used_tokens: u64,
    context_size: Option<u64>,
    tokens_left: Option<u64>,
    invocations: u64,
    cost: Option<&(f64, String)>,
    avail_width: usize,
) -> Line<'static> {
    let mut spans = vec![Span::styled("[ ", Style::default().fg(Color::DarkGray))];

    if avail_width >= 85 {
        spans.push(Span::styled("Tokens: ", Style::default().fg(Color::Yellow)));
        spans.push(Span::styled(
            format_number(total_tokens),
            Style::default().fg(Color::Cyan).bold(),
        ));
        if let (Some(left), Some(size)) = (tokens_left, context_size) {
            let pct = if size > 0 {
                (left as f64 / size as f64) * 100.0
            } else {
                0.0
            };
            spans.push(Span::raw(" | "));
            spans.push(Span::styled(
                "Context: ",
                Style::default().fg(Color::Yellow),
            ));
            spans.push(Span::styled(
                format!(
                    "{}/{} ({} left, {:.1}%)",
                    format_number(used_tokens),
                    format_number(size),
                    format_number(left),
                    pct
                ),
                Style::default().fg(Color::Green).bold(),
            ));
        } else if let Some(left) = tokens_left {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled(
                format!("Left: {}", format_number(left)),
                Style::default().fg(Color::Green).bold(),
            ));
        } else if used_tokens > 0 {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled(
                format!("In Context: {}", format_number(used_tokens)),
                Style::default().fg(Color::Cyan),
            ));
        }
        spans.push(Span::raw(" | "));
        spans.push(Span::styled("Calls: ", Style::default().fg(Color::Yellow)));
        spans.push(Span::styled(
            format_number(invocations),
            Style::default().fg(Color::Cyan).bold(),
        ));
        if let Some((amount, cur)) = cost {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled(
                format!("{:.4} {}", amount, cur),
                Style::default().fg(Color::Green),
            ));
        }
    } else if avail_width >= 55 {
        spans.push(Span::styled("Tokens: ", Style::default().fg(Color::Yellow)));
        spans.push(Span::styled(
            format_number(total_tokens),
            Style::default().fg(Color::Cyan).bold(),
        ));
        if let (Some(left), Some(size)) = (tokens_left, context_size) {
            let pct = if size > 0 {
                (left as f64 / size as f64) * 100.0
            } else {
                0.0
            };
            spans.push(Span::raw(" | "));
            spans.push(Span::styled(
                format!("{} left ({:.1}%)", format_number(left), pct),
                Style::default().fg(Color::Green).bold(),
            ));
        } else if let Some(left) = tokens_left {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled(
                format!("{} left", format_number(left)),
                Style::default().fg(Color::Green).bold(),
            ));
        }
        spans.push(Span::raw(" | "));
        spans.push(Span::styled("Calls: ", Style::default().fg(Color::Yellow)));
        spans.push(Span::styled(
            format_number(invocations),
            Style::default().fg(Color::Cyan).bold(),
        ));
    } else if avail_width >= 35 {
        spans.push(Span::styled(
            format!("{} tok", format_number(total_tokens)),
            Style::default().fg(Color::Cyan).bold(),
        ));
        if let Some(left) = tokens_left {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled(
                format!("{} left", format_number(left)),
                Style::default().fg(Color::Green).bold(),
            ));
        } else {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled(
                format!("{} calls", format_number(invocations)),
                Style::default().fg(Color::Yellow),
            ));
        }
    } else {
        if let Some(left) = tokens_left {
            spans.push(Span::styled(
                format!("{} left", format_number(left)),
                Style::default().fg(Color::Green).bold(),
            ));
        } else {
            spans.push(Span::styled(
                format!("{} tok", format_number(total_tokens)),
                Style::default().fg(Color::Cyan).bold(),
            ));
        }
    }

    spans.push(Span::styled(" ]", Style::default().fg(Color::DarkGray)));
    Line::from(spans)
}

/// Render the tab navigation bar.
fn render_tab_bar(model: &Model, frame: &mut Frame, area: Rect) {
    let tab_titles = if area.width >= 50 {
        vec!["[1] Dashboard", "[2] ACP Servers", "[3] Swarm"]
    } else {
        vec!["[1] Dash", "[2] ACP", "[3] Swarm"]
    };
    let tabs_len = if area.width >= 50 {
        44.min(area.width)
    } else {
        30.min(area.width)
    };
    let tabs = Tabs::new(tab_titles)
        .select(model.selected_tab)
        .style(Style::default().fg(Color::DarkGray))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )
        .divider(" | ")
        .padding(" ", " ");

    let total_tokens = model.telemetry.total_tokens.max(model.acp_tokens_used());
    let invocations = model.telemetry.invocations.max(model.acp_calls_made);
    let used_tokens = model.acp_tokens_used();
    let context_size = model.acp_context_size();
    let tokens_left = model.acp_tokens_left();
    let cost = model
        .acp_total_cost()
        .or_else(|| model.telemetry.total_cost.clone());

    let badge_len = area.width.saturating_sub(tabs_len);
    if badge_len >= 10 {
        let chunks =
            Layout::horizontal([Constraint::Length(tabs_len), Constraint::Fill(1)]).split(area);
        frame.render_widget(tabs, chunks[0]);
        let badge_line = format_top_usage_badge(
            total_tokens,
            used_tokens,
            context_size,
            tokens_left,
            invocations,
            cost.as_ref(),
            chunks[1].width as usize,
        );
        let badge_widget = Paragraph::new(badge_line).alignment(Alignment::Right);
        frame.render_widget(badge_widget, chunks[1]);
    } else {
        frame.render_widget(tabs, area);
    }
}

/// Render Tab 0: Dashboard with 3 frames.
fn render_dashboard_tab(model: &Model, frame: &mut Frame, area: Rect) {
    // Split area horizontally into Left Frame (50%) and Right Area (50%)
    let horizontal_chunks =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area);

    // Frame 1 is the Left Column
    let frame1_area = horizontal_chunks[0];

    // Split Right Area vertically into Frame 2 (Top) and Frame 3 (Bottom)
    let right_vertical_chunks =
        Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(horizontal_chunks[1]);

    let frame2_area = right_vertical_chunks[0];
    let frame3_area = right_vertical_chunks[1];

    render_dashboard_frame_1(model, frame, frame1_area);
    render_dashboard_frame_2(model, frame, frame2_area);
    render_dashboard_frame_3(model, frame, frame3_area);
}

/// Frame 1: Swarm Agents & Status.
fn render_dashboard_frame_1(model: &Model, frame: &mut Frame, area: Rect) {
    let is_focused = model.dashboard_frame == 0;
    let border_color = if is_focused {
        Color::Cyan
    } else {
        Color::White
    };
    let title_tag = if is_focused { " [ACTIVE] " } else { " " };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(format!(" Frame 1: Swarm Agents & Status{} ", title_tag));

    let mut lines = vec![Line::from(vec![
        Span::styled("Swarm Config: ", Style::default().fg(Color::Yellow).bold()),
        Span::raw(model.swarm_config_path.clone()),
    ])];

    if let Some(err) = &model.swarm_config_error {
        lines.push(Line::from(vec![Span::styled(
            format!("Config error: {err}"),
            Style::default().fg(Color::Red),
        )]));
    } else {
        let (idle, running, done, error) = model.swarm_status_counts();
        lines.push(Line::from(vec![
            Span::styled("Panes: ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(format!("{} total | ", model.swarm_panes.len())),
            Span::styled(
                format!("{idle} idle",),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(" | "),
            Span::styled(
                format!("{running} running"),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(" | "),
            Span::styled(format!("{done} done"), Style::default().fg(Color::Green)),
            Span::raw(" | "),
            Span::styled(format!("{error} error"), Style::default().fg(Color::Red)),
        ]));
        let (pending, done, failed) = model.task_counts();
        if pending + done + failed > 0 {
            lines.push(Line::from(vec![
                Span::styled("Tasks: ", Style::default().fg(Color::Yellow).bold()),
                Span::raw(format!("{} dispatched | ", pending + done + failed)),
                Span::styled(
                    format!("{pending} pending"),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(" | "),
                Span::styled(format!("{done} done"), Style::default().fg(Color::Green)),
                Span::raw(" | "),
                Span::styled(format!("{failed} failed"), Style::default().fg(Color::Red)),
            ]));
        }

        let total_tok = model.telemetry.total_tokens.max(model.acp_tokens_used());
        let total_inv = model.telemetry.invocations.max(model.acp_calls_made);
        let mut usage_spans = vec![
            Span::styled("Usage: ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(format!(
                "{} tokens | {} calls",
                format_number(total_tok),
                format_number(total_inv)
            )),
        ];
        if let (Some(u), Some(sz)) = (Some(model.acp_tokens_used()), model.acp_context_size())
            && sz > 0
        {
            let left = sz.saturating_sub(u);
            let pct = (left as f64 / sz as f64) * 100.0;
            usage_spans.push(Span::raw(" | "));
            usage_spans.push(Span::styled(
                format!(
                    "{}/{} ctx ({} left, {:.1}%)",
                    format_number(u),
                    format_number(sz),
                    format_number(left),
                    pct
                ),
                Style::default().fg(Color::Green),
            ));
        }
        if let Some((cost, cur)) = model.acp_total_cost() {
            usage_spans.push(Span::raw(" | "));
            usage_spans.push(Span::styled(
                format!("{:.4} {}", cost, cur),
                Style::default().fg(Color::Green),
            ));
        }
        lines.push(Line::from(usage_spans));

        lines.push(Line::from(
            "---------------------------------------------------------",
        ));

        for pane in &model.swarm_panes {
            let name = truncate(&pane.name, 18);
            let mut row = vec![
                Span::styled(format!("{name:<18} "), Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("{:<7}", pane.status.label()),
                    swarm_status_style(pane.status),
                ),
                Span::styled(
                    format!(" {:<12}", live_badge(pane).unwrap_or_default()),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(format!(" {:<6}", pane.assignment.agent)),
                Span::styled(
                    format!(" {}", short_model(&pane.assignment.model)),
                    Style::default().fg(Color::DarkGray),
                ),
            ];
            if let (Some(u), Some(sz)) = (pane.tokens_used, pane.context_size) {
                let left = sz.saturating_sub(u);
                let pct = if sz > 0 {
                    (left as f64 / sz as f64) * 100.0
                } else {
                    0.0
                };
                row.push(Span::styled(
                    format!(
                        " | {}/{} ({} left, {:.1}%)",
                        format_number(u),
                        format_number(sz),
                        format_number(left),
                        pct
                    ),
                    Style::default().fg(Color::Green),
                ));
            } else if let Some(u) = pane.tokens_used {
                row.push(Span::styled(
                    format!(" | {} tok", format_number(u)),
                    Style::default().fg(Color::DarkGray),
                ));
            } else {
                row.push(Span::styled(
                    " | ready",
                    Style::default().fg(Color::DarkGray),
                ));
            }
            lines.push(Line::from(row));
        }
    }

    lines.push(Line::from(
        "---------------------------------------------------------",
    ));
    lines.push(Line::from(vec![
        Span::styled("Active Directory: ", Style::default().fg(Color::Yellow)),
        Span::raw(
            std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "?".to_string()),
        ),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Tip: [3] opens the Swarm tab to run any agent above for real.",
        Style::default().fg(Color::DarkGray),
    )));

    let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

/// Frame 2: cross-agent activity tracer -- one real line per status change across every
/// ACP connection and every swarm pane run, tailed to the newest entries.
fn render_dashboard_frame_2(model: &Model, frame: &mut Frame, area: Rect) {
    let is_focused = model.dashboard_frame == 1;
    let border_color = if is_focused {
        Color::Cyan
    } else {
        Color::White
    };
    let title_tag = if is_focused { " [ACTIVE] " } else { " " };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(format!(
            " Frame 2: Activity Tracer ({} events){} ",
            model.activity_log.len(),
            title_tag
        ));

    let lines: Vec<Line> = if model.activity_log.is_empty() {
        vec![Line::from(Span::styled(
            "No activity yet. Connect an ACP agent or run a swarm pane to see it here.",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        model
            .activity_log
            .iter()
            .map(|entry| {
                let style = if entry.contains("ERROR") {
                    Style::default().fg(Color::Red)
                } else if entry.contains("done:") || entry.contains("online") {
                    Style::default().fg(Color::Green)
                } else if entry.contains("launching") || entry.contains("connecting") {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::White)
                };
                Line::from(Span::styled(entry.clone(), style))
            })
            .collect()
    };

    // Tail the log: scroll so the newest entries are visible instead of the oldest.
    let inner_width = area.width.saturating_sub(2) as usize;
    let visible_rows = area.height.saturating_sub(2) as usize;
    let total_visual = count_rendered_lines(&lines, inner_width);
    let scroll = total_visual.saturating_sub(visible_rows) as u16;

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: true })
        .scroll((scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Frame 3: Telemetry & Performance Metrics (Real Live System & Swarm Metrics).
fn render_dashboard_frame_3(model: &Model, frame: &mut Frame, area: Rect) {
    let is_focused = model.dashboard_frame == 2;
    let border_color = if is_focused {
        Color::Cyan
    } else {
        Color::White
    };
    let title_tag = if is_focused { " [ACTIVE] " } else { " " };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(format!(" Frame 3: Telemetry & Metrics{} ", title_tag));

    let t = &model.telemetry;
    let cpu_bar = render_bar(t.cpu_percent, 14);
    let ram_bar = render_bar(t.ram_percent, 14);
    let gpu_bar = render_bar(t.gpu.vram_percent, 14);

    let uptime_h = t.process_uptime_secs / 3600;
    let uptime_m = (t.process_uptime_secs % 3600) / 60;
    let uptime_s = t.process_uptime_secs % 60;
    let uptime_str = format!("{:02}h {:02}m {:02}s", uptime_h, uptime_m, uptime_s);

    let gpu_name = if !t.gpu.name.is_empty() {
        if t.gpu.name.len() > 22 {
            format!("{}...", &t.gpu.name[..19])
        } else {
            t.gpu.name.clone()
        }
    } else {
        "GPU".to_string()
    };

    let lines = vec![
        Line::from(vec![
            Span::styled("CPU Load:  ", Style::default().fg(Color::Yellow)),
            Span::styled(
                format!("{} ", cpu_bar),
                Style::default().fg(metric_color(t.cpu_percent)),
            ),
            Span::raw(format!("{:>3}%", t.cpu_percent)),
        ]),
        Line::from(vec![
            Span::styled("Memory:    ", Style::default().fg(Color::Yellow)),
            Span::styled(
                format!("{} ", ram_bar),
                Style::default().fg(metric_color(t.ram_percent)),
            ),
            Span::raw(format!(
                "{:>3}% ({:.1} GB / {:.1} GB)",
                t.ram_percent, t.ram_used_gb, t.ram_total_gb
            )),
        ]),
        Line::from(vec![
            Span::styled("GPU VRAM:  ", Style::default().fg(Color::Yellow)),
            Span::styled(
                format!("{} ", gpu_bar),
                Style::default().fg(metric_color(t.gpu.vram_percent)),
            ),
            Span::raw(format!(
                "{:>3}% ({:.1} GB / {:.1} GB) [{}]",
                t.gpu.vram_percent, t.gpu.vram_used_gb, t.gpu.vram_total_gb, gpu_name
            )),
        ]),
        Line::from("---------------------------------------------------------"),
        Line::from(vec![
            Span::styled("Tokens:    ", Style::default().fg(Color::Yellow)),
            Span::raw(format!(
                "Prompt: {} | Compl: {} | Total: {}",
                format_number(t.prompt_tokens),
                format_number(t.completion_tokens),
                format_number(t.total_tokens)
            )),
        ]),
        Line::from(vec![
            Span::styled("Calls:     ", Style::default().fg(Color::Yellow)),
            Span::raw(format!(
                "{} API Invocations | Active Agents: {}/{}",
                format_number(t.invocations),
                t.active_agents,
                t.total_agents
            )),
        ]),
        Line::from(vec![
            Span::styled("Health:    ", Style::default().fg(Color::Yellow)),
            Span::styled(
                format!("OK (PID: {} | Uptime: {})", t.pid, uptime_str),
                Style::default().fg(Color::Green).bold(),
            ),
        ]),
        Line::from(vec![
            Span::styled("ACP:       ", Style::default().fg(Color::Yellow)),
            Span::raw(format!(
                "{} real RPC call(s) | {} token(s) in context{}{}",
                model.acp_calls_made,
                format_number(model.acp_tokens_used()),
                match model.acp_tokens_left() {
                    Some(left) => format!(" ({} left)", format_number(left)),
                    None => String::new(),
                },
                match model.acp_total_cost() {
                    Some((amount, currency)) => format!(" | {:.4} {}", amount, currency),
                    None => String::new(),
                }
            )),
        ]),
    ];

    let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

/// Render Tab 1 / Window: ACP Servers.
fn render_acp_servers_window(model: &Model, frame: &mut Frame, area: Rect) {
    // One flat vstack: server list and server config split 70/30 against each other, and
    // the calls (RPC traffic log) sized evenly against the two of them combined.
    let horizontal_chunks =
        Layout::horizontal([Constraint::Percentage(30), Constraint::Percentage(70)]).split(area);

    // Split Right Area vertically into Frame 2 (Top) and Frame 3 (Bottom)
    let right_vertical_chunks =
        Layout::vertical([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(horizontal_chunks[0]);

    let frame1_area = horizontal_chunks[1];
    let frame2_area = right_vertical_chunks[0];
    let frame3_area = right_vertical_chunks[1];

    render_acp_server_logs(model, frame, frame1_area);
    render_acp_server_list(model, frame, frame2_area);
    render_acp_server_details(model, frame, frame3_area);
}

/// Render ACP Server List pane.
fn render_acp_server_list(model: &Model, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(format!(
            " ACP Servers Window ({} Registered) ",
            model.acp_servers.len()
        ));

    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        "Select server with [Up/Down] or [k/j]:",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from("----------------------------------------"));

    for (idx, server) in model.acp_servers.iter().enumerate() {
        let is_selected = idx == model.acp_selected_server;
        let marker = if is_selected { "> " } else { "  " };

        let status_style = status_style(server.status);

        let line_style = if is_selected {
            Style::default().fg(Color::Yellow).bold()
        } else {
            Style::default().fg(Color::White)
        };

        lines.push(Line::from(vec![
            Span::styled(marker, Style::default().fg(Color::Cyan).bold()),
            Span::styled(format!("{:<18}", server.id), line_style),
            Span::raw(" ["),
            Span::styled(server.status.label(), status_style),
            Span::raw("]"),
        ]));

        lines.push(Line::from(vec![
            Span::raw("    Proto: "),
            Span::styled(
                server.protocol.clone(),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(format!(" | Ping: {}ms", server.ping_ms)),
        ]));

        if let (Some(used), Some(size)) = (server.tokens_used, server.context_size) {
            let left = size.saturating_sub(used);
            let pct = if size > 0 {
                (left as f64 / size as f64) * 100.0
            } else {
                0.0
            };
            lines.push(Line::from(vec![
                Span::raw("    Tokens: "),
                Span::styled(
                    format!(
                        "{}/{} ({} left, {:.1}%)",
                        format_number(used),
                        format_number(size),
                        format_number(left),
                        pct
                    ),
                    Style::default().fg(Color::Green),
                ),
            ]));
        } else if let Some(used) = server.tokens_used {
            lines.push(Line::from(vec![
                Span::raw("    Tokens: "),
                Span::styled(
                    format!("{} in context", format_number(used)),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::raw("    Tokens: "),
                Span::styled("(not probed yet)", Style::default().fg(Color::DarkGray)),
            ]));
        }

        lines.push(Line::from(""));
    }

    let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

/// Render details for selected ACP server.
fn render_acp_server_details(model: &Model, frame: &mut Frame, area: Rect) {
    let selected = model.selected_server();

    let title = match selected {
        Some(s) => format!(" Server Details: {} ", s.name),
        None => " Server Details ".to_string(),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White))
        .title(title);

    let lines = if let Some(s) = selected {
        let uptime = match s.connected_at {
            Some(since) => {
                let secs = since.elapsed().as_secs();
                format!(
                    "{:02}h {:02}m {:02}s",
                    secs / 3600,
                    (secs % 3600) / 60,
                    secs % 60
                )
            }
            None => "-".to_string(),
        };
        let capabilities = if s.capabilities.is_empty() {
            "(connect to discover)".to_string()
        } else {
            s.capabilities.join(", ")
        };

        vec![
            Line::from(vec![
                Span::styled(
                    "Server ID:      ",
                    Style::default().fg(Color::Yellow).bold(),
                ),
                Span::styled(s.id, Style::default().fg(Color::White).bold()),
            ]),
            Line::from(vec![
                Span::styled(
                    "Status:         ",
                    Style::default().fg(Color::Yellow).bold(),
                ),
                Span::styled(s.status.label(), status_style(s.status).bold()),
                Span::raw(format!(" (Uptime: {})", uptime)),
            ]),
            Line::from(vec![
                Span::styled(
                    "Transport:      ",
                    Style::default().fg(Color::Yellow).bold(),
                ),
                Span::raw(s.transport),
            ]),
            Line::from(vec![
                Span::styled(
                    "Launch Command: ",
                    Style::default().fg(Color::Yellow).bold(),
                ),
                Span::raw(s.command_line.clone()),
            ]),
            Line::from(vec![
                Span::styled(
                    "Protocol:       ",
                    Style::default().fg(Color::Yellow).bold(),
                ),
                Span::raw(s.protocol.clone()),
            ]),
            Line::from(vec![
                Span::styled(
                    "Ping Latency:   ",
                    Style::default().fg(Color::Yellow).bold(),
                ),
                Span::raw(format!("{} ms", s.ping_ms)),
            ]),
            Line::from(vec![
                Span::styled(
                    "Capabilities:   ",
                    Style::default().fg(Color::Yellow).bold(),
                ),
                Span::styled(capabilities, Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled(
                    "Tokens:         ",
                    Style::default().fg(Color::Yellow).bold(),
                ),
                Span::raw(match (s.tokens_used, s.context_size) {
                    (Some(used), Some(size)) => format!(
                        "{} / {} in context{}",
                        format_number(used),
                        format_number(size),
                        match &s.cost {
                            Some((amount, currency)) => format!(" | {:.4} {}", amount, currency),
                            None => String::new(),
                        }
                    ),
                    (Some(used), None) => format!(
                        "{} in context{}",
                        format_number(used),
                        match &s.cost {
                            Some((amount, currency)) => format!(" | {:.4} {}", amount, currency),
                            None => String::new(),
                        }
                    ),
                    _ => "(not probed yet)".to_string(),
                }),
            ]),
            Line::from(vec![
                Span::styled(
                    "Capacity Left:  ",
                    Style::default().fg(Color::Yellow).bold(),
                ),
                match (s.tokens_used, s.context_size) {
                    (Some(used), Some(size)) => {
                        let left = size.saturating_sub(used);
                        let pct = if size > 0 {
                            (left as f64 / size as f64) * 100.0
                        } else {
                            0.0
                        };
                        let style = if pct < 15.0 {
                            Style::default().fg(Color::Red).bold()
                        } else if pct < 35.0 {
                            Style::default().fg(Color::Yellow).bold()
                        } else {
                            Style::default().fg(Color::Green).bold()
                        };
                        Span::styled(
                            format!("{} left ({:.1}% available)", format_number(left), pct),
                            style,
                        )
                    }
                    (Some(_), None) => Span::styled(
                        "(context limit unknown)",
                        Style::default().fg(Color::DarkGray),
                    ),
                    _ => Span::styled("(not probed yet)", Style::default().fg(Color::DarkGray)),
                },
            ]),
        ]
    } else {
        vec![Line::from("No server selected.")]
    };

    let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

/// Render traffic & message logs for selected ACP server.
fn render_acp_server_logs(model: &Model, frame: &mut Frame, area: Rect) {
    let selected = model.selected_server();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White))
        .title(" ACP RPC Traffic & Event Log ");

    let mut lines = Vec::new();

    if let Some(s) = selected {
        for log in &s.recent_logs {
            let style = if log.starts_with("-->") {
                Style::default().fg(Color::Cyan)
            } else if log.starts_with("<--") {
                Style::default().fg(Color::Green)
            } else if log.contains("ERROR") || log.contains("timed out") {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::White)
            };

            for line in log.lines() {
                lines.push(Line::from(Span::styled(line.to_string(), style)));
            }
        }
    } else {
        lines.push(Line::from("No logs available."));
    }

    let inner_width = area.width.saturating_sub(2) as usize;
    let visible_rows = area.height.saturating_sub(2) as usize;
    let total_visual = count_rendered_lines(&lines, inner_width);
    let scroll = total_visual.saturating_sub(visible_rows) as u16;

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: true })
        .scroll((scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Render bottom status / keybinding footer. Keybinding hints change with the active tab,
/// and with whether the Controller chat input has focus. Usage telemetry is always shown.
fn render_footer(model: &Model, frame: &mut Frame, area: Rect) {
    let (keys_area, usage_area) = if area.width >= 95 {
        let chunks = Layout::horizontal([Constraint::Min(50), Constraint::Length(38)]).split(area);
        (chunks[0], Some(chunks[1]))
    } else if area.width >= 75 {
        let chunks = Layout::horizontal([Constraint::Min(45), Constraint::Length(25)]).split(area);
        (chunks[0], Some(chunks[1]))
    } else {
        (area, None)
    };

    let spans = if model.is_controller_focused() {
        vec![
            Span::styled(" [Enter] ", Style::default().fg(Color::Yellow).bold()),
            Span::raw("Send | "),
            Span::styled("[Backspace] ", Style::default().fg(Color::Yellow).bold()),
            Span::raw("Delete | "),
            Span::styled("[Esc/Tab] ", Style::default().fg(Color::Yellow).bold()),
            Span::raw("Unfocus"),
        ]
    } else {
        let mut s = vec![
            Span::styled(" [1-9] ", Style::default().fg(Color::Yellow).bold()),
            Span::raw("Tabs | "),
            Span::styled(
                "[Left/Right, h/l] ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw("Prev/Next | "),
        ];

        if model.selected_tab == 2 {
            s.push(Span::styled(
                "[Up/Down, j/k] ",
                Style::default().fg(Color::Yellow).bold(),
            ));
            s.push(Span::raw("Select | "));
            s.push(Span::styled(
                "[Tab] ",
                Style::default().fg(Color::Yellow).bold(),
            ));
            s.push(Span::raw("Chat | "));
            s.push(Span::styled(
                "[Enter/c] ",
                Style::default().fg(Color::Yellow).bold(),
            ));
            s.push(Span::raw("Run | "));
            s.push(Span::styled(
                "[p] ",
                Style::default().fg(Color::Yellow).bold(),
            ));
            s.push(Span::raw("Probe | "));
            s.push(Span::styled(
                "[PgUp/PgDn] ",
                Style::default().fg(Color::Yellow).bold(),
            ));
            s.push(Span::raw("Scroll | "));
            s.push(Span::styled(
                "[z] ",
                Style::default().fg(Color::Yellow).bold(),
            ));
            s.push(Span::raw("Zoom | "));
            s.push(Span::styled(
                "[t] ",
                Style::default().fg(Color::Yellow).bold(),
            ));
            s.push(Span::raw("Thinking | "));
        } else if model.selected_tab == 1 {
            s.push(Span::styled(
                "[Up/Down, j/k] ",
                Style::default().fg(Color::Yellow).bold(),
            ));
            s.push(Span::raw("Select | "));
            s.push(Span::styled(
                "[Enter/c/p] ",
                Style::default().fg(Color::Yellow).bold(),
            ));
            s.push(Span::raw("Probe | "));
            s.push(Span::styled(
                "[d] ",
                Style::default().fg(Color::Yellow).bold(),
            ));
            s.push(Span::raw("Disconnect | "));
        } else {
            s.push(Span::styled(
                "[Tab] ",
                Style::default().fg(Color::Yellow).bold(),
            ));
            s.push(Span::raw("Frame | "));
            s.push(Span::styled(
                "[p] ",
                Style::default().fg(Color::Yellow).bold(),
            ));
            s.push(Span::raw("Probe | "));
        }

        s.push(Span::styled(
            "[r] ",
            Style::default().fg(Color::Yellow).bold(),
        ));
        s.push(Span::raw("Retry | "));
        s.push(Span::styled(
            "[x] ",
            Style::default().fg(Color::Yellow).bold(),
        ));
        s.push(Span::raw("Cancel | "));
        s.push(Span::styled(
            "[e] ",
            Style::default().fg(Color::Yellow).bold(),
        ));
        s.push(Span::raw("Export | "));

        s.push(Span::styled(
            "[q/Esc] ",
            Style::default().fg(Color::Yellow).bold(),
        ));
        s.push(Span::raw("Quit"));
        s
    };

    frame.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Left),
        keys_area,
    );

    if let Some(u_area) = usage_area {
        let total_tokens = model.telemetry.total_tokens.max(model.acp_tokens_used());
        let tokens_left = model.acp_tokens_left();
        let footer_usage = if u_area.width >= 35 {
            match tokens_left {
                Some(left) => format!(
                    "[ Usage: {} tok | {} left ]",
                    format_number(total_tokens),
                    format_number(left)
                ),
                None => format!("[ Usage: {} total tokens ]", format_number(total_tokens)),
            }
        } else {
            match tokens_left {
                Some(left) => format!("[ {}k tok | {}k left ]", total_tokens / 1000, left / 1000),
                None => format!("[ {}k tokens ]", total_tokens / 1000),
            }
        };
        let p = Paragraph::new(Line::from(Span::styled(
            footer_usage,
            Style::default().fg(Color::DarkGray),
        )))
        .alignment(Alignment::Right);
        frame.render_widget(p, u_area);
    }
}

fn render_permission_modal(perm: &crate::app::model::PermissionModal, frame: &mut Frame) {
    let area = frame.area();
    let modal_width = 62.min(area.width);
    let modal_height = 12.min(area.height);
    let x = (area.width.saturating_sub(modal_width)) / 2;
    let y = (area.height.saturating_sub(modal_height)) / 2;
    let rect = Rect::new(x, y, modal_width, modal_height);

    frame.render_widget(ratatui::widgets::Clear, rect);

    let secs_left = perm
        .expires_at
        .checked_duration_since(std::time::Instant::now())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" Agent Permission Request (Default: Deny on Timeout) ")
        .title_alignment(Alignment::Center);

    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let lines = vec![
        Line::from(vec![
            Span::styled("Agent:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(&perm.agent_pane, Style::default().fg(Color::White).bold()),
        ]),
        Line::from(vec![
            Span::styled("Action:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(&perm.tool_title, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("Input:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                if perm.detail.is_empty() {
                    "(the agent sent no details)".to_string()
                } else {
                    perm.detail.clone()
                },
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("Timeout: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{secs_left}s remaining"),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::styled(" [A] ", Style::default().fg(Color::Green).bold()),
            Span::styled("Allow Once    ", Style::default().fg(Color::White)),
            Span::styled(" [D / Esc] ", Style::default().fg(Color::Red).bold()),
            Span::styled("Deny (Default)", Style::default().fg(Color::White)),
        ]),
    ];

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

/// Pick the display color/style for an ACP connection status.
fn status_style(status: AcpStatus) -> Style {
    match status {
        AcpStatus::Online => Style::default().fg(Color::Green).bold(),
        AcpStatus::Connecting => Style::default().fg(Color::Yellow),
        AcpStatus::Error => Style::default().fg(Color::Red).bold(),
        AcpStatus::Offline => Style::default().fg(Color::DarkGray),
    }
}

/// Pick the display color/style for a swarm pane's run status.
fn swarm_status_style(status: SwarmRunStatus) -> Style {
    match status {
        SwarmRunStatus::Running => Style::default().fg(Color::Yellow).bold(),
        SwarmRunStatus::Done => Style::default().fg(Color::Green).bold(),
        SwarmRunStatus::Error => Style::default().fg(Color::Red).bold(),
        SwarmRunStatus::Idle => Style::default().fg(Color::DarkGray),
    }
}

/// Render Tab 2 / Window: Swarm. Left is every spawned agent in one arrow-key list; right
/// is the full log/detail of whichever is selected; the bottom strip is the Controller --
/// a persistent chat with the Mission Orchestrator that's always there, independent of
/// whatever's selected in the list. [Tab] moves focus between the list and the chat input.
fn render_swarm_tab(model: &Model, frame: &mut Frame, area: Rect) {
    if let Some(err) = &model.swarm_config_error {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red))
            .title(" Swarm Pipeline: config error ");
        let paragraph = Paragraph::new(Line::from(vec![Span::styled(
            err.clone(),
            Style::default().fg(Color::Red),
        )]))
        .block(block)
        .wrap(Wrap { trim: true });
        frame.render_widget(paragraph, area);
        return;
    }

    if model.swarm_zoom {
        render_swarm_pane_detail(model, frame, area);
        return;
    }

    let chunks =
        Layout::vertical([Constraint::Percentage(65), Constraint::Percentage(35)]).split(area);
    let top_chunks = Layout::horizontal([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(chunks[0]);

    render_swarm_agent_list(model, frame, top_chunks[0]);
    render_swarm_pane_detail(model, frame, top_chunks[1]);
    render_controller_pane(model, frame, chunks[1]);
}

/// Left pane: every spawned agent, arrow-key selectable. Each line is labeled with its
/// agents.yaml tab (e.g. "CTL") so agents from the same tier stay recognizable in one flat
/// list.
fn render_swarm_agent_list(model: &Model, frame: &mut Frame, area: Rect) {
    let is_focused = model.swarm_focus == SwarmFocus::List;
    let border_color = if is_focused {
        Color::Cyan
    } else {
        Color::White
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(format!(" Agents ({}) ", model.swarm_panes.len()));

    let lines: Vec<Line> = model
        .swarm_panes
        .iter()
        .enumerate()
        .map(|(i, pane)| {
            let is_selected = i == model.swarm_selected_pane;
            let marker = if is_selected { "> " } else { "  " };
            let line_style = if is_selected {
                Style::default().fg(Color::Yellow).bold()
            } else {
                Style::default().fg(Color::White)
            };
            let mut spans = vec![
                Span::styled(marker, Style::default().fg(Color::Cyan).bold()),
                Span::styled(
                    format!("{:<5} ", short_swarm_label(&pane.tab)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(format!("{:<20}", truncate(&pane.name, 20)), line_style),
                Span::styled(
                    format!(" {:<7}", pane.status.label()),
                    swarm_status_style(pane.status),
                ),
            ];
            if let Some(badge) = live_badge(pane) {
                spans.push(Span::styled(
                    format!(" {badge}"),
                    Style::default().fg(Color::Yellow),
                ));
            }
            if let (Some(u), Some(sz)) = (pane.tokens_used, pane.context_size) {
                let left = sz.saturating_sub(u);
                spans.push(Span::styled(
                    format!(" | {} left", format_number(left)),
                    Style::default().fg(Color::DarkGray),
                ));
            } else if let Some(u) = pane.tokens_used {
                spans.push(Span::styled(
                    format!(" | {} tok", format_number(u)),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            Line::from(spans)
        })
        .collect();

    let visible_rows = area.height.saturating_sub(2) as usize;
    let scroll = if model.swarm_selected_pane >= visible_rows {
        (model.swarm_selected_pane + 1 - visible_rows) as u16
    } else {
        0
    };

    let paragraph = Paragraph::new(lines).block(block).scroll((scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Bottom strip: the persistent Controller chat. Visible and populated regardless of which
/// agent is selected in the list above -- it's its own independent conversation.
fn render_controller_pane(model: &Model, frame: &mut Frame, area: Rect) {
    let is_focused = model.swarm_focus == SwarmFocus::Controller;
    let border_color = if is_focused {
        Color::Cyan
    } else {
        Color::White
    };
    let connection = model.controller_connection_status();
    let hint = if is_focused {
        "[Esc/Tab] unfocus, [PgUp/PgDn] scroll"
    } else {
        "[Tab] focus to chat"
    };

    let mut title_spans = vec![
        Span::raw(" Controller (Mission Orchestrator) "),
        Span::styled(connection.label(), status_style(connection)),
        Span::raw(format!(" -- {hint} ")),
    ];
    if model.controller_scroll_back > 0 {
        title_spans.push(Span::styled(
            "[scrolled - End follows] ",
            Style::default().fg(Color::Magenta),
        ));
    }
    let total_tokens = model.telemetry.total_tokens.max(model.acp_tokens_used());
    let tokens_left = model.acp_tokens_left();
    if let Some(left) = tokens_left {
        title_spans.push(Span::styled(
            format!(
                "[Usage: {} tok | {} left] ",
                format_number(total_tokens),
                format_number(left)
            ),
            Style::default().fg(Color::Cyan),
        ));
    } else if total_tokens > 0 {
        title_spans.push(Span::styled(
            format!("[Usage: {} tok] ", format_number(total_tokens)),
            Style::default().fg(Color::Cyan),
        ));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Line::from(title_spans));

    let mut lines: Vec<Line> = Vec::new();
    for msg in &model.controller_log {
        match msg.role {
            ControllerRole::User => {
                let mut first = true;
                for l in msg.text.lines() {
                    let prefix = if first { "> " } else { "  " };
                    first = false;
                    lines.push(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(Color::Yellow).bold()),
                        Span::raw(l.to_string()),
                    ]));
                }
                if msg.text.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "> ",
                        Style::default().fg(Color::Yellow).bold(),
                    )));
                }
            }
            ControllerRole::Agent => {
                let mut first = true;
                for l in msg.text.lines() {
                    let prefix = if first {
                        "orchestrator: "
                    } else {
                        "              "
                    };
                    first = false;
                    lines.push(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(Color::Green).bold()),
                        Span::raw(l.to_string()),
                    ]));
                }
                if msg.text.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "orchestrator: ",
                        Style::default().fg(Color::Green).bold(),
                    )));
                }
            }
            ControllerRole::System => {
                for l in msg.text.lines() {
                    lines.push(Line::from(Span::styled(
                        l.to_string(),
                        Style::default().fg(Color::Red),
                    )));
                }
            }
        }
    }

    lines.push(Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::Cyan).bold()),
        Span::raw(model.controller_input.clone()),
        Span::styled(
            if is_focused { "_" } else { "" },
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    let inner_width = area.width.saturating_sub(2) as usize;
    let visible_rows = area.height.saturating_sub(2) as usize;
    let total_visual = count_rendered_lines(&lines, inner_width);
    let max_scroll = total_visual.saturating_sub(visible_rows);
    let scroll = max_scroll.saturating_sub(model.controller_scroll_back.min(max_scroll));
    let scroll = scroll.min(u16::MAX as usize) as u16;

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: true })
        .scroll((scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Drop the common "claude-" / "gemini-3.8-" prefix so a model name fits in the Dashboard's
/// narrow per-agent line, e.g. "claude-sonnet-5" -> "sonnet-5".
fn short_model(model: &str) -> String {
    let stripped = model
        .strip_prefix("claude-")
        .or_else(|| model.strip_prefix("gemini-3.8-"))
        .unwrap_or(model);
    if stripped.chars().count() > 14 {
        let truncated: String = stripped.chars().take(11).collect();
        format!("{truncated}...")
    } else {
        stripped.to_string()
    }
}

/// Short label for the agent list: "AGT-CTL" -> "CTL". Names that don't use the "AGT-"
/// prefix convention (a pane's own bare name, or a custom roster) are already short.
fn short_swarm_label(name: &str) -> String {
    match name.strip_prefix("AGT-") {
        Some(rest) => rest
            .split_whitespace()
            .next()
            .map(String::from)
            .unwrap_or_else(|| name.to_string()),
        None => name.to_string(),
    }
}

/// Truncate to at most `max` characters, with a trailing ellipsis if anything was cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let truncated: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{truncated}...")
    } else {
        s.to_string()
    }
}

/// Render the full detail of whichever agent is currently selected in the list.
fn render_swarm_pane_detail(model: &Model, frame: &mut Frame, area: Rect) {
    let Some(pane) = model.selected_swarm_pane() else {
        frame.render_widget(Paragraph::new("No pane selected."), area);
        return;
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(format!(
            " {} ({}/{}){}{} ",
            pane.name,
            model.swarm_selected_pane + 1,
            model.swarm_panes.len(),
            if model.swarm_zoom { " ZOOM" } else { "" },
            if pane.scroll_back > 0 {
                " [scrolled - End follows]"
            } else {
                " [PgUp/PgDn scroll, z zoom, t thinking]"
            }
        ));

    let mut status_spans = vec![
        Span::styled("Status:  ", Style::default().fg(Color::Yellow)),
        Span::styled(pane.status.label(), swarm_status_style(pane.status)),
    ];
    if pane.status == SwarmRunStatus::Running {
        status_spans.push(Span::styled(
            format!(
                " | {} | {}s elapsed | last event {}s ago (idle limit {}s)",
                pane.live.phase.label(),
                pane.live.elapsed_secs().unwrap_or(0),
                pane.live.silent_secs().unwrap_or(0),
                model.limits.timeout_secs
            ),
            Style::default().fg(
                // Turn red once three quarters of the idle limit has passed in silence.
                if pane.live.silent_secs().unwrap_or(0) * 4 >= model.limits.timeout_secs * 3 {
                    Color::Red
                } else {
                    Color::Yellow
                },
            ),
        ));
        if pane.live.tools_started > 0 {
            status_spans.push(Span::raw(format!(
                " | tools {}/{}",
                pane.live.tools_done, pane.live.tools_started
            )));
        }
    }

    let mut lines = vec![
        Line::from(status_spans),
        Line::from(vec![
            Span::styled("Agent:   ", Style::default().fg(Color::Yellow)),
            Span::raw(format!(
                "{} | model: {} | effort: {}",
                pane.assignment.agent,
                pane.assignment.model,
                pane.assignment.effort.as_deref().unwrap_or("-")
            )),
        ]),
        Line::from(vec![
            Span::styled("Skills:  ", Style::default().fg(Color::Yellow)),
            Span::raw(if pane.assignment.skills.is_empty() {
                "-".to_string()
            } else {
                pane.assignment.skills.join(", ")
            }),
        ]),
        Line::from(vec![
            Span::styled("Task:    ", Style::default().fg(Color::Yellow)),
            Span::raw(pane.assignment.effective_task()),
        ]),
        Line::from(vec![
            Span::styled("Tokens:  ", Style::default().fg(Color::Yellow)),
            Span::raw(match (pane.tokens_used, pane.context_size) {
                (Some(u), Some(sz)) => {
                    let left = sz.saturating_sub(u);
                    let pct = if sz > 0 {
                        (left as f64 / sz as f64) * 100.0
                    } else {
                        0.0
                    };
                    format!(
                        "{}/{} in context ({} left, {:.1}%)",
                        format_number(u),
                        format_number(sz),
                        format_number(left),
                        pct
                    )
                }
                (Some(u), None) => format!("{} in context", format_number(u)),
                _ => "-".to_string(),
            }),
            Span::raw(match &pane.cost {
                Some((amt, cur)) => format!(" | Cost: {:.4} {}", amt, cur),
                None => String::new(),
            }),
        ]),
        Line::from("---"),
    ];

    if let Some(tool) = &pane.live.tool {
        lines.push(Line::from(vec![
            Span::styled("Tool:    ", Style::default().fg(Color::Yellow)),
            Span::raw(tool.clone()),
        ]));
    }

    if model.show_thinking && !pane.live.thought.is_empty() {
        let dim = Style::default()
            .fg(Color::DarkGray)
            .add_modifier(ratatui::style::Modifier::ITALIC);
        let thought_lines: Vec<&str> = pane.live.thought.lines().collect();
        // Zoomed: show all of it. Normal: only the newest few lines so the reply stays visible.
        let keep = if model.swarm_zoom {
            thought_lines.len()
        } else {
            8
        };
        let skipped = thought_lines.len().saturating_sub(keep);
        lines.push(Line::from(Span::styled(
            format!(
                "--- Thinking ({} lines{}; t hides) ---",
                thought_lines.len(),
                if skipped > 0 {
                    format!(", showing last {keep}; z shows all")
                } else {
                    String::new()
                }
            ),
            Style::default().fg(Color::Magenta),
        )));
        for l in thought_lines.iter().skip(skipped) {
            lines.push(Line::from(Span::styled(l.to_string(), dim)));
        }
        lines.push(Line::from(Span::styled(
            "--- Reply ---",
            Style::default().fg(Color::Magenta),
        )));
    }

    if pane.output.is_empty() {
        lines.push(Line::from(Span::styled(
            "(press [Enter] to run this agent for real)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for line in pane.output.lines() {
            lines.push(Line::from(line.to_string()));
        }
    }

    let inner_width = area.width.saturating_sub(2) as usize;
    let visible_rows = area.height.saturating_sub(2) as usize;
    let total_visual = count_rendered_lines(&lines, inner_width);
    let max_scroll = total_visual.saturating_sub(visible_rows);
    // scroll_back counts lines up from the bottom; 0 follows the newest line.
    let scroll = max_scroll.saturating_sub(pane.scroll_back.min(max_scroll));
    let scroll = scroll.min(u16::MAX as usize) as u16;

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: true })
        .scroll((scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Format large numbers with thousands separators.
pub fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let len = s.len();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(c);
    }
    result
}

/// Render a fixed-width ASCII progress bar.
pub fn render_bar(percent: u32, width: usize) -> String {
    let filled = ((percent.min(100) as usize) * width) / 100;
    let empty = width.saturating_sub(filled);
    format!("[{}{}]", "=".repeat(filled), ".".repeat(empty))
}

/// Pick color for utilization percentages.
pub fn metric_color(percent: u32) -> Color {
    if percent < 65 {
        Color::Green
    } else if percent < 85 {
        Color::Yellow
    } else {
        Color::Red
    }
}

/// Short live badge for a running pane, e.g. "thinking 12s" or "tool 3s". `None` when idle.
fn live_badge(pane: &crate::app::model::SwarmPaneState) -> Option<String> {
    use crate::app::model::{SwarmRunStatus, TurnPhase};
    if pane.status != SwarmRunStatus::Running {
        return None;
    }
    let word = match pane.live.phase {
        TurnPhase::Idle | TurnPhase::Waiting => "waiting",
        TurnPhase::Thinking => "thinking",
        TurnPhase::Writing => "writing",
        TurnPhase::Tool => "tool",
    };
    Some(format!("{word} {}s", pane.live.elapsed_secs().unwrap_or(0)))
}

/// Count total visual lines when text is wrapped to inner_width.
pub fn count_rendered_lines(lines: &[Line], inner_width: usize) -> usize {
    if inner_width == 0 {
        return lines.len();
    }
    let mut total = 0;
    for line in lines {
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        if text.is_empty() {
            total += 1;
            continue;
        }
        let mut visual_rows = 1;
        let mut current_col = 0;
        for word in text.split_whitespace() {
            let word_len = word.chars().count();
            if current_col == 0 {
                current_col = word_len;
                while current_col > inner_width {
                    visual_rows += 1;
                    current_col = current_col.saturating_sub(inner_width);
                }
            } else if current_col + 1 + word_len <= inner_width {
                current_col += 1 + word_len;
            } else {
                visual_rows += 1;
                current_col = word_len;
                while current_col > inner_width {
                    visual_rows += 1;
                    current_col = current_col.saturating_sub(inner_width);
                }
            }
        }
        total += visual_rows;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn test_render_dashboard_frames() {
        let mut model = Model::default();
        model.selected_tab = 0;

        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| render(&model, frame)).unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content = format!("{:?}", buffer);

        assert!(content.contains("Frame 1: Swarm Agents"));
        assert!(content.contains("Frame 2: Activity Tracer"));
        assert!(content.contains("Frame 3: Telemetry & Metrics"));
        assert!(content.contains("Dashboard"));
    }

    #[test]
    fn test_render_acp_servers_window() {
        let mut model = Model::default();
        model.selected_tab = 1;

        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| render(&model, frame)).unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content = format!("{:?}", buffer);

        assert!(content.contains("ACP Servers Window"));
        assert!(content.contains("Server Details"));
        assert!(content.contains("ACP RPC Traffic"));
        assert!(content.contains("acp-claude"));
    }

    #[test]
    fn test_render_acp_servers_window_with_usage_left() {
        let mut model = Model::default();
        model.selected_tab = 1;
        model.acp_servers[0].tokens_used = Some(1500);
        model.acp_servers[0].context_size = Some(200_000);

        let backend = TestBackend::new(180, 50);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| render(&model, frame)).unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content = format!("{:?}", buffer);

        assert!(content.contains("1,500 / 200,000"));
        assert!(content.contains("198,500 left"));
        assert!(content.contains("99.2% available"));
        assert!(content.contains("Capacity Left:"));
    }

    #[test]
    fn test_render_swarm_tab_shows_agent_list_detail_and_controller() {
        let mut model = Model::default();
        model.select_tab(2);

        let backend = TestBackend::new(160, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&model, frame)).unwrap();
        let content = format!("{:?}", terminal.backend().buffer());

        // The agent list shows every spawned agent, labeled by its tab.
        assert!(content.contains("CTL"));
        assert!(content.contains("SPC"));
        assert!(content.contains("IMP"));
        assert!(content.contains("VER"));
        assert!(content.contains("OPS"));
        assert!(content.contains("Mission Orchestrator"));

        // The detail panel shows the selected agent's full task.
        assert!(content.contains("AGT-CTL-01, the Mission Orchestrator"));

        // The Controller pane is always present, independent of the list selection. Its
        // badge is just the shared ACP pool's status for "acp-claude" -- nobody has
        // connected it yet in this bare Model, so it reads OFFLINE, same as the ACP
        // Servers tab would show for that row.
        assert!(content.contains("Controller (Mission Orchestrator)"));
        assert!(content.contains("OFFLINE"));
    }

    #[test]
    fn test_render_controller_chat_history_and_focus() {
        let mut model = Model::default();
        model.select_tab(2);
        model.toggle_swarm_focus();
        model.controller_type_char('h');
        model.controller_type_char('i');
        model.controller_submit();
        model.push_controller_reply(Ok("hello back".to_string()));

        // The Controller's connection badge is just the shared ACP pool's status for
        // "acp-claude" -- same record the ACP Servers tab reads.
        model.apply_acp_update(crate::acp::AgentUpdate {
            server_id: crate::acp::CONTROLLER_SPEC_ID,
            outcome: crate::acp::AgentOutcome::Connected(crate::acp::HandshakeInfo {
                protocol_version: 1,
                agent_name: Some("Claude Code".to_string()),
                agent_version: None,
                capabilities: vec![],
                auth_methods: vec![],
                ping_ms: 10,
            }),
        });

        let backend = TestBackend::new(160, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&model, frame)).unwrap();
        let content = format!("{:?}", terminal.backend().buffer());

        assert!(content.contains("hi"));
        assert!(content.contains("hello back"));
        assert!(content.contains("ONLINE"));
    }

    #[test]
    fn test_render_frame_3_real_telemetry() {
        let mut model = Model::default();
        model.selected_tab = 0;
        model.dashboard_frame = 2;

        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| render(&model, frame)).unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content = format!("{:?}", buffer);

        assert!(content.contains("Frame 3: Telemetry & Metrics [ACTIVE]"));
        assert!(content.contains("CPU Load:"));
        assert!(content.contains("Memory:"));
        assert!(content.contains("GPU VRAM:"));
        assert!(content.contains("Tokens:"));
        assert!(content.contains("PID:"));
    }

    #[test]
    fn test_usage_badge_at_different_widths() {
        let line_wide = format_top_usage_badge(
            500_000,
            2_000,
            Some(128_000),
            Some(126_000),
            10,
            Some(&(0.05, "USD".to_string())),
            100,
        );
        let s_wide = line_wide.to_string();
        assert!(s_wide.contains("Tokens: 500,000"));
        assert!(s_wide.contains("Context: 2,000/128,000"));
        assert!(s_wide.contains("126,000 left"));
        assert!(s_wide.contains("Calls: 10"));
        assert!(s_wide.contains("0.0500 USD"));

        let line_med =
            format_top_usage_badge(500_000, 2_000, Some(128_000), Some(126_000), 10, None, 60);
        let s_med = line_med.to_string();
        assert!(s_med.contains("Tokens: 500,000"));
        assert!(s_med.contains("126,000 left"));

        let line_compact = format_top_usage_badge(500_000, 0, None, Some(100_000), 5, None, 40);
        let s_compact = line_compact.to_string();
        assert!(s_compact.contains("500,000 tok"));
        assert!(s_compact.contains("100,000 left"));
    }

    #[test]
    fn test_usage_is_always_displayed_on_all_tabs() {
        let mut model = Model::default();
        model.acp_servers[0].tokens_used = Some(1500);
        model.acp_servers[0].context_size = Some(128_000);

        for tab_idx in 0..3 {
            model.select_tab(tab_idx);
            let backend = TestBackend::new(140, 45);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal.draw(|frame| render(&model, frame)).unwrap();
            let content = format!("{:?}", terminal.backend().buffer());

            // Usage must be visible in the top header and footer on EVERY tab
            assert!(content.contains("Tokens:"));
            assert!(content.contains("Usage:"));
            assert!(content.contains("126,500 left"));
        }
    }

    fn draw_text(model: &Model, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|frame| render(model, frame)).unwrap();
        format!("{:?}", terminal.backend().buffer())
    }

    #[test]
    fn test_scroll_back_shows_older_lines() {
        let mut model = Model::default();
        model.select_tab(2);
        model.swarm_panes[0].output = (1..=60)
            .map(|i| format!("review line {i:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(draw_text(&model, 140, 30).contains("review line 060"));
        model.scroll_pane_top();
        let top = draw_text(&model, 140, 30);
        assert!(top.contains("review line 001"));
        assert!(!top.contains("review line 060"));
        model.scroll_pane_follow();
        assert!(draw_text(&model, 140, 30).contains("review line 060"));
    }

    #[test]
    fn test_running_pane_shows_phase_elapsed_and_thinking() {
        use crate::acp::TurnEvent;
        let mut model = Model::default();
        model.select_tab(2);
        let name = model.swarm_panes[0].name.clone();
        model.begin_run_pane(0);
        model.apply_turn_event(&name, TurnEvent::Thought("weighing two designs".into()));
        let text = draw_text(&model, 140, 30);
        assert!(text.contains("thinking"));
        assert!(text.contains("elapsed"));
        assert!(text.contains("weighing two designs"));
        model.toggle_show_thinking();
        assert!(!draw_text(&model, 140, 30).contains("weighing two designs"));
    }

    #[test]
    fn test_task_board_and_help_overlays_render() {
        let mut model = Model::default();
        let a = model.swarm_panes[0].name.clone();
        let b = model.swarm_panes[1].name.clone();
        model.apply_swarm_run_outcome(crate::swarm::RunOutcome::new(
            a,
            Ok(format!("@[{b}]: please review the parser")),
        ));
        model.show_tasks = true;
        let text = draw_text(&model, 140, 40);
        assert!(text.contains("Task board"));
        assert!(text.contains("PENDING"));
        assert!(text.contains("please review the parser"));
        model.show_tasks = false;
        model.show_help = true;
        let text = draw_text(&model, 140, 40);
        assert!(text.contains("switch project"));
    }

    #[test]
    fn test_permission_modal_shows_what_the_tool_will_do() {
        let mut model = Model::default();
        let (tx, _rx) = std::sync::mpsc::channel();
        model.set_permission_modal(crate::acp::manager::PermissionPrompt {
            id: 1,
            agent_pane: "Code Implementation".into(),
            tool_title: "Run command".into(),
            detail: "rm -rf build/".into(),
            expires_at: std::time::Instant::now() + std::time::Duration::from_secs(30),
            reply_to: tx,
        });
        let text = draw_text(&model, 140, 40);
        assert!(text.contains("rm -rf build/"));
    }

    #[test]
    fn test_controller_chat_scrolls_to_older_messages() {
        use crate::app::model::{ControllerMessage, ControllerRole};
        let mut model = Model::default();
        model.select_tab(2);
        for i in 1..=40 {
            model.controller_log.push(ControllerMessage {
                role: ControllerRole::User,
                text: format!("chat message {i:03}"),
            });
        }
        let newest = draw_text(&model, 140, 40);
        assert!(newest.contains("chat message 040"));
        assert!(!newest.contains("chat message 001"));
        model.controller_scroll_top();
        let oldest = draw_text(&model, 140, 40);
        assert!(oldest.contains("chat message 001"));
        assert!(oldest.contains("scrolled"));
        model.controller_scroll_follow();
        assert!(draw_text(&model, 140, 40).contains("chat message 040"));
    }

    #[test]
    fn test_zoom_hides_controller_and_list() {
        let mut model = Model::default();
        model.select_tab(2);
        model.swarm_zoom = true;
        let text = draw_text(&model, 140, 30);
        assert!(text.contains("ZOOM"));
        assert!(!text.contains("Agents ("));
    }

    #[test]
    fn test_auto_scroll_swarm_pane_detail() {
        let mut model = Model::default();
        model.select_tab(2);
        let long_output = (1..=50)
            .map(|i| format!("agent output line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        model.swarm_panes[0].output = long_output;

        let backend = TestBackend::new(140, 25);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&model, frame)).unwrap();
        let content = format!("{:?}", terminal.backend().buffer());

        // Must have auto-scrolled to bottom so the newest line is visible
        assert!(content.contains("agent output line 50"));
    }

    #[test]
    fn test_auto_scroll_controller_pane() {
        let mut model = Model::default();
        model.select_tab(2);
        for i in 1..=40 {
            model
                .controller_log
                .push(crate::app::model::ControllerMessage {
                    role: ControllerRole::Agent,
                    text: format!("orchestrator msg {i}"),
                });
        }

        let backend = TestBackend::new(140, 25);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&model, frame)).unwrap();
        let content = format!("{:?}", terminal.backend().buffer());

        // Must have auto-scrolled to bottom so the newest message is visible
        assert!(content.contains("orchestrator msg 40"));
    }

    #[test]
    fn test_auto_scroll_acp_server_logs() {
        let mut model = Model::default();
        model.select_tab(1);
        for i in 1..=50 {
            model.acp_servers[0]
                .recent_logs
                .push(format!("rpc traffic line {i}"));
        }

        let backend = TestBackend::new(140, 25);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&model, frame)).unwrap();
        let content = format!("{:?}", terminal.backend().buffer());

        // Must have auto-scrolled to bottom so the newest log line is visible
        assert!(content.contains("rpc traffic line 50"));
    }

    #[test]
    fn test_auto_scroll_activity_tracer() {
        let mut model = Model::default();
        model.select_tab(0);
        for i in 1..=50 {
            model.activity_log.push(format!("trace event {i}"));
        }

        let backend = TestBackend::new(140, 25);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&model, frame)).unwrap();
        let content = format!("{:?}", terminal.backend().buffer());

        // Must have auto-scrolled to bottom so the newest trace event is visible
        assert!(content.contains("trace event 50"));
    }
}
