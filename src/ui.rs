use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap};
use ratatui::Frame;

use crate::app::{App, Focus, Mailbox};
use crate::theme;

/// Render the entire UI from the current app state.
pub fn view(app: &App, frame: &mut Frame) {
    let area = frame.area();

    // Vertical split: main area + status bar
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let main_area = outer[0];
    let status_area = outer[1];

    // Horizontal split for main area: sidebar + list + preview
    let show_preview = app.terminal_width >= 80;
    let show_sidebar = app.terminal_width >= 40;

    let panels = if show_sidebar && show_preview {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(14),
                Constraint::Percentage(50),
                Constraint::Percentage(50),
            ])
            .split(main_area)
    } else if show_sidebar {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(14), Constraint::Min(0)])
            .split(main_area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0)])
            .split(main_area)
    };

    // Render panels
    let mut panel_idx = 0;

    if show_sidebar {
        render_sidebar(app, frame, panels[panel_idx]);
        panel_idx += 1;
    }

    // Email list panel
    render_email_list(app, frame, panels[panel_idx]);
    panel_idx += 1;

    if show_preview && panel_idx < panels.len() {
        render_preview(app, frame, panels[panel_idx]);
    }

    // Status bar
    render_status_bar(app, frame, status_area);

    // Confirmation dialog overlay (renders on top of everything)
    if let Some(dialog) = &app.confirm_dialog {
        render_confirm_dialog(dialog, frame, area);
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

/// Render the email list as a table.
fn render_email_list(app: &App, frame: &mut Frame, area: Rect) {
    let border_style = pane_border_style(app.focus, Focus::List);
    let block = Block::default()
        .title(format!(" {} ", app.active_mailbox.label()))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .style(Style::default().bg(theme::BASE));

    if app.emails.is_empty() {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let empty = Paragraph::new(format!(
            "\n  No emails in {}\n\n  Press f to fetch new emails",
            app.active_mailbox.label()
        ))
        .style(Style::default().fg(theme::SUBTEXT0));
        frame.render_widget(empty, inner);
        return;
    }

    let inner = block.inner(area);
    // Calculate column widths from available space
    let available_width = inner.width as usize;
    let date_width = 10; // YYYY-MM-DD
    let status_width = 8;
    let spacing = 6; // gaps between columns
    let contact_width =
        20.min(available_width.saturating_sub(date_width + status_width + spacing));
    let subject_width =
        available_width.saturating_sub(date_width + contact_width + status_width + spacing);

    let header = Row::new(vec![
        Cell::from("DATE").style(Style::default().fg(theme::SUBTEXT0)),
        Cell::from("CONTACT").style(Style::default().fg(theme::SUBTEXT0)),
        Cell::from("SUBJECT").style(Style::default().fg(theme::SUBTEXT0)),
        Cell::from("STATUS").style(Style::default().fg(theme::SUBTEXT0)),
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
            let status_style = status_color(&email.status);

            let row_style = if is_selected {
                Style::default().bg(theme::SURFACE0).fg(theme::GREEN)
            } else {
                Style::default().fg(theme::TEXT)
            };

            Row::new(vec![
                Cell::from(email.date_display.clone()),
                Cell::from(contact),
                Cell::from(subject),
                Cell::from(email.status.clone()).style(if is_selected {
                    row_style
                } else {
                    status_style
                }),
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
            Constraint::Length(status_width as u16),
        ],
    )
    .header(header)
    .column_spacing(1)
    .block(block)
    .row_highlight_style(
        Style::default()
            .bg(theme::SURFACE0)
            .fg(theme::GREEN)
            .add_modifier(Modifier::BOLD),
    );

    let mut state = TableState::default();
    state.select(Some(app.list_index));
    frame.render_stateful_widget(table, area, &mut state);
}

/// Render the preview panel showing headers and body of the selected email.
fn render_preview(app: &App, frame: &mut Frame, area: Rect) {
    let border_style = pane_border_style(app.focus, Focus::Preview);
    let block = Block::default()
        .title(" Preview ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .style(Style::default().bg(theme::BASE));

    let selected = app.emails.get(app.list_index);

    if selected.is_none() {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let empty = Paragraph::new("\n  No email selected")
            .style(Style::default().fg(theme::SUBTEXT0));
        frame.render_widget(empty, inner);
        return;
    }

    let email = selected.unwrap();

    let mut lines: Vec<Line> = Vec::new();

    // Header fields
    lines.push(Line::from(vec![
        Span::styled(
            "From: ",
            Style::default()
                .fg(theme::GREEN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&email.from, Style::default().fg(theme::GREEN)),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            "To: ",
            Style::default()
                .fg(theme::BLUE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&email.to, Style::default().fg(theme::BLUE)),
    ]));
    if let Some(cc) = &email.cc {
        if !cc.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    "Cc: ",
                    Style::default()
                        .fg(theme::BLUE)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(cc.as_str(), Style::default().fg(theme::BLUE)),
            ]));
        }
    }
    lines.push(Line::from(vec![
        Span::styled(
            "Subject: ",
            Style::default()
                .fg(theme::YELLOW)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&email.subject, Style::default().fg(theme::YELLOW)),
    ]));
    if !email.date_display.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(
                "Date: ",
                Style::default()
                    .fg(theme::MAUVE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(&email.date_display, Style::default().fg(theme::MAUVE)),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled(
            "Status: ",
            Style::default()
                .fg(theme::TEAL)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&email.status, Style::default().fg(theme::TEAL)),
    ]));

    // Separator
    let inner_width = block.inner(area).width as usize;
    lines.push(Line::from(Span::styled(
        "\u{2500}".repeat(inner_width),
        Style::default().fg(theme::OVERLAY0),
    )));
    lines.push(Line::from(""));

    // Body (replace {{SIGNATURE}} placeholder)
    let body = email.body.replace("{{SIGNATURE}}", "[signature]");
    for line in body.lines() {
        lines.push(Line::from(Span::styled(
            line.to_string(),
            Style::default().fg(theme::TEXT),
        )));
    }

    let preview = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.preview_scroll, 0));

    frame.render_widget(preview, area);
}

/// Render the status bar at the bottom.
fn render_status_bar(app: &App, frame: &mut Frame, area: Rect) {
    let content = if let Some(msg) = &app.status_message {
        // Show status feedback message
        Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(msg.as_str(), Style::default().fg(theme::GREEN)),
        ])
    } else {
        // Show context-sensitive keybinding hints
        match app.focus {
            Focus::Sidebar => Line::from(vec![
                hint_span(" j/k"),
                desc_span("nav "),
                hint_span("Enter"),
                desc_span("select "),
                hint_span("q"),
                desc_span("quit"),
            ]),
            Focus::List => Line::from(vec![
                hint_span(" Enter"),
                desc_span("edit "),
                hint_span("r"),
                desc_span("reply "),
                hint_span("a"),
                desc_span("archive "),
                hint_span("A"),
                desc_span("approve "),
                hint_span("x"),
                desc_span("send "),
                hint_span("y"),
                desc_span("copy "),
                hint_span("n"),
                desc_span("new "),
                hint_span("f"),
                desc_span("fetch "),
                hint_span("F"),
                desc_span("sync"),
            ]),
            Focus::Preview => Line::from(vec![
                hint_span(" j/k"),
                desc_span("scroll "),
                hint_span("d/u"),
                desc_span("page "),
                hint_span("h"),
                desc_span("back "),
                hint_span("q"),
                desc_span("quit"),
            ]),
        }
    };

    let status = Paragraph::new(content)
        .style(Style::default().fg(theme::SUBTEXT0).bg(theme::SURFACE0));
    frame.render_widget(status, area);
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

/// Return a style for the email status field.
fn status_color(status: &str) -> Style {
    match status {
        "draft" => Style::default().fg(theme::YELLOW),
        "approved" => Style::default().fg(theme::GREEN),
        "sent" => Style::default().fg(theme::BLUE),
        "inbox" => Style::default().fg(theme::TEXT),
        "archived" => Style::default().fg(theme::OVERLAY0),
        _ => Style::default().fg(theme::SUBTEXT0),
    }
}

/// Return border style based on whether this pane is focused.
fn pane_border_style(current_focus: Focus, pane: Focus) -> Style {
    if current_focus == pane {
        Style::default().fg(theme::BLUE)
    } else {
        Style::default().fg(theme::OVERLAY0)
    }
}
