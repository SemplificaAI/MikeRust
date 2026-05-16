<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  PDF renderer. Renders every page to a canvas with a transparent,
  selectable text layer over it (so cited passages can be highlighted
  and text can be copied). Zoom via buttons or ctrl/⌘ + wheel.
-->
<script lang="ts">
  import { getDocument, TextLayer } from '$lib/utils/pdf'
  import { highlightCitation } from '$lib/utils/highlight'
  import { PAGE_BREAK_SENTINEL } from '$lib/types/citation'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { ZoomIn, ZoomOut } from 'lucide-svelte'

  interface Props {
    bytes: Uint8Array
    quote?: string
    page?: number | string
    /** Bumped by the store when an open tab is re-targeted. */
    revision?: number
  }

  let { bytes, quote, page, revision = 0 }: Props = $props()

  let scale = $state(1.3)
  let numPages = $state(0)
  let currentPage = $state(1)
  let loading = $state(true)
  let err = $state<string | null>(null)
  let scrollEl: HTMLDivElement
  let pagesEl: HTMLDivElement
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let pdfDoc: any = null
  let renderToken = 0

  function pageHint(): number | null {
    if (page == null) return null
    const n = typeof page === 'number' ? page : parseInt(String(page), 10)
    return Number.isFinite(n) ? n : null
  }

  async function renderAll() {
    const token = ++renderToken
    if (!pagesEl || !pdfDoc) return
    pagesEl.innerHTML = ''
    for (let p = 1; p <= numPages; p++) {
      if (token !== renderToken) return
      const pageObj = await pdfDoc.getPage(p)
      const viewport = pageObj.getViewport({ scale })

      const wrap = document.createElement('div')
      wrap.className = 'pdf-page relative shadow-(--shadow-card) bg-white'
      wrap.style.width = `${viewport.width}px`
      wrap.style.height = `${viewport.height}px`
      wrap.style.margin = '0 auto 12px'
      wrap.dataset.page = String(p)

      const canvas = document.createElement('canvas')
      canvas.width = viewport.width
      canvas.height = viewport.height
      canvas.style.width = `${viewport.width}px`
      canvas.style.height = `${viewport.height}px`
      wrap.appendChild(canvas)

      const textDiv = document.createElement('div')
      textDiv.className = 'pdf-text-layer'
      textDiv.style.setProperty('--scale-factor', String(scale))
      textDiv.style.width = `${viewport.width}px`
      textDiv.style.height = `${viewport.height}px`
      wrap.appendChild(textDiv)

      pagesEl.appendChild(wrap)

      const ctx = canvas.getContext('2d')
      if (ctx) await pageObj.render({ canvasContext: ctx, viewport }).promise

      const layer = new TextLayer({
        textContentSource: pageObj.streamTextContent(),
        container: textDiv,
        viewport,
      })
      await layer.render()
    }
    if (token === renderToken) applyHighlight()
  }

  function applyHighlight() {
    if (!quote || !pagesEl) return
    const hint = pageHint()
    const order: HTMLElement[] = []
    const all = Array.from(pagesEl.querySelectorAll<HTMLElement>('.pdf-text-layer'))
    if (hint && all[hint - 1]) order.push(all[hint - 1])
    for (const l of all) if (!order.includes(l)) order.push(l)

    for (const layer of order) {
      const mark = highlightCitation(layer, quote, PAGE_BREAK_SENTINEL)
      if (mark) {
        mark.classList.add('doc-hl-flash')
        mark.scrollIntoView({ block: 'center', behavior: 'smooth' })
        return
      }
    }
    // Quote not found anywhere — at least jump to the cited page.
    if (hint) {
      const pageEl = pagesEl.querySelector<HTMLElement>(`.pdf-page[data-page="${hint}"]`)
      pageEl?.scrollIntoView({ block: 'start', behavior: 'smooth' })
    }
  }

  async function load() {
    loading = true
    err = null
    try {
      // getDocument takes ownership of the buffer — hand it a copy.
      pdfDoc = await getDocument({ data: bytes.slice() }).promise
      numPages = pdfDoc.numPages
      await renderAll()
    } catch (e) {
      err = (e as Error).message
    } finally {
      loading = false
    }
  }

  function zoom(delta: number) {
    scale = Math.min(4, Math.max(0.4, Math.round((scale + delta) * 100) / 100))
  }

  function onWheel(e: WheelEvent) {
    if (!(e.ctrlKey || e.metaKey)) return
    e.preventDefault()
    zoom(e.deltaY < 0 ? 0.15 : -0.15)
  }

  function onScroll() {
    if (!pagesEl || !scrollEl) return
    const mid = scrollEl.scrollTop + scrollEl.clientHeight / 2
    const pages = Array.from(pagesEl.querySelectorAll<HTMLElement>('.pdf-page'))
    for (const pg of pages) {
      if (pg.offsetTop <= mid && pg.offsetTop + pg.offsetHeight >= mid) {
        currentPage = Number(pg.dataset.page)
        break
      }
    }
  }

  // Initial load + reload when the document bytes change.
  $effect(() => {
    void bytes
    void load()
  })

  // Re-render on zoom.
  let firstScale = true
  $effect(() => {
    void scale
    if (firstScale) {
      firstScale = false
      return
    }
    void renderAll()
  })

  // Re-run the highlight pass when the tab is re-targeted to a new quote.
  $effect(() => {
    void revision
    void quote
    if (!loading) applyHighlight()
  })
</script>

<div class="flex flex-col h-full min-h-0">
  <div
    bind:this={scrollEl}
    onscroll={onScroll}
    onwheel={onWheel}
    class="flex-1 min-h-0 overflow-auto bg-(--color-surface-100) p-4"
  >
    {#if loading}
      <div class="flex items-center justify-center gap-2 py-12 text-sm text-(--color-text-secondary)">
        <Spinner size="sm" />
        {i18n.t('Documents.viewer.loadingDocument')}
      </div>
    {:else if err}
      <p class="text-sm text-(--color-danger-500) py-12 text-center">
        {i18n.t('Documents.viewer.errorLoading')} — {err}
      </p>
    {/if}
    <div bind:this={pagesEl}></div>
  </div>

  {#if !loading && !err}
    <div class="flex items-center justify-between px-3 h-9 shrink-0 border-t border-(--color-surface-200) text-xs text-(--color-text-secondary)">
      <span>{i18n.t('Documents.viewer.page')} {currentPage} {i18n.t('Documents.viewer.of')} {numPages}</span>
      <div class="flex items-center gap-1">
        <button
          type="button"
          class="p-1 rounded hover:bg-(--color-hover-bg)"
          aria-label={i18n.t('Documents.viewer.zoomOut')}
          onclick={() => zoom(-0.15)}
        >
          <ZoomOut size={14} />
        </button>
        <span class="tabular-nums w-10 text-center">{Math.round(scale * 100)}%</span>
        <button
          type="button"
          class="p-1 rounded hover:bg-(--color-hover-bg)"
          aria-label={i18n.t('Documents.viewer.zoomIn')}
          onclick={() => zoom(0.15)}
        >
          <ZoomIn size={14} />
        </button>
      </div>
    </div>
  {/if}
</div>
