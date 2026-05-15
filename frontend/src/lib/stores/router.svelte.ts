// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

/**
 * Minimal SPA router (plan §13). No history integration yet — the app
 * is a desktop shell, not a website. Pre-unlock routes (boot/setup/
 * unlock) precede the authenticated feature routes.
 */
export type Route =
  | 'boot'
  | 'setup'
  | 'unlock'
  | 'assistant'
  | 'projects'
  | 'tabular'
  | 'workflows'
  | 'templates'
  | 'settings'

/** Feature routes — the ones rendered inside the authenticated Shell. */
export const FEATURE_ROUTES = [
  'assistant',
  'projects',
  'tabular',
  'workflows',
  'templates',
  'settings',
] as const
export type FeatureRoute = (typeof FEATURE_ROUTES)[number]

export function isFeatureRoute(route: Route): route is FeatureRoute {
  return (FEATURE_ROUTES as readonly string[]).includes(route)
}

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
