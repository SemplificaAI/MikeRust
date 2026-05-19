<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Project detail: header (name, domain, retrieval-scope toggle, export)
  plus three tabs — Documents, Conversations and Tabular reviews. The
  backend has no folder/version model, so documents are a flat list.
-->
<script lang="ts">
  import Badge from '$lib/components/ui/Badge.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import Modal from '$lib/components/ui/Modal.svelte'
  import Checkbox from '$lib/components/ui/Checkbox.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import ProjectFolderTree from './ProjectFolderTree.svelte'
  import { projectsApi } from '$lib/api/projects'
  import { chatApi } from '$lib/api/chat'
  import { tabularApi } from '$lib/api/tabular'
  import { chatStore } from '$lib/stores/chat.svelte'
  import { router } from '$lib/stores/router.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { domainLabel } from '$lib/types/domain'
  import type { ProjectDetail, IsolationMode } from '$lib/types/project'
  import type { Chat } from '$lib/types/chat'
  import type { TabularReview } from '$lib/types/tabular'
  import { ArrowLeft, Download, MessageSquare, Table2 } from 'lucide-svelte'

  let { id, onback }: { id: string; onback: () => void } = $props()

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  type Tab = 'documents' | 'chats' | 'reviews'
  let tab = $state<Tab>('documents')

  let project = $state<ProjectDetail | null>(null)
  let loading = $state(true)
  let error = $state<string | null>(null)

  let chats = $state<Chat[]>([])
  let chatsLoaded = $state(false)
  let reviews = $state<TabularReview[]>([])
  let reviewsLoaded = $state(false)


  // ── load ─────────────────────────────────────────────────────────
  $effect(() => {
    loading = true
    error = null
    project = null
    chatsLoaded = reviewsLoaded = false
    projectsApi
      .get(id)
      .then((p) => (project = p))
      .catch((e) => (error = (e as Error).message))
      .finally(() => (loading = false))
  })

  async function loadChats() {
    try {
      const r = await chatApi.list()
      chats = r.chats.filter((c) => c.project_id === id)
    } catch {
      chats = []
    } finally {
      chatsLoaded = true
    }
  }
  async function loadReviews() {
    try {
      reviews = await tabularApi.list({ project_id: id })
    } catch {
      reviews = []
    } finally {
      reviewsLoaded = true
    }
  }

  $effect(() => {
    // The documents tab owns its own loading (ProjectFolderTree).
    if (tab === 'chats' && !chatsLoaded) void loadChats()
    if (tab === 'reviews' && !reviewsLoaded) void loadReviews()
  })

  // ── isolation mode ───────────────────────────────────────────────
  const isolationOptions = $derived([
    { value: 'shared', label: t('Projects.isolationShared') },
    { value: 'strict', label: t('Projects.isolationStrict') },
  ])

  async function changeIsolation(mode: IsolationMode) {
    if (!project || project.isolation_mode === mode) return
    try {
      await projectsApi.update(id, { isolation_mode: mode })
      project.isolation_mode = mode
      toastStore.success(t('Projects.isolationSaved'))
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    }
  }

  // ── conversations ────────────────────────────────────────────────
  function openChat(chatId: string) {
    void chatStore.selectChat(chatId)
    router.go('assistant')
  }
  async function newChat() {
    try {
      const created = await chatApi.createRecord(id)
      await loadChats()
      openChat(created.id)
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    }
  }

  // ── export ───────────────────────────────────────────────────────
  let exportOpen = $state(false)
  let exportEmail = $state('')
  let exportChats = $state(false)
  let exporting = $state(false)

  async function runExport() {
    if (!exportEmail.trim() || !project) return
    exporting = true
    try {
      const blob = await projectsApi.exportProject(id, exportEmail.trim(), exportChats)
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `${project.name}.mikeprj`
      a.click()
      URL.revokeObjectURL(url)
      toastStore.success(t('ProjectExport.downloadStarted'))
      exportOpen = false
    } catch (e) {
      toastStore.danger(t('ProjectExport.errorExport'), { detail: (e as Error).message })
    } finally {
      exporting = false
    }
  }

</script>

<div class="max-w-4xl mx-auto p-8 space-y-5">
  <button
    type="button"
    onclick={onback}
    class="flex items-center gap-1.5 text-sm text-(--color-text-secondary) hover:text-(--color-text-primary)"
  >
    <ArrowLeft size={15} />{t('Projects.title')}
  </button>

  {#if loading}
    <div class="flex items-center gap-2 text-sm text-(--color-text-secondary) py-12 justify-center">
      <Spinner size="sm" />
      {t('Common.loading')}
    </div>
  {:else if error || !project}
    <EmptyState title={t('Projects.loadFailed')} description={error ?? ''} />
  {:else}
    <header class="space-y-3">
      <div class="flex items-center justify-between gap-4">
        <h2 class="text-2xl font-semibold text-(--color-text-primary)">{project.name}</h2>
        <div class="flex items-center gap-2">
          <Badge tone="brand">{domainLabel(project.domain)}</Badge>
          <Button size="sm" variant="secondary" onclick={() => (exportOpen = true)}>
            <Download size={14} class="mr-1" />{t('ProjectExport.exportButton')}
          </Button>
        </div>
      </div>
      {#if project.description}
        <p class="text-sm text-(--color-text-secondary)">{project.description}</p>
      {/if}
      <div class="flex items-center gap-2">
        <span class="text-xs text-(--color-text-secondary)">{t('Projects.isolationLabel')}</span>
        <Select
          options={isolationOptions}
          value={project.isolation_mode}
          size="sm"
          class="w-56"
          onchange={(e) =>
            changeIsolation((e.currentTarget as HTMLSelectElement).value as IsolationMode)}
        />
      </div>
      <p class="text-xs text-(--color-text-disabled)">{t('Projects.isolationHint')}</p>
    </header>

    <!-- tabs -->
    <div class="flex gap-1 border-b border-(--color-surface-200)">
      {#each [['documents', 'Projects.documents'], ['chats', 'Projects.chats'], ['reviews', 'Projects.tabularReviews']] as [key, labelKey] (key)}
        <button
          type="button"
          onclick={() => (tab = key as Tab)}
          class="px-3 h-9 text-sm border-b-2 -mb-px
                 {tab === key
                   ? 'border-(--color-brand-500) text-(--color-text-primary) font-medium'
                   : 'border-transparent text-(--color-text-secondary) hover:text-(--color-text-primary)'}"
        >
          {t(labelKey)}
        </button>
      {/each}
    </div>

    {#if tab === 'documents'}
      <ProjectFolderTree projectId={id} />
    {:else if tab === 'chats'}
      <div class="flex justify-end">
        <Button size="sm" onclick={newChat}>
          <MessageSquare size={14} class="mr-1" />{t('Assistant.newChat')}
        </Button>
      </div>
      {#if !chatsLoaded}
        <div class="flex justify-center py-8"><Spinner size="sm" /></div>
      {:else if chats.length === 0}
        <EmptyState title={t('Sidebar.noChats')} description={t('Projects.emptyHint')} />
      {:else}
        <ul class="flex flex-col gap-2">
          {#each chats as c (c.id)}
            <li>
              <button
                type="button"
                onclick={() => openChat(c.id)}
                class="w-full flex items-center gap-2 px-4 py-2.5 text-left
                       bg-(--color-surface-0) border border-(--color-surface-200) rounded-(--radius-md)
                       hover:border-(--color-surface-300)"
              >
                <MessageSquare size={14} class="text-(--color-text-secondary) shrink-0" />
                <span class="flex-1 min-w-0 truncate text-sm text-(--color-text-primary)">
                  {c.title || t('Assistant.untitledChat')}
                </span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    {:else}
      {#if !reviewsLoaded}
        <div class="flex justify-center py-8"><Spinner size="sm" /></div>
      {:else if reviews.length === 0}
        <EmptyState title={t('TabularReviews.noReviews')} description={t('Projects.emptyHint')} />
      {:else}
        <ul class="flex flex-col gap-2">
          {#each reviews as r (r.id)}
            <li class="flex items-center gap-3 px-4 py-2.5 bg-(--color-surface-0) border border-(--color-surface-200) rounded-(--radius-md)">
              <Table2 size={14} class="text-(--color-text-secondary) shrink-0" />
              <span class="flex-1 min-w-0 truncate text-sm text-(--color-text-primary)">{r.title}</span>
              <span class="text-xs text-(--color-text-secondary)">
                {t('Ui.columnCountFull', { n: r.columns_config.length })}
              </span>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
  {/if}
</div>

<!-- export modal -->
<Modal bind:open={exportOpen} title={t('ProjectExport.title')} size="md">
  <div class="space-y-3">
    <p class="text-sm text-(--color-text-secondary)">{t('ProjectExport.subtitle')}</p>
    <Input
      label={t('ProjectExport.recipientEmail')}
      bind:value={exportEmail}
      placeholder={t('ProjectExport.recipientEmailPlaceholder')}
      type="email"
    />
    <Checkbox label={t('ProjectExport.includeChats')} bind:checked={exportChats} />
    <p class="text-xs text-(--color-text-secondary)">{t('ProjectExport.includeChatsHint')}</p>
  </div>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (exportOpen = false)}>{t('Common.cancel')}</Button>
    <Button loading={exporting} disabled={!exportEmail.trim()} onclick={runExport}>
      {t('ProjectExport.exportNow')}
    </Button>
  {/snippet}
</Modal>
