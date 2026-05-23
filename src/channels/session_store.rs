// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::channels::session_backend::SessionBackend;
use crate::providers::traits::ChatMessage;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

pub struct SessionStore {
    sessions_dir: PathBuf,
}

impl SessionStore {

    pub fn new(workspace_dir: &Path) -> std::io::Result<Self> {
        let sessions_dir = workspace_dir.join("sessions");
        std::fs::create_dir_all(&sessions_dir)?;
        Ok(Self { sessions_dir })
    }

    fn session_path(&self, session_key: &str) -> PathBuf {
        let safe_key: String = session_key
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.sessions_dir.join(format!("{safe_key}.jsonl"))
    }

    pub fn load(&self, session_key: &str) -> Vec<ChatMessage> {
        let path = self.session_path(session_key);
        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        let reader = std::io::BufReader::new(file);
        let mut messages = Vec::new();

        for line in reader.lines() {
            let Ok(line) = line else { continue };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(msg) = serde_json::from_str::<ChatMessage>(trimmed) {
                messages.push(msg);
            }
        }

        messages
    }

    pub fn append(&self, session_key: &str, message: &ChatMessage) -> std::io::Result<()> {
        let path = self.session_path(session_key);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        let json = serde_json::to_string(message)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        writeln!(file, "{json}")?;
        Ok(())
    }

    pub fn remove_last(&self, session_key: &str) -> std::io::Result<bool> {
        let mut messages = self.load(session_key);
        if messages.is_empty() {
            return Ok(false);
        }
        messages.pop();
        self.rewrite(session_key, &messages)?;
        Ok(true)
    }

    pub fn compact(&self, session_key: &str) -> std::io::Result<()> {
        let messages = self.load(session_key);
        self.rewrite(session_key, &messages)
    }

    fn rewrite(&self, session_key: &str, messages: &[ChatMessage]) -> std::io::Result<()> {
        let path = self.session_path(session_key);
        let mut file = std::fs::File::create(&path)?;
        for msg in messages {
            let json = serde_json::to_string(msg)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            writeln!(file, "{json}")?;
        }
        Ok(())
    }

    pub fn delete_session(&self, session_key: &str) -> std::io::Result<bool> {
        let path = self.session_path(session_key);
        if !path.exists() {
            return Ok(false);
        }
        std::fs::remove_file(&path)?;
        Ok(true)
    }

    pub fn list_sessions(&self) -> Vec<String> {
        let entries = match std::fs::read_dir(&self.sessions_dir) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };

        entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let name = entry.file_name().into_string().ok()?;
                name.strip_suffix(".jsonl").map(String::from)
            })
            .collect()
    }
}

impl SessionBackend for SessionStore {
    fn load(&self, session_key: &str) -> Vec<ChatMessage> {
        self.load(session_key)
    }

    fn append(&self, session_key: &str, message: &ChatMessage) -> std::io::Result<()> {
        self.append(session_key, message)
    }

    fn remove_last(&self, session_key: &str) -> std::io::Result<bool> {
        self.remove_last(session_key)
    }

    fn list_sessions(&self) -> Vec<String> {
        self.list_sessions()
    }

    fn compact(&self, session_key: &str) -> std::io::Result<()> {
        self.compact(session_key)
    }

    fn delete_session(&self, session_key: &str) -> std::io::Result<bool> {
        self.delete_session(session_key)
    }
}
