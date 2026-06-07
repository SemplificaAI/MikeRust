<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Settings → LLM models. Catalogue-driven (GET /models) editor over the
  user's LlmSettings. Four configurable providers (Anthropic, Google,
  OpenAI, Local); model-role assignments (main / title / tabular);
  active-provider selection. Saves the whole form as a patch — the
  backend PUT has COALESCE / empty-string-clears semantics so unchanged
  fields are preserved and blanked fields are cleared.
-->
<script lang="ts" module>
  import type { LlmProvider, LlmSettings } from '$lib/types/user'

  // View-model: every text field is a non-null string so it binds
  // cleanly to Input/Select (whose value types exclude null).
  interface LlmForm {
    main_model: string
    title_model: string
    tabular_model: string
    claude_api_key: string
    gemini_api_key: string
    gemini_region: string
    gemini_model: string
    openai_api_key: string
    openai_model: string
    mistral_api_key: string
    mistral_model: string
    local_base_url: string
    local_api_key: string
    local_model: string
    active_provider: LlmProvider | null
  }

  const s = (v: string | null | undefined): string => v ?? ''

  function toForm(x: LlmSettings): LlmForm {
    return {
      main_model: s(x.main_model),
      title_model: s(x.title_model),
      tabular_model: s(x.tabular_model),
      claude_api_key: s(x.claude_api_key),
      gemini_api_key: s(x.gemini_api_key),
      gemini_region: s(x.gemini_region),
      gemini_model: s(x.gemini_model),
      openai_api_key: s(x.openai_api_key),
      openai_model: s(x.openai_model),
      mistral_api_key: s(x.mistral_api_key),
      mistral_model: s(x.mistral_model),
      local_base_url: s(x.local_base_url),
      local_api_key: s(x.local_api_key),
      local_model: s(x.local_model),
      active_provider: x.active_provider ?? null,
    }
  }
</script>

<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import Badge from '$lib/components/ui/Badge.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import ChipGroup from '$lib/components/ui/ChipGroup.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import { modelsStore } from '$lib/stores/models.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { api } from '$lib/api/client'

  let form = $state<LlmForm>(toForm({}))
  let initialized = $state(false)
  let activeProvidersUi = $state<string[]>([])
  let localRuntimeModels = $state<string[]>([])
  let localModelsLoaded = $state(false)
  let localModelsLoading = $state(false)

  let localFetchSeq = 0
  const ACTIVE_PROVIDERS_STORAGE_KEY = 'mikerust.settings.activeProviders.v1'

  function isProviderId(v: string): v is LlmProvider {
    return ['anthropic', 'google', 'openai', 'mistral', 'local'].includes(v)
  }

  function readPersistedActiveProviders(): string[] {
    if (typeof window === 'undefined') return []
    try {
      const raw = window.localStorage.getItem(ACTIVE_PROVIDERS_STORAGE_KEY)
      if (!raw) return []
      const parsed = JSON.parse(raw)
      if (!Array.isArray(parsed)) return []
      return parsed.filter((x): x is string => typeof x === 'string' && isProviderId(x))
    } catch {
      return []
    }
  }

  function writePersistedActiveProviders(v: string[]) {
    if (typeof window === 'undefined') return
    try {
      window.localStorage.setItem(ACTIVE_PROVIDERS_STORAGE_KEY, JSON.stringify(v))
    } catch {
      // no-op (private mode / quota)
    }
  }

  function setActiveProvidersUi(v: string[]) {
    const clean = Array.from(new Set(v.filter((x): x is string => typeof x === 'string' && isProviderId(x))))
    activeProvidersUi = clean
    writePersistedActiveProviders(clean)
  }

  function normalizeBaseUrl(url: string): string {
    return url.trim().replace(/\/+$/, '')
  }

  function configuredProvidersFromForm(v: LlmForm): string[] {
    const out: string[] = []
    if (keySet(v.claude_api_key)) out.push('anthropic')
    if (keySet(v.gemini_api_key)) out.push('google')
    if (keySet(v.openai_api_key)) out.push('openai')
    if (keySet(v.mistral_api_key)) out.push('mistral')
    if (keySet(v.local_base_url)) out.push('local')
    return out
  }

  async function refreshLocalRuntimeModels() {
    const base = normalizeBaseUrl(form.local_base_url)
    const seq = ++localFetchSeq

    if (!base) {
      localRuntimeModels = []
      localModelsLoaded = false
      localModelsLoading = false
      return
    }

    localModelsLoading = true

    try {
      // Route the model-list probe through MikeRust's own backend
      // instead of `fetch` against the Ollama / llama-server URL
      // directly. The WebView origin is `http://tauri.localhost`, and
      // external OpenAI-compatible runtimes rarely advertise that
      // origin in their `Access-Control-Allow-Origin` header, so the
      // browser used to block the probe with the CORS message users
      // reported. The backend proxy at `/models/local/probe` does the
      // server-to-server fetch (no Origin involved) and returns the
      // same payload shape Ollama / lm-studio / vLLM produce.
      const payload = await api<{ data?: Array<{ id?: string }> }>(
        '/models/local/probe',
        {
          query: {
            base,
            api_key: form.local_api_key.trim() || undefined,
          },
        },
      )
      const ids = (payload.data ?? [])
        .map((x) => (x.id ?? '').trim())
        .filter((x) => x.length > 0)

      if (seq !== localFetchSeq) return
      localRuntimeModels = Array.from(new Set(ids))
      localModelsLoaded = true
      localModelsLoading = false

      const first = ids[0] ?? ''
      if (first) {
        const currentMain = form.main_model.trim()
        const hasValidLocalMain =
          currentMain.startsWith('local:') && ids.includes(currentMain.slice('local:'.length))
        if (!currentMain || !hasValidLocalMain) {
          form.main_model = `local:${first}`
        }
        if (!form.local_model.trim()) {
          form.local_model = first
        }
      }
    } catch {
      if (seq !== localFetchSeq) return
      localRuntimeModels = []
      localModelsLoaded = false
      localModelsLoading = false
    }
  }

  $effect(() => {
    void modelsStore.load()
  })

  $effect(() => {
    if (!initialized && !modelsStore.loading && modelsStore.catalogue) {
      form = toForm(modelsStore.settings)
      const configured = configuredProvidersFromForm(form)
      const persisted = readPersistedActiveProviders().filter((p) => configured.includes(p))
      setActiveProvidersUi(
        persisted.length > 0
          ? persisted
          : form.active_provider
            ? [form.active_provider]
            : configured
      )
      initialized = true
      if (keySet(form.local_base_url)) {
        void refreshLocalRuntimeModels()
      }
    }
  })

  const dirty = $derived.by(() => {
    if (!initialized) return false
    const formDirty = JSON.stringify(form) !== JSON.stringify(toForm(modelsStore.settings))
    const currentActive = modelsStore.settings.active_provider ?? null
    const nextActive = pickPersistedActiveProvider()
    return formDirty || currentActive !== nextActive
  })

  function modelOptions(providerId: string) {
    const p = modelsStore.providerById(providerId)
    return [
      { value: '', label: i18n.t('Settings.selectModel') },
      ...(p?.models ?? []).map((m) => ({
        value: m.id,
        label: m.preview ? `${m.display_name} (preview)` : m.display_name,
      })),
    ]
  }
  const regionOptions = $derived(
    (modelsStore.providerById('google')?.regions ?? []).map((r) => ({
      value: r.id,
      label: r.display_name,
    }))
  )
  const keySet = (v: string): boolean => v.trim().length > 0

  // Providers the user has actually configured.
  // Drives which models the role dropdowns may offer.
  const configuredProviders = $derived.by(() => {
    const s = new Set<string>()
    if (keySet(form.claude_api_key)) s.add('anthropic')
    if (keySet(form.gemini_api_key)) s.add('google')
    if (keySet(form.openai_api_key)) s.add('openai')
    if (keySet(form.mistral_api_key)) s.add('mistral')
    if (keySet(form.local_base_url)) s.add('local')
    return s
  })

  const activeRoleProviders = $derived.by(() => {
    const cfg = configuredProviders
    return activeProvidersUi.filter((p) => cfg.has(p))
  })

  const localRoleIds = $derived.by(() => {
    if (localModelsLoaded) return new Set(localRuntimeModels)
    return new Set<string>()
  })

  // Role-model options — only models from configured providers. Ids
  // carry the dispatch prefix the backend expects: openai:/mistral: for
  // those providers, bare id for Claude/Gemini.
  const roleOptions = $derived.by(() => {
    const out = [{ value: '', label: i18n.t('Settings.notSet') }]

    const nonLocal = modelsStore.allModels
      .filter((m) => m.providerId !== 'local')
      .filter((m) => activeRoleProviders.includes(m.providerId))
      .map((m) => ({
        value: m.providerId === 'openai' ? `openai:${m.id}` : m.providerId === 'mistral' ? `mistral:${m.id}` : m.id,
        label: `${m.display_name} · ${m.provider}`,
      }))

    const local = activeRoleProviders.includes('local')
      ? Array.from(localRoleIds).map((id) => ({
          value: `local:${id}`,
          label: `${id} · ${i18n.t('Settings.providerLocal')}`,
        }))
      : []

    return [...out, ...nonLocal, ...local]
  })

  const providerChips = $derived([
    { value: 'anthropic', label: 'Anthropic', disabled: !configuredProviders.has('anthropic') },
    { value: 'google', label: 'Google', disabled: !configuredProviders.has('google') },
    { value: 'openai', label: 'OpenAI', disabled: !configuredProviders.has('openai') },
    { value: 'mistral', label: 'Mistral', disabled: !configuredProviders.has('mistral') },
    { value: 'local', label: i18n.t('Settings.providerLocal'), disabled: !configuredProviders.has('local') },
  ])

  function normalizeRoleModelValue(value: string): string {
    const v = value.trim()
    if (!v) return ''
    if (v.startsWith('openai:') || v.startsWith('mistral:') || v.startsWith('local:')) return v
    if (localRoleIds.has(v)) return `local:${v}`
    return v
  }

  function pickPersistedActiveProvider(): LlmProvider | null {
    for (const p of ['anthropic', 'google', 'openai', 'mistral', 'local'] as const) {
      if (activeRoleProviders.includes(p)) return p
    }
    return null
  }

  async function save() {
    try {
      const main = normalizeRoleModelValue(form.main_model)
      const title = normalizeRoleModelValue(form.title_model)
      const tabular = normalizeRoleModelValue(form.tabular_model)
      const mainLocal = main.startsWith('local:') ? main.slice('local:'.length) : ''
      await modelsStore.save({
        ...form,
        main_model: main,
        title_model: title,
        tabular_model: tabular,
        local_model: mainLocal || form.local_model,
        active_provider: pickPersistedActiveProvider(),
      })
      writePersistedActiveProviders(activeProvidersUi)
      toastStore.success(i18n.t('Settings.llmSettingsSaved'))
    } catch (e) {
      toastStore.danger(i18n.t('Settings.llmSettingsError'), { detail: (e as Error).message })
    }
  }
</script>

{#if modelsStore.loading && !initialized}
  <div class="flex items-center gap-2 text-sm text-(--color-text-secondary) py-12 justify-center">
    <Spinner size="sm" />
    {i18n.t('Settings.loadingCatalogue')}
  </div>
{:else if modelsStore.error && !initialized}
  <EmptyState title={i18n.t('Settings.loadModelsError')} description={modelsStore.error}>
    {#snippet action()}
      <Button size="sm" variant="secondary" onclick={() => modelsStore.load()}>
        {i18n.t('Common.retry')}
      </Button>
    {/snippet}
  </EmptyState>
{:else}
  <div class="space-y-4">
    <Card title={i18n.t('Settings.activeProvider')} subtitle={i18n.t('Settings.activeProviderHint')}>
      <ChipGroup
        chips={providerChips}
        multi
        selected={activeProvidersUi}
        onchange={(v) => setActiveProvidersUi((Array.isArray(v) ? v : []) as string[])}
      />
    </Card>

    <Card>
      {#snippet header()}
        <div class="flex items-center gap-2">
          <h3 class="text-sm font-semibold text-(--color-text-primary)">Anthropic (Claude)</h3>
          {#if keySet(form.claude_api_key)}<Badge tone="success" size="xs">{i18n.t('Settings.keySet')}</Badge>{/if}
        </div>
      {/snippet}
      <Input
        label={i18n.t('Settings.apiKey')}
        type="password"
        bind:value={form.claude_api_key}
        placeholder="sk-ant-…"
        autocomplete="off"
      />
    </Card>

    <Card>
      {#snippet header()}
        <div class="flex items-center gap-2">
          <h3 class="text-sm font-semibold text-(--color-text-primary)">Google Gemini</h3>
          {#if keySet(form.gemini_api_key)}<Badge tone="success" size="xs">{i18n.t('Settings.keySet')}</Badge>{/if}
        </div>
      {/snippet}
      <div class="space-y-3">
        <Input
          label="API key"
          type="password"
          bind:value={form.gemini_api_key}
          placeholder="AIza…"
          autocomplete="off"
        />
        <div class="grid grid-cols-2 gap-3">
          <Select label={i18n.t('Settings.model')} options={modelOptions('google')} bind:value={form.gemini_model} />
          <Select label={i18n.t('Settings.region')} options={regionOptions} bind:value={form.gemini_region} />
        </div>
      </div>
    </Card>

    <Card>
      {#snippet header()}
        <div class="flex items-center gap-2">
          <h3 class="text-sm font-semibold text-(--color-text-primary)">OpenAI</h3>
          {#if keySet(form.openai_api_key)}<Badge tone="success" size="xs">{i18n.t('Settings.keySet')}</Badge>{/if}
        </div>
      {/snippet}
      <div class="space-y-3">
        <Input
          label="API key"
          type="password"
          bind:value={form.openai_api_key}
          placeholder="sk-…"
          autocomplete="off"
        />
        <Select label={i18n.t('Settings.model')} options={modelOptions('openai')} bind:value={form.openai_model} />
      </div>
    </Card>

    <Card>
      {#snippet header()}
        <div class="flex items-center gap-2">
          <h3 class="text-sm font-semibold text-(--color-text-primary)">Mistral AI</h3>
          {#if keySet(form.mistral_api_key)}<Badge tone="success" size="xs">{i18n.t('Settings.keySet')}</Badge>{/if}
        </div>
      {/snippet}
      <div class="space-y-3">
        <Input
          label="API key"
          type="password"
          bind:value={form.mistral_api_key}
          autocomplete="off"
        />
        <Select label={i18n.t('Settings.model')} options={modelOptions('mistral')} bind:value={form.mistral_model} />
      </div>
    </Card>

    <Card title={i18n.t('Settings.localProvider')}>
      <div class="space-y-3">
        <div class="grid grid-cols-[1fr_auto] gap-2 items-end">
          <Input
            label={i18n.t('Settings.baseUrl')}
            bind:value={form.local_base_url}
            placeholder="http://127.0.0.1:11434/v1"
            autocomplete="off"
          />
          <Button
            size="sm"
            variant="secondary"
            disabled={!keySet(form.local_base_url)}
            loading={localModelsLoading}
            onclick={() => void refreshLocalRuntimeModels()}
          >
            Refresh
          </Button>
        </div>
        <Input
          label={i18n.t('Settings.apiKeyOptional')}
          type="password"
          bind:value={form.local_api_key}
          autocomplete="off"
        />
      </div>
    </Card>

    <Card title={i18n.t('Settings.modelRoles')} subtitle={i18n.t('Settings.modelRolesHint')}>
      <div class="grid grid-cols-3 gap-3">
        <Select label={i18n.t('Settings.roleMain')} options={roleOptions} bind:value={form.main_model} />
        <Select label={i18n.t('Settings.roleTitles')} options={roleOptions} bind:value={form.title_model} />
        <Select label={i18n.t('Settings.roleTabular')} options={roleOptions} bind:value={form.tabular_model} />
      </div>
    </Card>

    <div class="flex justify-end">
      <Button disabled={!dirty} loading={modelsStore.saving} onclick={save}>
        {i18n.t('Settings.saveChanges')}
      </Button>
    </div>
  </div>
{/if}
