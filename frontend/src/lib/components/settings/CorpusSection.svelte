<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  Generic corpus panel. Drives any registered corpus plugin — Italian
  Legal (dedicated /italian-legal/* routes) or a declarative plugin
  like CNIL (/corpora/{id}/* routes) — from its capability flags:
  bulk import with progress, search → index, and the indexed-document
  list. EUR-Lex keeps its own richer panel.
-->
<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import Badge from '$lib/components/ui/Badge.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import Progress from '$lib/components/ui/Progress.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import {
    italianLegalApi,
    genericCorpusApi,
    type CorpusItem,
    type CorpusDocument,
    type ImportStatus,
  } from '$lib/api/data-sources'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { openExternal } from '$lib/tauri/commands'
  import { Trash2, ExternalLink } from 'lucide-svelte'

  let { corpus }: { corpus: CorpusItem } = $props()

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)
  const suzieLawGithub = 'https://github.com/firelex/suzielaw'

  const sourceDisclaimers: Record<string, string> = {
    'at-ris': 'Disclaimer: contenuti da RIS/BKA Austria; verificare sempre la versione vigente ufficiale.',
    'de-openlegaldata': 'Disclaimer: contenuti da OpenLegalData; metadati e testi possono riflettere normalizzazioni del provider.',
    'de-gesetze': 'Disclaimer: contenuti da Gesetze im Internet; per uso professionale confermare su pubblicazione ufficiale.',
    'es-boe': 'Disclaimer: contenuti da BOE; fanno fede gli atti pubblicati nel Bolletín Oficial del Estado.',
    'ie-irishstatutebook': 'Disclaimer: contenuti da Irish Statute Book; usare citazioni complete (year/act|si/number).',
    'it-normattiva': 'Disclaimer: contenuti da Normattiva; verificare consolidato e testo vigente prima dell\'uso.',
    'jp-egov': 'Disclaimer: contenuti da e-Gov Japan; confermare la versione ufficiale in lingua originale.',
    'nl-wetten': 'Disclaimer: contenuti da Wetten.nl; riferimenti e versioni possono cambiare per consolidamenti.',
    'uk-findcaselaw': 'Disclaimer: contenuti da Find Case Law UK; validare citazioni e neutral citation nel registro ufficiale.',
    'uk-legislation': 'Disclaimer: contenuti da legislation.gov.uk; stato di revisione e in-force da verificare per ogni sezione.',
    'us-cfr': 'Disclaimer: contenuti da eCFR; l\'eCFR è aggiornato ma non sostituisce la pubblicazione legale ufficiale.',
    'us-courtlistener': 'Disclaimer: contenuti da CourtListener; verificare sempre il testo ufficiale della corte.',
    'au-federalregister': 'Disclaimer: contenuti da Federal Register of Legislation AU; verificare eventuali versioni consolidate.',
  }

  /** Italian Legal has bespoke routes; everything else is a plugin. */
  const isItalian = $derived(corpus.id === 'italian-legal')
  const corpusDisclaimer = $derived(
    sourceDisclaimers[corpus.id] ??
      `Disclaimer: contenuti da ${corpus.display_name}; verificare sempre la versione ufficiale della fonte.`
  )
  const indexedEmptyTitle = $derived(
    `Nessun documento ${corpus.display_name} ancora indicizzato. Cerca un atto qui sopra per aggiungerlo.`
  )

  interface Hit {
    id: string
    title: string
    sub: string
    date?: string
  }

  let query = $state('')
  let hits = $state<Hit[]>([])
  let searching = $state(false)
  let indexingId = $state<string | null>(null)
  let indexingController = $state<AbortController | null>(null)

  let docs = $state<CorpusDocument[]>([])
  let docsLoading = $state(true)

  let importStatus = $state<ImportStatus | null>(null)
  let importing = $state(false)
  let pollTimer: ReturnType<typeof setInterval> | undefined

  async function loadDocs() {
    if (!corpus.runnable) {
      docs = []
      docsLoading = false
      return
    }
    docsLoading = true
    try {
      const r = isItalian
        ? await italianLegalApi.documents()
        : await genericCorpusApi(corpus.id).documents()
      docs = r.documents
    } catch {
      docs = []
    } finally {
      docsLoading = false
    }
  }

  async function loadImportStatus() {
    if (!corpus.runnable) {
      importStatus = null
      return
    }
    try {
      importStatus = isItalian
        ? await italianLegalApi.importStatus()
        : await genericCorpusApi(corpus.id).importStatus()
    } catch {
      importStatus = null
    }
  }

  $effect(() => {
    void corpus.id
    void loadDocs()
    if (corpus.capabilities.bulk_import) void loadImportStatus()
    return () => clearInterval(pollTimer)
  })

  function startPolling() {
    clearInterval(pollTimer)
    pollTimer = setInterval(async () => {
      await loadImportStatus()
      if (importStatus?.job_state !== 'running') {
        clearInterval(pollTimer)
        importing = false
        void loadDocs()
      }
    }, 2000)
  }

  async function runImport() {
    if (!corpus.runnable) return
    importing = true
    try {
      if (isItalian) await italianLegalApi.startImport()
      else await genericCorpusApi(corpus.id).startImport()
      importStatus = { job_state: 'running' }
      startPolling()
    } catch (e) {
      importing = false
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    }
  }

  async function search() {
    if (!corpus.runnable) return
    if (!query.trim()) return
    searching = true
    hits = []
    try {
      if (isItalian) {
        const r = await italianLegalApi.search(query.trim())
        hits = r.hits.map((h) => ({
          id: h.hf_id,
          title: h.title ?? h.hf_id,
          date: h.date ?? undefined,
          sub: [h.authority, h.number, h.year].filter(Boolean).join(' · '),
        }))
      } else {
        const r = await genericCorpusApi(corpus.id).search(query.trim())
        hits = r.hits.map((h) => ({
          id: h.identifier,
          title: h.title,
          date: h.date ?? undefined,
          sub: [h.identifier, h.date].filter(Boolean).join(' · '),
        }))
      }
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    } finally {
      searching = false
    }
  }

  function findIndexedDoc(hit: Hit): CorpusDocument | undefined {
    return docs.find((d) => d.corpus_identifier === hit.id)
  }

  function indexButtonLabel(hit: Hit): string {
    if (indexingId === hit.id) return 'Stop'
    return findIndexedDoc(hit) ? 'Re-indicizza' : t('Corpora.indexHit')
  }

  function indexButtonVariant(hit: Hit): 'secondary' | 'danger' {
    return indexingId === hit.id ? 'danger' : 'secondary'
  }

  function indexedDocDate(doc: CorpusDocument): string | null {
    if (doc.corpus_date) return doc.corpus_date
    const fromHit = hits.find((h) => h.id === doc.corpus_identifier)?.date
    if (fromHit) return fromHit
    return null
  }

  async function indexHit(hit: Hit) {
    if (!corpus.runnable) return
    if (indexingId === hit.id) {
      indexingController?.abort()
      indexingController = null
      indexingId = null
      return
    }

    const controller = new AbortController()
    indexingController = controller
    indexingId = hit.id
    try {
      const indexedDoc = findIndexedDoc(hit)
      if (indexedDoc && corpus.capabilities.documents_resync) {
        if (isItalian) await italianLegalApi.resyncDocument(indexedDoc.id, { signal: controller.signal })
        else await genericCorpusApi(corpus.id).resyncDocument(indexedDoc.id, { signal: controller.signal })
      } else {
        if (isItalian) await italianLegalApi.fetchRow(hit.id, { signal: controller.signal })
        else await genericCorpusApi(corpus.id).fetch(hit.id, { signal: controller.signal, date: hit.date })
      }
      toastStore.success(t('Corpora.statusReady'))
      await loadDocs()
    } catch (e) {
      if ((e as Error).name === 'AbortError') {
        toastStore.info('Indicizzazione interrotta')
        return
      }
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    } finally {
      indexingController = null
      indexingId = null
    }
  }

  async function removeDoc(doc: CorpusDocument) {
    if (!corpus.runnable) return
    try {
      if (isItalian) await italianLegalApi.deleteDocument(doc.id)
      else await genericCorpusApi(corpus.id).deleteDocument(doc.id)
      docs = docs.filter((d) => d.id !== doc.id)
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    }
  }
</script>

<div class="space-y-4">
  <Card title={corpus.display_name} subtitle={corpus.description}>
    <div class="space-y-1.5">
      {#if corpus.homepage}
        <button
          type="button"
          class="flex items-center gap-1.5 text-xs text-(--color-brand-600) hover:underline"
          onclick={() => openExternal(corpus.homepage)}
        >
          <ExternalLink size={12} />{corpus.homepage}
        </button>
      {/if}
      <p class="text-xs text-(--color-text-secondary)">{corpusDisclaimer}</p>
      <button
        type="button"
        class="flex items-center gap-1.5 text-xs text-(--color-brand-600) hover:underline"
        onclick={() => openExternal(suzieLawGithub)}
      >
        <ExternalLink size={12} />Versione originale della fonte: thanks to SuzieLaw ({suzieLawGithub})
      </button>
    </div>
  </Card>

  {#if !corpus.runnable}
    <Card title="Fonte pianificata">
      <p class="text-sm text-(--color-text-secondary)">
        Questa fonte è visibile in catalogo ma non è ancora attiva nel runtime locale.
      </p>

      {#if corpus.sources?.length}
        <div class="mt-3 space-y-2">
          {#each corpus.sources as source (source.id)}
            <div class="rounded-(--radius-md) border border-(--color-surface-200) px-3 py-2">
              <div class="flex items-center justify-between gap-2">
                <p class="text-sm text-(--color-text-primary)">{source.display_name}</p>
                <Badge tone={source.available ? 'success' : 'neutral'} size="xs">
                  {source.available ? 'available' : (source.status_label ?? 'planned')}
                </Badge>
              </div>
              {#if source.subtitle}
                <p class="text-xs text-(--color-text-secondary)">{source.subtitle}</p>
              {/if}
              {#if source.description}
                <p class="mt-1 text-xs text-(--color-text-secondary)">{source.description}</p>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </Card>
  {/if}

  {#if corpus.runnable && corpus.capabilities.bulk_import}
    <Card title={t('Corpora.bulk.snapshotImport')}>
      <div class="space-y-2">
        {#if importStatus && importStatus.job_state === 'running'}
          <Progress value={importStatus.percent != null ? importStatus.percent / 100 : null} />
          <p class="text-xs text-(--color-text-secondary)">
            {t('Corpora.bulk.importing')}
            {#if importStatus.current_shard != null && importStatus.total_shards}
              · {importStatus.current_shard}/{importStatus.total_shards}
            {/if}
            {#if importStatus.rows_imported != null}· {importStatus.rows_imported}{/if}
          </p>
        {:else}
          <div class="flex items-center justify-between gap-3">
            <p class="text-xs text-(--color-text-secondary)">
              {#if importStatus?.row_count}
                {importStatus.row_count} · {importStatus.last_import_at ?? ''}
              {:else}
                {t('Corpora.bulk.snapshotFresh')}
              {/if}
            </p>
            <Button size="sm" loading={importing} onclick={runImport}>
              {importStatus?.row_count
                ? t('Corpora.bulk.updateAction')
                : t('Corpora.bulk.importAction')}
            </Button>
          </div>
          {#if importStatus?.job_error}
            <p class="text-xs text-(--color-danger-500)">{importStatus.job_error}</p>
          {/if}
        {/if}
      </div>
    </Card>
  {/if}

  {#if corpus.runnable && corpus.capabilities.search}
    <Card title={t('Corpora.searchButton')}>
      <div class="space-y-3">
        <div class="flex items-end gap-2">
          <Input
            bind:value={query}
            placeholder={t('Corpora.exampleHint', { example: corpus.identifier_example })}
            class="flex-1"
            onkeydown={(e: KeyboardEvent) => {
              if (e.key === 'Enter') search()
            }}
          />
          <Button loading={searching} disabled={!query.trim()} onclick={search}>
            {t('Corpora.searchButton')}
          </Button>
        </div>
        {#if hits.length}
          <ul class="flex flex-col gap-2">
            {#each hits as hit (hit.id)}
              <li class="flex items-center gap-3 px-3 py-2 border border-(--color-surface-200) rounded-(--radius-md)">
                <div class="flex-1 min-w-0">
                  <p class="text-sm text-(--color-text-primary) truncate">{hit.title}</p>
                  <p class="text-xs text-(--color-text-secondary) font-mono truncate">{hit.sub}</p>
                  {#if indexingId === hit.id}
                    <div class="mt-2">
                      <Progress value={null} size="sm" />
                    </div>
                  {/if}
                </div>
                <Button
                  size="sm"
                  variant={indexButtonVariant(hit)}
                  onclick={() => indexHit(hit)}
                >
                  {indexButtonLabel(hit)}
                </Button>
              </li>
            {/each}
          </ul>
        {:else if !searching && query}
          <p class="text-sm text-(--color-text-secondary)">
            {t('Corpora.noResultsFor', { query })}
          </p>
        {/if}
      </div>
    </Card>
  {/if}

  {#if corpus.runnable}
    <Card title={t('Corpora.indexedHeader', { count: docs.length })}>
      {#if docsLoading}
        <div class="flex justify-center py-6"><Spinner size="sm" /></div>
      {:else if docs.length === 0}
        <EmptyState title={indexedEmptyTitle} />
      {:else}
        <ul class="flex flex-col gap-2">
          {#each docs as doc (doc.id)}
            <li class="flex items-center gap-3 px-3 py-2 border border-(--color-surface-200) rounded-(--radius-md)">
              <div class="flex-1 min-w-0">
                <p class="text-sm text-(--color-text-primary) truncate">{doc.filename}</p>
                {#if doc.corpus_identifier || indexedDocDate(doc)}
                  <p class="text-xs text-(--color-text-secondary) font-mono">
                    {#if doc.corpus_identifier}{doc.corpus_identifier}{/if}
                    {#if doc.corpus_identifier && indexedDocDate(doc)} · {/if}
                    {#if indexedDocDate(doc)}{indexedDocDate(doc)}{/if}
                  </p>
                {/if}
              </div>
              <Badge tone={doc.status === 'ready' ? 'success' : 'neutral'} size="xs">{doc.status}</Badge>
              <IconButton label={t('Corpora.removeDoc')} size="sm" variant="danger"
                onclick={() => removeDoc(doc)}>
                <Trash2 size={14} />
              </IconButton>
            </li>
          {/each}
        </ul>
      {/if}
    </Card>
  {/if}
</div>
