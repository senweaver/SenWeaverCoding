// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState } from 'react'
import type { CronTask } from '../../types/task'
import { TaskRow } from './TaskRow'
import { useTranslation } from '../../i18n'

type Props = {
  tasks: CronTask[]
}

export function TaskList({ tasks }: Props) {
  const t = useTranslation()
  const enabledCount = tasks.filter((task) => task.enabled).length
  const [expandedLogsId, setExpandedLogsId] = useState<string | null>(null)

  return (
    <div>
      <div className="mb-4 grid grid-cols-3 gap-3">
        <StatCard label={t('tasks.totalTasks')} value={String(tasks.length)} />
        <StatCard label={t('tasks.active')} value={String(enabledCount)} />
        <StatCard label={t('tasks.disabled')} value={String(tasks.length - enabledCount)} />
      </div>

      <div className="overflow-visible rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] divide-y divide-[var(--color-border)]">
        {tasks.map((task) => (
          <TaskRow
            key={task.id}
            task={task}
            showLogs={expandedLogsId === task.id}
            onToggleLogs={() => setExpandedLogsId(expandedLogsId === task.id ? null : task.id)}
          />
        ))}
      </div>
    </div>
  )
}

function StatCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2">
      <div className="text-xs font-semibold text-[var(--color-text-primary)]">{value}</div>
      <div className="text-xs text-[var(--color-text-tertiary)]">{label}</div>
    </div>
  )
}
