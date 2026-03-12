//! TUI rendering.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use super::app::{App, DisplayHealth, DisplayState, ServiceDisplayInfo, UiMode};

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
            Constraint::Percentage(30), // Services
            Constraint::Percentage(70), // Logs
        ])
        .split(area);

    // Get selected service name for log filtering
    let selected_service = service_infos.get(app.selected).map(|s| s.name.as_str());

    draw_services(f, app, service_infos, chunks[0]);
    draw_logs(f, app, selected_service, chunks[1]);
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

/// Draw the footer with keybindings.
fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let keybindings = [
        ("q", "Quit"),
        ("↑↓", "Select"),
        ("r", "Restart"),
        ("s", "Stop"),
        ("c", "Clear"),
    ];

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
