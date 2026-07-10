// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useMemo, useState } from 'react'
import { useTranslation } from '../../i18n'
import { confirmDialog } from '../../lib/dialogs'
import { useLanStore } from '../../stores/lanStore'
import { useLanGroupStore } from '../../stores/lanGroupStore'
import type { LanGroupRole, LanGroupSnapshot } from '../../types/lanGroup'
import { canManage, initials, ROLE_LABEL } from './shared'

const ASSIGNABLE_ROLES: LanGroupRole[] = ['manager', 'member', 'viewer']

export function GroupMembers({
  groupId,
  snapshot,
}: {
  groupId: string
  snapshot: LanGroupSnapshot
}) {
  const t = useTranslation()
  const selfId = useLanStore((s) => s.identity?.userId ?? '')
  const peers = useLanStore((s) => s.peers)
  const invite = useLanGroupStore((s) => s.invite)
  const setRole = useLanGroupStore((s) => s.setRole)
  const removeMember = useLanGroupStore((s) => s.removeMember)
  const leaveGroup = useLanGroupStore((s) => s.leaveGroup)

  const manager = canManage(snapshot.group.role)
  const [inviteRole, setInviteRole] = useState<LanGroupRole>('member')

  const invitable = useMemo(() => {
    const memberIds = new Set(snapshot.members.map((m) => m.userId))
    return peers.filter((p) => !memberIds.has(p.userId))
  }, [peers, snapshot.members])

  return (
    <div className="flex h-full flex-col overflow-y-auto p-3">
      <div className="space-y-1.5">
        {snapshot.members.map((member) => {
          const isSelf = member.userId === selfId
          const isOwner = member.role === 'owner'
          return (
            <div
              key={member.userId}
              className="flex items-center gap-2.5 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-hover)] p-2"
            >
              <div className="relative flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-[var(--color-surface-selected)] text-xs font-semibold text-[var(--color-text-primary)]">
                {initials(member.nickname)}
                {member.online && (
                  <span className="absolute -bottom-0.5 -right-0.5 h-2.5 w-2.5 rounded-full border-2 border-[var(--color-surface)] bg-[var(--color-success,#16a34a)]" />
                )}
              </div>
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-1 truncate text-sm font-medium text-[var(--color-text-primary)]">
                  {member.nickname}
                  {isSelf && (
                    <span className="text-[10px] text-[var(--color-text-tertiary)]">
                      ({t('lanGroup.you')})
                    </span>
                  )}
                </div>
                <div className="text-[10px] text-[var(--color-text-tertiary)]">
                  {t(ROLE_LABEL[member.role])}
                  {!member.online && ` · ${t('lanGroup.offline')}`}
                </div>
              </div>
              {manager && !isOwner && !isSelf && (
                <div className="flex items-center gap-1">
                  <select
                    value={member.role}
                    onChange={(e) => void setRole(groupId, member.userId, e.target.value as LanGroupRole)}
                    className="h-7 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-1.5 text-[11px] text-[var(--color-text-primary)]"
                  >
                    {ASSIGNABLE_ROLES.map((r) => (
                      <option key={r} value={r}>
                        {t(ROLE_LABEL[r])}
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    title={t('lanGroup.remove')}
                    onClick={() => void removeMember(groupId, member.userId)}
                    className="inline-flex h-7 w-7 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-selected)] hover:text-[var(--color-error)]"
                  >
                    <span className="material-symbols-outlined text-[15px]">person_remove</span>
                  </button>
                </div>
              )}
            </div>
          )
        })}
      </div>

      {manager && (
        <div className="mt-4">
          <div className="mb-1.5 text-[10px] font-bold uppercase tracking-widest text-[var(--color-text-tertiary)]">
            {t('lanGroup.invite')}
          </div>
          {invitable.length === 0 ? (
            <div className="py-2 text-center text-xs text-[var(--color-text-tertiary)]">
              {t('lanGroup.noOnlinePeers')}
            </div>
          ) : (
            <div className="space-y-1.5">
              <select
                value={inviteRole}
                onChange={(e) => setInviteRole(e.target.value as LanGroupRole)}
                className="h-8 w-full rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-2 text-xs text-[var(--color-text-primary)]"
              >
                {ASSIGNABLE_ROLES.map((r) => (
                  <option key={r} value={r}>
                    {t(ROLE_LABEL[r])}
                  </option>
                ))}
              </select>
              {invitable.map((peer) => (
                <div
                  key={peer.userId}
                  className="flex items-center gap-2 rounded-lg border border-[var(--color-border)] p-2"
                >
                  <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-[var(--color-surface-selected)] text-[11px] font-semibold text-[var(--color-text-primary)]">
                    {initials(peer.nickname)}
                  </div>
                  <span className="min-w-0 flex-1 truncate text-sm text-[var(--color-text-primary)]">
                    {peer.nickname}
                  </span>
                  <button
                    type="button"
                    onClick={() => void invite(groupId, peer.userId, inviteRole)}
                    className="rounded-md bg-[var(--color-brand)] px-2 py-0.5 text-[11px] font-semibold text-white hover:opacity-90"
                  >
                    {t('lanGroup.invite')}
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      <div className="mt-4 border-t border-[var(--color-border)] pt-3">
        <button
          type="button"
          onClick={() => {
            void (async () => {
              if (await confirmDialog(t('lanGroup.leaveConfirm'))) {
                await leaveGroup(groupId)
              }
            })()
          }}
          className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs text-[var(--color-error)] hover:bg-[var(--color-surface-hover)]"
        >
          <span className="material-symbols-outlined text-[15px]">logout</span>
          {t('lanGroup.leave')}
        </button>
      </div>
    </div>
  )
}
