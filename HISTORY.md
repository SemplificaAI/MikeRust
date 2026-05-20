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

## 2026-05-20 — Auto-fetch native DLLs + bundle them into the MSI

End-to-end: a clean checkout can now do `pnpm --dir frontend install`
followed by `./scripts/build-release.ps1` and end up with two MSIs in
`dist/` that **install with all native runtime DLLs already next to
the binary**. Operator zero-touch.

### Added

- **`scripts/fetch-native-libs.ps1`** — downloads the pinned versions
  of `onnxruntime` and `pdfium` for both Windows architectures and
  places them under `libs/<lib>/win-<arch>/`. Pinned to:
  - `onnxruntime 1.20.0` (matches `ort = "=2.0.0-rc.9"` per
    `libs/onnxruntime/README.md`; cross-version DLL deadlocks
    `TextEmbedding::try_new_from_user_defined` silently).
  - `pdfium chromium/7834` (recent bblanchon prebuilt; pdfium-render
    0.8+ binds dynamically through the stable `FPDF_*` C API).
  Idempotent — existing DLLs are kept unless `-Force` is passed.
  HEAD-checked every URL is alive (onnxruntime ≈ 62 MB / arch,
  pdfium ≈ 3.5 MB / arch).

### Changed

- **`scripts/build-release.ps1`** auto-invokes the fetch script when a
  target arch's DLLs are missing, then injects a per-arch
  `bundle.resources` overlay via a second `--config` argument. Each
  MSI bundles only the matching arch's DLLs (no double-arch payload).
- **`src/pdf/mod.rs::load_pdfium`** primary lookup is now per-arch
  (`libs/pdfium/win-x64/pdfium.dll` / `libs/pdfium/win-arm64/...`)
  mirroring the onnxruntime layout. Legacy flat `libs/pdfium/pdfium.dll`
  is kept as a fallback so pre-existing developer checkouts don't
  break before they run the fetch script.
- **`src/pdf/mod.rs` + `src/embeddings/service.rs`** lookups also
  walk `<exe_dir>/resources/` — Tauri MSI installs land
  `bundle.resources` files at `<install>/resources/<dest>`, so adding
  that directory to the candidate bases means the existing
  walker finds the bundled DLLs without any platform-specific
  branching.

### Install layout post-MSI

```
C:\Program Files\MikeRust\
├── mike-tauri.exe
└── resources\
    └── libs\
        ├── pdfium\win-<arch>\pdfium.dll
        └── onnxruntime\win-<arch>\onnxruntime.dll
```

Both loaders now find the DLLs at first boot of the installed
application — no manual operator step.

### Tests

`cargo test -p mike --lib` → **381/381** green (no test regressions
from the loader changes; the existing
`onnxruntime_subdir_and_filename_matches_compile_target`,
`find_onnxruntime_dylib_*` and `build_execution_providers_*` cover
the surface that moved).

PowerShell scripts parse-checked via
`[System.Management.Automation.Language.Parser]::ParseFile()`.

---

## 2026-05-20 — Build & dev launch scripts (`scripts/build-release.ps1`, `scripts/dev.ps1`)

Two PowerShell scripts that replace the ad-hoc command lines we have
been pasting into terminals all session.

### Added

- **`scripts/build-release.ps1`** — drives `tauri build --bundles msi`
  twice, once per Windows target triple (`x86_64-pc-windows-msvc` and
  `aarch64-pc-windows-msvc`), and collects the produced `.msi` files
  into the top-level `dist/` directory. Each MSI is renamed with a
  short `_x64` / `_arm64` suffix so the two architectures coexist
  without clobbering each other. Parameters: `-Target` (`x64` /
  `arm64` / `both`, default `both`), `-Clean` (wipe the per-triple
  bundle dir first), `-FrontendInstall` (run `pnpm install
  --frozen-lockfile` before the Rust build — useful in CI). Verifies
  that the rustc targets are actually installed before kicking off a
  long build.
- **`scripts/dev.ps1`** — wraps `tauri dev` with the right config
  pre-selected and the local `@tauri-apps/cli` resolved out of
  `frontend/node_modules`. Optional `-LogTrace` switch sets
  `RUST_LOG=mike=debug,info` for a richer backend trace.
- **`dist/`** is now a tracked directory (carries its own
  `.gitignore` that excludes everything but itself) so the build
  script can rely on its existence and `git status` stays clean
  after a build.

### Caveat documented in the script header

The runtime still needs `onnxruntime.dll` (matching the `ort` version
pin) and `pdfium.dll` next to the installed binary for the RAG and
PDF paths to work. Today these are **not** bundled via
`tauri.bundle.resources` in `src-tauri/tauri.svelte.conf.json`, so the
MSI installs only the application binary; post-install the operator
has to drop the matching DLLs into the install folder by hand. The
script's docstring points at `libs/onnxruntime/README.md` for the
right version pairing.

---

## 2026-05-20 — PDF viewer: serialize citation highlight against page rasterisation

Opening a citation on a **long** source PDF often landed the user on
the top of the document instead of on the cited page, with no
highlight. Cause: a race between the page-by-page render loop and the
highlight effect. `loading` only covers the initial PDF fetch; once
that resolves, the per-page rasterisation (canvas + text layer) keeps
running asynchronously for seconds. The highlight effect fired the
moment the user-supplied `quote` / `page` props arrived, scanned an
empty or partial `pagesEl`, found nothing, and silently no-op'd.

### Fixed

- **New `renderInProgress` reactive flag** in
  [PdfView.svelte](frontend/src/lib/components/documents/PdfView.svelte).
  `renderAll` flips it `true` at the start and `false` in a `finally`
  block — only the latest render (`token === renderToken`) clears it,
  so an aborted older pass can't race the new one. The flag is the
  promise-based "document fully painted" signal Svelte already had in
  hand (every `pageObj.render(...).promise` and `TextLayer.render()`
  return promises the loop awaits); we just expose its derivative as
  reactive state.
- **Highlight effect gates on both `loading` AND `renderInProgress`**.
  The effect re-fires automatically on the `true → false` transition,
  so an early citation click is honoured the moment the document is
  actually paintable — no `setTimeout`, no polling.
- **`renderAll` no longer calls `applyHighlight` directly** at its end.
  Removing that direct invocation lets the reactive effect be the
  single source of truth: same wake-up signal, no duplicate scrolls.
- **`applyHighlight` falls back to a page-scroll even without a quote
  match.** Citations that carry only `page` (KB chunks where verbatim
  text was lost in re-flow / page-break) used to silently do nothing;
  now they at least scroll the viewport to the right page.

`pnpm typecheck` → 0 errors / 0 warnings.

---

## 2026-05-20 — Gemini wire payload: explicit `safetySettings` + per-model `thinkingConfig`

Our `gemini.rs` was sending neither `safetySettings` nor a
`generationConfig.thinkingConfig`, falling back to Google's defaults
on both. Two consequences:

1. **Silent content-filter refusals.** Default safety is
   `BLOCK_MEDIUM_AND_ABOVE` across the four harm categories. Legal,
   insurance and PA workflows legitimately handle sentences citing
   violence, harassment investigations, anti-corruption files and
   discrimination cases — Gemini would empty-respond or refuse without
   any error surface in our logs.
2. **Default `thinkingLevel = HIGH` on Gemini 3.5.** The model burns
   output budget thinking before writing, which is implicated in the
   truncated NIS2 report we fixed earlier in the parser.

### Added

- **`safety_settings_off()`** in [src/llm/gemini.rs](src/llm/gemini.rs)
  builds the four-category payload with `threshold: "OFF"` — matches
  the canonical SDK example shape and the same toggles available in
  AI Studio.
- **`thinking_config_for_model(model)`** returns the right shape for
  the model family:
  - `gemini-3.5*` / `gemini-3-flash*` / `gemini-3-pro*` →
    `{"thinkingLevel": "MEDIUM"}` (3.5 API)
  - `gemini-2.5*` → `{"thinkingBudget": -1}` (2.5 API, dynamic
    thinking)
  - everything else (1.5, 2.0, future names) → `None`, omit the
    field — the older API rejects it with 400.
- Both are wired into `build_body`: every request now carries
  `safetySettings`, and Gemini 2.5+ requests also carry
  `generationConfig.thinkingConfig`.

### Not changed (deliberate)

- **`maxOutputTokens`** — left unset. Setting it to 65 535 is one
  token below the model cap, so it would be a no-op disguised as a
  change. Lower values risk truncation; higher are clamped.
- **`temperature`**, **`topP`** — already the Gemini defaults
  (1.0 and 0.95). Explicit values would be cargo-cult.
- **`seed`** — DELIBERATELY omitted in chat. A fixed seed would
  pin every response to the same output for the same prompt — useful
  for deterministic test fixtures, not for live chat.

### Tests

Five new tests in `llm::gemini`:
- `build_body_always_attaches_safety_settings_off`
- `build_body_thinking_config_shape_is_model_aware` (3.5 → enum
  `thinkingLevel`; 2.5 → integer `thinkingBudget: -1`; cross-checks
  that neither shape leaks into the other model family)
- `build_body_omits_thinking_config_on_legacy_families`
  (gemini-1.5 / 2.0)
- `build_body_still_carries_tools_and_system_instruction`
  (regression guard: new fields don't displace tools / system prompt)
- plus `empty_params` helper.

`cargo test -p mike --lib` → **381/381** green (was 377 +4).

---

## 2026-05-20 — `<CITATIONS>` block parser: tolerate truncated output

A NIS2 report generation produced **31** `[cN]` markers in prose plus
a fully-written `<CITATIONS>` JSON array of 31 entries — but the
response ran out of output tokens **before emitting the closing
`</CITATIONS>` tag**. `extract_citations_block` required that closing
tag and returned `None`, so all 31 citations were dropped silently
(`parsed 0 citations from response`) and every pill rendered as plain
text. The user saw `[c1] [c2] … [c31]` with no clickable behaviour.

### Fixed

- **`extract_citations_block`** now treats the closing tag as
  optional: if missing, it takes everything from the opening tag to
  end-of-text. This recovers the common "model ran out of tokens"
  case where the JSON itself is syntactically complete.
- **New `recover_truncated_citations_array`** — last-resort safety
  net for genuinely truncated JSON arrays (cut mid-entry). Walks the
  prefix as JSON respecting string scope (a `}` inside a quoted
  value doesn't confuse the depth tracker), remembers the offset of
  the most recent top-level `}` that closed an entry, and rebuilds
  `[ entry₁, …, entryₖ ]` as a valid prefix. Logs a warn so we know
  when this path fires.
- **`get_messages` retroactive recovery** — when the persisted
  `annotations` column is `NULL` (older turn or a turn the live
  pass silently dropped) but the message body still contains the
  `<CITATIONS>` block, the loader re-parses the content with the
  smarter extractor. Chats that broke silently in earlier builds
  now render pills again on reload.

### Tests

Four new tests in `routes::chat`:
- `unclosed_block_still_parses_when_inner_is_valid_json`
- `recovers_block_without_closing_tag` (the exact shape from the
  NIS2 report turn).
- `recovers_truncated_array_with_partial_last_entry`.
- `truncation_recovery_handles_quote_with_brace_inside` (security:
  a `}` inside a string must not be mistaken for an entry-close).
- `recovers_block_without_closing_tag_via_escape_repair` (the
  truncation tolerance composes with the existing `\'` repair).

`cargo test -p mike --lib` → **377/377** green (was 373 +4).

---

## 2026-05-20 — Rewrite free-form `[doc-id: …]` references into clickable `[cN]` pills

Verbose `generate_docx` follow-up descriptions occasionally bypassed the
`[cN]` + `<CITATIONS>` contract and emitted free-form bracketed
references such as `[doc-id: cdbe5ce0-…, page 1]` or `[doc-id: doc-0,
pages 2-3]` inline in the prose. These rendered as plain text — the
file and page were visible but not clickable. Fixed end-to-end.

### Added

- **`src/routes/chat.rs::extract_inline_docid_refs`** — manual scanner
  that finds every `[doc-id: <handle>[, page[s] <N|N-M>]]` occurrence
  in the assistant's prose. Tolerates variable whitespace, `page` vs
  `pages`, capital `Doc-ID:` / `DOC-ID:`, and the form without a page.
  Returns matches in order with byte offsets so the rewriter can do
  in-place substitution.
- **`src/routes/chat.rs::rewrite_inline_docid_citations`** — given the
  scanned references and a `resolve(handle) -> Option<(uuid, filename)>`
  closure (security: never reveals an arbitrary UUID through the
  viewer), assigns sequential `c1, c2, …` refs, replaces each
  occurrence in the prose, and produces a `<CITATIONS>`-compatible
  JSON array. Two references with the same `(uuid, page)` share one
  ref so the block stays compact.
- **`content_replace` SSE event** — the chat handler streams the
  rewritten body to the live view immediately after the rewrite, so
  pills render on the current turn without waiting for a reload.
  The frontend (`api/chat.ts` + `stores/chat.svelte.ts::onContentReplace`)
  swaps `message.content` wholesale on receipt.

### Changed

- **Chat handler** rewrites the body BEFORE persistence and BEFORE
  `extract_citations_block`, so the DB row also has `[cN]` markers and
  a synthesised citations array — reload of the chat shows the same
  clickable pills the live turn did.
- **`MRUST_SYSTEM_PROMPT`** picked up an explicit anti-pattern rule
  banning free-form `[doc-id: …]` style references in prose. The
  rewriter is a safety net; the prompt is the prevention.

### Tests

Seven new tests in `routes::chat` pin the scanner and the rewriter:
- `extract_inline_docid_refs_picks_up_the_observed_pattern` — the
  exact NIS2-turn shape (UUID + `page N` and `page N-M`).
- `extract_inline_docid_refs_handles_doc_n_and_pages_and_no_page`.
- `extract_inline_docid_refs_ignores_malformed_and_unrelated_brackets`.
- `rewrite_inline_docid_citations_collapses_repeats_into_one_ref`.
- `rewrite_inline_docid_citations_distinct_pages_get_distinct_refs`.
- `rewrite_inline_docid_citations_leaves_unresolved_handles_alone`
  (the security guarantee).
- `rewrite_inline_docid_citations_returns_none_when_nothing_resolves`.

`cargo test -p mike --lib` → **373/373** green (was 366 +7).
Frontend `pnpm typecheck` → 0 errors / 0 warnings.

---

## 2026-05-20 — Citation viewer: resolve corpus-inventory `doc_id` via canonical key

Clicking a citation pill could return `Impossibile caricare il documento
— Not Found` when the model emitted the inventory-line form of a corpus
document id (e.g. `[italian-legal] corte_costituzionale_1990_241`,
bracket + space included) instead of the `[gN]`/`[pN]` KB tag. The
backend resolver fell through every existing fallback, the frontend
hit `GET /document/<the-broken-string>/display` → 404.

### Fixed

- **`src/routes/chat.rs`** — new `canonical_corpus_key()` helper that
  strips every non-alphanumeric character and lowercases. A new
  per-request `library_corpus_index` (canonical-key → `(uuid,
  filename)`) is built from one cheap query against the user's full
  corpus library (`WHERE corpus_id IS NOT NULL AND corpus_identifier
  IS NOT NULL`), so it works even when the turn produced **zero** KB
  chunks (the existing `corpus_ref_to_tag` map then stays empty and
  doesn't help). The resolver's last-resort branch now canonicalises
  the model's `doc_id`, looks it up in that index, and stamps
  `document_id` + `filename` directly. The hallucinated `page` (if
  any) is dropped so the viewer falls back to text-searching the
  quote rather than scrolling to a fake page.

### Tests

`canonical_corpus_key_collapses_inventory_variants_onto_one_key`
pins five sloppy variants the model produces in practice — all
collapse to the same canonical string. Full suite:
`cargo test -p mike --lib` → **366/366** green (was 365 +1).

---

## 2026-05-20 — Pubblica Amministrazione domain + Fase-1 workflow pack

Introduced `pa` as a new canonical professional vertical and shipped
the seven workflow presets specified in `docs/pa-prompts.md` with full
prompt templates (the Fase-1 four-pack plus determina, RUP checklist
and PNRR milestone). All other workflows from blocks 1–5 that the spec
sketches with only a one-line description are deferred to a follow-up
pack — they don't have an authoring prompt yet.

### Added — domain registration

- **`src/domain.rs`** — `pa` appended to `DOMAINS` (between `compliance`
  and `others`). Schema-default validation (`is_valid`) accepts it
  automatically.
- **`frontend/src/lib/types/domain.ts`** — `pa` appended to the TS
  mirror, so the picker dropdown lists it.
- **`src/llm/builtin_tools.rs`** — `list_docx_templates` schema enum
  description extended with `'pa'` so the model can filter on it.
- **`frontend/locales/{it,en,fr,de,es,pt}.json`** — `Domains.values.pa`
  added across all six locales:
  - it: Pubblica Amministrazione · en: Public Administration
  - fr: Administration publique · de: Öffentliche Verwaltung
  - es: Administración pública · pt: Administração pública

### Added — workflows (`config/workflow-presets/pa/`)

Block 1 — Atti amministrativi:
- **`pa-delibera`** (assistant) — analisi delibera: competenza, motivazione (art. 3 L. 241/1990), iter procedimentale, copertura finanziaria (art. 183 TUEL), vizi rilevati, termini di impugnazione (TAR / PR / autotutela).
- **`pa-determina`** (assistant) — estrazione tabellare di 12 campi obbligatori della determina dirigenziale (numero, oggetto, norma attributiva, CIG, importo, capitolo, visti, pareri, pubblicazione, esecutività) con conformità sì/no/da verificare.

Block 2 — Appalti pubblici (D.Lgs. 36/2023):
- **`pa-appalto-review`** (assistant) — review clausola-per-clausola con focus obbligatorio su 10 aree (penali, SAL, subappalto, recesso, ADR, garanzie, varianti, riserve, revisione prezzi, clausole sociali) + sintesi rischi con semaforo 🔴🟡🟢.
- **`pa-rup-checklist`** (tabular) — 1 riga = 1 documento del fascicolo gara; 8 colonne mappano la fase (programmazione/progettazione/affidamento/esecuzione/chiusura), l'adempimento RUP, la norma D.Lgs. 36/2023 e l'esito.

Block 3 — Procedimento (L. 241/1990):
- **`pa-241-check`** (assistant) — verifica dei 7 controlli L. 241/90 (responsabile, comunicazione avvio, termine, preavviso rigetto, motivazione, partecipazione, accesso) con mini-scheda per punto e tabella riassuntiva esito+rischio+sanatoria.

Block 4 — PNRR e fondi UE:
- **`pa-pnrr-milestone`** (tabular) — 1 riga = 1 documento probatorio; 10 colonne riconducono il doc a Missione/Componente/Investimento, milestone/target ID, scadenza UE, documentazione richiesta vs disponibile, spese rendicontate, rischio (🔴🟡🟢), azione raccomandata.

Block 5 — Trasparenza e anticorruzione:
- **`pa-ptpct`** (assistant) — analisi del PTPCT in 6 sezioni (struttura · analisi del rischio · misure di prevenzione · obblighi di trasparenza D.Lgs. 33/2013 · RPCT · gap+raccomandazioni con scala Obbligatorio-Mancante/Presente-carente/Raccomandato-Mancante).

### Added — tests

- **`src/presets/workflow.rs::shipped_pa_workflows_present_and_typed`**
  anchors the seven PA preset ids, asserts each carries `domain = "pa"`
  and the spec's prescribed kind (assistant/tabular), each `practice`
  starts with `PA — `, and tabular presets have non-empty
  `columns_config` while assistants don't carry one.

### Tests

`cargo test -p mike --lib` → **365/365** green (was 364 +1).
Frontend `pnpm typecheck` → 0 errors / 0 warnings.

### Deferred

The fourteen workflows from `docs/pa-prompts.md` that are sketched
with only a one-line description (pa-ordinanza, pa-parere, pa-bando,
pa-collaudo, pa-variante, pa-silenzio, pa-autotutela, pa-accesso,
pa-rendiconto, pa-audit, pa-irregolarita, pa-foia, pa-conflitto,
pa-dati-aperti) require new authoring prompts and are tracked as a
follow-up batch.

---

## 2026-05-20 — NIS2 compliance pack: docx template + assistant workflow + tabular review

Translated `docs/nis2-prompts.md` into three ready-to-ship MikeRust
artefacts, all anchored to the **compliance** domain and to the Italian
NIS2 transposition (D.Lgs. 138/2024).

### Added

- **`config/docx-templates/compliance/nis2-audit-readiness-report.json`**
  — DOCX layout sidecar (`id: compliance/nis2-audit-readiness-report`,
  A4 portrait, L1, square-bracket placeholders). Section skeleton
  matches the spec one-to-one: title block, intestazione metadata,
  fixed 10-domain scoring table (`1. Governance` → `10. HR & accessi`)
  with TOTAL row + conformity semaphore, top-3 sanction risks under
  art. 38, three 4-week remediation phases, free-text final notes, and
  the signature block for valutatore + organo di gestione (art. 20).
  Required metadata: `ORGANIZZAZIONE`, `DATA_ASSESSMENT`, `VALUTATORE`,
  `CLASSIFICAZIONE_SOGGETTO` (`ESSENZIALE`/`IMPORTANTE`),
  `APPROVATORE_GESTIONE`, `DATA_APPROVAZIONE` (+ optional
  `FATTURATO_GLOBALE` to compute the 2% / 1,4% sanction cap in euro).
- **`config/workflow-presets/compliance/nis2-audit-readiness.json`**
  — `assistant`-type workflow (`builtin-compliance-nis2-audit-readiness`),
  practice "NIS2 — Cybersecurity & resilience". Walks the model
  through the full report (sections 1–4 of the spec), enforces the
  fixed 10-domain ordering / dicitura, requires evidence citations
  per score, and links to the docx via
  `default_output_template: "compliance/nis2-audit-readiness-report"`
  so `read_workflow` auto-bundles the template's authoring contract
  and the closing instruction calls `generate_docx` with the right
  template_id.
- **`config/workflow-presets/compliance/nis2-policy-inventory.json`**
  — `tabular`-type workflow (`builtin-compliance-nis2-policy-inventory`)
  for inventorying multiple policy/procedure documents against the
  ten NIS2 domains. Nine columns: title, type (policy/procedura/
  piano/registro/delibera/evidenza_tecnica), NIS2 domains covered,
  version+date, art. 20 approval, NIS2 controls present, gaps,
  maturity score 0–3, evaluator notes. Designed as the pre-step to
  the audit-readiness assistant workflow.
- **`src/presets/workflow.rs::tests`** — two new tests:
  - `shipped_workflow_presets_all_load_cleanly`: strongly-typed
    loader must accept every JSON file in `config/workflow-presets/`,
    every id must be unique, every `domain` canonical, every `kind`
    in `{assistant, tabular}`. Future-proofs against silent
    typo-induced startup drops.
  - `shipped_compliance_nis2_presets_present_and_wired`: anchors the
    three NIS2 ids and asserts the assistant workflow points at the
    matching docx template id, and that the tabular has the expected
    column shape.

### Tests

`cargo test -p mike --lib` → **364/364** green (was 360 +4: the two
new workflow-preset tests above, plus the two thought_signature tests
from the Gemini 3.5 entry).

The dev binary auto-loads presets at startup, so a tauri-dev restart
exposes "Audit readiness NIS2" in the Workflow picker and "Inventario
policy NIS2" in the Tabular reviews picker.

---

## 2026-05-20 — Gemini 3.5 Flash (GA) + thought_signature plumbing

Google released **Gemini 3.5 Flash** as GA at Google I/O '26 (model id
`gemini-3.5-flash`, 1M input / 64K output context, vision, tools,
agentic thinking, no region restrictions). Registered the new model
and promoted it to the default chat model. The release also surfaced
a hard requirement that broke our tool-use loop on day one and is now
fixed.

### Added

- **`config/model.json`** — `gemini-3.5-flash` registered at the top
  of the Google model list (`standard` tier, `1_048_576` context,
  `65_536` output, vision + tools + thinking, `preview: false`).
  Released 2026-05-19 GA, available globally.
- **`src/llm/types.rs`** — `ToolCall::thought_signature: Option<String>`.
  Gemini 2.5+ tags `functionCall` parts produced during a thinking
  pass with an opaque `thoughtSignature`; clients are required to echo
  it back on replay, or Gemini 3.5+ rejects the next turn with
  `400 INVALID_ARGUMENT: Function call is missing a thought_signature
  in functionCall parts`. The field is serialised only when set, so
  non-Gemini providers keep emitting a clean shape.
- **`src/llm/gemini.rs`** — parse `thoughtSignature` from incoming SSE
  `functionCall` parts; echo it back in `to_wire_contents` when
  replaying assistant tool-calls; two new unit tests pin both halves
  of the round-trip.

### Changed

- **`src/llm/summarize.rs`** — `context_window_tokens` recognises
  `gemini-3.5-flash` as a 1M-token model.
- **`src/llm/mod.rs`** — tests extended to cover `gemini-3.5-flash`
  on `provider_for_model` / `supports_mcp_tools` /
  `is_vision_capable` (the runtime predicates already match via
  prefix / `contains`).
- **`src/routes/chat.rs`** — the five default-model literals
  (`gemini-3-flash-preview`) replaced with `gemini-3.5-flash`. New
  chats, the generate-title fallback and the "first configured
  provider" fallback all land on the GA model.
- **`config/model.json`** — `gemini-3-flash-preview` marked
  `legacy: true` with a `_legacy_note` pointing users at the new
  3.5 entry; pinned-preview settings keep working but new picks see
  the GA model first.

### Fixed

- **Gemini 3.5 Flash tool-use** — the first `read_workflow` call in
  the running app failed with `400 INVALID_ARGUMENT: Function call
  is missing a thought_signature in functionCall parts. ... Additional
  data, function call default_api:read_workflow, position 6`. Plumbing
  the `thoughtSignature` through `ToolCall` and back into the wire
  payload on replay clears it; covered by
  `parse_sse_function_call_captures_thought_signature` and
  `to_wire_contents_echoes_thought_signature_on_replay`.

### Tests

`cargo test -p mike --lib llm::` → **66/66** green, including the
two new thought-signature round-trip tests.

---

## 2026-05-20 — ONNX Runtime downgrade preparatory to data-privacy work

Downgraded the embedding stack from `ort 2.0.0-rc.12` / `fastembed 5.13.4`
(onnxruntime 1.24.2) back to `ort 2.0.0-rc.9` / `fastembed 4.9.1`
(onnxruntime 1.20.0). The downgrade is **propedeutic to a new
data-security and privacy feature**: that work relies on the smaller,
audited 1.20.0 runtime surface and on the set of execution providers
that shipped with it.

### Changed

- **`Cargo.toml`** — `ort` pinned at `=2.0.0-rc.9` (was `=2.0.0-rc.12`)
  and `fastembed` pinned at `=4.9.1` (was `5`). The `std` feature flag
  on `ort` is dropped — it never existed on rc.9. The `image-models`
  feature on `fastembed` is dropped — it was a 5.x addition we never
  used.
- **EP type names** — rc.9 exposes execution providers only via
  `ort::execution_providers::<Name>ExecutionProvider`; the short
  `ort::ep::<Name>` aliases (`QNN`, `CUDA`, `DirectML`, …) were
  introduced in a later rc and do not exist here. Every EP push in
  `embeddings/service.rs::build_execution_providers` was renamed to
  the long form. Vitis specifically is `VitisAIExecutionProvider`.
- **`UserDefinedEmbeddingModel` construction** — fastembed 4.9.1 marks
  the struct `#[non_exhaustive]`; the explicit struct literal we used
  on 5.x no longer compiles. Now built through `::new(onnx, tokenizer)
  .with_pooling(Pooling::Mean).with_quantization(QuantizationMode::None)`.
  The `external_initializers` and `output_key` fields fastembed 5.x
  exposes simply don't exist on this version — we never relied on
  them, but they will need re-adding when we eventually bump back.

### Removed

- **`rag-webgpu`** and **`rag-azure`** cargo features and their EP
  pushes in `build_execution_providers` — neither EP exists in
  onnxruntime 1.20.0. Removed from the platform packs
  (`rag-accel-windows`/`linux`/`macos`) and from `rag-accel-all`.
  Will return when the runtime moves forward again.

### Docs

- **`libs/onnxruntime/README.md`** — version note flipped from
  "must be exactly 1.24.2 for rc.12" to "must be exactly 1.20.0 for
  rc.9", with the strings-probe pattern updated. New "Why we are
  deliberately on the rc.9 line" subsection points back here.

### Migration note for contributors

Re-vendor the matching DLL before relaunching the dev binary:
`libs/onnxruntime/<platform>/onnxruntime.dll` (or `.so`/`.dylib`) MUST
be a **v1.20.0** Microsoft build. Keeping the 1.24.2 DLL while running
rc.9 silently deadlocks `TextEmbedding::try_new_from_user_defined`
during embedding-model load — the failure mode that triggered the
original "lock to one version" rule. The README has a one-liner
PowerShell snippet that fetches and drops the right file.

### Tests

All **360 mike lib tests pass** with default features. The
`embeddings::service::` block (45 tests covering vector packing,
dylib discovery walking, execution-provider construction with every
EP enabled, …) is green both under default features and under
`--features rag-accel-all`. The end-to-end ONNX inference path is
only exercised when the dev binary loads the matched 1.20.0 DLL —
contributors should re-run `tauri dev` and trigger an embedding to
validate that side of the migration on their machine.

---

## 2026-05-19 — Declarative legal-corpus connectors + data-sources UX overhaul

Brought the JSON-manifest corpus system from scaffold to a working
declarative connector engine, reworked the Settings → Data sources
panel, and cleared a long tail of QA findings on the corpus workflow.

### Added

- **Corpus discovery metadata** — `jurisdiction`, `doc_types`, `auth`,
  `search_mode`, `fetch_format` added to the manifest schema and to all
  28 manifests; surfaced through `GET /corpora`.
- **Data-sources filters** — Jurisdiction and Type dropdowns over the
  source tabs, per-source badges (document type, auth, search mode,
  fetch format), and a search-mode hint under the search box.
- **Per-corpus enable/disable** — generic `GET|PUT /corpora/:id/config`
  (shares the existing `corpus_settings` table, no migration), with an
  enable toggle on every source panel — not just EUR-Lex.
- **Unified search box — year filter** — the engine extracts a 4-digit
  year from the query and routes to a date-filtered endpoint via the
  new optional `url_template_year` manifest field (wired for US/eCFR
  and US/CourtListener; available to any manifest).
- **Manifest hot-reload (dev)** — a background watcher re-reads
  `config/corpora-plugins/` and swaps the plugin + adapter registry
  in-process, so connector edits no longer need a restart. Debug builds
  only; a packaged app ships frozen manifests.
- **Per-hit indexing queue** — multiple "Index" clicks now queue up,
  each hit showing its own state (queued → running → done/error) with
  a retry, instead of only the last click showing a progress bar.
- `PLAN_FONTI_INTERNAZIONALI.md` — a matrix of the 28 SuzieLaw-derived
  international legal sources (download mode, auth, format, doc_id).

### Changed

- The `http-fetch-per-id` `ManifestAdapter` is now the live engine for
  the declarative corpora (search + fetch via URL templates and
  CSS/JSONPath extraction) — previously scaffold-only.
- CNIL, EUR-Lex and Italian-Legal are no longer hidden from the Data
  sources panel — an over-broad exclusion list left EUR-Lex's dedicated
  panel unreachable.
- Display-name cleanups: `CNIL` → `FR / CNIL`,
  `Italian Legal Corpus` → `IT / Italian-Legal`; the jurisdiction
  filter shows localized names ("Italiana", "Tedesca", …) in all six
  locales.
- `http-fetch-per-id` search: a keyword search that returns nothing now
  falls back to an identifier probe — one box accepts free text and
  corpus-native identifiers.
- Connector manifests corrected against the live APIs: **DE/OpenLegalData**
  (search + fetch through the JSON API by numeric id — the website
  pages are Cloudflare-gated, the API is not), **JP/e-Gov** (keyword
  search added via the v2 API), **US/eCFR** (section-level result
  headings), **US/CourtListener** (year filter).
- **IT/Normattiva** set to citation-only: the portal is a JS app with
  no search API — free-text Italian search lives in IT/Italian-Legal
  (local FTS index).
- Indexed-document rows now show the stored `.txt` size.

### Fixed

- JSON `null` no longer leaks into search results as the literal text
  `"null"` — a `null` field is treated as absent.
- "Import already in progress" is now an informational toast, not a red
  error; the import progress bar tracks the real `downloading` /
  `importing` job states (it previously watched a `running` string the
  backend never emits) and resumes polling when the panel is reopened
  mid-import.

---

## 2026-05-18 — KB citations open in viewer + formal verification trace

Implemented the frontend wiring needed to open/download knowledge-base
citations (`[gN]` / `[pN]`) from chat directly in the document viewer,
and recorded a formal verification step so the change can be
re-checked manually.

### Fixed

- KB citation payload `path` is now preserved in the frontend citation
  model (`kbPath`) and routed through the viewer tab state (`source:
  kb`) instead of being treated like `/document/{id}` only.
- Viewer fetch path now switches by source:
  - `document` -> `/document/{id}/display` and `/document/{id}/download`
  - `kb` -> `/sync/kb-doc?path=...` (for both open and download)
- DOCX viewer now supports tracked-change mode with per-tab policy
  `show | accept | reject`, surfaced in the header controls.
- `Accept` / `Reject` are implemented as renderer-level visual policies
  (non-destructive): no backend persistence/mutation is performed in
  this step.
- Chat token strategy switched to quality-first: frontend compaction is
  now emergency-only (very large payloads), while normal context
  reduction is delegated to backend model-aware summarization (80%
  context-window trigger). Emergency compaction preserves first user
  anchor + recent window + selected older structured user turns.
- Emergency compaction budgets are now model-aware on the frontend:
  they read `context_window` from the model catalogue when available,
  with dedicated fallback for local/OpenAI-compatible endpoints
  (including Ollama/vLLM patterns) to avoid over-trimming on large local
  context models.
- `config/model.json` now includes a dedicated `local` provider catalogue
  aligned to a 24GB-VRAM target: professional quantized shortlist only
  (Qwen 3.6 27B, Qwen 3.5 27B, Qwen 2.5 32B, Mistral Small 3.1 24B),
  with 70B entries removed.
- Added `scripts/recommend-context-window.ps1` to auto-profile local host
  resources (RAM/VRAM), read installed Ollama models via `ollama list`
  + `ollama show`, and output a per-model recommended `context_window`
  ready for `config/model.json` (`text`, `json`, or `markdown` formats).
- Added `scripts/ollama-context-profiles.json` with Snapdragon X Elite
  tuned profiles (`qwen3.5:4b` @ 16K, `qwen3.5:9b` @ 8K,
  `gemma4:e2b` @ 8K), plus two operational scripts:
  `scripts/apply-ollama-context-profiles.ps1` (dry-run by default,
  `-Apply` to create aliases) and
  `scripts/verify-ollama-context-profiles.ps1` (checks effective context
  via `ollama show`).
- `config/model.json` local provider now includes tuned alias IDs
  aligned to those profiles:
  `mikerust-qwen35-4b:ctx16k`, `mikerust-qwen35-9b:ctx8k`,
  `mikerust-gemma4-e2b:ctx8k`.
- Fixed `src-tauri/tauri.svelte.conf.json` dev/build commands pathing
  for repository-root launches: `pnpm --dir ../frontend ...` ->
  `pnpm --dir ./frontend ...`, avoiding accidental resolution to
  `C:\Progetti\frontend`.
- Fixed Settings → Model roles provider-filter logic so local models are
  visible in role dropdowns when a local endpoint is configured
  (`local_base_url` set). This unblocks selection of local tuned aliases
  like `mikerust-qwen35-4b:ctx16k` as `main_model`.
- Fixed broader Settings provider/model coherence to prevent cross-provider
  mismatches (e.g. local model accidentally sent to Gemini):
  - Active provider chips are now toggle-style multi-select (non-exclusive),
    with enabled state tied to actually configured providers.
  - Role-model dropdowns now show only models from configured + toggled
    providers.
  - Local models in role dropdowns are now emitted with `local:` prefix,
    so backend provider routing is unambiguous.
  - Local provider role options are filtered against the runtime
    OpenAI-compatible `/models` list when available, so only truly
    reachable local models are shown.
  - Save path normalizes role model ids and persists a coherent
    `active_provider` from toggled providers.
- Updated `README.md` Quick start run command to the working Windows
  repo-local Tauri CLI invocation:
  - `.\frontend\node_modules\.bin\tauri.cmd dev --config src-tauri/tauri.svelte.conf.json`
  - Added the global `cargo tauri dev ...` form as optional fallback only.
- Updated `config/model.json` local provider catalogue for GPU-server
  deployment, adding balanced runtime profiles discovered on the remote
  Ollama endpoint:
  - `rtx4090-qwen36:balanced`
  - `rtx4090-qwen35-9b:balanced`
  - `rtx4090-gemma4-26b:balanced`
  - `rtx4090-gemma4-e4b:balanced`
  - `rtx4090-gemma4-e2b:balanced`
  and refreshed the local-provider quality note accordingly.
- Fixed local OpenAI-compatible response handling for models that emit
  reasoning text instead of assistant `content`:
  - streaming parser now falls back to `delta.reasoning` /
    `delta.reasoning_content` when `delta.content` is empty;
  - non-stream completion parser now falls back to
    `message.reasoning` / `message.reasoning_content`.
  This prevents empty assistant turns with some `*balanced` profiles.
- Fixed local model resolution in `build_local_config`: when the selected
  model is explicitly prefixed (e.g. `local:rtx4090-qwen36:balanced`),
  the backend now uses that requested model id instead of silently
  overriding it with `local_model` from saved provider defaults. This
  removes cross-model mismatches and `502` errors on `/generate-title`.
- Refined local-output behavior to avoid exposing model thinking traces in
  chat UI: `llm/local` now emits only assistant `content` as user-visible
  text (stream + non-stream paths), ignoring `reasoning` fields that some
  OpenAI-compatible backends include separately.
- Added explicit server-console diagnostics for OpenAI-compatible upstream
  payloads (`llm/local`):
  - log each incoming SSE `data:` line from Ollama/vLLM as
    `[llm/local] upstream_sse ...` (truncated preview);
  - log non-stream completion raw body as
    `[llm/local] upstream_complete_body ...` (truncated preview).
- Forced non-thinking mode on OpenAI-compatible local requests
  (`llm/local` stream + complete now send `think: false`) to reduce
  pathological outputs where `content` was a single character (e.g. `S`)
  while long reasoning text consumed the completion budget.
- Fixed `llm/local` SSE parser buffering logic: when multiple upstream
  `data:` lines arrive in the same buffer, the parser now drains and
  queues all parseable events instead of returning only the first and
  dropping the rest. This addresses truncated one-character replies seen
  in chat despite richer upstream payloads.
- Fixed `/chat/:id/generate-title` empty-title persistence: when the title
  model returns empty `content`, backend now derives a deterministic
  fallback from the first user message (up to 5 words), and finally
  falls back to `New chat` if that is also empty. This prevents chats
  remaining `Untitled`.
- Updated Settings → Local provider UX:
  - swapped field order so `API key (optional)` appears before `Model`;
  - added a manual refresh button next to `Base URL` to query
    `<base>/models` on demand;
  - model roles (`Main`, `Title`, `Tabular`) now take local choices
    strictly from the runtime `/models` response (when local provider is
    active), so users can only pick actually available local models.
- Finalized Local provider constraint: `Model` is now a runtime-backed
  dropdown (not free text), populated only from `<base>/models` after
  refresh, so users cannot save arbitrary local model IDs.
- Follow-up per UX request: removed Local `Model` selector entirely from
  the Local provider card. Users now choose runtime local models only in
  Model roles (`Main`/`Title`/`Tabular`), with `Main` auto-defaulted to
  the first runtime local model after refresh when unset/invalid.
- Tightened global chat response-style instructions in
  `MRUST_SYSTEM_PROMPT` to improve formatting quality:
  - answer in the user's language;
  - keep responses concise and structured;
  - avoid repeating the same filename and duplicate document lists;
  - avoid verbose meta-reasoning unless explicitly requested.
- Added Recent Chats rename UX in the sidebar:
  - pencil button added immediately left of the trash icon;
  - inline title edit mode with Enter / blur save and Escape cancel;
  - rename persisted through `PATCH /chat/:id` and reflected in local
    chat list state.
- New-chat UX tweak: creating a fresh chat now closes the right-side
  document viewer panel (`docViewer.closeAll()`), so the new conversation
  starts with a clean workspace.
- Chat-rename UX polish: entering inline rename now autofocuses the input
  and places the caret at the end of the current title text.
- Settings → Active providers persistence: multi-selected provider chips
  are now restored on reopen (filtered to currently configured providers)
  via localStorage, so combinations like `Google + Local` survive page
  reopen; disabling `Local` and saving leaves only `Google` active.
- Fixed provider-toggle persistence timing: active-provider chips now
  write to localStorage immediately on toggle (not only on Save), so
  `Google + Local` remains selected after reopen even before a full
  settings submit.

### Tests

- Formal validation command executed:
  - `pnpm --dir frontend typecheck`
- Result:
  - `svelte-check found 0 errors and 0 warnings`
- Added unit coverage for citation mapping of backend `path` ->
  frontend `kbPath` in `frontend/src/lib/types/citation.test.ts`.
- Formal validation re-run after tracked-change implementation:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`
- Formal validation re-run after chat context-compaction update:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`
- Formal validation re-run after quality-first compaction strategy update:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`
- Formal validation re-run after model-aware budget update:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`
- Formal validation after local model-catalogue update:
  - `cargo check -p mike`
  - `Finished dev profile` (compilation OK; non-blocking warnings only)
- Formal validation after 24GB local shortlist alignment (Qwen 27B variants):
  - `cargo check -p mike`
  - `Finished dev profile` (compilation OK; non-blocking warnings only)
- Formal validation after context-window recommendation script:
  - `pwsh -File .\scripts\recommend-context-window.ps1 -Format json`
  - Exit code `0`; JSON report generated with model max context + recommended context.
- Formal validation after profile scripts and tuned aliases update:
  - `pwsh -File .\scripts\apply-ollama-context-profiles.ps1`
  - Exit code `0`; dry-run generated valid `ollama create` commands.
  - `pwsh -File .\scripts\verify-ollama-context-profiles.ps1`
  - Exit code `0`; aliases currently missing until explicit apply step.
  - `cargo check -p mike`
  - Compilation OK (`Finished dev profile`; non-blocking warnings only).
- Formal validation after Tauri Svelte config fix:
  - `.\frontend\node_modules\.bin\tauri.cmd dev --config src-tauri/tauri.svelte.conf.json`
  - Vite dev server started on `http://127.0.0.1:5173/` and Tauri DevCommand started successfully (running background process).
- Formal validation after local-role dropdown fix:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after provider/model coherence fix:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after README command update:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after GPU-server balanced catalogue update:
  - `cargo check -p mike`
  - Compilation OK (`Finished dev profile`; non-blocking warnings only).
- Runtime validation after balanced catalogue update:
  - `.\frontend\node_modules\.bin\tauri.cmd dev --config src-tauri/tauri.svelte.conf.json`
  - Vite dev server started on `http://127.0.0.1:5173/`; Tauri DevCommand running.
- Formal validation after local-balanced runtime fixes:
  - `cargo check -p mike`
  - Compilation OK (`Finished dev profile`; non-blocking warnings only).
- Formal validation after thinking-visibility refinement:
  - `cargo check -p mike`
  - Compilation OK (`Finished dev profile`; non-blocking warnings only).
- Formal validation after upstream-payload logging enhancement:
  - `cargo check -p mike`
  - Compilation OK (`Finished dev profile`; non-blocking warnings only).
- Formal validation after forcing `think: false` on local requests:
  - `cargo check -p mike`
  - Compilation OK (`Finished dev profile`; non-blocking warnings only).
- Formal validation after SSE buffering/parser fix:
  - `cargo check -p mike`
  - Compilation OK (`Finished dev profile`; non-blocking warnings only).
- Formal validation after non-empty title fallback:
  - `cargo check -p mike`
  - Compilation OK (`Finished dev profile`; non-blocking warnings only).
- Formal validation after Local provider settings UX/runtime-model binding:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after Local model field dropdown lock:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after removing Local model selector and defaulting
  `Main` from runtime refresh:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after response-style prompt hardening:
  - `cargo check -p mike`
  - Compilation OK (`Finished dev profile`; non-blocking warnings only).
- Formal validation after Recent Chats rename feature:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after new-chat doc-viewer auto-close:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after rename autofocus/caret UX polish:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after active-providers persistence:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after immediate provider-toggle persistence:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.

### Process

- Per user request, every future development step must include at least
  one formal technical validation (typecheck/build/targeted tests), and
  both `HISTORY.md` and `PLAN_CODEX.md` must be updated with the
  executed command(s) and result(s) for manual re-verification.

## 2026-05-17 — Clean-room frontend rewrite (React → Svelte 5)

The AGPL Next.js UI has been replaced by a clean-room Svelte 5 + Vite +
Tailwind v4 application. The rewrite keeps the Rust backend contracts
intact while rebuilding every screen in a new component system and store
architecture.

### Added — Svelte 5 foundations

- clean-room scaffold + bootable Tauri shell; new design-system
  primitives and layout shell (`Fase 0–1`). ([`b86fe5f`](#),
  [`a6b7f3f`](#), [`9f20c21`](#))
- API layer + Svelte runes stores + auth flow (`Fase 2–3`).
  ([`5b45e71`](#))
- Routed shell + Workflows screen (`Fase 4`). ([`2e42c92`](#))
- Theme toggle (light/dark/system) and i18n store; locale bundles back
  to 6 languages. ([`70e75d9`](#), [`43251fb`](#), [`501f58d`](#))

### Added — Core product screens

- Assistant chat with streaming responses, attachments, and model
  selection. ([`c92f2a0`](#))
- Projects screen (list/create/edit/delete) with project detail workflow.
  ([`6ca253e`](#))
- Tabular reviews screen. ([`0b8fe49`](#))
- Workflow creation modal + editor path. ([`def3757`](#))
- Templates (DOCX) screen. ([`4e1f6bd`](#))
- Settings screens: profile/security, LLM models, MCP servers.
  ([`f8a7700`](#), [`3c6863e`](#), [`5f0b4ea`](#))
- Sidebar refinements (chat list, sticky settings). ([`9593df8`](#))

### Removed

- Legacy React/Next.js frontend removed from the repository.
  ([`0a9bbcf`](#))

### Docs

- README reframed to explain the blind Svelte rewrite and code
  independence; screenshots refreshed. ([`6342131`](#), [`46de002`](#))
- Svelte rewrite gap-analysis document added. ([`def85a1`](#))

## 2026-05-14 — Toolkit commercialista (22 workflows + 3 DOCX templates)

Mirror del toolkit medico-legale shipped earlier today, this time for
the Italian commercialista / dottore commercialista vertical. Maps
the 6 operational areas of `docs/piano_toolkit_commercialista.md`
under the canonical `finance` domain.

### Added — Workflow tabular (17)

**Trasversale (Fase 1 del piano, working life dello studio):**
- `commerc-inventario-documenti` (8 cols) — classificazione
  documentale universale per tutte le aree, ancorata a `DOC-NN`.
- `commerc-scadenzario-annuale` (7 cols) — calendario adempimenti
  fiscali italiani con responsabile interno.
- `commerc-portafoglio-clienti-stato` (10 cols) — dashboard
  semaforica per cliente su tutti gli adempimenti dell'anno.
- `commerc-quality-check-preinvio` (6 cols) — checklist quality
  pre-invio dichiarazioni (Mod. Redditi / IVA / 770).

**Area 1 — Perizie e stime di valore:**
- `commerc-riclassificazione-bilanci` (8 cols) — riclassificazione
  CE pluriennale a valore aggiunto.
- `commerc-indicatori-econ-finanz` (8 cols) — ROE/ROI/EBITDA
  margin/PFN-EBITDA/Current/DSO/DPO con benchmark di settore.
- `commerc-metodi-valutativi` (7 cols) — patrimoniale / reddituale
  / DCF / multipli ponderati → valore finale.

**Area 2 — CTU tributaria:**
- `commerc-contestazioni-ufficio` (7 cols) — estrazione rilievi
  da avviso di accertamento / PVC / cartella.
- `commerc-analisi-bancaria` (8 cols) — mappa movimenti c/c ex
  Art. 32 DPR 600/73 con classificazione Cass. SS.UU. 24823/2015.
- `commerc-rideterminazione-reddito` (6 cols) — confronto importi
  ufficio vs rideterminati.

**Area 3 — Crisi d'impresa e procedure concorsuali (CCII):**
- `commerc-indicatori-crisi` (7 cols) — DSCR / PFN-EBITDA / PN /
  liquidità / CCN con soglie di allerta.
- `commerc-stato-passivo` (8 cols) — classificazione creditori per
  rango (privilegiati / chirografari / postergati Art. 2467 c.c.).
- `commerc-confronto-piano-liquidatoria` (6 cols) — best interest
  of creditors test ex Art. 88 c.4 CCII.
- `commerc-cashflow-previsionale` (7 cols) — FCF previsionale
  5 anni con DSCR di piano.

**Area 4 — Due diligence fiscale e societaria:**
- `commerc-checklist-dd-documenti` (6 cols) — tracking documenti
  per area (fiscale / societario / previdenziale / giuslavoristico).
- `commerc-rischi-dd-semaforo` (10 cols) — risk map con semaforo
  🔴 / 🟡 / 🟢 + azione SPA consigliata (accantonamento / escrow /
  monitoraggio).
- `commerc-esposizione-fiscale-anno-tributo` (11 cols) — matrice
  anno × tributo (IRES / IRAP / IVA / Ritenute / INPS+INAIL) +
  sanzioni min/max + interessi → esposizione worst case.

**Area 5 — Contenzioso tributario (D.Lgs. 546/92):**
- `commerc-rilievi-controdeduzioni` (10 cols) — matrice
  rilievo × tesi difensiva × giurisprudenza × DOC probanti.
- `commerc-scadenze-processuali` (6 cols) — termini D.Lgs. 546/92
  (60 gg ricorso, 90 gg reclamo, 30 gg costituzione, 20 gg memoria,
  appello, Cassazione) con giorni residui.

**Area 6 — Adempimenti periodici:**
- `commerc-checklist-redditi-pf` (6 cols) — documenti per
  dichiarazione persone fisiche (CU, oneri detraibili / deducibili
  con soglie normative).

### Added — Workflow assistant (5)

Narrative output che chiude ciascuna area con la relazione
professionale finale:

- `commerc-relazione-stima` (Area 1) — relazione di stima d'azienda
  per cessione / conferimento / fusione / asseverata / Art. 2343 c.c.
- `commerc-relazione-ctu-tributaria` (Area 2) — relazione CTU /
  perizia di parte tributaria.
- `commerc-relazione-att-33-ccii` (Area 3) — attestazione del
  professionista indipendente ex Art. 33 CCII (D.Lgs. 14/2019).
- `commerc-report-due-diligence` (Area 4) — executive summary +
  raccomandazioni SPA.
- `commerc-ricorso-tributario` (Area 5) — ricorso tributario di
  primo grado ex Art. 18 D.Lgs. 546/92.

### Added — DOCX templates (3)

A4 portrait, Calibri 11pt justify. `also_applicable_to` cross-domain
per le perizie e contenziosi che attraversano legal / insurance /
compliance:

- `it/relazione-stima-valore` — Area 1, 9 sezioni dal Premessa agli
  Allegati. `also_applicable_to: [legal, real_estate, insurance,
  compliance]`.
- `it/attestazione-art-33-ccii` — Area 3, 11 sezioni con la formula
  finale ATTESTAZIONE POSITIVA / NEGATIVA in grassetto centrato.
  `also_applicable_to: [legal, compliance]`.
- `it/ricorso-tributario` — Area 5, 8 sezioni conformi a D.Lgs.
  546/92: intestazione → fatto → motivi → richieste → valore →
  istanza cautelare opzionale → firma → allegati.
  `also_applicable_to: [legal]`.

### Verified at boot

  55 workflow-preset(s) loaded (was 31, +22 finance + +2 ribilanciamenti)
  9 docx-template(s) loaded (was 6, +3 finance)

Backend Tauri dev ha auto-rilevato ciascun file e ricaricato senza
manual intervention. Test `presets::*` continuano a passare.

---

## 2026-05-14 — Medico-legale toolkit + chat-history persistence + generated-doc cleanup

Three converging streams shipped on the same day, all addressing the
"will my work still be here tomorrow?" question that the assistant
chat keeps raising:

### Added — Medico-legale toolkit (11 workflow-presets + 1 DOCX template)

Maps the 7 operational modules of `docs/piano_toolkit_medico_legale.md`
into MikeRust's preset registry under the canonical `medical` domain.
All assets load automatically at the next backend boot. Designed to
chain: each workflow references the `DOC-NN` codes minted by the
inventario, so the entire perizia flows from one tabella inventario
through timeline → diagnosi → ITT / IP → relazione finale without
re-numbering or re-attaching documents.

- **Tabular** (`config/workflow-presets/medical/`):
  `medlegal-inventario-documenti` (Mod. 1, 8 cols),
  `medlegal-timeline-cronologica` (Mod. 2, 8 cols),
  `medlegal-diagnosi-strumentali` (Mod. 3b, 9 cols — the workflow
  that classifies each reperto DIRETTA / INDIRETTA / ESCLUSIVA /
  DA VALUTARE for the nesso causale),
  `medlegal-postumi-temporanei-itt` (Mod. 4, 7 cols, includes the
  equivalente-ITT-100% formula in the prompt),
  `medlegal-postumi-permanenti-rc` (Mod. 5a, 9 cols — covers SIMLA
  2016 / 2025 + micropermanenti Art. 138-139 D.Lgs. 209/2005 +
  Balthazard),
  `medlegal-danno-biologico-inail` (Mod. 5b, 6 cols — D.Lgs. 38/2000
  + DM 12 luglio 2000 / DM 9 aprile 2008, soglie capitale / rendita),
  `medlegal-invalidita-civile` (Mod. 5c, 6 cols — DM 5 febbraio
  1992, soglie 33% / 46% / 67% / 74% / accompagnamento).
- **Assistant** (narrative): `medlegal-diagnosi-ingresso` (Mod. 3a,
  scheda strutturata da verbale PS), `medlegal-diagnosi-dimissione`
  (Mod. 3c, scheda strutturata da lettera di dimissione),
  `medlegal-nesso-causale` (Sez. 6 della relazione, sei sotto-sezioni
  cronologico / topografico / continuità / esclusione / concausa /
  conclusione), `medlegal-quality-check` (Mod. 7, 10-point checklist
  pre-firma).
- **DOCX template** (`config/docx-templates/it/relazione-medico-legale.json`):
  A4 portrait, Calibri 11pt justify, 11 sezioni dal Premessa agli
  Allegati. `also_applicable_to: [legal, insurance]` perché CTU e
  perizie assicurative la usano trasversalmente. 8 required_metadata
  (PERITO_NOME, PERITO_QUALIFICA, INCARICO_TIPO, DATA_EVENTO,
  QUESITI, REGIME_APPLICABILE, IP_TOTALE, ITT_GIORNI_EQUIV).

Registry totals at boot: **31 workflow-presets** (was 20, +11
medical) and **6 DOCX templates** (was 5, +1 medical-legal).

### Fixed — Chat reopen no longer loses uploaded files / workflow chip / template chip

Migration `0021_messages_user_metadata.sql` plus surgical updates to
`stream_chat` and `get_chat` close the symmetric gap left by
migration 0020:

- New nullable JSON columns on `messages`: `files`, `workflow`,
  `template`. Same pattern as `annotations` (0012) and `events`
  (0020) — one JSON blob per non-text aspect of a message.
- `stream_chat` now also fixes a related bug: the user message
  used to be stored with the marker-augmented content
  (`[Workflow: …]` / `[Template: …]` prefix injected for the LLM),
  which surfaced as literal text on chat reopen. Stored content is
  now the raw user-typed text; markers stay in the LLM input only.
- `get_chat` reads the three new columns and attaches them to user-
  role entries in the response, matching the wire shape the
  frontend's `getChat` already expects — no frontend change needed.
- `tests/chat_history_smoke.rs` is new: two round-trip cases
  exercising the full files/workflow/template/events path against
  a tempdir SQLite.

### Fixed — Generated `.docx` now linked to its originating chat (cascade cleanup)

Pre-existing loophole: `exec_generate_docx` inserted into `documents`
without `chat_id`, so deleting the chat orphaned the file on disk.
Three pieces:

- `builtin_tools::dispatch` gains a `chat_id: Option<&str>`
  parameter forwarded to `exec_generate_docx`. The single chat call
  site passes `Some(&chat_id_clone)`; `None` is kept legal for a
  future REST-only tool surface.
- `INSERT INTO documents` in `exec_generate_docx` now binds
  `chat_id` so the FK cascade on chat deletion catches the row.
- `delete_chat` cleanup loop gains a second branch for
  non-hashed (generated) documents: each has a unique
  `documents/<user_id>/<doc_id>` storage path with no possible
  aliasing, so the file is freed unconditionally. Tracing now logs
  the split between cache-hash deletions and generated-doc
  deletions.

Net effect on the user-visible side: the toolkit produces relazioni
medico-legali whose DOC-NN inventario, timeline, diagnosi, ITT,
IP, nesso, and final `.docx` all persist together for the lifetime
of the chat, then disappear cleanly when the chat is deleted. No
orphans on either end.

---

## 2026-05-14 — ort load-dynamic resolved: onnxruntime 1.24.2 ABI match (critical RAG fix)

After three failed attempts that diagnosed the wrong cause, the
load-dynamic embedding path is finally **stable on Windows 11 ARM64
(Snapdragon X Elite) and x64**. The root cause was *not* the DLL
binary (we had Microsoft's official stock release), but an **ABI
version mismatch**: `ort 2.0.0-rc.12` statically compiles against
**onnxruntime 1.24.2**, while the vendored DLL we were shipping was
1.20.0. Four releases of drift in the function-pointer table caused
`TextEmbedding::try_new_from_user_defined` to deadlock silently at
the first missing symbol lookup — no error, no log, the
`spawn_blocking` just never returned.

### Fixed — RAG hang on first chat (Snapdragon X Elite)

- **Vendored DLLs bumped to onnxruntime 1.24.2** for `win-arm64`
  (SHA256 `D4C4D939…`) and `win-x64` (SHA256 `114947D6…`). Source:
  Microsoft's official `v1.24.2` release on GitHub. With matching
  versions, session init returns in **2269 ms ARM64 native** /
  **3248 ms x64 under Prism emulation** — equivalent to the
  previously-working static-link timing.
- **`ort/load-dynamic` re-enabled** (cf. commit `c781d57`,
  reversing the revert from `56ab9a1`). The runtime DLL is now a
  distributable artifact users can swap independently of the Rust
  toolchain. The earlier reverts had concluded "load-dynamic
  fundamentally hangs on ARM64" — that diagnosis was wrong; the
  fault was downstream of the load itself.
- **`ensure_onnxruntime_dylib_path()` call restored** in
  `src/lib.rs::run_server_with_channels()`, pre-AppState so the
  env-var is visible before EmbeddingService constructs its session.
- **Step 1/4 → 4/4 logging restored** in
  `src/embeddings/service.rs::ensure_model_ready()`. The cleanup
  commit `b8c25b8` had removed these as "diagnostic noise"; without
  them, the second hang attempt produced zero observable signal
  inside `spawn_blocking` (cargo test buffers stdout) and required
  `Get-Process` to confirm the binary was even alive. They stay in
  as the canonical diagnostic for any future version-drift regression.

### Added — diagnostic recipe

Knowing the **exact** onnxruntime version `ort-sys` linked against
is the only way to choose the right vendored DLL. The string
`branch=rel-1.24.2, git-commit=...` is baked into the compiled
`mike-tauri.exe` by pyke's CI build — extract it with:

```powershell
Select-String -Path target\debug\mike-tauri.exe `
  -Pattern 'branch=rel-\d+\.\d+\.\d+' -Encoding Default
```

This must be checked **every time `ort` / `fastembed` / `ort-sys` is
bumped**. ABI drift across more than one minor (1.20 → 1.24 is the
worst observed so far) is a silent deadlock, not a build error or a
runtime panic — there is no fail-fast.

### Verified

| metric | ARM64 native | x64 emulated (Prism) |
|---|---|---|
| `try_new_from_user_defined` | 2269 ms | 3248 ms |
| Cold embed (1 query) | 26 ms | 131 ms |
| Warm embed | 5 ms | 26 ms |
| Batch-16 passages | 81 ms (5 ms/passage) | 238 ms (14 ms/passage) |
| FP32 quality drift mean | 0.984 | — |
| Top-1 ranking agreement | 4/4 | — |

Test suite green on the new build: `embedding_perf` 3/3
(`perf_fp32_intfloat`, `perf_int8_xenova`, `quality_fp32_vs_int8`)
+ `workflows_smoke` 6/6 + `service::tests::find_onnxruntime_dylib`
6/6, plus the x64 cross-compiled `embedding_perf` executed under
Windows 11's Prism translation layer.

### Docs

- [`README.md`](README.md) — "Critical fix — ort/onnxruntime ABI
  version match" subsection in "RAG: hardware acceleration", listing
  the three changes that restored the pipeline and the upgrade
  protocol.
- [`libs/onnxruntime/README.md`](libs/onnxruntime/README.md) —
  "Version: must be exactly 1.24.2 for ort 2.0.0-rc.12" subsection
  + `branch=rel-` `Select-String` recipe + every download URL bumped
  from 1.20.x to 1.24.2.

### Changed — Cargo.toml

- `fastembed`: `default-features = false`, features `[hf-hub-native-tls,
  image-models, ort-load-dynamic]`.
- `ort = "=2.0.0-rc.12"`: `default-features = false`, features
  `[std, load-dynamic]`.

Both back to the shape commit `b565e08` originally introduced — the
revert that intervened (`56ab9a1`) was based on the misdiagnosis. The
Cargo.toml comment block documents both prior attempts so the next
person tempted to "ship the DLL separately" reads about the
*real* failure mode (ABI drift) instead of repeating the chase.

---

## 2026-05-14 — DOCX template wiring (Phase 1.A.2 — LLM tools + HTTP endpoint)

Glue between the renderer (Phase 1.A.1) and the rest of the system:
LLM tools to discover and use templates, an HTTP endpoint for the
frontend to download `.docx` files directly, and workflow → template
wiring so a workflow can declare "I produce a Diffida" and the
authoring contract flows through to the model in one round-trip.

### Added — LLM tools

- **`list_docx_templates(domain?, locale?)`** — returns every loaded
  template with id, display_name, category, domain, automation_level,
  required_metadata, and source_reference. Filters by canonical
  domain enum or locale prefix. First step of the workflow:
  "user wants to produce a structured doc → discover what's
  available".
- **`describe_docx_template(template_id)`** — returns the full
  authoring contract: the auto-generated `prompt_md` (composed
  programmatically from sidecar fields per `docs/TEMPLATE_PRONTUARIO.md`
  Parte V) plus the raw sidecar for introspection. Second step:
  "I picked the Diffida — how do I write it?". The model injects
  the returned `prompt_md` into its working context and then writes
  the body following the section_skeleton.
- **`generate_docx(body_md, template_id?, metadata?, title?)`**
  extended — when `template_id` is supplied, routes through the new
  `src/docx::render` pipeline with `metadata` as the `[PLACEHOLDER]`
  bag; when omitted, falls back to the legacy
  `markdown_to_docx(title, body)` path for backwards compat. Response
  includes `unresolved_placeholders` when any `[TOKEN]` was left
  unfilled, so the LLM can surface the gap to the user instead of
  pretending the document is complete.

### Added — `DocxTemplate::auto_generated_prompt_md(locale)`

Composes the closing-formatter contract entirely from structured
sidecar fields — layout (paper, margins, typography, footnotes),
placeholder syntax, required_metadata with per-field guidance from
`field_prompts`, the full section_skeleton with `[REPEATING BLOCK]`
markers for L3 templates, character_limits for atti difensivi, and
the author override `prompt_md_extra` if present. Closes with the
canonical "call `generate_docx(template_id="...", ...)`" instruction.

Means: editing the sidecar JSON propagates to the LLM prompt at the
next restart. Zero drift between what the engine renders and what
the LLM thinks it should write. Two unit tests anchor the output to
the shipped Diffida (lists DEBITORE/IMPORTO/TERMINE_GG, mentions
the canonical "DIFFIDA E METTE IN MORA" formula) and Parcella
(marks `voci_onorario` as `[REPEATING BLOCK]`).

### Added — Workflow → Template wiring

- `WorkflowPreset.default_output_template: Option<String>` — opt-in
  field on the workflow sidecar. When set on an `assistant`-type
  workflow, the chat handler doesn't need to pre-load anything —
  the existing `read_workflow` tool now bundles the linked template
  in its response when invoked.
- `read_workflow` extended: short-circuits to the preset registry
  first (previously DB-only — preset workflows would 404), and when
  the workflow has a `default_output_template`, attaches the
  template's id + display_name + automation_level + required_metadata
  + auto-generated prompt_md as `default_output_template.*` in the
  same JSON response. A `closing_instruction` field explicitly tells
  the LLM "this workflow ends with a `generate_docx` call".

### Added — `POST /docx-templates/{describe,render}`

Frontend-facing HTTP endpoints (LLM-tool-equivalent but for the UI):

- **`POST /docx-templates/describe`** — body `{ template_id, locale? }`,
  returns the same payload as `describe_docx_template` tool. Powers
  the workflow editor's "Preview authoring contract" affordance.
- **`POST /docx-templates/render`** — body `{ template_id, body_md,
  metadata, filename? }`, returns the rendered `.docx` bytes with
  `Content-Disposition: attachment` and a custom
  `X-Unresolved-Placeholders` header listing any `[TOKEN]` still
  present. The UI uses this for "anteprima" / "scarica subito" flows
  without persisting the doc in the user's documents list.

Template ids contain `/` (e.g. `it/diffida-messa-in-mora`) so we
pass them in the JSON body instead of the URL path — sidesteps
URL-encoding pitfalls in clients.

### Tests

- `auto_generated_prompt_md_contains_layout_and_skeleton` —
  anchors the prompt output to the real shipped Diffida sidecar.
- `auto_generated_prompt_md_marks_repeating_blocks` — verifies
  `[REPEATING BLOCK]` markup for L3 templates (Parcella).
- Updated `schemas_have_required_fields` to expect 7 tools (was 5).
- Updated `is_builtin_recognises_each_tool` to include the two new
  tool names.

Full crate test count: 296/296 green (the 2 new + the 294 baseline).

### What's still deferred to later

- The `read_workflow` extension only walks the preset registry for
  the workflow itself; the DB branch can't carry
  `default_output_template` yet because the SQL `workflows` table
  doesn't have a corresponding column. Phase 2 will add a migration
  + expose the field in the workflow editor.
- Frontend `/account/templates` page (preview / list / link from
  workflows) — Phase 2.

---

## 2026-05-14 — DOCX renderer (Phase 1.A.1 — end-to-end rendering)

The other half of the template subsystem: the actual pipeline that
turns a sidecar JSON + LLM-produced Markdown + a metadata bag into a
print-ready `.docx`. No `.dotx` intermediate — every template ships
as JSON only and the renderer builds styles.xml and document.xml
from scratch at request time. Aligns with the architectural
decision recorded yesterday: the tool is the closing formatter, not
a Word template management system.

### Added — `src/docx/` module

Five sub-modules, all pure-function, all zero-I/O on the hot path:

- **`it_helpers.rs`** — Italian-locale formatters: `format_italian_date`
  ("Cremona, 14 maggio 2026"), `format_italian_amount` ("€ 1.234,56"
  with dot-thousands / comma-decimal), `format_protocol_block`
  ("Prot. n. 12345/2026<TAB>Cremona, 14 maggio 2026" for PA letters).
- **`placeholders.rs`** — `[NAME]` substitution against a HashMap bag.
  Pure-text, UTF-8-safe (regression test guards against
  `bytes[i] as char` corrupting multi-byte sequences), with grammar
  `[A-Z0-9_.]+` so `[label](url)` Markdown links and lowercase
  brackets are correctly ignored. `find_remaining_tokens` reports
  the unfilled fields back to the renderer.
- **`styles_xml.rs`** — builds `word/styles.xml` from the sidecar's
  `typography` + `style_map_baseline` + `style_map`. Every conversion
  in one place: points → half-points, cm → twips
  (`cm_to_twips(3.5) == 1984` matches Word's own value),
  line-spacing multiplier → 240-base, alignment → `<w:jc>`. Style
  IDs are PascalCase ASCII (Word's grammar); `w:name` carries the
  localised Italian text. The 4 baseline styles (`BodyText`,
  `SectionHeading` with `<w:b/>` + `<w:caps/>` and +2pt bump,
  `Citation` with italic + 1.5cm indent, `Footnote` 10pt single)
  are always emitted; extra per-template styles inherit from
  `BodyText`.
- **`document_xml.rs`** — `pulldown-cmark` events → WML paragraphs.
  Headings 1/2/3 → `SectionHeading`, paragraphs → `BodyText`,
  ordered/unordered lists with manual marker (`•` / `1.`), bold +
  italic + code spans. Page size from `paper`, margins from
  `margins_cm` via `cm_to_twips`. Tables / footnotes / blockquotes /
  page-break directive deferred to Phase 2 (the 4 shipped templates
  don't need them yet).
- **`package.rs`** — zip the result with the canonical OOXML layout:
  `[Content_Types].xml`, `_rels/.rels`, `word/_rels/document.xml.rels`,
  `word/document.xml`, `word/styles.xml`. The three boilerplate
  fragments are const strings (they never vary).
- **`mod.rs`** — public API: `render(template, body_md, metadata) →
  RenderOutcome` with `bytes` + `unresolved_placeholders`. Pipeline
  is: substitute on the Markdown source (BEFORE the parser, so
  pulldown-cmark doesn't consume `[X]` as a CommonMark
  shortcut-reference link) → render body XML → wrap with page setup
  → build styles.xml → package zip. Missing required_metadata fields
  log a warning but the render proceeds anyway — gaps surface as
  `[UNFILLED_TOKEN]` in the document, which is the loudest possible
  proofread cue. The Markdown parser's text-event escape pass writes
  every text run with `xml_escape`, so values with `&`, `<`, `>`,
  apostrophe end up as proper XML entities (`&amp;`, `&lt;`, …).

### Tests

- 58 unit tests across the 5 sub-modules: every public function has
  at least one focused test for its contract; cross-cutting
  invariants (XML well-formedness, ASCII style IDs, UTF-8 round-trip,
  alignment-to-OOXML mapping) covered by dedicated tests.
- 3 integration tests in `src/docx/mod.rs::tests` exercising the
  full render against the shipped `it/diffida-messa-in-mora`
  template: end-to-end zip output with the canonical 5 parts,
  XML-special-char escaping in metadata values, and the
  unresolved-placeholder reporting on missing fields. All 294 crate
  tests green (the 58 new + the 236 pre-existing).
- Drive-by fix to `corpora::plugin::tests::walk_ancestors_finds_corpora_plugins`
  whose test fixture predated commit f9d6bf5 (which moved
  `corpora-plugins/` under `config/`).

### What's still deferred to later sub-phases

- Phase 2 in module: tables (GFM), footnotes (`[^N]`), blockquotes
  (`> testo` → `Citation` style), `---PAGE---` directive, YAML
  front-matter parser for cover-page metadata.
- Phase 1.A.2 (next commit): wire `generate_docx` builtin tool +
  `list_docx_templates` + `describe_docx_template` so the LLM can
  discover and use the registry; the `POST /docx-templates/:id/render`
  HTTP endpoint that streams the bytes.

---

## 2026-05-14 — DOCX template registry (Phase 1.A foundation)

First lap of the structured-output system. The DOCX template registry
becomes a first-class entity alongside workflows / column-presets /
model catalogue, with sidecar JSON files describing layout, authoring
contract, and automation level for each shipped template. Loaded at
startup, exposed via `GET /docx-templates`, ready for the renderer
pipeline (Phase 1.A.1, next lap).

### Added — DocxTemplate registry

- **`docs/TEMPLATE_PRONTUARIO.md`** — authoritative Italian-professional
  template specification (Versione 1.0, May 2026): 9 schede + 5 sotto-
  schede ad alto volume covering CTU, Atto difensivo, Comunicazione PA,
  Risposta AdE, Commercialista, Rogito notarile, Diffida, Locazione,
  Verbale, Istanza PA, Parcella, Procedura ISO, Ricorso tributario.
  Each scheda lists paper size, margins, typography, structure, layout
  conventions, automation level (L1-L4) and priority (★).
- **`src/presets/docx_template.rs`** — schema parser mirroring the
  workflow / column-preset registry pattern. Full type system:
  `Paper` (with `format: standard | uso_bollo`), `UsoBollo` (special
  notarial-deed constraints), `MarginsCm`, `Typography`, `Footnotes`,
  `SectionSkeletonEntry` (with `repeating: bool` for L3 automation),
  `CharacterLimits` (D.M. 110/2023 — atti difensivi),
  `FewShotExample`, `DocxTemplate` root. Style-map baseline (4
  canonical styles: `body_text`, `section_heading`, `citation`,
  `footnote`) hard-coded as default — every template inherits, can
  override. English snake_case canonical IDs everywhere (memory feedback),
  Word style names embedded inside the `.dotx` files stay localised.
- **`AppState.docx_templates`** populated at startup from
  `config/docx-templates/<domain>/<slug>.json`. Same fail-soft policy
  as the other registries: a broken template logs a warning and the
  rest still load.
- **`config_subdir(name)`** helper in `src/presets/mod.rs` for
  resolving any config subfolder that doesn't follow the
  `<kind>-presets` naming. Used by docx-templates (`config/docx-templates/`);
  generalises cleanly to future registries.
- **`GET /docx-templates`** route with optional `?domain=` and
  `?locale=` filter query params. Returns each template's full
  sidecar JSON plus synthesised `is_system: true` / `is_owner: false`.

### Added — first 4 shipped templates (★★★★★)

The four highest-priority templates from the Prontuario, all on disk
at `config/docx-templates/`:

- **`it/diffida-messa-in-mora`** (legal, L1) — Calibri 11 / 1.15 /
  margini 2.5 cm, 1 pagina, formula `DIFFIDA E METTE IN MORA` con
  termine perentorio in grassetto corsivo. 7 campi richiesti
  (`DEBITORE`, `CF_PIVA`, `INDIRIZZO`, `DESCRIZIONE_INADEMPIMENTO`,
  `IMPORTO`, `TERMINE_GG`, `ALLEGATO_PROVA`).
- **`it/parcella-professionale`** (finance, L3) — Calibri 11 / 1.15 /
  blocco americano. Voci di onorario come blocco ripetibile (L3),
  contributo integrativo 4% (avvocati) o 2% (commercialisti), IVA 22%
  con esenzione per regime forfetario, IBAN obbligatorio.
- **`it/contratto-locazione`** (real_estate, L2) — TNR 11 / 1.5 /
  margine sx 3 cm. Rami condizionali su `TIPO_CONTRATTO` ("Canone
  libero 4+4" / "Canone concordato 3+2" / "Contratto transitorio")
  che cambiano artt. 2 (durata) e 3 (aggiornamento ISTAT). 11 articoli
  fissi, parti in MAIUSCOLETTO, foro inderogabile.
- **`compliance/procedura-iso-sgi`** (compliance, L3) — Calibri 11 /
  1.15 / margini 2.5 cm uniformi. Header con `[AZIENDA] · CODICE ·
  REVISIONE · DATA`. Sezione 4.2.2 "Scheda processo" come blocco
  ripetibile L3 con 15 campi fissi (Nome, Responsabile, Documenti,
  Vincoli normativi, Input/Output, Indicatori, Rischi, Punti di forza,
  ecc.). Domain `compliance` estende il sistema oltre il legal-tech.

### Tests

- 9 unit test in `src/presets/docx_template.rs`:
  schema parsing (minimal + character_limits + display_name fallback),
  validation (domain enum, automation_level enum, uso_bollo block
  presence, placeholder_syntax enum), API-shape projection, and
  `shipped_templates_all_load_cleanly` — integration test that opens
  the real `config/docx-templates/` tree and verifies the 4 shipped
  templates are all parseable + valid. All green.

### Next

Phase 1.A.1 (next commit): the actual rendering pipeline —
`src/docx/` module that maps Markdown + YAML front-matter into the
companion `.dotx`, binds `[PLACEHOLDERS]`, and emits print-ready
`.docx`. Then the LLM tool `generate_docx` learns to accept
`template_id` and pull `prompt_md` from the sidecar.

---

## 2026-05-14 — ONNX Runtime: load-dynamic + every execution provider

The embeddings pipeline switches from the statically-baked
onnxruntime that fastembed used to ship to a fully **load-dynamic**
setup, with all 18 execution providers ort exposes now available
behind cargo feature flags.

### Changed — ONNX Runtime loading

- **`ort/load-dynamic` everywhere** — the onnxruntime native library
  is no longer downloaded by fastembed at build time and no longer
  statically linked into the binary. Instead the runtime DLL lives in
  `libs/onnxruntime/<platform>/` (vendored locally to the project, the
  same pattern already used for pdfium) and is loaded at process
  start via `ORT_DYLIB_PATH`. Three motivations:
  *(a)* reproducibility — the runtime version we test against is the
  one we ship; *(b)* sovereignty — no implicit dependency on whatever
  `onnxruntime.dll` lives in `system32`; *(c)* security — no
  `LoadLibrary` against the system search path means no DLL-hijack
  surface where a poisoned onnxruntime on PATH could be picked up
  before ours.
- **`ensure_onnxruntime_dylib_path()`** in
  [`src/embeddings/service.rs`](src/embeddings/service.rs) — the
  startup hook called from `run_server_with_channels()` before
  AppState is constructed. Walks cwd and exe ancestors looking for
  `libs/onnxruntime/<platform>/{onnxruntime.dll | libonnxruntime.so
  | libonnxruntime.dylib}` and exports the absolute path via
  `ORT_DYLIB_PATH`. Mirrors the pdfium discovery walk so dev runs
  (cwd = workspace root) and bundled runs (cwd = `src-tauri/`) both
  work without extra config.
- **`fastembed`** is now pulled in with `default-features = false`
  and only the `hf-hub-native-tls`, `image-models`,
  `ort-load-dynamic` features turned on — the previous default
  brought in `ort/download-binaries` which baked a CPU-only
  onnxruntime into every release build.

### Added — every ort execution provider as a cargo feature

Each ONNX Runtime execution provider exposed by ort 2.0.0-rc.12 now
has its own `rag-<ep>` cargo feature. Enabling a feature only
toggles the Rust API surface — the actual EP code lives in the
runtime DLL the user drops into `libs/onnxruntime/<platform>/`, so a
binary built with `--features rag-accel-all` works on a machine that
only has the CPU build of onnxruntime (ort silently skips providers
whose backend DLLs aren't loadable).

- **NPU class:** `rag-qnn` (Qualcomm Hexagon), `rag-cann`
  (Huawei Ascend), `rag-nnapi` (Android NN API), `rag-rknpu`
  (Rockchip), `rag-vitis` (AMD/Xilinx FPGA).
- **GPU class:** `rag-tensorrt`, `rag-cuda` (NVIDIA),
  `rag-migraphx`, `rag-rocm` (AMD), `rag-directml` (DX12 on
  Windows), `rag-coreml` (Apple Silicon ANE/GPU),
  `rag-webgpu` (cross-platform), `rag-openvino` (Intel
  CPU/iGPU/VPU).
- **Optimised-CPU class:** `rag-onednn` (Intel), `rag-acl` (ARM
  Compute Library), `rag-xnnpack` (mobile-class kernels),
  `rag-tvm` (Apache TVM).
- **Service class:** `rag-azure` (Cognitive Services off-load).
- **Convenience umbrellas:** `rag-accel-windows`, `rag-accel-linux`,
  `rag-accel-macos`, and `rag-accel-all` (every EP at once).

The `build_execution_providers()` helper in `src/embeddings/service.rs`
now registers every configured provider in the canonical
NPU → GPU → optimised-CPU → service order. ArmNN was retired
(removed from upstream ONNX Runtime — use ACL / XNNPACK / the
Kleidi-optimised CPU EP instead).

### Added — `libs/onnxruntime/` directory layout

- `libs/onnxruntime/{win-x64, win-arm64, linux-x64, linux-aarch64,
  macos-x64, macos-arm64}/` — one subdir per platform. Tracked in
  git as empty (`.gitkeep`) so the structure is discoverable on a
  fresh clone, but the runtime libraries themselves are
  **gitignored** (`*.dll`, `*.so`, `*.dylib`, `DirectML.dll`,
  `onnxruntime_providers_*.dll`, `Qnn*.dll`, archives) — each
  contributor fetches the variant matching their hardware.
- `libs/onnxruntime/README.md` — full layout reference + per-EP
  download recipes (CPU / DirectML / CUDA / TensorRT / OpenVINO /
  QNN / CoreML / ROCm) with PowerShell quick-fetch examples.
- README quickstart updated with a new "step 2" pointing at the
  onnxruntime fetch flow, alongside the existing pdfium step.

---

## 2026-05-13 — Plugin system, French locale, project refactor, dynamic port, migration auto-heal, domain column, JSON-preset registries, insurance vertical

The big day for declarative ingestion. The medium-term goal of the
project ("download legal documents locally via a plugin manifest") goes
from prose to running code: a JSON schema, a generic loader, a generic
HTTP-driven and a generic DILA-bulk-XML strategy, and a first
proof-of-concept consumer — CNIL — that imports ~26 000 délibérations
from the French Open Data archive into a local FTS5 index in a single
click. The same day brings a project-page refactor mirrored from
upstream `willchen96/mike`, the French locale, a UX polish pass on the
new corpus panel, two pieces of platform hardening in the afternoon
(the axum backend no longer binds to a hardcoded port, and sqlx
migration checksum drift now self-heals at startup instead of crashing
the app), and — in the late afternoon — a second wave that lifts the
workflow / column-preset built-ins out of TypeScript constants into
JSON files, adds a professional-vertical `domain` column across the
four config tables, ships the insurance vertical with 6 workflows
plus 17 column-presets, and polishes the picker modals so the user
can switch domain on the fly.

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

### Added — professional domain column (late afternoon)

- **`domain` column on 4 tables** (migration 0018) — `workflows`,
  `tabular_reviews`, `projects`, `documents` each gain a
  `TEXT NOT NULL DEFAULT 'legal'` column with a `(user_id, domain)`
  composite index. Mike-inherited rows backfill to `legal` (the
  upstream tool was law-firm-focused). Canonical 9-domain set
  validated at the API boundary (`crate::domain::DOMAINS`): `legal`,
  `medical`, `finance`, `real_estate`, `hr`, `insurance`, `ip`,
  `compliance`, `others`. No SQL CHECK — adding a future domain is a
  one-line edit to the Rust constant + the frontend `Domain` union +
  one i18n label per locale.
- **Backend filters** — list endpoints accept `?domain=…`; create
  endpoints accept an optional `domain` body field (normalised to
  `legal` when missing/invalid); update endpoints are strict (reject
  unknown values with 400, treat omitted as "leave unchanged"). The
  document upload path inherits from the parent project at INSERT
  time when `project_id` is set; standalone uploads accept the
  `domain` form field.
- **Frontend wiring** — `Domain` union + `DOMAINS` array constant in
  `shared/types.ts`; `MikeWorkflow` / `MikeProject` / `MikeDocument`
  / `TabularReview` gain an optional `domain` field. New
  `DomainSelect` + `DomainFilter` shared components,
  i18n-backed via the `Domains.values.*` namespace (it/en/fr, 9
  labels per locale). All 14 built-in workflows tagged
  `domain: "legal"`. Domain filter chip in the Workflows list page;
  domain dropdown in the new-workflow modal (above the practice
  chips).
- **Per-user default domain** (migration 0019) —
  `user_settings.default_domain TEXT` lets the user pick a
  preferred vertical once in Account → Generali (next to the
  language switcher). The `NewWorkflowModal`, `AddNewTRModal`,
  `AddColumnModal` and `WFEditColumnModal` all pre-select this
  default on every open instead of hard-coding `legal`. Persisted
  via `PUT /user/default-domain`; cached in `UserProfileContext`.

### Added — JSON-driven workflow & column-preset registries (late afternoon)

- **System-shipped workflow templates moved out of TypeScript** —
  the 14 built-in workflows that used to live as a 1244-line
  `BUILT_IN_WORKFLOWS` constant in
  `frontend/.../builtinWorkflows.ts` are now per-file JSONs under
  `config/workflow-presets/<domain>/<slug>.json`. The backend
  loads them once at startup into `AppState.workflow_presets`
  (mirror of the corpus-plugin pattern: ancestor-walking directory
  resolution, fail-soft parsing, override via
  `MIKE_WORKFLOW_PRESETS_DIR`). The `/workflow` list endpoint
  merges them with user-owned DB rows at response time
  (`is_system: true`, `user_id: null` for presets). The
  `/workflow/:id` handler short-circuits the lookup to the in-memory
  registry before falling through to the DB so the detail page
  works for built-ins too. Built-ins remain immutable from the API:
  update/delete handlers keep their `WHERE user_id = ?` filter, so
  a row with `user_id NULL` never matches.
- **Column-preset registry** — same pattern. The 13 column
  shortcuts that lived as a `PROMPT_PRESETS` array in
  `columnPresets.ts` move to `config/column-presets/<domain>/*.json`
  and serve over a new `/column-presets` endpoint. Regex pattern
  is split into `match_pattern` + `match_flags` so the JSON is
  portable; the frontend rebuilds `new RegExp(...)` at use time in
  `AddColumnModal` / `WFEditColumnModal`. Both consumers fetch on
  open and scope the auto-match (column-name → preset) to the
  active domain so a legal preset doesn't fire when the user is
  filling an insurance review.
- **`config/` consolidation** — three preset families that used to
  sit at the repo root (`corpora-plugins/`, `workflow-presets/`,
  `column-presets/`) now live together under `config/`. The
  ancestor-walking loaders look for `config/<dir-name>/` instead
  of `<dir-name>/`. Repo root stays tidy, future config families
  inherit the convention.
- **Picker modals — on-the-fly domain switch** — `AssistantWorkflowModal`
  (chat composer's "Aggiungi workflow"), `AddNewTRModal` (tabular
  template picker), `AddColumnModal` and `WFEditColumnModal`
  (column presets popover) each render a `DomainSelect` inline
  (full 9-domain set, sempre visibile). Selection persists during
  the modal session, resets to the user's saved default on every
  next open. Empty-slice case is allowed (no entries shown) so
  the user can scout other verticals freely.

### Added — insurance vertical (late afternoon)

First non-legal vertical shipped with real, useful content (driven
by the user's `docs/insurance-workflows-plan.md`):

- **3 tabular comparison workflows** — `RC Professionale Review`,
  `RC Prodotti Review`, `D&O Review`. Each has **24 columns**: 16
  common (Blocco A — assicuratore, contraente, decorrenza/scadenza,
  premio, massimali, franchigia/SIR, territorialità, condizioni di
  rinnovo, recesso infrannuale, liquidazione sinistri, subrogazione,
  legge applicabile, foro/ADR) plus 8 type-specific (RCP: trigger
  di copertura, retroattività, ultrattività, copertura sanzioni
  disciplinari, …; RCD: product recall, completed operations,
  copertura USA/Canada, …; D&O: Side A/B/C, esclusione condotte
  fraudolente con final-adjudication clause, …). Column prompts
  in Italian, style guard: cite article, "Non previsto" fallback,
  ISO dates, currency always specified.
- **3 assistant-type workflows** for the chat composer —
  `Riassunto copertura polizza` (single-policy 1-page Markdown
  brief in 8 sections), `Due Diligence assicurativa`
  (portfolio-level M&A review with inventory table, per-policy
  analysis, gap list prioritised 🔴/🟡/🟢, suggested
  rep&warranties for the SPA), `Inventario beni assicurati`
  (Property/All-Risks asset extraction → 9-column Markdown table
  with category, descrizione, identificativo, ubicazione, valore,
  criterio valutazione, sottolimite, note, plus a "Totale
  assicurato" sum check and red-flag section).
- **17 column-presets** under `config/column-presets/insurance/` —
  the 16 Blocco-A columns extracted as shortcuts plus "Esclusioni
  principali" (recurring across all three specific blocks). Regex
  patterns accept Italian and English variants
  (`insurer`/`assicuratore`, `deductible`/`franchigia`, …) so
  international users get the suggestion when typing in either
  language.

### Changed (late afternoon)

- Workflow list "Origine" column renamed `Mike` → `MikeRust` for
  system-shipped rows. The two sibling labels `Myself` / `Shared`
  that were also hardcoded got i18n'd to
  `Workflows.originSelf` / `originShared` in it/en/fr.

### Removed (late afternoon)

- `frontend/src/app/components/workflows/builtinWorkflows.ts`
  (1244 lines) and `frontend/src/app/components/tabular/columnPresets.ts`
  (104 lines) — single source of truth is now the JSON files on
  disk under `config/`.

### Security — defensive cleanup around the runtime DB (late evening)

Forensic verification of the GitHub remote (every branch, every
historical blob, `git log -p -S "AIzaSy"` over all refs) confirmed
**no `.db` file or API-key fragment has ever been pushed** — the
runtime DB lives outside the project tree by design
(`<user-home>/mikerust-data/mike.db`) and `.gitignore` had already
been excluding `*.db` and `data/`. Three defensive tidy-ups landed
anyway so the safeguard is explicit and survives future refactors:

- **Legacy `data/mike.db` removed from the working tree.** The stub
  was a leftover from the pre-MikeRust upstream layout (only migration
  0001 applied, no user data), but a SQLite file sitting in the
  project root is a `git add .` accident waiting to happen.
- **`.gitignore` annotated with a "DO NOT REMOVE" rationale** above
  the DB exclusion block, calling out that the runtime DB stores user
  API keys in plaintext and that `*.db` / `*.sqlite` / `data/` are
  the primary defence against accidental commits.
- **`.env.example` no longer suggests an in-repo DB path.** The
  `DATABASE_URL=sqlite://mike.db` line that shipped from upstream
  Mike would have landed the DB next to the executable (i.e. inside
  the project tree if run from the repo root). Now the variable is
  commented out so the code's secure default
  (`<user-home>/mikerust-data/mike.db`) wins by default; same
  treatment for `STORAGE_PATH`.

### Added — German, Spanish, Portuguese locales (evening)

- **`de.json`, `es.json`, `pt.json`** — full UI catalogues translated
  from the English source (767 lines each, every key present, no
  missing or extra strings). Lawyer-appropriate formal register: "Sie"
  in German, "usted" in Spanish, European Portuguese with formal
  pronouns. Practice-area labels and professional-domain values
  translated to natural local conventions (e.g. "Corporate" →
  "Gesellschaftsrecht" / "Mercantil" / "Direito Societário").
- **English fallback chain** — `src/i18n/request.ts` now deep-merges
  the active locale onto the English catalogue, so any string that
  hasn't been translated in a non-English locale silently falls back
  to English instead of rendering the literal key. Makes future
  catalogue additions safe: drop a new key into `en.json` and the UI
  works in all six locales immediately while translations catch up.
- **`/user/locale` whitelist** expanded to accept `de` / `es` / `pt`
  alongside the existing `it` / `en` / `fr`.
- **LanguageSwitcher** redesigned as a Radix dropdown with inline SVG
  flags for each locale. Emoji flags fall back to the bare regional
  indicator letters on Windows (no built-in flag font), so the
  component ships its own minimal hand-rolled SVGs (Italy, simplified
  Union Jack, France, Germany, Spain, Portugal) that render identically
  on every supported OS.

### Changed — Account profile layout (evening)

- All four Profile fields (Nome utente, Nome visualizzato, Lingua,
  Settore) now share a single layout pattern: block label above + flex
  row with the control at `flex-1` and a phantom 80px slot mirroring
  the Save button used by Nome visualizzato. Result: all controls end
  at the same x-coordinate inside the `max-w-md` container, so the
  panel looks ordered even though only Nome visualizzato actually
  carries a Save action.

### Fixed — chat model picker (evening)

- The chat ModelToggle was surfacing Anthropic models even when the
  user had no Claude API key saved, because the custom-model branch
  fired on the saved `claudeModel` field unconditionally. Same picker
  also fell back to the full preset list when `profile.llm` hadn't
  loaded yet. Both gated now — the picker only shows providers with a
  saved API key, mirroring the gating already applied to the Settings
  page provider toggle.

### Added — LLM model catalogue (evening)

- **`config/model.json`** — single source of truth for the LLM
  provider/model/region catalogue. Four providers (Anthropic, Google
  Gemini, OpenAI, Mistral) with their model lists, capability flags
  (`supports_vision`, `supports_tools`, `supports_prompt_cache`, …),
  and Gemini's 30-region matrix (11 Europe, 6 US, 6 Asia, plus
  Canada, South America, Oceania and the Middle East). Preview models
  carry a `preview: true` flag so consumers can force `region=global`
  automatically (preview deployments are global-only by spec across
  all three vendors).
- **`src/presets/model.rs` + `GET /models`** — typed loader and
  read-only route serving the catalogue. Same fail-soft policy as the
  workflow / column-preset registries: a missing or malformed
  `model.json` logs a warning and falls back to an empty catalogue
  rather than blocking startup.
- **Settings → Modelli LLM rewrite** — text inputs for model and
  region replaced with catalogue-driven `<select>` dropdowns. Picking
  a preview Gemini model auto-snaps the region back to "global" and
  disables the region selector. A "Custom" option preserves any
  pre-existing model id that isn't in the catalogue so configuration
  isn't silently lost on a catalogue refresh.
- **Gated active-provider toggle** — the four provider buttons in the
  "Provider attivo" group are now disabled (greyed out, lock icon,
  tooltip explaining why) for providers that don't have an API key
  saved. The local provider stays enabled as long as the base URL is
  configured. Clearing a saved key for the currently-active provider
  automatically falls back to the local provider so the chat picker
  doesn't try to use a credentialless cloud endpoint on the next turn.

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
