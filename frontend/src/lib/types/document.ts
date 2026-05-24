// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

import type { Domain } from './domain'

/** Document metadata from `GET /document` (src/routes/documents.rs). */
export interface DocumentMeta {
  id: string
  filename: string
  file_type: string
  size_bytes: number
  status: string
  domain: Domain
  created_at: string
  /** Folder within the project (`GET /document`). null = project root. */
  project_folder_id?: string | null
  /** Per-chat accept/reject state (migration 0029). Defaults to
   *  `accepted` for legacy rows. */
  decision?: 'accepted' | 'rejected'
  decision_reason?: string | null
  decision_summary?: string | null
}
