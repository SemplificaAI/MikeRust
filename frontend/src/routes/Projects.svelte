<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Projects screen. List + create + edit + delete over /project.
  Export / import .mikeprj is a later phase.
-->
<script lang="ts">
  import Badge from '$lib/components/ui/Badge.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import ConfirmDialog from '$lib/components/ui/ConfirmDialog.svelte'
  import ProjectModal from '$lib/components/projects/ProjectModal.svelte'
  import { projectStore } from '$lib/stores/projects.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { DOMAINS, domainLabel } from '$lib/types/domain'
  import type { Project } from '$lib/types/project'
  import { Search, Pencil, Trash2 } from 'lucide-svelte'

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  let search = $state('')
  let domainFilter = $state('')
  let modalOpen = $state(false)
  let editTarget = $state<Project | null>(null)
  let deleteTarget = $state<Project | null>(null)

  $effect(() => {
    void projectStore.refresh()
  })

  const domainOptions = $derived([
    { value: '', label: t('Domains.filterPlaceholder') },
    ...DOMAINS.map((d) => ({ value: d, label: domainLabel(d) })),
  ])

  const rows = $derived.by<Project[]>(() => {
    const q = search.trim().toLowerCase()
    return projectStore.items.filter((p) => {
      if (domainFilter && p.domain !== domainFilter) return false
      if (q) {
        const hay = `${p.name} ${p.description ?? ''}`.toLowerCase()
        if (!hay.includes(q)) return false
      }
      return true
    })
  })

  function openCreate() {
    editTarget = null
    modalOpen = true
  }
  function openEdit(p: Project) {
    editTarget = p
    modalOpen = true
  }

  async function confirmDelete() {
    if (!deleteTarget) return
    try {
      await projectStore.remove(deleteTarget.id)
      toastStore.info(t('Projects.deletedToast'))
    } catch (e) {
      toastStore.danger(t('Projects.deleteError'), { detail: (e as Error).message })
    } finally {
      deleteTarget = null
    }
  }

  function fmtDate(iso: string): string {
    const d = new Date(iso)
    return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString()
  }
</script>

<div class="max-w-4xl mx-auto p-8 space-y-5">
  <header class="flex items-end justify-between gap-4">
    <div class="space-y-1">
      <h2 class="text-2xl font-semibold text-(--color-text-primary)">{t('Projects.title')}</h2>
      <p class="text-sm text-(--color-text-secondary)">{t('Projects.emptyHint')}</p>
    </div>
    <Button onclick={openCreate}>{t('Projects.newProject')}</Button>
  </header>

  <div class="flex items-end gap-3">
    <Input bind:value={search} placeholder={t('Projects.searchPlaceholder')} size="sm" class="w-60">
      {#snippet iconBefore()}
        <Search size={14} />
      {/snippet}
    </Input>
    <div class="flex-1"></div>
    <Select options={domainOptions} bind:value={domainFilter} size="sm" class="w-44" />
  </div>

  {#if projectStore.loading}
    <div class="flex items-center gap-2 text-sm text-(--color-text-secondary) py-12 justify-center">
      <Spinner size="sm" />
      {t('Common.loading')}
    </div>
  {:else if projectStore.error}
    <EmptyState title={t('Projects.loadFailed')} description={projectStore.error}>
      {#snippet action()}
        <Button size="sm" variant="secondary" onclick={() => projectStore.refresh()}>
          {t('Common.retry')}
        </Button>
      {/snippet}
    </EmptyState>
  {:else if rows.length === 0}
    <EmptyState title={t('Projects.noProjects')} description={t('Projects.emptyHint')}>
      {#snippet action()}
        <Button size="sm" onclick={openCreate}>{t('Projects.newProject')}</Button>
      {/snippet}
    </EmptyState>
  {:else}
    <ul class="flex flex-col gap-2">
      {#each rows as p (p.id)}
        <li class="flex items-center gap-3 px-4 py-3 bg-(--color-surface-0) border border-(--color-surface-200) rounded-(--radius-md)">
          <div class="flex-1 min-w-0">
            <span class="text-sm font-medium text-(--color-text-primary) truncate">{p.name}</span>
            {#if p.description}
              <p class="text-xs text-(--color-text-secondary) truncate">{p.description}</p>
            {:else}
              <p class="text-xs text-(--color-text-disabled)">
                {t('Ui.createdOn', { date: fmtDate(p.created_at) })}
              </p>
            {/if}
          </div>
          <Badge tone="brand">{domainLabel(p.domain)}</Badge>
          <IconButton label={t('Projects.renameProject')} size="sm" onclick={() => openEdit(p)}>
            <Pencil size={14} />
          </IconButton>
          <IconButton
            label={t('Projects.deleteProject')}
            size="sm"
            variant="danger"
            onclick={() => (deleteTarget = p)}
          >
            <Trash2 size={14} />
          </IconButton>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<ProjectModal bind:open={modalOpen} project={editTarget} onsuccess={() => projectStore.refresh()} />

<ConfirmDialog
  open={deleteTarget !== null}
  title={t('Projects.deleteConfirmTitle')}
  message={t('Projects.deleteConfirmBody', { name: deleteTarget?.name ?? '' })}
  confirmLabel={t('Common.delete')}
  danger
  onconfirm={confirmDelete}
  oncancel={() => (deleteTarget = null)}
/>
