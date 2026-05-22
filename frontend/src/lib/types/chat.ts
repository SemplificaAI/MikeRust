// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

/** Types mirroring `src/routes/chat.rs`. */

import type { Citation } from './citation'

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
  /** When true, the backend runs the file's extracted text through
   *  GLiNER2 PII redaction (`crate::ner::mask_pii`) before stuffing
   *  it into the LLM payload. Toggled by the per-file checkbox in
   *  the chat composer chip. Default false. */
  piiProtected?: boolean
}

/**
 * A chat message in the UI. `streaming` marks an assistant message that
 * is still receiving SSE deltas. Attachment fields are echoed back so
 * the user turn can show its chips after send.
 */
/**
 * A non-text step inside an assistant turn — a running tool or a
 * generated document. Rendered as an ordered "steps" block above the
 * answer text.
 */
export type ChatStep =
  | { kind: 'tool'; name: string; elapsedSecs: number; done: boolean }
  | { kind: 'doc'; filename: string; documentId: string; downloadUrl: string }
  /** read_document finished — typed render of the generic tool step. */
  | { kind: 'doc_read'; filename: string }
  /** find_in_document finished. */
  | { kind: 'doc_find'; query: string; filename: string; occurrences: number }
  /** read_workflow finished — a workflow was applied to this turn. */
  | { kind: 'workflow_applied'; title: string }

export interface ChatMessage {
  role: ChatRole
  content: string
  streaming?: boolean
  workflow?: WorkflowRef
  template?: TemplateRef
  files?: FileRef[]
  /** Resolved citations for an assistant message (from the SSE stream). */
  citations?: Citation[]
  /** Ordered tool / document steps for an assistant message. */
  steps?: ChatStep[]
  /** Model reasoning / "thinking" text — streamed, shown collapsed,
   *  not persisted (it is not part of the saved answer). */
  reasoning?: string
}

/** One message in the `POST /chat` request body. */
export interface OutgoingMessage {
  role: ChatRole
  content: string
  workflow?: WorkflowRef
  template?: TemplateRef
  /** Per-file flags travel snake_case to match the Rust handler. */
  files?: { document_id: string; pii_protected?: boolean }[]
}
