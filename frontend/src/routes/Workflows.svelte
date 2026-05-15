<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Workflows screen — first real feature route. Lists DB workflows merged
  with shipped presets (GET /workflow), with tab filtering (all / built-in
  / custom / hidden), a domain filter, and hide/unhide on built-ins.
  Create + edit modals land in a later phase.
-->
<script lang="ts">
  import Tabs from '$lib/components/ui/Tabs.svelte'
  import Badge from '$lib/components/ui/Badge.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import { workflowStore } from '$lib/stores/workflows.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { DOMAINS, domainLabel } from '$lib/types/domain'
  import type { Workflow } from '$lib/types/workflow'
  import { Eye, EyeOff } from 'lucide-svelte'

  type TabId = 'all' | 'builtin' | 'custom' | 'hidden'
  let activeTab = $state<TabId>('all')
  let domainFilter = $state<string>('')

  $effect(() => {
    void workflowStore.refresh()
  })

  // Tab partitioning is purely client-side over the fetched set.
  const builtin = $derived(workflowStore.items.filter((w) => w.is_system))
  const custom = $derived(workflowStore.items.filter((w) => !w.is_system))
  const hiddenItems = $derived(
    workflowStore.items.filter((w) => workflowStore.isHidden(w.id))
  )

  const tabs = $derived([
    { id: 'all', label: 'All', count: workflowStore.visible.length },
    { id: 'builtin', label: 'Built-in', count: builtin.filter((w) => !workflowStore.isHidden(w.id)).length },
    { id: 'custom', label: 'Custom', count: custom.length },
    { id: 'hidden', label: 'Hidden', count: hiddenItems.length },
  ])

  const domainOptions = [
    { value: '', label: 'All domains' },
    ...DOMAINS.map((d) => ({ value: d, label: domainLabel(d) })),
  ]

  const rows = $derived.by<Workflow[]>(() => {
    let list: Workflow[]
    switch (activeTab) {
      case 'builtin':
        list = builtin.filter((w) => !workflowStore.isHidden(w.id))
        break
      case 'custom':
        list = custom
        break
      case 'hidden':
        list = hiddenItems
        break
      default:
        list = workflowStore.visible
    }
    if (domainFilter) list = list.filter((w) => w.domain === domainFilter)
    return list
  })

  async function toggleHidden(w: Workflow) {
    try {
      if (workflowStore.isHidden(w.id)) {
        await workflowStore.unhide(w.id)
        toastStore.info(`"${w.title}" restored`)
      } else {
        await workflowStore.hide(w.id)
        toastStore.info(`"${w.title}" hidden`)
      }
    } catch (e) {
      toastStore.danger('Could not update workflow', { detail: (e as Error).message })
    }
  }
</script>

<div class="max-w-4xl mx-auto p-8 space-y-5">
  <header class="flex items-end justify-between gap-4">
    <div class="space-y-1">
      <h2 class="text-2xl font-semibold text-(--color-text-primary)">Workflows</h2>
      <p class="text-sm text-(--color-text-secondary)">
        Reusable assistant and tabular-review templates.
      </p>
    </div>
    <Button variant="primary" disabled>New workflow</Button>
  </header>

  <div class="flex items-end justify-between gap-4">
    <Tabs tabs={tabs} bind:active={activeTab} />
    <Select
      options={domainOptions}
      bind:value={domainFilter}
      size="sm"
      class="w-44"
    />
  </div>

  {#if workflowStore.loading}
    <div class="flex items-center gap-2 text-sm text-(--color-text-secondary) py-12 justify-center">
      <Spinner size="sm" />
      Loading workflows…
    </div>
  {:else if workflowStore.error}
    <EmptyState
      title="Could not load workflows"
      description={workflowStore.error}
    >
      {#snippet action()}
        <Button size="sm" variant="secondary" onclick={() => workflowStore.refresh()}>
          Retry
        </Button>
      {/snippet}
    </EmptyState>
  {:else if rows.length === 0}
    <EmptyState
      title="No workflows here"
      description={domainFilter
        ? `No ${activeTab} workflows in the ${domainLabel(domainFilter)} domain.`
        : `No ${activeTab} workflows yet.`}
    />
  {:else}
    <ul class="flex flex-col gap-2">
      {#each rows as w (w.id)}
        <li
          class="flex items-center gap-3 px-4 py-3
                 bg-(--color-surface-0) border border-(--color-surface-200)
                 rounded-(--radius-md)"
        >
          <div class="flex-1 min-w-0">
            <div class="flex items-center gap-2">
              <span class="text-sm font-medium text-(--color-text-primary) truncate">
                {w.title}
              </span>
              {#if w.is_system}
                <Badge tone="neutral" size="xs">preset</Badge>
              {/if}
            </div>
            {#if w.practice}
              <p class="text-xs text-(--color-text-secondary) truncate">{w.practice}</p>
            {/if}
          </div>

          <Badge tone={w.type === 'assistant' ? 'assistant' : 'tabular'}>
            {w.type === 'assistant' ? 'Assistant' : 'Tabular'}
          </Badge>
          <Badge tone="brand">{domainLabel(w.domain)}</Badge>

          {#if w.type === 'tabular'}
            <span class="text-xs text-(--color-text-secondary) tabular-nums w-16 text-right">
              {w.columns_config.length} col{w.columns_config.length === 1 ? '' : 's'}
            </span>
          {:else}
            <span class="w-16"></span>
          {/if}

          {#if w.is_system}
            <IconButton
              label={workflowStore.isHidden(w.id) ? 'Restore workflow' : 'Hide workflow'}
              size="sm"
              onclick={() => toggleHidden(w)}
            >
              {#if workflowStore.isHidden(w.id)}
                <EyeOff size={15} />
              {:else}
                <Eye size={15} />
              {/if}
            </IconButton>
          {:else}
            <span class="w-7"></span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>
