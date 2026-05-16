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
  import DangerZoneSection from '$lib/components/settings/DangerZoneSection.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'

  type SectionId = 'profile' | 'security' | 'models' | 'mcp' | 'data' | 'danger'

  interface SectionEntry {
    id: SectionId
    label: string
    ready: boolean
  }

  const sections: SectionEntry[] = [
    { id: 'profile', label: 'Profile', ready: true },
    { id: 'security', label: 'Security', ready: true },
    { id: 'models', label: 'LLM models', ready: true },
    { id: 'mcp', label: 'MCP servers', ready: false },
    { id: 'data', label: 'Data sources', ready: false },
    { id: 'danger', label: 'Danger zone', ready: true },
  ]

  let active = $state<SectionId>('profile')
</script>

<div class="max-w-4xl mx-auto p-8">
  <h2 class="text-2xl font-semibold text-(--color-text-primary) mb-5">Settings</h2>

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
          {s.label}
          {#if !s.ready}
            <span class="text-[10px] uppercase tracking-wide ml-1">soon</span>
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
      {:else if active === 'danger'}
        <DangerZoneSection />
      {:else}
        <EmptyState
          title="Coming soon"
          description="This settings section is built in a later phase of the UI rewrite."
        />
      {/if}
    </div>
  </div>
</div>
