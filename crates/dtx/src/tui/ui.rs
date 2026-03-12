//! TUI rendering.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::app::{App, DisplayHealth, DisplayState, ServiceDetail, ServiceDisplayInfo, UiMode};

/// Main draw function with service infos.
pub fn draw_with_infos(f: &mut Frame, app: &App, service_infos: &[ServiceDisplayInfo]) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(10),   // Main content
            Constraint::Length(3), // Footer
        ])
        .split(f.area());

    draw_header(f, chunks[0]);
    draw_main(f, app, service_infos, chunks[1]);
    draw_footer(f, app, chunks[2]);

    // Render confirm dialog overlay if in Confirm mode
    if let UiMode::Confirm { ref message, .. } = app.mode {
        draw_confirm_dialog(f, message, f.area());
    }

    if matches!(app.mode, UiMode::Help) {
        draw_help_overlay(f, f.area());
    }
}

/// Draw the header.
fn draw_header(f: &mut Frame, area: Rect) {
    let header = Paragraph::new(vec![Line::from(vec![
        Span::styled(
            " dtx ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled("native", Style::default().fg(Color::Cyan)),
        Span::raw(" process manager"),
    ])])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    f.render_widget(header, area);
}

/// Draw the main content area (services + logs).
fn draw_main(f: &mut Frame, app: &App, service_infos: &[ServiceDisplayInfo], area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(70),
        ])
        .split(area);

    let selected_service = service_infos.get(app.selected).map(|s| s.name.as_str());

    draw_services(f, app, service_infos, chunks[0]);

    if let (UiMode::Detail, Some(ref detail)) = (&app.mode, &app.detail) {
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(10),
                Constraint::Min(5),
            ])
            .split(chunks[1]);
        draw_detail(f, detail, right_chunks[0]);
        draw_logs(f, app, selected_service, right_chunks[1]);
    } else {
        draw_logs(f, app, selected_service, chunks[1]);
    }
}

/// Draw the service list.
fn draw_services(f: &mut Frame, app: &App, service_infos: &[ServiceDisplayInfo], area: Rect) {
    let items: Vec<ListItem> = service_infos
        .iter()
        .enumerate()
        .map(|(i, svc)| {
            let (indicator, color) = match &svc.state {
                DisplayState::Running { .. } => ("●", Color::Green),
                DisplayState::Starting => ("◐", Color::Yellow),
                DisplayState::Pending => ("○", Color::Yellow),
                DisplayState::Stopped => ("○", Color::DarkGray),
                DisplayState::Completed { .. } => ("✓", Color::Blue),
                DisplayState::Failed { .. } => ("✗", Color::Red),
            };

            let state_label = match &svc.state {
                DisplayState::Running { .. } => "RUN",
                DisplayState::Starting => "STR",
                DisplayState::Pending => "PND",
                DisplayState::Stopped => "STP",
                DisplayState::Completed { .. } => "DON",
                DisplayState::Failed { .. } => "ERR",
            };

            let restart_info = if svc.restarts > 0 {
                format!(" ({})", svc.restarts)
            } else {
                String::new()
            };

            let health_bg = match &svc.health {
                DisplayHealth::Healthy if matches!(svc.state, DisplayState::Running { .. }) => {
                    Some(Color::Rgb(20, 40, 20))
                }
                DisplayHealth::Unhealthy { .. } => Some(Color::Rgb(50, 20, 20)),
                _ => None,
            };

            let port_span = match svc.port {
                Some(p) => Span::styled(format!(" :{}", p), Style::default().fg(Color::DarkGray)),
                None => Span::raw(""),
            };

            let style = if i == app.selected {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else if let Some(bg) = health_bg {
                Style::default().bg(bg)
            } else {
                Style::default()
            };

            let content = Line::from(vec![
                Span::styled(format!(" {} ", indicator), Style::default().fg(color)),
                Span::styled(&svc.name, style),
                port_span,
                Span::styled(
                    format!(" [{}]{}", state_label, restart_info),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);

            ListItem::new(content).style(style)
        })
        .collect();

    let services = List::new(items).block(
        Block::default()
            .title(" Services ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );

    f.render_widget(services, area);
}

/// Check if a log line looks like an error based on content.
/// Uses byte-level case-insensitive matching to avoid allocating on the render hot path.
fn is_error_line(content: &str) -> bool {
    fn starts_with_ci(s: &str, pat: &str) -> bool {
        s.as_bytes()
            .get(..pat.len())
            .is_some_and(|b| b.eq_ignore_ascii_case(pat.as_bytes()))
    }
    fn contains_ci(s: &str, pat: &str) -> bool {
        s.as_bytes()
            .windows(pat.len())
            .any(|w| w.eq_ignore_ascii_case(pat.as_bytes()))
    }
    starts_with_ci(content, "error")
        || starts_with_ci(content, "fatal")
        || starts_with_ci(content, "panic")
        || contains_ci(content, "\"level\":\"error\"")
        || contains_ci(content, "\"level\":\"fatal\"")
        || contains_ci(content, "[error]")
        || contains_ci(content, "[fatal]")
        || contains_ci(content, " error:")
        || contains_ci(content, " fatal:")
        || contains_ci(content, "level=error")
        || contains_ci(content, "level=fatal")
}

/// Draw the service detail panel.
fn draw_detail(f: &mut Frame, detail: &ServiceDetail, area: Rect) {
    let state_str = match &detail.state {
        DisplayState::Running { pid } => format!("Running (PID {})", pid),
        DisplayState::Starting => "Starting".to_string(),
        DisplayState::Pending => "Pending".to_string(),
        DisplayState::Stopped => "Stopped".to_string(),
        DisplayState::Completed { exit_code } => format!("Completed (exit {})", exit_code),
        DisplayState::Failed { error } => {
            format!("Failed{}", error.as_ref().map(|e| format!(": {}", e)).unwrap_or_default())
        }
    };

    let health_str = match &detail.health {
        DisplayHealth::Unknown => "Unknown".to_string(),
        DisplayHealth::Healthy => "Healthy".to_string(),
        DisplayHealth::Unhealthy { reason } => format!("Unhealthy: {}", reason),
    };

    let (health_style, health_symbol) = match &detail.health {
        DisplayHealth::Healthy => (Style::default().fg(Color::Green), "♥"),
        DisplayHealth::Unhealthy { .. } => (Style::default().fg(Color::Red), "✗"),
        DisplayHealth::Unknown => (Style::default().fg(Color::DarkGray), "?"),
    };

    let uptime_str = detail.uptime.map(|d| {
        let secs = d.as_secs();
        if secs >= 3600 {
            format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
        } else if secs >= 60 {
            format!("{}m {}s", secs / 60, secs % 60)
        } else {
            format!("{}s", secs)
        }
    }).unwrap_or_else(|| "-".to_string());

    let port_str = detail.port.map(|p| p.to_string()).unwrap_or_else(|| "-".to_string());

    let mut lines = vec![
        Line::from(vec![
            Span::styled(" State:   ", Style::default().fg(Color::DarkGray)),
            Span::raw(&state_str),
        ]),
        Line::from(vec![
            Span::styled(" Health:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{} {}", health_symbol, health_str), health_style),
        ]),
        Line::from(vec![
            Span::styled(" Port:    ", Style::default().fg(Color::DarkGray)),
            Span::raw(&port_str),
            Span::styled("    Uptime: ", Style::default().fg(Color::DarkGray)),
            Span::raw(&uptime_str),
        ]),
        Line::from(vec![
            Span::styled(" Restart: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{}", detail.restart_count)),
        ]),
    ];

    if let Some(ref cmd) = detail.command {
        lines.push(Line::from(vec![
            Span::styled(" Command: ", Style::default().fg(Color::DarkGray)),
            Span::raw(cmd),
        ]));
    }
    if !detail.dependencies.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(" Deps:    ", Style::default().fg(Color::DarkGray)),
            Span::raw(detail.dependencies.join(", ")),
        ]));
    }

    let title = format!(" {} — Detail ", detail.name);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);
}

/// Draw the log panel (filtered by selected service).
fn draw_logs(f: &mut Frame, app: &App, selected_service: Option<&str>, area: Rect) {
    // Calculate how many lines we can show
    let inner_height = area.height.saturating_sub(2) as usize;

    // Get visible logs from LogStore
    let total = app.filtered_log_count();
    let visible = match &app.active_filter {
        Some(filter) => app.log_store.get_visible_filtered(
            selected_service,
            filter,
            app.log_scroll.offset_from_bottom,
            inner_height,
        ),
        None => app.log_store.get_visible(
            selected_service,
            app.log_scroll.offset_from_bottom,
            inner_height,
        ),
    };
    let end = total.saturating_sub(app.log_scroll.offset_from_bottom);
    let search_query = app.search_state.as_ref().map(|s| s.query.to_lowercase());

    let visible_logs: Vec<Line> = visible
        .iter()
        .map(|log| {
            let base_style = if log.is_stderr || is_error_line(&log.content) {
                Style::default().fg(Color::Red)
            } else {
                Style::default()
            };

            if let Some(ref query) = search_query {
                let content_lower = log.content.to_lowercase();
                let mut spans = Vec::new();
                let mut last_end = 0;

                for (start, _) in content_lower.match_indices(query) {
                    if start > last_end {
                        spans.push(Span::styled(&log.content[last_end..start], base_style));
                    }
                    let end = start + query.len();
                    spans.push(Span::styled(
                        &log.content[start..end],
                        Style::default().bg(Color::Yellow).fg(Color::Black),
                    ));
                    last_end = end;
                }
                if last_end < log.content.len() {
                    spans.push(Span::styled(&log.content[last_end..], base_style));
                }
                Line::from(spans)
            } else {
                Line::from(vec![Span::styled(&log.content, base_style)])
            }
        })
        .collect();

    // Title shows selected service name and scroll position
    let title = match selected_service {
        Some(name) => {
            let mut t = format!(" Logs: {} ", name);
            if let Some(ref filter) = app.active_filter {
                t = format!(" Logs: {} [filter: {}] ", name, filter);
            }
            if app.log_scroll.offset_from_bottom > 0 {
                t = format!("{} [{}/{}] ", t.trim(), end, total);
                format!(" {} ", t.trim())
            } else {
                t
            }
        }
        None => " Logs ".to_string(),
    };

    let logs = Paragraph::new(visible_logs).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );

    f.render_widget(logs, area);

    // Render search bar if in search mode
    if let UiMode::Search { ref query, .. } = app.mode {
        let search_area = Rect {
            x: area.x + 1,
            y: area.y + area.height.saturating_sub(2),
            width: area.width.saturating_sub(2),
            height: 1,
        };
        let search_text = Line::from(vec![
            Span::styled("/", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(query.as_str()),
            Span::styled("█", Style::default().fg(Color::Yellow)),
        ]);
        f.render_widget(Paragraph::new(search_text), search_area);
    }

    // Render filter bar if in filter mode
    if let UiMode::Filter { ref query, .. } = app.mode {
        let filter_area = Rect {
            x: area.x + 1,
            y: area.y + area.height.saturating_sub(2),
            width: area.width.saturating_sub(2),
            height: 1,
        };
        let filter_text = Line::from(vec![
            Span::styled("filter: ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::raw(query.as_str()),
            Span::styled("█", Style::default().fg(Color::Magenta)),
        ]);
        f.render_widget(Paragraph::new(filter_text), filter_area);
    }
}

/// Draw the footer with context-aware keybindings.
fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let keybindings: Vec<(&str, &str)> = match &app.mode {
        UiMode::Normal => vec![
            ("j/k", "Navigate"),
            ("Enter", "Detail"),
            ("/", "Search"),
            ("F", "Filter"),
            ("s", "Stop"),
            ("S", "Start"),
            ("r", "Restart"),
            ("?", "Help"),
            ("q", "Quit"),
        ],
        UiMode::Detail => vec![
            ("Esc", "Back"),
            ("j/k", "Navigate"),
            ("s", "Stop"),
            ("S", "Start"),
            ("r", "Restart"),
        ],
        UiMode::Search { .. } => vec![
            ("Enter", "Find"),
            ("Esc", "Cancel"),
        ],
        UiMode::Filter { .. } => vec![
            ("Enter", "Apply"),
            ("Esc", "Clear"),
        ],
        UiMode::Confirm { .. } => vec![
            ("y", "Yes"),
            ("n", "No"),
        ],
        UiMode::Help => vec![
            ("?/Esc", "Close"),
        ],
    };

    let mut spans = Vec::new();
    for (i, (key, action)) in keybindings.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            format!("[{}]", key),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(format!(" {}", action)));
    }

    // Add reload indicator if config changed
    if app.config_changed {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            "[a] Reload",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
    }

    // Add status message if present
    if let Some(ref msg) = app.status_message {
        spans.push(Span::raw("  │  "));
        spans.push(Span::styled(msg, Style::default().fg(Color::Yellow)));
    }

    let footer = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    f.render_widget(footer, area);
}

fn draw_help_overlay(f: &mut Frame, area: Rect) {
    let width = 50u16.min(area.width.saturating_sub(4));
    let height = 20u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let overlay_area = Rect::new(x, y, width, height);

    f.render_widget(Clear, overlay_area);

    let key_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let group_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);

    let lines = vec![
        Line::from(Span::styled(" Navigation", group_style)),
        Line::from(vec![
            Span::styled("  j/k ↑↓  ", key_style), Span::raw("Select service"),
        ]),
        Line::from(vec![
            Span::styled("  g/G     ", key_style), Span::raw("Jump to top/bottom"),
        ]),
        Line::from(vec![
            Span::styled("  Enter   ", key_style), Span::raw("Detail view"),
        ]),
        Line::from(vec![
            Span::styled("  Esc     ", key_style), Span::raw("Back / Close"),
        ]),
        Line::raw(""),
        Line::from(Span::styled(" Logs", group_style)),
        Line::from(vec![
            Span::styled("  PgUp/Dn ", key_style), Span::raw("Scroll logs"),
        ]),
        Line::from(vec![
            Span::styled("  /       ", key_style), Span::raw("Search logs"),
        ]),
        Line::from(vec![
            Span::styled("  n/N     ", key_style), Span::raw("Next/prev match"),
        ]),
        Line::from(vec![
            Span::styled("  F       ", key_style), Span::raw("Filter logs"),
        ]),
        Line::from(vec![
            Span::styled("  c       ", key_style), Span::raw("Clear logs"),
        ]),
        Line::raw(""),
        Line::from(Span::styled(" Control", group_style)),
        Line::from(vec![
            Span::styled("  s       ", key_style), Span::raw("Stop service"),
        ]),
        Line::from(vec![
            Span::styled("  S       ", key_style), Span::raw("Start service"),
        ]),
        Line::from(vec![
            Span::styled("  r       ", key_style), Span::raw("Restart service"),
        ]),
        Line::from(vec![
            Span::styled("  d       ", key_style), Span::raw("Delete service"),
        ]),
        Line::from(vec![
            Span::styled("  q       ", key_style), Span::raw("Quit"),
        ]),
    ];

    let help = Paragraph::new(lines).block(
        Block::default()
            .title(" Help — ? to close ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );

    f.render_widget(help, overlay_area);
}

fn draw_confirm_dialog(f: &mut Frame, message: &str, area: Rect) {
    let width = (message.len() as u16 + 6).min(area.width.saturating_sub(4));
    let height = 5;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let dialog_area = Rect::new(x, y, width, height);

    f.render_widget(Clear, dialog_area);

    let lines = vec![
        Line::raw(""),
        Line::from(vec![Span::raw(format!(" {} ", message))]),
        Line::from(vec![
            Span::styled(" [y]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(" Yes  "),
            Span::styled("[n]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(" No "),
        ]),
    ];

    let dialog = Paragraph::new(lines).block(
        Block::default()
            .title(" Confirm ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );

    f.render_widget(dialog, dialog_area);
}
