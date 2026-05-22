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

  get: (id: string) => api<DocumentMeta>(`/document/${encodeURIComponent(id)}`),

  /**
   * Upload a document. `cache` flags a chat-composer upload (stored in
   * the cache pool, garbage-collected with the chat). The backend
   * extracts text and indexes synchronously, so the result is `ready`.
   */
  upload: (
    file: File,
    opts: { projectId?: string; cache?: boolean; domain?: Domain } = {},
  ) => {
    const fd = new FormData()
    fd.append('file', file)
    if (opts.projectId) fd.append('project_id', opts.projectId)
    if (opts.cache) fd.append('cache', 'true')
    if (opts.domain) fd.append('domain', opts.domain)
    return api<{
      id: string
      filename: string
      file_type: string
      size_bytes: number
      domain: Domain
      status: string
    }>('/document', { method: 'POST', multipart: fd })
  },

  remove: (id: string) =>
    api<{ ok: boolean }>(`/document/${encodeURIComponent(id)}`, { method: 'DELETE' }),

  /**
   * Fetch the displayable bytes of a document. The backend returns a
   * PDF rendition when one exists, otherwise the original bytes — the
   * caller inspects the resulting Blob's MIME type to pick a renderer.
   */
  displayBytes: (id: string) =>
    api<Blob>(`/document/${encodeURIComponent(id)}/display`, { asBlob: true }),

  /** Fetch the original document bytes for download. */
  downloadBytes: (id: string) =>
    api<Blob>(`/document/${encodeURIComponent(id)}/download`, { asBlob: true }),

  /**
   * Fetch bytes for a synced knowledge-base source path cited in chat
   * (`[gN]` / `[pN]`). The backend validates the allowlist of indexed
   * files before serving the payload.
   */
  kbBytes: (path: string) =>
    api<Blob>('/sync/kb-doc', { asBlob: true, query: { path } }),

  /**
   * Re-join a document's indexed chunks back into the original body.
   * Audio docs return per-segment `[T MM:SS]`-marked text alongside a
   * structured `segments` array with millisecond timestamps; the
   * AudioView uses those to render a clickable transcript that seeks
   * the `<audio>` element on click. Non-audio docs return
   * `segments: []` and `text` = joined chunks with overlap stripped —
   * useful as a generic "what got indexed" preview.
   */
  transcript: (id: string) =>
    api<TranscriptResponse>(`/document/${encodeURIComponent(id)}/transcript`),
}

export interface TranscriptSegment {
  start_ms: number
  end_ms: number
  text: string
}

export interface TranscriptResponse {
  text: string
  segments: TranscriptSegment[]
  duration_ms: number
}
