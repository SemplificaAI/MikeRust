<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Authenticated app shell: sidebar nav + topbar + the active feature
  route. Workflows is fully implemented; the other feature routes show
  a placeholder until their phase lands.
-->
<script lang="ts">
  import AppShell from '$lib/components/layout/AppShell.svelte'
  import Sidebar from '$lib/components/layout/Sidebar.svelte'
  import SidebarItem from '$lib/components/layout/SidebarItem.svelte'
  import TopBar from '$lib/components/layout/TopBar.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import ThemeToggle from '$lib/components/ui/ThemeToggle.svelte'
  import Workflows from './Workflows.svelte'
  import { router, type FeatureRoute } from '$lib/stores/router.svelte'
  import { authStore } from '$lib/stores/auth.svelte'
  import { userStore } from '$lib/stores/user.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import {
    MessageSquare,
    FolderKanban,
    Table2,
    Workflow,
    FileText,
    Settings,
  } from 'lucide-svelte'

  interface NavEntry {
    route: FeatureRoute
    label: string
    icon: typeof MessageSquare
  }

  const nav: NavEntry[] = [
    { route: 'assistant', label: 'Assistant', icon: MessageSquare },
    { route: 'projects', label: 'Projects', icon: FolderKanban },
    { route: 'tabular', label: 'Tabular reviews', icon: Table2 },
    { route: 'workflows', label: 'Workflows', icon: Workflow },
    { route: 'templates', label: 'Templates', icon: FileText },
    { route: 'settings', label: 'Settings', icon: Settings },
  ]

  const activeLabel = $derived(
    nav.find((n) => n.route === router.current)?.label ?? 'MikeRust'
  )

  const greetingName = $derived(
    userStore.profile?.display_name ??
      authStore.user?.display_name ??
      authStore.user?.username ??
      ''
  )

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
      {#each nav as entry (entry.route)}
        <SidebarItem
          label={entry.label}
          active={router.current === entry.route}
          onclick={() => router.go(entry.route)}
        >
          {#snippet icon()}
            <entry.icon size={16} />
          {/snippet}
        </SidebarItem>
      {/each}
    </Sidebar>
  {/snippet}

  {#snippet topbar()}
    <TopBar title={activeLabel}>
      {#snippet actions()}
        <ThemeToggle />
        {#if greetingName}
          <span class="text-xs text-(--color-text-secondary)">{greetingName}</span>
        {/if}
        <Button size="sm" variant="ghost" onclick={logout}>Sign out</Button>
      {/snippet}
    </TopBar>
  {/snippet}

  {#if router.current === 'workflows'}
    <Workflows />
  {:else}
    <div class="p-8">
      <EmptyState
        title="{activeLabel} — coming soon"
        description="This screen is built in a later phase of the UI rewrite."
      />
    </div>
  {/if}
</AppShell>
