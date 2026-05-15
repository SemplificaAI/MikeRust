<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import Input from '$lib/components/ui/Input.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import { authStore } from '$lib/stores/auth.svelte'
  import { isValidPinFormat, PIN_MIN_LENGTH, PIN_MAX_LENGTH } from '$lib/types/auth'
  import { ApiError } from '$lib/types/error'
  import type { SessionUser } from '$lib/types/auth'

  interface Props {
    onsuccess: (user: SessionUser) => void
  }

  let { onsuccess }: Props = $props()

  let username = $state('')
  let displayName = $state('')
  let pin = $state('')
  let pinConfirm = $state('')
  let submitting = $state(false)
  let formError = $state<string | null>(null)

  const pinError = $derived.by(() => {
    if (pin.length === 0) return undefined
    if (!isValidPinFormat(pin)) return `PIN must be ${PIN_MIN_LENGTH}–${PIN_MAX_LENGTH} digits`
    return undefined
  })

  const pinConfirmError = $derived.by(() => {
    if (pinConfirm.length === 0) return undefined
    if (pinConfirm !== pin) return 'PINs do not match'
    return undefined
  })

  const canSubmit = $derived(
    username.trim().length > 0 &&
      isValidPinFormat(pin) &&
      pin === pinConfirm &&
      !submitting
  )

  async function submit(e: SubmitEvent) {
    e.preventDefault()
    if (!canSubmit) return
    submitting = true
    formError = null
    try {
      const user = await authStore.setup({
        username: username.trim(),
        pin,
        display_name: displayName.trim() || undefined,
      })
      onsuccess(user)
    } catch (err) {
      formError =
        err instanceof ApiError ? err.detail : (err as Error).message
    } finally {
      submitting = false
    }
  }
</script>

<form class="space-y-4" onsubmit={submit}>
  <Input
    label="Username"
    bind:value={username}
    placeholder="How the app addresses you"
    autocomplete="username"
    required
  />

  <Input
    label="Display name (optional)"
    bind:value={displayName}
    placeholder="Shown in the greeting"
    autocomplete="name"
  />

  <Input
    label="PIN"
    type="password"
    bind:value={pin}
    placeholder="{PIN_MIN_LENGTH}–{PIN_MAX_LENGTH} digits"
    inputmode="numeric"
    maxlength={PIN_MAX_LENGTH}
    error={pinError}
    autocomplete="new-password"
  />

  <Input
    label="Confirm PIN"
    type="password"
    bind:value={pinConfirm}
    placeholder="Re-enter your PIN"
    inputmode="numeric"
    maxlength={PIN_MAX_LENGTH}
    error={pinConfirmError}
    autocomplete="new-password"
  />

  {#if formError}
    <p class="text-sm text-(--color-danger-500)">{formError}</p>
  {/if}

  <Button type="submit" full loading={submitting} disabled={!canSubmit}>
    Create profile
  </Button>
</form>
