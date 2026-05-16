<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Document-viewer side panel: a resizable column on the right with one
  browser-style tab per opened document. Picks a renderer per file type
  (PDF / DOCX / spreadsheet / text) and shows a per-tab header card for
  citations. All rendering is in-browser JS — no native plugin.
-->
<script lang="ts">
  import { docViewer, type ViewerTab } from '$lib/stores/doc-viewer.svelte'
  import { documentsApi } from '$lib/api/documents'
  import PdfView from './PdfView.svelte'
  import DocxView from './DocxView.svelte'
  import SheetView from './SheetView.svelte'
  import TextView from './TextView.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { X, Download, Quote, PanelRightClose, PanelRightOpen } from 'lucide-svelte'

  type RendererKind = 'pdf' | 'docx' | 'sheet' | 'md' | 'rtf' | 'text' | 'unsupported'

  interface Loaded {
    kind: RendererKind
    blob: Blob
    bytes: Uint8Array
    text: string
  }

  let loading = $state(false)
  let loadError = $state<string | null>(null)
  let loaded = $state<Loaded | null>(null)
  let loadedDocId: string | null = null

  const cache = new Map<string, Loaded>()

  const activeTab = $derived(docViewer.activeTab)

  function extOf(name: string): string {
    const m = /\.([a-z0-9]+)$/i.exec(name.trim())
    return m ? m[1].toLowerCase() : ''
  }

  function rendererFor(blob: Blob, filename: string): RendererKind {
    const t = blob.type.toLowerCase()
    const ext = extOf(filename)
    if (t.includes('pdf') || ext === 'pdf') return 'pdf'
    if (t.includes('wordprocessing') || t.includes('msword') || ext === 'docx' || ext === 'doc')
      return 'docx'
    if (
      t.includes('spreadsheet') ||
      t.includes('excel') ||
      ['xlsx', 'xls', 'xlsb', 'ods', 'csv'].includes(ext)
    )
      return 'sheet'
    if (ext === 'md' || ext === 'markdown') return 'md'
    if (ext === 'rtf' || t.includes('rtf')) return 'rtf'
    if (t.startsWith('text/') || ['txt', 'log'].includes(ext)) return 'text'
    // A PDF rendition with a stale extension still sniffs as PDF above;
    // anything left is genuinely unknown.
    return 'unsupported'
  }

  async function loadActive(tab: ViewerTab) {
    if (loadedDocId === tab.docId && loaded) return
    const cached = cache.get(tab.docId)
    if (cached) {
      loaded = cached
      loadedDocId = tab.docId
      return
    }
    loading = true
    loadError = null
    loaded = null
    try {
      const blob = await documentsApi.displayBytes(tab.docId)
      const buf = new Uint8Array(await blob.arrayBuffer())
      const kind = rendererFor(blob, tab.title)
      const text =
        kind === 'md' || kind === 'rtf' || kind === 'text'
          ? new TextDecoder('utf-8').decode(buf)
          : ''
      const result: Loaded = { kind, blob, bytes: buf, text }
      cache.set(tab.docId, result)
      loaded = result
      loadedDocId = tab.docId
    } catch (e) {
      loadError = (e as Error).message
    } finally {
      loading = false
    }
  }

  $effect(() => {
    const tab = activeTab
    if (tab) void loadActive(tab)
  })

  async function download(tab: ViewerTab) {
    try {
      const blob = await documentsApi.downloadBytes(tab.docId)
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = tab.title || 'document'
      a.click()
      URL.revokeObjectURL(url)
    } catch {
      // surfaced elsewhere; download failure is non-fatal
    }
  }

  // ── Resize handle ────────────────────────────────────────────────
  let resizing = false
  function startResize(e: PointerEvent) {
    resizing = true
    e.preventDefault()
    const onMove = (ev: PointerEvent) => {
      if (!resizing) return
      docViewer.setWidth(window.innerWidth - ev.clientX)
    }
    const onUp = () => {
      resizing = false
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
    }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
  }
</script>

{#if docViewer.open}
  <aside
    class="relative shrink-0 h-full flex flex-col border-l border-(--color-surface-200) bg-(--color-surface-0)"
    style:width={docViewer.collapsed ? '38px' : `${docViewer.width}px`}
  >
    {#if docViewer.collapsed}
      <!-- collapsed: a thin strip; click to restore the previous width -->
      <button
        type="button"
        onclick={() => docViewer.toggleCollapse()}
        aria-label={i18n.t('DocViewer.expand')}
        title={i18n.t('DocViewer.expand')}
        class="h-full w-full flex items-start justify-center pt-2.5
               text-(--color-text-secondary) hover:text-(--color-text-primary)
               hover:bg-(--color-hover-bg)"
      >
        <PanelRightOpen size={16} />
      </button>
    {:else}
    <!-- resize grip -->
    <div
      role="separator"
      aria-label="Resize document viewer"
      tabindex="-1"
      onpointerdown={startResize}
      class="absolute left-0 top-0 h-full w-1 -ml-0.5 cursor-col-resize z-10
             hover:bg-(--color-brand-400)"
    ></div>

    <!-- tab bar -->
    <div class="flex items-stretch shrink-0 border-b border-(--color-surface-200) bg-(--color-surface-50)">
      <button
        type="button"
        onclick={() => docViewer.toggleCollapse()}
        aria-label={i18n.t('DocViewer.collapse')}
        title={i18n.t('DocViewer.collapse')}
        class="shrink-0 w-9 flex items-center justify-center border-r border-(--color-surface-200)
               text-(--color-text-secondary) hover:text-(--color-text-primary) hover:bg-(--color-hover-bg)"
      >
        <PanelRightClose size={15} />
      </button>
      <div class="flex-1 min-w-0 flex items-stretch overflow-x-auto">
        {#each docViewer.tabs as tab (tab.id)}
          <div
            class="group flex items-center gap-1.5 pl-3 pr-1.5 h-9 max-w-44 border-r border-(--color-surface-200)
                   {tab.id === docViewer.activeId
                     ? 'bg-(--color-surface-0)'
                     : 'bg-(--color-surface-50) hover:bg-(--color-hover-bg)'}"
          >
            <button
              type="button"
              class="flex-1 min-w-0 truncate text-left text-xs
                     {tab.id === docViewer.activeId
                       ? 'text-(--color-text-primary) font-medium'
                       : 'text-(--color-text-secondary)'}"
              onclick={() => docViewer.select(tab.id)}
            >
              {tab.title}
            </button>
            <button
              type="button"
              class="p-0.5 rounded opacity-0 group-hover:opacity-100 hover:bg-(--color-hover-bg)"
              aria-label={i18n.t('DocViewer.closeTab')}
              onclick={() => docViewer.closeTab(tab.id)}
            >
              <X size={12} />
            </button>
          </div>
        {/each}
      </div>
      <button
        type="button"
        class="px-2 shrink-0 text-xs text-(--color-text-secondary) hover:text-(--color-text-primary)"
        onclick={() => docViewer.closeAll()}
      >
        {i18n.t('DocViewer.closeAll')}
      </button>
    </div>

    {#if activeTab}
      <!-- per-tab header card -->
      <div class="shrink-0 px-3 py-2 border-b border-(--color-surface-200)">
        {#if activeTab.mode === 'citation' && activeTab.quote}
          <div class="rounded-(--radius-md) bg-(--color-surface-50) border border-(--color-surface-200) p-2.5">
            <div class="flex items-center justify-between gap-2 mb-1">
              <span class="flex items-center gap-1 text-xs font-semibold text-(--color-brand-600)">
                <Quote size={12} />{i18n.t('DocViewer.citation')}
                {#if activeTab.page != null}
                  · {i18n.t('DocViewer.page')} {activeTab.page}
                {/if}
              </span>
              <button
                type="button"
                class="flex items-center gap-1 text-xs text-(--color-text-secondary) hover:text-(--color-text-primary)"
                onclick={() => download(activeTab)}
              >
                <Download size={12} />{i18n.t('Documents.viewer.download')}
              </button>
            </div>
            <p class="text-xs text-(--color-text-secondary) line-clamp-3 whitespace-pre-wrap">
              {activeTab.quote.replaceAll('[[PAGE_BREAK]]', ' … ')}
            </p>
          </div>
        {:else}
          <div class="flex items-center justify-between gap-2">
            <span class="text-xs text-(--color-text-secondary) truncate">{activeTab.title}</span>
            <button
              type="button"
              class="flex items-center gap-1 text-xs text-(--color-text-secondary) hover:text-(--color-text-primary)"
              onclick={() => download(activeTab)}
            >
              <Download size={12} />{i18n.t('Documents.viewer.download')}
            </button>
          </div>
        {/if}
      </div>

      <!-- renderer -->
      <div class="flex-1 min-h-0">
        {#if loading}
          <div class="flex items-center justify-center gap-2 h-full text-sm text-(--color-text-secondary)">
            <Spinner size="sm" />
            {i18n.t('Documents.viewer.loadingDocument')}
          </div>
        {:else if loadError}
          <p class="text-sm text-(--color-danger-500) p-8 text-center">
            {i18n.t('Documents.viewer.errorLoading')} — {loadError}
          </p>
        {:else if loaded}
          {#if loaded.kind === 'pdf'}
            <PdfView
              bytes={loaded.bytes}
              quote={activeTab.quote}
              page={activeTab.page}
              revision={docViewer.revision}
            />
          {:else if loaded.kind === 'docx'}
            <DocxView blob={loaded.blob} quote={activeTab.quote} revision={docViewer.revision} />
          {:else if loaded.kind === 'sheet'}
            <SheetView bytes={loaded.bytes} quote={activeTab.quote} revision={docViewer.revision} />
          {:else if loaded.kind === 'md'}
            <TextView text={loaded.text} kind="md" quote={activeTab.quote} revision={docViewer.revision} />
          {:else if loaded.kind === 'rtf'}
            <TextView text={loaded.text} kind="rtf" quote={activeTab.quote} revision={docViewer.revision} />
          {:else if loaded.kind === 'text'}
            <TextView text={loaded.text} kind="plain" quote={activeTab.quote} revision={docViewer.revision} />
          {:else}
            <p class="text-sm text-(--color-text-secondary) p-8 text-center">
              {i18n.t('DocViewer.unsupported')}
            </p>
          {/if}
        {/if}
      </div>
    {/if}
    {/if}
  </aside>
{/if}
