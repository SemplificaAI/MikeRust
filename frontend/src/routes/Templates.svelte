<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  DOCX Templates screen. Read-only registry view (GET /docx-templates) —
  the backend has no write endpoint; templates are added by dropping a
  .dotx + sidecar JSON into config/docx-templates/. Filter by domain,
  locale and free-text search; level (L1/L2/L3) and domain badges.
-->
<script lang="ts">
  import Badge from '$lib/components/ui/Badge.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import { templateStore } from '$lib/stores/templates.svelte'
  import { DOMAINS, domainLabel } from '$lib/types/domain'
  import { templateDisplayName, type DocxTemplate } from '$lib/types/template'
  import { Search } from 'lucide-svelte'

  let domainFilter = $state<string>('')
  let localeFilter = $state<string>('')
  let search = $state<string>('')

  $effect(() => {
    void templateStore.refresh()
  })

  const domainOptions = [
    { value: '', label: 'All domains' },
    ...DOMAINS.map((d) => ({ value: d, label: domainLabel(d) })),
  ]
  const localeOptions = $derived([
    { value: '', label: 'All locales' },
    ...templateStore.locales.map((l) => ({ value: l, label: l })),
  ])

  function matchesDomain(t: DocxTemplate): boolean {
    if (!domainFilter) return true
    return t.domain === domainFilter || t.also_applicable_to.includes(domainFilter as never)
  }

  const rows = $derived.by<DocxTemplate[]>(() => {
    const q = search.trim().toLowerCase()
    return templateStore.items.filter((t) => {
      if (!matchesDomain(t)) return false
      if (localeFilter && t.locale !== localeFilter) return false
      if (q) {
        const hay = `${templateDisplayName(t, 'en')} ${t.id} ${t.category}`.toLowerCase()
        if (!hay.includes(q)) return false
      }
      return true
    })
  })
</script>

<div class="max-w-4xl mx-auto p-8 space-y-5">
  <header class="space-y-1">
    <h2 class="text-2xl font-semibold text-(--color-text-primary)">DOCX templates</h2>
    <p class="text-sm text-(--color-text-secondary)">
      Closing-formatter templates — applied to finished content to produce
      print-ready Word documents.
    </p>
  </header>

  <div class="flex items-end gap-3 flex-wrap">
    <Input
      bind:value={search}
      placeholder="Search templates…"
      size="sm"
      class="w-60"
    >
      {#snippet iconBefore()}
        <Search size={14} />
      {/snippet}
    </Input>
    <Select options={domainOptions} bind:value={domainFilter} size="sm" class="w-44" />
    <Select options={localeOptions} bind:value={localeFilter} size="sm" class="w-36" />
    <div class="flex-1"></div>
    <span class="text-xs text-(--color-text-secondary) pb-2">
      {rows.length} of {templateStore.items.length}
    </span>
  </div>

  {#if templateStore.loading}
    <div class="flex items-center gap-2 text-sm text-(--color-text-secondary) py-12 justify-center">
      <Spinner size="sm" />
      Loading templates…
    </div>
  {:else if templateStore.error}
    <EmptyState title="Could not load templates" description={templateStore.error}>
      {#snippet action()}
        <Button size="sm" variant="secondary" onclick={() => templateStore.refresh()}>
          Retry
        </Button>
      {/snippet}
    </EmptyState>
  {:else if rows.length === 0}
    <EmptyState
      title="No templates match"
      description="Try clearing the search or filters."
    />
  {:else}
    <ul class="flex flex-col gap-2">
      {#each rows as t (t.id)}
        <li
          class="px-4 py-3 bg-(--color-surface-0) border border-(--color-surface-200)
                 rounded-(--radius-md) space-y-1.5"
        >
          <div class="flex items-center gap-2">
            <Badge tone="level" size="xs">{t.automation_level}</Badge>
            <span class="text-sm font-medium text-(--color-text-primary) flex-1 min-w-0 truncate">
              {templateDisplayName(t, 'en')}
            </span>
            <Badge tone="brand">{domainLabel(t.domain)}</Badge>
            <span class="text-xs text-(--color-text-secondary) font-mono">{t.locale}</span>
          </div>
          <div class="flex items-center gap-2 flex-wrap">
            <span class="text-xs text-(--color-text-secondary) font-mono">{t.id}</span>
            {#if t.category}
              <span class="text-xs text-(--color-text-secondary)">· {t.category}</span>
            {/if}
            {#each t.also_applicable_to as extra (extra)}
              <Badge tone="neutral" size="xs">also: {domainLabel(extra)}</Badge>
            {/each}
            {#if t.required_metadata.length > 0}
              <span class="text-xs text-(--color-text-disabled)">
                · {t.required_metadata.length} required field{t.required_metadata.length === 1 ? '' : 's'}
              </span>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>
