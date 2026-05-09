import { useSettingsStore } from '../stores/settingsStore'

const FALLBACK_PLAN_MODE_TOOLS: ReadonlySet<string> = new Set([

  'file_read',
  'glob_search',
  'content_search',
  'dir_list',
  'present_files',
  'view_image',
  'image_info',
  'screenshot',
  'memory_recall',
  'memory_export',
  'calculator',
  'weather',
  'web_search',
  'web_fetch',
  'mcp_resources_list',
  'mcp_resources_read',
  'lsp',
  'task_list',
  'task_get',
  'task_output',
  'structured_output',

  'grep',
  'code_search',
  'code_outline',
  'code_graph_query',
  'tool_search',
  'lsp_symbols',
  'pdf_read',
  'multi_search',
  'tavily_search',
  'exa_search',
  'youtube_search',
  'github_search',
  'reddit_search',
  'image_search',
  'discord_search',
  'cron_list',
  'cron_runs',
  'web_search_tool',

  'enter_plan_mode',
  'exit_plan_mode',
  'update_plan',

  'ask_question',
  'ask_user',
  'AskQuestion',

  'read_skill',
  'cloud_patterns',
  'brief',
  'now',
])

export const PLAN_MODE_ALLOWED_TOOLS: ReadonlySet<string> = FALLBACK_PLAN_MODE_TOOLS

function readBackendPlanTools(): readonly string[] | null {
  try {
    const modes = useSettingsStore.getState().codingModes
    const planEntry = modes.find((m) => m.id === 'plan')
    if (planEntry?.allowedTools && planEntry.allowedTools.length > 0) {
      return planEntry.allowedTools
    }
  } catch {

  }
  return null
}

export function isPlanModeAllowedTool(name: string): boolean {
  if (!name) return false
  const fromBackend = readBackendPlanTools()
  if (fromBackend) {
    return fromBackend.includes(name) || (name === 'AskQuestion' && fromBackend.includes('ask_question'))
  }
  return FALLBACK_PLAN_MODE_TOOLS.has(name)
}
