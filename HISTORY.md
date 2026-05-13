# History

Release notes for MikeRust. No semantic-version tags yet — entries are
grouped by the date the work landed on `main` (Europe/Rome), most recent
first. Each entry follows a light Keep-a-Changelog shape (Added /
Changed / Fixed / Docs / Removed) so contributors can skim by intent.

Commits referenced are short SHAs; run `git log <sha>` for the full
diff. For the upstream-sync audit trail (which fixes were ported from
`willchen96/mike` and which we declined), see
[`docs/UPSTREAM_SYNC.md`](docs/UPSTREAM_SYNC.md).

---

## 2026-05-13 — Plugin system, French locale, project refactor, dynamic port, migration auto-heal

The big day for declarative ingestion. The medium-term goal of the
project ("download legal documents locally via a plugin manifest") goes
from prose to running code: a JSON schema, a generic loader, a generic
HTTP-driven and a generic DILA-bulk-XML strategy, and a first
proof-of-concept consumer — CNIL — that imports ~26 000 délibérations
from the French Open Data archive into a local FTS5 index in a single
click. The same day brings a project-page refactor mirrored from
upstream `willchen96/mike`, the French locale, a UX polish pass on the
new corpus panel, and in the afternoon two pieces of platform hardening:
the axum backend no longer binds to a hardcoded port, and sqlx
migration checksum drift now self-heals at startup instead of crashing
the app.

### Added — plugin system

- **JSON-manifest plugin schema v1** for legal corpora
  (`corpora-plugins/*.json`): each manifest declares `id`, `display_name`
  (with optional per-locale overrides), `homepage`, `languages`,
  `identifier_label`, `license`, and a `strategy` discriminator. Loader
  walks `corpora-plugins/` from the workspace root or the binary's
  ancestors so both `cargo run` and `cargo tauri dev` find the manifests
  ([`f777405`](#), [`dd1c3c2`](#)).
- **Schema v2** — adds explicit `capabilities` (`search`, `fetch`,
  `documents`, `documents_delete`, `documents_resync`, `embed_progress`,
  `bulk_import`, `user_config`) and a list of `sources` so a single
  corpus can ship multiple sub-toggles (e.g. CNIL → délibérations /
  recommandations / avis / guides pratiques). Sources are surfaced in
  the UI with available / coming-soon grouping ([`c123400`](#)).
- **`http-fetch-per-id` strategy** — declarative single-document fetcher
  driven by a URL template, with CSS / JSONPath extractors for body and
  metadata, automatic anti-bot detection (Cloudflare, AWS WAF, Akamai)
  and a structured `ManifestAdapter` registry on `AppState`
  ([`dcb5e43`](#), [`cfc38b9`](#)).
- **`dila-bulk-xml` strategy** — generic importer for DILA OPENDATA
  archives (CNIL / LEGI / JORF / CASS / KALI share the same XML schema):
  scrape the Apache directory index for the latest `Freemium_*` tarball,
  download, walk the tar in `tokio::task::spawn_blocking`, SAX-parse each
  XML entry (`quick-xml`), batch-insert into a new `corpus_documents`
  table backed by an FTS5 virtual table with `unicode61 +
  remove_diacritics`. Idempotent: skip the import if the same
  archive_ts is already on record ([`87f6907`](#), [`c388beb`](#)).
- **CNIL** — first plugin to exercise the bulk strategy end-to-end. ~18 MB
  archive, ~26 000 délibérations indexed locally, Etalab 2.0 licence
  attribution wired through to the UI footer. Zero anti-bot exposure
  (vs. the abandoned Légifrance attempt earlier the same morning).
- **Generic routes**: `/corpora`, `/corpora/:id`, `/corpora/:id/search`,
  `/fetch`, `/documents`, `/documents/:doc_id` (DELETE),
  `/import`, `/import-status`, `/import-progress`. Dispatch picks
  `dila-bulk-xml` short-circuit, `ManifestAdapter` registry, or 501
  with a hint for builtin corpora that still ride their dedicated
  routes ([`812638f`](#)).
- **`migrations/0017_corpus_documents.sql`** — composite-key table
  `(corpus_id, identifier)` plus FTS5 virtual table indexing
  `titre / titre_full / numero / body`, plus a per-corpus
  `corpus_imports` snapshot row so the UI can render
  "Snapshot from YYYY-MM-DD · N documents indexed".
- **End-to-end test** for the DILA bulk path: insert a real
  CNIL XML fixture into a fresh DB, run `search_local_index`, assert FTS5
  matches. Pinned a production bug: FTS5 `MATCH` clauses must reference
  the bare table name, not an alias ([`f21c2db`](#)).
- **Data-driven sidebar** — the Account → Documents & sources section
  now renders from `/corpora`; runnable corpora are clickable, declared-
  but-not-yet-wired corpora are dimmed with the manifest description as
  tooltip. New corpora appear in the sidebar the moment their JSON ships
  ([`a83d2a4`](#)).
- **Generic corpus settings page** (`/account/corpora/[id]`) — renders
  capabilities, sources, search box, bulk-import button + live progress
  bar, indexed-docs list with trash icon, license footer with deep link
  to the original source opened in the OS default browser ([`9c444b6`](#),
  [`f78c8bb`](#)).
- **Per-row indeterminate progress strip** under each search hit while
  its fetch is in flight, so the user sees motion during the
  multi-second embedding step ([`f78c8bb`](#)).
- **Concurrent syncs** — `syncing` state is now a `Set<string>` with
  functional `setState`, so multiple "Indicizza" clicks each keep their
  own spinner + strip and the backend `POST /corpora/:id/fetch` calls
  run in parallel ([`f78c8bb`](#)).
- **Re-indicizza state** — the index-button in the search list switches
  to "Re-indicizza" when the hit's identifier is already in the local
  index (cross-referenced via Set) ([`f78c8bb`](#)).

### Added — projects refactor (upstream sync)

- **`PATCH /project/:id/document/:docId/rename`** — port from upstream
  `willchen96/mike@f39f175`: rename a document inside a project, with
  filename uniqueness check and `updated_at` bump. Wired to a
  `RowActions` UI in the Documents tab ([`ca4073b`](#), [`0ea5161`](#)).
- Frontend `ProjectPage` decomposed into focused components — layout
  helpers + `DocVersionHistory` moved to `ProjectPageParts`,
  `ProjectPageSkeleton` extracted, Assistant tab and Reviews tab become
  their own files. Same behaviour, smaller diffs from now on
  ([`b60feda`](#), [`a967dab`](#), [`41ea283`](#)).

### Added — internationalisation

- **French locale** (`fr.json`) shipping with 658-key parity to `en` and
  `it`. Locale list goes from `["it", "en"]` to `["it", "en", "fr"]`;
  `LanguageSwitcher` picks up a `Français` button; backend
  `PUT /user/locale` whitelist accepts `"fr"`.
- New i18n namespace `Corpora` (with sub-namespace `Corpora.bulk`)
  feeds the generic corpus settings page; EUR-Lex page stragglers
  (`Sync…` / `Indicizzato` / `Indicizza`) are now translated through
  `useTranslations` too.

### Added — platform hardening (afternoon)

- **Dynamic port for the embedded axum server.** Default mode is now
  `port = 0` (OS picks a free high port); the actual port is reported
  back to the Tauri shell via a `tokio::oneshot` channel, stashed in
  `Arc<Mutex<Option<String>>>` managed state, and exposed to the
  frontend through a new `api_base_url` Tauri command. The frontend
  has a tiny `lib/apiBase.ts` module that calls `invoke("api_base_url")`
  once at boot and caches the result; all 22 existing `API_BASE`
  consumers (login, signup, AuthContext, UserProfileContext, account
  pages, components, hooks, the central `apiRequest` wrapper) now read
  through `apiBase()` instead of a build-time env constant. Pre-mount
  fetches (`AuthContext`, login probes) await `apiBaseReady` to avoid
  racing the IPC handshake. Outside Tauri (plain Next.js dev in a
  browser), discovery fails fast and falls back to
  `NEXT_PUBLIC_API_BASE_URL` ([`src/lib.rs`](src/lib.rs),
  [`src-tauri/src/lib.rs`](src-tauri/src/lib.rs),
  [`frontend/src/lib/apiBase.ts`](frontend/src/lib/apiBase.ts)).
  `.env.example` updated to leave `PORT` commented out by default.
- **Migration auto-heal at startup.** When sqlx detects checksum drift
  on a previously-applied migration (the dev edited a migration file
  in place after first run), `AppState::run_migrations` now catches the
  error, asks sqlx itself for the expected checksum of every bundled
  migration, rewrites the `_sqlx_migrations` rows in-process, and
  re-runs. Safe because every migration in the repo uses
  `CREATE … IF NOT EXISTS`. Self-contained — no external `sqlite3`
  binary, no manual `DELETE FROM _sqlx_migrations` SQL, no second
  helper executable. Surfaces `WARN [migrations] checksum drift
  detected — auto-healing` then `INFO [migrations] checksum drift
  healed; resume normal startup` in the log
  ([`src/db/mod.rs`](src/db/mod.rs)).

### Added — branding & docs

- **`NOTICE.md`** — trademark policy alongside AGPL-3.0: the Semplifica
  wordmark, corporate name, and logo are reserved trademarks of
  Semplifica s.r.l. Forks with substantive changes are asked to drop the
  marks and rename the binary. Same pattern as GitLab CE, Mastodon,
  Nextcloud, Element, Plausible ([`0709f6d`](#)).
- **README rewrite** of the "Authoritative legal corpora" section:
  reframed around the stated intent of a JSON-manifest plugin system,
  with an explicit trade-off table comparing it against an MCP-backend
  alternative that is under evaluation for connectors the manifest
  cannot express declaratively (PISTE OAuth, multi-step approval flows,
  etc.). Maintainer attribution to Semplifica s.r.l. + a collaboration
  invite (GitHub issues / PRs / email) added near the top
  ([`42ad45f`](#)).
- **`docs/CORPUS_PLUGINS.md`** — full schema reference and "how to add
  a corpus" walk-through ([`f777405`](#)).
- **`docs/UPSTREAM_SYNC.md`** — policy + audit log for the periodic
  upstream-sync passes against `willchen96/mike` ([`74eab2b`](#),
  [`c94199e`](#), [`606cd50`](#)).
- **`docs/WORKFLOWS.md`** — end-user manual for Workflows, Tabular
  Reviews, and Assistant injection. Twelve sections covering the
  mental model, column-format reference (9 formats), prompt-writing
  guide, step-by-step authoring flow, and 7 worked examples *outside*
  the legal domain (medical records review, M&A IC memo, real-estate
  inspection, HR resume screening, insurance claims triage, patent
  landscape, compliance audit register). Built-ins remain legal-focused
  but the framework is domain-agnostic.
- **`HISTORY.md`** — release-notes index introduced; this very file.

### Changed

- Sidebar user-pill dropdown now carries a Semplifica brand link to
  `https://semplifica.ai`; the link opens the user's OS default browser
  via Tauri's `open_external_url` command (avoids navigating the
  in-app WebView). Logo image dropped per design feedback; the asset
  stays under `frontend/public/semplifica/` for future use
  ([`e94c178`](#), [`0709f6d`](#)).
- Bulk-import progress bar no longer renders 100% green when the
  snapshot is already current (backend emits `phase=done` /
  `total=0`); the green text line stays but the misleading "26K docs
  indexed now" bar is hidden ([`f78c8bb`](#)).

### Fixed

- `ProjectsOverview` fetch is now gated on auth state and gracefully
  surfaces load errors instead of throwing ([`e9f4f4a`](#)).
- `generic_search` short-circuits to `dila-bulk-xml` *before* the
  `corpus_adapters` registry lookup, so DILA-backed corpora actually
  serve queries instead of 501-ing because they have no `ManifestAdapter`
  ([`812638f`](#)).
- CNIL routing heuristic: prefer search-first endpoints over deep-link
  identifier fetches (CNIL/Légifrance HTML lookups returned anti-bot
  pages on direct URN access); detector now scans Cloudflare / AWS WAF
  / Akamai signatures and surfaces a descriptive error
  ([`3a59276`](#), [`cfc38b9`](#)).
- DILA `tar::Archive` walks are wrapped in `tokio::task::spawn_blocking`
  so the iterator's non-`Send` state doesn't trip an `.await` in the
  importer ([`c388beb`](#)).

### Removed

- `frontend/src/app/components/projects/ProjectPage.tsx` shrunk by ~600
  lines as layout helpers, `DocVersionHistory`, the skeleton and the
  Assistant / Reviews tabs moved to their own files (see Refactor above).

---

## 2026-05-09 — EUR-Lex robustness, MCP async hardening, UX docs

Stabilisation day on top of the initial bootstrap: the EUR-Lex
connector learns to detect partial responses, the MCP client gets a
visible "waiting on server" indicator for slow human-in-the-loop tools,
and the README gets its first screenshot pair.

### Added — MCP

- **Auto-chain `request_*` → `get_*`** for the Edge / Semplifica.Edge
  async pattern: when an MCP tool returns `{status: "pending",
  session_id: ...}`, the dispatcher polls the matching `get_*` tool
  until completion or timeout, transparently to the model
  ([`2f83a7a`](#)).
- **5-minute dispatch timeout** for MCP tool calls (was the
  Tower default), configurable via `MCP_CALL_TIMEOUT_SECS`. Short
  result bodies are logged at info level for diagnosis ([`a6b78d5`](#)).
- **Per-user MCP discovery cache** so the tool catalogue isn't
  re-fetched on every chat message; cache invalidates on MCP-config
  change. Silent tool-injection failures now log the exact reason
  ([`5d526f8`](#)).
- **Phase-by-phase dispatch logging** + SSE heartbeats so the chat
  channel doesn't look dead during a long MCP roundtrip
  ([`813a8e4`](#)).
- **Live wait indicator** in the chat composer — when a tool call is in
  flight on the MCP server side (e.g. a human has to click "Approve" in
  Edge's UI), the assistant message shows a pulsing "waiting" affordance
  with a hint that the result will arrive as soon as the server releases
  it ([`72e1f21`](#)).

### Added — EUR-Lex

- **Cellar REST fallback** as a fourth path after the public HTML / SOAP
  / SPARQL routes. Activates when the prior three return AWS WAF
  challenge markers ([`d9408d1`](#)).
- **Phantom-row guard** — refuse to persist a fetched document under
  1 KB. The page often comes back as a stub when EUR-Lex's CDN gets
  testy, and a 0-chunk row was getting marked `ready` ([`b6099bd`](#),
  [`c691e57`](#)).
- **Retry-with-backoff** in the fetch path; threshold raised from 400 to
  2 000 characters so genuinely short acts (one-page decisions) still
  pass but actual stubs are caught.

### Added — citations / viewer

- Numeric citation pills (`[1]`, `[2]`) and KB tags (`[gN]` / `[pN]`)
  now open EUR-Lex / corpus-document sources in the side panel with
  the cited quote highlighted on the page (previously only chat-
  attached PDFs worked) ([`3da46f0`](#)).

### Added — docs

- README gains the chat-with-citations and EUR-Lex-sync screenshot pair
  ([`6de8073`](#)) and a frank note that MCP async multi-step flows are
  partial ([`aa174da`](#)).
- New `docs/` diagrams: local-MCP-gatekeeper flow and the MikeRust
  architectural-stack overview ([`d093369`](#), [`615a4dc`](#),
  [`01dd8cc`](#)).

### Fixed

- `ChatView.upsertTab` assigned the same id to multiple tabs on rapid
  open / close cycles — fixed by deriving the id from a monotonic
  counter ([`82fa2af`](#)).
- `isDocxFilename` no longer panics on `null` / `undefined` input
  ([`10f8a74`](#)).
- `.taurignore` widened so editing docs or i18n catalogues doesn't
  trigger a Tauri dev-binary restart ([`56d832d`](#)).

---

## 2026-05-08 — Initial release: MikeRust fork from `willchen96/mike`

The starting point. MikeRust forks `willchen96/mike` (an AGPL-3.0
TypeScript / Express / Supabase / S3 / LibreOffice cloud-native AI
legal assistant), keeps the Next.js frontend largely intact, and
replaces the backend with a Rust + axum implementation designed to run
**entirely on the user's machine**.

### Added — sovereign backend

- **Rust + axum** API on `:3001`, single-binary Tauri shell wrapping the
  Next.js frontend on `:3000`.
- **SQLite** via `sqlite-vec` for vector storage (no Supabase, no
  pgvector). Schema covers users, chats, projects, documents,
  tabular reviews, workflows, MCP servers, settings.
- **Local filesystem storage** — `data/db/mike.db` + `data/storage/`
  for uploads and chat cache. Optional S3 trait scaffolded for later.
- **Auth** — PIN with Argon2id, opaque sessions, Windows Hello biometric
  unlock path.
- **`.mikeprj` envelope** — AES-256-GCM-encrypted project bundles with
  Argon2id-derived key, keyed to a recipient email so a colleague can
  import a project the sender explicitly shared with them.

### Added — local document pipeline

- **PDF** extraction via `pdfium-render` with per-page text and scanned-
  PDF detection.
- **DOCX** extraction in pure Rust (ZIP + XML) with tracked-deletion
  (`<w:del>`) and strike-through detection, surfaced inline as
  `[removed by author: …]` markers (see `docs/DOCX.md`).
- **RTF** via `rtf-parser`; **XLSX / XLS / XLSB / ODS** via `calamine`;
  **TXT / MD / CSV** as UTF-8 lossy decode.
- **ONNX embeddings** with `multilingual-e5-base` (768 dims) via
  `fastembed` + `ort`. Hardware acceleration is opt-in: `rag-directml`
  for Windows GPU, `rag-qnn` for Qualcomm NPU. Service tries
  QNN → DirectML → CPU, silently skipping providers whose DLLs aren't
  loadable.
- **Chunker + scanner** — recursive folder walk with `.gitignore`-style
  patterns, ~800-token chunks with 200-token overlap, embeddings stored
  in `sqlite-vec` virtual tables partitioned by
  `(user_id, project_id_or_global)` for cross-tenant isolation.
- **Hash-keyed cache** for chat attachments: documents land under
  `data/storage/cache/<sha256>.<ext>` with a pre-extracted
  `<sha256>.txt`. Multiple `documents` rows can reference the same
  hash, and ref-counted cleanup deletes the on-disk pair only when no
  doc still references it.

### Added — LLMs and tools

- **LLM providers**: Anthropic, Gemini, OpenAI, vLLM, Ollama. API keys
  stored locally in `data/db/mike.db`, never sent anywhere except the
  provider the user explicitly configured.
- **MCP client** (HTTP / SSE) — synchronous tool calls dispatch end-to-
  end; configured servers come from `MCP_SERVERS` env or the in-app
  MCP settings page.
- **MCP tool-schema injection** for tool-capable models so the assistant
  knows which tools are available without re-fetching per turn
  ([`c043a3b`](#)).

### Added — authoritative corpus framework

- **`LegalCorpusAdapter` trait** + builtin EUR-Lex V1 connector
  (CELEX-based fetch via public HTML, 24-language picker, English
  fallback when a doc isn't published in the requested language).
- **Italian Legal Corpus V1** — Normattiva (~69 K acts) + Corte
  Costituzionale (~22 K decisions) via the HuggingFace
  `datasets-server` `/rows` endpoint with Parquet bulk for the offline
  snapshot.

### Added — frontend

- Forked Mike's Next.js UI: chat with citation pills, document viewer
  with PDF.js, tabular-review workflow editor, projects + workflows +
  members + sharing.
- Bilingual i18n via `next-intl` — Italian primary, English mirror.

### Notes

- License: AGPL-3.0 (inherited from `willchen96/mike`); backend code
  ships under the same licence for consistency.
- Telemetry: none. No remote logging, no anonymous metrics. Outbound
  traffic only when the user explicitly invokes a remote LLM or a
  remote MCP server they configured themselves.
