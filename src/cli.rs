use std::path::Path;
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

/// Run `email reply [--all] <file>` (interactive -- opens hx).
pub fn reply(path: &Path, reply_all: bool) -> Result<()> {
    let mut cmd = Command::new("email");
    cmd.arg("reply");
    if reply_all {
        cmd.arg("--all");
    }
    cmd.arg(path);
    let status = cmd.status().context("Failed to run email reply")?;
    if !status.success() {
        anyhow::bail!("email reply exited with status: {}", status);
    }
    Ok(())
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

/// Run `email send <file>` (interactive -- has confirmation prompt).
pub fn send(path: &Path) -> Result<()> {
    let status = Command::new("email")
        .arg("send")
        .arg(path)
        .status()
        .context("Failed to run email send")?;
    if !status.success() {
        anyhow::bail!("email send exited with status: {}", status);
    }
    Ok(())
}

/// Run `email send-approved [dir]` (interactive -- has confirmation prompt).
pub fn send_approved(dir: &Path) -> Result<()> {
    let status = Command::new("email")
        .arg("send-approved")
        .arg(dir)
        .status()
        .context("Failed to run email send-approved")?;
    if !status.success() {
        anyhow::bail!("email send-approved exited with status: {}", status);
    }
    Ok(())
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

/// Delete a file from disk.
pub fn delete_file(path: &Path) -> Result<()> {
    std::fs::remove_file(path)
        .with_context(|| format!("Failed to delete: {}", path.display()))?;
    Ok(())
}

/// Archive an email: update status in frontmatter, move to archive directory.
pub fn archive_file(path: &Path, archive_dir: &Path) -> Result<()> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read: {}", path.display()))?;
    let updated = content.replace("status: inbox", "status: archived");
    let filename = path
        .file_name()
        .context("No filename on email path")?;
    let dest = archive_dir.join(filename);
    std::fs::write(&dest, updated)
        .with_context(|| format!("Failed to write: {}", dest.display()))?;
    std::fs::remove_file(path)
        .with_context(|| format!("Failed to remove original: {}", path.display()))?;
    Ok(())
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
