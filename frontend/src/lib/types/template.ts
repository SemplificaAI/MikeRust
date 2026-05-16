// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

import type { Domain } from './domain'

/**
 * Types mirroring `src/presets/docx_template.rs` (the `to_api_json()`
 * shape). Only the fields the Templates screen needs are typed
 * explicitly; the layout/typography sidecar fields are permitted via
 * the index signature so the full payload round-trips untyped.
 */

export type AutomationLevel = 'L1' | 'L2' | 'L3'

export interface SectionSkeletonEntry {
  id: string
  title?: string
  render?: string
  guidance?: string
  repeating?: boolean
}

export interface DocxTemplate {
  id: string
  /** locale code → display name; pick via templateDisplayName(). */
  display_name: Record<string, string>
  category: string
  domain: Domain
  also_applicable_to: Domain[]
  locale: string
  automation_level: AutomationLevel
  placeholder_syntax: string
  source_reference?: string
  required_metadata: string[]
  section_skeleton: SectionSkeletonEntry[]
  is_system: boolean
  is_owner: boolean
  /** layout/typography sidecar fields not needed by the list view. */
  [extra: string]: unknown
}

/** `POST /docx-templates/describe` response. */
export interface TemplateDescription {
  template_id: string
  display_name: string
  prompt_md: string
  sidecar: DocxTemplate
}

/**
 * Resolve a display name for the given UI locale. Falls back to the
 * `en` entry, then to any available entry, then to the raw id.
 */
export function templateDisplayName(t: DocxTemplate, locale: string): string {
  return (
    t.display_name[locale] ??
    t.display_name[locale.slice(0, 2)] ??
    t.display_name.en ??
    Object.values(t.display_name)[0] ??
    t.id
  )
}
