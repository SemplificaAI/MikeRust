// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

/**
 * Light / dark / system theme (plan §4).
 *
 * The chosen mode maps to a class on <html> consumed by app.css:
 *   light  → .theme-light  (no overrides)
 *   dark   → .theme-dark   (always dark)
 *   system → .theme-system (dark only when the OS prefers dark)
 *
 * Persistence uses localStorage on purpose: theme is a device-local
 * display preference (a user may want dark on a laptop, light on a
 * desktop), not portable account data — so it does not belong on a
 * /user/* endpoint. The value is non-sensitive.
 */
export type ThemeMode = 'light' | 'dark' | 'system'

const STORAGE_KEY = 'mikerust.theme'
const MODES: ThemeMode[] = ['light', 'dark', 'system']

function isThemeMode(v: unknown): v is ThemeMode {
  return typeof v === 'string' && (MODES as string[]).includes(v)
}

function createThemeStore() {
  let mode = $state<ThemeMode>('system')

  function applyClass() {
    const html = document.documentElement
    html.classList.remove('theme-light', 'theme-dark', 'theme-system')
    html.classList.add(`theme-${mode}`)
  }

  return {
    get mode() {
      return mode
    },

    /** Read the persisted choice and apply it. Call once at startup. */
    init() {
      try {
        const saved = localStorage.getItem(STORAGE_KEY)
        if (isThemeMode(saved)) mode = saved
      } catch {
        // localStorage unavailable — fall back to 'system'
      }
      applyClass()
    },

    set(next: ThemeMode) {
      mode = next
      applyClass()
      try {
        localStorage.setItem(STORAGE_KEY, next)
      } catch {
        // non-fatal — the in-memory choice still applies this session
      }
    },
  }
}

export const themeStore = createThemeStore()
