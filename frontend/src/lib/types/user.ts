// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

/** Types mirroring `src/routes/user.rs`. */

/** `GET /user/profile`. */
export interface UserProfile {
  id: string
  username: string
  display_name: string | null
  created_at: string
}

/** Supported UI locales. English is canonical (plan §14, decision Q8). */
export const LOCALES = ['en', 'it', 'fr', 'de', 'es', 'pt'] as const
export type Locale = (typeof LOCALES)[number]
export const DEFAULT_LOCALE: Locale = 'en'

export function isLocale(value: unknown): value is Locale {
  return typeof value === 'string' && (LOCALES as readonly string[]).includes(value)
}

/** Active LLM provider. */
export type LlmProvider = 'anthropic' | 'google' | 'openai' | 'local'

/**
 * `GET/PUT /user/llm-settings`. Every field is optional: PUT has patch
 * semantics (absent/null = unchanged; empty string = explicit clear).
 */
export interface LlmSettings {
  main_model?: string | null
  title_model?: string | null
  tabular_model?: string | null
  claude_api_key?: string | null
  gemini_api_key?: string | null
  gemini_region?: string | null
  gemini_model?: string | null
  openai_api_key?: string | null
  openai_model?: string | null
  local_base_url?: string | null
  local_api_key?: string | null
  local_model?: string | null
  active_provider?: LlmProvider | null
}

/** MCP server config — mirror of `McpServerOut` in user.rs. */
export type McpTransport = 'http' | 'sse' | 'stdio'

export interface McpServer {
  name: string
  transport: McpTransport
  url?: string
  command?: string
  args: string[]
  env: Record<string, unknown>
  headers: Record<string, unknown>
  api_key?: string
  enabled: boolean
}
