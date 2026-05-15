<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte'
  import UnlockForm from '$lib/components/auth/UnlockForm.svelte'
  import { router } from '$lib/stores/router.svelte'
  import { userStore } from '$lib/stores/user.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import type { SessionUser } from '$lib/types/auth'

  interface Props {
    /** Known username from /auth/status, for a personalised greeting. */
    username?: string
  }

  let { username }: Props = $props()

  async function onUnlocked(user: SessionUser) {
    toastStore.success(`Welcome back, ${user.display_name ?? user.username}`)
    try {
      await userStore.hydrate()
    } catch {
      // non-fatal
    }
    router.go('workflows')
  }
</script>

<div class="min-h-full flex items-center justify-center p-8 bg-(--color-surface-50)">
  <div class="w-full max-w-sm space-y-6">
    <header class="text-center space-y-1">
      <h1 class="text-2xl font-semibold text-(--color-text-primary)">
        {username ? `Unlock, ${username}` : 'Unlock MikeRust'}
      </h1>
      <p class="text-sm text-(--color-text-secondary)">
        Enter your PIN to continue.
      </p>
    </header>

    <Card>
      <UnlockForm onsuccess={onUnlocked} />
    </Card>
  </div>
</div>
