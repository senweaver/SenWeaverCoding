// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { cliTasksApi } from '../api/cliTasks'
import type { CLITask, TaskStatus } from '../types/cliTask'

type TodoItem = {
  id?: string
  content: string
  status: string
  activeForm?: string
}

type CLITaskStore = {
  tasksBySessionId: Record<string, CLITask[]>
  resettingBySession: Record<string, boolean>
  expandedBySession: Record<string, boolean>
  completedAndDismissedBySession: Record<string, boolean>
  dismissedCompletionKeyBySession: Record<string, string | null>

  fetchSessionTasks: (sessionId: string) => Promise<void>
  refreshTasks: (sessionId: string) => Promise<void>
  setTasksFromTodos: (todos: TodoItem[], sessionId: string) => void
  markCompletedAndDismissed: (sessionId: string) => void
  resetCompletedTasks: (sessionId: string) => Promise<void>
  clearTasks: (sessionId: string) => void
  toggleExpanded: (sessionId: string) => void
  finalizeTasksOnTurnEnd: (sessionId: string) => void
}

function buildCompletedTaskKey(tasks: CLITask[]): string | null {
  if (tasks.length === 0 || tasks.some((task) => task.status !== 'completed')) return null

  return tasks
    .map((task) => [
      task.taskListId,
      task.id,
      task.subject,
      task.status,
      task.activeForm ?? '',
      task.owner ?? '',
    ].join('::'))
    .join('|')
}

function resolveDismissState(
  tasks: CLITask[],
  dismissedCompletionKey: string | null,
): { completedAndDismissed: boolean; dismissedCompletionKey: string | null } {
  const completionKey = buildCompletedTaskKey(tasks)
  const keepDismissed = completionKey !== null && completionKey === dismissedCompletionKey

  return {
    completedAndDismissed: keepDismissed,
    dismissedCompletionKey: keepDismissed ? completionKey : null,
  }
}

function mapTodosToTasks(todos: TodoItem[], sessionId: string): CLITask[] {
  return todos.map((todo, index) => ({
    id: todo.id?.trim() ? todo.id.trim() : String(index + 1),
    subject: todo.content,
    description: '',
    activeForm: todo.activeForm,
    status: (['pending', 'in_progress', 'completed'].includes(todo.status)
      ? todo.status
      : 'pending') as TaskStatus,
    blocks: [],
    blockedBy: [],
    taskListId: sessionId,
  }))
}

export const useCLITaskStore = create<CLITaskStore>((set, get) => ({
  tasksBySessionId: {},
  resettingBySession: {},
  expandedBySession: {},
  completedAndDismissedBySession: {},
  dismissedCompletionKeyBySession: {},

  fetchSessionTasks: async (sessionId) => {
    if (!sessionId) return
    try {
      const { tasks } = await cliTasksApi.getTasksForList(sessionId)
      if (get().resettingBySession[sessionId]) return
      const filtered = tasks.filter((task) => {
        const owner = (task as { taskListId?: string }).taskListId
        return !owner || owner === sessionId
      })
      const normalized = filtered.map((task) => ({
        ...task,
        taskListId: sessionId,
      }))
      set((state) => {
        const prevKey = state.dismissedCompletionKeyBySession[sessionId] ?? null
        const { completedAndDismissed, dismissedCompletionKey } = resolveDismissState(
          normalized,
          prevKey,
        )
        return {
          tasksBySessionId: { ...state.tasksBySessionId, [sessionId]: normalized },
          completedAndDismissedBySession: {
            ...state.completedAndDismissedBySession,
            [sessionId]: completedAndDismissed,
          },
          dismissedCompletionKeyBySession: {
            ...state.dismissedCompletionKeyBySession,
            [sessionId]: dismissedCompletionKey,
          },
        }
      })
    } catch {
      if (get().resettingBySession[sessionId]) return
      set((state) => ({
        tasksBySessionId: { ...state.tasksBySessionId, [sessionId]: [] },
        completedAndDismissedBySession: {
          ...state.completedAndDismissedBySession,
          [sessionId]: false,
        },
        dismissedCompletionKeyBySession: {
          ...state.dismissedCompletionKeyBySession,
          [sessionId]: null,
        },
        expandedBySession: {
          ...state.expandedBySession,
          [sessionId]: false,
        },
      }))
    }
  },

  refreshTasks: async (sessionId) => {
    if (!sessionId) return
    try {
      const { tasks } = await cliTasksApi.getTasksForList(sessionId)
      if (get().resettingBySession[sessionId]) return
      const filtered = tasks.filter((task) => {
        const owner = (task as { taskListId?: string }).taskListId
        return !owner || owner === sessionId
      })
      const normalized = filtered.map((task) => ({
        ...task,
        taskListId: sessionId,
      }))
      set((state) => {
        const prevKey = state.dismissedCompletionKeyBySession[sessionId] ?? null
        const { completedAndDismissed, dismissedCompletionKey } = resolveDismissState(
          normalized,
          prevKey,
        )
        return {
          tasksBySessionId: { ...state.tasksBySessionId, [sessionId]: normalized },
          completedAndDismissedBySession: {
            ...state.completedAndDismissedBySession,
            [sessionId]: completedAndDismissed,
          },
          dismissedCompletionKeyBySession: {
            ...state.dismissedCompletionKeyBySession,
            [sessionId]: dismissedCompletionKey,
          },
        }
      })
    } catch {

    }
  },

  setTasksFromTodos: (todos, sessionId) => {
    if (!sessionId) return
    const tasks = mapTodosToTasks(todos, sessionId)
    set((state) => {
      const prevKey = state.dismissedCompletionKeyBySession[sessionId] ?? null
      const { completedAndDismissed, dismissedCompletionKey } = resolveDismissState(
        tasks,
        prevKey,
      )
      return {
        tasksBySessionId: { ...state.tasksBySessionId, [sessionId]: tasks },
        completedAndDismissedBySession: {
          ...state.completedAndDismissedBySession,
          [sessionId]: completedAndDismissed,
        },
        dismissedCompletionKeyBySession: {
          ...state.dismissedCompletionKeyBySession,
          [sessionId]: dismissedCompletionKey,
        },
      }
    })
  },

  markCompletedAndDismissed: (sessionId) => {
    const tasks = get().tasksBySessionId[sessionId] ?? []
    const completionKey = buildCompletedTaskKey(tasks)
    if (!completionKey) return
    set((state) => ({
      completedAndDismissedBySession: {
        ...state.completedAndDismissedBySession,
        [sessionId]: true,
      },
      dismissedCompletionKeyBySession: {
        ...state.dismissedCompletionKeyBySession,
        [sessionId]: completionKey,
      },
      expandedBySession: { ...state.expandedBySession, [sessionId]: false },
    }))
  },

  resetCompletedTasks: async (sessionId) => {
    const tasks = get().tasksBySessionId[sessionId] ?? []
    const completionKey = buildCompletedTaskKey(tasks)
    if (!sessionId || !completionKey) return
    set((state) => ({
      tasksBySessionId: { ...state.tasksBySessionId, [sessionId]: [] },
      resettingBySession: { ...state.resettingBySession, [sessionId]: true },
      completedAndDismissedBySession: {
        ...state.completedAndDismissedBySession,
        [sessionId]: false,
      },
      dismissedCompletionKeyBySession: {
        ...state.dismissedCompletionKeyBySession,
        [sessionId]: null,
      },
      expandedBySession: { ...state.expandedBySession, [sessionId]: false },
    }))
    try {
      await cliTasksApi.resetTaskList(sessionId)
    } finally {
      set((state) => ({
        resettingBySession: { ...state.resettingBySession, [sessionId]: false },
      }))
    }
  },

  clearTasks: (sessionId) => {
    if (!sessionId) return
    set((state) => {
      const nextTasks = { ...state.tasksBySessionId }
      delete nextTasks[sessionId]
      const nextResetting = { ...state.resettingBySession }
      delete nextResetting[sessionId]
      const nextExpanded = { ...state.expandedBySession }
      delete nextExpanded[sessionId]
      const nextDismissed = { ...state.completedAndDismissedBySession }
      delete nextDismissed[sessionId]
      const nextKey = { ...state.dismissedCompletionKeyBySession }
      delete nextKey[sessionId]
      return {
        tasksBySessionId: nextTasks,
        resettingBySession: nextResetting,
        expandedBySession: nextExpanded,
        completedAndDismissedBySession: nextDismissed,
        dismissedCompletionKeyBySession: nextKey,
      }
    })
  },

  toggleExpanded: (sessionId) => {
    if (!sessionId) return
    set((state) => ({
      expandedBySession: {
        ...state.expandedBySession,
        [sessionId]: !(state.expandedBySession[sessionId] ?? false),
      },
    }))
  },

  finalizeTasksOnTurnEnd: (sessionId) => {
    if (!sessionId) return
    set((state) => {
      const tasks = state.tasksBySessionId[sessionId] ?? []
      if (tasks.length === 0) return state

      const incomplete = tasks.filter((task) => task.status !== 'completed')

      let nextTasks = tasks
      if (incomplete.length === 1 && tasks.length > 1) {
        nextTasks = tasks.map((task) =>
          task.status === 'pending' || task.status === 'in_progress'
            ? { ...task, status: 'completed' as TaskStatus }
            : task,
        )
      }

      const completionKey = buildCompletedTaskKey(nextTasks)
      if (!completionKey) return { ...state, tasksBySessionId: { ...state.tasksBySessionId, [sessionId]: nextTasks } }

      return {
        tasksBySessionId: { ...state.tasksBySessionId, [sessionId]: nextTasks },
        completedAndDismissedBySession: {
          ...state.completedAndDismissedBySession,
          [sessionId]: true,
        },
        dismissedCompletionKeyBySession: {
          ...state.dismissedCompletionKeyBySession,
          [sessionId]: completionKey,
        },
        expandedBySession: { ...state.expandedBySession, [sessionId]: false },
      }
    })
  },
}))
