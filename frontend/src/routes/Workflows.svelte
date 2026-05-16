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
  import WorkflowModal from '$lib/components/workflow/WorkflowModal.svelte'
  import WorkflowEditor from '$lib/components/workflow/WorkflowEditor.svelte'
  import { workflowStore } from '$lib/stores/workflows.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { DOMAINS, domainLabel } from '$lib/types/domain'
  import type { Workflow } from '$lib/types/workflow'
  import { Eye, EyeOff } from 'lucide-svelte'

  type TabId = 'all' | 'builtin' | 'custom' | 'hidden'
  let activeTab = $state<TabId>('all')
  let domainFilter = $state<string>('')
  let modalOpen = $state(false)
  let editId = $state<string | null>(null)

  $effect(() => {
    void workflowStore.refresh()
  })

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  // Tab partitioning is purely client-side over the fetched set.
  const builtin = $derived(workflowStore.items.filter((w) => w.is_system))
  const custom = $derived(workflowStore.items.filter((w) => !w.is_system))
  const hiddenItems = $derived(
    workflowStore.items.filter((w) => workflowStore.isHidden(w.id))
  )

  const tabs = $derived([
    { id: 'all', label: t('Common.all'), count: workflowStore.visible.length },
    { id: 'builtin', label: t('Workflows.tabBuiltin'), count: builtin.filter((w) => !workflowStore.isHidden(w.id)).length },
    { id: 'custom', label: t('Workflows.tabCustom'), count: custom.length },
    { id: 'hidden', label: t('Workflows.tabHidden'), count: hiddenItems.length },
  ])

  const domainOptions = $derived([
    { value: '', label: t('Domains.filterPlaceholder') },
    ...DOMAINS.map((d) => ({ value: d, label: domainLabel(d) })),
  ])

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
        toastStore.info(t('Workflows.restoredToast', { title: w.title }))
      } else {
        await workflowStore.hide(w.id)
        toastStore.info(t('Workflows.hiddenToast', { title: w.title }))
      }
    } catch (e) {
      toastStore.danger(t('Workflows.updateError'), { detail: (e as Error).message })
    }
  }
</script>

{#if editId}
  <WorkflowEditor
    id={editId}
    onback={() => { editId = null; void workflowStore.refresh() }}
    ondeleted={() => { editId = null; void workflowStore.refresh() }}
  />
{:else}
<div class="max-w-4xl mx-auto p-8 space-y-5">
  <header class="flex items-end justify-between gap-4">
    <div class="space-y-1">
      <h2 class="text-2xl font-semibold text-(--color-text-primary)">{t('Workflows.title')}</h2>
      <p class="text-sm text-(--color-text-secondary)">
        {t('Workflows.allHint')}
      </p>
    </div>
    <Button variant="primary" onclick={() => (modalOpen = true)}>{t('Workflows.newWorkflow')}</Button>
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
      {t('Common.loading')}
    </div>
  {:else if workflowStore.error}
    <EmptyState
      title={t('Errors.somethingWrong')}
      description={workflowStore.error}
    >
      {#snippet action()}
        <Button size="sm" variant="secondary" onclick={() => workflowStore.refresh()}>
          {t('Common.retry')}
        </Button>
      {/snippet}
    </EmptyState>
  {:else if rows.length === 0}
    <EmptyState title={t('Workflows.noWorkflows')} />
  {:else}
    <ul class="flex flex-col gap-2">
      {#each rows as w (w.id)}
        <li
          class="flex items-center gap-3 px-4 py-3
                 bg-(--color-surface-0) border border-(--color-surface-200)
                 rounded-(--radius-md)"
        >
          <button type="button" class="flex-1 min-w-0 text-left" onclick={() => (editId = w.id)}>
            <div class="flex items-center gap-2">
              <span class="text-sm font-medium text-(--color-text-primary) truncate">
                {w.title}
              </span>
              {#if w.is_system}
                <Badge tone="neutral" size="xs">{t('Ui.preset')}</Badge>
              {/if}
            </div>
            {#if w.practice}
              <p class="text-xs text-(--color-text-secondary) truncate">{w.practice}</p>
            {/if}
          </button>

          <Badge tone={w.type === 'assistant' ? 'assistant' : 'tabular'}>
            {w.type === 'assistant' ? t('Workflows.typeAssistant') : t('Workflows.typeTabular')}
          </Badge>
          <Badge tone="brand">{domainLabel(w.domain)}</Badge>

          {#if w.type === 'tabular'}
            <span class="text-xs text-(--color-text-secondary) tabular-nums w-16 text-right">
              {t('Ui.columnCount', { n: w.columns_config.length })}
            </span>
          {:else}
            <span class="w-16"></span>
          {/if}

          {#if w.is_system}
            <IconButton
              label={workflowStore.isHidden(w.id) ? t('Workflows.unhide') : t('Ui.hide')}
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
{/if}

<WorkflowModal
  bind:open={modalOpen}
  onsuccess={(createdId) => {
    void workflowStore.refresh()
    if (createdId) editId = createdId
  }}
/>
