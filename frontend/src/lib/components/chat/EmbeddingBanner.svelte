<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Embedding-status banner. Visible only while the chat is waiting for a
  reply AND the embedding subsystem is busy (downloading or loading the
  model, or computing embeddings). Invisible in the steady state.
-->
<script lang="ts">
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import { syncApi, eurlexApi, type ModelStatus, type EmbedProgress } from '$lib/api/data-sources'
  import { i18n } from '$lib/stores/i18n.svelte'

  let { active }: { active: boolean } = $props()

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  let model = $state<ModelStatus | null>(null)
  let progress = $state<EmbedProgress | null>(null)
  let timer: ReturnType<typeof setInterval> | undefined

  async function poll() {
    try {
      ;[model, progress] = await Promise.all([syncApi.modelStatus(), eurlexApi.embedProgress()])
    } catch {
      /* transient — keep last snapshot */
    }
  }

  $effect(() => {
    clearInterval(timer)
    if (active) {
      void poll()
      timer = setInterval(poll, 600)
    } else {
      model = null
      progress = null
    }
    return () => clearInterval(timer)
  })

  const message = $derived.by(() => {
    if (model?.state === 'downloading') {
      const mb = Math.round((model.downloaded / 1_048_576) * 10) / 10
      const totalMb = Math.round((model.total / 1_048_576) * 10) / 10
      return `${t('EmbeddingStatus.downloadingTitle')} (${mb}/${totalMb} MB)`
    }
    if (model?.state === 'loading') return t('EmbeddingStatus.loadingModelTitle')
    if (progress && progress.total > 0) {
      return `${t('EmbeddingStatus.embeddingTitle')} ${progress.current}/${progress.total}`
    }
    return null
  })
</script>

{#if active && message}
  <div class="flex items-center gap-2 px-3 py-1.5 mb-2 rounded-(--radius-md)
              bg-(--color-surface-100) text-xs text-(--color-text-secondary)">
    <Spinner size="sm" />
    <span>{message}</span>
  </div>
{/if}
