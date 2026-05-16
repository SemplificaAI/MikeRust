// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

import { api } from './client'
import { apiBase } from '$lib/stores/api-base.svelte'
import { authStore } from '$lib/stores/auth.svelte'
import type { Chat, ChatMessage, OutgoingMessage } from '$lib/types/chat'

/** Non-streaming wrappers for `src/routes/chat.rs`. */
export const chatApi = {
  list: () => api<{ chats: Chat[] }>('/chat'),

  /** Create an empty chat record (no messages). */
  createRecord: (project_id?: string, title?: string) =>
    api<{ id: string }>('/chat', { method: 'POST', body: { project_id, title } }),

  messages: (id: string) =>
    api<{ messages: { id: string; role: string; content: string; created_at: string }[] }>(
      `/chat/${encodeURIComponent(id)}/messages`,
    ),

  remove: (id: string) =>
    api<{ ok: boolean }>(`/chat/${encodeURIComponent(id)}`, { method: 'DELETE' }),

  rename: (id: string, title: string) =>
    api<unknown>(`/chat/${encodeURIComponent(id)}`, { method: 'PATCH', body: { title } }),
}

export interface DocCreatedEvent {
  filename: string
  documentId: string
  downloadUrl: string
}

export interface ChatStreamCallbacks {
  onChatId?: (chatId: string) => void
  onDelta: (text: string) => void
  onToolCallStart?: (name: string) => void
  onToolCallProgress?: (name: string, elapsedSecs: number) => void
  onDocCreated?: (doc: DocCreatedEvent) => void
  onCitations?: (data: unknown) => void
  onError: (message: string) => void
  onDone: () => void
}

function dispatchSseChunk(chunk: string, cb: ChatStreamCallbacks): void {
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
    switch (ev.type) {
      case 'chat_id':
        cb.onChatId?.(String(ev.chatId ?? ''))
        break
      case 'content_delta':
      case 'content':
        cb.onDelta(String(ev.text ?? ''))
        break
      case 'tool_call_start':
        cb.onToolCallStart?.(String(ev.name ?? ''))
        break
      case 'tool_call_progress':
        cb.onToolCallProgress?.(String(ev.name ?? ''), Number(ev.elapsed_secs ?? 0))
        break
      case 'doc_created':
        cb.onDocCreated?.({
          filename: String(ev.filename ?? ''),
          documentId: String(ev.document_id ?? ''),
          downloadUrl: String(ev.download_url ?? ''),
        })
        break
      case 'citations':
        cb.onCitations?.(ev)
        break
      case 'error':
        cb.onError(String(ev.message ?? 'stream error'))
        break
    }
  }
}

/**
 * Stream an assistant reply. POSTs the conversation to `/chat`
 * (the rich SSE path: handles document/workflow/template attachments)
 * and parses the `data: {type}` event stream. Uses fetch + ReadableStream
 * because EventSource cannot send the Authorization header.
 * Returns an AbortController so the caller can stop generation.
 */
export function streamChat(
  payload: { messages: OutgoingMessage[]; chat_id?: string; model?: string },
  cb: ChatStreamCallbacks,
): AbortController {
  const ctrl = new AbortController()

  void (async () => {
    let res: Response
    try {
      res = await fetch(new URL('/chat', apiBase.url || 'http://127.0.0.1:3001'), {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Accept: 'text/event-stream',
          Authorization: `Bearer ${authStore.token ?? ''}`,
        },
        body: JSON.stringify(payload),
        signal: ctrl.signal,
      })
    } catch (e) {
      if ((e as Error).name !== 'AbortError') cb.onError((e as Error).message)
      cb.onDone()
      return
    }

    if (!res.ok || !res.body) {
      cb.onError(`stream failed (${res.status})`)
      cb.onDone()
      return
    }

    const reader = res.body.getReader()
    const decoder = new TextDecoder()
    let buf = ''
    try {
      for (;;) {
        const { value, done } = await reader.read()
        if (done) break
        buf += decoder.decode(value, { stream: true })
        let idx: number
        while ((idx = buf.indexOf('\n\n')) >= 0) {
          dispatchSseChunk(buf.slice(0, idx), cb)
          buf = buf.slice(idx + 2)
        }
      }
      if (buf.trim()) dispatchSseChunk(buf, cb)
    } catch (e) {
      if ((e as Error).name !== 'AbortError') cb.onError((e as Error).message)
    }
    cb.onDone()
  })()

  return ctrl
}

export type { ChatMessage }
