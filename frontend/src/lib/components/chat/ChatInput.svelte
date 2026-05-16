<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Chat composer: a textarea plus attachment pickers for documents,
  projects, workflows and templates. On send it emits the message text
  and the assembled attachment payload.
-->
<script lang="ts">
  import Button from '$lib/components/ui/Button.svelte'
  import Badge from '$lib/components/ui/Badge.svelte'
  import PickerModal from '$lib/components/ui/PickerModal.svelte'
  import type { PickerItem } from '$lib/components/ui/PickerModal.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { documentsApi } from '$lib/api/documents'
  import { workflowsApi } from '$lib/api/workflows'
  import { templatesApi } from '$lib/api/templates'
  import { projectsApi } from '$lib/api/projects'
  import { templateDisplayName } from '$lib/types/template'
  import { domainLabel } from '$lib/types/domain'
  import type { SendAttachments } from '$lib/stores/chat.svelte'
  import type { FileRef, TemplateRef, WorkflowRef } from '$lib/types/chat'
  import { Paperclip, FolderKanban, Workflow as WorkflowIcon, FileType2, X } from 'lucide-svelte'

  interface Props {
    streaming: boolean
    onsend: (text: string, attach: SendAttachments) => void
    onstop: () => void
  }

  let { streaming, onsend, onstop }: Props = $props()

  const t = (k: string) => i18n.t(k)

  let text = $state('')
  let files = $state<FileRef[]>([])
  let workflow = $state<WorkflowRef | null>(null)
  let template = $state<TemplateRef | null>(null)
  let project = $state<{ id: string; name: string } | null>(null)

  // ── pickers ─────────────────────────────────────────────────────────
  type Kind = 'doc' | 'project' | 'workflow' | 'template'
  let pickerKind = $state<Kind | null>(null)
  let pickerOpen = $state(false)
  let pickerItems = $state<PickerItem[]>([])
  let pickerLoading = $state(false)

  async function openPicker(kind: Kind) {
    pickerKind = kind
    pickerItems = []
    pickerLoading = true
    pickerOpen = true
    try {
      if (kind === 'doc') {
        const r = await documentsApi.list()
        pickerItems = r.documents.map((d) => ({
          id: d.id,
          label: d.filename,
          sublabel: d.file_type,
        }))
      } else if (kind === 'project') {
        const r = await projectsApi.list()
        pickerItems = r.projects.map((p) => ({ id: p.id, label: p.name }))
      } else if (kind === 'workflow') {
        const r = await workflowsApi.list()
        pickerItems = r.workflows.map((w) => ({
          id: w.id,
          label: w.title,
          sublabel: domainLabel(w.domain),
        }))
      } else {
        const r = await templatesApi.list()
        pickerItems = r.docx_templates.map((tpl) => ({
          id: tpl.id,
          label: templateDisplayName(tpl, i18n.locale),
          sublabel: tpl.id,
        }))
      }
    } catch {
      pickerItems = []
    } finally {
      pickerLoading = false
    }
  }

  function onPick(ids: string[]) {
    const byId = (id: string) => pickerItems.find((i) => i.id === id)
    if (pickerKind === 'doc') {
      files = ids.map((id) => ({ document_id: id, filename: byId(id)?.label }))
    } else if (pickerKind === 'project') {
      const it = byId(ids[0])
      project = it ? { id: it.id, name: it.label } : null
    } else if (pickerKind === 'workflow') {
      const it = byId(ids[0])
      workflow = it ? { id: it.id, title: it.label } : null
    } else if (pickerKind === 'template') {
      const it = byId(ids[0])
      template = it ? { id: it.id, title: it.label } : null
    }
  }

  const pickerTitle = $derived(
    pickerKind === 'doc'
      ? t('Assistant.attachDocuments')
      : pickerKind === 'project'
        ? t('Assistant.attachProject')
        : pickerKind === 'workflow'
          ? t('Assistant.attachWorkflow')
          : t('Assistant.attachTemplate')
  )

  function send() {
    if (streaming || !text.trim()) return
    onsend(text, {
      files: files.length ? files : undefined,
      workflow: workflow ?? undefined,
      template: template ?? undefined,
      projectId: project?.id,
    })
    text = ''
    files = []
    workflow = null
    template = null
    project = null
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      send()
    }
  }

  const hasAttachments = $derived(
    files.length > 0 || workflow !== null || template !== null || project !== null
  )
</script>

<div class="border border-(--color-surface-200) rounded-(--radius-lg) bg-(--color-surface-0) shadow-(--shadow-sm)">
  {#if hasAttachments}
    <div class="flex flex-wrap gap-1.5 px-3 pt-3">
      {#each files as f (f.document_id)}
        <Badge tone="neutral">
          {f.filename ?? f.document_id}
          <button class="ml-1" aria-label="Remove" onclick={() => (files = files.filter((x) => x !== f))}>
            <X size={10} />
          </button>
        </Badge>
      {/each}
      {#if project}
        <Badge tone="brand">
          {project.name}
          <button class="ml-1" aria-label="Remove" onclick={() => (project = null)}><X size={10} /></button>
        </Badge>
      {/if}
      {#if workflow}
        <Badge tone="assistant">
          {workflow.title}
          <button class="ml-1" aria-label="Remove" onclick={() => (workflow = null)}><X size={10} /></button>
        </Badge>
      {/if}
      {#if template}
        <Badge tone="level">
          {template.title}
          <button class="ml-1" aria-label="Remove" onclick={() => (template = null)}><X size={10} /></button>
        </Badge>
      {/if}
    </div>
  {/if}

  <textarea
    bind:value={text}
    onkeydown={onKey}
    placeholder={t('Assistant.inputPlaceholder')}
    rows={2}
    class="w-full block resize-none bg-transparent px-3.5 py-3 text-sm leading-relaxed
           text-(--color-text-primary) placeholder:text-(--color-text-disabled)
           focus:outline-none"
  ></textarea>

  <div class="flex items-center gap-1 px-2.5 py-2 border-t border-(--color-surface-100)">
    <button type="button" title={t('Assistant.attachDocuments')} aria-label={t('Assistant.attachDocuments')}
      onclick={() => openPicker('doc')}
      class="inline-flex h-8 w-8 items-center justify-center rounded-(--radius-md) text-(--color-text-secondary) hover:bg-(--color-hover-bg) hover:text-(--color-text-primary)">
      <Paperclip size={16} />
    </button>
    <button type="button" title={t('Assistant.attachProject')} aria-label={t('Assistant.attachProject')}
      onclick={() => openPicker('project')}
      class="inline-flex h-8 w-8 items-center justify-center rounded-(--radius-md) text-(--color-text-secondary) hover:bg-(--color-hover-bg) hover:text-(--color-text-primary)">
      <FolderKanban size={16} />
    </button>
    <button type="button" title={t('Assistant.attachWorkflow')} aria-label={t('Assistant.attachWorkflow')}
      onclick={() => openPicker('workflow')}
      class="inline-flex h-8 w-8 items-center justify-center rounded-(--radius-md) text-(--color-text-secondary) hover:bg-(--color-hover-bg) hover:text-(--color-text-primary)">
      <WorkflowIcon size={16} />
    </button>
    <button type="button" title={t('Assistant.attachTemplate')} aria-label={t('Assistant.attachTemplate')}
      onclick={() => openPicker('template')}
      class="inline-flex h-8 w-8 items-center justify-center rounded-(--radius-md) text-(--color-text-secondary) hover:bg-(--color-hover-bg) hover:text-(--color-text-primary)">
      <FileType2 size={16} />
    </button>

    <div class="flex-1"></div>

    {#if streaming}
      <Button size="sm" variant="secondary" onclick={onstop}>{t('Common.stop')}</Button>
    {:else}
      <Button size="sm" disabled={!text.trim()} onclick={send}>{t('Common.send')}</Button>
    {/if}
  </div>
</div>

<PickerModal
  bind:open={pickerOpen}
  title={pickerTitle}
  items={pickerItems}
  loading={pickerLoading}
  multi={pickerKind === 'doc'}
  onpick={onPick}
/>
