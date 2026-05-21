// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

import { api } from './client'
import type { Domain } from '$lib/types/domain'
import type { LlmSettings, Locale, McpServer, McpTransport, UserProfile } from '$lib/types/user'

/** Input for upsertMcpServer — every field but `name` is optional. */
export interface McpServerInput {
  name: string
  transport?: McpTransport
  url?: string
  command?: string
  args?: string[]
  env?: Record<string, unknown>
  headers?: Record<string, unknown>
  api_key?: string
  enabled?: boolean
}

/** Wrappers for `src/routes/user.rs`. All endpoints require auth. */
export const userApi = {
  getProfile: () => api<UserProfile>('/user/profile'),

  updateProfile: (display_name: string | null) =>
    api<{ ok: boolean }>('/user/profile', { method: 'PUT', body: { display_name } }),

  getLocale: () => api<{ locale: Locale | null }>('/user/locale'),

  updateLocale: (locale: Locale) =>
    api<{ ok: boolean; locale: Locale }>('/user/locale', { method: 'PUT', body: { locale } }),

  getDefaultDomain: () => api<{ default_domain: Domain | null }>('/user/default-domain'),

  updateDefaultDomain: (default_domain: Domain) =>
    api<{ ok: boolean; default_domain: Domain }>('/user/default-domain', {
      method: 'PUT',
      body: { default_domain },
    }),

  /** `null` = no explicit preference (every domain enabled). */
  getEnabledDomains: () =>
    api<{ enabled_domains: Domain[] | null }>('/user/enabled-domains'),

  updateEnabledDomains: (enabled_domains: Domain[]) =>
    api<{ ok: boolean; enabled_domains: Domain[] | null }>('/user/enabled-domains', {
      method: 'PUT',
      body: { enabled_domains },
    }),

  getLlmSettings: () => api<LlmSettings>('/user/llm-settings'),

  /** Patch semantics: omit fields to leave them unchanged. */
  updateLlmSettings: (patch: Partial<LlmSettings>) =>
    api<{ ok: boolean }>('/user/llm-settings', { method: 'PUT', body: patch }),

  listMcpServers: () => api<{ servers: McpServer[] }>('/user/mcp-servers'),

  /**
   * Create or update a server. Only `name` is strictly required — the
   * backend `UpsertMcpBody` applies serde defaults for transport (http)
   * and enabled (true) and treats the rest as optional.
   */
  upsertMcpServer: (server: McpServerInput) =>
    api<{ ok: boolean; name: string }>('/user/mcp-servers', { method: 'POST', body: server }),

  deleteMcpServer: (name: string) =>
    api<{ ok: boolean }>(`/user/mcp-servers/${encodeURIComponent(name)}`, { method: 'DELETE' }),

  probeMcpServer: (payload: { url: string; api_key?: string; headers?: Record<string, unknown> }) =>
    api<McpProbeResult>('/user/mcp-servers/probe', { method: 'POST', body: payload }),

  /** Irreversible — CASCADE-deletes all user data. */
  deleteAccount: () => api<{ ok: boolean }>('/user/account', { method: 'DELETE' }),
}

/** Shape of `POST /user/mcp-servers/probe` (success or transport hint). */
export interface McpProbeResult {
  ok: boolean
  transport_detected?: 'http' | 'sse'
  suggested_url?: string | null
  hint?: string
  server_info?: unknown
  capabilities?: unknown
  instructions?: string | null
  tools?: { name: string; description: string }[]
  tool_count?: number
  prompts?: { name: string; description: string; arguments: unknown[] }[]
  prompt_count?: number
  resources?: { uri: string; name: string; description: string; mimeType: string }[]
  resource_count?: number
}
