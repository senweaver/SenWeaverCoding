// SPDX-License-Identifier: MIT

export type ToolCategory =
  | 'read'
  | 'list'
  | 'search'
  | 'web'
  | 'edit'
  | 'exec'
  | 'memory_recall'
  | 'memory_other'
  | 'plan'
  | 'tasks'
  | 'task'
  | 'mcp'
  | 'git'
  | 'diagnostics'
  | 'sessions'
  | 'comm'
  | 'flow'
  | 'code_intel'
  | 'model'
  | 'integration'
  | 'ops'
  | 'hardware'
  | 'other'

const STATIC_CATEGORY_MAP: Record<string, ToolCategory> = {

  file_read: 'read',
  pdf_read: 'read',
  view_image: 'read',
  screenshot: 'read',
  read_skill: 'read',
  image_info: 'read',
  now: 'read',
  Read: 'read',
  NotebookRead: 'read',

  dir_list: 'list',
  present_files: 'list',
  glob_search: 'list',
  Glob: 'list',
  LS: 'list',

  content_search: 'search',
  tool_search: 'search',
  discord_search: 'search',
  Grep: 'search',

  code_graph_query: 'code_intel',
  code_search: 'code_intel',
  code_outline: 'code_intel',
  code_to_spec: 'code_intel',
  code_xfile_refactor: 'code_intel',
  code_review: 'code_intel',
  lsp: 'code_intel',
  lsp_symbols: 'code_intel',
  inline_complete: 'code_intel',

  web_search_tool: 'web',
  web_search: 'web',
  web_fetch: 'web',
  tavily_search: 'web',
  exa_search: 'web',
  multi_search: 'web',
  youtube_search: 'web',
  reddit_search: 'web',
  image_search: 'web',
  image_gen: 'web',
  github_search: 'web',
  http_request: 'web',
  text_browser: 'web',
  browser_open: 'web',
  browser: 'web',
  WebSearch: 'web',
  WebFetch: 'web',

  file_write: 'edit',
  file_edit: 'edit',
  multi_edit: 'edit',
  notebook_edit: 'edit',
  glob_edit: 'edit',
  patch_apply: 'edit',
  diff_apply: 'edit',
  lsp_rename: 'edit',
  restore_file: 'edit',
  copy_path: 'edit',
  move_path: 'edit',
  delete_path: 'edit',
  create_directory: 'edit',
  document_convert: 'edit',
  pdf_ops: 'edit',
  presentation_create: 'edit',
  Write: 'edit',
  Edit: 'edit',
  MultiEdit: 'edit',
  NotebookEdit: 'edit',

  shell: 'exec',
  powershell: 'exec',
  browser_delegate: 'exec',
  claude_code: 'exec',
  claude_code_runner: 'exec',
  codex_cli: 'exec',
  gemini_cli: 'exec',
  opencode_cli: 'exec',
  google_workspace: 'exec',
  calculator: 'exec',

  weather: 'exec',
  weather_tool: 'exec',
  Bash: 'exec',

  memory_recall: 'memory_recall',

  knowledge: 'memory_recall',
  knowledge_tool: 'memory_recall',

  memory_store: 'memory_other',
  memory_forget: 'memory_other',
  memory_export: 'memory_other',
  memory_purge: 'memory_other',

  workspace: 'memory_other',
  workspace_tool: 'memory_other',
  project_intel: 'memory_other',

  vi_verify: 'memory_other',
  verifiable_intent: 'memory_other',
  incremental_optimize: 'memory_other',
  structured_output: 'memory_other',

  enter_plan_mode: 'plan',
  exit_plan_mode: 'plan',
  update_plan: 'plan',
  write_plan: 'plan',

  send_user_message: 'plan',
  setup_agent: 'plan',
  ExitPlanMode: 'plan',

  todo_write: 'tasks',
  TodoWrite: 'tasks',

  task_create: 'task',
  task_get: 'task',
  task_update: 'task',
  task_list: 'task',
  task_output: 'task',
  task_stop: 'task',
  delegate: 'task',
  delegate_parallel: 'task',
  spawn_workers: 'task',
  swarm: 'task',
  llm_task: 'task',
  team_create: 'task',
  team_delete: 'task',
  Agent: 'task',
  Task: 'task',
  TaskCreate: 'task',
  TaskUpdate: 'task',
  TaskGet: 'task',
  TaskList: 'task',

  mcp_resources_list: 'mcp',
  mcp_resources_read: 'mcp',
  composio: 'mcp',

  git_operations: 'git',
  worktree_enter: 'git',
  worktree_exit: 'git',

  diagnostics: 'diagnostics',
  debug_test_report: 'diagnostics',

  sessions_list: 'sessions',
  sessions_history: 'sessions',
  sessions_send: 'sessions',

  send_message: 'comm',
  ask_user: 'comm',
  ask_question: 'comm',
  escalate_to_human: 'comm',
  poll: 'comm',
  reaction: 'comm',

  pushover: 'comm',

  flow_run: 'flow',
  flow_rollback: 'flow',
  execute_pipeline: 'flow',
  sleep: 'flow',
  schedule: 'flow',
  cron_add: 'flow',
  cron_list: 'flow',
  cron_remove: 'flow',
  cron_run: 'flow',
  cron_runs: 'flow',
  cron_update: 'flow',
  sop_advance: 'flow',
  sop_approve: 'flow',
  sop_execute: 'flow',
  sop_list: 'flow',
  sop_status: 'flow',

  model_switch: 'model',
  model_routing_config: 'model',
  proxy_config: 'model',

  linkedin: 'integration',
  notion: 'integration',
  jira: 'integration',
  microsoft365: 'integration',
  whatsapp: 'integration',

  backup: 'ops',
  data_management: 'ops',
  security_ops: 'ops',
  cloud_ops: 'ops',
  cloud_patterns: 'ops',
  canvas: 'ops',
  report_template: 'ops',

  gpio_read: 'hardware',
  gpio_write: 'hardware',
  hardware_board_info: 'hardware',
  hardware_memory_map: 'hardware',
  hardware_memory_read: 'hardware',
}

export function getToolCategory(name: string | undefined | null): ToolCategory {
  if (!name) return 'other'
  const exact = STATIC_CATEGORY_MAP[name]
  if (exact) return exact

  const lc = name.toLowerCase()
  for (const key of Object.keys(STATIC_CATEGORY_MAP)) {
    if (key.toLowerCase() === lc) return STATIC_CATEGORY_MAP[key]!
  }
  if (name.includes('__')) return 'mcp'
  if (name.startsWith('mcp_')) return 'mcp'
  if (name.startsWith('node:')) return 'other'
  if (name.startsWith('skill.')) return 'other'
  if (name.startsWith('custom_')) return 'other'
  return 'other'
}

export function getCategoryIcon(category: ToolCategory): string {
  switch (category) {
    case 'read':
      return 'description'
    case 'list':
      return 'folder_open'
    case 'search':
      return 'search'
    case 'web':
      return 'travel_explore'
    case 'edit':
      return 'edit_note'
    case 'exec':
      return 'terminal'
    case 'memory_recall':
      return 'history'
    case 'memory_other':
      return 'inventory_2'
    case 'plan':
      return 'menu_book'
    case 'tasks':
      return 'checklist'
    case 'task':
      return 'smart_toy'
    case 'mcp':
      return 'extension'
    case 'git':
      return 'commit'
    case 'diagnostics':
      return 'health_and_safety'
    case 'sessions':
      return 'forum'
    case 'comm':
      return 'forward_to_inbox'
    case 'flow':
      return 'account_tree'
    case 'code_intel':
      return 'code'
    case 'model':
      return 'tune'
    case 'integration':
      return 'hub'
    case 'ops':
      return 'admin_panel_settings'
    case 'hardware':
      return 'memory'
    case 'other':
    default:
      return 'build'
  }
}

const EXPLORE_CATEGORIES: ReadonlySet<ToolCategory> = new Set([
  'search',
  'memory_recall',
  'code_intel',
])

export function isExploreCategory(category: ToolCategory): boolean {
  return EXPLORE_CATEGORIES.has(category)
}

export function isExploreToolName(name: string | undefined | null): boolean {
  return isExploreCategory(getToolCategory(name))
}
