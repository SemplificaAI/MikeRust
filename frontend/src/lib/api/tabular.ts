// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

import { api } from './client'
import { apiBase } from '$lib/stores/api-base.svelte'
import { authStore } from '$lib/stores/auth.svelte'
import type { Domain } from '$lib/types/domain'
import type { CreateTabularReviewBody, TabularReview } from '$lib/types/tabular'
import type { WorkflowColumn } from '$lib/types/workflow'

// `type` (not interface) — assignable to the client's query Record.
export type TabularFilter = {
  project_id?: string
  domain?: Domain
}

/** Body for `PATCH /tabular-review/{id}`. */
export interface PatchTabularBody {
  title?: string
  columns_config?: WorkflowColumn[]
  /** Reconciles the review's attached documents to exactly this set. */
  document_ids?: string[]
}

/** Wrappers for `src/routes/tabular_reviews.rs`. All require auth. */
export const tabularApi = {
  /** GET /tabular-review — returns a bare array. */
  list: (filter?: TabularFilter) =>
    api<TabularReview[]>('/tabular-review', { query: filter }),

  /** GET /tabular-review/{id} — review metadata + document rows + cells. */
  get: (id: string) => api<TabularReview>(`/tabular-review/${encodeURIComponent(id)}`),

  create: (body: CreateTabularReviewBody) =>
    api<{ id: string; title: string; domain: Domain }>('/tabular-review', {
      method: 'POST',
      body,
    }),

  /** Update title / columns / attached documents. Returns the fresh review. */
  patch: (id: string, body: PatchTabularBody) =>
    api<TabularReview>(`/tabular-review/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body,
    }),

  remove: (id: string) =>
    api<{ ok: boolean }>(`/tabular-review/${encodeURIComponent(id)}`, { method: 'DELETE' }),

  /** Regenerate a single cell; returns its new status + content. */
  regenerateCell: (id: string, row_id: string, column_key: string) =>
    api<{ row_id: string; column_key: string; status: string; content: string }>(
      `/tabular-review/${encodeURIComponent(id)}/regenerate-cell`,
      { method: 'POST', body: { row_id, column_key } },
    ),

  /** Reset cells to pending — all rows, or just the given ones. */
  clearCells: (id: string, row_ids?: string[]) =>
    api<{ ok: boolean }>(`/tabular-review/${encodeURIComponent(id)}/clear-cells`, {
      method: 'POST',
      body: { row_ids },
    }),

  /** Import a spreadsheet — one review per worksheet. */
  importXlsx: async (
    file: File,
  ): Promise<{ reviews: { id: string; title: string }[] }> => {
    const res = await fetch(
      new URL('/tabular-review/import', apiBase.url || 'http://127.0.0.1:3001'),
      {
        method: 'POST',
        headers: { Authorization: `Bearer ${authStore.token ?? ''}` },
        body: await file.arrayBuffer(),
      },
    )
    if (!res.ok) {
      let detail = `import failed (${res.status})`
      try {
        detail = ((await res.json()) as { detail?: string }).detail ?? detail
      } catch {
        /* keep the generic message */
      }
      throw new Error(detail)
    }
    return res.json() as Promise<{ reviews: { id: string; title: string }[] }>
  },

  /** Download the review grid as an .xlsx blob. */
  exportXlsx: async (id: string): Promise<Blob> => {
    const res = await fetch(
      new URL(
        `/tabular-review/${encodeURIComponent(id)}/export`,
        apiBase.url || 'http://127.0.0.1:3001',
      ),
      { headers: { Authorization: `Bearer ${authStore.token ?? ''}` } },
    )
    if (!res.ok) throw new Error(`export failed (${res.status})`)
    return res.blob()
  },
}

export interface GenerateCallbacks {
  onCell: (rowId: string, columnKey: string, status: string, content: string) => void
  onError: (message: string) => void
  onDone: () => void
}

/**
 * Stream a review run. POSTs to `/tabular-review/{id}/generate` and
 * parses the `data: {type}` SSE stream — `cell_update` events update
 * one cell, `done` ends the run. Returns an AbortController.
 */
export function streamGenerate(id: string, cb: GenerateCallbacks): AbortController {
  const ctrl = new AbortController()

  void (async () => {
    let res: Response
    try {
      res = await fetch(
        new URL(
          `/tabular-review/${encodeURIComponent(id)}/generate`,
          apiBase.url || 'http://127.0.0.1:3001',
        ),
        {
          method: 'POST',
          headers: {
            Accept: 'text/event-stream',
            Authorization: `Bearer ${authStore.token ?? ''}`,
          },
          signal: ctrl.signal,
        },
      )
    } catch (e) {
      const err = e as Error
      if (err.name !== 'AbortError') {
        // v0.6.2 diagnostic hook: surface tabular cell errors in
        // the DevTools console too. The tabular UI renders a small
        // red exclamation per failed cell which doesn't carry the
        // backend error string; the console.warn is where the user
        // can triage 429 storms vs auth failures vs network drops.
        console.warn('[tabular] generate stream failed:', err.message, {
          reviewId: id,
        })
        cb.onError(err.message)
      }
      cb.onDone()
      return
    }

    if (!res.ok || !res.body) {
      let detail = `stream failed (${res.status})`
      try {
        const j = (await res.json()) as { detail?: string }
        if (j.detail) detail = j.detail
      } catch {
        /* keep status */
      }
      cb.onError(detail)
      cb.onDone()
      return
    }

    const reader = res.body.getReader()
    const decoder = new TextDecoder()
    let buf = ''
    const dispatch = (chunk: string) => {
      for (const line of chunk.split('\n')) {
        const l = line.replace(/^\s+/, '')
        if (!l.startsWith('data:')) continue
        const data = l.slice(5).trim()
        if (!data || data === '[DONE]') continue
        let ev: Record<string, unknown>
        try {
          ev = JSON.parse(data)
        } catch {
          continue
        }
        if (ev.type === 'cell_update') {
          cb.onCell(
            String(ev.row_id ?? ''),
            String(ev.column_key ?? ''),
            String(ev.status ?? ''),
            String(ev.content ?? ''),
          )
        } else if (ev.type === 'error') {
          // v0.6.2 diagnostic hook: log the backend SSE error event
          // verbatim with the review id so the user can correlate
          // failed cell pills with their backend cause (typically
          // Mistral 429 cascade or auth failure).
          console.warn('[tabular] stream error event:', ev.message, {
            reviewId: id,
            raw: ev,
          })
          cb.onError(String(ev.message ?? 'stream error'))
        }
      }
    }
    try {
      for (;;) {
        const { value, done } = await reader.read()
        if (done) break
        buf += decoder.decode(value, { stream: true })
        let idx: number
        while ((idx = buf.indexOf('\n\n')) >= 0) {
          dispatch(buf.slice(0, idx))
          buf = buf.slice(idx + 2)
        }
      }
      if (buf.trim()) dispatch(buf)
    } catch (e) {
      if ((e as Error).name !== 'AbortError') cb.onError((e as Error).message)
    }
    cb.onDone()
  })()

  return ctrl
}
