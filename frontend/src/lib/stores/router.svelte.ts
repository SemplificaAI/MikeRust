// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

/**
 * Minimal SPA router (plan §13). No history integration yet — the app
 * is a desktop shell, not a website. Pre-unlock routes (boot/setup/
 * unlock) and the post-unlock landing are all this phase needs; the
 * feature routes (assistant/projects/…) land in later phases.
 */
export type Route =
  | 'boot'
  | 'setup'
  | 'unlock'
  | 'home'

function createRouter() {
  let current = $state<Route>('boot')

  return {
    get current() {
      return current
    },
    go(route: Route) {
      current = route
    },
  }
}

export const router = createRouter()
