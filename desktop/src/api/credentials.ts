// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'

export type CredentialKind = 'username' | 'password' | 'token' | 'url' | 'other'

export type CredentialMeta = {
  name: string
  kind: CredentialKind
  created_at?: string | number | null
  updated_at?: string | number | null
}

export type CredentialListResponse = {
  credentials: CredentialMeta[]
}

export type CredentialPutBody = {
  name: string
  kind: CredentialKind
  value: string
}

export type CredentialPutResponse = {
  status: string
  credential: CredentialMeta
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
