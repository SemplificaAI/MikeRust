// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

import { api } from './client'
import type { Domain } from '$lib/types/domain'
import type {
  CreateProjectBody,
  Project,
  ProjectDetail,
  UpdateProjectBody,
} from '$lib/types/project'

// `type` (not interface) — assignable to the client's query Record.
export type ProjectFilter = {
  domain?: Domain
}

/** Wrappers for `src/routes/projects.rs`. All require auth. */
export const projectsApi = {
  list: (filter?: ProjectFilter) =>
    api<{ projects: Project[] }>('/project', { query: filter }),

  get: (id: string) => api<ProjectDetail>(`/project/${encodeURIComponent(id)}`),

  create: (body: CreateProjectBody) =>
    api<{ id: string; name: string; domain: Domain }>('/project', {
      method: 'POST',
      body,
    }),

  update: (id: string, body: UpdateProjectBody) =>
    api<{ ok: boolean }>(`/project/${encodeURIComponent(id)}`, { method: 'PUT', body }),

  remove: (id: string) =>
    api<{ ok: boolean }>(`/project/${encodeURIComponent(id)}`, { method: 'DELETE' }),

  /** Export to an encrypted .mikeprj blob (UI wiring is a later phase). */
  exportProject: (id: string, recipient_email: string, include_chats = false) =>
    api<Blob>(`/project/${encodeURIComponent(id)}/export`, {
      method: 'POST',
      body: { recipient_email, include_chats },
      asBlob: true,
    }),

  /** Import a .mikeprj blob (UI wiring is a later phase). */
  importProject: (file: File, recipient_email: string) => {
    const fd = new FormData()
    fd.append('file', file)
    fd.append('recipient_email', recipient_email)
    return api<{ ok: boolean; project_id: string; document_count: number; chat_count: number }>(
      '/project/import',
      { method: 'POST', multipart: fd },
    )
  },
}
