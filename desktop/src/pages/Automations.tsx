// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { useTaskStore } from '../stores/taskStore'
import { useUIStore } from '../stores/uiStore'
import { useTranslation } from '../i18n'
import { Button } from '../components/shared/Button'
import { TaskList } from '../components/tasks/TaskList'
import { TaskEmptyState } from '../components/tasks/TaskEmptyState'
import { NewTaskModal } from '../components/tasks/NewTaskModal'

export function Automations() {
  const tasks = useTaskStore((s) => s.tasks)
  const fetchTasks = useTaskStore((s) => s.fetchTasks)
  const isLoading = useTaskStore((s) => s.isLoading)
  const activeModal = useUIStore((s) => s.activeModal)
  const openModal = useUIStore((s) => s.openModal)
  const closeModal = useUIStore((s) => s.closeModal)
  const t = useTranslation()
  const [initialized, setInitialized] = useState(false)

  useEffect(() => {
    fetchTasks().then(() => setInitialized(true))
  }, [fetchTasks])

  return (
    <div className="flex-1 overflow-y-auto">
      <div className="px-6 py-4">
        <div className="mb-4 flex items-center justify-between">
          <div>
            <h1 className="text-xs font-semibold text-[var(--color-text-primary)]">{t('automations.page.title')}</h1>
            <p className="mt-0.5 text-xs text-[var(--color-text-tertiary)]">
              {t('automations.page.subtitle')}
            </p>
          </div>
          <Button size="sm" onClick={() => openModal('new-task')}>{t('tasks.newTask')}</Button>
        </div>

        <div className="mb-4 flex items-center gap-2 rounded-xl border border-[var(--color-warning)]/15 bg-[var(--color-warning)]/8 px-3 py-2">
          <span className="material-symbols-outlined text-[16px] text-[var(--color-warning)]">schedule</span>
          <span className="text-xs text-[var(--color-text-tertiary)]">
            {t('automations.page.desktopNotice')}
          </span>
        </div>

        {!initialized && isLoading ? (
          <div className="flex items-center justify-center py-16">
            <div className="animate-spin w-6 h-6 border-2 border-[var(--color-brand)] border-t-transparent rounded-full" />
          </div>
        ) : tasks.length === 0 ? (
          <TaskEmptyState onCreateTask={() => openModal('new-task')} />
        ) : (
          <TaskList tasks={tasks} />
        )}
      </div>

      {activeModal === 'new-task' && (
        <NewTaskModal
          open
          onClose={closeModal}
        />
      )}
    </div>
  )
}
