<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Assistant screen — the active conversation. The chat history lives in
  the app sidebar; this screen is just the message stream + composer.
-->
<script lang="ts">
  import ChatMessage from '$lib/components/chat/ChatMessage.svelte'
  import ChatInput from '$lib/components/chat/ChatInput.svelte'
  import { chatStore, type SendAttachments } from '$lib/stores/chat.svelte'
  import { userStore } from '$lib/stores/user.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  let scroller: HTMLDivElement | undefined = $state()

  // Auto-scroll to the bottom as messages grow / stream.
  $effect(() => {
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

<div class="flex flex-col h-full">
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
