// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

import { api } from './client'
import type { Domain } from '$lib/types/domain'
import type { DocumentMeta } from '$lib/types/document'

// `type` (not interface) — assignable to the client's query Record.
export type DocumentFilter = {
  project_id?: string
  domain?: Domain
}

/** Wrappers for `src/routes/documents.rs`. All require auth. */
export const documentsApi = {
  list: (filter?: DocumentFilter) =>
    api<{ documents: DocumentMeta[] }>('/document', { query: filter }),

  remove: (id: string) =>
    api<{ ok: boolean }>(`/document/${encodeURIComponent(id)}`, { method: 'DELETE' }),
}
