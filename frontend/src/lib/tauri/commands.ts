// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

import { invoke } from '@tauri-apps/api/core'

/**
 * Discover the axum backend base URL.
 *
 * The Tauri shell spawns the backend on a free port at startup and reports it
 * back via the `api_base_url` command. If we're not running inside Tauri (e.g.
 * `vite dev` opened in a regular browser for component dev), the command will
 * throw — fall through to the `VITE_API_BASE_URL` env var, then to the
 * canonical localhost default.
 */
export async function getApiBaseUrl(): Promise<string> {
  try {
    const u = await invoke<string>('api_base_url')
    if (u) return u
  } catch {
    // not in Tauri context, fall through
  }
  return import.meta.env.VITE_API_BASE_URL ?? 'http://127.0.0.1:3001'
}

/**
 * Open an external http(s) URL in the user's default browser instead of
 * letting the Tauri WebView intercept it. Throws on any non-http(s) input.
 */
export async function openExternal(url: string): Promise<void> {
  if (!/^https?:\/\//.test(url)) {
    throw new Error('openExternal: only http(s) URLs allowed')
  }
  await invoke('open_external_url', { url })
}

/**
 * Open the native folder picker. Returns the chosen absolute path, or
 * null if the user cancelled or we are not running inside Tauri.
 */
export async function pickFolder(): Promise<string | null> {
  try {
    return (await invoke<string | null>('pick_folder')) ?? null
  } catch {
    return null
  }
}
