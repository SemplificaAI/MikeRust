<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Full-page workflow editor. Assistant workflows edit a Markdown prompt;
  tabular workflows edit a column table. Both auto-save (debounced).
  System presets open read-only.
-->
<script lang="ts">
  import Input from '$lib/components/ui/Input.svelte'
  import Textarea from '$lib/components/ui/Textarea.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import Badge from '$lib/components/ui/Badge.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import ConfirmDialog from '$lib/components/ui/ConfirmDialog.svelte'
  import MarkdownEditor from '$lib/components/ui/MarkdownEditor.svelte'
  import { workflowsApi } from '$lib/api/workflows'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { domainLabel } from '$lib/types/domain'
  import {
    COLUMN_FORMATS,
    type ColumnFormat,
    type Workflow,
    type WorkflowColumn,
  } from '$lib/types/workflow'
  import { ArrowLeft, Trash2, Check } from 'lucide-svelte'

  let { id, onback, ondeleted }: { id: string; onback: () => void; ondeleted: () => void } =
    $props()

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  interface ColumnDraft {
    name: string
    format: ColumnFormat
    prompt: string
  }

  let wf = $state<Workflow | null>(null)
  let loading = $state(true)
  let loadError = $state<string | null>(null)

  let title = $state('')
  let promptMd = $state('')
  let columns = $state<ColumnDraft[]>([])
  let saveState = $state<'idle' | 'saving' | 'saved'>('idle')
  let deleteOpen = $state(false)

  const readOnly = $derived(wf?.is_system ?? true)

  const formatOptions = $derived(
    COLUMN_FORMATS.map((f) => ({ value: f, label: t(`ColumnFormats.${f}`) })),
  )

  $effect(() => {
    loading = true
    loadError = null
    workflowsApi
      .get(id)
      .then((w) => {
        wf = w
        title = w.title
        promptMd = w.prompt_md ?? ''
        columns = (w.columns_config ?? []).map((c) => ({
          name: (c.label ?? c.key ?? '') as string,
          format: (c.format as ColumnFormat) ?? 'free_text',
          prompt: (c.prompt ?? '') as string,
        }))
      })
      .catch((e) => (loadError = (e as Error).message))
      .finally(() => (loading = false))
  })

  function slugKey(name: string, i: number): string {
    const s = name
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '_')
      .replace(/^_+|_+$/g, '')
    return s || `col_${i + 1}`
  }

  let saveTimer: ReturnType<typeof setTimeout> | undefined

  function scheduleSave() {
    if (readOnly) return
    saveState = 'saving'
    clearTimeout(saveTimer)
    saveTimer = setTimeout(doSave, 800)
  }

  async function doSave() {
    if (!wf || readOnly) return
    const payload: Partial<Workflow> = { title: title.trim() || wf.title }
    if (wf.type === 'assistant') {
      payload.prompt_md = promptMd
    } else {
      payload.columns_config = columns.map(
        (c, i): WorkflowColumn => ({
          key: slugKey(c.name, i),
          label: c.name.trim(),
          prompt: c.prompt.trim(),
          format: c.format,
        }),
      )
    }
    try {
      await workflowsApi.update(id, payload)
      saveState = 'saved'
    } catch (e) {
      saveState = 'idle'
      toastStore.danger(t('Workflows.updateError'), { detail: (e as Error).message })
    }
  }

  function addColumn() {
    columns = [...columns, { name: '', format: 'free_text', prompt: '' }]
    scheduleSave()
  }
  function removeColumn(i: number) {
    columns = columns.filter((_, idx) => idx !== i)
    scheduleSave()
  }

  async function confirmDelete() {
    deleteOpen = false
    try {
      await workflowsApi.remove(id)
      toastStore.info(t('Workflows.deletedToast'))
      ondeleted()
    } catch (e) {
      toastStore.danger(t('Workflows.updateError'), { detail: (e as Error).message })
    }
  }
</script>

<div class="max-w-3xl mx-auto p-8 space-y-5">
  <div class="flex items-center justify-between gap-3">
    <button
      type="button"
      onclick={onback}
      class="flex items-center gap-1.5 text-sm text-(--color-text-secondary) hover:text-(--color-text-primary)"
    >
      <ArrowLeft size={15} />{t('Workflows.title')}
    </button>
    {#if !loading && wf}
      <div class="flex items-center gap-2">
        {#if readOnly}
          <Badge tone="neutral" size="xs">{t('Workflows.readOnly')}</Badge>
        {:else}
          <span class="text-xs text-(--color-text-secondary)">
            {#if saveState === 'saving'}{t('Common.saving')}
            {:else if saveState === 'saved'}<span class="inline-flex items-center gap-1">
                <Check size={12} class="text-(--color-success-500)" />{t('Workflows.savedStatus')}
              </span>{/if}
          </span>
          <IconButton label={t('Workflows.deleteWorkflow')} size="sm" variant="danger"
            onclick={() => (deleteOpen = true)}>
            <Trash2 size={15} />
          </IconButton>
        {/if}
      </div>
    {/if}
  </div>

  {#if loading}
    <div class="flex items-center gap-2 text-sm text-(--color-text-secondary) py-12 justify-center">
      <Spinner size="sm" />
      {t('Common.loading')}
    </div>
  {:else if loadError || !wf}
    <EmptyState title={t('Errors.somethingWrong')} description={loadError ?? ''} />
  {:else}
    <input
      bind:value={title}
      disabled={readOnly}
      oninput={scheduleSave}
      class="w-full bg-transparent text-2xl font-semibold text-(--color-text-primary)
             focus:outline-none disabled:opacity-100"
    />
    <div class="flex items-center gap-2">
      <Badge tone={wf.type === 'assistant' ? 'assistant' : 'tabular'}>
        {wf.type === 'assistant' ? t('Workflows.typeAssistant') : t('Workflows.typeTabular')}
      </Badge>
      <Badge tone="brand">{domainLabel(wf.domain)}</Badge>
      {#if wf.practice}
        <span class="text-xs text-(--color-text-secondary)">{wf.practice}</span>
      {/if}
    </div>

    {#if wf.type === 'assistant'}
      <div class="space-y-1.5">
        <span class="text-xs font-medium text-(--color-text-secondary)">{t('Workflows.prompt')}</span>
        {#if readOnly}
          <div class="md-body text-sm border border-(--color-surface-200) rounded-(--radius-md) p-4
                      min-h-40 whitespace-pre-wrap text-(--color-text-primary)">
            {promptMd || t('Assistant.noPromptDefined')}
          </div>
        {:else}
          <MarkdownEditor
            bind:value={promptMd}
            placeholder={t('Workflows.promptPlaceholder')}
            minHeight="320px"
            oninput={scheduleSave}
          />
        {/if}
      </div>
    {:else}
      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <span class="text-xs font-medium text-(--color-text-secondary)">{t('Workflows.columns')}</span>
          {#if !readOnly}
            <Button size="sm" variant="secondary" onclick={addColumn}>{t('Workflows.addColumn')}</Button>
          {/if}
        </div>
        {#if columns.length === 0}
          <p class="text-sm text-(--color-text-secondary) py-4 text-center">
            {t('Workflows.columnsEmptyHint')}
          </p>
        {/if}
        {#each columns as col, i (i)}
          <div class="border border-(--color-surface-200) rounded-(--radius-md) p-3 space-y-2.5">
            <div class="flex items-center gap-2">
              <Input
                bind:value={col.name}
                placeholder={t('Workflows.columnName')}
                disabled={readOnly}
                class="flex-1"
                oninput={scheduleSave}
              />
              <Select
                options={formatOptions}
                bind:value={col.format}
                size="md"
                class="w-44"
                disabled={readOnly}
                onchange={scheduleSave}
              />
              {#if !readOnly}
                <IconButton label={t('Common.delete')} variant="danger" size="md"
                  onclick={() => removeColumn(i)}>
                  <Trash2 size={15} />
                </IconButton>
              {/if}
            </div>
            <Textarea
              bind:value={col.prompt}
              placeholder={t('Workflows.columnPromptPlaceholder')}
              minRows={2}
              disabled={readOnly}
              oninput={scheduleSave}
            />
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<ConfirmDialog
  bind:open={deleteOpen}
  title={t('Workflows.deleteConfirmTitle')}
  message={t('Workflows.deleteConfirmBody', { title: wf?.title ?? '' })}
  confirmLabel={t('Common.delete')}
  danger
  onconfirm={confirmDelete}
/>
