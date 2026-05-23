// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::map_unwrap_or)]
#![allow(clippy::redundant_closure_for_method_calls)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::trim_split_whitespace)]
#![allow(clippy::doc_link_with_quotes)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::unnecessary_map_or)]

use anyhow::{Result, anyhow};
use async_imap::Session;
use async_imap::extensions::idle::IdleResponse;
use async_imap::types::Fetch;
use async_trait::async_trait;
use futures_util::TryStreamExt;
use lettre::message::SinglePart;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use mail_parser::{MessageParser, MimeHeaders};
use rustls::{ClientConfig, RootCertStore};
use rustls_pki_types::DnsName;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc};
use tokio::time::{sleep, timeout};
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::traits::{Channel, ChannelMessage, SendMessage};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EmailConfig {

    pub imap_host: String,

    #[serde(default = "default_imap_port")]
    pub imap_port: u16,

    #[serde(default = "default_imap_folder")]
    pub imap_folder: String,

    pub smtp_host: String,

    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,

    #[serde(default = "default_true")]
    pub smtp_tls: bool,

    pub username: String,

    pub password: String,

    pub from_address: String,

    #[serde(default = "default_idle_timeout", alias = "poll_interval_secs")]
    pub idle_timeout_secs: u64,

    #[serde(default)]
    pub allowed_senders: Vec<String>,

    #[serde(default = "default_subject")]
    pub default_subject: String,
}

impl crate::config::traits::ChannelConfig for EmailConfig {
    fn name() -> &'static str {
        "Email"
    }
    fn desc() -> &'static str {
        "Email over IMAP/SMTP"
    }
}

fn default_imap_port() -> u16 {
    993
}
fn default_smtp_port() -> u16 {
    465
}
fn default_imap_folder() -> String {
    "INBOX".into()
}
fn default_idle_timeout() -> u64 {
    1740
}
fn default_true() -> bool {
    true
}
fn default_subject() -> String {
    "SenWeaverCoding Message".into()
}

impl Default for EmailConfig {
    fn default() -> Self {
        Self {
            imap_host: String::new(),
            imap_port: default_imap_port(),
            imap_folder: default_imap_folder(),
            smtp_host: String::new(),
            smtp_port: default_smtp_port(),
            smtp_tls: true,
            username: String::new(),
            password: String::new(),
            from_address: String::new(),
            idle_timeout_secs: default_idle_timeout(),
            allowed_senders: Vec::new(),
            default_subject: default_subject(),
        }
    }
}

type ImapSession = Session<TlsStream<TcpStream>>;

pub struct EmailChannel {
    pub config: EmailConfig,
    seen_messages: Arc<Mutex<HashSet<String>>>,
}

impl EmailChannel {
    pub fn new(config: EmailConfig) -> Self {
        Self {
            config,
            seen_messages: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn is_sender_allowed(&self, email: &str) -> bool {
        if self.config.allowed_senders.is_empty() {
            return false;
        }
        if self.config.allowed_senders.iter().any(|a| a == "*") {
            return true;
        }
        let email_lower = email.to_lowercase();
        self.config.allowed_senders.iter().any(|allowed| {
            if allowed.starts_with('@') {

                email_lower.ends_with(&allowed.to_lowercase())
            } else if allowed.contains('@') {

                allowed.eq_ignore_ascii_case(email)
            } else {

                email_lower.ends_with(&format!("@{}", allowed.to_lowercase()))
            }
        })
    }

    pub fn strip_html(html: &str) -> String {
        let mut result = String::new();
        let mut in_tag = false;
        for ch in html.chars() {
            match ch {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => result.push(ch),
                _ => {}
            }
        }
        let mut normalized = String::with_capacity(result.len());
        for word in result.split_whitespace() {
            if !normalized.is_empty() {
                normalized.push(' ');
            }
            normalized.push_str(word);
        }
        normalized
    }

    fn extract_sender(parsed: &mail_parser::Message) -> String {
        parsed
            .from()
            .and_then(|addr| addr.first())
            .and_then(|a| a.address())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".into())
    }

    fn extract_text(parsed: &mail_parser::Message) -> String {
        if let Some(text) = parsed.body_text(0) {
            return text.to_string();
        }
        if let Some(html) = parsed.body_html(0) {
            return Self::strip_html(html.as_ref());
        }
        for part in parsed.attachments() {
            let part: &mail_parser::MessagePart = part;
            if let Some(ct) = MimeHeaders::content_type(part) {
                if ct.ctype() == "text" {
                    if let Ok(text) = std::str::from_utf8(part.contents()) {
                        let name = MimeHeaders::attachment_name(part).unwrap_or("file");
                        return format!("[Attachment: {}]\n{}", name, text);
                    }
                }
            }
        }
        "(no readable content)".to_string()
    }

    async fn connect_imap(&self) -> Result<ImapSession> {
        let addr = format!("{}:{}", self.config.imap_host, self.config.imap_port);
        debug!("Connecting to IMAP server at {}", addr);

        let tcp = TcpStream::connect(&addr).await?;

        let certs = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.into(),
        };
        let config = ClientConfig::builder()
            .with_root_certificates(certs)
            .with_no_client_auth();
        let tls_stream: TlsConnector = Arc::new(config).into();
        let sni: DnsName = self.config.imap_host.clone().try_into()?;
        let stream = tls_stream.connect(sni.into(), tcp).await?;

        let client = async_imap::Client::new(stream);

        let session = client
            .login(&self.config.username, &self.config.password)
            .await
            .map_err(|(e, _)| anyhow!("IMAP login failed: {}", e))?;

        debug!("IMAP login successful");
        Ok(session)
    }

    const MAX_FETCH_BATCH: usize = 10;

    async fn fetch_unseen(&self, session: &mut ImapSession) -> Result<Vec<ParsedEmail>> {

        let uids = session.uid_search("UNSEEN").await?;
        if uids.is_empty() {
            return Ok(Vec::new());
        }

        debug!("Found {} unseen messages", uids.len());

        let uid_list: Vec<u32> = uids.into_iter().collect();
        let mut results = Vec::new();

        for chunk in uid_list.chunks(Self::MAX_FETCH_BATCH) {
            let uid_set: String = chunk
                .iter()
                .map(|u| u.to_string())
                .collect::<Vec<_>>()
                .join(",");

            let messages = session.uid_fetch(&uid_set, "RFC822").await?;
            let messages: Vec<Fetch> = messages.try_collect().await?;

            for msg in messages {
                let uid = msg.uid.unwrap_or(0);
                if let Some(body) = msg.body() {
                    if let Some(parsed) = MessageParser::default().parse(body) {
                        let sender = Self::extract_sender(&parsed);
                        let subject = parsed.subject().unwrap_or("(no subject)").to_string();
                        let body_text = Self::extract_text(&parsed);
                        let content = format!("Subject: {}\n\n{}", subject, body_text);
                        let msg_id = parsed
                            .message_id()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| format!("gen-{}", Uuid::new_v4()));

                        #[allow(clippy::cast_sign_loss)]
                        let ts = parsed
                            .date()
                            .map(|d| {
                                let naive = chrono::NaiveDate::from_ymd_opt(
                                    d.year as i32,
                                    u32::from(d.month),
                                    u32::from(d.day),
                                )
                                .and_then(|date| {
                                    date.and_hms_opt(
                                        u32::from(d.hour),
                                        u32::from(d.minute),
                                        u32::from(d.second),
                                    )
                                });
                                naive.map_or(0, |n| n.and_utc().timestamp() as u64)
                            })
                            .unwrap_or_else(|| {
                                SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .map(|d| d.as_secs())
                                    .unwrap_or(0)
                            });

                        results.push(ParsedEmail {
                            _uid: uid,
                            msg_id,
                            sender,
                            content,
                            timestamp: ts,
                        });
                    }
                }
            }

            let _ = session
                .uid_store(&uid_set, "+FLAGS (\\Seen)")
                .await?
                .try_collect::<Vec<_>>()
                .await;
        }

        Ok(results)
    }

    async fn wait_for_changes(
        &self,
        session: ImapSession,
    ) -> Result<(IdleWaitResult, ImapSession)> {
        let idle_timeout = Duration::from_secs(self.config.idle_timeout_secs);

        let mut idle = session.idle();
        idle.init().await?;

        debug!("Entering IMAP IDLE mode");

        let (wait_future, _stop_source) = idle.wait();

        let result = timeout(idle_timeout, wait_future).await;

        match result {
            Ok(Ok(response)) => {
                debug!("IDLE response: {:?}", response);

                let session = idle.done().await?;
                let wait_result = match response {
                    IdleResponse::NewData(_) => IdleWaitResult::NewMail,
                    IdleResponse::Timeout => IdleWaitResult::Timeout,
                    IdleResponse::ManualInterrupt => IdleWaitResult::Interrupted,
                };
                Ok((wait_result, session))
            }
            Ok(Err(e)) => {

                let _ = idle.done().await;
                Err(anyhow!("IDLE error: {}", e))
            }
            Err(_) => {

                debug!("IDLE timeout reached, will re-establish");
                let session = idle.done().await?;
                Ok((IdleWaitResult::Timeout, session))
            }
        }
    }

    async fn listen_with_idle(&self, tx: mpsc::Sender<ChannelMessage>) -> Result<()> {
        let mut backoff = Duration::from_secs(1);
        let max_backoff = Duration::from_secs(60);

        loop {
            match self.run_idle_session(&tx).await {
                Ok(()) => {

                    return Ok(());
                }
                Err(e) => {
                    error!(
                        "IMAP session error: {}. Reconnecting in {:?}...",
                        e, backoff
                    );
                    sleep(backoff).await;

                    backoff = std::cmp::min(backoff * 2, max_backoff);
                }
            }
        }
    }

    async fn run_idle_session(&self, tx: &mpsc::Sender<ChannelMessage>) -> Result<()> {

        let mut session = self.connect_imap().await?;

        session.select(&self.config.imap_folder).await?;
        info!(
            "Email IDLE listening on {} (instant push enabled)",
            self.config.imap_folder
        );

        self.process_unseen(&mut session, tx).await?;

        loop {

            match self.wait_for_changes(session).await {
                Ok((IdleWaitResult::NewMail, returned_session)) => {
                    debug!("New mail notification received");
                    session = returned_session;
                    self.process_unseen(&mut session, tx).await?;
                }
                Ok((IdleWaitResult::Timeout, returned_session)) => {

                    session = returned_session;
                    self.process_unseen(&mut session, tx).await?;
                }
                Ok((IdleWaitResult::Interrupted, _)) => {
                    info!("IDLE interrupted, exiting");
                    return Ok(());
                }
                Err(e) => {

                    return Err(e);
                }
            }
        }
    }

    async fn process_unseen(
        &self,
        session: &mut ImapSession,
        tx: &mpsc::Sender<ChannelMessage>,
    ) -> Result<()> {
        let messages = self.fetch_unseen(session).await?;

        for email in messages {

            if !self.is_sender_allowed(&email.sender) {
                warn!("Blocked email from {}", email.sender);
                continue;
            }

            let is_new = {
                let mut seen = self.seen_messages.lock().await;
                seen.insert(email.msg_id.clone())
            };
            if !is_new {
                continue;
            }

            let msg = ChannelMessage {
                id: email.msg_id,
                reply_target: email.sender.clone(),
                sender: email.sender,
                content: email.content,
                channel: "email".to_string(),
                timestamp: email.timestamp,
                thread_ts: None,
                interruption_scope_id: None,
                attachments: vec![],
            };

            if tx.send(msg).await.is_err() {

                return Ok(());
            }
        }

        Ok(())
    }

    fn create_smtp_transport(&self) -> Result<SmtpTransport> {
        let creds = Credentials::new(self.config.username.clone(), self.config.password.clone());
        let transport = if self.config.smtp_tls {
            SmtpTransport::relay(&self.config.smtp_host)?
                .port(self.config.smtp_port)
                .credentials(creds)
                .build()
        } else {
            SmtpTransport::builder_dangerous(&self.config.smtp_host)
                .port(self.config.smtp_port)
                .credentials(creds)
                .build()
        };
        Ok(transport)
    }
}

struct ParsedEmail {
    _uid: u32,
    msg_id: String,
    sender: String,
    content: String,
    timestamp: u64,
}

enum IdleWaitResult {
    NewMail,
    Timeout,
    Interrupted,
}

#[async_trait]
impl Channel for EmailChannel {
    fn name(&self) -> &str {
        "email"
    }

    async fn send(&self, message: &SendMessage) -> Result<()> {

        let default_subject = self.config.default_subject.as_str();
        let (subject, body) = if let Some(ref subj) = message.subject {
            (subj.as_str(), message.content.as_str())
        } else if message.content.starts_with("Subject: ") {
            if let Some(pos) = message.content.find('\n') {
                (&message.content[9..pos], message.content[pos + 1..].trim())
            } else {
                (default_subject, message.content.as_str())
            }
        } else {
            (default_subject, message.content.as_str())
        };

        let email = Message::builder()
            .from(self.config.from_address.parse()?)
            .to(message.recipient.parse()?)
            .subject(subject)
            .singlepart(SinglePart::plain(body.to_string()))?;

        let transport = self.create_smtp_transport()?;
        transport.send(&email)?;
        info!("Email sent to {}", message.recipient);
        Ok(())
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> Result<()> {
        info!(
            "Starting email channel with IDLE support on {}",
            self.config.imap_folder
        );
        self.listen_with_idle(tx).await
    }

    async fn health_check(&self) -> bool {

        match timeout(Duration::from_secs(10), self.connect_imap()).await {
            Ok(Ok(mut session)) => {

                let _ = session.logout().await;
                true
            }
            Ok(Err(e)) => {
                debug!("Health check failed: {}", e);
                false
            }
            Err(_) => {
                debug!("Health check timed out");
                false
            }
        }
    }
}
