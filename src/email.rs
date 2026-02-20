use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::NaiveDate;
use gray_matter::engine::YAML;
use gray_matter::Matter;
use serde::Deserialize;

use crate::app::Mailbox;

/// Parsed email entry for display in the list and preview.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct EmailEntry {
    pub path: PathBuf,
    pub from: String,
    pub to: String,
    pub cc: Option<String>,
    pub subject: String,
    pub status: String,
    pub date_display: String,
    pub date_sort: String,
    pub body: String,
    pub has_attachments: bool,
}

impl EmailEntry {
    /// The contact to display depends on the mailbox:
    /// Inbox/Archive show `from`, Drafts/Sent show `to`.
    pub fn display_contact(&self, mailbox: Mailbox) -> &str {
        match mailbox {
            Mailbox::Inbox | Mailbox::Archive => &self.from,
            Mailbox::Drafts | Mailbox::Sent => &self.to,
        }
    }
}

/// Raw frontmatter fields (all optional to handle varying formats).
#[derive(Debug, Deserialize, Default)]
struct Frontmatter {
    from: Option<String>,
    to: Option<String>,
    cc: Option<String>,
    subject: Option<String>,
    status: Option<String>,
    date: Option<String>,
    sent_at: Option<String>,
    has_attachments: Option<bool>,
}

/// Load all emails from a directory.
pub fn load_emails(dir: &Path) -> Vec<EmailEntry> {
    let mut entries = Vec::new();

    let walker = walkdir::WalkDir::new(dir)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.path().extension().is_some_and(|ext| ext == "md")
        });

    for entry in walker {
        match parse_email(entry.path()) {
            Ok(email) => entries.push(email),
            Err(_) => continue, // Skip unparseable files
        }
    }

    // Sort by date descending (newest first)
    entries.sort_by(|a, b| b.date_sort.cmp(&a.date_sort));
    entries
}

/// Parse a single email markdown file.
fn parse_email(path: &Path) -> Result<EmailEntry> {
    let content = std::fs::read_to_string(path)?;
    let matter = Matter::<YAML>::new();
    let result = matter.parse(&content);

    let fm: Frontmatter = result
        .data
        .and_then(|d| d.deserialize().ok())
        .unwrap_or_default();

    let body = result.content;

    let from = fm.from.unwrap_or_default();
    let to = fm.to.unwrap_or_default();
    let subject = fm.subject.unwrap_or_else(|| "(no subject)".to_string());
    let status = fm.status.unwrap_or_else(|| "unknown".to_string());

    // Resolve date: try `date` field (RFC 2822), then `sent_at` (ISO 8601),
    // then fall back to filename prefix (YYYY-MM-DD).
    let (date_display, date_sort) = resolve_date(&fm.date, &fm.sent_at, path);

    Ok(EmailEntry {
        path: path.to_path_buf(),
        from: extract_display_name(&from),
        to: extract_display_name(&to),
        cc: fm.cc,
        subject,
        status,
        date_display,
        date_sort,
        body,
        has_attachments: fm.has_attachments.unwrap_or(false),
    })
}

/// Extract a short display name from an email address.
/// "Sylvain Hellin <sylvain.hellin@tum.de>" -> "Sylvain Hellin"
/// "sylvain.hellin@tum.de" -> "sylvain.hellin@tum.de"
fn extract_display_name(addr: &str) -> String {
    let addr = addr.trim().trim_matches('"');
    if let Some(idx) = addr.find('<') {
        let name = addr[..idx].trim().trim_matches('"');
        if name.is_empty() {
            // "<foo@bar.com>" -> "foo@bar.com"
            addr.trim_matches(|c| c == '<' || c == '>').to_string()
        } else {
            name.to_string()
        }
    } else {
        addr.to_string()
    }
}

/// Resolve date for display and sorting.
fn resolve_date(
    date_field: &Option<String>,
    sent_at_field: &Option<String>,
    path: &Path,
) -> (String, String) {
    // Try RFC 2822 date field (inbox emails)
    if let Some(date_str) = date_field {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(date_str) {
            let display = dt.format("%Y-%m-%d").to_string();
            let sort = dt.format("%Y-%m-%dT%H:%M:%S").to_string();
            return (display, sort);
        }
    }

    // Try ISO 8601 sent_at field (sent emails)
    if let Some(sent_str) = sent_at_field {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(sent_str) {
            let display = dt.format("%Y-%m-%d").to_string();
            let sort = dt.format("%Y-%m-%dT%H:%M:%S").to_string();
            return (display, sort);
        }
        // Try without timezone (some sent_at may be bare ISO)
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(sent_str, "%Y-%m-%dT%H:%M:%SZ") {
            let display = dt.format("%Y-%m-%d").to_string();
            let sort = dt.format("%Y-%m-%dT%H:%M:%S").to_string();
            return (display, sort);
        }
    }

    // Fall back to filename prefix: "2026-02-19-1307_..." or "2026-02-19_..."
    let filename = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    if filename.len() >= 10 {
        let date_part = &filename[..10];
        // Try "2026-02-19" directly
        if NaiveDate::parse_from_str(date_part, "%Y-%m-%d").is_ok() {
            // Extract time from "2026-02-19-1307" pattern if present
            if filename.len() >= 15 && filename.as_bytes()[10] == b'-' {
                let time_part = &filename[11..15];
                if time_part.chars().all(|c| c.is_ascii_digit()) && time_part.len() == 4 {
                    let sort = format!(
                        "{}T{}:{}:00",
                        date_part,
                        &time_part[..2],
                        &time_part[2..4]
                    );
                    return (date_part.to_string(), sort);
                }
            }
            return (date_part.to_string(), format!("{date_part}T00:00:00"));
        }
    }

    ("".to_string(), "".to_string())
}
