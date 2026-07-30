// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'

export type CredentialKind = 'username' | 'password' | 'token' | 'url' | 'other'

export type CredentialShape = 'single' | 'group'

export type CredentialFieldMeta = {
  key: string
  kind: CredentialKind
}

export type CredentialMeta = {
  name: string
  kind: CredentialKind
  created_at?: string | number | null
  updated_at?: string | number | null
  shape?: CredentialShape
  fields?: CredentialFieldMeta[]
}

export type CredentialListResponse = {
  credentials: CredentialMeta[]
}

export type CredentialFieldInput = {
  key: string
  kind: CredentialKind
  value: string
}

export type CredentialPutBody =
  | {
      name: string
      kind: CredentialKind
      value: string
      fields?: undefined
    }
  | {
      name: string
      fields: CredentialFieldInput[]
      kind?: undefined
      value?: undefined
    }

export type CredentialPutResponse = {
  status: string
  credential: CredentialMeta
}

export function isCredentialGroup(meta: CredentialMeta): boolean {
  return meta.shape === 'group' || (meta.fields?.length ?? 0) > 0
}

export const credentialsApi = {
  list: () => api.get<CredentialListResponse>('/api/credentials'),
  upsert: (body: CredentialPutBody) =>
    api.put<CredentialPutResponse>('/api/credentials', body),
  remove: (name: string) =>
    api.delete<{ status: string; name: string }>(
      `/api/credentials/${encodeURIComponent(name)}`,
    ),
}
