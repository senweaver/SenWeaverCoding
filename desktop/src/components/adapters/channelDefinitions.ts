// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { AdapterFileConfig, ChannelId, FeatureFlags } from '../../types/adapter'

export type ChannelCategory = 'im' | 'whatsapp' | 'email' | 'webhook_social' | 'voice'

export type FieldType =
  | 'text'
  | 'password'
  | 'number'
  | 'checkbox'
  | 'tribool'
  | 'select'
  | 'csv'
  | 'csv_number'
  | 'textarea'

export type SelectOption = {
  value: string
  i18nLabel: string
}

export type ChannelField = {
  key: string
  type: FieldType
  i18nLabel: string
  i18nPlaceholder?: string
  i18nHint?: string
  required?: boolean
  options?: ReadonlyArray<SelectOption>

  span?: 1 | 2
}

export type ChannelDefinition = {
  id: ChannelId
  i18nName: string
  i18nTagline?: string
  category: ChannelCategory
  icon: string

  featureFlag?: keyof FeatureFlags

  i18nNotice?: string

  supportsPairing?: boolean

  platformOnly?: 'macos' | 'windows' | 'linux'
  fields: ReadonlyArray<ChannelField>

  isConfigured: (value: AdapterFileConfig[ChannelId]) => boolean
}

const STREAM_MODE_OPTIONS: readonly SelectOption[] = [
  { value: 'off', i18nLabel: 'settings.adapters.streamMode.off' },
  { value: 'partial', i18nLabel: 'settings.adapters.streamMode.partial' },
  { value: 'multi_message', i18nLabel: 'settings.adapters.streamMode.multi' },
]

const WHATSAPP_MODE_OPTIONS: readonly SelectOption[] = [
  { value: 'business', i18nLabel: 'settings.adapters.whatsappMode.business' },
  { value: 'personal', i18nLabel: 'settings.adapters.whatsappMode.personal' },
]

const WHATSAPP_POLICY_OPTIONS: readonly SelectOption[] = [
  { value: 'allowlist', i18nLabel: 'settings.adapters.whatsappPolicy.allowlist' },
  { value: 'ignore', i18nLabel: 'settings.adapters.whatsappPolicy.ignore' },
  { value: 'all', i18nLabel: 'settings.adapters.whatsappPolicy.all' },
]

const LARK_RECEIVE_OPTIONS: readonly SelectOption[] = [
  { value: 'websocket', i18nLabel: 'settings.adapters.larkReceive.websocket' },
  { value: 'webhook', i18nLabel: 'settings.adapters.larkReceive.webhook' },
]

const VOICE_PROVIDER_OPTIONS: readonly SelectOption[] = [
  { value: 'twilio', i18nLabel: 'settings.adapters.voiceProvider.twilio' },
  { value: 'telnyx', i18nLabel: 'settings.adapters.voiceProvider.telnyx' },
  { value: 'plivo', i18nLabel: 'settings.adapters.voiceProvider.plivo' },
]

const SESSION_BACKEND_OPTIONS: readonly SelectOption[] = [
  { value: 'sqlite', i18nLabel: 'settings.adapters.sessionBackend.sqlite' },
  { value: 'jsonl', i18nLabel: 'settings.adapters.sessionBackend.jsonl' },
]

function hasAny(value: unknown, keys: string[]): boolean {
  if (!value || typeof value !== 'object') return false
  const obj = value as Record<string, unknown>
  return keys.some((key) => {
    const v = obj[key]
    if (v === undefined || v === null) return false
    if (typeof v === 'string') return v.trim().length > 0
    if (Array.isArray(v)) return v.length > 0
    return true
  })
}

export const CHANNEL_DEFINITIONS: readonly ChannelDefinition[] = [

  {
    id: 'telegram',
    i18nName: 'settings.adapters.channels.telegram.name',
    i18nTagline: 'settings.adapters.channels.telegram.tagline',
    category: 'im',
    icon: 'send',
    supportsPairing: true,
    isConfigured: (v) => hasAny(v, ['botToken']),
    fields: [
      { key: 'botToken', type: 'password', i18nLabel: 'settings.adapters.fields.botToken', i18nPlaceholder: 'settings.adapters.placeholders.telegramBotToken', required: true, span: 2 },
      { key: 'allowedUsers', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedUsers', i18nHint: 'settings.adapters.hints.telegramAllowedUsers', span: 2 },
      { key: 'mentionOnly', type: 'checkbox', i18nLabel: 'settings.adapters.fields.mentionOnly', i18nHint: 'settings.adapters.hints.mentionOnly' },
      { key: 'interruptOnNewMessage', type: 'checkbox', i18nLabel: 'settings.adapters.fields.interruptOnNewMessage', i18nHint: 'settings.adapters.hints.interruptOnNewMessage' },
      { key: 'streamMode', type: 'select', i18nLabel: 'settings.adapters.fields.streamMode', options: STREAM_MODE_OPTIONS },
      { key: 'draftUpdateIntervalMs', type: 'number', i18nLabel: 'settings.adapters.fields.draftUpdateIntervalMs' },
      { key: 'ackReactions', type: 'tribool', i18nLabel: 'settings.adapters.fields.ackReactionsOverride', i18nHint: 'settings.adapters.hints.ackReactionsOverride' },
      { key: 'proxyUrl', type: 'text', i18nLabel: 'settings.adapters.fields.proxyUrl', i18nPlaceholder: 'settings.adapters.placeholders.proxyUrl', span: 2 },
    ],
  },
  {
    id: 'discord',
    i18nName: 'settings.adapters.channels.discord.name',
    i18nTagline: 'settings.adapters.channels.discord.tagline',
    category: 'im',
    icon: 'forum',
    isConfigured: (v) => hasAny(v, ['botToken']),
    fields: [
      { key: 'botToken', type: 'password', i18nLabel: 'settings.adapters.fields.botToken', i18nPlaceholder: 'settings.adapters.placeholders.discordBotToken', required: true, span: 2 },
      { key: 'guildId', type: 'text', i18nLabel: 'settings.adapters.fields.guildId', i18nHint: 'settings.adapters.hints.discordGuildId' },
      { key: 'allowedUsers', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedUsers', i18nHint: 'settings.adapters.hints.discordAllowedUsers' },
      { key: 'mentionOnly', type: 'checkbox', i18nLabel: 'settings.adapters.fields.mentionOnly' },
      { key: 'listenToBots', type: 'checkbox', i18nLabel: 'settings.adapters.fields.listenToBots', i18nHint: 'settings.adapters.hints.listenToBots' },
      { key: 'interruptOnNewMessage', type: 'checkbox', i18nLabel: 'settings.adapters.fields.interruptOnNewMessage' },
      { key: 'streamMode', type: 'select', i18nLabel: 'settings.adapters.fields.streamMode', options: STREAM_MODE_OPTIONS },
      { key: 'draftUpdateIntervalMs', type: 'number', i18nLabel: 'settings.adapters.fields.draftUpdateIntervalMs' },
      { key: 'multiMessageDelayMs', type: 'number', i18nLabel: 'settings.adapters.fields.multiMessageDelayMs' },
      { key: 'proxyUrl', type: 'text', i18nLabel: 'settings.adapters.fields.proxyUrl', span: 2 },
    ],
  },
  {
    id: 'discordHistory',
    i18nName: 'settings.adapters.channels.discordHistory.name',
    i18nTagline: 'settings.adapters.channels.discordHistory.tagline',
    category: 'im',
    icon: 'history',
    isConfigured: (v) => hasAny(v, ['botToken']),
    fields: [
      { key: 'botToken', type: 'password', i18nLabel: 'settings.adapters.fields.botToken', required: true, span: 2 },
      { key: 'guildId', type: 'text', i18nLabel: 'settings.adapters.fields.guildId' },
      { key: 'channelIds', type: 'csv', i18nLabel: 'settings.adapters.fields.channelIds', i18nHint: 'settings.adapters.hints.discordChannelIds' },
      { key: 'allowedUsers', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedUsers', span: 2 },
      { key: 'storeDms', type: 'checkbox', i18nLabel: 'settings.adapters.fields.storeDms' },
      { key: 'respondToDms', type: 'checkbox', i18nLabel: 'settings.adapters.fields.respondToDms' },
      { key: 'proxyUrl', type: 'text', i18nLabel: 'settings.adapters.fields.proxyUrl', span: 2 },
    ],
  },
  {
    id: 'slack',
    i18nName: 'settings.adapters.channels.slack.name',
    i18nTagline: 'settings.adapters.channels.slack.tagline',
    category: 'im',
    icon: 'tag',
    isConfigured: (v) => hasAny(v, ['botToken']),
    fields: [
      { key: 'botToken', type: 'password', i18nLabel: 'settings.adapters.fields.slackBotToken', i18nPlaceholder: 'settings.adapters.placeholders.slackBotToken', required: true },
      { key: 'appToken', type: 'password', i18nLabel: 'settings.adapters.fields.slackAppToken', i18nPlaceholder: 'settings.adapters.placeholders.slackAppToken' },
      { key: 'channelId', type: 'text', i18nLabel: 'settings.adapters.fields.slackChannelId' },
      { key: 'channelIds', type: 'csv', i18nLabel: 'settings.adapters.fields.slackChannelIds' },
      { key: 'allowedUsers', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedUsers', span: 2 },
      { key: 'mentionOnly', type: 'checkbox', i18nLabel: 'settings.adapters.fields.mentionOnly' },
      { key: 'threadReplies', type: 'tribool', i18nLabel: 'settings.adapters.fields.threadReplies' },
      { key: 'useMarkdownBlocks', type: 'checkbox', i18nLabel: 'settings.adapters.fields.useMarkdownBlocks' },
      { key: 'streamDrafts', type: 'checkbox', i18nLabel: 'settings.adapters.fields.streamDrafts' },
      { key: 'interruptOnNewMessage', type: 'checkbox', i18nLabel: 'settings.adapters.fields.interruptOnNewMessage' },
      { key: 'draftUpdateIntervalMs', type: 'number', i18nLabel: 'settings.adapters.fields.draftUpdateIntervalMs' },
      { key: 'proxyUrl', type: 'text', i18nLabel: 'settings.adapters.fields.proxyUrl', span: 2 },
    ],
  },
  {
    id: 'mattermost',
    i18nName: 'settings.adapters.channels.mattermost.name',
    i18nTagline: 'settings.adapters.channels.mattermost.tagline',
    category: 'im',
    icon: 'chat',
    isConfigured: (v) => hasAny(v, ['url', 'botToken']),
    fields: [
      { key: 'url', type: 'text', i18nLabel: 'settings.adapters.fields.mattermostUrl', i18nPlaceholder: 'settings.adapters.placeholders.mattermostUrl', required: true, span: 2 },
      { key: 'botToken', type: 'password', i18nLabel: 'settings.adapters.fields.botToken', required: true, span: 2 },
      { key: 'channelId', type: 'text', i18nLabel: 'settings.adapters.fields.mattermostChannelId' },
      { key: 'allowedUsers', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedUsers' },
      { key: 'mentionOnly', type: 'tribool', i18nLabel: 'settings.adapters.fields.mentionOnly' },
      { key: 'threadReplies', type: 'tribool', i18nLabel: 'settings.adapters.fields.threadReplies' },
      { key: 'interruptOnNewMessage', type: 'checkbox', i18nLabel: 'settings.adapters.fields.interruptOnNewMessage' },
      { key: 'proxyUrl', type: 'text', i18nLabel: 'settings.adapters.fields.proxyUrl', span: 2 },
    ],
  },
  {
    id: 'lark',
    i18nName: 'settings.adapters.channels.lark.name',
    i18nTagline: 'settings.adapters.channels.lark.tagline',
    category: 'im',
    icon: 'workspaces',
    isConfigured: (v) => hasAny(v, ['appId', 'appSecret']),
    fields: [
      { key: 'appId', type: 'text', i18nLabel: 'settings.adapters.fields.appId', required: true },
      { key: 'appSecret', type: 'password', i18nLabel: 'settings.adapters.fields.appSecret', required: true },
      { key: 'encryptKey', type: 'password', i18nLabel: 'settings.adapters.fields.encryptKey' },
      { key: 'verificationToken', type: 'password', i18nLabel: 'settings.adapters.fields.verificationToken' },
      { key: 'allowedUsers', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedUsers', span: 2 },
      { key: 'useFeishu', type: 'checkbox', i18nLabel: 'settings.adapters.fields.useFeishu', i18nHint: 'settings.adapters.hints.useFeishu' },
      { key: 'mentionOnly', type: 'checkbox', i18nLabel: 'settings.adapters.fields.mentionOnly' },
      { key: 'receiveMode', type: 'select', i18nLabel: 'settings.adapters.fields.receiveMode', options: LARK_RECEIVE_OPTIONS },
      { key: 'port', type: 'number', i18nLabel: 'settings.adapters.fields.webhookPort', i18nHint: 'settings.adapters.hints.webhookPortLark' },
      { key: 'proxyUrl', type: 'text', i18nLabel: 'settings.adapters.fields.proxyUrl', span: 2 },
    ],
  },
  {
    id: 'feishu',
    i18nName: 'settings.adapters.channels.feishu.name',
    i18nTagline: 'settings.adapters.channels.feishu.tagline',
    category: 'im',
    icon: 'workspaces',
    supportsPairing: true,
    isConfigured: (v) => hasAny(v, ['appId', 'appSecret']),
    fields: [
      { key: 'appId', type: 'text', i18nLabel: 'settings.adapters.fields.appId', i18nPlaceholder: 'settings.adapters.placeholders.feishuAppId', required: true },
      { key: 'appSecret', type: 'password', i18nLabel: 'settings.adapters.fields.appSecret', required: true },
      { key: 'encryptKey', type: 'password', i18nLabel: 'settings.adapters.fields.encryptKey' },
      { key: 'verificationToken', type: 'password', i18nLabel: 'settings.adapters.fields.verificationToken' },
      { key: 'allowedUsers', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedUsers', i18nHint: 'settings.adapters.hints.feishuAllowedUsers', span: 2 },
      { key: 'receiveMode', type: 'select', i18nLabel: 'settings.adapters.fields.receiveMode', options: LARK_RECEIVE_OPTIONS },
      { key: 'port', type: 'number', i18nLabel: 'settings.adapters.fields.webhookPort' },
      { key: 'streamingCard', type: 'checkbox', i18nLabel: 'settings.adapters.fields.streamingCard', i18nHint: 'settings.adapters.hints.streamingCard' },
      { key: 'proxyUrl', type: 'text', i18nLabel: 'settings.adapters.fields.proxyUrl', span: 2 },
    ],
  },
  {
    id: 'dingtalk',
    i18nName: 'settings.adapters.channels.dingtalk.name',
    i18nTagline: 'settings.adapters.channels.dingtalk.tagline',
    category: 'im',
    icon: 'business',
    isConfigured: (v) => hasAny(v, ['clientId', 'clientSecret']),
    fields: [
      { key: 'clientId', type: 'text', i18nLabel: 'settings.adapters.fields.clientId', required: true },
      { key: 'clientSecret', type: 'password', i18nLabel: 'settings.adapters.fields.clientSecret', required: true },
      { key: 'allowedUsers', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedUsers', span: 2 },
      { key: 'proxyUrl', type: 'text', i18nLabel: 'settings.adapters.fields.proxyUrl', span: 2 },
    ],
  },
  {
    id: 'wecom',
    i18nName: 'settings.adapters.channels.wecom.name',
    i18nTagline: 'settings.adapters.channels.wecom.tagline',
    category: 'im',
    icon: 'business_center',
    isConfigured: (v) => hasAny(v, ['webhookKey']),
    fields: [
      { key: 'webhookKey', type: 'password', i18nLabel: 'settings.adapters.fields.webhookKey', i18nPlaceholder: 'settings.adapters.placeholders.wecomWebhookKey', required: true, span: 2 },
      { key: 'allowedUsers', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedUsers', span: 2 },
    ],
  },
  {
    id: 'qq',
    i18nName: 'settings.adapters.channels.qq.name',
    i18nTagline: 'settings.adapters.channels.qq.tagline',
    category: 'im',
    icon: 'sms',
    isConfigured: (v) => hasAny(v, ['appId', 'appSecret']),
    fields: [
      { key: 'appId', type: 'text', i18nLabel: 'settings.adapters.fields.appId', required: true },
      { key: 'appSecret', type: 'password', i18nLabel: 'settings.adapters.fields.appSecret', required: true },
      { key: 'allowedUsers', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedUsers', span: 2 },
      { key: 'proxyUrl', type: 'text', i18nLabel: 'settings.adapters.fields.proxyUrl', span: 2 },
    ],
  },
  {
    id: 'matrix',
    i18nName: 'settings.adapters.channels.matrix.name',
    i18nTagline: 'settings.adapters.channels.matrix.tagline',
    category: 'im',
    icon: 'hub',
    featureFlag: 'channelMatrix',
    isConfigured: (v) => hasAny(v, ['homeserver', 'accessToken', 'roomId']),
    fields: [
      { key: 'homeserver', type: 'text', i18nLabel: 'settings.adapters.fields.matrixHomeserver', i18nPlaceholder: 'settings.adapters.placeholders.matrixHomeserver', required: true, span: 2 },
      { key: 'accessToken', type: 'password', i18nLabel: 'settings.adapters.fields.accessToken', required: true, span: 2 },
      { key: 'roomId', type: 'text', i18nLabel: 'settings.adapters.fields.matrixRoomId', i18nPlaceholder: 'settings.adapters.placeholders.matrixRoomId', required: true, span: 2 },
      { key: 'userId', type: 'text', i18nLabel: 'settings.adapters.fields.matrixUserId' },
      { key: 'deviceId', type: 'text', i18nLabel: 'settings.adapters.fields.matrixDeviceId' },
      { key: 'allowedUsers', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedUsers', span: 2 },
      { key: 'allowedRooms', type: 'csv', i18nLabel: 'settings.adapters.fields.matrixAllowedRooms', span: 2 },
      { key: 'streamMode', type: 'select', i18nLabel: 'settings.adapters.fields.streamMode', options: STREAM_MODE_OPTIONS },
      { key: 'interruptOnNewMessage', type: 'checkbox', i18nLabel: 'settings.adapters.fields.interruptOnNewMessage' },
      { key: 'draftUpdateIntervalMs', type: 'number', i18nLabel: 'settings.adapters.fields.draftUpdateIntervalMs' },
      { key: 'multiMessageDelayMs', type: 'number', i18nLabel: 'settings.adapters.fields.multiMessageDelayMs' },
      { key: 'recoveryKey', type: 'password', i18nLabel: 'settings.adapters.fields.matrixRecoveryKey', i18nHint: 'settings.adapters.hints.matrixRecoveryKey', span: 2 },
    ],
  },
  {
    id: 'signal',
    i18nName: 'settings.adapters.channels.signal.name',
    i18nTagline: 'settings.adapters.channels.signal.tagline',
    category: 'im',
    icon: 'lock',
    i18nNotice: 'settings.adapters.notices.signal',
    isConfigured: (v) => hasAny(v, ['httpUrl', 'account']),
    fields: [
      { key: 'httpUrl', type: 'text', i18nLabel: 'settings.adapters.fields.signalHttpUrl', i18nPlaceholder: 'settings.adapters.placeholders.signalHttpUrl', required: true, span: 2 },
      { key: 'account', type: 'text', i18nLabel: 'settings.adapters.fields.signalAccount', i18nPlaceholder: 'settings.adapters.placeholders.signalAccount', required: true, span: 2 },
      { key: 'groupId', type: 'text', i18nLabel: 'settings.adapters.fields.signalGroupId', i18nHint: 'settings.adapters.hints.signalGroupId', span: 2 },
      { key: 'allowedFrom', type: 'csv', i18nLabel: 'settings.adapters.fields.signalAllowedFrom', span: 2 },
      { key: 'ignoreAttachments', type: 'checkbox', i18nLabel: 'settings.adapters.fields.ignoreAttachments' },
      { key: 'ignoreStories', type: 'checkbox', i18nLabel: 'settings.adapters.fields.ignoreStories' },
      { key: 'proxyUrl', type: 'text', i18nLabel: 'settings.adapters.fields.proxyUrl', span: 2 },
    ],
  },
  {
    id: 'irc',
    i18nName: 'settings.adapters.channels.irc.name',
    i18nTagline: 'settings.adapters.channels.irc.tagline',
    category: 'im',
    icon: 'terminal',
    isConfigured: (v) => hasAny(v, ['server', 'nickname']),
    fields: [
      { key: 'server', type: 'text', i18nLabel: 'settings.adapters.fields.ircServer', i18nPlaceholder: 'settings.adapters.placeholders.ircServer', required: true, span: 2 },
      { key: 'port', type: 'number', i18nLabel: 'settings.adapters.fields.ircPort' },
      { key: 'nickname', type: 'text', i18nLabel: 'settings.adapters.fields.ircNickname', required: true },
      { key: 'username', type: 'text', i18nLabel: 'settings.adapters.fields.ircUsername' },
      { key: 'channels', type: 'csv', i18nLabel: 'settings.adapters.fields.ircChannels', i18nPlaceholder: 'settings.adapters.placeholders.ircChannels' },
      { key: 'allowedUsers', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedUsers', span: 2 },
      { key: 'serverPassword', type: 'password', i18nLabel: 'settings.adapters.fields.ircServerPassword' },
      { key: 'nickservPassword', type: 'password', i18nLabel: 'settings.adapters.fields.ircNickservPassword' },
      { key: 'saslPassword', type: 'password', i18nLabel: 'settings.adapters.fields.ircSaslPassword' },
      { key: 'verifyTls', type: 'tribool', i18nLabel: 'settings.adapters.fields.ircVerifyTls' },
    ],
  },
  {
    id: 'imessage',
    i18nName: 'settings.adapters.channels.imessage.name',
    i18nTagline: 'settings.adapters.channels.imessage.tagline',
    category: 'im',
    icon: 'phone_iphone',
    platformOnly: 'macos',
    i18nNotice: 'settings.adapters.notices.imessage',
    isConfigured: (v) => hasAny(v, ['allowedContacts']),
    fields: [
      { key: 'allowedContacts', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedContacts', i18nPlaceholder: 'settings.adapters.placeholders.allowedContacts', i18nHint: 'settings.adapters.hints.allowedContacts', span: 2, required: true },
    ],
  },
  {
    id: 'nextcloudTalk',
    i18nName: 'settings.adapters.channels.nextcloudTalk.name',
    i18nTagline: 'settings.adapters.channels.nextcloudTalk.tagline',
    category: 'im',
    icon: 'cloud',
    isConfigured: (v) => hasAny(v, ['baseUrl', 'appToken']),
    fields: [
      { key: 'baseUrl', type: 'text', i18nLabel: 'settings.adapters.fields.nextcloudBaseUrl', i18nPlaceholder: 'settings.adapters.placeholders.nextcloudBaseUrl', required: true, span: 2 },
      { key: 'appToken', type: 'password', i18nLabel: 'settings.adapters.fields.appToken', required: true, span: 2 },
      { key: 'webhookSecret', type: 'password', i18nLabel: 'settings.adapters.fields.webhookSecret', span: 2 },
      { key: 'allowedUsers', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedUsers', span: 2 },
      { key: 'botName', type: 'text', i18nLabel: 'settings.adapters.fields.botName', i18nHint: 'settings.adapters.hints.botName' },
      { key: 'proxyUrl', type: 'text', i18nLabel: 'settings.adapters.fields.proxyUrl' },
    ],
  },
  {
    id: 'mochat',
    i18nName: 'settings.adapters.channels.mochat.name',
    i18nTagline: 'settings.adapters.channels.mochat.tagline',
    category: 'im',
    icon: 'support_agent',
    isConfigured: (v) => hasAny(v, ['apiUrl', 'apiToken']),
    fields: [
      { key: 'apiUrl', type: 'text', i18nLabel: 'settings.adapters.fields.apiUrl', required: true, span: 2 },
      { key: 'apiToken', type: 'password', i18nLabel: 'settings.adapters.fields.apiToken', required: true, span: 2 },
      { key: 'allowedUsers', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedUsers', span: 2 },
      { key: 'pollIntervalSecs', type: 'number', i18nLabel: 'settings.adapters.fields.pollIntervalSecs' },
    ],
  },
  {
    id: 'nostr',
    i18nName: 'settings.adapters.channels.nostr.name',
    i18nTagline: 'settings.adapters.channels.nostr.tagline',
    category: 'im',
    icon: 'public',
    featureFlag: 'channelNostr',
    isConfigured: (v) => hasAny(v, ['privateKey']),
    fields: [
      { key: 'privateKey', type: 'password', i18nLabel: 'settings.adapters.fields.nostrPrivateKey', i18nPlaceholder: 'settings.adapters.placeholders.nostrPrivateKey', required: true, span: 2 },
      { key: 'relays', type: 'csv', i18nLabel: 'settings.adapters.fields.nostrRelays', i18nHint: 'settings.adapters.hints.nostrRelays', span: 2 },
      { key: 'allowedPubkeys', type: 'csv', i18nLabel: 'settings.adapters.fields.nostrAllowedPubkeys', span: 2 },
    ],
  },

  {
    id: 'whatsapp',
    i18nName: 'settings.adapters.channels.whatsapp.name',
    i18nTagline: 'settings.adapters.channels.whatsapp.tagline',
    category: 'whatsapp',
    icon: 'chat',
    i18nNotice: 'settings.adapters.notices.whatsapp',
    isConfigured: (v) => hasAny(v, ['phoneNumberId', 'sessionPath']),
    fields: [
      { key: 'accessToken', type: 'password', i18nLabel: 'settings.adapters.fields.whatsappAccessToken', i18nHint: 'settings.adapters.hints.whatsappCloudOnly', span: 2 },
      { key: 'phoneNumberId', type: 'text', i18nLabel: 'settings.adapters.fields.whatsappPhoneNumberId' },
      { key: 'verifyToken', type: 'password', i18nLabel: 'settings.adapters.fields.whatsappVerifyToken' },
      { key: 'appSecret', type: 'password', i18nLabel: 'settings.adapters.fields.whatsappAppSecret', span: 2 },
      { key: 'sessionPath', type: 'text', i18nLabel: 'settings.adapters.fields.whatsappSessionPath', i18nHint: 'settings.adapters.hints.whatsappWebOnly', span: 2 },
      { key: 'pairPhone', type: 'text', i18nLabel: 'settings.adapters.fields.whatsappPairPhone', i18nHint: 'settings.adapters.hints.whatsappPairPhone' },
      { key: 'pairCode', type: 'text', i18nLabel: 'settings.adapters.fields.whatsappPairCode' },
      { key: 'allowedNumbers', type: 'csv', i18nLabel: 'settings.adapters.fields.whatsappAllowedNumbers', span: 2 },
      { key: 'mode', type: 'select', i18nLabel: 'settings.adapters.fields.whatsappMode', options: WHATSAPP_MODE_OPTIONS },
      { key: 'selfChatMode', type: 'checkbox', i18nLabel: 'settings.adapters.fields.whatsappSelfChat' },
      { key: 'dmPolicy', type: 'select', i18nLabel: 'settings.adapters.fields.whatsappDmPolicy', options: WHATSAPP_POLICY_OPTIONS },
      { key: 'groupPolicy', type: 'select', i18nLabel: 'settings.adapters.fields.whatsappGroupPolicy', options: WHATSAPP_POLICY_OPTIONS },
      { key: 'dmMentionPatterns', type: 'csv', i18nLabel: 'settings.adapters.fields.whatsappDmMentionPatterns', span: 2 },
      { key: 'groupMentionPatterns', type: 'csv', i18nLabel: 'settings.adapters.fields.whatsappGroupMentionPatterns', span: 2 },
      { key: 'proxyUrl', type: 'text', i18nLabel: 'settings.adapters.fields.proxyUrl', span: 2 },
    ],
  },
  {
    id: 'linq',
    i18nName: 'settings.adapters.channels.linq.name',
    i18nTagline: 'settings.adapters.channels.linq.tagline',
    category: 'whatsapp',
    icon: 'forum',
    isConfigured: (v) => hasAny(v, ['apiToken', 'fromPhone']),
    fields: [
      { key: 'apiToken', type: 'password', i18nLabel: 'settings.adapters.fields.apiToken', required: true, span: 2 },
      { key: 'fromPhone', type: 'text', i18nLabel: 'settings.adapters.fields.fromPhone', i18nPlaceholder: 'settings.adapters.placeholders.e164', required: true },
      { key: 'signingSecret', type: 'password', i18nLabel: 'settings.adapters.fields.signingSecret' },
      { key: 'allowedSenders', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedSenders', span: 2 },
    ],
  },
  {
    id: 'wati',
    i18nName: 'settings.adapters.channels.wati.name',
    i18nTagline: 'settings.adapters.channels.wati.tagline',
    category: 'whatsapp',
    icon: 'storefront',
    isConfigured: (v) => hasAny(v, ['apiToken']),
    fields: [
      { key: 'apiToken', type: 'password', i18nLabel: 'settings.adapters.fields.apiToken', required: true, span: 2 },
      { key: 'apiUrl', type: 'text', i18nLabel: 'settings.adapters.fields.apiUrl', i18nPlaceholder: 'settings.adapters.placeholders.watiApiUrl' },
      { key: 'tenantId', type: 'text', i18nLabel: 'settings.adapters.fields.tenantId' },
      { key: 'allowedNumbers', type: 'csv', i18nLabel: 'settings.adapters.fields.whatsappAllowedNumbers', span: 2 },
      { key: 'proxyUrl', type: 'text', i18nLabel: 'settings.adapters.fields.proxyUrl', span: 2 },
    ],
  },

  {
    id: 'email',
    i18nName: 'settings.adapters.channels.email.name',
    i18nTagline: 'settings.adapters.channels.email.tagline',
    category: 'email',
    icon: 'mail',
    isConfigured: (v) => hasAny(v, ['imapHost', 'username']),
    fields: [
      { key: 'imapHost', type: 'text', i18nLabel: 'settings.adapters.fields.imapHost', i18nPlaceholder: 'settings.adapters.placeholders.imapHost', required: true },
      { key: 'imapPort', type: 'number', i18nLabel: 'settings.adapters.fields.imapPort' },
      { key: 'imapFolder', type: 'text', i18nLabel: 'settings.adapters.fields.imapFolder' },
      { key: 'idleTimeoutSecs', type: 'number', i18nLabel: 'settings.adapters.fields.idleTimeoutSecs', i18nHint: 'settings.adapters.hints.idleTimeoutSecs' },
      { key: 'smtpHost', type: 'text', i18nLabel: 'settings.adapters.fields.smtpHost', i18nPlaceholder: 'settings.adapters.placeholders.smtpHost', required: true },
      { key: 'smtpPort', type: 'number', i18nLabel: 'settings.adapters.fields.smtpPort' },
      { key: 'smtpTls', type: 'checkbox', i18nLabel: 'settings.adapters.fields.smtpTls' },
      { key: 'username', type: 'text', i18nLabel: 'settings.adapters.fields.emailUsername', required: true },
      { key: 'password', type: 'password', i18nLabel: 'settings.adapters.fields.emailPassword', required: true },
      { key: 'fromAddress', type: 'text', i18nLabel: 'settings.adapters.fields.fromAddress', i18nPlaceholder: 'settings.adapters.placeholders.fromAddress', required: true, span: 2 },
      { key: 'allowedSenders', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedSenders', i18nHint: 'settings.adapters.hints.emailAllowedSenders', span: 2 },
      { key: 'defaultSubject', type: 'text', i18nLabel: 'settings.adapters.fields.defaultSubject', span: 2 },
    ],
  },
  {
    id: 'gmailPush',
    i18nName: 'settings.adapters.channels.gmailPush.name',
    i18nTagline: 'settings.adapters.channels.gmailPush.tagline',
    category: 'email',
    icon: 'mark_email_unread',
    i18nNotice: 'settings.adapters.notices.gmailPush',
    isConfigured: (v) => hasAny(v, ['topic']),
    fields: [
      { key: 'enabled', type: 'checkbox', i18nLabel: 'settings.adapters.fields.enabled' },
      { key: 'topic', type: 'text', i18nLabel: 'settings.adapters.fields.gmailTopic', i18nPlaceholder: 'settings.adapters.placeholders.gmailTopic', required: true, span: 2 },
      { key: 'oauthToken', type: 'password', i18nLabel: 'settings.adapters.fields.oauthToken', span: 2 },
      { key: 'labelFilter', type: 'csv', i18nLabel: 'settings.adapters.fields.gmailLabelFilter', i18nHint: 'settings.adapters.hints.gmailLabelFilter' },
      { key: 'allowedSenders', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedSenders' },
      { key: 'webhookUrl', type: 'text', i18nLabel: 'settings.adapters.fields.webhookUrl', span: 2 },
      { key: 'webhookSecret', type: 'password', i18nLabel: 'settings.adapters.fields.webhookSecret', span: 2 },
    ],
  },

  {
    id: 'webhook',
    i18nName: 'settings.adapters.channels.webhook.name',
    i18nTagline: 'settings.adapters.channels.webhook.tagline',
    category: 'webhook_social',
    icon: 'webhook',
    isConfigured: (v) => hasAny(v, ['port']),
    fields: [
      { key: 'port', type: 'number', i18nLabel: 'settings.adapters.fields.port', required: true },
      { key: 'listenPath', type: 'text', i18nLabel: 'settings.adapters.fields.listenPath', i18nPlaceholder: 'settings.adapters.placeholders.listenPath' },
      { key: 'sendUrl', type: 'text', i18nLabel: 'settings.adapters.fields.sendUrl', i18nHint: 'settings.adapters.hints.sendUrl', span: 2 },
      { key: 'sendMethod', type: 'select', i18nLabel: 'settings.adapters.fields.sendMethod', options: [
        { value: 'POST', i18nLabel: 'settings.adapters.httpMethod.post' },
        { value: 'PUT', i18nLabel: 'settings.adapters.httpMethod.put' },
      ] },
      { key: 'authHeader', type: 'password', i18nLabel: 'settings.adapters.fields.authHeader' },
      { key: 'secret', type: 'password', i18nLabel: 'settings.adapters.fields.webhookSecret', i18nHint: 'settings.adapters.hints.webhookSecret', span: 2 },
    ],
  },
  {
    id: 'twitter',
    i18nName: 'settings.adapters.channels.twitter.name',
    i18nTagline: 'settings.adapters.channels.twitter.tagline',
    category: 'webhook_social',
    icon: 'public',
    i18nNotice: 'settings.adapters.notices.twitter',
    isConfigured: (v) => hasAny(v, ['bearerToken']),
    fields: [
      { key: 'bearerToken', type: 'password', i18nLabel: 'settings.adapters.fields.bearerToken', required: true, span: 2 },
      { key: 'allowedUsers', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedUsers', span: 2 },
    ],
  },
  {
    id: 'reddit',
    i18nName: 'settings.adapters.channels.reddit.name',
    i18nTagline: 'settings.adapters.channels.reddit.tagline',
    category: 'webhook_social',
    icon: 'groups',
    i18nNotice: 'settings.adapters.notices.reddit',
    isConfigured: (v) => hasAny(v, ['clientId', 'username']),
    fields: [
      { key: 'clientId', type: 'text', i18nLabel: 'settings.adapters.fields.clientId', required: true },
      { key: 'clientSecret', type: 'password', i18nLabel: 'settings.adapters.fields.clientSecret', required: true },
      { key: 'refreshToken', type: 'password', i18nLabel: 'settings.adapters.fields.refreshToken', required: true, span: 2 },
      { key: 'username', type: 'text', i18nLabel: 'settings.adapters.fields.redditUsername', i18nHint: 'settings.adapters.hints.redditUsername', required: true },
      { key: 'subreddit', type: 'text', i18nLabel: 'settings.adapters.fields.redditSubreddit' },
    ],
  },
  {
    id: 'bluesky',
    i18nName: 'settings.adapters.channels.bluesky.name',
    i18nTagline: 'settings.adapters.channels.bluesky.tagline',
    category: 'webhook_social',
    icon: 'cloud_circle',
    isConfigured: (v) => hasAny(v, ['handle', 'appPassword']),
    fields: [
      { key: 'handle', type: 'text', i18nLabel: 'settings.adapters.fields.blueskyHandle', i18nPlaceholder: 'settings.adapters.placeholders.blueskyHandle', required: true },
      { key: 'appPassword', type: 'password', i18nLabel: 'settings.adapters.fields.blueskyAppPassword', required: true },
    ],
  },

  {
    id: 'voiceCall',
    i18nName: 'settings.adapters.channels.voiceCall.name',
    i18nTagline: 'settings.adapters.channels.voiceCall.tagline',
    category: 'voice',
    icon: 'call',
    i18nNotice: 'settings.adapters.notices.voiceCall',
    isConfigured: (v) => hasAny(v, ['accountId', 'authToken', 'fromNumber']),
    fields: [
      { key: 'provider', type: 'select', i18nLabel: 'settings.adapters.fields.voiceProvider', options: VOICE_PROVIDER_OPTIONS },
      { key: 'fromNumber', type: 'text', i18nLabel: 'settings.adapters.fields.voiceFromNumber', i18nPlaceholder: 'settings.adapters.placeholders.e164', required: true },
      { key: 'accountId', type: 'text', i18nLabel: 'settings.adapters.fields.voiceAccountId', i18nHint: 'settings.adapters.hints.voiceAccountId', required: true },
      { key: 'authToken', type: 'password', i18nLabel: 'settings.adapters.fields.voiceAuthToken', required: true },
      { key: 'webhookPort', type: 'number', i18nLabel: 'settings.adapters.fields.voiceWebhookPort' },
      { key: 'maxCallDurationSecs', type: 'number', i18nLabel: 'settings.adapters.fields.maxCallDurationSecs' },
      { key: 'requireOutboundApproval', type: 'checkbox', i18nLabel: 'settings.adapters.fields.requireOutboundApproval' },
      { key: 'transcriptionLogging', type: 'checkbox', i18nLabel: 'settings.adapters.fields.transcriptionLogging' },
      { key: 'ttsVoice', type: 'text', i18nLabel: 'settings.adapters.fields.ttsVoice', i18nHint: 'settings.adapters.hints.ttsVoice' },
      { key: 'webhookBaseUrl', type: 'text', i18nLabel: 'settings.adapters.fields.webhookBaseUrl', i18nHint: 'settings.adapters.hints.webhookBaseUrl', span: 2 },
    ],
  },
  {
    id: 'telnyx',
    i18nName: 'settings.adapters.channels.telnyx.name',
    i18nTagline: 'settings.adapters.channels.telnyx.tagline',
    category: 'voice',
    icon: 'voice_chat',
    isConfigured: (v) => hasAny(v, ['apiKey', 'connectionId']),
    fields: [
      { key: 'apiKey', type: 'password', i18nLabel: 'settings.adapters.fields.clawdApiKey', required: true, span: 2 },
      { key: 'connectionId', type: 'text', i18nLabel: 'settings.adapters.fields.clawdConnectionId', required: true },
      { key: 'fromNumber', type: 'text', i18nLabel: 'settings.adapters.fields.voiceFromNumber', i18nPlaceholder: 'settings.adapters.placeholders.e164', required: true },
      { key: 'allowedDestinations', type: 'csv', i18nLabel: 'settings.adapters.fields.allowedDestinations', i18nHint: 'settings.adapters.hints.allowedDestinations', span: 2 },
      { key: 'webhookSecret', type: 'password', i18nLabel: 'settings.adapters.fields.webhookSecret', span: 2 },
    ],
  },
  {
    id: 'voiceWake',
    i18nName: 'settings.adapters.channels.voiceWake.name',
    i18nTagline: 'settings.adapters.channels.voiceWake.tagline',
    category: 'voice',
    icon: 'mic',
    featureFlag: 'voiceWake',
    isConfigured: (v) => hasAny(v, ['wakeWord']),
    fields: [
      { key: 'wakeWord', type: 'text', i18nLabel: 'settings.adapters.fields.wakeWord', i18nPlaceholder: 'settings.adapters.placeholders.wakeWord', span: 2 },
      { key: 'silenceTimeoutMs', type: 'number', i18nLabel: 'settings.adapters.fields.silenceTimeoutMs' },
      { key: 'maxCaptureSecs', type: 'number', i18nLabel: 'settings.adapters.fields.maxCaptureSecs' },
      { key: 'energyThreshold', type: 'number', i18nLabel: 'settings.adapters.fields.energyThreshold', i18nHint: 'settings.adapters.hints.energyThreshold', span: 2 },
    ],
  },
] as const

export const CHANNEL_CATEGORIES: ReadonlyArray<{
  id: ChannelCategory
  i18nLabel: string
  icon: string
}> = [
  { id: 'im', i18nLabel: 'settings.adapters.categories.im', icon: 'forum' },
  { id: 'whatsapp', i18nLabel: 'settings.adapters.categories.whatsapp', icon: 'chat' },
  { id: 'email', i18nLabel: 'settings.adapters.categories.email', icon: 'mail' },
  { id: 'webhook_social', i18nLabel: 'settings.adapters.categories.webhookSocial', icon: 'public' },
  { id: 'voice', i18nLabel: 'settings.adapters.categories.voice', icon: 'call' },
]

export const SESSION_BACKEND_CHOICES = SESSION_BACKEND_OPTIONS

export function findChannelDefinition(id: string): ChannelDefinition | undefined {
  return CHANNEL_DEFINITIONS.find((def) => def.id === id)
}

export function channelsByCategory(category: ChannelCategory): ChannelDefinition[] {
  return CHANNEL_DEFINITIONS.filter((def) => def.category === category)
}
