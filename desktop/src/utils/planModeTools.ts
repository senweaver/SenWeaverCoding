
export const PLAN_MODE_ALLOWED_TOOLS: ReadonlySet<string> = new Set([

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

export function isPlanModeAllowedTool(name: string): boolean {
  return PLAN_MODE_ALLOWED_TOOLS.has(name)
}
