// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

import { api } from './client'
import type { Domain } from '$lib/types/domain'
import type { DocxTemplate, TemplateDescription } from '$lib/types/template'

// `type` (not interface) so it stays assignable to the client's
// Record<string, …> query parameter.
export type TemplateFilter = {
  domain?: Domain
  locale?: string
}

/** Wrappers for `src/routes/docx_templates.rs`. All require auth. */
export const templatesApi = {
  /** List DOCX templates, optionally filtered by domain + locale prefix. */
  list: (filter?: TemplateFilter) =>
    api<{ docx_templates: DocxTemplate[] }>('/docx-templates', { query: filter }),

  /** Full authoring contract (auto-generated prompt_md + sidecar). */
  describe: (template_id: string, locale?: string) =>
    api<TemplateDescription>('/docx-templates/describe', {
      method: 'POST',
      body: { template_id, locale },
    }),

  /**
   * Render a template to a .docx blob. The `X-Unresolved-Placeholders`
   * response header (CSV) is not surfaced here — a render dialog will
   * handle that when it's built.
   */
  render: (payload: {
    template_id: string
    body_md: string
    metadata?: Record<string, string>
    filename?: string
  }) => api<Blob>('/docx-templates/render', { method: 'POST', body: payload, asBlob: true }),
}
