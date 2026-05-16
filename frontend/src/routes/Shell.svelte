<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Authenticated app shell: sidebar (nav + collapsible chat list +
  pinned Settings) + topbar + the active feature route.
-->
<script lang="ts">
  import AppShell from '$lib/components/layout/AppShell.svelte'
  import Sidebar from '$lib/components/layout/Sidebar.svelte'
  import SidebarItem from '$lib/components/layout/SidebarItem.svelte'
  import TopBar from '$lib/components/layout/TopBar.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
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
  import { chatStore } from '$lib/stores/chat.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import {
    MessageSquare,
    FolderKanban,
    Table2,
    Workflow,
    FileText,
    Settings as SettingsIcon,
    Plus,
    ChevronDown,
    ChevronRight,
    Trash2,
  } from 'lucide-svelte'

  interface NavEntry {
    route: FeatureRoute
    labelKey: string
    icon: typeof MessageSquare
  }

  // Assistant is rendered separately (it carries the new-chat "+");
  // these are the remaining feature routes.
  const nav: NavEntry[] = [
    { route: 'projects', labelKey: 'Sidebar.projects', icon: FolderKanban },
    { route: 'tabular', labelKey: 'Sidebar.tabularReviews', icon: Table2 },
    { route: 'workflows', labelKey: 'Sidebar.workflows', icon: Workflow },
    { route: 'templates', labelKey: 'Sidebar.docxTemplates', icon: FileText },
  ]

  const titleByRoute: Record<string, string> = {
    assistant: 'Sidebar.assistant',
    projects: 'Sidebar.projects',
    tabular: 'Sidebar.tabularReviews',
    workflows: 'Sidebar.workflows',
    templates: 'Sidebar.docxTemplates',
    settings: 'Common.settings',
  }
  const activeLabel = $derived(
    titleByRoute[router.current] ? i18n.t(titleByRoute[router.current]) : 'MikeRust'
  )

  const greetingName = $derived(
    userStore.profile?.display_name ??
      authStore.user?.display_name ??
      authStore.user?.username ??
      ''
  )

  let chatsCollapsed = $state(false)

  $effect(() => {
    void chatStore.refreshChats()
  })

  function newChat() {
    chatStore.newChat()
    router.go('assistant')
  }
  function openChat(id: string) {
    void chatStore.selectChat(id)
    router.go('assistant')
  }

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

      <!-- nav -->
      <div class="px-2 pt-2 flex flex-col gap-0.5 shrink-0">
        <!-- Assistant row carries the new-chat button -->
        <div class="flex items-center gap-1">
          <div class="flex-1 min-w-0">
            <SidebarItem
              label={i18n.t('Sidebar.assistant')}
              active={router.current === 'assistant'}
              onclick={() => router.go('assistant')}
            >
              {#snippet icon()}<MessageSquare size={16} />{/snippet}
            </SidebarItem>
          </div>
          <IconButton label={i18n.t('Assistant.newChat')} size="md" onclick={newChat}>
            <Plus size={16} />
          </IconButton>
        </div>

        {#each nav as entry (entry.route)}
          <SidebarItem
            label={i18n.t(entry.labelKey)}
            active={router.current === entry.route}
            onclick={() => router.go(entry.route)}
          >
            {#snippet icon()}<entry.icon size={16} />{/snippet}
          </SidebarItem>
        {/each}
      </div>

      <!-- collapsible chat list -->
      <div class="flex-1 min-h-0 flex flex-col mt-2 border-t border-(--color-surface-200)">
        <button
          type="button"
          onclick={() => (chatsCollapsed = !chatsCollapsed)}
          class="flex items-center gap-1 px-3 h-8 shrink-0 text-xs font-medium uppercase
                 tracking-wide text-(--color-text-secondary)
                 hover:text-(--color-text-primary) focus:outline-none"
        >
          {#if chatsCollapsed}<ChevronRight size={13} />{:else}<ChevronDown size={13} />{/if}
          {i18n.t('Sidebar.recentChats')}
        </button>

        {#if !chatsCollapsed}
          <div class="flex-1 min-h-0 overflow-y-auto px-2 pb-2 flex flex-col gap-0.5">
            {#each chatStore.chats as c (c.id)}
              <div
                class="group flex items-center gap-1 rounded-(--radius-md)
                       {chatStore.activeId === c.id && router.current === 'assistant'
                         ? 'bg-(--color-active-bg)'
                         : 'hover:bg-(--color-hover-bg)'}"
              >
                <button
                  type="button"
                  onclick={() => openChat(c.id)}
                  class="flex-1 min-w-0 flex items-center gap-2 px-2.5 h-8 text-left text-sm
                         focus:outline-none
                         {chatStore.activeId === c.id && router.current === 'assistant'
                           ? 'text-(--color-brand-700)'
                           : 'text-(--color-text-secondary)'}"
                >
                  <MessageSquare size={13} class="shrink-0" />
                  <span class="truncate">{c.title || i18n.t('Assistant.untitledChat')}</span>
                </button>
                <IconButton
                  label={i18n.t('Common.delete')}
                  size="sm"
                  class="opacity-0 group-hover:opacity-100"
                  onclick={() => chatStore.remove(c.id)}
                >
                  <Trash2 size={13} />
                </IconButton>
              </div>
            {/each}
            {#if chatStore.chats.length === 0}
              <p class="text-xs text-(--color-text-disabled) px-2.5 py-2">
                {i18n.t('Sidebar.noChats')}
              </p>
            {/if}
          </div>
        {/if}
      </div>

      {#snippet footer()}
        <SidebarItem
          label={i18n.t('Common.settings')}
          active={router.current === 'settings'}
          onclick={() => router.go('settings')}
        >
          {#snippet icon()}<SettingsIcon size={16} />{/snippet}
        </SidebarItem>
      {/snippet}
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
