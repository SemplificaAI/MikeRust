<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Presentational boot screen. The boot sequence itself (port discovery,
  /healthz probe, /auth/status) lives in App.svelte; this just renders
  the connecting spinner or a connection-failed panel.
-->
<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import Button from '$lib/components/ui/Button.svelte'

  interface Props {
    error?: string | null
    onretry: () => void
  }

  let { error = null, onretry }: Props = $props()
</script>

<div class="min-h-full flex items-center justify-center p-8 bg-(--color-surface-50)">
  <div class="w-full max-w-sm">
    {#if error}
      <Card title="Cannot reach the backend">
        <div class="space-y-3">
          <p class="text-sm text-(--color-danger-500) font-mono whitespace-pre-wrap">
            {error}
          </p>
          <p class="text-xs text-(--color-text-secondary)">
            Make sure the MikeRust backend is running. In dev, launch it from
            the repo root with
            <code class="font-mono">cargo tauri dev --config src-tauri/tauri.svelte.conf.json</code>.
          </p>
          <Button size="sm" variant="secondary" onclick={onretry}>Retry</Button>
        </div>
      </Card>
    {:else}
      <div class="flex flex-col items-center gap-4 text-(--color-text-secondary)">
        <div class="text-(--color-brand-500)">
          <Spinner size="lg" />
        </div>
        <p class="text-sm">Connecting to MikeRust…</p>
      </div>
    {/if}
  </div>
</div>
