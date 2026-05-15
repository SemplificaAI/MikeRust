<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte'
  import SetupForm from '$lib/components/auth/SetupForm.svelte'
  import { router } from '$lib/stores/router.svelte'
  import { userStore } from '$lib/stores/user.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import type { SessionUser } from '$lib/types/auth'

  async function onSetupDone(user: SessionUser) {
    toastStore.success(`Welcome, ${user.display_name ?? user.username}`)
    // Setup returns a live token — hydrate preferences then enter the app.
    try {
      await userStore.hydrate()
    } catch {
      // non-fatal: defaults are fine, Settings can fix later
    }
    router.go('home')
  }
</script>

<div class="min-h-full flex items-center justify-center p-8 bg-(--color-surface-50)">
  <div class="w-full max-w-sm space-y-6">
    <header class="text-center space-y-1">
      <h1 class="text-2xl font-semibold text-(--color-text-primary)">Welcome to MikeRust</h1>
      <p class="text-sm text-(--color-text-secondary)">
        Create your local profile. Everything stays on this machine.
      </p>
    </header>

    <Card>
      <SetupForm onsuccess={onSetupDone} />
    </Card>

    <p class="text-xs text-(--color-text-secondary) text-center">
      Your PIN protects local access. There is no password recovery —
      keep it somewhere safe.
    </p>
  </div>
</div>
