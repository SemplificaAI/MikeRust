// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

/**
 * Document-viewer store. Drives the resizable side panel that holds one
 * browser-style tab per opened document. A citation pill, a generated-
 * document card or a "read document" step all funnel through here; the
 * panel reuses an existing tab for the same document and only refreshes
 * its highlight/page hint.
 */

import type { Citation } from '$lib/types/citation'

/** How a tab was opened — drives the per-tab header card. */
export type ViewerMode = 'citation' | 'plain'

export interface ViewerTab {
  /** Unique per tab (a document may be reopened from several places). */
  id: string
  /** Backing document identifier. */
  docId: string
  /** Filename / label shown on the tab. */
  title: string
  mode: ViewerMode
  /** Passage to highlight inside the rendered document, if any. */
  quote?: string
  /** Page hint (number) or `"41-42"` range string. */
  page?: number | string
  /** Source label for the citation header card. */
  citationSource?: string
}

interface OpenOptions {
  docId: string
  title: string
  mode?: ViewerMode
  quote?: string
  page?: number | string
  citationSource?: string
}

const MIN_WIDTH = 360
const MAX_WIDTH = 1100
const DEFAULT_WIDTH = 600

function createDocViewer() {
  let tabs = $state<ViewerTab[]>([])
  let activeId = $state<string | null>(null)
  let open = $state(false)
  let collapsed = $state(false)
  let width = $state(DEFAULT_WIDTH)
  /** Bumped whenever an existing tab is re-targeted, so the view re-runs
   *  its highlight pass even though the tab object identity is stable. */
  let revision = $state(0)

  function open_(opts: OpenOptions) {
    const existing = tabs.find((t) => t.docId === opts.docId)
    if (existing) {
      existing.mode = opts.mode ?? existing.mode
      existing.quote = opts.quote
      existing.page = opts.page
      existing.citationSource = opts.citationSource
      activeId = existing.id
      revision++
    } else {
      const tab: ViewerTab = {
        id:
          typeof crypto !== 'undefined' && crypto.randomUUID
            ? crypto.randomUUID()
            : `tab-${Date.now()}-${tabs.length}`,
        docId: opts.docId,
        title: opts.title,
        mode: opts.mode ?? 'plain',
        quote: opts.quote,
        page: opts.page,
        citationSource: opts.citationSource,
      }
      tabs = [...tabs, tab]
      activeId = tab.id
    }
    open = true
    collapsed = false
  }

  return {
    get tabs() {
      return tabs
    },
    get activeId() {
      return activeId
    },
    get activeTab() {
      return tabs.find((t) => t.id === activeId) ?? null
    },
    get open() {
      return open
    },
    get collapsed() {
      return collapsed
    },
    get width() {
      return width
    },

    /** Collapse the panel to a thin strip, or restore the prior width. */
    toggleCollapse() {
      collapsed = !collapsed
    },
    get revision() {
      return revision
    },

    /** Open (or re-target) a plain document view. */
    openDocument(docId: string, title: string) {
      open_({ docId, title, mode: 'plain' })
    },

    /** Open a citation: highlights the quoted passage on the cited page. */
    openCitation(c: Citation) {
      open_({
        docId: c.docId,
        title: c.source || c.docId,
        mode: 'citation',
        quote: c.quote,
        page: c.page,
        citationSource: c.source,
      })
    },

    select(id: string) {
      if (tabs.some((t) => t.id === id)) {
        activeId = id
        open = true
      }
    },

    closeTab(id: string) {
      const idx = tabs.findIndex((t) => t.id === id)
      if (idx < 0) return
      tabs = tabs.filter((t) => t.id !== id)
      if (activeId === id) {
        activeId = tabs[Math.min(idx, tabs.length - 1)]?.id ?? null
      }
      if (tabs.length === 0) open = false
    },

    closeAll() {
      tabs = []
      activeId = null
      open = false
    },

    setWidth(px: number) {
      width = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, Math.round(px)))
    },
  }
}

export const docViewer = createDocViewer()
