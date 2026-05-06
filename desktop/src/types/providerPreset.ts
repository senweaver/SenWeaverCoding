import type { ApiFormat } from './provider'

export type ProviderPreset = {
  id: string
  name: string
  baseUrl: string
  apiFormat: ApiFormat

  defaultModels: string[]
  needsApiKey: boolean
  websiteUrl: string
}
