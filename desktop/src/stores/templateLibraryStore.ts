// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import {
  templateLibraryApi,
  type TemplateLibraryCatalog,
} from '../api/templateLibrary'

type TemplateLibraryState = {
  catalog: TemplateLibraryCatalog | null
  loading: boolean
  error: string | null
  load: () => Promise<void>
  refresh: () => Promise<void>
}

const EMPTY: TemplateLibraryCatalog = {
  designSystems: [],
  designerTemplates: [],
  promptTemplates: [],
  curatorTemplates: [],
}

async function fetchCatalog(set: (partial: Partial<TemplateLibraryState>) => void) {
  set({ loading: true, error: null })
  try {
    const catalog = await templateLibraryApi.catalog()
    set({
      catalog: {
        designSystems: catalog.designSystems ?? [],
        designerTemplates: catalog.designerTemplates ?? [],
        promptTemplates: catalog.promptTemplates ?? [],
        curatorTemplates: catalog.curatorTemplates ?? [],
      },
      loading: false,
    })
  } catch (err) {
    set({
      catalog: EMPTY,
      loading: false,
      error: err instanceof Error ? err.message : String(err),
    })
  }
}

export const useTemplateLibraryStore = create<TemplateLibraryState>((set) => ({
  catalog: null,
  loading: false,
  error: null,
  load: () => fetchCatalog(set),
  refresh: () => fetchCatalog(set),
}))
