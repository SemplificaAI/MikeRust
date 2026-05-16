// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

import { templatesApi } from '$lib/api/templates'
import type { DocxTemplate } from '$lib/types/template'

function createTemplateStore() {
  let items = $state<DocxTemplate[]>([])
  let loading = $state<boolean>(false)
  let error = $state<string | null>(null)

  /** Distinct locale codes present in the loaded set (for the filter). */
  const locales = $derived([...new Set(items.map((t) => t.locale))].sort())

  return {
    get items() {
      return items
    },
    get locales() {
      return locales
    },
    get loading() {
      return loading
    },
    get error() {
      return error
    },

    async refresh() {
      loading = true
      error = null
      try {
        const res = await templatesApi.list()
        items = res.docx_templates
      } catch (e) {
        error = (e as Error).message
      } finally {
        loading = false
      }
    },
  }
}

export const templateStore = createTemplateStore()
