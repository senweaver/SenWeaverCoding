// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.



export const previewSessions = {
  today: [
    { id: 's1', title: 'Refactor login flow', modifiedAt: new Date().toISOString() },
    { id: 's2', title: 'Fix CSS responsive layout', modifiedAt: new Date().toISOString() },
  ],
  previous7Days: [
    { id: 's3', title: 'Add user authentication', modifiedAt: new Date(Date.now() - 3 * 86400000).toISOString() },
    { id: 's4', title: 'Database migration script', modifiedAt: new Date(Date.now() - 5 * 86400000).toISOString() },
  ],
  older: [
    { id: 's5', title: 'Initial project setup', modifiedAt: new Date(Date.now() - 30 * 86400000).toISOString() },
  ],
}

export const previewTeam = {
  name: 'session-dev',
  memberCount: 4,
  members: [
    { id: 'a1', role: 'Architect', status: 'completed' as const, color: '#16a34a' },
    { id: 'a2', role: 'Frontend Dev', status: 'running' as const, color: '#dc2626' },
    { id: 'a3', role: 'Backend Dev', status: 'running' as const, color: '#2563eb' },
    { id: 'a4', role: 'Tester', status: 'idle' as const, color: '#9333ea' },
  ],
}

export const previewTeamMessages = {
  userMessage:
    'Refactor the authentication middleware to support JWT and OAuth2 simultaneously. Ensure we have proper test coverage for the edge cases.',
  assistantMessage:
    "I've initiated the agent team for this task. The architect is designing the interface, while the developers are preparing the boilerplate for the new strategies.",
  systemInfo: `Info: spawning child_processes for parallel development
active: session-dev cluster initiated
ready: 4 agents assigned`,
}

export const previewScheduledTasks = {
  stats: {
    totalTasks: 12,
    activeHealthy: 9,
    nextRun: { name: 'Nightly linting', time: 'Today, 11:30 PM' },
    systemHealth: 99.8,
    healthPeriod: 'Last 30 days execution rate',
  },
  tasks: [
    {
      id: 'task1',
      name: 'Nightly linting',
      frequency: 'Daily',
      lastResult: 'Success' as const,
      nextExecution: 'Today, 11:30 PM',
    },
    {
      id: 'task2',
      name: 'Clean up temp files',
      description: 'Clean TempOutput/**',
      frequency: 'Weekly',
      lastResult: 'Success' as const,
      nextExecution: 'Sun, 2:00 AM',
    },
    {
      id: 'task3',
      name: 'Database Vacuum',
      description: 'Postgres maintenance',
      frequency: 'Monthly',
      lastResult: 'Failed (Disk Full)' as const,
      nextExecution: 'Dec 01, 9:01 AM',
    },
  ],
}

export const previewPermissionModes = [
  { id: 'ask', label: 'Ask permissions', description: 'Confirm every file edit or terminal command.', icon: 'lock' },
  { id: 'auto', label: 'Auto accept edits', description: 'SenWeaverCoding writes to disk without asking.', icon: 'edit_note' },
  { id: 'plan', label: 'Plan mode', description: 'Architecture & reasoning only. No writes.', icon: 'architecture' },
  { id: 'bypass', label: 'Bypass permissions', description: 'Full root access for shell and file system.', icon: 'warning' },
]

export const previewModels = [
  { id: 'opus', name: 'Opus 4.7', active: false },
  { id: 'sonnet', name: 'Sonnet 4.6', active: true },
  { id: 'haiku', name: 'Haiku 4.5', active: false },
]

export const previewEffortLevels = ['Low', 'Medium', 'High', 'Max']

export const previewToolInspection = {
  toolType: 'TOOL CALL',
  toolName: 'edit_file',
  description: 'Updating login logic to use new SDK',
  filePath: 'src/lib/auth.ts',
  dryRunStatus: 'Dry-run Success',
  linesChanged: { added: 12, removed: 8 },
  diffLines: [
    { type: 'context' as const, lineNo: 1, content: 'export async function loginCredentials: LoginCredentials): Promise<LoginResponse> {' },
    { type: 'context' as const, lineNo: 2, content: '  try {' },
    { type: 'removed' as const, lineNo: 3, content: '    const response = await legacyHttpClient.authenticate()' },
    { type: 'added' as const, lineNo: 3, content: '    const response = await httpClient.authenticate({' },
    { type: 'added' as const, lineNo: 4, content: '      user: credentials.username,' },
    { type: 'added' as const, lineNo: 5, content: '      pass: credentials.password,' },
    { type: 'context' as const, lineNo: 6, content: '    })' },
    { type: 'context' as const, lineNo: 7, content: '' },
    { type: 'removed' as const, lineNo: 8, content: '    const client = await createClient();' },
    { type: 'added' as const, lineNo: 8, content: '    const client = await newSdkClient.create({' },
    { type: 'added' as const, lineNo: 9, content: '      identifier: credentials.username,' },
    { type: 'added' as const, lineNo: 10, content: '      secret: credentials.password,' },
    { type: 'context' as const, lineNo: 11, content: '' },
    { type: 'added' as const, lineNo: 12, content: '      options: { persistent: true }' },
    { type: 'context' as const, lineNo: 13, content: '    })' },
    { type: 'context' as const, lineNo: 14, content: '' },
    { type: 'context' as const, lineNo: 15, content: '    if (response.status === 200) {' },
    { type: 'context' as const, lineNo: 16, content: '      return response.data;' },
  ],
}

export const previewNewTaskDefaults = {
  permissionModes: ['Restricted', 'Standard', 'Full Access'],
  models: ['Sonnet 4.6', 'Haiku 4.5', 'Opus 4.7'],
  frequencies: ['Hourly', 'Daily at 9:00 AM', 'Weekly', 'Monthly', 'Custom cron'],
}

export const previewStatusBar = {
  user: 'User Avatar',
  username: 'username',
  plan: 'Pro Plan',
  branch: 'main-branch',
  worktreeToggle: 'worktree-toggle',
  localSwitch: 'local-switch',
  status: 'Ready',
}
