<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Full-screen overlay shown while a biometric verification is in flight.
  The actual Windows Hello / Touch ID dialog is driven by the Tauri shell
  (backend → BiometricRequest channel); the frontend only waits for the
  POST /auth/unlock-biometric response and shows this while it pends.
-->
<script lang="ts">
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'

  interface Props {
    open: boolean
    reason?: string
  }

  let { open, reason }: Props = $props()
  const displayReason = $derived(reason ?? i18n.t('Settings.verifyingIdentity'))
</script>

{#if open}
  <div
    class="fixed inset-0 z-[70] flex items-center justify-center
           bg-black/50 backdrop-blur-sm"
    role="alertdialog"
    aria-label={i18n.t('Settings.biometricVerifyAria')}
    aria-busy="true"
  >
    <div
      class="flex flex-col items-center gap-4 px-8 py-7
             bg-(--color-surface-0) rounded-(--radius-lg) shadow-(--shadow-modal)"
    >
      <div class="text-(--color-brand-500)">
        <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M12 11c0 3.5-.6 6.3-1.5 8.5" />
          <path d="M5 8a7 7 0 0 1 11-1" />
          <path d="M4 12a8 8 0 0 1 .5-3" />
          <path d="M8.5 19.5C9.5 17 10 14.5 10 12a2 2 0 1 1 4 0c0 1.3-.1 2.5-.3 3.6" />
          <path d="M15.5 18.5c.4-1.4.5-2.9.5-4.5" />
          <path d="M2 16c.6-1 1-2.4 1-4a9 9 0 0 1 15-6.7" />
          <path d="M20 5.5A9 9 0 0 1 21 12c0 1-.1 2-.3 3" />
        </svg>
      </div>
      <p class="text-sm font-medium text-(--color-text-primary) text-center max-w-xs">
        {displayReason}
      </p>
      <div class="text-(--color-brand-500)">
        <Spinner size="sm" />
      </div>
      <p class="text-xs text-(--color-text-secondary) text-center">
        {i18n.t('Settings.followSystemPrompt')}
      </p>
    </div>
  </div>
{/if}
