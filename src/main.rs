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

use app::{Action, App, Mailbox};

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

        // Process pending action (side-effects outside the pure update)
        if let Some(action) = app.pending_action.take() {
            handle_action(&mut app, terminal, action)?;
        }
    }

    Ok(())
}

fn handle_action(
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    action: Action,
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
                        app.invalidate_cache(Mailbox::Drafts);
                    }
                    Err(e) => app.set_status(format!("Reply failed: {e}")),
                }
                app.reload_current_mailbox();
            }
        }

        Action::Send => {
            if let Some(path) = app.selected_email_path() {
                match cli::send(&path) {
                    Ok(msg) => {
                        app.set_status(if msg.is_empty() {
                            "Email sent".to_string()
                        } else {
                            msg
                        });
                        app.invalidate_all_caches();
                    }
                    Err(e) => app.set_status(format!("Send failed: {e}")),
                }
                app.reload_current_mailbox();
            }
        }

        Action::SendApproved => {
            if let Some(dir) = &app.mailbox_dirs[app.active_mailbox.index()] {
                let dir = dir.clone();
                match cli::send_approved(&dir) {
                    Ok(msg) => {
                        app.set_status(if msg.is_empty() {
                            "Approved emails sent".to_string()
                        } else {
                            msg
                        });
                        app.invalidate_all_caches();
                    }
                    Err(e) => app.set_status(format!("Send-approved failed: {e}")),
                }
                app.reload_current_mailbox();
            }
        }

        Action::NewDraft => {
            let name = chrono::Local::now().format("draft-%Y%m%d-%H%M%S").to_string();
            match cli::new_draft(&name) {
                Ok(msg) => {
                    // Try to open the new draft in the editor
                    if let Some(drafts_dir) = &app.mailbox_dirs[Mailbox::Drafts.index()] {
                        let draft_path = drafts_dir.join(format!("{name}.md"));
                        if draft_path.exists() {
                            suspend_terminal(terminal)?;
                            let _ = cli::edit_file(&draft_path);
                            resume_terminal(terminal)?;
                        }
                    }
                    app.set_status(msg);
                    app.invalidate_cache(Mailbox::Drafts);
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
                match cli::archive(&path) {
                    Ok(msg) => {
                        app.set_status(if msg.is_empty() {
                            "Email archived".to_string()
                        } else {
                            msg
                        });
                        app.invalidate_cache(Mailbox::Archive);
                        app.reload_current_mailbox();
                    }
                    Err(e) => app.set_status(format!("Archive failed: {e}")),
                }
            }
        }

        Action::Delete => {
            if let Some(path) = app.selected_email_path() {
                match cli::delete(&path) {
                    Ok(msg) => {
                        app.set_status(if msg.is_empty() {
                            "Email deleted".to_string()
                        } else {
                            msg
                        });
                        app.reload_current_mailbox();
                    }
                    Err(e) => app.set_status(format!("Delete failed: {e}")),
                }
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
            app.set_status("Fetching...".to_string());
            terminal.draw(|frame| ui::view(app, frame))?;

            match cli::fetch() {
                Ok(msg) => {
                    app.set_status(if msg.is_empty() {
                        "Fetch complete".to_string()
                    } else {
                        msg
                    });
                    app.invalidate_all_caches();
                    app.reload_current_mailbox();
                }
                Err(e) => app.set_status(format!("Fetch failed: {e}")),
            }
        }

        Action::Sync => {
            app.set_status("Syncing...".to_string());
            // Force a draw so the user sees the "Syncing..." message
            terminal.draw(|frame| ui::view(app, frame))?;

            match cli::sync() {
                Ok(msg) => {
                    app.set_status(if msg.is_empty() {
                        "Sync complete".to_string()
                    } else {
                        msg
                    });
                    app.invalidate_all_caches();
                    app.reload_current_mailbox();
                }
                Err(e) => app.set_status(format!("Sync failed: {e}")),
            }
        }

        Action::Reconcile => {
            app.set_status("Reconciling...".to_string());
            terminal.draw(|frame| ui::view(app, frame))?;

            match cli::sync_reconcile() {
                Ok(msg) => {
                    app.set_status(if msg.is_empty() {
                        "Reconcile complete".to_string()
                    } else {
                        msg
                    });
                    app.invalidate_all_caches();
                    app.reload_current_mailbox();
                }
                Err(e) => app.set_status(format!("Reconcile failed: {e}")),
            }
        }
    }

    Ok(())
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
