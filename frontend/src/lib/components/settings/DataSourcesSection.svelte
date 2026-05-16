<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Settings → Data sources. Sub-nav over the document corpora the user
  can index into the RAG knowledge base: local folder sync and EUR-Lex.
-->
<script lang="ts">
  import SyncSection from './SyncSection.svelte'
  import EurlexSection from './EurlexSection.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'

  type Source = 'sync' | 'eurlex'
  let active = $state<Source>('sync')

  const sources: { id: Source; labelKey: string }[] = [
    { id: 'sync', labelKey: 'Account.localDocsLink' },
    { id: 'eurlex', labelKey: 'Account.eurlexLink' },
  ]
</script>

<div class="space-y-4">
  <div class="flex gap-1 border-b border-(--color-surface-200)">
    {#each sources as s (s.id)}
      <button
        type="button"
        onclick={() => (active = s.id)}
        class="px-3 h-9 text-sm border-b-2 -mb-px
               {active === s.id
                 ? 'border-(--color-brand-500) text-(--color-text-primary) font-medium'
                 : 'border-transparent text-(--color-text-secondary) hover:text-(--color-text-primary)'}"
      >
        {i18n.t(s.labelKey)}
      </button>
    {/each}
  </div>

  {#if active === 'sync'}
    <SyncSection />
  {:else}
    <EurlexSection />
  {/if}
</div>
