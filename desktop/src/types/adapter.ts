

export type StreamMode = 'off' | 'partial' | 'multi_message'

export type WhatsAppWebMode = 'business' | 'personal'
export type WhatsAppChatPolicy = 'allowlist' | 'ignore' | 'all'
export type LarkReceiveMode = 'websocket' | 'webhook'
export type VoiceProvider = 'twilio' | 'telnyx' | 'plivo'

export type PairedUser = {
  userId: string | number
  displayName: string
  pairedAt: number
}

export type PairingState = {
  code: string | null
  expiresAt: number | null
  createdAt: number | null
}

export type FeatureFlags = {
  channelNostr?: boolean
  voiceWake?: boolean
  channelMatrix?: boolean
}

export type GlobalChannelConfig = {
  cli?: boolean
  messageTimeoutSecs?: number
  ackReactions?: boolean
  showToolCalls?: boolean
  sessionPersistence?: boolean
  sessionBackend?: 'sqlite' | 'jsonl' | string
  sessionTtlHours?: number
  debounceMs?: number
}

export type TelegramConfig = {
  botToken?: string
  allowedUsers?: string[]
  streamMode?: StreamMode
  draftUpdateIntervalMs?: number
  interruptOnNewMessage?: boolean
  mentionOnly?: boolean
  ackReactions?: boolean | null
  proxyUrl?: string | null

  pairedUsers?: PairedUser[]
  defaultWorkDir?: string | null
}

export type DiscordConfig = {
  botToken?: string
  guildId?: string | null
  allowedUsers?: string[]
  listenToBots?: boolean
  interruptOnNewMessage?: boolean
  mentionOnly?: boolean
  proxyUrl?: string | null
  streamMode?: StreamMode
  draftUpdateIntervalMs?: number
  multiMessageDelayMs?: number
}

export type DiscordHistoryConfig = {
  botToken?: string
  guildId?: string | null
  allowedUsers?: string[]
  channelIds?: string[]
  storeDms?: boolean
  respondToDms?: boolean
  proxyUrl?: string | null
}

export type SlackConfig = {
  botToken?: string
  appToken?: string | null
  channelId?: string | null
  channelIds?: string[]
  allowedUsers?: string[]
  interruptOnNewMessage?: boolean
  threadReplies?: boolean | null
  mentionOnly?: boolean
  useMarkdownBlocks?: boolean
  proxyUrl?: string | null
  streamDrafts?: boolean
  draftUpdateIntervalMs?: number
}

export type MattermostConfig = {
  url?: string
  botToken?: string
  channelId?: string | null
  allowedUsers?: string[]
  threadReplies?: boolean | null
  mentionOnly?: boolean | null
  interruptOnNewMessage?: boolean
  proxyUrl?: string | null
}

export type WebhookConfig = {
  port?: number
  listenPath?: string | null
  sendUrl?: string | null
  sendMethod?: string | null
  authHeader?: string | null
  secret?: string | null
}

export type IMessageConfig = {
  allowedContacts?: string[]
}

export type MatrixConfig = {
  homeserver?: string
  accessToken?: string
  userId?: string | null
  deviceId?: string | null
  roomId?: string
  allowedUsers?: string[]
  allowedRooms?: string[]
  interruptOnNewMessage?: boolean
  streamMode?: StreamMode
  draftUpdateIntervalMs?: number
  multiMessageDelayMs?: number
  recoveryKey?: string | null
}

export type SignalConfig = {
  httpUrl?: string
  account?: string
  groupId?: string | null
  allowedFrom?: string[]
  ignoreAttachments?: boolean
  ignoreStories?: boolean
  proxyUrl?: string | null
}

export type WhatsAppConfig = {
  accessToken?: string | null
  phoneNumberId?: string | null
  verifyToken?: string | null
  appSecret?: string | null
  sessionPath?: string | null
  pairPhone?: string | null
  pairCode?: string | null
  allowedNumbers?: string[]
  mode?: WhatsAppWebMode
  dmPolicy?: WhatsAppChatPolicy
  groupPolicy?: WhatsAppChatPolicy
  selfChatMode?: boolean
  dmMentionPatterns?: string[]
  groupMentionPatterns?: string[]
  proxyUrl?: string | null
}

export type LinqConfig = {
  apiToken?: string
  fromPhone?: string
  signingSecret?: string | null
  allowedSenders?: string[]
}

export type WatiConfig = {
  apiToken?: string
  apiUrl?: string
  tenantId?: string | null
  allowedNumbers?: string[]
  proxyUrl?: string | null
}

export type NextcloudTalkConfig = {
  baseUrl?: string
  appToken?: string
  webhookSecret?: string | null
  allowedUsers?: string[]
  proxyUrl?: string | null
  botName?: string | null
}

export type EmailConfig = {
  imapHost?: string
  imapPort?: number
  imapFolder?: string
  smtpHost?: string
  smtpPort?: number
  smtpTls?: boolean
  username?: string
  password?: string
  fromAddress?: string
  idleTimeoutSecs?: number
  allowedSenders?: string[]
  defaultSubject?: string
}

export type GmailPushConfig = {
  enabled?: boolean
  topic?: string
  labelFilter?: string[]
  oauthToken?: string
  allowedSenders?: string[]
  webhookUrl?: string
  webhookSecret?: string
}

export type IrcConfig = {
  server?: string
  port?: number
  nickname?: string
  username?: string | null
  channels?: string[]
  allowedUsers?: string[]
  serverPassword?: string | null
  nickservPassword?: string | null
  saslPassword?: string | null
  verifyTls?: boolean | null
}

export type LarkConfig = {
  appId?: string
  appSecret?: string
  encryptKey?: string | null
  verificationToken?: string | null
  allowedUsers?: string[]
  mentionOnly?: boolean
  useFeishu?: boolean
  receiveMode?: LarkReceiveMode
  port?: number | null
  proxyUrl?: string | null
}

export type FeishuConfig = {
  appId?: string
  appSecret?: string
  encryptKey?: string | null
  verificationToken?: string | null
  allowedUsers?: string[]
  receiveMode?: LarkReceiveMode
  port?: number | null
  proxyUrl?: string | null

  pairedUsers?: PairedUser[]
  defaultWorkDir?: string | null
  streamingCard?: boolean
}

export type DingTalkConfig = {
  clientId?: string
  clientSecret?: string
  allowedUsers?: string[]
  proxyUrl?: string | null
}

export type WeComConfig = {
  webhookKey?: string
  allowedUsers?: string[]
}

export type QQConfig = {
  appId?: string
  appSecret?: string
  allowedUsers?: string[]
  proxyUrl?: string | null
}

export type TwitterConfig = {
  bearerToken?: string
  allowedUsers?: string[]
}

export type MochatConfig = {
  apiUrl?: string
  apiToken?: string
  allowedUsers?: string[]
  pollIntervalSecs?: number
}

export type NostrConfig = {
  privateKey?: string
  relays?: string[]
  allowedPubkeys?: string[]
}

export type ClawdTalkConfig = {
  apiKey?: string
  connectionId?: string
  fromNumber?: string
  allowedDestinations?: string[]
  webhookSecret?: string | null
}

export type RedditConfig = {
  clientId?: string
  clientSecret?: string
  refreshToken?: string
  username?: string
  subreddit?: string | null
}

export type BlueskyConfig = {
  handle?: string
  appPassword?: string
}

export type VoiceCallConfig = {
  provider?: VoiceProvider
  accountId?: string
  authToken?: string
  fromNumber?: string
  webhookPort?: number
  requireOutboundApproval?: boolean
  transcriptionLogging?: boolean
  ttsVoice?: string | null
  maxCallDurationSecs?: number
  webhookBaseUrl?: string | null
}

export type VoiceWakeConfig = {
  wakeWord?: string
  silenceTimeoutMs?: number
  energyThreshold?: number
  maxCaptureSecs?: number
}

export type AdapterFileConfig = {
  serverUrl?: string
  defaultProjectDir?: string
  pairing?: PairingState
  features?: FeatureFlags
  global?: GlobalChannelConfig
  telegram?: TelegramConfig | null
  discord?: DiscordConfig | null
  discordHistory?: DiscordHistoryConfig | null
  slack?: SlackConfig | null
  mattermost?: MattermostConfig | null
  webhook?: WebhookConfig | null
  imessage?: IMessageConfig | null
  matrix?: MatrixConfig | null
  signal?: SignalConfig | null
  whatsapp?: WhatsAppConfig | null
  linq?: LinqConfig | null
  wati?: WatiConfig | null
  nextcloudTalk?: NextcloudTalkConfig | null
  email?: EmailConfig | null
  gmailPush?: GmailPushConfig | null
  irc?: IrcConfig | null
  lark?: LarkConfig | null
  feishu?: FeishuConfig | null
  dingtalk?: DingTalkConfig | null
  wecom?: WeComConfig | null
  qq?: QQConfig | null
  twitter?: TwitterConfig | null
  mochat?: MochatConfig | null
  nostr?: NostrConfig | null
  clawdtalk?: ClawdTalkConfig | null
  reddit?: RedditConfig | null
  bluesky?: BlueskyConfig | null
  voiceCall?: VoiceCallConfig | null
  voiceWake?: VoiceWakeConfig | null
}

export const CHANNEL_IDS = [
  'telegram',
  'discord',
  'discordHistory',
  'slack',
  'mattermost',
  'webhook',
  'imessage',
  'matrix',
  'signal',
  'whatsapp',
  'linq',
  'wati',
  'nextcloudTalk',
  'email',
  'gmailPush',
  'irc',
  'lark',
  'feishu',
  'dingtalk',
  'wecom',
  'qq',
  'twitter',
  'mochat',
  'nostr',
  'clawdtalk',
  'reddit',
  'bluesky',
  'voiceCall',
  'voiceWake',
] as const

export type ChannelId = (typeof CHANNEL_IDS)[number]

export const PAIRING_CHANNELS: readonly ChannelId[] = ['telegram', 'feishu'] as const

export type PairingChannelId = (typeof PAIRING_CHANNELS)[number]
