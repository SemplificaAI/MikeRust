<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Settings screen. Vertical sub-nav + active section. Profile, Security
  and Danger zone are implemented; Models / MCP / Data sources are
  listed as upcoming sections (their API layers land in later passes).
-->
<script lang="ts">
  import ProfileSection from '$lib/components/settings/ProfileSection.svelte'
  import SecuritySection from '$lib/components/settings/SecuritySection.svelte'
  import ModelsSection from '$lib/components/settings/ModelsSection.svelte'
  import McpSection from '$lib/components/settings/McpSection.svelte'
  import DataSourcesSection from '$lib/components/settings/DataSourcesSection.svelte'
  import DangerZoneSection from '$lib/components/settings/DangerZoneSection.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'

  type SectionId = 'profile' | 'security' | 'models' | 'mcp' | 'data' | 'danger'

  interface SectionEntry {
    id: SectionId
    labelKey: string
    ready: boolean
  }

  const sections: SectionEntry[] = [
    { id: 'profile', labelKey: 'Account.profile', ready: true },
    { id: 'security', labelKey: 'Account.security', ready: true },
    { id: 'models', labelKey: 'Settings.llmModels', ready: true },
    { id: 'mcp', labelKey: 'Settings.mcpServers', ready: true },
    { id: 'data', labelKey: 'Settings.dataSources', ready: true },
    { id: 'danger', labelKey: 'Settings.dangerZone', ready: true },
  ]

  let active = $state<SectionId>('profile')
</script>

<div class="max-w-4xl mx-auto p-8">
  <h2 class="text-2xl font-semibold text-(--color-text-primary) mb-5">{i18n.t('Common.settings')}</h2>

  <div class="flex gap-8 items-start">
    <nav class="w-44 shrink-0 flex flex-col gap-0.5">
      {#each sections as s (s.id)}
        <button
          type="button"
          disabled={!s.ready}
          onclick={() => (active = s.id)}
          class="text-left px-3 h-9 rounded-(--radius-md) text-sm
                 transition-colors duration-(--transition-fast)
                 focus:outline-none focus-visible:ring-2 focus-visible:ring-(--color-brand-500)
                 disabled:opacity-40 disabled:cursor-not-allowed
                 {active === s.id
                   ? 'bg-(--color-active-bg) text-(--color-brand-700) font-medium'
                   : 'text-(--color-text-secondary) hover:bg-(--color-hover-bg) hover:text-(--color-text-primary)'}"
        >
          {i18n.t(s.labelKey)}
          {#if !s.ready}
            <span class="text-[10px] uppercase tracking-wide ml-1">{i18n.t('Ui.soon')}</span>
          {/if}
        </button>
      {/each}
    </nav>

    <div class="flex-1 min-w-0">
      {#if active === 'profile'}
        <ProfileSection />
      {:else if active === 'security'}
        <SecuritySection />
      {:else if active === 'models'}
        <ModelsSection />
      {:else if active === 'mcp'}
        <McpSection />
      {:else if active === 'data'}
        <DataSourcesSection />
      {:else if active === 'danger'}
        <DangerZoneSection />
      {:else}
        <EmptyState
          title={i18n.t('Ui.comingSoonShort')}
          description={i18n.t('Ui.comingSoonBody')}
        />
      {/if}
    </div>
  </div>
</div>
