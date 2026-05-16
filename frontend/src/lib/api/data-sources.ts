// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

import { api } from './client'

/** Wrappers for `src/routes/sync.rs` and `src/routes/eurlex.rs`. */

// ── Local folder sync ────────────────────────────────────────────────

export interface SyncFolder {
  id: string
  path: string
  label: string | null
  recursive: boolean
  enabled: boolean
  last_scan_at: string | null
  project_id: string | null
}

export interface ScanStatus {
  status: 'idle' | 'running' | 'done' | 'failed'
  total?: number
  processed?: number
  indexed?: number
  skipped?: number
  failed?: number
  current_file?: string | null
  current_step?: string | null
  last_error?: string | null
}

export interface SyncedFile {
  path: string
  status: string
  document_id: string | null
  skip_reason: string | null
  size_bytes: number
  chunk_count: number
  indexed_at: string | null
  mtime: string | null
}

export type ModelStatus =
  | { state: 'idle' | 'loading' | 'ready' | 'unavailable' }
  | { state: 'downloading'; downloaded: number; total: number; file: string }
  | { state: 'failed'; error: string }

export const syncApi = {
  listFolders: () => api<SyncFolder[]>('/sync/folders'),

  addFolder: (body: { path: string; recursive?: boolean; label?: string; project_id?: string }) =>
    api<{ id: string }>('/sync/folders', { method: 'POST', body }),

  deleteFolder: (id: string) =>
    api<{ ok: boolean }>(`/sync/folders/${encodeURIComponent(id)}`, { method: 'DELETE' }),

  startScan: (id: string) =>
    api<{ started?: boolean; already_running?: boolean }>(
      `/sync/folders/${encodeURIComponent(id)}/scan`,
      { method: 'POST' },
    ),

  scanStatus: (id: string) =>
    api<ScanStatus>(`/sync/folders/${encodeURIComponent(id)}/status`),

  listFiles: (id: string) =>
    api<SyncedFile[]>(`/sync/folders/${encodeURIComponent(id)}/files`),

  modelStatus: () => api<ModelStatus>('/sync/model-status'),
}

// ── EUR-Lex corpus ───────────────────────────────────────────────────

export interface EurlexConfig {
  enabled: boolean
  language: string
  fallback_en: boolean
}

export interface CorpusHit {
  identifier: string
  title: string
  date: string | null
  url: string
  languages_available: string[]
}

export interface EurlexDocument {
  id: string
  filename: string
  corpus_identifier: string | null
  corpus_language: string | null
  fetched_with_fallback: boolean
  size_bytes: number
  created_at: string
  status: string
  chunks_indexed: number
  source_url: string | null
}

export interface EmbedProgress {
  document_id: string
  current: number
  total: number
  percent: number
}

export const eurlexApi = {
  getConfig: () => api<EurlexConfig>('/eurlex/config'),

  putConfig: (body: EurlexConfig) =>
    api<EurlexConfig>('/eurlex/config', { method: 'PUT', body }),

  search: (query: string, language?: string) =>
    api<{ hits: CorpusHit[]; note: string | null }>('/eurlex/search', {
      method: 'POST',
      body: { query, language },
    }),

  fetchCelex: (celex: string, language?: string) =>
    api<unknown>('/eurlex/fetch', { method: 'POST', body: { celex, language } }),

  listDocuments: () => api<{ documents: EurlexDocument[] }>('/eurlex/documents'),

  deleteDocument: (id: string) =>
    api<{ ok: boolean }>(`/eurlex/documents/${encodeURIComponent(id)}`, { method: 'DELETE' }),

  resyncDocument: (id: string) =>
    api<unknown>(`/eurlex/documents/${encodeURIComponent(id)}/resync`, { method: 'POST' }),

  embedProgress: () => api<EmbedProgress | null>('/eurlex/embed-progress'),
}
