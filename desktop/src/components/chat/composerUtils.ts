// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { SettingsTab } from '../../stores/uiStore'
import type { TranslationKey } from '../../i18n'

export const PANEL_SLASH_COMMANDS = [
  { name: 'mcp', descriptionKey: 'slash.mcp.desc' },
  { name: 'skills', descriptionKey: 'slash.skills.desc' },
] as const satisfies ReadonlyArray<{ name: string; descriptionKey: TranslationKey }>

export const SETTINGS_SLASH_COMMANDS: ReadonlyArray<{
  name: string
  descriptionKey: TranslationKey
  tab: SettingsTab
}> = [
  { name: 'plugins', descriptionKey: 'slash.plugins.desc', tab: 'plugins' },
]

// Only commands that actually execute on this surface: local UI panels plus
// backend registry commands runnable over the desktop WS (no TTY). The
// authoritative list comes from the slash-commands API; this is the offline
// fallback and must never advertise commands the backend cannot run.
const FALLBACK_SLASH_COMMAND_KEYS: ReadonlyArray<{
  name: string
  descriptionKey: TranslationKey
}> = [
  ...PANEL_SLASH_COMMANDS.map(({ name, descriptionKey }) => ({ name, descriptionKey })),
  ...SETTINGS_SLASH_COMMANDS.map(({ name, descriptionKey }) => ({ name, descriptionKey })),
  { name: 'compact', descriptionKey: 'slash.compact.desc' },
  { name: 'clear', descriptionKey: 'slash.clear.desc' },
  { name: 'help', descriptionKey: 'slash.help.desc' },
  { name: 'review', descriptionKey: 'slash.review.desc' },
  { name: 'config', descriptionKey: 'slash.config.desc' },
  { name: 'cost', descriptionKey: 'slash.cost.desc' },
  { name: 'doctor', descriptionKey: 'slash.doctor.desc' },
  { name: 'memory', descriptionKey: 'slash.memory.desc' },
  { name: 'model', descriptionKey: 'slash.model.desc' },
  { name: 'permissions', descriptionKey: 'slash.permissions.desc' },
  { name: 'status', descriptionKey: 'slash.status.desc' },
]

export function localizedFallbackSlashCommands(
  t: (key: TranslationKey) => string,
): SlashCommandOption[] {
  return FALLBACK_SLASH_COMMAND_KEYS.map(({ name, descriptionKey }) => ({
    name,
    description: t(descriptionKey),
  }))
}

export type SlashCommandOption = {
  name: string
  description: string
}

export type SlashUiAction =
  | {
      type: 'panel'
      command: typeof PANEL_SLASH_COMMANDS[number]['name']
    }
  | {
      type: 'settings'
      tab: SettingsTab
    }

export function resolveSlashUiAction(value: string): SlashUiAction | null {
  const panelCommand = PANEL_SLASH_COMMANDS.find((command) => command.name === value)
  if (panelCommand) {
    return { type: 'panel', command: panelCommand.name }
  }

  const settingsCommand = SETTINGS_SLASH_COMMANDS.find((command) => command.name === value)
  if (settingsCommand) {
    return { type: 'settings', tab: settingsCommand.tab }
  }

  return null
}

export function mergeSlashCommands(
  preferred: ReadonlyArray<SlashCommandOption>,
  fallback: ReadonlyArray<SlashCommandOption>,
): SlashCommandOption[] {
  const merged = new Map<string, SlashCommandOption>()

  for (const command of preferred) {
    if (!command?.name) continue
    merged.set(command.name, {
      name: command.name,
      description: command.description?.trim() || '',
    })
  }

  for (const command of fallback) {
    if (!command?.name) continue
    const existing = merged.get(command.name)
    if (existing) {
      if (!existing.description && command.description) {
        merged.set(command.name, {
          ...existing,
          description: command.description,
        })
      }
      continue
    }
    merged.set(command.name, command)
  }

  return [...merged.values()]
}

export type SlashTrigger = {
  slashPos: number
  filter: string
}

export function findSlashTrigger(value: string, cursorPos: number): SlashTrigger | null {
  const textBeforeCursor = value.slice(0, cursorPos)
  let slashPos = -1

  for (let i = textBeforeCursor.length - 1; i >= 0; i--) {
    const ch = textBeforeCursor[i]!
    if (ch === '/') {
      if (i === 0 || /\s/.test(textBeforeCursor[i - 1]!)) {
        slashPos = i
        break
      }
      break
    }
    if (/\s/.test(ch)) {
      break
    }
  }

  if (slashPos < 0) return null

  const filter = textBeforeCursor.slice(slashPos + 1)
  if (/\s/.test(filter)) return null

  return { slashPos, filter }
}

export function replaceSlashToken(
  input: string,
  cursorPos: number,
  command: string,
  options?: { trailingSpace?: boolean },
): { value: string; cursorPos: number } {
  const trigger = findSlashTrigger(input, cursorPos)
  if (!trigger) {
    const prefix = input && !/\s$/.test(input) ? `${input} ` : input
    const token = `/${command}`
    const suffix = options?.trailingSpace !== false ? ' ' : ''
    const value = `${prefix}${token}${suffix}`
    return { value, cursorPos: value.length }
  }

  const before = input.slice(0, trigger.slashPos)
  const after = input.slice(cursorPos)
  const token = `/${command}`
  const suffix = options?.trailingSpace !== false ? ' ' : ''
  const value = `${before}${token}${suffix}${after}`
  const nextCursorPos = before.length + token.length + suffix.length
  return { value, cursorPos: nextCursorPos }
}

export type SlashToken = {
  start: number
  filter: string
}

export function findSlashToken(value: string, cursorPos: number): SlashToken | null {
  const trigger = findSlashTrigger(value, cursorPos)
  if (!trigger) return null
  return { start: trigger.slashPos, filter: trigger.filter }
}

export function replaceSlashCommand(
  value: string,
  cursorPos: number,
  command: string,
): { value: string; cursorPos: number } | null {
  const trigger = findSlashTrigger(value, cursorPos)
  if (!trigger) return null

  return replaceSlashToken(value, cursorPos, command, { trailingSpace: true })
}

export function insertSlashTrigger(
  value: string,
  cursorPos: number,
): { value: string; cursorPos: number } {
  const before = value.slice(0, cursorPos)
  const after = value.slice(cursorPos)
  const needsLeadingSpace = before.length > 0 && !/\s$/.test(before)
  const token = `${needsLeadingSpace ? ' ' : ''}/`
  return {
    value: `${before}${token}${after}`,
    cursorPos: before.length + token.length,
  }
}
