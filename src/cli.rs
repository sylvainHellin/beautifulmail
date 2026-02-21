use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

/// Return the user's preferred editor (from $EDITOR, fallback to hx).
pub fn editor() -> String {
    std::env::var("EDITOR").unwrap_or_else(|_| "hx".to_string())
}

/// Open a file in $EDITOR (interactive -- requires TUI suspended).
pub fn edit_file(path: &Path) -> Result<()> {
    let editor = editor();
    let status = Command::new(&editor)
        .arg(path)
        .status()
        .with_context(|| format!("Failed to launch editor: {}", editor))?;
    if !status.success() {
        anyhow::bail!("Editor exited with status: {}", status);
    }
    Ok(())
}

/// Run `email reply [--all] <file>` non-interactively, returning the draft path.
pub fn reply(path: &Path, reply_all: bool) -> Result<PathBuf> {
    let mut cmd = Command::new("email");
    cmd.arg("reply");
    if reply_all {
        cmd.arg("--all");
    }
    cmd.arg(path);
    cmd.env("NO_COLOR", "1");
    let output = cmd.output().context("Failed to run email reply")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("email reply failed: {}", err);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(path_str) = line.strip_prefix("✓ Reply draft created: ") {
            return Ok(PathBuf::from(path_str.trim()));
        }
    }
    anyhow::bail!("Could not parse draft path from email reply output")
}

/// Run `email mark-approved <file>` (silent).
pub fn approve(path: &Path) -> Result<String> {
    let output = Command::new("email")
        .arg("mark-approved")
        .arg(path)
        .output()
        .context("Failed to run email mark-approved")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("mark-approved failed: {}", err);
    }
    let msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(msg)
}

/// Run `email send --yes <file>` (non-interactive, captures output).
pub fn send(path: &Path) -> Result<String> {
    let output = Command::new("email")
        .args(["send", "--yes"])
        .arg(path)
        .env("NO_COLOR", "1")
        .output()
        .context("Failed to run email send")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("email send failed: {}", err);
    }
    let msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(msg)
}

/// Run `email send-approved --yes [dir]` (non-interactive, captures output).
pub fn send_approved(dir: &Path) -> Result<String> {
    let output = Command::new("email")
        .args(["send-approved", "--yes"])
        .arg(dir)
        .env("NO_COLOR", "1")
        .output()
        .context("Failed to run email send-approved")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("email send-approved failed: {}", err);
    }
    let msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(msg)
}

/// Run `email fetch` (silent, captures output).
pub fn fetch() -> Result<String> {
    let output = Command::new("email")
        .arg("fetch")
        .output()
        .context("Failed to run email fetch")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("email fetch failed: {}", err);
    }
    let msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(msg)
}

/// Run `email sync` (silent, captures output).
pub fn sync() -> Result<String> {
    let output = Command::new("email")
        .arg("sync")
        .output()
        .context("Failed to run email sync")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("email sync failed: {}", err);
    }
    let msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(msg)
}

/// Run `email new <name>` (silent, returns output message).
pub fn new_draft(name: &str) -> Result<String> {
    let output = Command::new("email")
        .arg("new")
        .arg(name)
        .output()
        .context("Failed to run email new")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("email new failed: {}", err);
    }
    let msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(msg)
}

/// Run `email delete <file>` (deletes server-side via IMAP + removes locally).
pub fn delete(path: &Path) -> Result<String> {
    let output = Command::new("email")
        .arg("delete")
        .arg(path)
        .env("NO_COLOR", "1")
        .output()
        .context("Failed to run email delete")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("email delete failed: {}", err);
    }
    let msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(msg)
}

/// Run `email archive <file>` (archives server-side via IMAP + moves locally).
pub fn archive(path: &Path) -> Result<String> {
    let output = Command::new("email")
        .arg("archive")
        .arg(path)
        .env("NO_COLOR", "1")
        .output()
        .context("Failed to run email archive")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("email archive failed: {}", err);
    }
    let msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(msg)
}

/// Copy text to system clipboard.
pub fn copy_to_clipboard(text: &str) -> Result<()> {
    let mut clipboard =
        arboard::Clipboard::new().context("Failed to access clipboard")?;
    clipboard
        .set_text(text)
        .context("Failed to copy to clipboard")?;
    Ok(())
}
