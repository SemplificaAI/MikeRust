<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Assistant screen — chat history pane + active conversation with
  streaming replies and a composer that attaches documents, projects,
  workflows and templates.
-->
<script lang="ts">
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import ChatMessage from '$lib/components/chat/ChatMessage.svelte'
  import ChatInput from '$lib/components/chat/ChatInput.svelte'
  import { chatStore, type SendAttachments } from '$lib/stores/chat.svelte'
  import { userStore } from '$lib/stores/user.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { Plus, MessageSquare, Trash2 } from 'lucide-svelte'

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  let scroller: HTMLDivElement | undefined = $state()

  $effect(() => {
    void chatStore.refreshChats()
  })

  // Auto-scroll to the bottom as messages grow / stream.
  $effect(() => {
    // touch the reactive deps
    void chatStore.messages.length
    void chatStore.messages.at(-1)?.content
    if (scroller) {
      queueMicrotask(() => {
        if (scroller) scroller.scrollTop = scroller.scrollHeight
      })
    }
  })

  const greetingName = $derived(
    userStore.profile?.display_name ?? userStore.profile?.username ?? ''
  )

  function onsend(text: string, attach: SendAttachments) {
    void chatStore.send(text, attach)
  }
</script>

<div class="flex h-full">
  <!-- chat history -->
  <aside class="w-64 shrink-0 flex flex-col border-r border-(--color-surface-200) bg-(--color-surface-50)">
    <div class="p-2">
      <Button variant="secondary" full onclick={() => chatStore.newChat()}>
        {#snippet iconBefore()}<Plus size={15} />{/snippet}
        {t('Assistant.newChat')}
      </Button>
    </div>
    <div class="flex-1 overflow-y-auto px-2 pb-2 flex flex-col gap-0.5">
      {#if chatStore.loadingChats && chatStore.chats.length === 0}
        <div class="flex justify-center py-6 text-(--color-text-secondary)"><Spinner size="sm" /></div>
      {:else}
        {#each chatStore.chats as c (c.id)}
          <div
            class="group flex items-center gap-1 rounded-(--radius-md)
                   {chatStore.activeId === c.id ? 'bg-(--color-active-bg)' : 'hover:bg-(--color-hover-bg)'}"
          >
            <button
              type="button"
              onclick={() => chatStore.selectChat(c.id)}
              class="flex-1 min-w-0 flex items-center gap-2 px-2.5 h-9 text-left
                     text-sm focus:outline-none
                     {chatStore.activeId === c.id ? 'text-(--color-brand-700)' : 'text-(--color-text-secondary)'}"
            >
              <MessageSquare size={14} class="shrink-0" />
              <span class="truncate">{c.title || t('Assistant.untitledChat')}</span>
            </button>
            <IconButton
              label={t('Common.delete')}
              size="sm"
              class="opacity-0 group-hover:opacity-100"
              onclick={() => chatStore.remove(c.id)}
            >
              <Trash2 size={13} />
            </IconButton>
          </div>
        {/each}
      {/if}
    </div>
  </aside>

  <!-- conversation -->
  <div class="flex-1 flex flex-col min-w-0">
    <div bind:this={scroller} class="flex-1 overflow-y-auto">
      {#if chatStore.messages.length === 0}
        <div class="h-full flex flex-col items-center justify-center text-center px-6 gap-2">
          <h2 class="text-2xl font-semibold text-(--color-text-primary)">
            {t('Assistant.greeting', { name: greetingName })}
          </h2>
          <p class="text-sm text-(--color-text-secondary) max-w-md">
            {t('Assistant.emptyHint')}
          </p>
        </div>
      {:else}
        <div class="max-w-3xl mx-auto px-6 py-6 flex flex-col gap-4">
          {#each chatStore.messages as msg, i (i)}
            <ChatMessage message={msg} />
          {/each}
        </div>
      {/if}
    </div>

    <div class="max-w-3xl w-full mx-auto px-6 pb-5 pt-1">
      {#if chatStore.error}
        <p class="text-xs text-(--color-danger-500) mb-2">{chatStore.error}</p>
      {/if}
      <ChatInput streaming={chatStore.streaming} {onsend} onstop={() => chatStore.abort()} />
    </div>
  </div>
</div>
