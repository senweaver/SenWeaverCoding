export type RuntimeSelection = {
  providerId: string | null
  modelId: string
}

export type RuntimeTaskGroup = {
  name: string
  count: number
  oldestAgeMs: number
}

export type RuntimeGatewayInfo = {
  host: string
  port: number
  url: string
  pathPrefix: string
}

export type RuntimeTasksInfo = {
  liveCount: number
  groups: RuntimeTaskGroup[]
}

export type RuntimeSnapshot = {
  version: string
  buildProfile: string
  pid: number
  cpuCount: number
  platform: string
  arch: string
  startedAt: string
  uptimeSecs: number
  workspaceDir: string
  defaultProvider: string | null
  defaultModel: string | null
  gateway: RuntimeGatewayInfo
  tasks: RuntimeTasksInfo
}
