<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Post-unlock landing. Placeholder shell for Fase 2/3 — the real feature
  routes (Assistant, Projects, Tabular, Workflows, Templates, Settings)
  arrive in later phases. For now it proves the authenticated session is
  live: it renders the app layout, the greeting, a /healthz diagnostics
  card, and a working logout.
-->
<script lang="ts">
  import AppShell from '$lib/components/layout/AppShell.svelte'
  import Sidebar from '$lib/components/layout/Sidebar.svelte'
  import SidebarItem from '$lib/components/layout/SidebarItem.svelte'
  import TopBar from '$lib/components/layout/TopBar.svelte'
  import Card from '$lib/components/ui/Card.svelte'
  import Badge from '$lib/components/ui/Badge.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import { authStore } from '$lib/stores/auth.svelte'
  import { userStore } from '$lib/stores/user.svelte'
  import { router } from '$lib/stores/router.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { healthApi } from '$lib/api/health'
  import type { HealthReport } from '$lib/types/health'

  const greetingName = $derived(
    userStore.profile?.display_name ??
      authStore.user?.display_name ??
      authStore.user?.username ??
      'there'
  )

  // Placeholder nav — feature routes land in Fase 4-5.
  const navItems = [
    'Assistant',
    'Projects',
    'Tabular Reviews',
    'Workflows',
    'Templates',
    'Settings',
  ]
  let activeNav = $state('Assistant')

  let health = $state<HealthReport | null>(null)
  $effect(() => {
    healthApi
      .get()
      .then((h) => (health = h))
      .catch(() => {
        /* diagnostics card just stays empty */
      })
  })

  async function logout() {
    await authStore.logout()
    userStore.reset()
    toastStore.info('Signed out')
    router.go('unlock')
  }
</script>

<AppShell>
  {#snippet sidebar()}
    <Sidebar>
      {#snippet brand()}
        <span class="text-base font-semibold text-(--color-brand-600)">MikeRust</span>
      {/snippet}
      {#each navItems as item (item)}
        <SidebarItem
          label={item}
          active={item === activeNav}
          onclick={() => (activeNav = item)}
        />
      {/each}
    </Sidebar>
  {/snippet}

  {#snippet topbar()}
    <TopBar title={activeNav}>
      {#snippet actions()}
        <span class="text-xs text-(--color-text-secondary)">
          {authStore.user?.username}
        </span>
        <Button size="sm" variant="ghost" onclick={logout}>Sign out</Button>
      {/snippet}
    </TopBar>
  {/snippet}

  <div class="max-w-3xl mx-auto p-8 space-y-6">
    <header class="space-y-1">
      <h2 class="text-2xl font-semibold text-(--color-text-primary)">
        Hello, {greetingName}
      </h2>
      <p class="text-sm text-(--color-text-secondary)">
        Authenticated session is live. Feature screens land in the next phases.
      </p>
    </header>

    {#if health}
      <Card title="Backend diagnostics" subtitle="GET /healthz">
        <dl class="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
          <dt class="text-(--color-text-secondary)">Status</dt>
          <dd>
            <Badge tone={health.status === 'ok' ? 'success' : 'warning'}>
              {health.status}
            </Badge>
          </dd>
          <dt class="text-(--color-text-secondary)">Version</dt>
          <dd class="font-mono">{health.version}</dd>
          <dt class="text-(--color-text-secondary)">RAG</dt>
          <dd class="font-mono">{health.rag.status}</dd>
          <dt class="text-(--color-text-secondary)">Workflows</dt>
          <dd class="font-mono">{health.presets.workflows}</dd>
          <dt class="text-(--color-text-secondary)">DOCX templates</dt>
          <dd class="font-mono">{health.presets.docx_templates}</dd>
          <dt class="text-(--color-text-secondary)">LLM providers</dt>
          <dd class="font-mono">{health.presets.model_providers}</dd>
        </dl>
      </Card>
    {/if}

    <Card padded={false}>
      <EmptyState
        title="{activeNav} — coming soon"
        description="This screen will be built in a later phase of the UI rewrite."
      />
    </Card>
  </div>
</AppShell>
