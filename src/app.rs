use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use serde::Deserialize;

use crate::email::{self, EmailEntry};

/// Result from a background CLI operation.
#[derive(Debug)]
pub enum BgResult {
    Fetch { result: Result<String, String> },
    Sync { result: Result<String, String> },
    Reconcile { result: Result<String, String> },
    Send { result: Result<String, String> },
    SendApproved { result: Result<String, String> },
    Archive { result: Result<String, String> },
    Delete { result: Result<String, String> },
}

/// Which pane currently has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    List,
    Headers,
    Preview,
    Search,
}

/// Messages that drive state transitions (TEA pattern).
#[derive(Debug)]
pub enum Message {
    Key(KeyEvent),
    Resize(u16, u16),
    Quit,
    /// Background watcher detected new mail.
    MailboxChanged,
}

/// Behavioral kind of a mailbox (used for action differentiation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailboxKind {
    Inbox,
    Drafts,
    Sent,
    Archive,
    Extra,
}

/// A mailbox entry with its metadata and resolved path.
#[derive(Debug, Clone)]
pub struct MailboxInfo {
    pub label: String,
    pub icon: &'static str,
    pub dir: PathBuf,
    pub kind: MailboxKind,
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
    /// Run `email sync --reconcile` to sync and reconcile (silent).
    Reconcile,
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

/// Persistent error notification (requires user action to dismiss).
pub struct PersistentError {
    pub message: String,
}

/// Top-level application state.
pub struct App {
    pub focus: Focus,
    pub running: bool,
    pub terminal_width: u16,
    pub terminal_height: u16,

    /// Dynamic list of mailboxes loaded from config.
    pub mailboxes: Vec<MailboxInfo>,
    /// Which mailbox is highlighted in the sidebar.
    pub sidebar_index: usize,
    /// Which mailbox is currently selected (index into `mailboxes`).
    pub active_mailbox: usize,
    /// Email count per mailbox.
    pub mailbox_counts: Vec<usize>,

    /// Loaded email entries for the active mailbox.
    pub emails: Vec<EmailEntry>,
    /// Selected email index in the list.
    pub list_index: usize,
    /// Whether the previous keypress was `g` (for `gg` to go to top).
    pub g_pending: bool,
    /// Vertical scroll offset for the headers panel.
    pub headers_scroll: u16,
    /// Vertical scroll offset for the preview/body panel.
    pub preview_scroll: u16,
    /// Cached emails per mailbox (lazy-loaded).
    email_cache: Vec<Option<Vec<EmailEntry>>>,

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
    /// Whether the background mail watcher is active.
    pub watcher_active: bool,

    /// Total number of background operations in flight.
    pub bg_count: usize,
    /// Number of mutation operations in flight (archive/delete) -- blocks fetch/sync.
    pub bg_mutations: usize,
    /// Spinner tick counter (counts up while bg_count > 0).
    pub bg_spin_tick: usize,
    /// Queued action to execute after all mutations complete (fetch/sync/reconcile).
    pub queued_action: Option<Action>,
    /// Persistent error notification (requires user action to dismiss).
    pub persistent_error: Option<PersistentError>,
}

impl App {
    pub fn new() -> Self {
        let mailboxes = load_mailboxes_from_config();
        let n = mailboxes.len();
        let counts = count_all_emails(&mailboxes);

        // Eagerly load the starting mailbox (first one, typically inbox)
        let emails = if !mailboxes.is_empty() {
            email::load_emails(&mailboxes[0].dir)
        } else {
            Vec::new()
        };

        let mut cache: Vec<Option<Vec<EmailEntry>>> = vec![None; n];
        if !cache.is_empty() {
            cache[0] = Some(emails.clone());
        }

        Self {
            focus: Focus::List,
            running: true,
            terminal_width: 0,
            terminal_height: 0,
            mailboxes,
            sidebar_index: 0,
            active_mailbox: 0,
            mailbox_counts: counts,
            emails,
            list_index: 0,
            g_pending: false,
            headers_scroll: 0,
            preview_scroll: 0,
            email_cache: cache,
            pending_action: None,
            confirm_dialog: None,
            status_message: None,
            status_ticks: 0,
            search_query: String::new(),
            search_includes_body: false,
            show_help: false,
            watcher_active: false,
            bg_count: 0,
            bg_mutations: 0,
            bg_spin_tick: 0,
            queued_action: None,
            persistent_error: None,
        }
    }

    /// Get the MailboxKind of the active mailbox.
    pub fn active_kind(&self) -> MailboxKind {
        self.mailboxes.get(self.active_mailbox)
            .map(|m| m.kind)
            .unwrap_or(MailboxKind::Inbox)
    }

    /// Get the label of the active mailbox.
    pub fn active_label(&self) -> &str {
        self.mailboxes.get(self.active_mailbox)
            .map(|m| m.label.as_str())
            .unwrap_or("Mail")
    }

    /// Get the directory of the active mailbox.
    pub fn active_dir(&self) -> Option<&PathBuf> {
        self.mailboxes.get(self.active_mailbox).map(|m| &m.dir)
    }

    /// Find the index of the first mailbox with the given kind.
    pub fn find_mailbox_by_kind(&self, kind: MailboxKind) -> Option<usize> {
        self.mailboxes.iter().position(|m| m.kind == kind)
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
            Message::MailboxChanged => {
                self.pending_action = Some(Action::Fetch);
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
        if self.bg_count > 0 {
            // Don't clear status while background ops are running
            return;
        }
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

    /// Remove the currently selected email from the in-memory list (optimistic UI).
    /// Does NOT touch the filesystem. Returns the removed email's path.
    pub fn remove_selected_from_list(&mut self) -> Option<PathBuf> {
        if self.emails.is_empty() {
            return None;
        }
        let path = self.emails[self.list_index].path.clone();
        self.emails.remove(self.list_index);

        // Also remove from cache so reloads don't restore it
        if let Some(Some(cached)) = self.email_cache.get_mut(self.active_mailbox) {
            cached.retain(|e| e.path != path);
        }

        // Adjust selection
        if !self.emails.is_empty() {
            self.list_index = self.list_index.min(self.emails.len() - 1);
        } else {
            self.list_index = 0;
        }

        // Update count
        if let Some(count) = self.mailbox_counts.get_mut(self.active_mailbox) {
            *count = self.emails.len();
        }

        self.headers_scroll = 0;
        self.preview_scroll = 0;

        Some(path)
    }

    /// Set a persistent error that requires user action to dismiss.
    pub fn set_persistent_error(&mut self, msg: String) {
        self.persistent_error = Some(PersistentError { message: msg });
    }

    /// Invalidate cache for a mailbox index so it reloads on next access.
    pub fn invalidate_cache_idx(&mut self, idx: usize) {
        if let Some(slot) = self.email_cache.get_mut(idx) {
            *slot = None;
        }
    }

    /// Invalidate all caches.
    pub fn invalidate_all_caches(&mut self) {
        for slot in &mut self.email_cache {
            *slot = None;
        }
    }

    /// Reload the currently active mailbox from disk.
    pub fn reload_current_mailbox(&mut self) {
        self.invalidate_cache_idx(self.active_mailbox);
        self.switch_mailbox(self.active_mailbox);
        // Clamp list_index in case emails were removed
        if !self.emails.is_empty() {
            self.list_index = self.list_index.min(self.emails.len() - 1);
        } else {
            self.list_index = 0;
        }
        // Also refresh all mailbox counts
        self.mailbox_counts = count_all_emails(&self.mailboxes);
    }

    /// Load (or use cached) emails for a mailbox index and set as active.
    fn switch_mailbox(&mut self, idx: usize) {
        let changing = self.active_mailbox != idx;
        self.active_mailbox = idx;
        if changing {
            self.search_query.clear();
            self.search_includes_body = false;
        }

        if let Some(cached) = self.email_cache.get(idx).and_then(|c| c.as_ref()) {
            self.emails = cached.clone();
        } else if let Some(mb) = self.mailboxes.get(idx) {
            let loaded = email::load_emails(&mb.dir);
            if let Some(slot) = self.email_cache.get_mut(idx) {
                *slot = Some(loaded.clone());
            }
            self.emails = loaded;
        } else {
            self.emails = Vec::new();
        }

        // Update count to match actual loaded data
        if let Some(count) = self.mailbox_counts.get_mut(idx) {
            *count = self.emails.len();
        }
        if changing {
            self.list_index = 0;
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<Message> {
        // If a confirmation dialog is open, handle it exclusively
        if self.confirm_dialog.is_some() {
            return self.handle_confirm_key(key);
        }

        // If persistent error overlay is showing, handle it exclusively
        if self.persistent_error.is_some() {
            return self.handle_persistent_error_key(key);
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
            // Number keys 1-9 jump to mailbox by index
            KeyCode::Char(c @ '1'..='9') => {
                let idx = (c as usize) - ('1' as usize);
                if idx < self.mailboxes.len() {
                    self.g_pending = false;
                    self.sidebar_index = idx;
                    self.switch_mailbox(idx);
                    self.focus = Focus::List;
                    return None;
                }
            }
            KeyCode::Char('s') => {
                self.g_pending = false;
                self.focus = Focus::Sidebar;
                return None;
            }
            KeyCode::Tab | KeyCode::Char('l') => {
                self.g_pending = false;
                // In sidebar, also select the highlighted mailbox
                if self.focus == Focus::Sidebar {
                    self.switch_mailbox(self.sidebar_index);
                }
                self.focus = match self.focus {
                    Focus::Sidebar => Focus::List,
                    Focus::List => Focus::Preview,
                    Focus::Preview => Focus::Headers,
                    Focus::Headers => Focus::Sidebar,
                    Focus::Search => Focus::List,
                };
                return None;
            }
            KeyCode::BackTab | KeyCode::Char('h') => {
                self.g_pending = false;
                self.focus = match self.focus {
                    Focus::Sidebar => Focus::Headers,
                    Focus::Headers => Focus::Preview,
                    Focus::Preview => Focus::List,
                    Focus::List => Focus::Sidebar,
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
            Focus::Headers => self.handle_headers_key(key),
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
                if self.sidebar_index < self.mailboxes.len().saturating_sub(1) {
                    self.sidebar_index += 1;
                }
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.sidebar_index = self.sidebar_index.saturating_sub(1);
                None
            }
            KeyCode::Enter => {
                self.switch_mailbox(self.sidebar_index);
                self.focus = Focus::List;
                None
            }
            _ => None,
        }
    }

    fn handle_headers_key(&mut self, key: KeyEvent) -> Option<Message> {
        self.g_pending = false;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.headers_scroll = self.headers_scroll.saturating_add(1);
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.headers_scroll = self.headers_scroll.saturating_sub(1);
                None
            }
            _ => None,
        }
    }

    fn handle_list_key(&mut self, key: KeyEvent) -> Option<Message> {
        if self.emails.is_empty() {
            self.g_pending = false;
            // Allow fetch/sync/reconcile/new even when list is empty
            match key.code {
                KeyCode::Char('f') => self.pending_action = Some(Action::Fetch),
                KeyCode::Char('F') => self.pending_action = Some(Action::Sync),
                KeyCode::Char('S') => self.pending_action = Some(Action::Reconcile),
                KeyCode::Char('n') => self.pending_action = Some(Action::NewDraft),
                _ => {}
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
                    detail: format!("In {}", self.active_label()),
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
            KeyCode::Char('S') => {
                self.g_pending = false;
                self.pending_action = Some(Action::Reconcile);
            }

            _ => {
                self.g_pending = false;
            }
        }

        // Reset scroll when selection changes
        if self.list_index != old_index {
            self.headers_scroll = 0;
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
            KeyCode::Esc => {
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

    fn handle_persistent_error_key(&mut self, key: KeyEvent) -> Option<Message> {
        match key.code {
            KeyCode::Char('s') => {
                self.persistent_error = None;
                self.pending_action = Some(Action::Sync);
            }
            KeyCode::Char('d') | KeyCode::Esc => {
                self.persistent_error = None;
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
        let idx = self.active_mailbox;
        let all_emails = self.email_cache.get(idx)
            .and_then(|c| c.as_ref())
            .cloned()
            .unwrap_or_default();

        if self.search_query.is_empty() {
            self.emails = all_emails;
        } else {
            let query = self.search_query.to_lowercase();
            let kind = self.active_kind();
            let includes_body = self.search_includes_body;
            self.emails = all_emails
                .into_iter()
                .filter(|e| {
                    e.subject.to_lowercase().contains(&query)
                        || e.display_contact(kind).to_lowercase().contains(&query)
                        || e.date_display.to_lowercase().contains(&query)
                        || e.from.to_lowercase().contains(&query)
                        || e.to.to_lowercase().contains(&query)
                        || (includes_body && e.body.to_lowercase().contains(&query))
                })
                .collect();
        }

        self.list_index = 0;
        self.headers_scroll = 0;
        self.preview_scroll = 0;
    }

    /// Reload emails from cache without invalidating (restores full unfiltered list).
    fn reload_from_cache(&mut self) {
        let idx = self.active_mailbox;
        if let Some(Some(cached)) = self.email_cache.get(idx) {
            self.emails = cached.clone();
        }
        self.list_index = 0;
        self.headers_scroll = 0;
        self.preview_scroll = 0;
    }
}

// -- Config parsing --

#[derive(Debug, Deserialize)]
struct ConfigFile {
    directories: Option<ConfigDirectories>,
    mailboxes: Option<ConfigMailboxes>,
}

#[derive(Debug, Deserialize)]
struct ConfigDirectories {
    root: Option<String>,
    drafts: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ConfigMailboxes {
    inbox: Option<ConfigMailbox>,
    archive: Option<ConfigMailbox>,
    sent: Option<ConfigMailbox>,
    extra: Option<Vec<ConfigMailbox>>,
}

#[derive(Debug, Deserialize)]
struct ConfigMailbox {
    #[allow(dead_code)]
    server: Option<String>,
    local: Option<String>,
}

fn expand_path(s: &str) -> PathBuf {
    PathBuf::from(shellexpand::tilde(s).into_owned())
}

/// Load mailboxes from `~/.config/email/config.toml`.
fn load_mailboxes_from_config() -> Vec<MailboxInfo> {
    let config_path = expand_path("~/.config/email/config.toml");
    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return default_mailboxes(),
    };
    let config: ConfigFile = match toml::from_str(&content) {
        Ok(c) => c,
        Err(_) => return default_mailboxes(),
    };

    let root = config.directories.as_ref()
        .and_then(|d| d.root.as_ref())
        .map(|r| expand_path(r))
        .unwrap_or_else(|| expand_path("~/notes/email"));

    let drafts_local = config.directories.as_ref()
        .and_then(|d| d.drafts.as_ref())
        .cloned()
        .unwrap_or_else(|| "drafts".to_string());

    let mbs = match &config.mailboxes {
        Some(m) => m,
        None => return default_mailboxes(),
    };

    let mut result = Vec::new();

    // Inbox
    let inbox_local = mbs.inbox.as_ref()
        .and_then(|m| m.local.as_ref())
        .cloned()
        .unwrap_or_else(|| "inbox".to_string());
    result.push(MailboxInfo {
        label: "Inbox".to_string(),
        icon: "\u{f0172}",  // 󰇮
        dir: root.join(&inbox_local),
        kind: MailboxKind::Inbox,
    });

    // Drafts
    result.push(MailboxInfo {
        label: "Drafts".to_string(),
        icon: "\u{f03eb}",  // 󰏫
        dir: root.join(&drafts_local),
        kind: MailboxKind::Drafts,
    });

    // Sent
    let sent_local = mbs.sent.as_ref()
        .and_then(|m| m.local.as_ref())
        .cloned()
        .unwrap_or_else(|| "sent".to_string());
    result.push(MailboxInfo {
        label: "Sent".to_string(),
        icon: "\u{f046b}",  // 󰑫
        dir: root.join(&sent_local),
        kind: MailboxKind::Sent,
    });

    // Archive
    let archive_local = mbs.archive.as_ref()
        .and_then(|m| m.local.as_ref())
        .cloned()
        .unwrap_or_else(|| "archive".to_string());
    result.push(MailboxInfo {
        label: "Archive".to_string(),
        icon: "\u{f013c}",  // 󰀼
        dir: root.join(&archive_local),
        kind: MailboxKind::Archive,
    });

    // Extra mailboxes
    if let Some(extras) = &mbs.extra {
        for extra in extras {
            let server_name = extra.server.as_deref().unwrap_or("Extra");
            let local = extra.local.as_deref().unwrap_or("extra");
            result.push(MailboxInfo {
                label: server_name.to_string(),
                icon: "\u{f0247}",  // 󰉇
                dir: root.join(local),
                kind: MailboxKind::Extra,
            });
        }
    }

    result
}

/// Fallback mailboxes when config is missing.
fn default_mailboxes() -> Vec<MailboxInfo> {
    let root = expand_path("~/notes/email");
    vec![
        MailboxInfo { label: "Inbox".to_string(), icon: "\u{f0172}", dir: root.join("inbox"), kind: MailboxKind::Inbox },
        MailboxInfo { label: "Drafts".to_string(), icon: "\u{f03eb}", dir: root.join("drafts"), kind: MailboxKind::Drafts },
        MailboxInfo { label: "Sent".to_string(), icon: "\u{f046b}", dir: root.join("sent"), kind: MailboxKind::Sent },
        MailboxInfo { label: "Archive".to_string(), icon: "\u{f013c}", dir: root.join("archive"), kind: MailboxKind::Archive },
    ]
}

/// Count .md files in each mailbox directory.
fn count_all_emails(mailboxes: &[MailboxInfo]) -> Vec<usize> {
    mailboxes.iter().map(|mb| {
        if mb.dir.is_dir() {
            walkdir::WalkDir::new(&mb.dir)
                .max_depth(1)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_type().is_file()
                        && e.path().extension().is_some_and(|ext| ext == "md")
                })
                .count()
        } else {
            0
        }
    }).collect()
}
