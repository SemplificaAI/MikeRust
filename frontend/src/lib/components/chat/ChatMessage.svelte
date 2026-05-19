<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import Badge from '$lib/components/ui/Badge.svelte'
  import Logo from '$lib/components/ui/Logo.svelte'
  import ChatSteps from './ChatSteps.svelte'
  import { renderMessageHtml } from '$lib/utils/citations'
  import { docViewer } from '$lib/stores/doc-viewer.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import type { ChatMessage } from '$lib/types/chat'
  import { FileText, Workflow as WorkflowIcon, FileType2 } from 'lucide-svelte'

  let { message }: { message: ChatMessage } = $props()

  const html = $derived(
    message.role === 'assistant'
      ? renderMessageHtml(message.content, message.citations ?? [])
      : ''
  )

  // While the model streams the trailing <CITATIONS> block, the visible
  // text stops growing — surface a label so it doesn't look stuck.
  const gatheringSources = $derived(
    message.streaming === true && /<citations>/i.test(message.content)
  )

  /** Open the document behind a clicked citation pill. */
  function openCitation(ref: string) {
    const c = (message.citations ?? []).find((x) => x.ref === ref)
    if (c) docViewer.openCitation(c)
  }

  function onBodyClick(e: MouseEvent) {
    const pill = (e.target as HTMLElement).closest<HTMLElement>('[data-cite-ref]')
    if (pill?.dataset.citeRef) openCitation(pill.dataset.citeRef)
  }

  function onBodyKeydown(e: KeyboardEvent) {
    if (e.key !== 'Enter' && e.key !== ' ') return
    const pill = (e.target as HTMLElement).closest<HTMLElement>('[data-cite-ref]')
    if (pill?.dataset.citeRef) {
      e.preventDefault()
      openCitation(pill.dataset.citeRef)
    }
  }
</script>

{#if message.role === 'user'}
  <div class="flex justify-end">
    <div class="max-w-[80%] bg-(--color-surface-100) rounded-(--radius-lg) px-3.5 py-2.5 space-y-1.5">
      {#if message.workflow || message.template || (message.files && message.files.length)}
        <div class="flex flex-wrap gap-1">
          {#if message.workflow}
            <Badge tone="assistant" size="xs">
              <WorkflowIcon size={10} class="mr-1" />{message.workflow.title}
            </Badge>
          {/if}
          {#if message.template}
            <Badge tone="level" size="xs">
              <FileType2 size={10} class="mr-1" />{message.template.title}
            </Badge>
          {/if}
          {#each message.files ?? [] as f (f.document_id)}
            <Badge tone="neutral" size="xs">
              <FileText size={10} class="mr-1" />{f.filename ?? f.document_id}
            </Badge>
          {/each}
        </div>
      {/if}
      <p class="text-sm text-(--color-text-primary) whitespace-pre-wrap break-words">
        {message.content}
      </p>
    </div>
  </div>
{:else}
  <div class="max-w-[85%]">
    {#if message.reasoning}
      <details class="mb-2">
        <summary
          class="cursor-pointer select-none text-xs text-(--color-text-secondary) hover:text-(--color-text-primary)"
        >
          {i18n.t('Assistant.reasoning')}
        </summary>
        <div
          class="mt-1 pl-3 border-l-2 border-(--color-surface-200) text-xs
                 text-(--color-text-secondary) whitespace-pre-wrap break-words"
        >
          {message.reasoning}
        </div>
      </details>
    {/if}
    {#if message.steps && message.steps.length}
      <ChatSteps steps={message.steps} />
    {/if}
    <!-- Citation pills inside the body are delegated click targets. -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="md-body text-sm text-(--color-text-primary)"
      onclick={onBodyClick}
      onkeydown={onBodyKeydown}
    >
      {@html html}{#if message.streaming}<span
          class="inline-flex items-center gap-1.5 align-text-bottom ml-1"
          ><Logo size={15} activity="thinking" />{#if gatheringSources}<span
              class="text-xs text-(--color-text-secondary)"
              >{i18n.t('Assistant.gatheringSources')}</span
            >{/if}</span
        >{/if}
    </div>
  </div>
{/if}
