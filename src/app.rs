use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};

use crate::email::{self, EmailEntry};

/// Which pane currently has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    List,
    Preview,
    Search,
}

/// Messages that drive state transitions (TEA pattern).
#[derive(Debug)]
pub enum Message {
    Key(KeyEvent),
    Resize(u16, u16),
    Quit,
}

/// A mailbox the user can navigate to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mailbox {
    Inbox,
    Drafts,
    Sent,
    Archive,
}

impl Mailbox {
    pub const ALL: [Mailbox; 4] = [
        Mailbox::Inbox,
        Mailbox::Drafts,
        Mailbox::Sent,
        Mailbox::Archive,
    ];

    pub fn icon(self) -> &'static str {
        match self {
            Mailbox::Inbox => "󰇮",
            Mailbox::Drafts => "󰏫",
            Mailbox::Sent => "󰑫",
            Mailbox::Archive => "󰀼",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mailbox::Inbox => "Inbox",
            Mailbox::Drafts => "Drafts",
            Mailbox::Sent => "Sent",
            Mailbox::Archive => "Archive",
        }
    }

    /// Index into Mailbox::ALL.
    pub fn index(self) -> usize {
        match self {
            Mailbox::Inbox => 0,
            Mailbox::Drafts => 1,
            Mailbox::Sent => 2,
            Mailbox::Archive => 3,
        }
    }
}

/// Side-effects that the main loop must execute (keeps update pure).
#[derive(Debug)]
pub enum Action {
    /// Open the currently selected email in $EDITOR.
    EditCurrent,
    /// Run `email reply [--all]` on the selected email (interactive).
    Reply(bool),
    /// Run `email send` on the selected email (interactive).
    Send,
    /// Run `email send-approved` on the drafts directory (interactive).
    SendApproved,
    /// Create a new draft, then open in $EDITOR (interactive).
    NewDraft,
    /// Run `email mark-approved` on the selected email (silent).
    Approve,
    /// Archive the selected email (move to archive dir).
    Archive,
    /// Delete the selected email file.
    Delete,
    /// Copy the selected email's file path to clipboard.
    CopyPath,
    /// Run `email fetch` to pull new mail (silent).
    Fetch,
    /// Run `email sync` to full re-sync (silent).
    Sync,
}

/// Which destructive action a confirmation dialog is guarding.
#[derive(Debug, Clone)]
pub enum ConfirmAction {
    Archive,
    Delete,
    Send,
    SendApproved,
}

/// Data for rendering the confirmation dialog overlay.
#[derive(Debug, Clone)]
pub struct ConfirmDialog {
    pub title: String,
    pub detail: String,
    pub action: ConfirmAction,
}

/// Top-level application state.
pub struct App {
    pub focus: Focus,
    pub running: bool,
    pub terminal_width: u16,
    pub terminal_height: u16,

    /// Which mailbox is highlighted in the sidebar.
    pub sidebar_index: usize,
    /// Which mailbox is currently selected (determines email list content).
    pub active_mailbox: Mailbox,
    /// Email count per mailbox, indexed same as Mailbox::ALL.
    pub mailbox_counts: [usize; 4],
    /// Resolved directory paths per mailbox, indexed same as Mailbox::ALL.
    pub mailbox_dirs: [Option<PathBuf>; 4],

    /// Loaded email entries for the active mailbox.
    pub emails: Vec<EmailEntry>,
    /// Selected email index in the list.
    pub list_index: usize,
    /// Whether the previous keypress was `g` (for `gg` to go to top).
    pub g_pending: bool,
    /// Vertical scroll offset for the preview panel.
    pub preview_scroll: u16,
    /// Cached emails per mailbox (lazy-loaded).
    email_cache: [Option<Vec<EmailEntry>>; 4],

    /// An action the main loop should execute after this update cycle.
    pub pending_action: Option<Action>,
    /// When set, a confirmation dialog is shown and intercepts all keys.
    pub confirm_dialog: Option<ConfirmDialog>,
    /// Feedback message shown in the status bar (auto-clears after a few ticks).
    pub status_message: Option<String>,
    /// Countdown ticks until status_message is cleared (~250ms per tick).
    pub status_ticks: u8,
    /// Current search query text (empty = no filter active).
    pub search_query: String,
    /// Whether the current search also matches email body content (`\`).
    pub search_includes_body: bool,
    /// Whether the help overlay is displayed.
    pub show_help: bool,
}

impl App {
    pub fn new() -> Self {
        let dirs = resolve_mailbox_dirs();
        let counts = count_emails(&dirs);

        // Eagerly load the starting mailbox (inbox)
        let emails = dirs[0]
            .as_ref()
            .map(|d| email::load_emails(d))
            .unwrap_or_default();

        let mut cache: [Option<Vec<EmailEntry>>; 4] = [None, None, None, None];
        cache[0] = Some(emails.clone());

        Self {
            focus: Focus::List,
            running: true,
            terminal_width: 0,
            terminal_height: 0,
            sidebar_index: 0,
            active_mailbox: Mailbox::Inbox,
            mailbox_counts: counts,
            mailbox_dirs: dirs,
            emails,
            list_index: 0,
            g_pending: false,
            preview_scroll: 0,
            email_cache: cache,
            pending_action: None,
            confirm_dialog: None,
            status_message: None,
            status_ticks: 0,
            search_query: String::new(),
            search_includes_body: false,
            show_help: false,
        }
    }

    /// Process a message and optionally return a follow-up message.
    pub fn update(&mut self, msg: Message) -> Option<Message> {
        match msg {
            Message::Key(key) => self.handle_key(key),
            Message::Resize(w, h) => {
                self.terminal_width = w;
                self.terminal_height = h;
                None
            }
            Message::Quit => {
                self.running = false;
                None
            }
        }
    }

    /// Set a status bar message that auto-clears after ~3 seconds.
    pub fn set_status(&mut self, msg: String) {
        self.status_message = Some(msg);
        self.status_ticks = 12; // ~3s at 250ms poll interval
    }

    /// Tick down the status message counter. Called when no event is received.
    pub fn tick_status(&mut self) {
        if self.status_ticks > 0 {
            self.status_ticks -= 1;
            if self.status_ticks == 0 {
                self.status_message = None;
            }
        }
    }

    /// Get the currently selected email (if any).
    pub fn selected_email(&self) -> Option<&EmailEntry> {
        self.emails.get(self.list_index)
    }

    /// Get the file path of the currently selected email.
    pub fn selected_email_path(&self) -> Option<PathBuf> {
        self.selected_email().map(|e| e.path.clone())
    }

    /// Invalidate cache for a mailbox so it reloads on next access.
    pub fn invalidate_cache(&mut self, mailbox: Mailbox) {
        self.email_cache[mailbox.index()] = None;
    }

    /// Invalidate all caches.
    pub fn invalidate_all_caches(&mut self) {
        self.email_cache = [None, None, None, None];
    }

    /// Reload the currently active mailbox from disk.
    pub fn reload_current_mailbox(&mut self) {
        self.invalidate_cache(self.active_mailbox);
        self.switch_mailbox(self.active_mailbox);
        // Clamp list_index in case emails were removed
        if !self.emails.is_empty() {
            self.list_index = self.list_index.min(self.emails.len() - 1);
        } else {
            self.list_index = 0;
        }
        // Also refresh all mailbox counts
        self.mailbox_counts = count_emails(&self.mailbox_dirs);
    }

    /// Load (or use cached) emails for a mailbox and set as active.
    fn switch_mailbox(&mut self, mailbox: Mailbox) {
        self.active_mailbox = mailbox;
        self.search_query.clear();
        self.search_includes_body = false;
        let idx = mailbox.index();

        if let Some(cached) = &self.email_cache[idx] {
            self.emails = cached.clone();
        } else {
            let loaded = self.mailbox_dirs[idx]
                .as_ref()
                .map(|d| email::load_emails(d))
                .unwrap_or_default();
            self.email_cache[idx] = Some(loaded.clone());
            self.emails = loaded;
        }

        // Update count to match actual loaded data
        self.mailbox_counts[idx] = self.emails.len();
        self.list_index = 0;
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<Message> {
        // If a confirmation dialog is open, handle it exclusively
        if self.confirm_dialog.is_some() {
            return self.handle_confirm_key(key);
        }

        // If help overlay is showing, handle it exclusively
        if self.show_help {
            return self.handle_help_key(key);
        }

        // If search bar is active, handle search input
        if self.focus == Focus::Search {
            return self.handle_search_key(key);
        }

        // Global keys (work in any pane)
        match key.code {
            KeyCode::Char('q') => return Some(Message::Quit),
            KeyCode::Char('?') => {
                self.g_pending = false;
                self.show_help = true;
                return None;
            }
            KeyCode::Char('/') => {
                self.g_pending = false;
                self.focus = Focus::Search;
                self.search_query.clear();
                self.search_includes_body = false;
                self.reload_from_cache();
                return None;
            }
            KeyCode::Char('\\') => {
                self.g_pending = false;
                self.focus = Focus::Search;
                self.search_query.clear();
                self.search_includes_body = true;
                self.reload_from_cache();
                return None;
            }
            KeyCode::Char('1') => {
                self.g_pending = false;
                self.sidebar_index = 0;
                self.switch_mailbox(Mailbox::Inbox);
                self.focus = Focus::List;
                return None;
            }
            KeyCode::Char('2') => {
                self.g_pending = false;
                self.sidebar_index = 1;
                self.switch_mailbox(Mailbox::Drafts);
                self.focus = Focus::List;
                return None;
            }
            KeyCode::Char('3') => {
                self.g_pending = false;
                self.sidebar_index = 2;
                self.switch_mailbox(Mailbox::Sent);
                self.focus = Focus::List;
                return None;
            }
            KeyCode::Char('4') => {
                self.g_pending = false;
                self.sidebar_index = 3;
                self.switch_mailbox(Mailbox::Archive);
                self.focus = Focus::List;
                return None;
            }
            KeyCode::Char('s') => {
                self.g_pending = false;
                self.focus = Focus::Sidebar;
                return None;
            }
            KeyCode::Tab => {
                self.g_pending = false;
                self.focus = match self.focus {
                    Focus::Sidebar => Focus::List,
                    Focus::List => Focus::Preview,
                    Focus::Preview => Focus::Sidebar,
                    Focus::Search => Focus::List,
                };
                return None;
            }
            KeyCode::BackTab => {
                self.g_pending = false;
                self.focus = match self.focus {
                    Focus::Sidebar => Focus::Preview,
                    Focus::List => Focus::Sidebar,
                    Focus::Preview => Focus::List,
                    Focus::Search => Focus::List,
                };
                return None;
            }
            _ => {}
        }

        // Pane-specific keys
        match self.focus {
            Focus::Sidebar => self.handle_sidebar_key(key),
            Focus::List => self.handle_list_key(key),
            Focus::Preview => self.handle_preview_key(key),
            Focus::Search => unreachable!(),
        }
    }

    fn handle_confirm_key(&mut self, key: KeyEvent) -> Option<Message> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                if let Some(dialog) = self.confirm_dialog.take() {
                    self.pending_action = Some(match dialog.action {
                        ConfirmAction::Archive => Action::Archive,
                        ConfirmAction::Delete => Action::Delete,
                        ConfirmAction::Send => Action::Send,
                        ConfirmAction::SendApproved => Action::SendApproved,
                    });
                }
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.confirm_dialog = None;
            }
            _ => {}
        }
        None
    }

    fn handle_sidebar_key(&mut self, key: KeyEvent) -> Option<Message> {
        self.g_pending = false;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if self.sidebar_index < Mailbox::ALL.len() - 1 {
                    self.sidebar_index += 1;
                }
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.sidebar_index = self.sidebar_index.saturating_sub(1);
                None
            }
            KeyCode::Enter | KeyCode::Char('l') => {
                let mailbox = Mailbox::ALL[self.sidebar_index];
                self.switch_mailbox(mailbox);
                self.focus = Focus::List;
                None
            }
            KeyCode::Esc | KeyCode::Char('h') => {
                self.focus = Focus::List;
                None
            }
            _ => None,
        }
    }

    fn handle_list_key(&mut self, key: KeyEvent) -> Option<Message> {
        if self.emails.is_empty() {
            self.g_pending = false;
            // Allow fetch/sync/new even when list is empty
            if key.code == KeyCode::Char('f') {
                self.pending_action = Some(Action::Fetch);
            } else if key.code == KeyCode::Char('F') {
                self.pending_action = Some(Action::Sync);
            } else if key.code == KeyCode::Char('n') {
                self.pending_action = Some(Action::NewDraft);
            }
            return None;
        }

        let old_index = self.list_index;

        match key.code {
            // -- Navigation --
            KeyCode::Char('g') => {
                if self.g_pending {
                    self.list_index = 0;
                    self.g_pending = false;
                } else {
                    self.g_pending = true;
                }
            }
            KeyCode::Char('G') => {
                self.g_pending = false;
                self.list_index = self.emails.len().saturating_sub(1);
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.g_pending = false;
                if self.list_index < self.emails.len() - 1 {
                    self.list_index += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.g_pending = false;
                self.list_index = self.list_index.saturating_sub(1);
            }
            KeyCode::Char('h') => {
                self.g_pending = false;
                self.focus = Focus::Sidebar;
            }
            KeyCode::Char('l') => {
                self.g_pending = false;
                self.focus = Focus::Preview;
            }

            // -- Actions --
            KeyCode::Enter | KeyCode::Char('e') => {
                self.g_pending = false;
                self.pending_action = Some(Action::EditCurrent);
            }
            KeyCode::Char('r') => {
                self.g_pending = false;
                self.pending_action = Some(Action::Reply(false));
            }
            KeyCode::Char('R') => {
                self.g_pending = false;
                self.pending_action = Some(Action::Reply(true));
            }
            KeyCode::Char('a') => {
                self.g_pending = false;
                if let Some(email) = self.selected_email() {
                    self.confirm_dialog = Some(ConfirmDialog {
                        title: "Archive this email?".to_string(),
                        detail: format!("{} - {}", email.from, email.subject),
                        action: ConfirmAction::Archive,
                    });
                }
            }
            KeyCode::Char('d') => {
                self.g_pending = false;
                if let Some(email) = self.selected_email() {
                    self.confirm_dialog = Some(ConfirmDialog {
                        title: "Delete this email?".to_string(),
                        detail: format!("{} - {}", email.from, email.subject),
                        action: ConfirmAction::Delete,
                    });
                }
            }
            KeyCode::Char('A') => {
                self.g_pending = false;
                self.pending_action = Some(Action::Approve);
            }
            KeyCode::Char('x') => {
                self.g_pending = false;
                if let Some(email) = self.selected_email() {
                    self.confirm_dialog = Some(ConfirmDialog {
                        title: "Send this email?".to_string(),
                        detail: format!("To: {} - {}", email.to, email.subject),
                        action: ConfirmAction::Send,
                    });
                }
            }
            KeyCode::Char('X') => {
                self.g_pending = false;
                self.confirm_dialog = Some(ConfirmDialog {
                    title: "Send all approved emails?".to_string(),
                    detail: format!("In {}", self.active_mailbox.label()),
                    action: ConfirmAction::SendApproved,
                });
            }
            KeyCode::Char('y') => {
                self.g_pending = false;
                self.pending_action = Some(Action::CopyPath);
            }
            KeyCode::Char('n') => {
                self.g_pending = false;
                self.pending_action = Some(Action::NewDraft);
            }
            KeyCode::Char('f') => {
                self.g_pending = false;
                self.pending_action = Some(Action::Fetch);
            }
            KeyCode::Char('F') => {
                self.g_pending = false;
                self.pending_action = Some(Action::Sync);
            }

            _ => {
                self.g_pending = false;
            }
        }

        // Reset preview scroll when selection changes
        if self.list_index != old_index {
            self.preview_scroll = 0;
        }

        None
    }

    fn handle_preview_key(&mut self, key: KeyEvent) -> Option<Message> {
        self.g_pending = false;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.preview_scroll = self.preview_scroll.saturating_add(1);
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.preview_scroll = self.preview_scroll.saturating_sub(1);
                None
            }
            KeyCode::Char('d') => {
                // Half-page down (approximate with 10 lines)
                self.preview_scroll = self.preview_scroll.saturating_add(10);
                None
            }
            KeyCode::Char('u') => {
                // Half-page up
                self.preview_scroll = self.preview_scroll.saturating_sub(10);
                None
            }
            KeyCode::Esc | KeyCode::Char('h') => {
                self.focus = Focus::List;
                None
            }
            _ => None,
        }
    }

    fn handle_help_key(&mut self, key: KeyEvent) -> Option<Message> {
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc => {
                self.show_help = false;
            }
            _ => {}
        }
        None
    }

    fn handle_search_key(&mut self, key: KeyEvent) -> Option<Message> {
        match key.code {
            KeyCode::Enter => {
                self.focus = Focus::List;
            }
            KeyCode::Esc => {
                self.search_query.clear();
                self.search_includes_body = false;
                self.reload_from_cache();
                self.focus = Focus::List;
            }
            KeyCode::Char(c) => {
                self.search_query.push(c);
                self.apply_search_filter();
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                self.apply_search_filter();
            }
            _ => {}
        }
        None
    }

    /// Re-filter emails from cache based on the current search query.
    fn apply_search_filter(&mut self) {
        let idx = self.active_mailbox.index();
        let all_emails = self.email_cache[idx].as_ref().cloned().unwrap_or_default();

        if self.search_query.is_empty() {
            self.emails = all_emails;
        } else {
            let query = self.search_query.to_lowercase();
            let mailbox = self.active_mailbox;
            let includes_body = self.search_includes_body;
            self.emails = all_emails
                .into_iter()
                .filter(|e| {
                    e.subject.to_lowercase().contains(&query)
                        || e.display_contact(mailbox).to_lowercase().contains(&query)
                        || e.date_display.to_lowercase().contains(&query)
                        || e.from.to_lowercase().contains(&query)
                        || e.to.to_lowercase().contains(&query)
                        || (includes_body && e.body.to_lowercase().contains(&query))
                })
                .collect();
        }

        self.list_index = 0;
        self.preview_scroll = 0;
    }

    /// Reload emails from cache without invalidating (restores full unfiltered list).
    fn reload_from_cache(&mut self) {
        let idx = self.active_mailbox.index();
        if let Some(cached) = &self.email_cache[idx] {
            self.emails = cached.clone();
        }
        self.list_index = 0;
        self.preview_scroll = 0;
    }
}

/// Load .env and resolve mailbox directory paths.
fn resolve_mailbox_dirs() -> [Option<PathBuf>; 4] {
    // Load .env from the email notes directory and standard locations
    dotenvy::dotenv().ok();

    // Also try loading from the email project directory
    let email_project = PathBuf::from("/Users/sylvainhellin/code/personal/email");
    if email_project.join(".env").exists() {
        dotenvy::from_path(email_project.join(".env")).ok();
    }

    let env_keys = ["INBOX_DIR", "DRAFTS_DIR", "SENT_DIR", "ARCHIVE_DIR"];
    let mut dirs: [Option<PathBuf>; 4] = [None, None, None, None];

    for (i, key) in env_keys.iter().enumerate() {
        dirs[i] = std::env::var(key).ok().map(|s| {
            let s = s.trim_matches('"').trim_matches('\'');
            PathBuf::from(shellexpand::tilde(s).into_owned())
        });
    }

    dirs
}

/// Count .md files in each mailbox directory.
fn count_emails(dirs: &[Option<PathBuf>; 4]) -> [usize; 4] {
    let mut counts = [0usize; 4];
    for (i, dir) in dirs.iter().enumerate() {
        if let Some(path) = dir {
            if path.is_dir() {
                counts[i] = walkdir::WalkDir::new(path)
                    .max_depth(1)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        e.file_type().is_file()
                            && e.path().extension().is_some_and(|ext| ext == "md")
                    })
                    .count();
            }
        }
    }
    counts
}
