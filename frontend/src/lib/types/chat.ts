// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

/** Types mirroring `src/routes/chat.rs`. */

export type ChatRole = 'user' | 'assistant'

/** Chat row from `GET /chat`. */
export interface Chat {
  id: string
  user_id: string
  project_id: string | null
  title: string | null
  updated_at: string
}

/** Reference chips a user message can carry. */
export interface WorkflowRef {
  id: string
  title: string
}
export interface TemplateRef {
  id: string
  title: string
}
export interface FileRef {
  document_id: string
  /** client-side only — for rendering the attachment chip. */
  filename?: string
}

/**
 * A chat message in the UI. `streaming` marks an assistant message that
 * is still receiving SSE deltas. Attachment fields are echoed back so
 * the user turn can show its chips after send.
 */
export interface ChatMessage {
  role: ChatRole
  content: string
  streaming?: boolean
  workflow?: WorkflowRef
  template?: TemplateRef
  files?: FileRef[]
}

/** One message in the `POST /chat` request body. */
export interface OutgoingMessage {
  role: ChatRole
  content: string
  workflow?: WorkflowRef
  template?: TemplateRef
  files?: { document_id: string }[]
}
