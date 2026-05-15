// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

/**
 * Professional verticals. Canonical English snake_case IDs — these are
 * the values that travel over the wire and live in the DB. Mirror of
 * `crate::domain::DOMAINS`. UI labels are localised via the i18n
 * `Domains.*` namespace; never translate the IDs themselves.
 */
export const DOMAINS = [
  'legal',
  'medical',
  'finance',
  'real_estate',
  'hr',
  'insurance',
  'ip',
  'compliance',
  'others',
] as const

export type Domain = (typeof DOMAINS)[number]

/** Per-row schema default (migration 0018) and frontend fallback. */
export const DEFAULT_DOMAIN: Domain = 'legal'

export function isDomain(value: unknown): value is Domain {
  return typeof value === 'string' && (DOMAINS as readonly string[]).includes(value)
}
