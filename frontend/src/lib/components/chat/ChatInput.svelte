<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Chat composer: a textarea plus attachment pickers for documents,
  projects, workflows and templates. On send it emits the message text
  and the assembled attachment payload.
-->
<script lang="ts">
  import Button from '$lib/components/ui/Button.svelte'
  import Badge from '$lib/components/ui/Badge.svelte'
  import Modal from '$lib/components/ui/Modal.svelte'
  import PickerModal from '$lib/components/ui/PickerModal.svelte'
  import type { PickerItem } from '$lib/components/ui/PickerModal.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { modelsStore } from '$lib/stores/models.svelte'
  import { composerPrefill } from '$lib/stores/composer.svelte'
  import { documentsApi } from '$lib/api/documents'
  import { workflowsApi } from '$lib/api/workflows'
  import { templatesApi } from '$lib/api/templates'
  import { projectsApi } from '$lib/api/projects'
  import { templateDisplayName } from '$lib/types/template'
  import { DOMAINS, domainLabel } from '$lib/types/domain'
  import { userStore } from '$lib/stores/user.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { chatStore } from '$lib/stores/chat.svelte'
  import type { SendAttachments } from '$lib/stores/chat.svelte'
  import type { FileRef, TemplateRef, WorkflowRef } from '$lib/types/chat'
  import {
    Paperclip,
    FolderKanban,
    Workflow as WorkflowIcon,
    FileType2,
    X,
    Upload,
    FolderSearch,
    ChevronDown,
  } from 'lucide-svelte'

  /** Formats the backend can ingest (plus images for multimodal models). */
  const UPLOAD_ACCEPT =
    '.pdf,.docx,.doc,.rtf,.xlsx,.xls,.xlsb,.ods,.csv,.txt,.md,.png,.jpg,.jpeg,.tiff'

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

  // PII protection state. The per-file checkbox toggles
  // `f.piiProtected`; the first time the user turns one on (in any
  // chat, app-wide) we surface a disclaimer explaining the blackbox
  // nature of GLiNER2 and pointing at edito-pdf.com / Omissis for
  // production-grade redaction. Acknowledgement is sticky in
  // localStorage — the user sees it once until they clear the
  // browser cache or open a fresh install.
  const PII_ACK_KEY = 'mikerust:pii-disclaimer-ack'
  let piiDisclaimerOpen = $state(false)
  let pendingPiiFile = $state<FileRef | null>(null)

  function piiAcked(): boolean {
    try {
      return localStorage.getItem(PII_ACK_KEY) === '1'
    } catch {
      return false
    }
  }

  function applyPiiToggle(f: FileRef, next: boolean) {
    files = files.map((x) => (x === f ? { ...x, piiProtected: next } : x))
  }

  function onPiiToggle(f: FileRef, next: boolean) {
    if (next && !piiAcked()) {
      pendingPiiFile = f
      piiDisclaimerOpen = true
      return
    }
    applyPiiToggle(f, next)
  }

  function ackPiiDisclaimer() {
    try {
      localStorage.setItem(PII_ACK_KEY, '1')
    } catch {
      /* private mode etc. — the disclaimer just shows again next time */
    }
    if (pendingPiiFile) {
      applyPiiToggle(pendingPiiFile, true)
      pendingPiiFile = null
    }
    piiDisclaimerOpen = false
  }

  function cancelPiiDisclaimer() {
    pendingPiiFile = null
    piiDisclaimerOpen = false
  }
  let project = $state<{ id: string; name: string; domain?: string } | null>(null)

  // A chat that lives in a project auto-attaches that project: the chip
  // keeps the context visible and its domain scopes the workflow /
  // template pickers. Re-derives when the active chat changes.
  $effect(() => {
    const pid = chatStore.activeProjectId
    if (!pid || project?.id === pid) return
    void projectsApi
      .get(pid)
      .then((p) => {
        project = { id: p.id, name: p.name, domain: p.domain }
      })
      .catch(() => {
        /* leave the composer without a project chip */
      })
  })

  // ── pickers ─────────────────────────────────────────────────────────
  type Kind = 'doc' | 'project' | 'workflow' | 'template'
  let pickerKind = $state<Kind | null>(null)
  let pickerOpen = $state(false)
  let pickerItems = $state<PickerItem[]>([])
  let pickerLoading = $state(false)
  // Domain filter for the workflow and template pickers — resets to the
  // user's default domain on open.
  let pickerDomain = $state('')
  const pickerFilterOptions = $derived(
    pickerKind === 'workflow' || pickerKind === 'template'
      ? [
          { value: '', label: t('Domains.filterPlaceholder') },
          ...DOMAINS.map((d) => ({ value: d, label: domainLabel(d) })),
        ]
      : undefined
  )

  // Consume a template queued by the DOCX-templates "Apply to chat" flow.
  $effect(() => {
    const queued = composerPrefill.takeTemplate()
    if (queued) template = queued
  })

  // ── document upload ─────────────────────────────────────────────────
  let attachMenuOpen = $state(false)
  let uploading = $state(false)
  let fileInput = $state<HTMLInputElement>()

  function triggerUpload() {
    attachMenuOpen = false
    fileInput?.click()
  }

  async function onFilesChosen(e: Event) {
    const input = e.currentTarget as HTMLInputElement
    const chosen = Array.from(input.files ?? [])
    input.value = ''
    if (chosen.length === 0) return
    uploading = true
    try {
      for (const f of chosen) {
        // `cache` — composer uploads live in the cache pool.
        const doc = await documentsApi.upload(f, { cache: true })
        files = [...files, { document_id: doc.id, filename: doc.filename }]
      }
    } catch (err) {
      toastStore.danger(t('Documents.viewer.errorLoading'), {
        detail: (err as Error).message,
      })
    } finally {
      uploading = false
    }
  }

  async function openPicker(kind: Kind) {
    pickerKind = kind
    pickerItems = []
    pickerLoading = true
    pickerOpen = true
    // The workflow and template pickers default to the user's domain;
    // resets each open.
    // Workflow / template pickers scope to the chat's project domain
    // when the chat lives in a project, else the user's default domain.
    pickerDomain =
      kind === 'workflow' || kind === 'template'
        ? (project?.domain ?? userStore.defaultDomain)
        : ''
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
          tag: w.domain,
        }))
      } else {
        const r = await templatesApi.list()
        pickerItems = r.docx_templates.map((tpl) => ({
          id: tpl.id,
          label: templateDisplayName(tpl, i18n.locale),
          sublabel: tpl.id,
          tag: tpl.domain,
          // A template surfaces under its primary domain and any of its
          // also-applicable ones, matching the Templates screen filter.
          tags: [tpl.domain, ...tpl.also_applicable_to],
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
    // Keep the chat's own project attached (persistent context);
    // clear only a manually-picked project attachment.
    if (project && project.id !== chatStore.activeProjectId) project = null
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

  // ── model picker ────────────────────────────────────────────────────
  $effect(() => {
    if (!modelsStore.catalogue && !modelsStore.loading) void modelsStore.load()
  })

  function keyset(v: string | null | undefined): boolean {
    return !!(v && v.trim())
  }

  // Only models from configured providers; ids carry the dispatch prefix
  // the backend expects. Falls back to every model when no key is visible.
  const modelOptions = $derived.by(() => {
    const s = modelsStore.settings
    const configured = new Set<string>()
    if (keyset(s.claude_api_key)) configured.add('anthropic')
    if (keyset(s.gemini_api_key)) configured.add('google')
    if (keyset(s.openai_api_key)) configured.add('openai')
    if (keyset(s.mistral_api_key)) configured.add('mistral')

    const opts = modelsStore.allModels
      .filter((m) => configured.size === 0 || configured.has(m.providerId))
      .map((m) => ({
        value:
          m.providerId === 'openai'
            ? `openai:${m.id}`
            : m.providerId === 'mistral'
              ? `mistral:${m.id}`
              : m.id,
        label: m.display_name,
      }))
    if (keyset(s.local_base_url) && keyset(s.local_model)) {
      opts.push({ value: `local:${s.local_model}`, label: s.local_model as string })
    }
    return opts
  })

  const currentModel = $derived(modelsStore.settings.main_model ?? '')

  function pickModel(value: string) {
    if (value && value !== modelsStore.settings.main_model) {
      void modelsStore.save({ main_model: value })
    }
  }
</script>

<div class="border border-(--color-surface-200) rounded-(--radius-lg) bg-(--color-surface-0) shadow-(--shadow-sm)">
  {#if hasAttachments}
    <div class="flex flex-wrap gap-1.5 px-3 pt-3">
      {#each files as f (f.document_id)}
        <Badge tone="neutral">
          <span class="inline-flex items-center gap-1.5">
            <label
              class="inline-flex items-center gap-1 text-[10px] uppercase tracking-wide
                     cursor-pointer select-none"
              title={t('ChatInput.pii.tooltip')}
            >
              PII
              <input
                type="checkbox"
                class="h-3 w-3 accent-(--color-brand-500) cursor-pointer"
                checked={f.piiProtected ?? false}
                onchange={(e) => onPiiToggle(f, (e.currentTarget as HTMLInputElement).checked)}
              />
            </label>
            <span class="truncate max-w-48">{f.filename ?? f.document_id}</span>
            <button
              class="ml-0.5"
              aria-label={t('Common.delete')}
              onclick={() => (files = files.filter((x) => x !== f))}
            >
              <X size={10} />
            </button>
          </span>
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

  <input
    bind:this={fileInput}
    type="file"
    multiple
    accept={UPLOAD_ACCEPT}
    class="hidden"
    onchange={onFilesChosen}
  />

  <div class="flex items-center gap-1 px-2.5 py-2 border-t border-(--color-surface-100)">
    <div class="relative">
      <button
        type="button"
        title={t('Assistant.attachDocuments')}
        aria-label={t('Assistant.attachDocuments')}
        onclick={() => (attachMenuOpen = !attachMenuOpen)}
        class="inline-flex h-8 w-8 items-center justify-center rounded-(--radius-md)
               text-(--color-text-secondary) hover:bg-(--color-hover-bg) hover:text-(--color-text-primary)
               {uploading ? 'animate-pulse' : ''}"
      >
        <Paperclip size={16} />
      </button>
      {#if attachMenuOpen}
        <button
          type="button"
          class="fixed inset-0 z-10 cursor-default"
          aria-label={t('Common.close')}
          onclick={() => (attachMenuOpen = false)}
        ></button>
        <div
          class="absolute bottom-full mb-1.5 left-0 z-20 w-48 py-1
                 rounded-(--radius-md) border border-(--color-surface-200)
                 bg-(--color-surface-0) shadow-(--shadow-modal)"
        >
          <button
            type="button"
            onclick={triggerUpload}
            class="flex w-full items-center gap-2 px-3 h-8 text-sm text-left
                   text-(--color-text-primary) hover:bg-(--color-hover-bg)"
          >
            <Upload size={14} />{t('Assistant.uploadFiles')}
          </button>
          <button
            type="button"
            onclick={() => {
              attachMenuOpen = false
              openPicker('doc')
            }}
            class="flex w-full items-center gap-2 px-3 h-8 text-sm text-left
                   text-(--color-text-primary) hover:bg-(--color-hover-bg)"
          >
            <FolderSearch size={14} />{t('Assistant.browseAll')}
          </button>
        </div>
      {/if}
    </div>
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

    {#if modelOptions.length}
      <div class="relative">
        <select
          value={currentModel}
          onchange={(e) => pickModel((e.currentTarget as HTMLSelectElement).value)}
          aria-label={t('Assistant.selectModel')}
          class="h-8 max-w-44 appearance-none rounded-(--radius-md) bg-transparent pl-1.5 pr-6 text-xs
                 text-(--color-text-secondary) hover:text-(--color-text-primary) cursor-pointer
                 focus:outline-none focus-visible:ring-2 focus-visible:ring-(--color-brand-500)"
        >
          {#if !currentModel}
            <option value="">{t('Assistant.selectModel')}</option>
          {/if}
          {#each modelOptions as opt (opt.value)}
            <option value={opt.value}>{opt.label}</option>
          {/each}
        </select>
        <span
          class="pointer-events-none absolute right-1 top-1/2 -translate-y-1/2 text-(--color-text-secondary)"
        >
          <ChevronDown size={14} />
        </span>
      </div>
    {/if}

    {#if streaming}
      <Button size="sm" variant="pill" onclick={onstop}>{t('Common.stop')}</Button>
    {:else}
      <Button size="sm" disabled={!text.trim()} onclick={send}>{t('Common.send')}</Button>
    {/if}
  </div>
</div>

<!-- One-shot PII disclaimer: fires the first time the user turns
     on the per-file PII checkbox; acknowledgement is sticky in
     localStorage so they don't see it every time. Cancel reverts
     the pending toggle; Acknowledge applies it and remembers the ack. -->
<Modal
  bind:open={piiDisclaimerOpen}
  size="md"
  title={t('ChatInput.pii.disclaimerTitle')}
  onclose={cancelPiiDisclaimer}
>
  <div class="space-y-3 text-sm text-(--color-text-primary) leading-relaxed">
    <p>{t('ChatInput.pii.disclaimerBody')}</p>
    <p class="text-(--color-text-secondary)">
      {t('ChatInput.pii.omissisHintPrefix')}
      <a
        href="https://edito-pdf.com"
        target="_blank"
        rel="noopener noreferrer"
        class="text-(--color-brand-600) underline hover:text-(--color-brand-700)"
      >edito-pdf.com</a>{t('ChatInput.pii.omissisHintSuffix')}
    </p>
  </div>
  {#snippet footer()}
    <Button size="sm" variant="ghost" onclick={cancelPiiDisclaimer}>
      {t('Common.cancel')}
    </Button>
    <Button size="sm" onclick={ackPiiDisclaimer}>
      {t('ChatInput.pii.acknowledge')}
    </Button>
  {/snippet}
</Modal>

<PickerModal
  bind:open={pickerOpen}
  title={pickerTitle}
  items={pickerItems}
  loading={pickerLoading}
  multi={pickerKind === 'doc'}
  filterOptions={pickerFilterOptions}
  bind:filterValue={pickerDomain}
  onpick={onPick}
/>
