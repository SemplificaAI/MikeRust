// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

import type { Domain } from './domain'

/** Types mirroring `src/routes/workflows.rs`. */

export type WorkflowType = 'assistant' | 'tabular'

/**
 * Per-column config for `tabular` workflows. The backend stores
 * `columns_config` as opaque JSON (preset-defined); kept permissive
 * here — the tabular screen will refine this when it's built.
 */
export interface WorkflowColumn {
  key?: string
  label?: string
  prompt?: string
  format?: string
  [extra: string]: unknown
}

export interface Workflow {
  id: string
  /** null for system presets, a user id for DB-stored workflows. */
  user_id: string | null
  title: string
  type: WorkflowType
  prompt_md: string | null
  columns_config: WorkflowColumn[]
  practice: string | null
  domain: Domain
  created_at: string
  /** true = shipped preset (config/workflows/*), not editable. */
  is_system: boolean
  /** true = the current user owns this DB row. */
  is_owner: boolean
}

// A `type` (not `interface`) so it stays assignable to the API client's
// `Record<string, …>` query parameter — interfaces lack an implicit
// index signature and TS rejects them there.
export type WorkflowFilter = {
  type?: WorkflowType
  domain?: Domain
}
