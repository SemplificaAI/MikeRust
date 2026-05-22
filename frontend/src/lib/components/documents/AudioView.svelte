<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Audio renderer. Two-pane:
   - top: HTML5 <audio controls> bound to a blob URL of the original
     file (wav/mp3/ogg/flac/m4a/aac). preload="metadata" so duration
     surfaces immediately without buffering the whole stream.
   - bottom: clickable transcript fetched from
     `GET /document/:id/transcript`. Each segment is a button that
     seeks the player to its `start_ms` (and starts playback). The
     cited segment — if any — gets a highlight ring and scrolls into
     view on mount + revision bump.

  Citation timestamps:
   - first preference: the structured `startMs` from the citation
     (set by the backend when the cited chunk text carries a
     `[T MM:SS]` marker, i.e. the chunk came from a whisper run);
   - fallback: parse `[T MM:SS]` / `[T HH:MM:SS]` out of the
     leading section of the quote text. Robust to either form.
-->
<script lang="ts">
  import { onDestroy } from 'svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { documentsApi, type TranscriptSegment } from '$lib/api/documents'
  import Spinner from '$lib/components/ui/Spinner.svelte'

  interface Props {
    /** Bytes blob of the original audio file. */
    blob: Blob
    filename: string
    /** Document UUID — used to fetch the transcript. */
    docId: string
    /** Cited passage from the assistant. We honour the leading
     *  `[T MM:SS]` marker as a seek hint when `startMs` is unset. */
    quote?: string
    /** Structured audio offset from the citation builder (preferred). */
    startMs?: number
    /** Bumped by DocViewerPanel when the user re-clicks the same citation
     *  pill so the seek + highlight re-fire even if the tab was already
     *  open. */
    revision?: number
  }

  let { blob, filename, docId, quote, startMs, revision = 0 }: Props = $props()

  let audioEl = $state<HTMLAudioElement>()
  let listHost = $state<HTMLUListElement>()
  let url = $state<string>('')
  let segments = $state<TranscriptSegment[]>([])
  let loadingTranscript = $state(false)
  let transcriptError = $state<string | null>(null)
  let activeStartMs = $state<number | null>(null)

  // Refresh the object URL only when the blob identity changes. A
  // citation re-click just bumps `revision` — we don't want to
  // recreate the URL because that would yank the player's state.
  let cachedBlob: Blob | null = null
  $effect(() => {
    if (cachedBlob !== blob) {
      if (url) URL.revokeObjectURL(url)
      url = URL.createObjectURL(blob)
      cachedBlob = blob
    }
  })

  // Fetch transcript once per docId. Generic across document kinds —
  // the backend returns segments=[] for non-audio docs, which we
  // simply don't render.
  let lastFetchedDocId: string | null = null
  $effect(() => {
    const id = docId
    if (!id || id === lastFetchedDocId) return
    lastFetchedDocId = id
    loadingTranscript = true
    transcriptError = null
    segments = []
    documentsApi
      .transcript(id)
      .then((r) => {
        segments = r.segments
      })
      .catch((e) => {
        transcriptError = (e as Error).message
      })
      .finally(() => {
        loadingTranscript = false
      })
  })

  onDestroy(() => {
    if (url) URL.revokeObjectURL(url)
  })

  /** Parse the first `[T MM:SS]` / `[T HH:MM:SS]` marker in a string.
   *  Used as a fallback when the citation's structured `startMs` is
   *  absent (older chats, or non-chunk-derived citations). */
  function parseLeadingTimestamp(q: string): number | null {
    const m = /\[T\s+(?:(\d{1,2}):)?(\d{1,2}):(\d{2})\]/.exec(q)
    if (!m) return null
    const h = m[1] ? Number(m[1]) : 0
    const min = Number(m[2])
    const s = Number(m[3])
    return (h * 3600 + min * 60 + s) * 1000
  }

  function seekTo(ms: number) {
    const el = audioEl
    if (!el) return
    const target = ms / 1000
    const apply = () => {
      try {
        el.currentTime = target
      } catch {
        /* readyState too low — the loadedmetadata listener below retries */
      }
    }
    if (el.readyState >= 1) {
      apply()
    } else {
      el.addEventListener('loadedmetadata', apply, { once: true })
    }
  }

  function pickActiveSegment(ms: number): number | null {
    if (segments.length === 0) return null
    // Find the segment whose [start, end) contains ms. Segments are
    // sorted by start by construction (backend BTreeMap).
    let best: TranscriptSegment | null = null
    for (const seg of segments) {
      if (seg.start_ms <= ms) best = seg
      else break
    }
    return best?.start_ms ?? null
  }

  // Citation effect: seek + highlight on mount + revision bump.
  $effect(() => {
    void revision
    void quote
    void startMs
    void segments
    const el = audioEl
    if (!el) return
    const target =
      startMs !== undefined
        ? startMs
        : quote
          ? parseLeadingTimestamp(quote)
          : null
    if (target === null) {
      activeStartMs = null
      return
    }
    seekTo(target)
    activeStartMs = pickActiveSegment(target)
    if (activeStartMs !== null) {
      // querySelector lookup keeps the segment refs out of the
      // component state — the only thing we ever do with the ref is
      // scrollIntoView, and a CSS-selector lookup is plenty fast
      // for transcript lengths we expect (hundreds of segments).
      queueMicrotask(() => {
        const sel = `[data-start-ms="${activeStartMs}"]`
        const segEl = listHost?.querySelector<HTMLButtonElement>(sel)
        segEl?.scrollIntoView({ block: 'center', behavior: 'smooth' })
      })
    }
  })

  function formatTime(ms: number): string {
    const totalSec = Math.floor(ms / 1000)
    const h = Math.floor(totalSec / 3600)
    const m = Math.floor((totalSec % 3600) / 60)
    const s = totalSec % 60
    if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
    return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
  }
</script>

<div class="h-full min-h-0 flex flex-col bg-(--color-surface-0)">
  <div class="shrink-0 flex flex-col gap-2 p-4 border-b border-(--color-surface-200)">
    <div class="text-sm font-medium text-(--color-text-primary) truncate" title={filename}>
      {filename}
    </div>
    <audio
      bind:this={audioEl}
      src={url}
      controls
      preload="metadata"
      class="w-full"
    ></audio>
  </div>

  <div class="flex-1 min-h-0 overflow-auto p-4">
    {#if loadingTranscript}
      <div class="flex items-center justify-center gap-2 py-8 text-sm text-(--color-text-secondary)">
        <Spinner size="sm" />
        {i18n.t('Documents.viewer.loadingDocument')}
      </div>
    {:else if transcriptError}
      <p class="text-sm text-(--color-text-secondary) py-6">
        {i18n.t('DocViewer.audio.transcriptHint')}
      </p>
    {:else if segments.length === 0}
      <p class="text-sm text-(--color-text-secondary) py-6">
        {i18n.t('DocViewer.audio.transcriptHint')}
      </p>
    {:else}
      <ul bind:this={listHost} class="flex flex-col gap-0.5">
        {#each segments as seg (seg.start_ms)}
          <li>
            <button
              type="button"
              data-start-ms={seg.start_ms}
              onclick={() => { seekTo(seg.start_ms); audioEl?.play(); activeStartMs = seg.start_ms }}
              class="w-full text-left flex gap-2 px-2 py-1.5 rounded-(--radius-sm)
                     text-xs hover:bg-(--color-hover-bg)
                     {activeStartMs === seg.start_ms
                       ? 'bg-(--color-active-bg) text-(--color-brand-700)'
                       : 'text-(--color-text-primary)'}"
            >
              <span class="shrink-0 font-mono text-(--color-text-secondary) min-w-[3.5rem]">
                {formatTime(seg.start_ms)}
              </span>
              <span class="flex-1 leading-relaxed">{seg.text}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>
