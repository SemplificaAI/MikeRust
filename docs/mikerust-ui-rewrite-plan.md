# MikeRust — Piano di riscrittura UI clean-room

> ✅ **ESEGUITO — riscrittura completata il 2026-05-17** (vedi HISTORY.md
> «Clean-room frontend rewrite (React → Svelte 5)»). Il frontend
> Next.js/React legacy è stato rimosso dal repository; `frontend/` è oggi
> l'applicazione Svelte 5 clean-room descritta in questo piano. Il
> documento è conservato come **piano storico e razionale legale** della
> riscrittura: alcuni path citati riflettono la struttura
> *pre-esecuzione* (es. `frontendMike/messages/` → oggi
> `frontend/locales/`; `config/workflows/` → oggi
> `config/workflow-presets/`) e non vanno letti come layout attuale. Per
> lo stato corrente fai riferimento al codice in `frontend/` e al
> README §Frontend.

**Stack target:** Tauri 2 · Svelte 5 (runes) · TypeScript · Tailwind CSS v4 · Vite
**Obiettivo:** eliminare qualsiasi derivazione AGPL dal frontend, mantenendo backend Rust, schema DB e compatibilità workflow / template JSON / preset bundle MikeRust.

> Il documento è ground-truth rispetto al backend `c:\Progetti\MikeRust` (commit `1e809c4` o successivi). Tutti i contratti API, le rotte e il comportamento descritti sono **letti direttamente dal sorgente Rust**, non inferiti dal frontend AGPL esistente.

---

## 1. Analisi del problema legale

### Perché esiste il problema AGPL
Il frontend attuale (`frontend/`) deriva dal progetto **Mike**, distribuito sotto licenza AGPL-3.0.
L'AGPL impone che qualsiasi software che *incorpora o modifica* codice AGPL, distribuito o reso accessibile via rete, debba anch'esso essere rilasciato sotto AGPL con sorgenti pubblici.

In un'app Tauri il frontend viene **bundlato nell'eseguibile finale** (vedi [src-tauri/tauri.conf.json](src-tauri/tauri.conf.json) — `frontendDist: "../frontend/out"`): questo attiva il copyleft AGPL sull'intera applicazione, incluse le parti proprietarie o commerciali.

### Cosa NON è contaminato dall'AGPL
| Componente | Stato | Motivazione |
|---|---|---|
| Backend Rust (`src/`) | ✅ Pulito | Scritto ex-novo, nessun codice Mike |
| Shell Tauri (`src-tauri/`) | ✅ Pulito | 2 comandi Tauri (`api_base_url`, `open_external_url`) + integrazione biometrica Windows Hello |
| Schema SQLite (`migrations/`) | ✅ Pulito | Schema dati puro, non copyrightabile |
| Preset workflow JSON (`config/workflows/`) | ✅ Pulito | File di dati editoriali, non codice |
| Preset column JSON (`config/column-presets/`) | ✅ Pulito | File di dati editoriali, non codice |
| Template DOCX JSON (`config/docx-templates/`) | ✅ Pulito | Contenuto editoriale, non codice |
| Catalogo modelli LLM (`config/model.json`) | ✅ Pulito | Dati strutturati, non codice |
| Manifest corpora (`config/corpora/*.yaml`) | ✅ Pulito | Specifiche dichiarative |
| Pacchetti i18n string (`frontendMike/messages/*.json`) | ✅ Pulito (parte MikeRust) / ⚠️ misto | Le **chiavi e stringhe scritte ex-novo in MikeRust** (commit `0b575ca`, `f78c8bb`, `94ed69f`, `407c296`, `16c8308`, `3faa20d`) sono copyright del proprietario → riutilizzabili. Le **stringhe ereditate da Mike** (chiavi pre-fork) vanno rigenerate o riformulate. Vedi §14 per la procedura di bonifica selettiva. |

### Cosa è contaminato
| Componente | Problema |
|---|---|
| Tutto il codice React/Next.js in `frontend/src` | Deriva da Mike AGPL |
| Componenti UI (sidebar, modal, tabelle, picker) | Copia/adattamento di Mike |
| Store / state management (Zustand, contexts) | Logica derivata da Mike |
| Routing Next.js + App Router pages | Logica derivata da Mike |
| Codice TS/TSX che consuma i18n (provider, helper, hook) | Da riscrivere ex-novo (il sistema, non i contenuti) |

### Soluzione: clean-room rewrite
Riscrivere il frontend da zero in un nuovo linguaggio (Svelte 5 vs React) su un nuovo repository, **senza copiare una singola riga** dal frontend attuale. Il cambio di linguaggio (React → Svelte) e il cambio di paradigma (Next.js App Router → SPA Tauri pura) sono la prova più forte dell'assenza di derivazione.

L'unico contatto consentito con `frontend/` è la **lettura degli screenshot** del prodotto finito (look & feel) e dei **commit di MikeRust** (`git log -- frontend/`) per identificare **quali feature** sono state aggiunte rispetto a Mike, senza guardarne l'implementazione.

---

## 2. Scelte tecnologiche

### Stack scelto e motivazioni

| Tecnologia | Scelta | Alternativa scartata | Motivazione |
|---|---|---|---|
| Framework UI | **Svelte 5** (runes) | React, Vue, Solid | Nessuna contaminazione AGPL; bundle più piccolo; runes = reattività compile-time senza runtime overhead |
| Meta-framework | **Nessuno** (SPA pura) | SvelteKit | SvelteKit aggiunge routing file-based e SSR inutili in una desktop SPA Tauri |
| Build tool | **Vite 6** | Webpack, Rollup standalone | Già integrato in `pnpm create tauri-app`; HMR velocissimo |
| Styling | **Tailwind CSS v4** | v3, CSS Modules, UnoCSS, Panda | Zero config file (CSS-first); engine Oxide (Rust); compatibilità nativa Vite; utility-first |
| Routing | **router custom leggero** | TanStack Router, svelte-routing | SPA con ~7 "pagine" non richiede router pesante; ~40 righe di `$state` |
| State globale | **Svelte 5 runes** (`$state`, `$derived`, `$effect`) | Zustand, Pinia, Nano Stores | Svelte 5 ha state management built-in senza librerie esterne |
| HTTP client | **`fetch` nativo + wrapper tipizzato** | axios, ky | Disponibile nel WebView Tauri; nessuna dipendenza |
| IPC Tauri | **@tauri-apps/api v2** | — | API ufficiale, licenza MIT/Apache2 (solo 2 comandi: `api_base_url`, `open_external_url`) |
| Icone | **Lucide Svelte** | Heroicons, Phosphor | MIT, tree-shakeable |
| Tabelle | **TanStack Table v8 (headless)** | AG Grid, custom | MIT, headless = pieno controllo stile |
| Virtualizzazione liste | **svelte-virtual** | — | MIT, necessario per liste lunghe di documenti, file KB, chat history |
| Markdown rendering | **marked** + **DOMPurify** | markdown-it, react-markdown | Marked: MIT, leggero; DOMPurify: Apache-2.0 per sanitizzazione |
| PDF viewer | **pdfjs-dist** | iframe Chromium, react-pdf | Apache-2.0, controllo completo su toolbar + dark mode + ricerca |
| Code highlight | **shiki** | highlight.js, Prism | MIT, syntax-highlighter di VSCode |
| Streaming SSE | **EventSource nativo** | sse.js | Disponibile nel WebView; non serve fallback IE |
| File upload progress | **XHR per `progress` event** | fetch + streams | `fetch` non espone upload progress; XHR sì |
| i18n | **`@intlify/core-base`** o wrapper minimale custom | i18next, react-intl, next-intl | Tutti MIT; per uno store con 6 locale + fallback bastano ~80 righe TypeScript |
| Testing | **Vitest + Playwright** | Jest, Cypress | Integrazione nativa Vite, MIT/Apache-2.0 |
| Mocking IPC in test | **@tauri-apps/api/mocks** | — | Ufficiale, MIT |

### Licenze di tutte le dipendenze principali
| Pacchetto | Licenza |
|---|---|
| svelte | MIT ✅ |
| vite | MIT ✅ |
| tailwindcss v4 | MIT ✅ |
| @tauri-apps/api | MIT/Apache-2.0 ✅ |
| typescript | Apache-2.0 ✅ |
| lucide-svelte | ISC ✅ |
| @tanstack/table-core | MIT ✅ |
| svelte-virtual | MIT ✅ |
| marked | MIT ✅ |
| dompurify | Apache-2.0 ✅ |
| pdfjs-dist | Apache-2.0 ✅ |
| @tauri-apps/plugin-stronghold | MIT/Apache-2.0 ✅ |
| @tauri-apps/plugin-single-instance | MIT/Apache-2.0 ✅ |
| @tauri-apps/plugin-dialog | MIT/Apache-2.0 ✅ |
| @tauri-apps/plugin-window-state | MIT/Apache-2.0 ✅ |
| shiki | MIT ✅ |
| @intlify/core-base | MIT ✅ |
| vitest | MIT ✅ |
| playwright | Apache-2.0 ✅ |

> **Nessuna dipendenza GPL/AGPL/LGPL.** Prima di aggiungere qualsiasi nuova dipendenza, verificare la licenza su npmjs.com.
> Audit automatico: `pnpm dlx license-checker --onlyAllow 'MIT;ISC;Apache-2.0;BSD-2-Clause;BSD-3-Clause;0BSD;CC0-1.0;Unlicense'`

---

## 3. Struttura del nuovo repository

**Decisione finale (vedi §23 Q1):** il nuovo codice vive in **`MikeRust/frontend/`** (sostituisce il vecchio). Il frontend AGPL attuale viene **rinominato `MikeRust/frontendMike/`** per consentire uno **switch rapido** tra vecchio (di backup, ancora funzionante) e nuovo durante lo sviluppo. Lo switch avviene a livello Tauri tramite due file di configurazione paralleli — vedi §7.4 e §18.

```
MikeRust/
├── frontend/                           ← NUOVO codice clean-room (Svelte 5)
│   ├── src/                              … (struttura descritta sotto)
│   ├── dist/                             ← Vite build output (Tauri prod)
│   ├── package.json
│   └── …
├── frontendMike/                       ← VECCHIO codice AGPL (rinominato, sola lettura)
│   └── out/                              ← Next.js build output (Tauri prod legacy)
├── src-tauri/
│   ├── tauri.conf.json                 ← punta a frontend/ (nuovo, default)
│   ├── tauri.legacy.conf.json          ← punta a frontendMike/ (switch)
│   └── …
└── (resto del backend Rust)

frontend/ (alberatura interna)
├── src/
│   ├── lib/
│   │   ├── components/
│   │   │   ├── layout/
│   │   │   │   ├── Sidebar.svelte
│   │   │   │   ├── SidebarItem.svelte
│   │   │   │   ├── TopBar.svelte
│   │   │   │   ├── AppShell.svelte
│   │   │   │   ├── StatusBar.svelte           ← progress RAG / sync / model status / MCP chip
│   │   │   │   ├── McpActivityChip.svelte      ← spia MCP nella StatusBar (count + popover)
│   │   │   │   └── PdfViewer.svelte            ← lazy pdfjs-dist wrapper
│   │   │   ├── ui/                            ← design system primitivi
│   │   │   │   ├── Button.svelte
│   │   │   │   ├── IconButton.svelte
│   │   │   │   ├── Input.svelte
│   │   │   │   ├── Textarea.svelte
│   │   │   │   ├── Select.svelte
│   │   │   │   ├── Combobox.svelte
│   │   │   │   ├── Modal.svelte
│   │   │   │   ├── Sheet.svelte               ← side panel
│   │   │   │   ├── Tabs.svelte
│   │   │   │   ├── Badge.svelte
│   │   │   │   ├── Checkbox.svelte
│   │   │   │   ├── Radio.svelte
│   │   │   │   ├── Toggle.svelte
│   │   │   │   ├── Dropdown.svelte
│   │   │   │   ├── Menu.svelte
│   │   │   │   ├── ChipGroup.svelte
│   │   │   │   ├── CodeBlock.svelte
│   │   │   │   ├── Avatar.svelte
│   │   │   │   ├── Progress.svelte
│   │   │   │   ├── Spinner.svelte
│   │   │   │   ├── Tooltip.svelte
│   │   │   │   ├── Toast.svelte
│   │   │   │   ├── ToastRegion.svelte
│   │   │   │   ├── ConfirmDialog.svelte
│   │   │   │   ├── EmptyState.svelte
│   │   │   │   ├── ErrorBoundary.svelte
│   │   │   │   └── Pagination.svelte
│   │   │   ├── auth/
│   │   │   │   ├── SetupForm.svelte
│   │   │   │   ├── UnlockForm.svelte
│   │   │   │   ├── BiometricPrompt.svelte
│   │   │   │   └── ChangePinForm.svelte
│   │   │   ├── chat/
│   │   │   │   ├── ChatInput.svelte
│   │   │   │   ├── ChatMessage.svelte
│   │   │   │   ├── ChatHistory.svelte
│   │   │   │   ├── ChatToolbar.svelte
│   │   │   │   ├── McpActivityDot.svelte       ← spia MCP accanto al ModelSelector
│   │   │   │   ├── MessageStream.svelte       ← consumer SSE fetch+stream
│   │   │   │   ├── ToolCallCard.svelte        ← MCP tool call inline
│   │   │   │   ├── DownloadDocxCard.svelte    ← generato da render template
│   │   │   │   └── ModelSelector.svelte
│   │   │   ├── documents/
│   │   │   │   ├── DocumentPicker.svelte
│   │   │   │   ├── DocumentList.svelte
│   │   │   │   ├── DocumentUploadDropZone.svelte
│   │   │   │   ├── DocumentRow.svelte
│   │   │   │   └── DocumentViewer.svelte      ← <iframe> / pdfjs / immagine
│   │   │   ├── projects/
│   │   │   │   ├── ProjectPicker.svelte
│   │   │   │   ├── ProjectModal.svelte
│   │   │   │   ├── ProjectExportModal.svelte
│   │   │   │   └── ProjectImportModal.svelte
│   │   │   ├── workflow/
│   │   │   │   ├── WorkflowList.svelte
│   │   │   │   ├── WorkflowPicker.svelte
│   │   │   │   ├── WorkflowModal.svelte
│   │   │   │   ├── WorkflowColumnEditor.svelte
│   │   │   │   └── TemplatePicker.svelte
│   │   │   ├── tabular/
│   │   │   │   ├── TabularReviewList.svelte
│   │   │   │   ├── TabularReviewTable.svelte
│   │   │   │   ├── TabularReviewModal.svelte
│   │   │   │   ├── AddColumnModal.svelte
│   │   │   │   └── CellExpansion.svelte
│   │   │   ├── settings/
│   │   │   │   ├── SettingsNav.svelte
│   │   │   │   ├── ProfileSection.svelte
│   │   │   │   ├── DefaultDomainSelect.svelte
│   │   │   │   ├── LocaleSelect.svelte
│   │   │   │   ├── LLMProviderCard.svelte
│   │   │   │   ├── MCPServerCard.svelte
│   │   │   │   ├── MCPProbeResult.svelte
│   │   │   │   ├── LocalFolderCard.svelte
│   │   │   │   ├── SyncFolderRow.svelte
│   │   │   │   ├── ScanProgressBar.svelte
│   │   │   │   ├── EmbeddingModelBanner.svelte
│   │   │   │   ├── CorpusCard.svelte          ← uno per corpus plugin
│   │   │   │   ├── EurlexCard.svelte
│   │   │   │   ├── ItalianLegalCard.svelte
│   │   │   │   └── DeleteAccountSection.svelte
│   │   │   └── domain/
│   │   │       ├── DomainSelect.svelte         ← 9-value enum dropdown
│   │   │       └── DomainFilter.svelte         ← chip filter su liste
│   │   ├── stores/                             ← stato globale Svelte 5 runes
│   │   │   ├── router.svelte.ts
│   │   │   ├── auth.svelte.ts
│   │   │   ├── api-base.svelte.ts              ← URL backend (da invoke `api_base_url`)
│   │   │   ├── user.svelte.ts                  ← profilo + locale + default_domain
│   │   │   ├── chat.svelte.ts
│   │   │   ├── projects.svelte.ts
│   │   │   ├── documents.svelte.ts
│   │   │   ├── workflows.svelte.ts
│   │   │   ├── columnPresets.svelte.ts
│   │   │   ├── tabular.svelte.ts
│   │   │   ├── templates.svelte.ts
│   │   │   ├── models.svelte.ts                ← catalogue + active provider
│   │   │   ├── mcp.svelte.ts
│   │   │   ├── sync.svelte.ts
│   │   │   ├── corpora.svelte.ts
│   │   │   ├── eurlex.svelte.ts
│   │   │   ├── italianLegal.svelte.ts
│   │   │   ├── embedModel.svelte.ts            ← stato live ONNX/fastembed
│   │   │   ├── health.svelte.ts                ← polling /healthz
│   │   │   ├── i18n.svelte.ts                  ← locale + dict loader
│   │   │   ├── toast.svelte.ts
│   │   │   └── theme.svelte.ts                 ← light/dark/system
│   │   ├── api/                                ← wrapper HTTP tipizzati
│   │   │   ├── client.ts                       ← fetch base + auth header + error mapping
│   │   │   ├── auth.ts
│   │   │   ├── user.ts
│   │   │   ├── chat.ts                         ← include EventSource SSE
│   │   │   ├── projects.ts
│   │   │   ├── documents.ts                    ← multipart upload + download
│   │   │   ├── workflows.ts
│   │   │   ├── presets.ts
│   │   │   ├── tabular.ts
│   │   │   ├── templates.ts
│   │   │   ├── models.ts
│   │   │   ├── mcp.ts
│   │   │   ├── sync.ts
│   │   │   ├── corpora.ts
│   │   │   ├── eurlex.ts
│   │   │   ├── italian-legal.ts
│   │   │   └── health.ts
│   │   ├── tauri/                              ← wrapper invoke + canale biometric
│   │   │   ├── commands.ts                     ← api_base_url, open_external_url
│   │   │   └── events.ts                       ← (riservato per futuri eventi)
│   │   ├── types/                              ← TypeScript types (specchio tipi Rust)
│   │   │   ├── auth.ts
│   │   │   ├── user.ts
│   │   │   ├── chat.ts
│   │   │   ├── document.ts
│   │   │   ├── project.ts
│   │   │   ├── workflow.ts
│   │   │   ├── tabular.ts
│   │   │   ├── template.ts
│   │   │   ├── preset.ts
│   │   │   ├── model.ts
│   │   │   ├── mcp.ts
│   │   │   ├── sync.ts
│   │   │   ├── corpus.ts
│   │   │   ├── domain.ts                       ← Domain union + DOMAINS const
│   │   │   ├── health.ts
│   │   │   └── error.ts                        ← shape { detail } + helper
│   │   └── utils/
│   │       ├── format.ts                        ← date, size, percent
│   │       ├── shortcuts.ts
│   │       ├── markdown.ts                      ← marked + DOMPurify pipeline
│   │       ├── sse.ts                           ← reconnect / abort helper
│   │       ├── download.ts                      ← saveBlob, Content-Disposition parse
│   │       └── debounce.ts
│   ├── routes/                                  ← pagine principali
│   │   ├── Boot.svelte                          ← splash + port discovery + /auth/status
│   │   ├── Setup.svelte                         ← prima installazione (POST /auth/setup)
│   │   ├── Unlock.svelte                        ← POST /auth/unlock + biometric
│   │   ├── Assistant.svelte
│   │   ├── Projects.svelte
│   │   ├── TabularReviews.svelte
│   │   ├── Workflows.svelte
│   │   ├── Templates.svelte
│   │   └── Settings.svelte
│   ├── App.svelte                               ← root: layout + router + ErrorBoundary
│   ├── app.css                                  ← Tailwind v4 + CSS vars brand
│   └── main.ts
├── public/
│   └── icon.png                                 ← app icon ex-novo
├── locales/                                     ← 6 lingue: it (canonica), en, fr, de, es, pt
│   ├── it.json
│   ├── en.json
│   ├── fr.json
│   ├── de.json
│   ├── es.json
│   └── pt.json
├── tests/
│   ├── unit/
│   └── e2e/
├── package.json
├── tsconfig.json
├── vite.config.ts
├── playwright.config.ts
├── vitest.config.ts
├── .eslintrc.cjs
├── .prettierrc
├── LICENSE
├── NOTICE
└── README.md
```

> Il **contenuto di `frontend/` nasce ex-novo** dallo scaffolding `pnpm create tauri-app` (template Svelte+TS), staged in un primo commit dedicato dopo aver svuotato la directory. Nessun file proveniente da `frontendMike/` o dal repo Mike originale. La cartella `frontendMike/` resta **read-only** durante lo sviluppo del nuovo: si guardano solo gli **screenshot** del prodotto, mai il sorgente.

---

## 4. Design system — token CSS

### 4.1 Brand audit (sorgente: sito marketing MikeRust + screenshot app)

Esecuzione del `web_fetch` sul sito MikeRust ha confermato:

- **Font:** il sito non hosta font custom; usa lo stack `-apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif`. L'app condivide visivamente lo stesso "system-clean". Scelta per la nuova UI: **Geist** (MIT, Vercel) come default + system-stack come fallback — visivamente identico al sistema attuale ma con controllo cross-platform Windows/macOS. *Niente download font runtime.*
- **Palette:**
  - **Brand** = Tailwind **Orange/Amber** (`#7c2d12` → `#fdba74`). Si possono referenziare i token Tailwind nativi senza inventare nulla.
  - **Neutri** = Tailwind **Gray** (`#f9fafb` → `#111827`).
  - **CTA primarie pill** (sito + bottone "+ Crea nuova" in-app): nero pieno `#111` su bianco, `border-radius: 999px`, padding generoso. **Variante**: usare `--color-text-primary` come fill per coerenza con dark mode.
  - **Badge livello template** (L1/L2/L3): arancio su warm `#fff7ed` (bg) / `#c2410c` (fg) — identico al brand.
  - **Badge tipo workflow** (uniche eccezioni alla palette brand):
    - **Assistant** = blu (`bg: #dbeafe`, `fg: #1e40af`)
    - **Tabular** = viola (`bg: #ede9fe`, `fg: #6d28d9`)

> **Conseguenza:** i token CSS sotto fanno coincidere `--color-brand-*` con i valori Tailwind Orange (no inversione, no shift). Si aggiungono token semantici dedicati per CTA pill, badge livello, badge tipo.

### 4.2 Token CSS

Tutti i token sono definiti ex-novo in `app.css`. I valori brand corrispondono a Tailwind Orange (presi dal brand audit §4.1), non derivati da nessun file Mike.

```css
/* src/app.css */
@import "tailwindcss";

@theme {
  /* Brand (palette ruggine, ex-novo) */
  --color-brand-50:  #fff7ed;
  --color-brand-100: #ffedd5;
  --color-brand-200: #fed7aa;
  --color-brand-300: #fdba74;
  --color-brand-400: #fb923c;
  --color-brand-500: #ea580c;
  --color-brand-600: #c2410c;
  --color-brand-700: #9a3412;
  --color-brand-800: #7c2d12;
  --color-brand-900: #431407;

  /* Superfici (light) */
  --color-surface-0:   #ffffff;
  --color-surface-50:  #f9fafb;
  --color-surface-100: #f3f4f6;
  --color-surface-200: #e5e7eb;
  --color-surface-300: #d1d5db;

  /* Testo (light) */
  --color-text-primary:   #111827;
  --color-text-secondary: #6b7280;
  --color-text-disabled:  #9ca3af;
  --color-text-inverse:   #ffffff;

  /* Stato semantico */
  --color-success-500: #16a34a;
  --color-warning-500: #f59e0b;
  --color-danger-500:  #dc2626;
  --color-info-500:    #2563eb;

  /* Layout */
  --sidebar-width: 272px;
  --topbar-height: 56px;
  --content-max-width: 1080px;

  /* Interattivo */
  --color-active-bg:  #f3f4f6;
  --color-hover-bg:   #f9fafb;
  --color-focus-ring: var(--color-brand-500);

  /* Typography (sistema-prima, Geist override quando bundlato) */
  --font-sans: "Geist", -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  --font-mono: "Geist Mono", ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  --text-xs:   0.75rem;
  --text-sm:   0.875rem;
  --text-base: 1rem;
  --text-lg:   1.125rem;
  --text-xl:   1.25rem;
  --text-2xl:  1.5rem;

  /* Radius */
  --radius-sm: 6px;
  --radius-md: 8px;
  --radius-lg: 12px;
  --radius-xl: 16px;

  /* Shadows */
  --shadow-xs:    0 1px 1px 0 rgb(0 0 0 / 0.04);
  --shadow-sm:    0 1px 2px 0 rgb(0 0 0 / 0.05);
  --shadow-md:    0 4px 6px -1px rgb(0 0 0 / 0.1);
  --shadow-lg:    0 10px 15px -3px rgb(0 0 0 / 0.1);
  --shadow-modal: 0 20px 60px -10px rgb(0 0 0 / 0.15);

  /* Motion */
  --transition-fast:   120ms cubic-bezier(0.4, 0, 0.2, 1);
  --transition-medium: 200ms cubic-bezier(0.4, 0, 0.2, 1);

  /* Componenti specifici (da brand audit §4.1) */
  --cta-pill-bg:       #111111;
  --cta-pill-fg:       #ffffff;
  --cta-pill-radius:   999px;
  --cta-pill-padding:  0.625rem 1.25rem;

  --badge-level-bg:    var(--color-brand-50);    /* #fff7ed */
  --badge-level-fg:    var(--color-brand-600);   /* #c2410c */

  --badge-assistant-bg: #dbeafe;
  --badge-assistant-fg: #1e40af;
  --badge-tabular-bg:   #ede9fe;
  --badge-tabular-fg:   #6d28d9;
}

@media (prefers-color-scheme: dark) {
  @theme {
    --color-surface-0:   #0b0b0e;
    --color-surface-50:  #111114;
    --color-surface-100: #18181c;
    --color-surface-200: #232328;
    --color-surface-300: #2f2f36;
    --color-text-primary:   #f3f4f6;
    --color-text-secondary: #9ca3af;
    --color-text-disabled:  #6b7280;
    --color-active-bg: #18181c;
    --color-hover-bg:  #111114;
  }
}
```

Tema scuro/chiaro **manuale** sopra il `prefers-color-scheme`: store `theme.svelte.ts` con tre stati (`light` | `dark` | `system`) che applica una classe `.theme-dark` al `<html>` quando serve forzare.

---

## 5. Mappatura schermate → componenti nuovi

### 5.1 Boot / Auth (nuove route MikeRust-specifiche)

| Schermata | Componente | Trigger backend |
|---|---|---|
| Splash + port discovery | `routes/Boot.svelte` | `invoke("api_base_url")` + `GET /healthz` + `GET /auth/status` |
| Prima installazione (setup PIN + display name) | `routes/Setup.svelte` + `SetupForm.svelte` | `POST /auth/setup` |
| Sblocco PIN / biometrico | `routes/Unlock.svelte` + `UnlockForm.svelte` + `BiometricPrompt.svelte` | `POST /auth/unlock`, `POST /auth/unlock-biometric`, `GET /auth/biometric-available` |
| Cambio PIN | `ChangePinForm.svelte` (inline in Settings) | `POST /auth/change-pin` |

### 5.2 Assistente

| Elemento UI | Componente nuovo | Backend |
|---|---|---|
| Greeting personalizzato (display_name) | `routes/Assistant.svelte` | `GET /user/profile` |
| Input area chat + invio | `components/chat/ChatInput.svelte` | `POST /chat/{id}/message` (SSE) |
| Dropdown "+ Documenti" | `components/documents/DocumentPicker.svelte` | `GET /document?project_id=…` + upload multipart `POST /document` |
| Modal "Avvia chat in un progetto" | `components/projects/ProjectPicker.svelte` | `GET /project?domain=…` |
| Modal "Aggiungi workflow" con filtro categoria + domain | `components/workflow/WorkflowPicker.svelte` + `DomainSelect.svelte` | `GET /workflow?type=assistant&domain=…` |
| Modal "Scegli template" con livelli L1/L2/L3 | `components/workflow/TemplatePicker.svelte` | `GET /docx-templates?domain=…&locale=…` + `POST /docx-templates/describe` |
| Selector modello LLM (Anthropic/Google/OpenAI/Mistral/Local) | `components/chat/ModelSelector.svelte` | `GET /models` + `GET /user/llm-settings` |
| Lista chat recenti sidebar | `components/chat/ChatHistory.svelte` | `GET /chat?project_id=…` |
| Card "scarica DOCX generato" (download persistente) | `components/chat/DownloadDocxCard.svelte` | (no fetch — riceve metadata via SSE event) |
| Tool-call MCP inline | `components/chat/ToolCallCard.svelte` | (riceve event via SSE) |

### 5.3 Progetti

| Elemento UI | Componente nuovo | Backend |
|---|---|---|
| Lista con tab Tutti / Indip. + filtro domain | `routes/Projects.svelte` + `DomainFilter.svelte` | `GET /project?domain=…` |
| Modal nuovo progetto (nome, descrizione, domain, isolation_mode) | `components/projects/ProjectModal.svelte` | `POST /project`, `PUT /project/{id}` |
| Export progetto `.mikeprj` | `components/projects/ProjectExportModal.svelte` | `POST /project/{id}/export` → blob binario |
| Import progetto `.mikeprj` | `components/projects/ProjectImportModal.svelte` | `POST /project/import` multipart |
| Rinomina documento progetto | inline in `DocumentRow.svelte` | `PATCH /project/{id}/documents/{doc_id}` |

### 5.4 Revisioni tabellari

| Elemento UI | Componente nuovo | Backend |
|---|---|---|
| Lista con colonne Nome/Colonne/Documenti/Progetto/Creato + filtro domain | `routes/TabularReviews.svelte` | `GET /tabular-review?domain=…` |
| Modal nuova revisione (nome, workflow, progetto, documenti, domain) | `components/tabular/TabularReviewModal.svelte` | `POST /tabular-review` |
| Tabella riga-per-documento × colonna-da-workflow | `components/tabular/TabularReviewTable.svelte` (TanStack Table headless) | (rendering client-side da `columns_config`) |
| Aggiungi colonna ad-hoc | `components/tabular/AddColumnModal.svelte` + `DomainSelect` per filtrare preset | `GET /column-presets` |

### 5.5 Workflow

| Elemento UI | Componente nuovo | Backend |
|---|---|---|
| Lista con badge Tipo (Assistant/Tabular) e Domain | `routes/Workflows.svelte` | `GET /workflow?type=…&domain=…` |
| Tab Tutti/Predefiniti/Personalizzati/Nascosti | `components/ui/Tabs.svelte` | `GET /workflow` + `GET /workflow/hidden` |
| Filtri type + practice + domain | composizione `Select.svelte` + `DomainSelect.svelte` | (filtering client-side dopo fetch) |
| Modal nuovo workflow (nome, tipo, domain, practice-chips, prompt_md, columns_config) | `components/workflow/WorkflowModal.svelte` | `POST /workflow`, `PUT /workflow/{id}` |
| Nascondi/mostra preset built-in | inline su riga lista | `POST /workflow/hidden`, `DELETE /workflow/hidden/{id}` |

### 5.6 Template DOCX

| Elemento UI | Componente nuovo | Backend |
|---|---|---|
| Lista con tag multipli, slug, origine | `routes/Templates.svelte` | `GET /docx-templates?domain=…&locale=…` |
| Filtro settore (`also_applicable_to`) + search | `Select.svelte` + `Input.svelte` | (client-side) |
| Preview / contratto template | `TemplatePicker.svelte` (riusato) | `POST /docx-templates/describe` |
| Render manuale (debug) | dialog "Renderizza" | `POST /docx-templates/render` → download .docx |

### 5.7 Impostazioni

Settings è la sezione più densa. Sotto-pagine in tab orizzontale gestite da `SettingsNav.svelte`.

| Sezione | Componente nuovo | Backend |
|---|---|---|
| Profilo (username, display_name, locale, default_domain, cambio PIN) | `settings/ProfileSection.svelte` + `LocaleSelect`, `DefaultDomainSelect`, `ChangePinForm` | `GET /user/profile`, `PUT /user/profile`, `GET/PUT /user/locale`, `GET/PUT /user/default-domain`, `POST /auth/change-pin` |
| Biometrico (enable/disable + stato) | inline `ProfileSection.svelte` | `GET /auth/biometric-available`, `POST /auth/biometric-enable`, `POST /auth/biometric-disable` |
| Modelli LLM (provider pill + 4 card) | `settings/LLMProviderCard.svelte` × 4 (Anthropic/Google/OpenAI/Local) | `GET /models` + `GET /user/llm-settings` + `PUT /user/llm-settings` |
| Server MCP | `settings/MCPServerCard.svelte` + `MCPProbeResult.svelte` | `GET/POST /user/mcp-servers`, `PUT/DELETE /user/mcp-servers/{name}`, `POST /user/mcp-servers/probe` |
| Documenti locali (cartelle indicizzate) | `settings/LocalFolderCard.svelte` + `SyncFolderRow.svelte` + `ScanProgressBar.svelte` | `GET/POST /sync/folders`, `DELETE /sync/folders/{id}`, `POST /sync/folders/{id}/scan`, `GET /sync/folders/{id}/status`, `GET /sync/folders/{id}/files` |
| Stato modello embedding | `settings/EmbeddingModelBanner.svelte` (anche globale in `StatusBar`) | `GET /sync/model-status` (poll 1s mentre downloading/loading) |
| EUR-Lex | `settings/EurlexCard.svelte` | `GET/PUT /eurlex/config`, `POST /eurlex/search`, `POST /eurlex/fetch`, `GET /eurlex/documents`, `DELETE /eurlex/documents/{id}`, `POST /eurlex/documents/{id}/resync`, `GET /eurlex/embed-progress` |
| Italian Legal Corpus | `settings/ItalianLegalCard.svelte` | `GET/PUT /italian-legal/config`, `POST /italian-legal/import`, `GET /italian-legal/import-status`, `POST /italian-legal/search`, `POST /italian-legal/fetch`, `GET /italian-legal/documents`, etc. |
| CNIL / Legifrance / altri corpora dichiarativi | `settings/CorpusCard.svelte` (uno per ogni elemento di `/corpora`) | `GET /corpora`, `POST /corpora/{id}/search`, `POST /corpora/{id}/fetch`, `POST /corpora/{id}/import`, `GET /corpora/{id}/import-status`, `GET /corpora/{id}/import-progress`, `GET /corpora/{id}/documents`, `DELETE /corpora/{id}/documents/{doc_id}` |
| Diagnostica / health | `settings/DiagnosticsSection.svelte` | `GET /healthz` |
| Elimina account | `settings/DeleteAccountSection.svelte` + `ConfirmDialog.svelte` | `DELETE /user/account` |

### 5.8 Componenti trasversali

- **`BiometricPrompt.svelte`** — overlay full-screen mostrato quando il backend richiede biometric (la richiesta arriva via canale Tauri lato backend; sul frontend basta polling/feedback durante l'unlock).
- **`StatusBar.svelte`** — barra inferiore con: stato modello embedding (download %, loading, ready, failed), scan in corso (folder + percentuale), health backend (offline/degraded/ok).
- **`ToastRegion.svelte`** — coda globale di toast per errori API e successi.
- **`ErrorBoundary.svelte`** — root-level fallback per crash UI; in DEV mostra stack, in PROD link a /healthz.

---

## 6. HTTP API surface (contratto completo)

> Il backend MikeRust **NON espone comandi Tauri** per le funzioni di dominio: tutto passa via axum su `http://127.0.0.1:<port>` (porta scoperta a runtime — vedi §7). I soli comandi Tauri esistenti sono `api_base_url` e `open_external_url`.

### 6.1 Mount table (da [src/lib.rs](src/lib.rs))

| Mount | Router | File |
|---|---|---|
| `/auth` | autenticazione | [src/routes/auth.rs](src/routes/auth.rs) |
| `/user` | profilo, locale, default_domain, LLM, MCP, account | [src/routes/user.rs](src/routes/user.rs) |
| `/chat` | chat + messaggi + SSE | [src/routes/chat.rs](src/routes/chat.rs) |
| `/project` | progetti + export/import .mikeprj | [src/routes/projects.rs](src/routes/projects.rs) |
| `/document` (alias `/single-documents`) | upload/download/dedup | [src/routes/documents.rs](src/routes/documents.rs) |
| `/workflow` | workflow CRUD + hidden | [src/routes/workflows.rs](src/routes/workflows.rs) |
| `/column-presets` | preset colonne tabular | [src/routes/presets.rs](src/routes/presets.rs) |
| `/docx-templates` | describe + render | [src/routes/docx_templates.rs](src/routes/docx_templates.rs) |
| `/models` | catalogo provider/modello/regione | [src/routes/models.rs](src/routes/models.rs) |
| `/tabular-review` | revisioni tabellari | [src/routes/tabular_reviews.rs](src/routes/tabular_reviews.rs) |
| `/sync` | cartelle locali + indicizzazione | [src/routes/sync.rs](src/routes/sync.rs) |
| `/eurlex` | corpus EUR-Lex | [src/routes/eurlex.rs](src/routes/eurlex.rs) |
| `/italian-legal` | Italian Legal Corpus (HuggingFace bulk) | [src/routes/italian_legal.rs](src/routes/italian_legal.rs) |
| `/corpora` | corpus generici (manifest YAML) | [src/routes/corpora.rs](src/routes/corpora.rs) |
| `/healthz` | liveness/readiness (no auth) | [src/routes/health.rs](src/routes/health.rs) |

CORS allowlist di default include `tauri://localhost`, `https://tauri.localhost`, `localhost:3000/3001` e `127.0.0.1:3000/3001`; override con env `MIKE_ALLOWED_ORIGINS`. Body limit globale 50 MB (100 MB su `/document`).

### 6.2 Schema tipi TypeScript (specchio Rust)

Generati a mano in `src/lib/types/`, allineati 1:1 ai `serde::Serialize/Deserialize` del backend. Esempio per `domain`:

```typescript
// src/lib/types/domain.ts
export const DOMAINS = [
  'legal',
  'medical',
  'finance',
  'real_estate',
  'hr',
  'insurance',
  'ip',
  'compliance',
  'others',
] as const
export type Domain = typeof DOMAINS[number]
export const DEFAULT_DOMAIN: Domain = 'legal'
```

```typescript
// src/lib/types/user.ts
export interface UserProfile {
  id: string
  username: string
  display_name: string | null
  created_at: string
}
export interface LlmSettings {
  main_model?: string
  title_model?: string
  tabular_model?: string
  claude_api_key?: string
  gemini_api_key?: string
  gemini_region?: string
  gemini_model?: string
  openai_api_key?: string
  openai_model?: string
  local_base_url?: string
  local_api_key?: string
  local_model?: string
  active_provider?: 'anthropic' | 'google' | 'openai' | 'local'
}
export type Locale = 'it' | 'en' | 'fr' | 'de' | 'es' | 'pt'
```

```typescript
// src/lib/types/workflow.ts
export interface Workflow {
  id: string
  title: string
  type: 'assistant' | 'tabular'
  prompt_md: string | null
  practice: string | null
  columns_config: ColumnConfig[] | null
  domain: Domain
  origin: 'user' | 'preset'
  created_at: string
}
export interface ColumnConfig {
  key: string
  label: string
  prompt: string
  format?: 'text' | 'list' | 'date' | 'number' | 'boolean' | 'reference'
}
```

(I tipi restanti — `Chat`, `Message`, `Document`, `Project`, `TabularReview`, `DocxTemplate`, `McpServer`, `SyncFolder`, `ModelStatus`, `CorpusPlugin`, etc. — seguono lo stesso pattern. Vedi §6.4 per la procedura di sync con il backend.)

### 6.3 Client HTTP centralizzato

```typescript
// src/lib/api/client.ts — scritto da zero
import { authStore } from '$lib/stores/auth.svelte'
import { apiBase } from '$lib/stores/api-base.svelte'
import { toastStore } from '$lib/stores/toast.svelte'

export class ApiError extends Error {
  constructor(public status: number, public detail: string, public raw?: unknown) {
    super(detail)
  }
}

interface RequestOptions {
  method?: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'
  body?: unknown
  query?: Record<string, string | number | boolean | undefined>
  signal?: AbortSignal
  asBlob?: boolean
  multipart?: FormData
}

export async function api<T>(path: string, opts: RequestOptions = {}): Promise<T> {
  const url = new URL(path, apiBase.url || 'http://127.0.0.1:3001')
  if (opts.query) {
    for (const [k, v] of Object.entries(opts.query)) {
      if (v !== undefined && v !== '') url.searchParams.set(k, String(v))
    }
  }

  const headers: HeadersInit = { Accept: 'application/json' }
  if (authStore.token) headers.Authorization = `Bearer ${authStore.token}`

  let body: BodyInit | undefined
  if (opts.multipart) {
    body = opts.multipart
  } else if (opts.body !== undefined) {
    headers['Content-Type'] = 'application/json'
    body = JSON.stringify(opts.body)
  }

  let res: Response
  try {
    res = await fetch(url, { method: opts.method ?? 'GET', headers, body, signal: opts.signal })
  } catch (e) {
    throw new ApiError(0, `Network error: ${(e as Error).message}`)
  }

  if (res.status === 401) {
    authStore.invalidate()
    throw new ApiError(401, 'unauthorized')
  }

  if (!res.ok) {
    let detail = res.statusText
    try {
      const j = await res.json()
      detail = (j as { detail?: string }).detail ?? detail
    } catch { /* non-JSON error */ }
    throw new ApiError(res.status, detail)
  }

  if (opts.asBlob) return (await res.blob()) as unknown as T
  if (res.status === 204) return undefined as unknown as T
  return (await res.json()) as T
}
```

### 6.4 Procedura di sync tipi Rust → TypeScript

Per evitare drift dei tipi tra Rust e TS:

1. **Convenzione naming**: i tipi Rust su rete usano snake_case (serde default); i tipi TS rispecchiano snake_case 1:1 (niente trasformazioni camelCase nascoste).
2. **Smoke test contratto**: in `tests/contract/`, fixtures JSON che il client deve poter deserializzare; aggiornati ogni volta che cambia un `serde` field.
3. **Generatore opzionale**: tool `typeshare-cli` o `ts-rs` (Apache-2.0/MIT) come arma di precisione futura; in MVP è sufficiente scrittura manuale + smoke test.

---

## 7. Tauri shell integration

Il backend Rust gira in **thread separato** lanciato da `src-tauri/src/lib.rs`. Il frontend dialoga **principalmente via HTTP**; i due comandi Tauri sono:

### 7.1 `api_base_url(): string`
Restituisce `http://127.0.0.1:<port>` dove `<port>` è scelta dall'OS al boot (`PORT=0`) o quella esplicita in `.env`. **Chiamato una sola volta** all'avvio. Se vuoto, il frontend ricade su `VITE_API_BASE_URL` (build env) o `http://127.0.0.1:3001`.

```typescript
// src/lib/tauri/commands.ts
import { invoke } from '@tauri-apps/api/core'

export async function getApiBaseUrl(): Promise<string> {
  try {
    const u = await invoke<string>('api_base_url')
    if (u) return u
  } catch { /* fallback */ }
  return import.meta.env.VITE_API_BASE_URL ?? 'http://127.0.0.1:3001'
}

export async function openExternal(url: string): Promise<void> {
  if (!/^https?:\/\//.test(url)) throw new Error('only http/https')
  await invoke('open_external_url', { url })
}
```

### 7.2 Biometric channel (lato backend)
La chiamata biometrica è iniziata da axum (`POST /auth/unlock-biometric` o flussi protetti), che invia una `BiometricRequest = (reason, oneshot)` sul canale Tauri (`bio_tx`). Il shell apre il dialog Windows Hello e risponde. **Il frontend NON gestisce direttamente Windows Hello**: vede solo l'attesa della risposta HTTP e mostra un overlay (`BiometricPrompt.svelte`) finché la POST non torna. Su piattaforme non-Windows il backend risponde 501.

### 7.3 Boot sequence (frontend)

```typescript
// src/routes/Boot.svelte (pseudocodice)
async function boot() {
  apiBase.url = await getApiBaseUrl()                  // 1. discovery porta
  await health.probe()                                 // 2. GET /healthz (timeout 5s)
  const status = await authApi.status()                // 3. GET /auth/status
  if (!status.has_profile) router.go('setup')
  else if (!authStore.token) router.go('unlock')
  else {
    await authApi.me()                                 // 4. validate cached token
    await loadGlobalCatalogues()                       // 5. /models, /workflow, /docx-templates, /corpora
    router.go('assistant')
  }
}
```

### 7.4 Switch vecchio/nuovo via doppio `tauri.conf.json`

Tauri 2 accetta il flag `--config <path>` su `cargo tauri dev` e `cargo tauri build`. Lo sfruttiamo per tenere **due configurazioni parallele** che differiscono **solo** nella sezione `build`:

#### `src-tauri/tauri.conf.json` (default, nuovo frontend Svelte)

```jsonc
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "MikeRust",
  "version": "0.1.0",
  "identifier": "ai.semplifica.mikerust",
  "build": {
    "frontendDist": "../frontend/dist",
    "devUrl": "http://localhost:5173",
    "beforeDevCommand": "pnpm --dir ../frontend dev",
    "beforeBuildCommand": "pnpm --dir ../frontend build"
  },
  "app": {
    "windows": [{
      "title": "MikeRust",
      "width": 1280, "height": 800,
      "minWidth": 960, "minHeight": 640,
      "resizable": true,
      "fullscreen": false
    }],
    "security": {
      "csp": null,
      "_csp_reco": "default-src 'self'; connect-src 'self' http://127.0.0.1:*; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; script-src 'self'",
      "_csp_note": "lasciato null finché Fase 6 non verifica che Svelte+Vite+Tailwind+marked non richiedano 'unsafe-eval' in prod"
    },
    "withGlobalTauri": false
  },
  "plugins": {}
}
```

#### `src-tauri/tauri.legacy.conf.json` (vecchio frontend Mike/Next.js)

Identico al precedente eccetto:

```jsonc
{
  "build": {
    "frontendDist": "../frontendMike/out",
    "devUrl": "http://localhost:3000",
    "beforeDevCommand": "npm --prefix ../frontendMike run dev",
    "beforeBuildCommand": "npm --prefix ../frontendMike run build:tauri"
  }
}
```

#### Wrapper script (root `package.json` di MikeRust)

```jsonc
{
  "scripts": {
    "dev":         "cargo tauri dev",
    "dev:legacy":  "cargo tauri dev --config src-tauri/tauri.legacy.conf.json",
    "build":       "cargo tauri build",
    "build:legacy":"cargo tauri build --config src-tauri/tauri.legacy.conf.json"
  }
}
```

> Per evitare di installare `pnpm` come dipendenza di MikeRust quando si lavora solo sul legacy: gli script default usano `cargo tauri ...` direttamente; `npm` continua a essere usato nel legacy come prima. Il nuovo frontend richiede `pnpm` per coerenza con lo scaffolding Tauri.

#### Workflow durante la migrazione

```pwsh
# Lavorare sul nuovo (default):
cargo tauri dev

# Tornare al legacy in 1 comando (es. per QA regression):
cargo tauri dev --config src-tauri/tauri.legacy.conf.json

# Build di entrambi i bundle in parallelo (CI):
cargo tauri build                                            # produce installer con nuovo
cargo tauri build --config src-tauri/tauri.legacy.conf.json  # produce installer con legacy
```

#### Note operative

- I due frontend **non possono girare contemporaneamente in dev** (collidere sulla finestra Tauri); l'app stessa ascolta su una porta diversa per ognuno (5173 vs 3000), ma Tauri lancia una sola window per istanza.
- Quando il nuovo frontend raggiunge parità feature, **rimuoviamo `tauri.legacy.conf.json` e `frontendMike/`** in un commit dedicato. Mantenere il legacy oltre Fase 8 è una contaminazione AGPL latente: il codice resta sul disco di sviluppo ma non deve finire in nessun bundle distribuito.
- L'integrazione **biometrica** e i 2 comandi Tauri (`api_base_url`, `open_external_url`) sono nella shell, **non nei file di conf**: funzionano identicamente per entrambi i frontend.

> **CSP** raccomandata (commento `_csp_reco`): chiude un buco oggi aperto. Da attivare in Fase 6 dopo verifica che Svelte+Vite+Tailwind+marked non richiedano `'unsafe-eval'`. Mantenuta `null` in fase di sviluppo per non bloccare HMR.

### 7.5 Plugin Tauri

| Plugin | Stato | Note |
|---|---|---|
| `tauri-plugin-single-instance` | **MVP — Fase 8** (crate Rust, no API JS) | Previene doppio launch (porta 3001 collide). Decisione Q3. Si aggiunge in `src-tauri/Cargo.toml`, NON con `pnpm add`. |
| `tauri-plugin-stronghold` | **MVP — Fase 3** | Persistenza cifrata del token "Mantieni accesso" (decisione Q10). Master-password derivata dal PIN. |
| `tauri-plugin-updater` | **Post-MVP** (schema endpoint definito ora — §23 Q4) | Endpoint `https://updates.mikerust.app/{target}/{current_version}`, firma minisign. Generare chiave pubblica e committarla nel conf quando si attiva. |
| `tauri-plugin-dialog` | **MVP — Fase 5** | File picker nativo per `.mikeprj` import (più ergonomico di `<input type=file>` in Tauri WebView). |
| `tauri-plugin-window-state` | **MVP — Fase 8** | Persistere size/position della window tra restart. |
| `tauri-plugin-os` | **Post-MVP** | Branding "MikeRust su Windows 11 ARM64" in About. |
| `tauri-plugin-fs` | **Non aggiungere** | Sconsigliato — preferire endpoint HTTP che validano i path (vedi `storage::safe_path_under`). |

---

## 8. Auth flow & session lifecycle

### 8.1 Stati possibili
```
       ┌────────────┐
       │  no profile│──── POST /auth/setup ──┐
       └────────────┘                        ▼
                                       ┌──────────┐
       locked  ◀────── logout ─────────│ unlocked │
        │                              └──────────┘
        ├── POST /auth/unlock (PIN) ─────────▲
        └── POST /auth/unlock-biometric ─────┘
```

### 8.2 Token & storage (decisione Q10)
- Bearer token restituito da `setup` / `unlock` / `unlock-biometric`.
- **Default:** token **in memoria sola** (rune `$state` in `auth.svelte.ts`). Su chiusura app → si perde → al prossimo avvio l'utente sblocca con PIN/biometrico.
- **Opt-in "Mantieni accesso fra riavvii"** (toggle in Settings → Profilo): al `true`, il token è salvato in **`tauri-plugin-stronghold`** (cifratura at-rest, master-password derivata dal PIN). Al boot:
  1. `Boot.svelte` chiama `stronghold.load(...)` → se vault esiste, mostra `Unlock` con PIN/biometrico
  2. unlock decifra → token restored in `auth.svelte.ts` → router → `Assistant`
- **Mai** `localStorage` né `sessionStorage` (coerente con regola persistente "prefer data/storage over localStorage" + immune da XSS).
- **Header:** `Authorization: Bearer <token>` su ogni request.
- **Hydratation:** all'avvio, dopo stronghold-restore, `GET /auth/me` → se 401, wipe + Unlock.

### 8.3 Rate-limit / lockout
Il backend ha (o avrà — Batch C in-flight) un rate-limit IP-based su `/auth/unlock`. Risposta lockout:
```json
HTTP 429 Too Many Requests
Retry-After: 60
{ "detail": "Too many failed attempts; retry in 60s" }
```
Il frontend mostra countdown nella `UnlockForm.svelte` ricavando i secondi da `Retry-After` (no polling).

### 8.4 Biometric flow
```
[BiometricPrompt overlay]                [backend]               [Tauri shell]
       │                                     │                         │
       │── POST /auth/unlock-biometric ─────▶│                         │
       │                                     │── BiometricRequest ────▶│
       │                                     │                         │  Windows Hello
       │                                     │◀──── result(true) ──────│
       │◀── { token, user } ─────────────────│                         │
```
Overlay disabilita input chat finché POST non torna; timeout 30s.

### 8.5 Logout
`POST /auth/logout` (revoke_all sul backend) + wipe token client-side + router → Unlock.

---

## 9. Streaming chat (SSE)

### 9.1 Contratto
- **Endpoint:** `POST /chat/{id}/message`
- **Content-Type response:** `text/event-stream`
- **Body request:** `application/json` o `multipart/form-data` se allegati inline (preferenza: documenti già caricati via `/document` e passati per ID).

### 9.2 Eventi
Eventi nominati (`event: <name>\n` + `data: <json>\n\n`). Naming finale **da congelare in §6.4 contract tests**; quelli osservati nel backend includono almeno:

| Evento | Payload | Significato |
|---|---|---|
| `start` | `{ chat_id, message_id, model }` | Inizio risposta assistant |
| `token` | `{ delta }` | Frammento testo |
| `tool_call_start` | `{ tool_call_id, name, server, args_partial? }` | MCP/local tool invocato |
| `tool_call_chunk` | `{ tool_call_id, args_delta }` | Argomenti tool in streaming |
| `tool_call_end` | `{ tool_call_id, result_preview? }` | Risultato tool |
| `phase` | `{ phase: "retrieving"\|"thinking"\|"generating"\|... }` | Indicatore stato |
| `doc_created` | `{ template_id, filename, document_id, download_url }` | DOCX renderizzato → card download |
| `usage` | `{ input_tokens, output_tokens }` | Tokens spesi |
| `error` | `{ detail, code? }` | Errore recuperabile (provider down, key missing) |
| `done` | `{ finish_reason }` | Fine risposta |
| `heartbeat` | `{}` | Keep-alive (60s) — backend invia per evitare proxy timeout |

### 9.3 Client

```typescript
// src/lib/api/chat.ts (estratto)
import { apiBase } from '$lib/stores/api-base.svelte'
import { authStore } from '$lib/stores/auth.svelte'

export interface StreamCallbacks {
  onEvent: (name: string, data: unknown) => void
  onError: (err: Error) => void
  onClose: () => void
}

export function streamMessage(
  chatId: string,
  payload: SendMessagePayload,
  cb: StreamCallbacks,
): AbortController {
  const ctrl = new AbortController()
  fetch(new URL(`/chat/${chatId}/message`, apiBase.url), {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Accept: 'text/event-stream',
      Authorization: `Bearer ${authStore.token}`,
    },
    body: JSON.stringify(payload),
    signal: ctrl.signal,
  }).then(async (res) => {
    if (!res.ok || !res.body) {
      cb.onError(new Error(`stream ${res.status}`))
      return
    }
    const reader = res.body.getReader()
    const decoder = new TextDecoder()
    let buf = ''
    while (true) {
      const { value, done } = await reader.read()
      if (done) break
      buf += decoder.decode(value, { stream: true })
      let idx
      while ((idx = buf.indexOf('\n\n')) >= 0) {
        const chunk = buf.slice(0, idx)
        buf = buf.slice(idx + 2)
        const ev = parseSseChunk(chunk)
        if (ev) cb.onEvent(ev.name, ev.data)
      }
    }
    cb.onClose()
  }).catch(cb.onError)
  return ctrl
}
```

> Si usa `fetch` + `ReadableStream` invece di `EventSource` perché EventSource **non supporta header `Authorization` né POST**. Reconnect manuale solo per `heartbeat` mancanti (>90s).

### 9.4 Abort
`Stop generation` button → `ctrl.abort()` + UI flush.

### 9.5 Streaming UX spec (decisione Q6)

L'obiettivo è la **miglior esperienza utente possibile** sullo streaming chat. Dettaglio implementativo per `MessageStream.svelte`:

#### Rendering
- Buffer di token in coda + flush con `requestAnimationFrame` (~16 ms) → niente layout thrashing su delta corte.
- Markdown re-parse incrementale: ogni N ms (200), il buffer corrente passa per `marked` + `DOMPurify` → render. Evita re-parse su ogni char.
- Code block: tokens dentro un blocco accumulati come testo grezzo, syntax-highlight (`shiki`) solo a chiusura del blocco (`` ``` `` riconosciuto).

#### Cursore "writing"
- Span con classe `.streaming-caret` (blinking 1 s) appeso al fondo dell'`assistant` durante streaming, rimosso su `done`.

#### Auto-scroll intelligente
- Listener `scroll` su contenitore chat.
- Stato `stickToBottom = true` di default.
- Se l'utente scrolla **verso l'alto** > 64 px dal fondo → `stickToBottom = false` e si mostra badge `↓ Continua a leggere (N nuovi)` (contatore di token arrivati nel frattempo).
- Click sul badge → torna in fondo + `stickToBottom = true`.
- Se l'utente scrolla **al fondo manualmente** → `stickToBottom = true`.

#### Controlli
- **Stop** (icona `Square`): visibile durante streaming, chiama `ctrl.abort()` + flush parziale + finalizza messaggio con `finish_reason: "aborted"`.
- **Regenerate** (icona `RotateCcw`): visibile su ogni messaggio `assistant` dopo `done`, riapre lo stream con lo stesso payload utente.
- **Copy** (icona `Copy`): copia il testo finale (no markdown).
- **Provider switch on error**: se l'errore include `code: "key_missing"` o `code: "provider_down"`, banner mostra anche bottone "Cambia provider" che apre `ModelSelector` overlay.

#### Tool-call inline (`ToolCallCard.svelte`)
- 3 stati visivi: `pending` (skeleton + spinner), `executing` (badge `MCP · <server>` + spinner + nome tool), `complete` (badge verde + preview risultato) / `error` (badge rosso + detail).
- Argomenti del tool collassati di default, expand al click.
- Risultato JSON pretty-printed con `shiki` lang `json`.

#### Phase indicator
- Sotto al messaggio in costruzione, pill grigia che mostra la fase corrente: `Recupero documenti…` / `Sto pensando…` / `Sto scrivendo…`. Sorgente: SSE event `phase`.

#### Watchdog connessione
- Timer client lato `MessageStream`: se non arriva nessun evento (incluso `heartbeat`) per **> 90 s**, mostra banner ambra "Connessione lenta — il modello potrebbe essere occupato" + bottone "Riprova" (= abort + retry con stesso body).
- Su `error: "network"` (status 0 in `ApiError`): banner rosso + "Backend offline" + check `/healthz`.

#### Riassunto: gerarchia visiva
```
┌─────────────────────────────────────────────┐
│ user message                                │
└─────────────────────────────────────────────┘
┌─────────────────────────────────────────────┐
│ assistant ┃ Recupero documenti…             │  ← phase pill (top)
│ [tool-call card: pdf_search · pending]      │
│ Ho trovato 3 documenti rilevanti. Il primo… │
│ ▌                                            │  ← streaming caret
│                                              │
│ [tool-call card: extract_text · executing]  │
│ …                                            │
│ [Stop ◾]                                    │  ← controlli (bottom-right)
└─────────────────────────────────────────────┘
↓ Continua a leggere (12 nuovi)                ← se utente scrolla su
```

---

## 10. File upload & download

### 10.1 Upload documento

```
POST /document
multipart/form-data
fields:
  file        : binary (required)
  project_id  : text (optional)
  cache       : text "true"|"false" (default false; true = dedup SHA-256 cross-user)
  domain      : text (optional; fallback project.domain → "legal")
```

Tipi supportati (backend `documents.rs`): pdf, docx, rtf, xlsx, xls, xlsb, ods, csv, txt, md, png, jpg/jpeg, tif/tiff, other.

Frontend:
- **Dropzone** con drag&drop + click → file picker.
- **Progress bar** durante upload (XHR per `onprogress`).
- Body limit 100 MB su `/document` — error UI mappa 413 in toast "File troppo grande".

### 10.2 Download / display documento

| Endpoint | Uso UI |
|---|---|
| `GET /document/{id}/display` | `PdfViewer.svelte` (basato su `pdfjs-dist`) per pdf; `<img>` per immagini; viewer testuale syntax-highlighted (`shiki`) per txt/md/csv |
| `GET /document/{id}/download` | `Content-Disposition: attachment` → trigger `<a download>` |
| `GET /document/{id}/url` | Restituisce URL stringa per viewer esterni |

#### `PdfViewer.svelte` (decisione Q5)

Wrapper attorno a `pdfjs-dist` (Apache-2.0, ~1 MB). Carica lazy con `import('pdfjs-dist')` solo quando il viewer viene aperto. Features minime:
- Worker pdfjs caricato come asset Vite (`?url` import per il `.worker.min.mjs`)
- Toolbar custom: page prev/next + page input + zoom (50%–400%) + ricerca testo
- Rendering canvas per pagina, virtualizzato (`svelte-virtual`) sulle pagine non-visibili
- Tema scuro: filtro CSS `invert(1) hue-rotate(180deg)` sul canvas quando `theme.mode === 'dark'`
- Selezione testo abilitata (text-layer overlay sopra il canvas)
- Bottone "Apri esternamente" → `openExternal(`api_base_url + /document/{id}/download`)` come fallback

```typescript
// src/lib/utils/download.ts
export async function downloadAs(url: string, filename?: string) {
  const res = await fetch(url, { headers: { Authorization: `Bearer ${token}` } })
  const blob = await res.blob()
  const cd = res.headers.get('Content-Disposition')
  const name = filename ?? parseContentDispositionFilename(cd) ?? 'download'
  const objectUrl = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = objectUrl; a.download = name; a.click()
  URL.revokeObjectURL(objectUrl)
}
```

### 10.3 DOCX render

`POST /docx-templates/render` → blob `.docx`. Header speciale `X-Unresolved-Placeholders` (CSV) → toast warning se non vuoto.

### 10.4 Project export/import

- Export: `POST /project/{id}/export` body `{ recipient_email, include_chats? }` → `application/octet-stream` (`.mikeprj`).
- Import: `POST /project/import` multipart `file` + `recipient_email` → `{ project_id, document_count, chat_count }`.

---

## 11. Workflow / template / preset architecture

### 11.1 Problema originale
In Mike, workflow e template erano **importati come moduli ES** nel bundle frontend → parte del codice AGPL.

### 11.2 Nuova architettura
Tutti i JSON vivono nel backend (`config/` del repo MikeRust) e vengono **letti a runtime in memoria** all'avvio:

```
config/
├── workflows/                ← caricati da AppState::load_workflow_presets
│   ├── legal/*.json
│   ├── medical/*.json
│   ├── finance/*.json
│   ├── insurance/*.json
│   ├── real_estate/*.json
│   ├── hr/*.json
│   ├── ip/*.json
│   └── compliance/*.json
├── column-presets/           ← caricati da AppState::load_column_presets
│   ├── legal/*.json
│   ├── insurance/*.json      ← Blocco A common (commit 852f558)
│   └── …
├── docx-templates/           ← caricati da AppState::load_docx_templates
│   ├── *.json + *.docx       ← coppie (manifest + binary)
│   └── relazione-medico-legale.json   (commit b7b30ed)
│   └── inventario-beni-assicurati.json (commit 6bdb0eb)
├── corpora/                  ← manifest YAML dei corpora dichiarativi
│   ├── cnil.yaml
│   └── …
└── model.json                ← catalogo provider/modello/regione
```

Il frontend **non legge filesystem**: ottiene i preset via `GET /workflow`, `GET /column-presets`, `GET /docx-templates`, `GET /models`, `GET /corpora`. Sono perciò **dati**, non codice — fuori dal perimetro AGPL.

### 11.3 API wrapper

```typescript
// src/lib/api/workflows.ts — scritto da zero
import { api } from './client'
import type { Workflow } from '$lib/types/workflow'
import type { Domain } from '$lib/types/domain'

export const workflowsApi = {
  list: (params?: { type?: 'assistant' | 'tabular'; domain?: Domain }) =>
    api<Workflow[]>('/workflow', { query: params }),
  get: (id: string) =>
    api<Workflow>(`/workflow/${id}`),
  create: (payload: Partial<Workflow>) =>
    api<Workflow>('/workflow', { method: 'POST', body: payload }),
  update: (id: string, payload: Partial<Workflow>) =>
    api<Workflow>(`/workflow/${id}`, { method: 'PUT', body: payload }),
  remove: (id: string) =>
    api<void>(`/workflow/${id}`, { method: 'DELETE' }),
  listHidden: () => api<{ workflow_id: string }[]>('/workflow/hidden'),
  hide: (workflow_id: string) =>
    api<void>('/workflow/hidden', { method: 'POST', body: { workflow_id } }),
  unhide: (workflow_id: string) =>
    api<void>(`/workflow/hidden/${workflow_id}`, { method: 'DELETE' }),
}
```

Pattern identico per `templatesApi`, `presetsApi`, `modelsApi`, `corporaApi`.

---

## 12. State management con Svelte 5 runes

Nessuna libreria esterna. Ogni store è un file `.svelte.ts` con factory + singleton export.

### 12.1 Pattern base

```typescript
// src/lib/stores/workflows.svelte.ts — scritto da zero
import type { Workflow } from '$lib/types/workflow'
import { workflowsApi } from '$lib/api/workflows'
import type { Domain } from '$lib/types/domain'

function createWorkflowStore() {
  let items = $state<Workflow[]>([])
  let hidden = $state<Set<string>>(new Set())
  let loading = $state(false)
  let lastError = $state<string | null>(null)

  const visible = $derived(items.filter(w => !hidden.has(w.id)))
  const byDomain = $derived.by(() => {
    const map = new Map<Domain, Workflow[]>()
    for (const w of visible) {
      if (!map.has(w.domain)) map.set(w.domain, [])
      map.get(w.domain)!.push(w)
    }
    return map
  })

  async function refresh(filter?: { type?: string; domain?: Domain }) {
    loading = true; lastError = null
    try {
      const [all, h] = await Promise.all([
        workflowsApi.list(filter),
        workflowsApi.listHidden(),
      ])
      items = all
      hidden = new Set(h.map(x => x.workflow_id))
    } catch (e) {
      lastError = (e as Error).message
    } finally {
      loading = false
    }
  }

  return {
    get items() { return items },
    get visible() { return visible },
    get byDomain() { return byDomain },
    get loading() { return loading },
    get lastError() { return lastError },
    refresh,
    async hide(id: string) { await workflowsApi.hide(id); hidden = new Set([...hidden, id]) },
    async unhide(id: string) { await workflowsApi.unhide(id); const s = new Set(hidden); s.delete(id); hidden = s },
  }
}

export const workflowStore = createWorkflowStore()
```

### 12.2 Store inventory (un riepilogo)

| Store | Stato | Operazioni chiave |
|---|---|---|
| `router` | `current: Route` | `navigate(r)` |
| `apiBase` | `url: string` | popolato al boot |
| `auth` | `token, user, locked` | `setup, unlock, unlockBiometric, logout, hydrate` |
| `user` | `profile, locale, defaultDomain, llmSettings` | `refresh, updateLocale, updateDefaultDomain, updateLlm` |
| `chat` | `sessions, activeId, streaming, events` | `create, send, abort, delete, generateTitle` |
| `projects` | `items, activeId` | `refresh, create, update, exportProject, importProject` |
| `documents` | `byProject: Map, uploading: Set` | `upload, delete, list` |
| `workflows` | `items, hidden, byDomain` | `refresh, create, update, hide, unhide` |
| `columnPresets` | `items` | `refresh` (read-only — backend non ha POST) |
| `tabular` | `items, active` | `refresh, create, delete` |
| `templates` | `items, byDomain` | `refresh, describe, render` |
| `models` | `catalogue, active` | `refresh, switchProvider` |
| `mcp` | `servers, lastProbe` | `refresh, upsert, delete, probe` |
| `sync` | `folders, scanStatus: Map, files: Map` | `addFolder, scan, pollStatus, listFiles` |
| `embedModel` | `state, downloaded, total, file, error` | `pollWhileBusy` |
| `corpora` | `plugins, eurlex, italianLegal, generic: Map` | `refresh, search, fetch, importBulk, pollImport` |
| `health` | `last, polling` | `probe` |
| `i18n` | `locale, dict, fallback` | `setLocale, t(key, params)` |
| `toast` | `queue` | `push, dismiss` |
| `theme` | `mode: 'light'\|'dark'\|'system'` | `setMode` |

---

## 13. Router SPA leggero

```typescript
// src/lib/stores/router.svelte.ts
export type Route =
  | 'boot' | 'setup' | 'unlock'
  | 'assistant' | 'projects' | 'tabular' | 'workflows' | 'templates' | 'settings'

export interface RouteState {
  name: Route
  params?: Record<string, string>
}

function createRouter() {
  let current = $state<RouteState>({ name: 'boot' })
  const history: RouteState[] = []
  return {
    get current() { return current },
    navigate(name: Route, params?: Record<string, string>) {
      history.push(current)
      current = { name, params }
    },
    back() {
      const prev = history.pop()
      if (prev) current = prev
    },
  }
}
export const router = createRouter()
```

App.svelte espone il dispatcher (vedi snippet originale §8 del piano). Aggiunte:
- `BiometricPrompt` come overlay globale controllato da `auth.svelte.ts`.
- `ToastRegion` montato in App.svelte.
- `StatusBar` montato in `AppShell` per route post-unlock.

---

## 14. I18n — 6 locali con fallback (decisione Q8)

MikeRust supporta **`en` canonica** + `it`, `fr`, `de`, `es`, `pt` come traduzioni. Backend persiste la scelta utente (`/user/locale`).

### 14.1 Riuso del bundle i18n MikeRust

Il bundle in `frontendMike/messages/*.json` (770+ chiavi su 6 locali) è stato **scritto ex-novo** dal proprietario di MikeRust nei commit:
`0b575ca` (i18n iniziale), `f78c8bb` (fr full), `94ed69f` (Domains namespace), `407c296` (Account → Generali), `16c8308` / `3faa20d` (refine), e successivi su preset bundle (medical/commercialista/insurance).

→ **Copyright del proprietario** → **riutilizzabile integralmente** nel nuovo frontend.

**Procedura di import:**
1. Copiare `frontendMike/messages/{en,it,fr,de,es,pt}.json` → `frontend/locales/`
2. Bonifica: per ogni chiave, verificare con `git blame frontendMike/messages/en.json` che la riga sia stata **introdotta in MikeRust** (commit con autore proprietario), non ereditata da un commit pre-fork. Quelle pre-fork si **rifrasano** (cambiare wording mantenendo significato).
3. Rinominare le chiavi se vuoi distanziarti ulteriormente (es. `chat.send` → `chat.action_send`). Non strettamente necessario perché i nomi-chiave sono fact descriptors, non opera creativa.
4. Adattare alla nuova struttura namespace (vedi §14.3).
5. Rimuovere le chiavi orfane (UI che non rifaremo, ad es. mode-specifici di Mike) — `Vitest` check parità chiavi le rivela.

### 14.2 Catene di caricamento
- All'avvio: `i18nStore.setLocale(user.locale ?? navigator.language.slice(0,2) ?? 'en')`
- Dict caricato via dynamic import: `await import(`../locales/${locale}.json`)`
- Fallback chain: `<locale>` → `en` (key missing → log warning in DEV)

### 14.3 Namespacing chiavi
```
{
  "Common": { "save": "Salva", "cancel": "Annulla", ... },
  "Auth":   { "unlock_title": "Sblocca MikeRust", ... },
  "Chat":   { ... },
  "Domains": {
    "legal": "Legale", "medical": "Medico", "finance": "Finanza",
    "real_estate": "Immobiliare", "hr": "HR", "insurance": "Assicurativo",
    "ip": "Proprietà intellettuale", "compliance": "Compliance", "others": "Altro"
  },
  "Errors": {
    "unauthorized": "Sessione scaduta", "network": "Backend offline", ...
  }
}
```

### 14.4 Funzione `t`

```typescript
// src/lib/stores/i18n.svelte.ts
type Dict = Record<string, Record<string, string>>
function createI18n() {
  let locale = $state<Locale>('it')
  let dict = $state<Dict>({})
  let fallback = $state<Dict>({})
  async function setLocale(loc: Locale) {
    locale = loc
    dict = (await import(`../../locales/${loc}.json`)).default
    if (loc !== 'en') fallback = (await import('../../locales/en.json')).default
  }
  function t(key: string, params?: Record<string, string | number>): string {
    const [ns, k] = key.split('.')
    let raw = dict[ns]?.[k] ?? fallback[ns]?.[k] ?? key
    if (params) for (const [p, v] of Object.entries(params))
      raw = raw.replaceAll(`{${p}}`, String(v))
    return raw
  }
  return { get locale() { return locale }, setLocale, t }
}
export const i18n = createI18n()
```

> **Regola di scrittura stringhe**: ogni nuovo testo UI **deve** essere aggiunto in **tutte e 6 le lingue** (regola pre-esistente di MikeRust). In dev, una check Vitest scansiona i `t(...)` e assicura la parità di chiavi.

### 14.5 Identificatori canonici inglesi
Convenzione MikeRust (vedi memoria persistente): identificatori di schema (enum value, JSON keys, route params) restano in **inglese snake_case**; le 6 lingue traducono solo i **label display**. Esempio: il valore `Domain` su rete è `legal`, l'UI mostra `"Legal"` (en, canonica) / `"Legale"` (it) / `"Juridique"` (fr).

---

## 15. Domain & model catalogue

### 15.1 Domain (9-value enum)
Sorgente di verità: backend (`src/domain.rs`).  Frontend espone:

```typescript
// src/lib/components/domain/DomainSelect.svelte (pseudo)
<script lang="ts">
  import { DOMAINS, type Domain } from '$lib/types/domain'
  import { i18n } from '$lib/stores/i18n.svelte'
  let { value = $bindable<Domain>(), allowed }: { value: Domain; allowed?: Domain[] } = $props()
  const opts = $derived(allowed ?? DOMAINS)
</script>
<select bind:value>
  {#each opts as d}
    <option value={d}>{i18n.t(`Domains.${d}`)}</option>
  {/each}
</select>
```

`DomainFilter.svelte`: chip group con stessa semantica, `multiple` opzionale.

### 15.2 Model catalogue
Sorgente: `GET /models` → `{ providers: [...] }` (forma verbatim, copia di `config/model.json`).

```typescript
// src/lib/types/model.ts (snippet)
export interface ModelCatalogue {
  providers: Provider[]
}
export interface Provider {
  id: 'anthropic' | 'google' | 'openai' | 'mistral' | 'local'
  display_name: string
  models: Model[]
  regions?: Region[]      // solo Gemini Vertex
}
export interface Model {
  id: string
  display_name: string
  capabilities: { vision: boolean; tools: boolean; streaming: boolean; prompt_cache: boolean; thinking?: boolean; reasoning?: boolean }
}
export interface Region {
  id: string
  display_name: string
  city?: string
}
```

`LLMProviderCard.svelte` rende:
- Toggle "Provider attivo" (radio cross-card, persistito in `llm-settings.active_provider`).
- Dropdown modello (filtrata sulle `capabilities` necessarie per main vs title vs tabular).
- Dropdown regione (solo Google; preview models force `global`).
- Input API key con `type="password"` e bottone "Mostra".
- Disabilita "Imposta come attivo" se API key è vuota (replica logica backend `commit e1e0814`).

### 15.3 MCP activity indicator ("spia") — decisione Q9

Il backend emette via SSE `tool_call_start` / `tool_call_end` (vedi §9.2). Lo store `mcp.svelte.ts` mantiene un set di `activeCalls: Map<call_id, { name, server, started_at }>`. Componenti:

- **`McpActivityDot.svelte`** — pallino animato (pulse arancio brand) accanto al `ModelSelector` in `ChatToolbar.svelte`. Visibile solo se `activeCalls.size > 0`. Tooltip: lista `<tool> · <server>` con tempo elapsed.
- **`McpActivityChip.svelte`** in `StatusBar.svelte` — chip persistente "MCP × N" cliccabile. Click apre popover con cronologia ultime 20 chiamate (timestamp, server, tool, durata, ok/err).

L'overhead di mantenere lo store è zero quando non ci sono call attive (Set vuoto). I dati live arrivano dallo stream chat senza fetch aggiuntivi.

### 15.4 MCP form semplificato — decisione Q9

`MCPServerCard.svelte`:
- Form principale: `Nome` (richiesto), `URL` (richiesto), `API Key` (opzionale), `Abilitato` (toggle).
- Sezione "Avanzate" collassata di default:
  - Transport esplicito (`auto` default | `http` | `sse` | `stdio`)
  - Custom headers (key/value list)
  - Env vars (per stdio)
  - Args (per stdio)
- Bottone "Verifica connessione" → `POST /user/mcp-servers/probe` → `MCPProbeResult.svelte` mostra: ✓ transport rilevato, tools/prompts/resources scoperti con conteggio + lista compatta.

---

## 16. RAG / corpora UX

### 16.1 Cartelle locali (`/sync`)
- Lista folders con `enabled`, `recursive`, `last_scan_at`, `project_id?`.
- Per ogni folder: button **Scansiona** (`POST /folders/{id}/scan`) + progress bar con poll `GET /folders/{id}/status` ogni 1s mentre `status === 'running'`.
- Drill-down "File indicizzati" → tabella paginata da `GET /folders/{id}/files`, con motivo skip per i non-indicizzati.

### 16.2 EUR-Lex (`/eurlex`)
- Card config: toggle enabled, dropdown lingua, checkbox fallback_en.
- Search: input CELEX/keyword → tabella hit (titolo, CELEX, url → `openExternal`).
- "Sync questo documento" su ogni hit → `POST /eurlex/fetch` (status `syncing` → `ready`/`interrupted`).
- Lista doc indicizzati con badge (chunks, last_synced) + bottone Resync su `interrupted`.
- Progress embedding (overlay barra in `StatusBar` quando `GET /eurlex/embed-progress` != null).

### 16.3 Italian Legal Corpus (`/italian-legal`)
- Card config: enabled + multi-select sources.
- Bottone **Importa bulk** → `POST /import` + polling `import-status` (shard X/Y, % avanzamento).
- Search filtri (sources, doc_types, anno min/max).
- Stessi controlli di fetch/list/delete/resync di EUR-Lex.

### 16.4 Corpora dichiarativi (`/corpora`)
- Lista plugin (DILA, CNIL, ecc.) da `GET /corpora` con icona "runnable" / "config-only".
- Per ognuno: stessi 5 controlli (search, fetch, list, delete, import bulk) ma generati genericamente da `CorpusCard.svelte` parametrizzato sul plugin.
- Import progress live via `GET /corpora/{id}/import-progress` (phase, message, current, total).

### 16.5 Embedding model banner
Quando `GET /sync/model-status` ritorna `downloading` o `loading`, mostra banner persistente in `StatusBar` con:
- File corrente (`file`), bytes scaricati / totali, percentuale.
- Tempo stimato (interpolazione velocità ultimi 5s).
- Su `failed`: error message + bottone "Riprova" (`POST /sync/folders/{id}/scan` o equivalente endpoint che forza re-init).

---

## 17. Error model & toasts

### 17.1 Shape uniforme backend
Tutti gli errori backend tornano `{ "detail": "msg" }` con status 4xx/5xx. `ApiError` (vedi §6.3) li normalizza.

### 17.2 Status → toast / azione

| Status | Azione UI |
|---|---|
| 0 (network) | Toast danger "Backend non raggiungibile" + check `/healthz` |
| 401 | Wipe token + router → Unlock |
| 403 | Toast danger "Accesso negato" |
| 404 | Inline empty state nel componente |
| 409 | Toast warning con detail (es. "Username già in uso") |
| 413 | Toast warning "File troppo grande (max 100 MB)" |
| 422 | Inline validation accanto al campo |
| 429 | Countdown da `Retry-After` |
| 500 | Toast danger "Errore interno (vedi log)" + `console.error` |
| 501 | Toast info "Funzione non disponibile su questa piattaforma" |
| 503 | Toast warning "Servizio non pronto (modello in caricamento?)" |

### 17.3 Toast region
Coda LRU di max 5 toast (vecchi auto-dismiss dopo 6s, danger sticky finché chiusi).

---

## 18. Piano di migrazione a fasi

### Fase 0 — Setup (1-2 giorni)
- [x] **Rename** `MikeRust/frontend/` → `MikeRust/frontendMike/` con `git mv` (preserva la history)
- [x] Creare `src-tauri/tauri.legacy.conf.json` che punta a `frontendMike/out` (porta 3000)
- [x] Aggiornare `src-tauri/tauri.conf.json` perché punti a `frontend/dist` (porta 5173) — diventerà il default
- [x] Aggiungere wrapper script (`dev` / `dev:legacy` / `build` / `build:legacy`) in `package.json` di MikeRust root (creare se assente)
- [ ] Scaffolding nuovo `frontend/` con `pnpm create tauri-app@latest . -- --template svelte-ts --manager pnpm` (eseguito **da dentro `frontend/` vuota**, senza ri-generare il `src-tauri`)
- [ ] Configurare Tailwind CSS v4 con `@tailwindcss/vite`
- [ ] Configurare `tsconfig.json` strict, `eslint`, `prettier`
- [ ] Aggiungere `LICENSE` (vedi §20), `NOTICE`, `frontend/README.md` che attesta: data inizio, stack, assenza codice Mike/frontendMike, lista dipendenze
- [ ] CI di base (lint + typecheck + test) su PR
- [ ] Verifica switch funzionante: `cargo tauri dev --config src-tauri/tauri.legacy.conf.json` lancia ancora il vecchio prodotto

### Fase 1 — Boot + Tauri integration (2-3 giorni)
- [ ] `tauri/commands.ts` (api_base_url, open_external_url)
- [ ] `api/client.ts` + `ApiError`
- [ ] `routes/Boot.svelte` (port discovery + /healthz + /auth/status)
- [ ] Aggiornare `src-tauri/tauri.conf.json` di MikeRust con nuovi `frontendDist`/`devUrl` (vedi §7.4)
- [ ] Verifica end-to-end: shell Tauri lancia Vite dev → frontend riceve URL backend → /healthz risponde

### Fase 2 — Design system primitivi (3-5 giorni)
- [ ] CSS token in `app.css` (incluso dark mode)
- [ ] Componenti UI: Button, IconButton, Input, Textarea, Select, Combobox, Modal, Sheet, Tabs, Badge, Toggle, Checkbox, Radio, Dropdown, Menu, ChipGroup, Avatar, Progress, Spinner, Tooltip, Toast, ConfirmDialog, EmptyState, ErrorBoundary, Pagination
- [ ] `Sidebar` + `SidebarItem` + `TopBar` + `StatusBar` + `AppShell`
- [ ] **Pagina dev interna** `/__playground` con tutti i componenti (riferimento visivo per QA)

### Fase 3 — Tipi TS + API layer + Auth (3-4 giorni)
- [ ] Tipi `src/lib/types/` (mirror dei serde Rust)
- [ ] Wrapper `src/lib/api/*` 1:1 con i mount di [src/lib.rs](src/lib.rs)
- [ ] `routes/Setup.svelte`, `routes/Unlock.svelte`, `BiometricPrompt.svelte`
- [ ] `auth.svelte.ts` + `user.svelte.ts` + `i18n.svelte.ts`
- [ ] Contract tests (fixtures JSON) per ogni endpoint critico

### Fase 4 — Store globali + i18n (2-3 giorni)
- [ ] Tutti gli store di §12.2
- [ ] 6 file locale (it canonica + en + fr + de + es + pt) — chiavi minime per Phase 5
- [ ] Vitest check parità chiavi

### Fase 5 — Route e schermate (10-15 giorni)

| Schermata | Giorni stimati |
|---|---|
| Layout root + Router + StatusBar | 1 |
| Assistente (chat + streaming SSE + tool-call card + download docx card + picker) | 3 |
| Progetti (lista + modal + export/import .mikeprj) | 2 |
| Revisioni tabellari (lista + modal + TanStack Table + add column) | 2 |
| Workflow (lista + modal + chip + nascondi/mostra preset) | 1.5 |
| Template DOCX (lista + filtri + describe + render) | 1 |
| Impostazioni — Profilo + Locale + Default Domain + Cambio PIN + Biometric | 1 |
| Impostazioni — Modelli LLM (4 card) | 1 |
| Impostazioni — MCP server + probe | 1 |
| Impostazioni — Documenti locali (sync folders) | 1 |
| Impostazioni — EUR-Lex | 1 |
| Impostazioni — Italian Legal | 0.5 |
| Impostazioni — Corpora generici | 1 |
| Impostazioni — Diagnostica + Elimina account | 0.5 |

### Fase 6 — Test e build (3-5 giorni)
- [ ] Test unitari Vitest per store e utils
- [ ] Test E2E Playwright per:
  - boot → setup → unlock
  - send message + streaming
  - upload documento + invio in chat
  - creazione workflow custom
  - render template DOCX
  - import + export progetto
  - sync folder + scan
  - probe MCP server
- [ ] Contract tests (Vitest) contro un backend MikeRust live in CI
- [ ] Build Windows x64 + arm64
- [ ] Build macOS arm64 + x64
- [ ] **Verifica assenza file Mike nel bundle finale**: `unzip -l mike-tauri.exe | grep -i mike-frontend` ⇒ 0 risultati

### Fase 7 — Audit licenze e legal (1 giorno)
- [ ] `pnpm dlx license-checker --onlyAllow 'MIT;ISC;Apache-2.0;BSD-2-Clause;BSD-3-Clause;0BSD;CC0-1.0;Unlicense'`
- [ ] Generare `NOTICE` file con elenco dipendenze
- [ ] Parere legale su scelta licenza finale e su clean-room rewrite (§§1 e 22)

### Fase 8 — Rimozione frontend legacy & cleanup (1 giorno)
- [ ] Dopo conferma parità feature in Fase 6-7, rimuovere `frontendMike/` in un singolo commit "remove AGPL legacy frontend"
- [ ] Eliminare `src-tauri/tauri.legacy.conf.json` e gli script `dev:legacy` / `build:legacy`
- [ ] Aggiornare `README.md`, `HISTORY.md`, `NOTICE.md` di MikeRust riflettendo il cambio di licenza frontend
- [ ] Smoke test runtime finale: `/healthz` ritorna i preset corretti, boot → unlock → assistant → invio messaggio → upload doc → render template → import .mikeprj → search EUR-Lex → scan folder
- [ ] Tag release `v1.0-svelte` sul commit di rimozione, come bookmark "primo bundle pulito da AGPL"

---

## 19. Inizializzazione progetto

```pwsh
# Sequenza completa Fase 0 (Windows / PowerShell).
# Prerequisito: il rename git mv frontend frontendMike è già stato eseguito (vedi script Fase 0).

# 1. Scaffolding dentro frontend/ (cartella vuota dopo il rename)
cd c:\Progetti\MikeRust
New-Item -ItemType Directory -Path frontend -Force | Out-Null
cd frontend
pnpm create tauri-app@latest . -- `
  --template svelte-ts `
  --manager pnpm `
  --identifier ai.semplifica.mikerust
# NB: lo scaffolder propone di generare anche un src-tauri/ — RIFIUTARLO
# (lo shell Tauri esiste già nel root del progetto MikeRust).

# 2. Pulire src-tauri/ generato per errore (se creato)
Remove-Item -Recurse -Force src-tauri -ErrorAction SilentlyContinue

# 3. Dipendenze Tailwind v4
pnpm add -D tailwindcss @tailwindcss/vite

# UI runtime
pnpm add lucide-svelte
pnpm add @tanstack/table-core
pnpm add svelte-virtual
pnpm add marked dompurify
pnpm add pdfjs-dist
pnpm add shiki
pnpm add @intlify/core-base

# 4. Tauri plugins (Q3 single-instance, Q10 stronghold, file picker)
pnpm add @tauri-apps/plugin-stronghold
pnpm add @tauri-apps/plugin-single-instance
pnpm add @tauri-apps/plugin-dialog
pnpm add @tauri-apps/plugin-window-state

# Dev / test
pnpm add -D vitest @vitest/ui
pnpm add -D @playwright/test
pnpm add -D license-checker
pnpm add -D @types/dompurify
pnpm add -D typescript@latest
pnpm add -D eslint prettier eslint-plugin-svelte
```

`vite.config.ts`:
```typescript
import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import tailwindcss from '@tailwindcss/vite'
import path from 'node:path'

export default defineConfig({
  plugins: [tailwindcss(), svelte()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  envPrefix: ['VITE_', 'TAURI_'],
  resolve: {
    alias: { $lib: path.resolve(__dirname, 'src/lib') },
  },
  build: {
    target: ['es2022', 'chrome120', 'safari17'],
    sourcemap: true,
  },
})
```

`package.json` (estratto degli script):
```jsonc
{
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "preview": "vite preview",
    "typecheck": "tsc --noEmit",
    "lint": "eslint src",
    "format": "prettier --write src",
    "test": "vitest run",
    "test:e2e": "playwright test",
    "license-audit": "license-checker --onlyAllow 'MIT;ISC;Apache-2.0;BSD-2-Clause;BSD-3-Clause;0BSD;CC0-1.0;Unlicense' --excludePrivatePackages --production"
  }
}
```

---

## 20. Scelta della licenza per il nuovo frontend

| Opzione | Pro | Contro | Adatta se |
|---|---|---|---|
| **MIT** | Massima adozione, semplicità | Nessuna protezione commerciale | Vuoi massimizzare contributi OSS |
| **Apache 2.0** | MIT + protezione brevetti | File NOTICE richiesto nei fork | Vuoi protezione brevetti |
| **BSL 1.1** | Source-available, auto-OSS dopo N anni, blocca competitor | Non OSS certificata | Vuoi proteggere uso commerciale ma mostrare il codice |
| **Dual AGPL + Commercial** | Modello open-core sostenibile | CLA richiesto, gestione complessa | Vuoi revenue dalla licenza |

**DECISIONE (Q7): AGPL-3.0 per la prima release.**

Motivazione: come **unico titolare del copyright** del nuovo codice clean-room (`frontend/` Svelte ex-novo), il proprietario può rilasciare la versione corrente sotto AGPL e **cambiare licenza in futuro** quando vorrà (Apache 2.0, MIT, BSL, dual commerciale, proprietaria). La switch si applica alle **release N+1**: le release già distribuite restano sotto AGPL per chi le ha ricevute. Schema dual-licensing tipo Qt, completamente legittimo grazie al sole-copyright-holder status.

Implicazioni operative:
- **Backend** (`src/`, `src-tauri/`) anch'esso AGPL-3.0 (coerenza, semplifica audit).
- File `LICENSE` AGPL-3.0 in root del repo e in `frontend/` (specifico).
- Header copyright in ogni file `.svelte`/`.ts` ex-novo: `// Copyright (c) 2026 <proprietario>. AGPL-3.0.`
- `NOTICE` elenca dipendenze open source con relative licenze (tutte permissive — vedi §2).

> Re-licensing futuro: si raccomanda di **firmare un CLA leggero** (anche solo "Author Agreement") con eventuali contributor futuri, così da preservare il diritto di re-licensing. Senza CLA, ogni contributor diventa co-titolare del proprio contributo e il re-licensing richiede il loro consenso unanime.

---

## 21. Regole anti-contaminazione (da applicare sempre)

1. **Nessun file copiato** dal repo Mike o da `frontend/` di MikeRust, nemmeno parzialmente
2. **Nessuna ispirazione strutturale dal codice** — guardare gli screenshot della UI è ok; guardare il sorgente del frontend (sia Mike che MikeRust attuale) **non lo è**
3. **Conoscenza features MikeRust solo via commit log** — `git log -- frontend/` per leggere i message dei commit MikeRust (descrivono *cosa* fa la feature), mai il diff
4. **Naming indipendente** — scegliere nomi di variabili e funzioni senza guardare nessun codice frontend
5. **Dipendenze diverse** dove possibile (es. Lucide al posto di qualsiasi icon set usato in Mike; Marked al posto di altri md parser)
6. **Git history pulita** — primo commit = scaffolding Tauri/Svelte, mai un fork/copy
7. **Header copyright** su ogni file `.svelte` e `.ts` con data di creazione e autore
8. **Documentare tutto** — ogni decisione architetturale nel README, con data, come prova di sviluppo indipendente
9. **Stringhe i18n** — il bundle `frontendMike/messages/*.json` è in larga parte opera MikeRust (770+ chiavi, vedi commit elencati in §14.1). **È riutilizzabile** dopo bonifica via `git blame`: si tengono le chiavi/righe introdotte in commit MikeRust, si **rifrasano** quelle ereditate da commit pre-fork (origine Mike).
10. **PR review checklist** include "questo PR contiene snippet dal frontend Mike/MikeRust?" — se sì, rifiutare

---

## 22. Checklist finale pre-distribuzione

- [ ] `pnpm license-audit` → zero licenze GPL/AGPL/LGPL
- [ ] `git log --all --source` su `mikerust-ui` → nessun commit con file da Mike o da `frontend/` di MikeRust
- [ ] Tutti i file hanno header copyright MikeRust con data ≥ inizio progetto
- [ ] `LICENSE` presente nella root
- [ ] `NOTICE` con elenco completo dipendenze open source + licenze
- [ ] Build Windows x64 testata su macchina pulita (no Node installato)
- [ ] Build Windows arm64 testata (verifica integrazione con ort load-dynamic + onnxruntime 1.24.2 DLL — vedi memoria persistente MikeRust)
- [ ] Build macOS arm64 testata su macchina pulita
- [ ] Parità feature con frontend attuale verificata con checklist tester (NB: lista test ex-novo, non basata sul codice attuale ma sugli screenshot)
- [ ] `/healthz` ritorna `presets: { workflows: ≥56, columns: ≥30, docx_templates: ≥9, model_providers: 4 }` (smoke check copertura MikeRust additions)
- [ ] Smoke E2E: setup → unlock → invio messaggio chat con streaming → upload PDF → creazione workflow custom → render template DOCX → import .mikeprj → scan folder → search EUR-Lex
- [ ] Parere legale ottenuto prima della distribuzione pubblica

---

## 23. Domande aperte / decisioni da confermare

> Punti dove la mia scelta è ragionata ma non ancora confermata dal product owner. Default proposto in **grassetto**; pronto a cambiare se preferisci diversamente.

1. **Repo strategy.** **DECISO**: in-place dentro `MikeRust/`. Vecchio frontend rinominato in `frontendMike/`, nuovo in `frontend/`, switch via doppio `tauri.conf.json` (vedi §7.4). Rimozione di `frontendMike/` in Fase 8 dopo parità feature.
2. **CSP.** **DECISO**: lasciare `csp: null` durante Fasi 0-5 per non bloccare HMR; **attivare CSP non-null in Fase 6** dopo verifica che Vite/Tailwind/marked non chiedano `'unsafe-eval'` in produzione. Policy raccomandata: `default-src 'self'; connect-src 'self' http://127.0.0.1:*; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; script-src 'self'`.
3. **Single-instance plugin.** **DECISO**: aggiungere `tauri-plugin-single-instance` in Fase 8 (~1 ora). Previene doppio launch (e doppio bind sulla porta 3001 → conflitto SQLite/embeddings).
4. **Updater.** **DECISO**: il **plugin `tauri-plugin-updater` è post-MVP**, ma lo **schema dell'endpoint si definisce subito** così il primo bundle è già pronto a riceverlo. Spec endpoint:
   - **URL canonico (TBD hosting):** `https://updates.mikerust.app/{target}/{current_version}` — il client invia `User-Agent: mikerust/<version> <os>-<arch>`.
   - **Response 204 No Content** = nessun aggiornamento. **Response 200** = nuovo update disponibile, JSON:
     ```jsonc
     {
       "version":      "0.2.0",
       "pub_date":     "2026-06-01T10:00:00Z",
       "notes":        "Markdown release notes",
       "platforms": {
         "windows-x86_64": {
           "signature": "dW50cnVzdGVkIGNvbW1lbnQ6IC...",   // base64 minisign
           "url":       "https://updates.mikerust.app/dl/0.2.0/mikerust_0.2.0_x64-setup.nsis.zip"
         },
         "windows-aarch64": { … },
         "darwin-aarch64":  { … },
         "darwin-x86_64":   { … }
       }
     }
     ```
   - **Firma binari:** `minisign` (chiave pubblica bundlata nel `tauri.conf.json` sotto `plugins.updater.pubkey` quando il plugin sarà attivato). Generare la coppia ora: `cargo tauri signer generate -w ~/.tauri/mikerust.key`.
   - **Hosting:** S3 + CloudFront, o GitHub Releases come fallback economico (URL `https://github.com/.../releases/download/...`).
   - **Telemetria check:** opt-in in Settings → "Controlla aggiornamenti all'avvio" (default off finché endpoint non è live).
5. **PDF viewer.** **DECISO**: **`pdfjs-dist`** (Apache-2.0, ~1 MB) wrappato in `components/documents/PdfViewer.svelte`. Più controllo dell'iframe Chromium (toolbar custom, ricerca testo coerente, zoom binding, dark mode). Caricato lazy solo quando l'utente apre il viewer.
6. **Streaming chat — UX target.** **DECISO**: `fetch` + `ReadableStream` (vincolato dall'header `Authorization`). Pattern UX completo definito in §9.5 "Streaming UX spec" — bullet riassuntivi:
   - Token visibile in tempo reale, smoothed con `requestAnimationFrame` (no janky redraw a ogni delta)
   - **Cursore "writing"** lampeggiante al fondo del testo durante streaming
   - **Auto-scroll** alla fine, **disattivato se l'utente scrolla manualmente** verso l'alto (sticky position detector); badge "↓ Continua a leggere" quando ci sono nuovi token sotto la viewport
   - **Stop button** sempre visibile durante streaming → `AbortController.abort()` + flush stato parziale
   - **Regenerate** sul messaggio assistant dopo `done` → riapre stream con stesso input
   - **Tool-call card** inline (`ToolCallCard.svelte`) con stato `pending → executing → result/error` e collapse argomenti lunghi
   - **Phase indicator** sotto al messaggio (retrieving / thinking / generating)
   - **Error inline** (`error` event) → banner rosso con bottoni "Riprova" / "Cambia provider" (link a settings se `key missing`)
   - **Heartbeat watchdog** lato client: >90 s senza eventi → mostra "Connessione lenta?" + offer "Riprova"
7. **License finale.** **DECISO**: **AGPL-3.0 per la prima release.** Come sole copyright holder, il proprietario può rilasciare versioni future sotto qualsiasi licenza (Apache 2.0, MIT, BSL, commerciale, dual). Vincolo: le release **già distribuite** restano sotto AGPL per chi le ha ricevute — il cambio si applica solo da release N+1. Strategia: AGPL ora, valutare passaggio a Apache 2.0 / BSL quando si vorrà permettere uso commerciale chiuso da parte di terzi.
8. **Locale canonica.** **DECISO**: **inglese canonica**, le altre 5 lingue sono traduzioni (it/fr/de/es/pt). **Riuso del bundle i18n MikeRust** (770+ chiavi, commit `0b575ca`, `f78c8bb`, `94ed69f`, `407c296`, `16c8308`, `3faa20d`): è **opera originale del proprietario di MikeRust**, copyright proprio → riutilizzabile integralmente. Vedi §1 e §14.
9. **MCP UX.** **DECISO**: il form server MCP mostra **solo URL + API key + nome** al primo livello; transport rilevato automaticamente via `/user/mcp-servers/probe`. Override esplicito (`http`/`sse`/`stdio`) in una sezione "Avanzate" collassata.
   - **Activity indicator ("spia" MCP)**: badge persistente in `StatusBar.svelte` (e mini-icona accanto al `ModelSelector` durante una chat) che pulsa quando un tool MCP è in volo. Tooltip mostra "Sto chiamando `<tool>` su `<server>`" con conteggio aggregato. Sorgente eventi: SSE chat (`tool_call_start` / `tool_call_end`).
10. **Token storage.** **DECISO**: token vive **in memoria** durante la sessione (volatile, immune da XSS-su-localStorage). **Persistenza opt-in** in Settings → "Mantieni accesso fra riavvii": al toggle ON, il token è salvato in **`tauri-plugin-stronghold`** (cifratura at-rest con master-password derivata dal PIN/biometrico). Al successivo avvio, sblocca con PIN/biometrico → decifra → restore in memoria. **Niente `localStorage`** (coerente con regola persistente "prefer data/storage over localStorage"). Nessun `sessionStorage` neanche, per evitare leak XSS.

---

*MikeRust UI Rewrite Plan v2.1 — 2026-05-15 — decisioni Q1-Q10 congelate, pronto per Fase 0*
