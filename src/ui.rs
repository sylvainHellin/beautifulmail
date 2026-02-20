use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, Focus};
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
        let border_style = pane_border_style(app.focus, Focus::Sidebar);
        let sidebar = Block::default()
            .title(" Mailboxes ")
            .borders(Borders::ALL)
            .border_style(border_style)
            .style(Style::default().bg(theme::BASE));
        frame.render_widget(sidebar, panels[panel_idx]);
        panel_idx += 1;
    }

    // Email list panel
    let border_style = pane_border_style(app.focus, Focus::List);
    let list = Block::default()
        .title(" Emails ")
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(theme::BASE));
    frame.render_widget(list, panels[panel_idx]);
    panel_idx += 1;

    if show_preview && panel_idx < panels.len() {
        let border_style = pane_border_style(app.focus, Focus::Preview);
        let preview = Block::default()
            .title(" Preview ")
            .borders(Borders::ALL)
            .border_style(border_style)
            .style(Style::default().bg(theme::BASE));
        frame.render_widget(preview, panels[panel_idx]);
    }

    // Status bar
    let status = Paragraph::new(" [q]uit  [s]idebar  [Tab]cycle  [/]search  [?]help")
        .style(Style::default().fg(theme::SUBTEXT0).bg(theme::SURFACE0));
    frame.render_widget(status, status_area);
}

/// Return border style based on whether this pane is focused.
fn pane_border_style(current_focus: Focus, pane: Focus) -> Style {
    if current_focus == pane {
        Style::default().fg(theme::BLUE)
    } else {
        Style::default().fg(theme::OVERLAY0)
    }
}
