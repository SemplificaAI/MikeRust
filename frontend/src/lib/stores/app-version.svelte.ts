// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

/**
 * The bundle's semver, as reported by Tauri at runtime via
 * `@tauri-apps/api/app::getVersion`. Returned by the Tauri runtime as
 * the value of `version` in `src-tauri/tauri.svelte.conf.json`, so it
 * always matches the version we shipped — not what `package.json`
 * happens to say in a checked-out source tree.
 *
 * Resolved once at module load (Tauri is available the moment the
 * webview boots). Synchronous accessor returns `null` until the
 * promise resolves; callers should render an empty slot in that
 * window rather than block on it.
 */
import { getVersion } from '@tauri-apps/api/app'

let version = $state<string | null>(null)

void getVersion()
  .then((v) => {
    version = v
  })
  .catch(() => {
    // Web preview / unit-test environment — leave null, no fallback.
  })

export const appVersion = {
  get value(): string | null {
    return version
  },
}
