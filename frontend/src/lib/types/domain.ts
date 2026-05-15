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

/**
 * English display labels. Temporary until the i18n store lands — the
 * `Domains.*` namespace (plan §14) will localise these. Canonical IDs
 * never change; only these labels do.
 */
export const DOMAIN_LABELS: Record<Domain, string> = {
  legal: 'Legal',
  medical: 'Medical',
  finance: 'Finance',
  real_estate: 'Real estate',
  hr: 'HR',
  insurance: 'Insurance',
  ip: 'Intellectual property',
  compliance: 'Compliance',
  others: 'Other',
}

export function domainLabel(domain: string): string {
  return isDomain(domain) ? DOMAIN_LABELS[domain] : domain
}
