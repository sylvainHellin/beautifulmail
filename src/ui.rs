use ratatui::layout::{Alignment, Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap};
use ratatui::Frame;

use crate::app::{App, Focus, Mailbox};
use crate::theme;

/// Render the entire UI from the current app state.
pub fn view(app: &App, frame: &mut Frame) {
    let area = frame.area();

    // Vertical: main area + status bar
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let main_area = outer[0];
    let status_area = outer[1];

    let show_right = app.terminal_width >= 80;
    let show_sidebar = app.terminal_width >= 40;

    if show_right {
        // Two-column layout: left (sidebar + list) | right (headers + body)
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(35),
                Constraint::Percentage(65),
            ])
            .split(main_area);

        let left_col = columns[0];
        let right_col = columns[1];

        // Left column: sidebar (compact) + email list
        let left_panels = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7), // sidebar: 4 mailboxes + border
                Constraint::Min(0),    // email list fills rest
            ])
            .split(left_col);

        render_sidebar(app, frame, left_panels[0]);
        render_email_list(app, frame, left_panels[1]);

        // Right column: headers (fixed 6 lines, matching sidebar) + body
        let right_panels = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),
                Constraint::Min(0),
            ])
            .split(right_col);

        render_headers(app, frame, right_panels[0]);
        render_body(app, frame, right_panels[1]);
    } else if show_sidebar {
        // Stacked: sidebar + email list (no right column)
        let left_panels = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),
                Constraint::Min(0),
            ])
            .split(main_area);

        render_sidebar(app, frame, left_panels[0]);
        render_email_list(app, frame, left_panels[1]);
    } else {
        // List only
        render_email_list(app, frame, main_area);
    }

    // Status bar
    render_status_bar(app, frame, status_area);

    // Confirmation dialog overlay (renders on top of everything)
    if let Some(dialog) = &app.confirm_dialog {
        render_confirm_dialog(dialog, frame, area);
    }

    // Help overlay (renders on top of everything)
    if app.show_help {
        render_help_overlay(frame, area);
    }
}

/// Render the sidebar with mailbox list.
fn render_sidebar(app: &App, frame: &mut Frame, area: Rect) {
    let border_style = pane_border_style(app.focus, Focus::Sidebar);
    let block = Block::default()
        .title(" Mail ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .style(Style::default().bg(theme::BASE));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    for (i, mailbox) in Mailbox::ALL.iter().enumerate() {
        let is_selected = *mailbox == app.active_mailbox;
        let is_highlighted = app.focus == Focus::Sidebar && i == app.sidebar_index;
        let count = app.mailbox_counts[i];

        let marker = if is_selected { ">" } else { " " };

        let label = format!(
            "{} {} {} {:>2}",
            marker,
            mailbox.icon(),
            mailbox.label(),
            count
        );

        let style = if is_highlighted {
            Style::default()
                .fg(theme::GREEN)
                .add_modifier(Modifier::BOLD)
        } else if is_selected {
            Style::default().fg(theme::BLUE)
        } else {
            Style::default().fg(theme::TEXT)
        };

        lines.push(Line::from(Span::styled(label, style)));
    }

    let sidebar_content = Paragraph::new(lines);
    frame.render_widget(sidebar_content, inner);
}

/// Render the email list as a table, with optional search bar.
fn render_email_list(app: &App, frame: &mut Frame, area: Rect) {
    let border_style = pane_border_style(app.focus, Focus::List);
    let title = if !app.search_query.is_empty() && app.focus != Focus::Search {
        if app.search_includes_body {
            format!(" {} (content search) ", app.active_mailbox.label())
        } else {
            format!(" {} (filtered) ", app.active_mailbox.label())
        }
    } else {
        format!(" {} ", app.active_mailbox.label())
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .style(Style::default().bg(theme::BASE));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split inner area for optional search bar
    let search_visible = app.focus == Focus::Search || !app.search_query.is_empty();
    let (search_area, list_area) = if search_visible {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(inner);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, inner)
    };

    // Render search bar
    if let Some(search_rect) = search_area {
        let prefix = if app.search_includes_body { "\\" } else { "/" };
        let mut spans = vec![
            Span::styled(prefix, Style::default().fg(theme::BLUE)),
            Span::styled(app.search_query.as_str(), Style::default().fg(theme::TEXT)),
        ];
        if app.focus == Focus::Search {
            spans.push(Span::styled(
                "\u{2588}",
                Style::default().fg(theme::BLUE),
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), search_rect);
    }

    if app.emails.is_empty() {
        let msg = if !app.search_query.is_empty() {
            "  No matching emails".to_string()
        } else {
            format!(
                "\n  No emails in {}\n\n  Press f to fetch new emails",
                app.active_mailbox.label()
            )
        };
        let empty =
            Paragraph::new(msg).style(Style::default().fg(theme::SUBTEXT0));
        frame.render_widget(empty, list_area);
        return;
    }

    // Calculate column widths from available space
    let available_width = list_area.width as usize;
    let date_width = 10; // YYYY-MM-DD
    let spacing = 3; // gaps between columns

    if available_width > 45 {
        // 3 columns: DATE + CONTACT + SUBJECT
        let contact_width =
            15.min(available_width.saturating_sub(date_width + spacing + 10));
        let subject_width =
            available_width.saturating_sub(date_width + contact_width + spacing);

        let header = Row::new(vec![
            Cell::from("DATE").style(Style::default().fg(theme::SUBTEXT0)),
            Cell::from("CONTACT").style(Style::default().fg(theme::SUBTEXT0)),
            Cell::from("SUBJECT").style(Style::default().fg(theme::SUBTEXT0)),
        ])
        .height(1);

        let rows: Vec<Row> = app
            .emails
            .iter()
            .enumerate()
            .map(|(i, email)| {
                let is_selected = i == app.list_index;
                let contact = truncate(
                    email.display_contact(app.active_mailbox),
                    contact_width,
                );
                let subject = truncate(&email.subject, subject_width);

                let row_style = if is_selected {
                    Style::default().bg(theme::SURFACE0).fg(theme::GREEN)
                } else {
                    Style::default().fg(theme::TEXT)
                };

                Row::new(vec![
                    Cell::from(email.date_display.clone()),
                    Cell::from(contact),
                    Cell::from(subject),
                ])
                .style(row_style)
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(date_width as u16),
                Constraint::Length(contact_width as u16),
                Constraint::Min(subject_width as u16),
            ],
        )
        .header(header)
        .column_spacing(1)
        .row_highlight_style(
            Style::default()
                .bg(theme::SURFACE0)
                .fg(theme::GREEN)
                .add_modifier(Modifier::BOLD),
        );

        let mut state = TableState::default();
        state.select(Some(app.list_index));
        frame.render_stateful_widget(table, list_area, &mut state);
    } else {
        // 2 columns: DATE + SUBJECT only
        let subject_width = available_width.saturating_sub(date_width + 2);

        let header = Row::new(vec![
            Cell::from("DATE").style(Style::default().fg(theme::SUBTEXT0)),
            Cell::from("SUBJECT").style(Style::default().fg(theme::SUBTEXT0)),
        ])
        .height(1);

        let rows: Vec<Row> = app
            .emails
            .iter()
            .enumerate()
            .map(|(i, email)| {
                let is_selected = i == app.list_index;
                let subject = truncate(&email.subject, subject_width);

                let row_style = if is_selected {
                    Style::default().bg(theme::SURFACE0).fg(theme::GREEN)
                } else {
                    Style::default().fg(theme::TEXT)
                };

                Row::new(vec![
                    Cell::from(email.date_display.clone()),
                    Cell::from(subject),
                ])
                .style(row_style)
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(date_width as u16),
                Constraint::Min(subject_width as u16),
            ],
        )
        .header(header)
        .column_spacing(1)
        .row_highlight_style(
            Style::default()
                .bg(theme::SURFACE0)
                .fg(theme::GREEN)
                .add_modifier(Modifier::BOLD),
        );

        let mut state = TableState::default();
        state.select(Some(app.list_index));
        frame.render_stateful_widget(table, list_area, &mut state);
    }
}

/// Render a single header field as a styled Line.
fn header_line<'a>(label: &'a str, value: &'a str, color: Color) -> Line<'a> {
    Line::from(vec![
        Span::styled(
            format!(" {label}: "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(value, Style::default().fg(color)),
    ])
}

/// Render the email headers panel (fixed height, scrollable when focused).
fn render_headers(app: &App, frame: &mut Frame, area: Rect) {
    let border_style = pane_border_style(app.focus, Focus::Headers);
    let block = Block::default()
        .title(" Headers ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .style(Style::default().bg(theme::BASE));

    let selected = app.emails.get(app.list_index);
    if selected.is_none() {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new("  No email selected")
                .style(Style::default().fg(theme::SUBTEXT0)),
            inner,
        );
        return;
    }

    let email = selected.unwrap();
    let mut lines: Vec<Line> = Vec::new();

    lines.push(header_line("From", &email.from, theme::GREEN));
    lines.push(header_line("To", &email.to, theme::BLUE));
    if let Some(cc) = &email.cc {
        if !cc.is_empty() {
            lines.push(header_line("Cc", cc, theme::BLUE));
        }
    }
    lines.push(header_line("Subj", &email.subject, theme::YELLOW));

    // Date and status on one line
    let date_status = format!("{}  [{}]", email.date_display, email.status);
    lines.push(header_line("Date", &date_status, theme::MAUVE));

    let content = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.headers_scroll, 0));
    frame.render_widget(content, area);
}

/// Render the email body panel (scrollable, focused via Focus::Preview).
fn render_body(app: &App, frame: &mut Frame, area: Rect) {
    let border_style = pane_border_style(app.focus, Focus::Preview);
    let block = Block::default()
        .title(" Body ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .style(Style::default().bg(theme::BASE));

    let selected = app.emails.get(app.list_index);
    if selected.is_none() {
        frame.render_widget(block, area);
        return;
    }

    let email = selected.unwrap();
    let body = email.body.replace("{{SIGNATURE}}", "[signature]");

    // Pre-wrap text ourselves so quoted continuation lines keep their prefix
    let inner_width = block.inner(area).width as usize;
    let lines: Vec<Line> = wrap_and_style_body(&body, inner_width);

    let content = Paragraph::new(lines)
        .block(block)
        .scroll((app.preview_scroll, 0));

    frame.render_widget(content, area);
}

/// Parse quote depth and return (depth, remaining content after `>` markers).
fn parse_quote_depth(line: &str) -> (usize, &str) {
    let trimmed = line.trim_start();
    let mut depth = 0;
    let mut pos = 0;
    let bytes = trimmed.as_bytes();
    while pos < bytes.len() && bytes[pos] == b'>' {
        depth += 1;
        pos += 1;
        if pos < bytes.len() && bytes[pos] == b' ' {
            pos += 1;
        }
    }
    (depth, &trimmed[pos..])
}

/// Wrap body text manually, preserving quote prefixes on continuation lines.
fn wrap_and_style_body<'a>(body: &'a str, width: usize) -> Vec<Line<'a>> {
    let mut result: Vec<Line> = Vec::new();

    for line in body.lines() {
        // Signature placeholder
        if line.trim() == "[signature]" {
            result.push(Line::from(Span::styled(
                "  -- signature --".to_string(),
                Style::default().fg(theme::OVERLAY0),
            )));
            continue;
        }

        let (depth, content) = parse_quote_depth(line);

        if depth == 0 {
            // Regular or attribution line -- simple word wrap
            let style = if is_attribution(line.trim()) {
                Style::default()
                    .fg(theme::SUBTEXT0)
                    .add_modifier(Modifier::ITALIC)
            } else {
                Style::default().fg(theme::TEXT)
            };
            for wrapped in word_wrap(content, width) {
                result.push(Line::from(Span::styled(wrapped, style)));
            }
        } else {
            // Quoted line -- wrap with prefix on every continuation
            let prefix = "\u{2502} ".repeat(depth);
            let prefix_width = depth * 2; // "│ " is 2 chars per level
            let text_width = width.saturating_sub(prefix_width);

            let is_attr = is_attribution(content.trim());
            let text_style = if is_attr {
                Style::default()
                    .fg(theme::SUBTEXT0)
                    .add_modifier(Modifier::ITALIC)
            } else {
                match depth {
                    1 => Style::default().fg(theme::OVERLAY0),
                    _ => Style::default().fg(theme::SURFACE0),
                }
            };

            if text_width < 5 {
                // Too narrow to wrap meaningfully
                result.push(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(theme::BLUE)),
                    Span::styled(content.to_string(), text_style),
                ]));
            } else {
                for wrapped in word_wrap(content, text_width) {
                    result.push(Line::from(vec![
                        Span::styled(prefix.clone(), Style::default().fg(theme::BLUE)),
                        Span::styled(wrapped, text_style),
                    ]));
                }
            }
        }
    }

    result
}

/// Check if a line is an attribution ("On ..., ... wrote:").
fn is_attribution(line: &str) -> bool {
    line.starts_with("On ") && line.ends_with("wrote:")
}

/// Simple word wrap: split text into lines that fit within `width` chars.
/// Breaks on whitespace where possible, otherwise hard-breaks.
fn word_wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        let char_count = remaining.chars().count();
        if char_count <= width {
            lines.push(remaining.to_string());
            break;
        }

        // Find the last space within the width limit
        let byte_at_width: usize = remaining
            .char_indices()
            .nth(width)
            .map_or(remaining.len(), |(i, _)| i);

        let break_at = remaining[..byte_at_width]
            .rfind(' ')
            .map(|i| i + 1) // break after the space
            .unwrap_or(byte_at_width); // hard break if no space

        let (chunk, rest) = remaining.split_at(break_at);
        lines.push(chunk.trim_end().to_string());
        remaining = rest.trim_start();
    }

    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Render the status bar at the bottom.
fn render_status_bar(app: &App, frame: &mut Frame, area: Rect) {
    // Right side: optional WATCHING indicator + mailbox name + count
    let total = app.mailbox_counts[app.active_mailbox.index()];
    let shown = app.emails.len();
    let watch_prefix = if app.watcher_active { "WATCHING " } else { "" };
    let mailbox_text = if !app.search_query.is_empty() && shown != total {
        format!("{} {}/{} ", app.active_mailbox.label(), shown, total)
    } else {
        format!("{} {} ", app.active_mailbox.label(), total)
    };
    let right_len = (watch_prefix.len() + mailbox_text.len() + 1) as u16;

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(right_len)])
        .split(area);

    // Left side: hints or status message
    let left_content = if let Some(msg) = &app.status_message {
        Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(msg.as_str(), Style::default().fg(theme::GREEN)),
        ])
    } else {
        match app.focus {
            Focus::Sidebar => Line::from(vec![
                hint_span(" j/k"),
                desc_span("nav "),
                hint_span("Enter"),
                desc_span("select "),
                hint_span("/"),
                desc_span("search "),
                hint_span("?"),
                desc_span("help "),
                hint_span("q"),
                desc_span("quit"),
            ]),
            Focus::List => Line::from(vec![
                hint_span(" e"),
                desc_span("edit "),
                hint_span("r"),
                desc_span("reply "),
                hint_span("a"),
                desc_span("archive "),
                hint_span("A"),
                desc_span("approve "),
                hint_span("x"),
                desc_span("send "),
                hint_span("n"),
                desc_span("new "),
                hint_span("/"),
                desc_span("filter "),
                hint_span("\\"),
                desc_span("search "),
                hint_span("?"),
                desc_span("help"),
            ]),
            Focus::Headers => Line::from(vec![
                hint_span(" j/k"),
                desc_span("scroll "),
                hint_span("h"),
                desc_span("back "),
                hint_span("l"),
                desc_span("body "),
                hint_span("?"),
                desc_span("help "),
                hint_span("q"),
                desc_span("quit"),
            ]),
            Focus::Preview => Line::from(vec![
                hint_span(" j/k"),
                desc_span("scroll "),
                hint_span("d/u"),
                desc_span("page "),
                hint_span("h"),
                desc_span("back "),
                hint_span("/"),
                desc_span("search "),
                hint_span("?"),
                desc_span("help "),
                hint_span("q"),
                desc_span("quit"),
            ]),
            Focus::Search => {
                let mut spans = vec![
                    hint_span(" Enter"),
                    desc_span("confirm "),
                    hint_span("Esc"),
                    desc_span("cancel"),
                ];
                if app.search_includes_body {
                    spans.push(desc_span(" (content search)"));
                }
                Line::from(spans)
            }
        }
    };

    let left = Paragraph::new(left_content)
        .style(Style::default().fg(theme::SUBTEXT0).bg(theme::SURFACE0));
    frame.render_widget(left, chunks[0]);

    let mut right_spans = vec![Span::styled(" ", Style::default())];
    if app.watcher_active {
        right_spans.push(Span::styled(
            watch_prefix,
            Style::default().fg(theme::TEAL),
        ));
    }
    right_spans.push(Span::styled(
        mailbox_text,
        Style::default().fg(theme::BLUE),
    ));
    let right = Paragraph::new(Line::from(right_spans))
        .style(Style::default().bg(theme::SURFACE0))
        .alignment(Alignment::Right);
    frame.render_widget(right, chunks[1]);
}

/// Render a centered confirmation dialog overlay.
fn render_confirm_dialog(
    dialog: &crate::app::ConfirmDialog,
    frame: &mut Frame,
    area: Rect,
) {
    // Size the dialog
    let dialog_width = 40u16.min(area.width.saturating_sub(4));
    let dialog_height = 7u16;

    // Center it
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(dialog_width)])
        .flex(Flex::Center)
        .split(area);

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(dialog_height)])
        .flex(Flex::Center)
        .split(horizontal[0]);

    let dialog_area = vertical[0];

    // Clear the area behind the dialog
    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::YELLOW))
        .style(Style::default().bg(theme::BASE));

    let lines = vec![
        Line::from(Span::styled(
            &dialog.title,
            Style::default()
                .fg(theme::YELLOW)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            truncate(&dialog.detail, dialog_width.saturating_sub(4) as usize),
            Style::default().fg(theme::TEXT),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [y]", Style::default().fg(theme::GREEN)),
            Span::styled("es  ", Style::default().fg(theme::TEXT)),
            Span::styled("[n]", Style::default().fg(theme::RED)),
            Span::styled("o", Style::default().fg(theme::TEXT)),
        ]),
    ];

    let content = Paragraph::new(lines).block(block);
    frame.render_widget(content, dialog_area);
}

/// Styled span for a keybinding hint (e.g. "Enter").
fn hint_span(key: &str) -> Span<'_> {
    Span::styled(key, Style::default().fg(theme::BLUE))
}

/// Styled span for a keybinding description (e.g. "edit ").
fn desc_span(desc: &str) -> Span<'_> {
    Span::styled(desc, Style::default().fg(theme::SUBTEXT0))
}

/// Truncate a string to fit in `max_width` chars, adding ellipsis if needed.
fn truncate(s: &str, max_width: usize) -> String {
    if max_width <= 3 {
        return s.chars().take(max_width).collect();
    }
    let char_count = s.chars().count();
    if char_count <= max_width {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_width - 1).collect();
        format!("{truncated}\u{2026}")
    }
}

/// Render a full-screen help overlay listing all keybindings.
fn render_help_overlay(frame: &mut Frame, area: Rect) {
    let help_width = 50u16.min(area.width.saturating_sub(4));
    let help_height = 38u16.min(area.height.saturating_sub(2));

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(help_width)])
        .flex(Flex::Center)
        .split(area);

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(help_height)])
        .flex(Flex::Center)
        .split(horizontal[0]);

    let help_area = vertical[0];
    frame.render_widget(Clear, help_area);

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::BLUE))
        .style(Style::default().bg(theme::BASE));

    let section = |title: &str| -> Line {
        Line::from(Span::styled(
            format!("  {title}"),
            Style::default()
                .fg(theme::MAUVE)
                .add_modifier(Modifier::BOLD),
        ))
    };

    let entry = |key: &str, desc: &str| -> Line {
        Line::from(vec![
            Span::styled(format!("  {key:<12}"), Style::default().fg(theme::BLUE)),
            Span::styled(desc.to_string(), Style::default().fg(theme::TEXT)),
        ])
    };

    let lines = vec![
        section("GLOBAL"),
        entry("q", "Quit"),
        entry("1/2/3/4", "Jump to mailbox"),
        entry("s", "Focus sidebar"),
        entry("Tab", "Cycle focus forward"),
        entry("Shift+Tab", "Cycle focus backward"),
        entry("/", "Filter by metadata"),
        entry("\\", "Search email content"),
        entry("?", "Toggle this help"),
        Line::from(""),
        section("SIDEBAR"),
        entry("j/k", "Navigate mailboxes"),
        entry("Enter/l", "Select mailbox"),
        entry("Esc/h", "Return to list"),
        Line::from(""),
        section("EMAIL LIST"),
        entry("j/k", "Navigate emails"),
        entry("gg / G", "Jump to top / bottom"),
        entry("h / l", "Focus sidebar / body"),
        entry("Enter / e", "Open in editor"),
        entry("r / R", "Reply / Reply-all"),
        entry("a", "Archive"),
        entry("d", "Delete"),
        entry("A", "Approve draft"),
        entry("x / X", "Send / Send all approved"),
        entry("y", "Copy file path"),
        entry("n", "New draft"),
        entry("f / F", "Fetch / Full sync"),
        Line::from(""),
        section("HEADERS"),
        entry("j/k", "Scroll headers"),
        entry("h / l", "Back to list / body"),
        Line::from(""),
        section("BODY"),
        entry("j/k", "Scroll line by line"),
        entry("d/u", "Half-page down / up"),
        entry("Esc/h", "Return to list"),
    ];

    let help = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(help, help_area);
}

/// Return border style based on whether this pane is focused.
fn pane_border_style(current_focus: Focus, pane: Focus) -> Style {
    let focused =
        current_focus == pane || (current_focus == Focus::Search && pane == Focus::List);
    if focused {
        Style::default().fg(theme::BLUE)
    } else {
        Style::default().fg(theme::OVERLAY0)
    }
}
