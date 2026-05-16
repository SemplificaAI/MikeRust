// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

/**
 * Citation processing for assistant messages. The model emits inline
 * markers (`[1]`, `[g2]`, `[p3]`, or comma groups) plus a trailing
 * machine-readable `<CITATIONS>` block. This module hides that block
 * and turns resolvable markers into clickable pills.
 */

import { renderMarkdown } from './markdown'
import { scopeForRef, type Citation } from '$lib/types/citation'

/**
 * Drop the trailing `<CITATIONS>…</CITATIONS>` block from displayed
 * text. Works on a partial block too — during streaming the opening
 * tag can arrive before the rest, and it must never flash on screen.
 */
export function stripCitationsBlock(text: string): string {
  return (text ?? '').replace(/<CITATIONS>[\s\S]*$/i, '').trimEnd()
}

/** A marker token: bare digits, or a `g`/`p` prefix then digits. */
const MARKER_GROUP = /\[((?:[gp]?\d{1,3})(?:\s*,\s*[gp]?\d{1,3})*)\]/g
const SINGLE_TOKEN = /[gp]?\d{1,3}/g

/** Tags whose text must not be rewritten (code, links, existing pills). */
const SKIP_ANCESTORS = new Set(['CODE', 'PRE', 'A'])

function hasSkippedAncestor(node: Node): boolean {
  let el = node.parentElement
  while (el) {
    if (SKIP_ANCESTORS.has(el.tagName)) return true
    if (el.hasAttribute('data-cite-ref')) return true
    el = el.parentElement
  }
  return false
}

function makePill(ref: string): HTMLElement {
  const scope = scopeForRef(ref)
  const pill = document.createElement('sup')
  pill.className = `cite-pill cite-${scope}`
  pill.dataset.citeRef = ref
  pill.setAttribute('role', 'button')
  pill.setAttribute('tabindex', '0')
  // Display the bare number; the prefix only encodes the pool.
  pill.textContent = ref.replace(/^[gp]/, '')
  return pill
}

/**
 * Render an assistant message body to sanitised HTML, with every
 * resolvable citation marker replaced by a clickable pill. Markers
 * with no matching citation are left as plain text; numbers of four or
 * more digits (years, amounts) are never treated as citations.
 */
export function renderMessageHtml(md: string, citations: Citation[] = []): string {
  const safeHtml = renderMarkdown(stripCitationsBlock(md))
  if (citations.length === 0) return safeHtml

  const known = new Set(citations.map((c) => c.ref))
  const host = document.createElement('div')
  host.innerHTML = safeHtml

  const walker = document.createTreeWalker(host, NodeFilter.SHOW_TEXT)
  const targets: Text[] = []
  let n: Node | null
  while ((n = walker.nextNode())) {
    const t = n as Text
    if (t.data.includes('[') && !hasSkippedAncestor(t)) targets.push(t)
  }

  for (const textNode of targets) {
    const src = textNode.data
    MARKER_GROUP.lastIndex = 0
    if (!MARKER_GROUP.test(src)) continue

    MARKER_GROUP.lastIndex = 0
    const frag = document.createDocumentFragment()
    let cursor = 0
    let m: RegExpExecArray | null
    while ((m = MARKER_GROUP.exec(src))) {
      const tokens = m[1].match(SINGLE_TOKEN) ?? []
      const resolvable = tokens.filter((tk) => known.has(tk))
      if (resolvable.length === 0) continue // leave it as plain text

      if (m.index > cursor) {
        frag.appendChild(document.createTextNode(src.slice(cursor, m.index)))
      }
      tokens.forEach((tk, i) => {
        if (i > 0) frag.appendChild(document.createTextNode(' '))
        if (known.has(tk)) frag.appendChild(makePill(tk))
        else frag.appendChild(document.createTextNode(tk))
      })
      cursor = m.index + m[0].length
    }
    if (cursor === 0) continue // nothing was rewritten
    if (cursor < src.length) {
      frag.appendChild(document.createTextNode(src.slice(cursor)))
    }
    textNode.parentNode?.replaceChild(frag, textNode)
  }

  return host.innerHTML
}
