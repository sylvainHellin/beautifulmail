# beautifulmail

A polished terminal user interface for managing email, built with Rust.

beautifulmail is a TUI frontend for the [`email`](https://github.com/sylvainhellin/email) CLI. It provides a three-panel email client experience directly in the terminal -- sidebar, email list, and preview -- while delegating all backend operations (fetch, send, reply, archive) to the `email` binary via subprocess calls.

Emails are stored locally as Markdown files with YAML frontmatter.

![beautifulmail screenshot](./screenshot.png)

## Features

- **Three-panel layout** -- sidebar with mailbox list, email table, and headers + body preview
- **Responsive design** -- adapts from full three-panel (>=80 cols) to compact single-panel (<40 cols)
- **Vim-style navigation** -- `j`/`k` movement, `gg`/`G` jumps, `d`/`u` half-page scrolling
- **Search and filter** -- `/` for metadata search, `\` for full-text content search, with live match count
- **Background mail watcher** -- IMAP IDLE integration for real-time new mail notifications
- **Email actions** -- reply, reply-all, archive, delete, send, approve drafts, create new drafts
- **Editor integration** -- opens emails in `$EDITOR` with proper TUI suspend/resume
- **Quote-depth rendering** -- styled pipe characters for nested email quotes
- **Catppuccin Mocha theme** -- consistent dark color palette

## Installation

### Prerequisites

- Rust toolchain (edition 2021)
- The [`email`](https://github.com/sylvainhellin/email) CLI binary installed and on your PATH
- A [Nerd Font](https://www.nerdfonts.com/) for sidebar icons

### Build and install

```bash
cargo install --path .
```

## Usage

```bash
beautifulmail
```

No arguments needed -- the TUI launches directly and reads configuration from `~/.config/email/config.toml`.

### Keybindings

#### Global

| Key | Action |
|-----|--------|
| `q` | Quit |
| `?` | Toggle help overlay |
| `1`-`9` | Jump to mailbox by number |
| `s` | Focus sidebar |
| `Tab` / `l` | Cycle focus forward |
| `Shift+Tab` / `h` | Cycle focus backward |
| `/` | Filter by metadata |
| `\` | Search content |

#### Email list

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate up/down |
| `gg` / `G` | Jump to top/bottom |
| `Enter` / `e` | Open in editor |
| `r` / `R` | Reply / Reply all |
| `a` | Archive |
| `d` | Delete |
| `n` | New draft |
| `A` | Approve draft |
| `x` / `X` | Send / Send all approved |
| `f` / `F` | Fetch / Full sync |
| `S` | Sync with reconcile |
| `y` | Copy file path to clipboard |

#### Preview

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll line by line |
| `d` / `u` | Half-page down/up |
| `Esc` | Return to list |

## Configuration

beautifulmail shares its configuration with the `email` CLI at `~/.config/email/config.toml`:

```toml
[directories]
root = "~/notes/email"
drafts = "drafts"

[mailboxes.inbox]
server = "INBOX"
local = "inbox"

[mailboxes.archive]
server = "Archive"
local = "archive"

[mailboxes.sent]
server = "Sent Items"
local = "sent"

# Additional mailboxes
[[mailboxes.extra]]
server = "Projects"
local = "projects"
```

Without a config file, it defaults to `~/notes/email/` with inbox, drafts, sent, and archive mailboxes.

## Architecture

Built with the TEA (The Elm Architecture) pattern:

- **Model** (`app.rs`) -- application state
- **Update** (`app.rs`) -- message processing and state transitions
- **View** (`ui.rs`) -- pure rendering from state

Side effects (CLI calls, editor spawning) are deferred via an `Action` enum and executed in the main event loop.

### Dependencies

| Crate | Purpose |
|-------|---------|
| `ratatui` | TUI framework |
| `crossterm` | Terminal backend |
| `gray_matter` + `serde_yaml` | Frontmatter parsing |
| `chrono` | Date handling |
| `walkdir` | Directory traversal |
| `arboard` | Clipboard access |
| `anyhow` | Error handling |

## License

MIT -- see [LICENSE](./LICENSE).
