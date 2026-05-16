<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Tabular-review detail: title, domain and the inherited column
  definitions. The per-document extraction grid depends on backend
  endpoints that do not exist yet (the review row stores only metadata).
-->
<script lang="ts">
  import Badge from '$lib/components/ui/Badge.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import { tabularApi } from '$lib/api/tabular'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { domainLabel } from '$lib/types/domain'
  import type { TabularReview } from '$lib/types/tabular'
  import { ArrowLeft } from 'lucide-svelte'

  let { id, onback }: { id: string; onback: () => void } = $props()

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  let review = $state<TabularReview | null>(null)
  let loading = $state(true)
  let error = $state<string | null>(null)

  $effect(() => {
    loading = true
    error = null
    tabularApi
      .get(id)
      .then((r) => (review = r))
      .catch((e) => (error = (e as Error).message))
      .finally(() => (loading = false))
  })

  function formatLabel(fmt: string | undefined): string {
    return fmt ? t(`ColumnFormats.${fmt}`) : ''
  }
</script>

<div class="max-w-4xl mx-auto p-8 space-y-5">
  <button
    type="button"
    onclick={onback}
    class="flex items-center gap-1.5 text-sm text-(--color-text-secondary) hover:text-(--color-text-primary)"
  >
    <ArrowLeft size={15} />{t('TabularReviews.title')}
  </button>

  {#if loading}
    <div class="flex items-center gap-2 text-sm text-(--color-text-secondary) py-12 justify-center">
      <Spinner size="sm" />
      {t('Common.loading')}
    </div>
  {:else if error || !review}
    <EmptyState title={t('Errors.somethingWrong')} description={error ?? ''} />
  {:else}
    <header class="flex items-center justify-between gap-4">
      <h2 class="text-2xl font-semibold text-(--color-text-primary)">{review.title}</h2>
      <Badge tone="brand">{domainLabel(review.domain)}</Badge>
    </header>

    <section class="space-y-2">
      <h3 class="text-sm font-semibold text-(--color-text-primary)">
        {t('WorkflowColumns.columnsHeader')}
        <span class="text-(--color-text-secondary) font-normal">
          ({review.columns_config.length})
        </span>
      </h3>

      {#if review.columns_config.length === 0}
        <p class="text-sm text-(--color-text-secondary)">{t('WorkflowColumns.noColumns')}</p>
      {:else}
        <ul class="flex flex-col gap-2">
          {#each review.columns_config as col, i (col.key ?? i)}
            <li class="px-4 py-3 bg-(--color-surface-0) border border-(--color-surface-200) rounded-(--radius-md) space-y-1">
              <div class="flex items-center gap-2">
                <span class="text-sm font-medium text-(--color-text-primary)">
                  {col.label ?? col.key ?? `#${i + 1}`}
                </span>
                {#if col.format}
                  <Badge tone="neutral" size="xs">{formatLabel(col.format)}</Badge>
                {/if}
              </div>
              {#if col.prompt}
                <p class="text-xs text-(--color-text-secondary) whitespace-pre-wrap">{col.prompt}</p>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  {/if}
</div>
