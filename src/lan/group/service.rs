// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Weak};

use anyhow::{anyhow, bail, Context, Result};
use parking_lot::Mutex;
use serde_json::json;

use super::document::GroupDocStore;
use super::op::{now_ms_u64, GroupInbound, GroupOp, GroupOpPayload, GroupRole, HlcClock};
use super::state::{self, GroupMessageView, GroupSnapshot, GroupSummary};
use super::store::GroupStore;
use super::sync;
use crate::lan::discovery::PeerRegistry;
use crate::lan::identity::LanIdentity;
use crate::lan::protocol::ControlMessage;
use crate::lan::transport::{GroupDocReceived, LanTransport};

pub struct GroupService {
    identity: Arc<LanIdentity>,
    store: Arc<GroupStore>,
    docs: GroupDocStore,
    registry: Arc<PeerRegistry>,
    clock: HlcClock,
    seqs: Mutex<HashMap<String, u64>>,
    transport: Mutex<Weak<LanTransport>>,
}

impl GroupService {
    pub fn new(
        identity: Arc<LanIdentity>,
        store: Arc<GroupStore>,
        registry: Arc<PeerRegistry>,
        docs_root: PathBuf,
    ) -> Arc<Self> {
        Arc::new(Self {
            identity,
            store,
            docs: GroupDocStore::new(docs_root),
            registry,
            clock: HlcClock::new(),
            seqs: Mutex::new(HashMap::new()),
            transport: Mutex::new(Weak::new()),
        })
    }

    pub fn attach_transport(&self, transport: &Arc<LanTransport>) {
        *self.transport.lock() = Arc::downgrade(transport);
    }

    fn transport(&self) -> Option<Arc<LanTransport>> {
        self.transport.lock().upgrade()
    }

    fn user_id(&self) -> String {
        self.identity.user_id().to_string()
    }

    fn online_set(&self) -> HashSet<String> {
        self.registry
            .snapshot()
            .into_iter()
            .map(|p| p.user_id)
            .collect()
    }

    fn nick_for(&self, user_id: &str) -> String {
        if user_id == self.identity.user_id() {
            return self.identity.nickname();
        }
        self.registry
            .get(user_id)
            .map(|r| r.nickname)
            .unwrap_or_else(|| user_id.to_string())
    }

    fn next_seq(&self, group_id: &str) -> u64 {
        let mut map = self.seqs.lock();
        let next = match map.get(group_id) {
            Some(value) => value + 1,
            None => self.store.max_self_seq(group_id) + 1,
        };
        map.insert(group_id.to_string(), next);
        next
    }

    fn make_op(&self, group_id: &str, payload: GroupOpPayload) -> GroupOp {
        GroupOp {
            op_id: uuid::Uuid::new_v4().to_string(),
            group_id: group_id.to_string(),
            hlc: self.clock.tick(),
            seq: self.next_seq(group_id),
            author: self.user_id(),
            payload,
        }
    }

    fn require_manage(&self, group_id: &str) -> Result<GroupRole> {
        let role = self
            .store
            .self_role(group_id)
            .ok_or_else(|| anyhow!("not a member of this group"))?;
        if !role.can_manage() {
            bail!("insufficient permissions: requires manager role");
        }
        Ok(role)
    }

    fn require_contribute(&self, group_id: &str) -> Result<GroupRole> {
        let role = self
            .store
            .self_role(group_id)
            .ok_or_else(|| anyhow!("not a member of this group"))?;
        if !role.can_contribute() {
            bail!("insufficient permissions: read-only member");
        }
        Ok(role)
    }

    fn require_member(&self, group_id: &str) -> Result<GroupRole> {
        self.store
            .self_role(group_id)
            .ok_or_else(|| anyhow!("not a member of this group"))
    }

    fn has_other_owner(&self, group_id: &str, exclude_user_id: &str) -> bool {
        self.store
            .active_member_ids(group_id)
            .into_iter()
            .any(|m| {
                m != exclude_user_id
                    && matches!(self.store.role_of(group_id, &m), Some(GroupRole::Owner))
            })
    }

    // ---- queries ----------------------------------------------------------

    pub fn list_groups(&self) -> Vec<GroupSummary> {
        self.store.list_groups()
    }

    pub fn snapshot(&self, group_id: &str) -> Option<GroupSnapshot> {
        let online = self.online_set();
        let group = self.store.group_summary(group_id)?;
        let members = self.store.members(group_id, &online);
        let phases = self.store.phases(group_id);
        let mut documents = self.store.documents(group_id);
        for doc in &mut documents {
            doc.available = self.docs.is_available(group_id, &doc.id, &doc.name);
        }
        let tasks = self.store.tasks(group_id);
        Some(GroupSnapshot {
            group,
            members,
            phases,
            documents,
            tasks,
        })
    }

    pub fn messages(&self, group_id: &str, limit: i64) -> Vec<GroupMessageView> {
        self.store.messages(group_id, limit)
    }

    pub fn unread_total(&self) -> i64 {
        self.store.unread_total()
    }

    pub fn mark_read(&self, group_id: &str) {
        self.store.mark_read(group_id);
        self.emit_unread();
    }

    // ---- mutations --------------------------------------------------------

    pub fn create_group(self: &Arc<Self>, name: &str, description: &str) -> Result<GroupSummary> {
        let name = name.trim();
        if name.is_empty() {
            bail!("group name must not be empty");
        }
        let group_id = uuid::Uuid::new_v4().to_string();
        let mut ops = Vec::new();
        ops.push(self.make_op(
            &group_id,
            GroupOpPayload::GroupMeta {
                name: name.to_string(),
                description: description.trim().to_string(),
            },
        ));
        ops.push(self.make_op(
            &group_id,
            GroupOpPayload::MemberUpsert {
                user_id: self.user_id(),
                nickname: self.identity.nickname(),
                role: GroupRole::Owner,
            },
        ));
        for (index, phase) in state::default_phases().into_iter().enumerate() {
            ops.push(self.make_op(
                &group_id,
                GroupOpPayload::PhaseUpsert {
                    phase_id: phase.phase_id.to_string(),
                    name: phase.name.to_string(),
                    order: index as i64,
                    status: state::PHASE_STATUS_NOT_STARTED.to_string(),
                    color: phase.color.to_string(),
                },
            ));
        }
        for op in &ops {
            self.store.apply_op(op)?;
        }
        self.emit_groups();
        self.emit_group_changed(&group_id);
        self.store
            .group_summary(&group_id)
            .ok_or_else(|| anyhow!("failed to read created group"))
    }

    pub fn update_meta(
        self: &Arc<Self>,
        group_id: &str,
        name: &str,
        description: &str,
    ) -> Result<()> {
        self.require_manage(group_id)?;
        let name = name.trim();
        if name.is_empty() {
            bail!("group name must not be empty");
        }
        let op = self.make_op(
            group_id,
            GroupOpPayload::GroupMeta {
                name: name.to_string(),
                description: description.trim().to_string(),
            },
        );
        self.commit(group_id, vec![op])?;
        Ok(())
    }

    pub fn invite_member(
        self: &Arc<Self>,
        group_id: &str,
        user_id: &str,
        role: GroupRole,
    ) -> Result<()> {
        self.require_manage(group_id)?;
        if user_id.trim().is_empty() {
            bail!("user id must not be empty");
        }
        if matches!(role, GroupRole::Owner) {
            bail!("cannot invite as owner");
        }
        let nickname = self.nick_for(user_id);
        let op = self.make_op(
            group_id,
            GroupOpPayload::MemberUpsert {
                user_id: user_id.to_string(),
                nickname,
                role,
            },
        );
        self.store.apply_op(&op)?;
        let all_ops = self.store.ops_for_group(group_id);
        let invitee = user_id.to_string();
        self.send_invite(group_id.to_string(), invitee, all_ops);
        self.spawn_gossip(group_id.to_string(), vec![op]);
        self.emit_groups();
        self.emit_group_changed(group_id);
        Ok(())
    }

    pub fn set_role(
        self: &Arc<Self>,
        group_id: &str,
        user_id: &str,
        role: GroupRole,
    ) -> Result<()> {
        self.require_manage(group_id)?;
        if !matches!(role, GroupRole::Owner)
            && matches!(self.store.role_of(group_id, user_id), Some(GroupRole::Owner))
            && !self.has_other_owner(group_id, user_id)
        {
            bail!("cannot demote the last owner; promote another owner first");
        }
        let nickname = self.nick_for(user_id);
        let op = self.make_op(
            group_id,
            GroupOpPayload::MemberUpsert {
                user_id: user_id.to_string(),
                nickname,
                role,
            },
        );
        self.commit(group_id, vec![op])?;
        Ok(())
    }

    pub fn remove_member(self: &Arc<Self>, group_id: &str, user_id: &str) -> Result<()> {
        self.require_manage(group_id)?;
        if let Some(GroupRole::Owner) = self.store.role_of(group_id, user_id) {
            bail!("cannot remove the group owner");
        }
        let op = self.make_op(
            group_id,
            GroupOpPayload::MemberRemove {
                user_id: user_id.to_string(),
            },
        );
        self.commit(group_id, vec![op])?;
        Ok(())
    }

    pub fn leave_group(self: &Arc<Self>, group_id: &str) -> Result<()> {
        self.require_member(group_id)?;
        let me = self.user_id();
        if matches!(self.store.role_of(group_id, &me), Some(GroupRole::Owner)) {
            let members = self.store.active_member_ids(group_id);
            let only_self = members.iter().all(|m| m == &me);
            if !only_self && !self.has_other_owner(group_id, &me) {
                bail!("transfer ownership before leaving: assign another owner first");
            }
        }
        let op = self.make_op(
            group_id,
            GroupOpPayload::MemberRemove {
                user_id: self.user_id(),
            },
        );
        self.store.apply_op(&op)?;
        self.spawn_gossip(group_id.to_string(), vec![op]);
        self.emit_groups();
        self.emit_group_changed(group_id);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_phase(
        self: &Arc<Self>,
        group_id: &str,
        phase_id: Option<String>,
        name: &str,
        order: i64,
        status: &str,
        color: &str,
    ) -> Result<()> {
        self.require_manage(group_id)?;
        let name = name.trim();
        if name.is_empty() {
            bail!("phase name must not be empty");
        }
        let phase_id = phase_id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let op = self.make_op(
            group_id,
            GroupOpPayload::PhaseUpsert {
                phase_id,
                name: name.to_string(),
                order,
                status: state::normalize_phase_status(status).to_string(),
                color: color.trim().to_string(),
            },
        );
        self.commit(group_id, vec![op])?;
        Ok(())
    }

    pub fn remove_phase(self: &Arc<Self>, group_id: &str, phase_id: &str) -> Result<()> {
        self.require_manage(group_id)?;
        let op = self.make_op(
            group_id,
            GroupOpPayload::PhaseRemove {
                phase_id: phase_id.to_string(),
            },
        );
        self.commit(group_id, vec![op])?;
        Ok(())
    }

    pub async fn upload_document(
        self: &Arc<Self>,
        group_id: &str,
        source: &str,
        phase_id: &str,
        note: &str,
    ) -> Result<String> {
        self.require_contribute(group_id)?;
        let source_path = PathBuf::from(shellexpand::tilde(source).to_string());
        let doc_id = uuid::Uuid::new_v4().to_string();
        let version = self.store.next_doc_version(group_id, &doc_id);
        let imported = self.docs.import_local(group_id, &doc_id, &source_path).await?;
        let doc_op = self.make_op(
            group_id,
            GroupOpPayload::DocUpsert {
                doc_id: doc_id.clone(),
                name: imported.name.clone(),
                is_dir: imported.is_dir,
                size: imported.size,
                phase_id: phase_id.to_string(),
                uploader: self.user_id(),
                content_hash: imported.content_hash,
                version,
                note: note.trim().to_string(),
            },
        );
        let chat_op = self.make_op(
            group_id,
            GroupOpPayload::ChatPost {
                msg_id: uuid::Uuid::new_v4().to_string(),
                body: imported.name,
                kind: state::CHAT_KIND_FILE.to_string(),
                doc_id: doc_id.clone(),
                ts_ms: now_ms_u64() as i64,
            },
        );
        self.store.apply_op(&doc_op)?;
        self.store.apply_op(&chat_op)?;
        let chat_view = self.chat_view_from_op(&chat_op);
        self.spawn_gossip(group_id.to_string(), vec![doc_op, chat_op]);
        self.emit_groups();
        self.emit_group_changed(group_id);
        if let Some(view) = chat_view {
            self.emit_message(group_id, &view);
        }
        Ok(doc_id)
    }

    pub fn delete_document(self: &Arc<Self>, group_id: &str, doc_id: &str) -> Result<()> {
        let role = self.require_contribute(group_id)?;
        let doc = self
            .store
            .document(group_id, doc_id)
            .ok_or_else(|| anyhow!("document not found"))?;
        if doc.uploader != self.identity.user_id() && !role.can_manage() {
            bail!("only the uploader or a manager can delete this document");
        }
        let op = self.make_op(
            group_id,
            GroupOpPayload::DocRemove {
                doc_id: doc_id.to_string(),
            },
        );
        self.store.apply_op(&op)?;
        self.docs.remove_doc(group_id, doc_id);
        self.spawn_gossip(group_id.to_string(), vec![op]);
        self.emit_groups();
        self.emit_group_changed(group_id);
        Ok(())
    }

    pub fn request_download(self: &Arc<Self>, group_id: &str, doc_id: &str) -> Result<bool> {
        self.require_member(group_id)?;
        let doc = self
            .store
            .document(group_id, doc_id)
            .ok_or_else(|| anyhow!("document not found"))?;
        if doc.removed {
            bail!("document has been removed");
        }
        if self.docs.is_available(group_id, doc_id, &doc.name) {
            self.emit_group_changed(group_id);
            return Ok(true);
        }
        let online = self.online_set();
        let mut targets: Vec<String> = Vec::new();
        if doc.uploader != self.identity.user_id() && online.contains(&doc.uploader) {
            targets.push(doc.uploader.clone());
        }
        if targets.is_empty() {
            for member in self.store.active_member_ids(group_id) {
                if member != self.identity.user_id() && online.contains(&member) {
                    targets.push(member);
                }
            }
        }
        if targets.is_empty() {
            bail!("no online member currently holds this document");
        }
        let Some(transport) = self.transport() else {
            bail!("lan transport is not active");
        };
        let group = group_id.to_string();
        let doc_id = doc_id.to_string();
        tokio::spawn(async move {
            let msg = ControlMessage::GroupDocRequest {
                group_id: group,
                doc_id,
            };
            for target in targets {
                let _ = transport.send_control_message(&target, &msg).await;
            }
        });
        Ok(false)
    }

    pub async fn save_document(
        &self,
        group_id: &str,
        doc_id: &str,
        dest: &str,
    ) -> Result<String> {
        self.require_member(group_id)?;
        let doc = self
            .store
            .document(group_id, doc_id)
            .ok_or_else(|| anyhow!("document not found"))?;
        let source = self.docs.content_path(group_id, doc_id, &doc.name);
        if !source.exists() {
            bail!("document is not available locally, download it first");
        }
        let dest_root = PathBuf::from(shellexpand::tilde(dest).to_string());
        tokio::fs::create_dir_all(&dest_root).await?;
        let target = dest_root.join(crate::lan::group::document::file_name_for(&doc.name));
        let src = source.clone();
        let dst = target.clone();
        tokio::task::spawn_blocking(move || crate::lan::group::document::copy_into(&src, &dst))
            .await
            .map_err(|e| anyhow!("save task failed: {e}"))??;
        Ok(target.to_string_lossy().to_string())
    }

    pub fn document_content(
        &self,
        group_id: &str,
        doc_id: &str,
    ) -> Result<Option<(Vec<u8>, String, String)>> {
        self.require_member(group_id)?;
        let doc = self
            .store
            .document(group_id, doc_id)
            .ok_or_else(|| anyhow!("document not found"))?;
        if doc.removed {
            bail!("document has been removed");
        }
        if doc.is_dir {
            return Ok(None);
        }
        let path = self.docs.content_path(group_id, doc_id, &doc.name);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path)
            .with_context(|| format!("reading document {}", path.display()))?;
        let mime = crate::lan::guess_mime(&doc.name);
        Ok(Some((bytes, mime, doc.name)))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_task(
        self: &Arc<Self>,
        group_id: &str,
        task_id: Option<String>,
        payload: TaskInput,
    ) -> Result<String> {
        self.require_contribute(group_id)?;
        let title = payload.title.trim();
        if title.is_empty() {
            bail!("task title must not be empty");
        }
        let task_id = task_id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let op = self.make_op(
            group_id,
            GroupOpPayload::TaskUpsert {
                task_id: task_id.clone(),
                title: title.to_string(),
                description: payload.description.trim().to_string(),
                phase_id: payload.phase_id,
                assignee: payload.assignee,
                status: state::normalize_task_status(&payload.status).to_string(),
                priority: state::normalize_task_priority(&payload.priority).to_string(),
                due_ms: payload.due_ms,
                deps: payload.deps,
                parent: payload.parent,
                kind: state::normalize_task_kind(&payload.kind).to_string(),
                progress: payload.progress.clamp(0, 100),
            },
        );
        self.commit(group_id, vec![op])?;
        Ok(task_id)
    }

    pub fn remove_task(self: &Arc<Self>, group_id: &str, task_id: &str) -> Result<()> {
        self.require_contribute(group_id)?;
        let op = self.make_op(
            group_id,
            GroupOpPayload::TaskRemove {
                task_id: task_id.to_string(),
            },
        );
        self.commit(group_id, vec![op])?;
        Ok(())
    }

    pub fn post_message(self: &Arc<Self>, group_id: &str, body: &str) -> Result<String> {
        self.require_contribute(group_id)?;
        let body = body.trim();
        if body.is_empty() {
            bail!("message must not be empty");
        }
        let msg_id = uuid::Uuid::new_v4().to_string();
        let op = self.make_op(
            group_id,
            GroupOpPayload::ChatPost {
                msg_id: msg_id.clone(),
                body: body.to_string(),
                kind: state::CHAT_KIND_TEXT.to_string(),
                doc_id: String::new(),
                ts_ms: now_ms_u64() as i64,
            },
        );
        self.store.apply_op(&op)?;
        let view = self.chat_view_from_op(&op);
        self.spawn_gossip(group_id.to_string(), vec![op]);
        self.emit_groups();
        if let Some(view) = view {
            self.emit_message(group_id, &view);
        }
        Ok(msg_id)
    }

    // ---- inbound handling -------------------------------------------------

    pub async fn handle_inbound(self: &Arc<Self>, peer_id: &str, msg: GroupInbound) {
        match msg {
            GroupInbound::Gossip { ops, .. } => {
                self.ingest_and_emit(&ops);
            }
            GroupInbound::SyncResponse { groups } => {
                let mut all = Vec::new();
                for entry in groups {
                    all.extend(entry.ops);
                }
                self.ingest_and_emit(&all);
            }
            GroupInbound::Invite { ops, .. } => {
                self.ingest_and_emit(&ops);
            }
            GroupInbound::SyncRequest { groups } => {
                if let Some(response) =
                    sync::sync_response_for_request(&self.store, peer_id, &groups)
                {
                    if let Some(transport) = self.transport() {
                        let _ = transport.send_control_message(peer_id, &response).await;
                    }
                }
            }
            GroupInbound::DocRequest { group_id, doc_id } => {
                self.handle_doc_request(peer_id, &group_id, &doc_id).await;
            }
        }
    }

    pub async fn handle_peer_connected(self: &Arc<Self>, peer_id: &str) {
        let request = sync::sync_request(&self.store);
        if let Some(transport) = self.transport() {
            let _ = transport.send_control_message(peer_id, &request).await;
        }
    }

    pub fn on_peers_changed(self: &Arc<Self>) {
        let Some(transport) = self.transport() else {
            return;
        };
        let request = sync::sync_request(&self.store);
        for peer_id in self.online_set() {
            let transport = Arc::clone(&transport);
            let request = request.clone();
            tokio::spawn(async move {
                let _ = transport.send_control_message(&peer_id, &request).await;
            });
        }
    }

    pub async fn handle_doc_received(self: &Arc<Self>, info: GroupDocReceived) {
        let Some(doc) = self.store.document(&info.group_id, &info.doc_id) else {
            return;
        };
        if self
            .docs
            .place_received(&info.group_id, &info.doc_id, &doc.name, &info.path)
            .await
            .is_ok()
        {
            self.emit_group_changed(&info.group_id);
        }
    }

    async fn handle_doc_request(self: &Arc<Self>, peer_id: &str, group_id: &str, doc_id: &str) {
        let Some(doc) = self.store.document(group_id, doc_id) else {
            return;
        };
        if !self.docs.is_available(group_id, doc_id, &doc.name) {
            return;
        }
        let Some(transport) = self.transport() else {
            return;
        };
        let path = self.docs.content_path(group_id, doc_id, &doc.name);
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let _ = transport
            .send_group_doc(peer_id, &transfer_id, group_id, doc_id, &path)
            .await;
    }

    fn ingest_and_emit(self: &Arc<Self>, ops: &[GroupOp]) {
        let mut changed: HashSet<String> = HashSet::new();
        let mut chats: Vec<(String, GroupMessageView)> = Vec::new();
        for op in ops {
            self.clock.observe(op.hlc);
            if matches!(self.store.apply_op(op), Ok(true)) {
                changed.insert(op.group_id.clone());
                if op.author != self.identity.user_id() {
                    if let Some(view) = self.chat_view_from_op(op) {
                        chats.push((op.group_id.clone(), view));
                    }
                }
            }
        }
        if changed.is_empty() {
            return;
        }
        self.emit_groups();
        for group_id in &changed {
            self.emit_group_changed(group_id);
        }
        for (group_id, view) in &chats {
            self.emit_message(group_id, view);
        }
        if !chats.is_empty() {
            self.emit_unread();
        }
    }

    fn chat_view_from_op(&self, op: &GroupOp) -> Option<GroupMessageView> {
        if let GroupOpPayload::ChatPost {
            msg_id,
            body,
            kind,
            doc_id,
            ts_ms,
        } = &op.payload
        {
            Some(GroupMessageView {
                id: msg_id.clone(),
                author: op.author.clone(),
                author_nickname: self.nick_for(&op.author),
                body: body.clone(),
                kind: state::normalize_chat_kind(kind).to_string(),
                doc_id: doc_id.clone(),
                ts_ms: *ts_ms,
            })
        } else {
            None
        }
    }

    fn commit(self: &Arc<Self>, group_id: &str, ops: Vec<GroupOp>) -> Result<()> {
        for op in &ops {
            self.store.apply_op(op)?;
        }
        self.spawn_gossip(group_id.to_string(), ops);
        self.emit_groups();
        self.emit_group_changed(group_id);
        Ok(())
    }

    fn spawn_gossip(self: &Arc<Self>, group_id: String, ops: Vec<GroupOp>) {
        let Some(transport) = self.transport() else {
            return;
        };
        let members = self.store.active_member_ids(&group_id);
        let online = self.online_set();
        let me = self.user_id();
        tokio::spawn(async move {
            let msg = ControlMessage::GroupGossip { group_id, ops };
            for member in members {
                if member == me || !online.contains(&member) {
                    continue;
                }
                let _ = transport.send_control_message(&member, &msg).await;
            }
        });
    }

    fn send_invite(self: &Arc<Self>, group_id: String, invitee: String, ops: Vec<GroupOp>) {
        let Some(transport) = self.transport() else {
            return;
        };
        if !self.online_set().contains(&invitee) {
            return;
        }
        tokio::spawn(async move {
            let msg = ControlMessage::GroupInvite { group_id, ops };
            let _ = transport.send_control_message(&invitee, &msg).await;
        });
    }

    // ---- events -----------------------------------------------------------

    fn emit_groups(&self) {
        emit_group("lan_groups", json!({ "groups": self.store.list_groups() }));
    }

    fn emit_group_changed(&self, group_id: &str) {
        emit_group("lan_group_changed", json!({ "groupId": group_id }));
    }

    fn emit_message(&self, group_id: &str, view: &GroupMessageView) {
        emit_group(
            "lan_group_message",
            json!({ "groupId": group_id, "message": view }),
        );
    }

    fn emit_unread(&self) {
        emit_group(
            "lan_group_unread",
            json!({ "unread": self.store.unread_total() }),
        );
    }
}

pub struct TaskInput {
    pub title: String,
    pub description: String,
    pub phase_id: String,
    pub assignee: String,
    pub status: String,
    pub priority: String,
    pub due_ms: i64,
    pub deps: Vec<String>,
    pub parent: String,
    pub kind: String,
    pub progress: i64,
}

fn emit_group(kind: &str, data: serde_json::Value) {
    crate::gateway::emit_gateway_event(json!({
        "type": "lan_event",
        "kind": kind,
        "data": data,
    }));
}
