// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::providers::traits::ChatMessage;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct SessionMetadata {

    pub key: String,

    pub name: Option<String>,

    pub work_dir: Option<String>,

    pub created_at: DateTime<Utc>,

    pub last_activity: DateTime<Utc>,

    pub message_count: usize,
}

#[derive(Debug, Clone)]
pub struct LoadedMessage {

    pub id: i64,

    pub message: ChatMessage,

    pub tombstoned_at: Option<String>,

    pub hidden_for_ui: bool,
}

#[derive(Debug, Clone)]
pub struct RewindStash {
    pub rewind_id: String,
    pub session_key: String,
    pub user_message_index: i64,

    pub stash_json: String,
}

#[derive(Debug, Clone, Default)]
pub struct SessionQuery {

    pub keyword: Option<String>,

    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct DesignArtifactRecord {
    pub rel_path: String,
    pub submode: Option<String>,
    pub surface: String,
    pub created_at: i64,
    pub updated_at: i64,
}

pub trait SessionBackend: Send + Sync {

    fn load(&self, session_key: &str) -> Vec<ChatMessage>;

    fn append(&self, session_key: &str, message: &ChatMessage) -> std::io::Result<()>;

    fn append_hidden(&self, session_key: &str, message: &ChatMessage) -> std::io::Result<()> {
        self.append(session_key, message)
    }

    fn remove_last(&self, session_key: &str) -> std::io::Result<bool>;

    fn list_sessions(&self) -> Vec<String>;

    fn list_sessions_with_metadata(&self) -> Vec<SessionMetadata> {
        let now = Utc::now();
        self.list_sessions()
            .into_iter()
            .map(|key| {
                let messages = self.load(&key);
                SessionMetadata {
                    key,
                    name: None,
                    work_dir: None,
                    created_at: now,
                    last_activity: now,
                    message_count: messages.len(),
                }
            })
            .collect()
    }

    fn get_session_metadata(&self, session_key: &str) -> Option<SessionMetadata> {
        self.list_sessions_with_metadata()
            .into_iter()
            .find(|m| m.key == session_key)
    }

    fn count_user_messages(&self, session_key: &str) -> usize {
        self.load_with_tombstones(session_key)
            .iter()
            .filter(|m| m.message.role == "user")
            .count()
    }

    fn count_messages(&self, session_key: &str) -> usize {
        self.load_with_tombstones(session_key).len()
    }

    fn load_tail(&self, session_key: &str, limit: usize) -> Vec<ChatMessage> {
        let mut all = self.load(session_key);
        if all.len() > limit {
            all.split_off(all.len() - limit)
        } else {
            all
        }
    }

    fn load_with_tombstones_range(
        &self,
        session_key: &str,
        offset: usize,
        limit: usize,
    ) -> Vec<LoadedMessage> {
        self.load_with_tombstones(session_key)
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect()
    }

    fn compact(&self, _session_key: &str) -> std::io::Result<()> {
        Ok(())
    }

    fn cleanup_stale(&self, _ttl_hours: u32) -> std::io::Result<usize> {
        Ok(0)
    }

    fn search(&self, _query: &SessionQuery) -> Vec<SessionMetadata> {
        Vec::new()
    }

    fn delete_session(&self, _session_key: &str) -> std::io::Result<bool> {
        Ok(false)
    }

    fn delete_sessions(&self, session_keys: &[String]) -> std::io::Result<usize> {
        let mut deleted = 0usize;
        for key in session_keys {
            if self.delete_session(key)? {
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    fn set_session_name(&self, _session_key: &str, _name: &str) -> std::io::Result<()> {
        Ok(())
    }

    fn get_session_name(&self, _session_key: &str) -> std::io::Result<Option<String>> {
        Ok(None)
    }

    fn set_session_work_dir(&self, _session_key: &str, _dir: &str) -> std::io::Result<()> {
        Ok(())
    }

    fn get_session_work_dir(&self, _session_key: &str) -> std::io::Result<Option<String>> {
        Ok(None)
    }

    fn load_with_tombstones(&self, session_key: &str) -> Vec<LoadedMessage> {
        self.load(session_key)
            .into_iter()
            .enumerate()
            .map(|(i, message)| LoadedMessage {
                #[allow(clippy::cast_possible_wrap)]
                id: i as i64,
                message,
                tombstoned_at: None,
                hidden_for_ui: false,
            })
            .collect()
    }

    fn tombstone_from(&self, _session_key: &str, _first_id: i64) -> std::io::Result<usize> {
        Ok(0)
    }

    fn clear_tombstones(&self, _session_key: &str) -> std::io::Result<usize> {
        Ok(0)
    }

    fn purge_tombstoned(&self, _session_key: &str) -> std::io::Result<usize> {
        Ok(0)
    }

    fn record_edit_batch(
        &self,
        _session_key: &str,
        _user_message_index: i64,
        _edit_batch_id: &str,
    ) -> std::io::Result<()> {
        Ok(())
    }

    fn edit_batches_after(
        &self,
        _session_key: &str,
        _from_index: i64,
    ) -> Vec<String> {
        Vec::new()
    }

    fn drop_edit_batches_after(
        &self,
        _session_key: &str,
        _from_index: i64,
    ) -> std::io::Result<usize> {
        Ok(0)
    }

    fn save_rewind_stash(
        &self,
        _rewind_id: &str,
        _session_key: &str,
        _user_message_index: i64,
        _stash_json: &str,
    ) -> std::io::Result<()> {
        Ok(())
    }

    fn take_rewind_stash(&self, _rewind_id: &str) -> Option<RewindStash> {
        None
    }

    fn latest_rewind_stash_for_session(
        &self,
        _session_key: &str,
    ) -> Option<RewindStash> {
        None
    }

    fn record_design_artifact(
        &self,
        _session_key: &str,
        _rel_path: &str,
        _submode: Option<&str>,
        _surface: &str,
    ) -> std::io::Result<()> {
        Ok(())
    }

    fn list_design_artifacts(&self, _session_key: &str) -> Vec<DesignArtifactRecord> {
        Vec::new()
    }

    fn delete_design_artifact(&self, _session_key: &str, _rel_path: &str) -> std::io::Result<()> {
        Ok(())
    }
}
