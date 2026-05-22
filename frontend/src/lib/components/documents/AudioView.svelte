<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Audio renderer for the document viewer. Phase 1 surface:

  - HTML5 <audio controls> bound to a blob URL of the original file
    (wav/mp3/ogg/flac/m4a/aac). No transcoding in the browser —
    whatever the browser's codec set supports plays back natively.
  - When opened via a citation pill, the quote header card above the
    viewer (rendered by DocViewerPanel) already shows the transcript
    excerpt with its [T MM:SS] marker prefix. The marker is parsed
    on first paint to seek the player to the cited timestamp so the
    user hears the cited passage as soon as the tab is active.

  Phase 1.5 (separate work):
   - Inline transcript pane below the player, fetched from a future
     /documents/:id/transcript endpoint (the text already exists on
     the doc_chunks rows; the endpoint just needs to concatenate
     them respecting [T MM:SS] order).
   - Clickable transcript segments that seek the player.
-->
<script lang="ts">
  import { onDestroy } from 'svelte'
  import { i18n } from '$lib/stores/i18n.svelte'

  interface Props {
    blob: Blob
    filename: string
    /** Cited passage from the assistant, including any [T MM:SS] prefix
     *  from the transcript chunker. When present we seek the player to
     *  the leading timestamp on mount + reset on revision bump. */
    quote?: string
    /** Bumped by DocViewerPanel when the user re-clicks the same citation
     *  pill so the seek re-fires even if the tab was already open. */
    revision?: number
  }

  let { blob, filename, quote, revision = 0 }: Props = $props()

  let audioEl = $state<HTMLAudioElement>()
  let url = $state<string>('')

  // Refresh the object URL only when the blob actually changes (a
  // citation click on the same audio just bumps `revision`, not blob).
  let cachedBlob: Blob | null = null
  $effect(() => {
    if (cachedBlob !== blob) {
      if (url) URL.revokeObjectURL(url)
      url = URL.createObjectURL(blob)
      cachedBlob = blob
    }
  })

  onDestroy(() => {
    if (url) URL.revokeObjectURL(url)
  })

  /** Parse a leading `[T MM:SS]` / `[T HH:MM:SS]` marker out of the quote
   *  and return the offset in seconds. Returns null when the quote has
   *  no leading marker — common for v1 citations that don't yet carry
   *  the transcript timestamp. */
  function parseLeadingTimestamp(q: string): number | null {
    const m = /\[T\s+(?:(\d{1,2}):)?(\d{1,2}):(\d{2})\]/.exec(q)
    if (!m) return null
    const h = m[1] ? Number(m[1]) : 0
    const min = Number(m[2])
    const s = Number(m[3])
    return h * 3600 + min * 60 + s
  }

  $effect(() => {
    void revision
    void quote
    const el = audioEl
    if (!el || !quote) return
    const t = parseLeadingTimestamp(quote)
    if (t == null) return
    // Wait until metadata is loaded — `currentTime` is silently
    // clamped to 0 otherwise.
    const apply = () => {
      try {
        el.currentTime = t
      } catch {
        /* readyState too low; the loadedmetadata listener below will retry */
      }
    }
    if (el.readyState >= 1) {
      apply()
    } else {
      el.addEventListener('loadedmetadata', apply, { once: true })
    }
  })
</script>

<div class="h-full min-h-0 flex flex-col bg-(--color-surface-0) p-5 gap-4">
  <div class="flex flex-col gap-2">
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

  <div class="flex-1 min-h-0 overflow-auto text-xs text-(--color-text-secondary)">
    <p>
      {i18n.t('DocViewer.audio.transcriptHint')}
    </p>
  </div>
</div>
