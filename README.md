# MikeRust

Sovereign local AI document assistant — Rust+axum backend, SQLite, local filesystem storage, ONNX-based embeddings, Tauri shell, Next.js frontend (forked from [`willchen96/mike`][upstream] upstream).

Designed to run entirely on the user's machine: no cloud database, no external auth provider, no S3 bucket. Optional LLM API keys are stored locally and never leave the box except to call the model provider the user explicitly configured.

Maintained by **Semplifica s.r.l.** — [semplifica.ai](https://semplifica.ai). The code is AGPL-3.0; the Semplifica wordmark and logo are trademarks, see [NOTICE.md](NOTICE.md) for the brand-vs-code separation.

### Built in the open — please contribute

MikeRust is meant to be a **collaborative** project. Fixes, new corpus
plugins, translations, jurisdiction-specific feedback, design ideas,
half-formed proposals — all welcome, and a small idea filed as an issue
tends to land faster than a full feature held back until it's "ready".
You don't have to write the patch yourself: open an
[issue](https://github.com/SemplificaAI/MikeRust/issues) to discuss a
direction, send a pull request when you have something concrete, or
reach out at [git@semplifica.ai](mailto:git@semplifica.ai)
if a public thread is the wrong place.

## Lineage

MikeRust derives from the open-source **Mike** project by Will Chen
([`willchen96/mike`][upstream]) — an AGPL-3.0 AI legal assistant with
a TypeScript / Express / Supabase / S3 / LibreOffice stack. We keep
the frontend largely intact (UI, chat panel, citations, document
viewer) and replace the backend with a Rust+axum implementation that:

  * uses SQLite (via [`sqlite-vec`](https://github.com/asg017/sqlite-vec))
    instead of Supabase + pgvector;
  * embeds locally with ONNX (multilingual-e5-base via fastembed +
    optional DirectML / QNN execution providers);
  * extracts PDF / DOCX / RTF / XLSX in pure Rust — no LibreOffice
    process spawn;
  * ships as a Tauri desktop app with no server-side dependency.

For the original cloud-native upstream, see
[github.com/willchen96/mike][upstream]. For a different sister fork
specialised on Danish law, see
[github.com/marklok/danishmike](https://github.com/marklok/danishmike).

[upstream]: https://github.com/willchen96/mike

## Interface

A desktop window (Tauri) wrapping the Next.js frontend; the embedded axum
backend runs in the same process. Two views to set expectations:

### Chat with citations and inline document viewer

![Chat answer with [g7] citation pill open in the PDF viewer, the cited passage highlighted on the page](docs/images/chat-with-citations.png)

Numeric citation pills (`[1]`, `[2]`, …) and `[gN]`/`[pN]` KB tags both open
the source document in the side panel. PDF.js text-search highlights the
exact quote the model cited. Re-opening a chat re-renders all pills from
persisted annotations — no stale `[Page N]` contamination thanks to a
sanitisation pass on both the write and read path.

### Authoritative-corpus sync (EUR-Lex shown)

![EUR-Lex search panel: GDPR result with 'Sync...' button, indexed list with live 48/288 chunk-embed progress bar](docs/images/eurlex-sync.png)

Search by CELEX, ELI, year/number, or free-text keywords; the panel
auto-detects intent, probes EUR-Lex across all act types, and confirms
which actually exist in the requested language. Indexed rows show their
chunk count, status badge, and a live embedding progress bar driven by
`/eurlex/embed-progress` polling. Same shape for the Italian Legal
Corpus panel.

### Internationalisation

The UI is fully wired for i18n via
[`next-intl`](https://next-intl.dev/) — every user-facing string lives in
[`frontend/messages/`](frontend/messages/). Currently shipped: **Italian
(`it.json`), English (`en.json`), French (`fr.json`)**. Adding a locale is
one translation file plus an entry in the locale picker.

> ⚠️ Development and screenshots use the UI in **Italian** — that's the
> source-of-truth surface for visual review and copy iteration. The English
> and French locales are kept current but typically lag by one or two
> iterations on brand-new strings. Contributors adding UI: add the IT key
> first, then EN and FR. **Never hardcode user-facing strings**, always go
> through a `useTranslations` namespace key.

## Quick start

```bash
# 1. pdfium (PDF extraction)
# Download from https://github.com/bblanchon/pdfium-binaries/releases
# Place pdfium.dll / libpdfium.so / libpdfium.dylib in libs/pdfium/

# 2. Backend env
cp .env.example .env
# Edit .env: set JWT_SECRET. STORAGE_PATH, DATABASE_URL etc. have sensible defaults.

# 3. Run dev (Tauri shell + axum backend + Next.js frontend)
cd src-tauri && cargo tauri dev

# Or backend only:
cargo run --features rag
```

The first run will:
- create `data/db/mike.db` (SQLite) and apply all migrations
- create `data/storage/` (uploads, chat cache)
- download `multilingual-e5-base` ONNX weights (~280 MB) into `%USERPROFILE%/mikerust-data/fastembed/` on first scan / first chat with attachments

## Architecture

```
Browser / Tauri webview (Next.js :3000)
       │  HTTP + SSE
       ▼
axum backend (127.0.0.1:<random>)   ← OS-assigned high port; the Tauri
                                      shell publishes it to the frontend
                                      via the `api_base_url` invoke
                                      command at boot.
                                      Override with PORT=3001 for the
                                      standalone-backend dev story.
   ├── SQLite          mike.db          (schema, vector store, settings)
   ├── sqlite-vec      doc_chunks       (768-dim embeddings, partition-keyed)
   ├── fastembed/ort   multilingual-e5-base ONNX  (CPU / DirectML / QNN)
   ├── pdfium-render   PDF text extraction + page rendering
   ├── quick-xml+zip   DOCX extraction (incl. redline detection — see below)
   ├── rtf-parser      RTF text extraction
   ├── calamine        XLSX/XLS/XLSB/ODS extraction
   ├── Local storage   ./data/storage/{documents,cache}
   ├── LLM             Anthropic / Gemini / OpenAI / vLLM / Ollama
   └── MCP             any HTTP/SSE MCP server, including localhost
```

## Key features

### RAG: local folder sync
Configure folders under **Impostazioni → Documenti locali**. The scanner walks the tree (honouring `.gitignore`-style patterns), extracts text per format, chunks at ~800 tokens with 200-token overlap, and embeds with `multilingual-e5-base` (768 dims). Embeddings live in `sqlite-vec` virtual tables in the same `mike.db`. Search queries use cosine over the partitioned vector index; partitions are keyed by `(user_id, project_id_or_global)` so cross-tenant retrieval is impossible.

Supported formats:
- **PDF** — pdfium-render native text. Per-page extraction; pages stamped with `[Page N]` markers so chunks can carry locality metadata. Scanned PDFs (no embedded text) are skipped unless a vision LLM is configured.
- **DOCX** — pure Rust ZIP+XML. Detects tracked deletions (`<w:del>`) and strike-through formatting (`<w:strike/>`/`<w:dstrike/>`); both are surfaced inline as `[removed by author: …]` markers. See [docs/DOCX.md](docs/DOCX.md).
- **RTF** — `rtf-parser` body text (control words, font tables, pictures, fields stripped).
- **XLSX / XLS / XLSB / ODS** — `calamine` per-sheet flattening.
- **TXT / MD / CSV** — UTF-8 lossy decode.
- **Images / scanned PDFs** — surfaced via the chat composer's vision path when the selected model is vision-capable; not indexed for RAG.

### RAG: hardware acceleration
ONNX Runtime execution providers compiled in via opt-in features:

```bash
cargo build --features rag-directml   # Windows GPU (DX12 device, no extra SDK)
cargo build --features rag-qnn        # Qualcomm Snapdragon NPU (X Elite / 8 Gen 3)
```

The service tries QNN → DirectML → CPU, silently skipping providers whose DLLs aren't loadable.

### Chat with attachments — hash-keyed cache
Documents attached via the chat composer (the **+** button) land in `data/storage/cache/` keyed by SHA-256 of the binary, and are pre-extracted to plain text at upload time:

```
data/storage/cache/<hash>.<ext>     # original file
data/storage/cache/<hash>.txt       # extracted plain text
```

Effects:
- **Dedup across chats** — the same file uploaded in different chats reuses the same on-disk pair (multiple `documents` rows reference one hash).
- **No filename collisions** — two different chats can both have a file named `contratto.pdf`; the hash determines the path, not the user-facing filename.
- **Auto re-extract on edit** — modifying a docx changes its hash, so the next upload generates a fresh `.txt` instead of stale text.
- **Chat-delete cleanup** — when a chat is deleted, the backend ref-counts each `content_hash` of its linked docs and removes the on-disk binary + text only when no other doc still references that hash.

See [docs/CACHE.md](docs/CACHE.md) for the storage contract and migration history.

### Citations
Assistant responses inline numeric markers (`[1]`, `[2]`) plus a trailing `<CITATIONS>` JSON block. The frontend parses both:
- **Per-marker pills** — `AssistantMessage.tsx::preprocessCitations` maps each `[n]` to the matching annotation by `ref` (numeric) or `doc_id` (alphanumeric `[g1]`/`[p1]` from KB hits).
- **DocPanel jump** — clicking a pill opens the cited doc in the in-app viewer, scrolling to the cited page (PDFs) or section (DOCX).

Annotations are persisted on `messages.annotations` (migration 0012) so re-opening a chat re-renders all pills correctly. Page-marker contamination (`[Page N]` leaking into citation quotes) is sanitised on both write and read paths so PDF.js text-layer highlights work.

When the model skips the `<CITATIONS>` block (some providers do), `synthesise_kb_citations_from_markers` rebuilds citations from RAG hits keyed by `(g|p)<n>` tags in the response.

### Local-folder sync
**Impostazioni → Documenti locali** (formerly "Sincronizzazione"). Add a folder, optionally scope to a project, hit *Scansiona ora*. Scanner emits per-file progress over the `/sync/folders/:id/status` endpoint with coarse pipeline stages (`extracting`, `embedding`) so the user sees motion during the slow first PDF.

The embedding model state — including the one-shot ~280 MB download — is reported via `/sync/model-status`; the UI renders an amber progress bar above the folder list while in `downloading` or `loading` state.

### Authoritative legal corpora — plugin system

**The intent.** The medium-term goal for this project is a **plugin
system for downloading legal documents locally**: a contributor (or the
user) describes a new public source — Légifrance, BOE, Bundesgesetzblatt,
a regional bulletin — in a small JSON manifest, and MikeRust handles the
rest (sidebar entry, importer, bulk-snapshot ingestion, search, fetch,
embed). No Rust patch, no rebuild, no per-corpus bespoke UI. The user
keeps a fully offline mirror of the parts of public law they care about,
under their own AGPL-licensed copy of MikeRust.

The first implementation lives in [`config/corpora-plugins/`](config/corpora-plugins/)
and is documented in [docs/CORPUS_PLUGINS.md](docs/CORPUS_PLUGINS.md).
Today three strategies are supported:

- `builtin` — corpus served by a hand-written Rust adapter (EUR-Lex,
  Italian Legal). The manifest only carries metadata (display name,
  language list, license attribution).
- `dila-bulk-xml` — fully declarative: point at a DILA OPENDATA archive
  index URL, pick a *fonds* (CNIL / LEGI / JORF / CASS / KALI), and the
  generic importer takes care of download → tar walk → XML parse →
  `corpus_documents` insert → FTS5 index. **Proof of concept: CNIL**,
  ~26 000 délibérations indexed locally from an ~18 MB tar.gz, Etalab 2.0
  license, zero anti-bot exposure.
- `http-fetch-per-id` — scaffolded for single-document fetch with URL
  templates + CSS/JSONPath extraction (not yet exercised; first target
  was CNIL via Légifrance, abandoned because of Cloudflare).

**Alternative under evaluation — MCP-driven backend.** A second design
is on the table: instead of (or alongside) the JSON-plugin path, expose
legal-source ingestion as an **MCP backend**. Each public source becomes
an MCP server with tools like `search`, `fetch`, `bulk_import`,
`list_indexed`; MikeRust calls those tools the same way it already calls
any other MCP server. The trade-off is roughly:

| | JSON plugins (current) | MCP backend (evaluating) |
|---|---|---|
| Author-time cost | one JSON file | one MCP server (Rust/Python/TS) |
| Run-time cost | in-process, zero extra deps | extra long-lived process |
| Reach | bounded by the manifest schema | unbounded — arbitrary code |
| Sovereignty | data stays in `data/db/mike.db` | depends on the server's policy |
| Reuse outside MikeRust | none | usable from Claude / any MCP host |

Neither path forecloses the other. The JSON plugin is shipping now
because it's the smallest possible footprint; the MCP backend is being
weighed for the connectors that can't be expressed declaratively
(arbitrary auth flows, multi-step session protocols, jurisdiction-
specific quirks like Légifrance's PISTE OAuth or the Bundesanzeiger's
TOC-then-ZIP pattern). Feedback welcome — pick your favourite source and
tell us which path would be less painful for it.

| Corpus | Strategy | Languages | Status |
|---|---|---|---|
| **EUR-Lex** (EU) | builtin (REST/SOAP + SPARQL + Cellar) | 24 EU languages | ✅ V1 — CELEX fetch via public HTML, EN fallback |
| **Italia legale** (HF dataset) | builtin (HF datasets-server + Parquet bulk) | Italian | ✅ V1 — Normattiva (~69K) + Corte Costituzionale (~22K) |
| **CNIL** (France, via DILA OPENDATA) | `dila-bulk-xml` plugin | French | ✅ V1 — délibérations + recommandations + avis (~26K docs, Etalab 2.0) |
| Italian: OpenGA (TAR + Consiglio di Stato) | Same HF dataset, source filter | Italian | 🔲 in dataset, opt-in mancante |
| Italian: Cassazione (civile/penale/sez. unite) | da identificare | Italian | 🔲 V2 — sorgente fuori dataset HF |
| Italian: Normattiva post-snapshot live | URN single-fetch | Italian | 🔲 V2 — atti dopo 2026-03-01 |
| Italian: Leggi regionali (20 BUR) | per-regione | Italian | 🔲 V3 |
| Italian: Gazzetta Ufficiale (sumario quotidiano) | XML feed | Italian | 🔲 V3 |
| Italian: Decreti ministeriali / circolari | per-ministero | Italian | 🔲 V3 (import da URL) |
| French: LEGI / JORF / CASS / KALI (DILA bulk) | `dila-bulk-xml` plugin | French | 🔲 — same strategy as CNIL; one manifest each |
| **Légifrance** (France, via PISTE) | candidate for MCP backend | French | 🔲 OAuth2 REST — JSON plugin or MCP TBD |
| **Retsinformation** (Denmark) | JSON `/api/document/{eli}` + `/api/search` | Danish | planned |
| **BOE** (Spain) | Open Data API + daily XML sumarios | Spanish | planned |
| **Gesetze im Internet** (Germany) | TOC XML → per-law ZIP | German | scraping-only |
| **Normattiva** (Italy, direct) | none — HTML / Akoma Ntoso URN deep links | Italian | sostituito dal connettore HF; resta utile come V2 live-fetch |

### Sovereign data
Everything that contains user data lives under the workspace:
- `data/db/mike.db` — schema, embeddings, settings, all chats, all documents metadata
- `data/storage/` — uploads (`documents/`), chat cache (`cache/`)
- `%USERPROFILE%/mikerust-data/fastembed/` — ONNX weights (out-of-tree to avoid the Tauri watcher)
- `data/.mikeprj` envelopes — AES-256-GCM-encrypted project bundles, key derived via Argon2id from a recipient email

No telemetry, no remote logging, no anonymous metrics. Outbound traffic only when the user explicitly invokes a remote LLM (Anthropic / Gemini / OpenAI) or a remote MCP server they configured themselves.

## Environment variables

See `.env.example` for the full reference.

| Variable | Required | Default |
|---|---|---|
| `JWT_SECRET` | **yes** | — |
| `DATABASE_URL` | no | `sqlite://data/db/mike.db` |
| `STORAGE_PATH` | no | `./data/storage` |
| `FASTEMBED_CACHE_DIR` | no | `%USERPROFILE%/mikerust-data/fastembed` |
| `PDFIUM_DYNAMIC_LIB_PATH` | no | walks ancestors of cwd / exe for `libs/pdfium/` |
| `PORT` | no | `0` (OS picks a free high port — see Architecture) |
| `VLLM_BASE_URL` | for local LLM | — |
| `VLLM_API_KEY` | no | `local` |
| `ANTHROPIC_API_KEY` | for Claude | — |
| `GEMINI_API_KEY` | for Gemini | — |
| `MCP_SERVERS` | no | `[]` |

## Implementation status

| Area | Status |
|---|---|
| Auth (PIN/Argon2id + Windows Hello biometric + opaque sessions) | ✅ |
| SQLite + migrations (0001 → 0014) | ✅ |
| Local storage (filesystem) + S3 trait | ✅ filesystem ; 🔲 S3 |
| PDF extraction (pdfium) + scanned-PDF detection | ✅ |
| DOCX extraction with redline detection | ✅ |
| RTF / XLSX / TXT / MD / CSV extraction | ✅ |
| RAG: scanner, chunker, sqlite-vec, fastembed CPU | ✅ |
| RAG: DirectML / QNN execution providers | ✅ opt-in |
| LLM: Anthropic / Gemini / OpenAI / vLLM / Ollama | ✅ |
| MCP client (HTTP/SSE) — synchronous tools | ✅ |
| MCP client — multi-step async flows (request → poll → fetch) | ⚠️ partial — see note below |
| Routes: auth, user, chat, documents, projects, workflows, sync, tabular-review | ✅ |
| Project: documents (PDF/DOCX/RTF/XLSX) + folders + versions + rename | ✅ |
| Project: chats list, tabular-reviews list, owner/shared visibility | ✅ |
| Project: URL `?tab=` deep-linking (documents / assistant / reviews) | ✅ |
| Project: `.mikeprj` AES-256-GCM export + import | ✅ |
| Chat citations with persistence | ✅ |
| Chat-attachment hash cache + ref-counted cleanup | ✅ |
| Authoritative-corpus framework (`LegalCorpusAdapter` trait) | ✅ |
| EUR-Lex V1 (CELEX-based fetch + 24-language picker + EN fallback) | ✅ |
| EUR-Lex V2 (full-text search via SOAP CWS) | 🔲 [registration required](docs/EURLEX_REGISTRATION.md) |
| Italia legale V1 (Normattiva + Corte Cost via HF dataset) | ✅ |
| Italia legale V2 (OpenGA opt-in, Cassazione, live Normattiva) | 🔲 see [CORPORA.md](docs/CORPORA.md) |
| Italia legale V3 (regional laws, GU, ministerial decrees) | 🔲 |
| **JSON-manifest plugin system** (`config/corpora-plugins/*.json`) | ✅ schema + loader + adapter registry + generic /corpora routes |
| **`dila-bulk-xml` strategy** (download tar.gz → walk XML → FTS5) | ✅ end-to-end test + live import |
| CNIL via DILA OPENDATA (declarative plugin) | ✅ — first proof-of-concept consumer of the plugin system |
| Other DILA fondi (LEGI, JORF, CASS, KALI) as plugins | 🔲 — same strategy, one manifest each |
| MCP-backend alternative for ingestion (Légifrance / Bundesanzeiger / …) | 🔲 design phase — see "Authoritative legal corpora" |
| Other corpus ingestors (Retsinformation, BOE, …) | 🔲 planned |
| **Professional-domain column** across workflows / tabular_reviews / projects / documents (migration 0018) | ✅ 9 canonical domains, validated at API boundary, filter chips in list views |
| **Per-user `default_domain`** preference (migration 0019, Account → Generali UI) | ✅ pre-selects in every create / picker modal |
| **JSON-driven workflow & column-preset registries** (`config/workflow-presets/` + `config/column-presets/`) | ✅ in-memory loaders, no DB seed, merged into `/workflow` list; replaces the legacy TS constants |
| Built-in workflows shipped (legal vertical) | ✅ 14 — Generate CP Checklist, NDA Review, SPA Review, Credit Agreement Review/Summary, Commercial Agreement/Lease/Supply Review, LPA, Shareholder Agreement Review/Summary, Change of Control Review, E-Discovery Review, Employment Agreement Review |
| Built-in workflows shipped (insurance vertical) | ✅ 6 — 3 tabular comparison (RC Professionale, RC Prodotti, D&O, 24 cols each) + 3 assistant (Riassunto copertura, Due Diligence assicurativa, Inventario beni assicurati) |
| Insurance phase-2 workflows (Cyber / RC Generale / Property review / RC Medica / Key Man) | 🔲 see `docs/insurance-workflows-plan.md` |
| Column-preset shortcuts (auto-suggest column prompt+format from name match) | ✅ 13 legal + 17 insurance, domain-scoped auto-match |
| Picker modals with on-the-fly domain switch | ✅ workflow / tabular-template / column-preset pickers all expose a `DomainSelect` combo, pre-seeded with the user's default at every open |

### Note: MCP client — async multi-step flows

The MCP client successfully discovers servers and dispatches **synchronous
tool calls** (a tool that returns its real result on the same call). What
is **not yet reliably handled** is the multi-step async pattern that
human-in-the-loop MCP servers use — Edge / Semplifica.Edge being the
canonical case:

```
Mike → request_pseudonymized_documents       → {session_id, status:"pending"}
        (Edge waits for the human to approve in its GUI)
Mike →   get_pseudonymized_documents          → list of pseudonymised files
Mike →     count entities / download text     → next tools in the chain
```

Today Mike's auto-chain wrapper covers the immediate `request_*` →
`get_*` hop (with a 300 s timeout, configurable via
`MCP_CALL_TIMEOUT_SECS`), but a third-step or beyond — "list the
pseudonymised files, count their entities, download the pseudonymised
text" as separate tool invocations within the same conversational turn —
is not driven correctly by the dispatcher yet. The user has to nudge the
chat to perform each subsequent step manually.

Tracked as work-in-progress; do **not** modify `MIKE_SYSTEM_PROMPT` or
`build_mcp_system_prompt` when fixing this — the prompt structure is
preserved, only the dispatcher mechanics are in scope. See
[`src/routes/chat.rs`](src/routes/chat.rs) `dispatch_mcp_tool_with_async_chain`.

## Documentation

- [docs/MANUAL.md](docs/MANUAL.md) — operator manual (running, troubleshooting, recovery)
- [docs/DOCX.md](docs/DOCX.md) — DOCX extraction details (tracked changes, strikes, namespaces)
- [docs/CACHE.md](docs/CACHE.md) — chat-attachment cache layout + ref-counting
- [docs/CORPORA.md](docs/CORPORA.md) — EUR-Lex + national legal-corpora plan + API survey
- [docs/EURLEX_REGISTRATION.md](docs/EURLEX_REGISTRATION.md) — EUR-Lex V1 (no auth) + V2 SOAP registration steps
- [docs/SESSION_RECAP.md](docs/SESSION_RECAP.md) — historical session notes
- [docs/UPSTREAM_SYNC.md](docs/UPSTREAM_SYNC.md) — policy + audit log for syncing fixes from upstream `willchen96/mike`
- [docs/CORPUS_PLUGINS.md](docs/CORPUS_PLUGINS.md) — JSON-manifest plugin system for legal corpora (schema, strategies, add-a-corpus guide)
- [docs/WORKFLOWS.md](docs/WORKFLOWS.md) — user manual for Workflows, Tabular Reviews, and Assistant injection. Includes 7 non-legal examples (medical, finance, real estate, HR, insurance, IP, compliance) for designing your own templates.
- [HISTORY.md](HISTORY.md) — release notes / changelog, date-grouped
- For the pristine upstream README, see [`willchen96/mike`][upstream] directly

## License

Inherits AGPL-3.0 from the upstream [`willchen96/mike`][upstream] frontend. Backend (`src/`, `src-tauri/`) is original Rust and ships under the same license for consistency. See `LICENSE`.

**Brand assets are not AGPL.** The wordmark **Semplifica**, the corporate
name **Semplifica s.r.l.**, and the Semplifica logo shipped under
[`frontend/public/semplifica/`](frontend/public/semplifica/) are
trademarks of [Semplifica s.r.l.](https://semplifica.ai) and are reserved
separately from the code license. The **MikeRust** name is also reserved
as the identifier of this upstream (MikeRust ships without its own logo
for now — only the Semplifica mark is present in the UI). Forks with
substantive changes are asked to drop the Semplifica wordmark/logo and
rename the binary; see [NOTICE.md](NOTICE.md) for the full policy and
the precedent (GitLab CE, Mastodon, Nextcloud, Element, Plausible, …).
