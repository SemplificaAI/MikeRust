<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import Badge from '$lib/components/ui/Badge.svelte'
  import { renderMarkdown } from '$lib/utils/markdown'
  import type { ChatMessage } from '$lib/types/chat'
  import { FileText, Workflow as WorkflowIcon, FileType2 } from 'lucide-svelte'

  let { message }: { message: ChatMessage } = $props()

  const html = $derived(
    message.role === 'assistant' ? renderMarkdown(message.content) : ''
  )
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
    <div class="md-body text-sm text-(--color-text-primary)">
      {@html html}{#if message.streaming}<span class="streaming-caret"></span>{/if}
    </div>
  </div>
{/if}
