mod app;
mod cli;
mod email;
mod event;
mod theme;
mod ui;

use std::io::{self, stdout};
use std::panic;
use std::sync::mpsc;

use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::{Action, App, BgResult, MailboxKind};

enum WatchEvent {
    Changed,
    Error(String),
}

fn main() -> Result<()> {
    install_panic_hook();
    let mut terminal = init_terminal()?;
    let result = run(&mut terminal);
    restore_terminal()?;
    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let mut app = App::new();

    // Capture initial terminal size
    let size = terminal.size()?;
    app.terminal_width = size.width;
    app.terminal_height = size.height;

    // Spawn background mail watcher thread
    let (watch_tx, watch_rx) = mpsc::channel::<WatchEvent>();
    app.watcher_active = true;
    std::thread::spawn(move || {
        watcher_loop(watch_tx);
    });

    // Background task results channel
    let (bg_tx, bg_rx) = mpsc::channel::<BgResult>();

    while app.running {
        terminal.draw(|frame| ui::view(&app, frame))?;

        if let Some(msg) = event::poll_event()? {
            let mut current_msg = Some(msg);
            while let Some(m) = current_msg {
                current_msg = app.update(m);
            }
        } else {
            // No event this tick -- count down status message
            app.tick_status();
            // Advance spinner when background ops are running
            if app.bg_count > 0 {
                app.bg_spin_tick = app.bg_spin_tick.wrapping_add(1);
            }
        }

        // Check background watcher
        match watch_rx.try_recv() {
            Ok(WatchEvent::Changed) => {
                let mut current_msg = Some(app::Message::MailboxChanged);
                while let Some(m) = current_msg {
                    current_msg = app.update(m);
                }
            }
            Ok(WatchEvent::Error(e)) => {
                app.set_status(format!("Watch: {e}"));
                app.watcher_active = false;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                app.watcher_active = false;
            }
        }

        // Check background task results (drain all available)
        while let Ok(result) = bg_rx.try_recv() {
            handle_bg_result(&mut app, result);
        }

        // Auto-execute queued action when all mutations complete
        if app.bg_mutations == 0 {
            if let Some(action) = app.queued_action.take() {
                app.pending_action = Some(action);
            }
        }

        // Process pending action (side-effects outside the pure update)
        if let Some(action) = app.pending_action.take() {
            handle_action(&mut app, terminal, action, &bg_tx)?;
        }
    }

    Ok(())
}

fn handle_action(
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    action: Action,
    bg_tx: &mpsc::Sender<BgResult>,
) -> Result<()> {
    match action {
        Action::EditCurrent => {
            if let Some(path) = app.selected_email_path() {
                suspend_terminal(terminal)?;
                let result = cli::edit_file(&path);
                resume_terminal(terminal)?;
                match result {
                    Ok(()) => app.set_status("Returned from editor".to_string()),
                    Err(e) => app.set_status(format!("Edit failed: {e}")),
                }
                app.reload_current_mailbox();
            }
        }

        Action::Reply(reply_all) => {
            if let Some(path) = app.selected_email_path() {
                match cli::reply(&path, reply_all) {
                    Ok(draft_path) => {
                        suspend_terminal(terminal)?;
                        let edit_result = cli::edit_file(&draft_path);
                        resume_terminal(terminal)?;
                        match edit_result {
                            Ok(()) => app.set_status("Reply draft ready".to_string()),
                            Err(e) => app.set_status(format!("Editor failed: {e}")),
                        }
                        if let Some(idx) = app.find_mailbox_by_kind(MailboxKind::Drafts) {
                            app.invalidate_cache_idx(idx);
                        }
                    }
                    Err(e) => app.set_status(format!("Reply failed: {e}")),
                }
                app.reload_current_mailbox();
            }
        }

        Action::Send => {
            if let Some(path) = app.selected_email_path() {
                app.bg_count += 1;
                app.set_status("Sending...".to_string());
                let tx = bg_tx.clone();
                std::thread::spawn(move || {
                    let result = cli::send(&path);
                    let _ = tx.send(BgResult::Send {
                        result: result.map_err(|e| e.to_string()),
                    });
                });
            }
        }

        Action::SendApproved => {
            if let Some(dir) = app.active_dir().cloned() {
                app.bg_count += 1;
                app.set_status("Sending approved...".to_string());
                let tx = bg_tx.clone();
                std::thread::spawn(move || {
                    let result = cli::send_approved(&dir);
                    let _ = tx.send(BgResult::SendApproved {
                        result: result.map_err(|e| e.to_string()),
                    });
                });
            }
        }

        Action::NewDraft => {
            let name = chrono::Local::now().format("draft-%Y%m%d-%H%M%S").to_string();
            match cli::new_draft(&name) {
                Ok(msg) => {
                    // Try to open the new draft in the editor
                    let drafts_dir = app.find_mailbox_by_kind(MailboxKind::Drafts)
                        .map(|i| app.mailboxes[i].dir.clone());
                    if let Some(drafts_dir) = &drafts_dir {
                        let draft_path = drafts_dir.join(format!("{name}.md"));
                        if draft_path.exists() {
                            suspend_terminal(terminal)?;
                            let _ = cli::edit_file(&draft_path);
                            resume_terminal(terminal)?;
                        }
                    }
                    app.set_status(msg);
                    if let Some(idx) = app.find_mailbox_by_kind(MailboxKind::Drafts) {
                        app.invalidate_cache_idx(idx);
                    }
                    app.reload_current_mailbox();
                }
                Err(e) => app.set_status(format!("New draft failed: {e}")),
            }
        }

        Action::Approve => {
            if let Some(path) = app.selected_email_path() {
                match cli::approve(&path) {
                    Ok(msg) => {
                        app.set_status(msg);
                        app.reload_current_mailbox();
                    }
                    Err(e) => app.set_status(format!("Approve failed: {e}")),
                }
            }
        }

        Action::Archive => {
            if let Some(path) = app.selected_email_path() {
                // Optimistic UI: remove from list immediately
                app.remove_selected_from_list();
                app.bg_count += 1;
                app.bg_mutations += 1;
                app.set_status("Archiving...".to_string());
                // Force immediate redraw so the removal is visible
                terminal.draw(|frame| ui::view(app, frame))?;
                let tx = bg_tx.clone();
                std::thread::spawn(move || {
                    let result = cli::archive(&path);
                    let _ = tx.send(BgResult::Archive {
                        result: result.map_err(|e| e.to_string()),
                    });
                });
            }
        }

        Action::Delete => {
            if let Some(path) = app.selected_email_path() {
                // Optimistic UI: remove from list immediately
                app.remove_selected_from_list();
                app.bg_count += 1;
                app.bg_mutations += 1;
                app.set_status("Deleting...".to_string());
                // Force immediate redraw so the removal is visible
                terminal.draw(|frame| ui::view(app, frame))?;
                let tx = bg_tx.clone();
                std::thread::spawn(move || {
                    let result = cli::delete(&path);
                    let _ = tx.send(BgResult::Delete {
                        result: result.map_err(|e| e.to_string()),
                    });
                });
            }
        }

        Action::CopyPath => {
            if let Some(path) = app.selected_email_path() {
                match cli::copy_to_clipboard(&path.display().to_string()) {
                    Ok(()) => app.set_status("Path copied to clipboard".to_string()),
                    Err(e) => app.set_status(format!("Copy failed: {e}")),
                }
            }
        }

        Action::Fetch => {
            if app.bg_mutations > 0 {
                app.queued_action = Some(Action::Fetch);
                app.set_status(format!(
                    "Fetch queued ({} ops pending...)",
                    app.bg_mutations
                ));
                return Ok(());
            }
            app.bg_count += 1;
            app.set_status("Fetching...".to_string());
            let tx = bg_tx.clone();
            std::thread::spawn(move || {
                let result = cli::fetch();
                let _ = tx.send(BgResult::Fetch {
                    result: result.map_err(|e| e.to_string()),
                });
            });
        }

        Action::Sync => {
            if app.bg_mutations > 0 {
                app.queued_action = Some(Action::Sync);
                app.set_status(format!(
                    "Sync queued ({} ops pending...)",
                    app.bg_mutations
                ));
                return Ok(());
            }
            app.bg_count += 1;
            app.set_status("Syncing...".to_string());
            let tx = bg_tx.clone();
            std::thread::spawn(move || {
                let result = cli::sync();
                let _ = tx.send(BgResult::Sync {
                    result: result.map_err(|e| e.to_string()),
                });
            });
        }

        Action::Reconcile => {
            if app.bg_mutations > 0 {
                app.queued_action = Some(Action::Reconcile);
                app.set_status(format!(
                    "Reconcile queued ({} ops pending...)",
                    app.bg_mutations
                ));
                return Ok(());
            }
            app.bg_count += 1;
            app.set_status("Reconciling...".to_string());
            let tx = bg_tx.clone();
            std::thread::spawn(move || {
                let result = cli::sync_reconcile();
                let _ = tx.send(BgResult::Reconcile {
                    result: result.map_err(|e| e.to_string()),
                });
            });
        }
    }

    Ok(())
}

fn handle_bg_result(app: &mut App, result: BgResult) {
    app.bg_count = app.bg_count.saturating_sub(1);

    match result {
        BgResult::Archive { result } => {
            app.bg_mutations = app.bg_mutations.saturating_sub(1);
            match result {
                Ok(msg) => {
                    app.set_status(if msg.is_empty() { "Email archived".into() } else { msg });
                    if let Some(idx) = app.find_mailbox_by_kind(MailboxKind::Archive) {
                        app.invalidate_cache_idx(idx);
                    }
                }
                Err(e) => {
                    // CLI has already rolled back local changes.
                    // Reload from disk to restore the email in the list.
                    app.invalidate_all_caches();
                    app.reload_current_mailbox();
                    app.set_persistent_error(format!(
                        "Archive failed: {e}\nEmail restored to inbox. Sync to retry?"
                    ));
                }
            }
        }

        BgResult::Delete { result } => {
            app.bg_mutations = app.bg_mutations.saturating_sub(1);
            match result {
                Ok(msg) => {
                    app.set_status(if msg.is_empty() { "Email deleted".into() } else { msg });
                }
                Err(e) => {
                    // CLI has already restored local files.
                    // Reload from disk to restore the email in the list.
                    app.invalidate_all_caches();
                    app.reload_current_mailbox();
                    app.set_persistent_error(format!(
                        "Delete failed: {e}\nEmail restored. Sync to retry?"
                    ));
                }
            }
        }

        BgResult::Send { result } => {
            match result {
                Ok(msg) => {
                    app.set_status(if msg.is_empty() { "Email sent".into() } else { msg });
                    app.invalidate_all_caches();
                    app.reload_current_mailbox();
                }
                Err(e) => app.set_status(format!("Send failed: {e}")),
            }
        }

        BgResult::SendApproved { result } => {
            match result {
                Ok(msg) => {
                    app.set_status(if msg.is_empty() { "Approved emails sent".into() } else { msg });
                    app.invalidate_all_caches();
                    app.reload_current_mailbox();
                }
                Err(e) => app.set_status(format!("Send-approved failed: {e}")),
            }
        }

        BgResult::Fetch { result } => {
            match result {
                Ok(msg) => {
                    app.set_status(if msg.is_empty() { "Fetch complete".into() } else { msg });
                    app.invalidate_all_caches();
                    app.reload_current_mailbox();
                }
                Err(e) => app.set_status(format!("Fetch failed: {e}")),
            }
        }

        BgResult::Sync { result } => {
            match result {
                Ok(msg) => {
                    app.set_status(if msg.is_empty() { "Sync complete".into() } else { msg });
                    app.invalidate_all_caches();
                    app.reload_current_mailbox();
                }
                Err(e) => app.set_status(format!("Sync failed: {e}")),
            }
        }

        BgResult::Reconcile { result } => {
            match result {
                Ok(msg) => {
                    app.set_status(if msg.is_empty() { "Reconcile complete".into() } else { msg });
                    app.invalidate_all_caches();
                    app.reload_current_mailbox();
                }
                Err(e) => app.set_status(format!("Reconcile failed: {e}")),
            }
        }
    }
}

fn suspend_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn resume_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    terminal.hide_cursor()?;
    terminal.clear()?;
    Ok(())
}

fn init_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal() -> Result<()> {
    execute!(stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}

fn install_panic_hook() {
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let _ = execute!(stdout(), LeaveAlternateScreen);
        let _ = disable_raw_mode();
        original_hook(panic_info);
    }));
}

fn watcher_loop(tx: mpsc::Sender<WatchEvent>) {
    loop {
        let result = std::process::Command::new("email")
            .args(["watch", "--timeout", "300"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .status();

        match result {
            Ok(status) => match status.code() {
                Some(0) => {
                    if tx.send(WatchEvent::Changed).is_err() {
                        break; // receiver dropped, app is quitting
                    }
                }
                Some(2) => continue, // timeout, restart IDLE
                _ => {
                    let _ = tx.send(WatchEvent::Error("Watch connection lost".into()));
                    std::thread::sleep(std::time::Duration::from_secs(30));
                }
            },
            Err(_) => {
                // email binary not found or not executable -- stop retrying
                let _ = tx.send(WatchEvent::Error("email watch unavailable".into()));
                break;
            }
        }
    }
}
