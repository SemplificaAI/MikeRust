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
  import Templates from './Templates.svelte'
  import Tabular from './Tabular.svelte'
  import Projects from './Projects.svelte'
  import Assistant from './Assistant.svelte'
  import Settings from './Settings.svelte'
  import { router, type FeatureRoute } from '$lib/stores/router.svelte'
  import { authStore } from '$lib/stores/auth.svelte'
  import { userStore } from '$lib/stores/user.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import {
    MessageSquare,
    FolderKanban,
    Table2,
    Workflow,
    FileText,
    Settings as SettingsIcon,
  } from 'lucide-svelte'

  interface NavEntry {
    route: FeatureRoute
    labelKey: string
    icon: typeof MessageSquare
  }

  const nav: NavEntry[] = [
    { route: 'assistant', labelKey: 'Sidebar.assistant', icon: MessageSquare },
    { route: 'projects', labelKey: 'Sidebar.projects', icon: FolderKanban },
    { route: 'tabular', labelKey: 'Sidebar.tabularReviews', icon: Table2 },
    { route: 'workflows', labelKey: 'Sidebar.workflows', icon: Workflow },
    { route: 'templates', labelKey: 'Sidebar.docxTemplates', icon: FileText },
    { route: 'settings', labelKey: 'Common.settings', icon: SettingsIcon },
  ]

  const activeLabel = $derived.by(() => {
    const entry = nav.find((n) => n.route === router.current)
    return entry ? i18n.t(entry.labelKey) : 'MikeRust'
  })

  const greetingName = $derived(
    userStore.profile?.display_name ??
      authStore.user?.display_name ??
      authStore.user?.username ??
      ''
  )

  async function logout() {
    await authStore.logout()
    userStore.reset()
    toastStore.info(i18n.t('Common.logout'))
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
          label={i18n.t(entry.labelKey)}
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
        <Button size="sm" variant="ghost" onclick={logout}>{i18n.t('Common.logout')}</Button>
      {/snippet}
    </TopBar>
  {/snippet}

  {#if router.current === 'assistant'}
    <Assistant />
  {:else if router.current === 'workflows'}
    <Workflows />
  {:else if router.current === 'templates'}
    <Templates />
  {:else if router.current === 'tabular'}
    <Tabular />
  {:else if router.current === 'projects'}
    <Projects />
  {:else if router.current === 'settings'}
    <Settings />
  {:else}
    <div class="p-8">
      <EmptyState
        title={i18n.t('Ui.comingSoonTitle', { screen: activeLabel })}
        description={i18n.t('Ui.comingSoonBody')}
      />
    </div>
  {/if}
</AppShell>
