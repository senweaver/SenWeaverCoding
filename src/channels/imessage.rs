// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::channels::traits::{Channel, ChannelMessage, SendMessage};
use async_trait::async_trait;
use directories::UserDirs;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use tokio::sync::mpsc;

fn extract_text_from_attributed_body(blob: &[u8]) -> Option<String> {

    let marker_pos = blob.windows(2).position(|w| w == [0x01, 0x2B])?;
    let rest = blob.get(marker_pos + 2..)?;

    if rest.is_empty() {
        return None;
    }

    let (length, text_start) = match rest[0] {
        0x81 if rest.len() >= 3 => {
            let len = u16::from_le_bytes([rest[1], rest[2]]) as usize;
            (len, 3)
        }
        0x82 if rest.len() >= 5 => {
            let len = u32::from_le_bytes([rest[1], rest[2], rest[3], rest[4]]) as usize;
            (len, 5)
        }
        b if b <= 0x7F => (b as usize, 1),
        _ => return None,
    };

    let text_bytes = rest.get(text_start..text_start + length)?;
    std::str::from_utf8(text_bytes).ok().map(str::to_owned)
}

fn resolve_message_content(rowid: i64, text: Option<String>, body: Option<Vec<u8>>) -> String {
    text.filter(|t| !t.trim().is_empty())
        .or_else(|| {
            let parsed = body.as_deref().and_then(extract_text_from_attributed_body);
            if parsed.is_none() && body.as_ref().is_some_and(|b| !b.is_empty()) {
                tracing::warn!(rowid, "failed to parse attributedBody");
            }
            parsed
        })
        .unwrap_or_default()
}

#[derive(Clone)]
pub struct IMessageChannel {
    allowed_contacts: Vec<String>,
    poll_interval_secs: u64,
}

impl IMessageChannel {
    pub fn new(allowed_contacts: Vec<String>) -> Self {
        Self {
            allowed_contacts,
            poll_interval_secs: 3,
        }
    }

    fn is_contact_allowed(&self, sender: &str) -> bool {
        if self.allowed_contacts.iter().any(|u| u == "*") {
            return true;
        }
        self.allowed_contacts
            .iter()
            .any(|u| u.eq_ignore_ascii_case(sender))
    }
}

fn escape_applescript(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn is_valid_imessage_target(target: &str) -> bool {
    let target = target.trim();
    if target.is_empty() {
        return false;
    }

    if target.starts_with('+') {
        let digits_only: String = target.chars().filter(char::is_ascii_digit).collect();

        return digits_only.len() >= 7 && digits_only.len() <= 15;
    }

    if let Some(at_pos) = target.find('@') {
        let local = &target[..at_pos];
        let domain = &target[at_pos + 1..];

        let local_valid = !local.is_empty()
            && local
                .chars()
                .all(|c| c.is_alphanumeric() || "._+-".contains(c));

        let domain_valid = !domain.is_empty()
            && domain.contains('.')
            && domain
                .chars()
                .all(|c| c.is_alphanumeric() || ".-".contains(c));

        return local_valid && domain_valid;
    }

    false
}

#[async_trait]
impl Channel for IMessageChannel {
    fn name(&self) -> &str {
        "imessage"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {

        if !is_valid_imessage_target(&message.recipient) {
            anyhow::bail!(
                "Invalid iMessage target: must be a phone number (+1234567890) or email (user@example.com)"
            );
        }

        let escaped_msg = escape_applescript(&message.content);
        let escaped_target = escape_applescript(&message.recipient);

        let script = format!(
            r#"tell application "Messages"
    set targetService to 1st account whose service type = iMessage
    set targetBuddy to participant "{escaped_target}" of targetService
    send "{escaped_msg}" to targetBuddy
end tell"#
        );

        let output = tokio::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("iMessage send failed: {stderr}");
        }

        Ok(())
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        tracing::info!("iMessage channel listening (AppleScript bridge)...");

        let db_path = UserDirs::new()
            .map(|u| u.home_dir().join("Library/Messages/chat.db"))
            .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;

        if !db_path.exists() {
            anyhow::bail!(
                "Messages database not found at {}. Ensure Messages.app is set up and Full Disk Access is granted.",
                db_path.display()
            );
        }

        let path = db_path.to_path_buf();
        let conn = tokio::task::spawn_blocking(move || -> anyhow::Result<Connection> {
            Ok(Connection::open_with_flags(
                &path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?)
        })
        .await??;

        let (mut conn, initial_rowid) =
            tokio::task::spawn_blocking(move || -> anyhow::Result<(Connection, i64)> {
                let rowid = {
                    let mut stmt =
                        conn.prepare("SELECT MAX(ROWID) FROM message WHERE is_from_me = 0")?;
                    let rowid: Option<i64> = stmt.query_row([], |row| row.get(0))?;
                    rowid.unwrap_or(0)
                };
                Ok((conn, rowid))
            })
            .await??;
        let mut last_rowid = initial_rowid;

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(self.poll_interval_secs)).await;

            let since = last_rowid;
            let (returned_conn, poll_result) = tokio::task::spawn_blocking(
                move || -> (Connection, anyhow::Result<Vec<(i64, String, String)>>) {
                    let result = (|| -> anyhow::Result<Vec<(i64, String, String)>> {
                        let mut stmt = conn.prepare(
                            "SELECT m.ROWID, h.id, m.text, m.attributedBody \
                     FROM message m \
                     JOIN handle h ON m.handle_id = h.ROWID \
                     WHERE m.ROWID > ?1 \
                     AND m.is_from_me = 0 \
                     AND (m.text IS NOT NULL OR m.attributedBody IS NOT NULL) \
                     ORDER BY m.ROWID ASC \
                     LIMIT 20",
                        )?;
                        let rows = stmt.query_map([since], |row| {
                            let rowid = row.get::<_, i64>(0)?;
                            let sender = row.get::<_, String>(1)?;
                            let text: Option<String> = row.get(2)?;
                            let body: Option<Vec<u8>> = row.get(3)?;
                            Ok((rowid, sender, resolve_message_content(rowid, text, body)))
                        })?;
                        let results = rows.collect::<Result<Vec<_>, _>>()?;
                        Ok(results)
                    })();

                    (conn, result)
                },
            )
            .await
            .map_err(|e| anyhow::anyhow!("iMessage poll worker join error: {e}"))?;
            conn = returned_conn;

            match poll_result {
                Ok(messages) => {
                    for (rowid, sender, text) in messages {
                        if rowid > last_rowid {
                            last_rowid = rowid;
                        }

                        if !self.is_contact_allowed(&sender) {
                            continue;
                        }

                        if text.trim().is_empty() {
                            continue;
                        }

                        let msg = ChannelMessage {
                            id: rowid.to_string(),
                            sender: sender.clone(),
                            reply_target: sender.clone(),
                            content: text,
                            channel: "imessage".to_string(),
                            timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            thread_ts: None,
                            interruption_scope_id: None,
                            attachments: vec![],
                        };

                        if tx.send(msg).await.is_err() {
                            return Ok(());
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("iMessage poll error: {e}");
                }
            }
        }
    }

    async fn health_check(&self) -> bool {
        if !cfg!(target_os = "macos") {
            return false;
        }

        let db_path = UserDirs::new()
            .map(|u| u.home_dir().join("Library/Messages/chat.db"))
            .unwrap_or_default();

        db_path.exists()
    }
}

async fn get_max_rowid(db_path: &Path) -> anyhow::Result<i64> {
    let path = db_path.to_path_buf();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<i64> {
        let conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let mut stmt = conn.prepare("SELECT MAX(ROWID) FROM message WHERE is_from_me = 0")?;
        let rowid: Option<i64> = stmt.query_row([], |row| row.get(0))?;
        Ok(rowid.unwrap_or(0))
    })
    .await??;
    Ok(result)
}

async fn fetch_new_messages(
    db_path: &Path,
    since_rowid: i64,
) -> anyhow::Result<Vec<(i64, String, String)>> {
    let path = db_path.to_path_buf();
    let results =
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<(i64, String, String)>> {
            let conn = Connection::open_with_flags(
                &path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            let mut stmt = conn.prepare(
                "SELECT m.ROWID, h.id, m.text, m.attributedBody \
             FROM message m \
             JOIN handle h ON m.handle_id = h.ROWID \
             WHERE m.ROWID > ?1 \
             AND m.is_from_me = 0 \
             AND (m.text IS NOT NULL OR m.attributedBody IS NOT NULL) \
             ORDER BY m.ROWID ASC \
             LIMIT 20",
            )?;
            let rows = stmt.query_map([since_rowid], |row| {
                let rowid = row.get::<_, i64>(0)?;
                let sender = row.get::<_, String>(1)?;
                let text: Option<String> = row.get(2)?;
                let body: Option<Vec<u8>> = row.get(3)?;
                Ok((rowid, sender, resolve_message_content(rowid, text, body)))
            })?;
            let results: Vec<_> = rows
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|(_, _, content)| !content.trim().is_empty())
                .collect();
            Ok(results)
        })
        .await??;
    Ok(results)
}
