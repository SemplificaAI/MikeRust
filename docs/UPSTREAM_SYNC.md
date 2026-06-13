# Upstream sync policy — willchen96/mike → MikeRust

MikeRust forked the upstream Mike project at TypeScript+Express+Supabase
stage and **replaced the entire backend** with Rust+axum+SQLite
(2026-05-08). The frontend was subsequently **rewritten clean-room in
Svelte 5 + Vite + Tailwind v4** (2026-05-17 — see HISTORY.md and
`docs/mikerust-ui-rewrite-plan.md`); the legacy Next.js/React UI was
removed from the repository. Both layers are therefore now original
work: the only remaining tie to upstream is the **AGPL-3.0 licence**
and the conceptual API contracts.

> **Sync implication:** because the frontend is no longer the upstream
> React code, "Apply"/"Adapt" of upstream *frontend* commits below now
> means **porting the behaviour** into the Svelte components, never
> cherry-picking React source. Shared-component fixes (citation
> rendering, doc viewer, chat input) map to their Svelte equivalents
> under `frontend/src/lib/components/`, not to inherited files.

This document records how to keep an eye on upstream without inheriting
divergence-noise, and the audit log of past sync passes.

## Cadence

Every **2–4 weeks**: skim the last ~30 commits on
[willchen96/mike `main`](https://github.com/willchen96/mike/commits/main)
and triage each one into:

1. **Apply** — security fix or frontend bug fix that maps cleanly onto
   MikeRust's surface. Worth a focused cherry-pick branch.
2. **Adapt** — useful UX change or refactor whose backend half can't
   land as-is (Express → axum, Postgres → SQLite, S3 → filesystem) but
   whose frontend half is portable. Open a tracking issue describing
   what to port and what to skip; do it as its own commit so blame
   stays clean.
3. **Skip** — backend infra changes that don't apply, hosting/deploy
   tweaks, dep bumps for Cloudflare/OpenNext, Postgres-specific schema
   work (e.g. JSONB filtering), or features already present in
   MikeRust under a different implementation.

## What to always check

- **Security fixes** (look for `fix(security):`, `CWE-`, "scope by
  access", "authorization", "boundary check"). Even if the upstream
  endpoint shape is different, the threat model often carries over —
  worth a 10-minute audit of the analog route in MikeRust.
- **Frontend bug fixes** in the components upstream calls
  `AssistantMessage` / `DocPanel` / `DocView` / `ChatInput` / citation
  rendering. MikeRust no longer shares that React source — the
  equivalents are the Svelte components under
  `frontend/src/lib/components/` (`chat/ChatMessage.svelte`,
  `documents/DocViewerPanel.svelte`, `chat/ChatInput.svelte`, the
  citation-pill renderer). Treat an upstream fix here as a **behaviour
  bug report**: reproduce it against the Svelte component and fix it
  there, don't port the React diff.
- **i18n key additions**. If upstream adds new English copy, mirror
  the IT translation.

## What to always skip

- **`backend/src/`** Express routes — not a 1:1 port to `src/routes/`.
- **`backend/schema.sql`** — Postgres specific; MikeRust uses SQLite
  migrations in `migrations/`.
- **`backend/nixpacks.toml`, `wrangler.toml`, OpenNext/Cloudflare
  configs** — MikeRust ships as a Tauri desktop app, no cloud deploy.
- **S3 path-style, R2, presigned URL changes** — MikeRust uses
  filesystem storage by default; S3 trait stub exists but isn't the
  hot path.
- **Supabase auth, Stripe billing, signup flows** — MikeRust is local
  only (PIN + Windows Hello), no cloud auth provider.
- **JSONB array filters** (`.contains("shared_with", […])` etc.) —
  Postgres-specific operator; MikeRust uses simple table rows.

## How to drive a sync session

```bash
# 1. List recent upstream commits
curl -s "https://api.github.com/repos/willchen96/mike/commits?per_page=40" \
  | python -c "
import json, sys
for c in json.load(sys.stdin):
    print(f\"{c['commit']['author']['date'][:10]} {c['sha'][:7]} \
{c['commit']['message'].splitlines()[0]}\")
"

# 2. Inspect a candidate commit's full diff
curl -s "https://api.github.com/repos/willchen96/mike/commits/<sha>" \
  | python -c "
import json, sys
c = json.load(sys.stdin)
print(c['commit']['message'])
print()
for f in c.get('files', []):
    print(f\"--- {f['filename']} (+{f['additions']}/-{f['deletions']})\")
    print(f.get('patch', '(no patch)'))
"

# 3. If it looks worth porting, open a branch
git checkout -b sync/upstream-YYYY-MM-DD

# 4. Audit, port, test, commit, push, PR. Reference the upstream sha
#    in the commit message so future archaeology is easy.
```

## Audit log

### 2026-05-13 — sync against upstream `2e8eafc` (PR #64, 2026-05-12)

Reviewed last 24 commits since the MikeRust fork point.

| Upstream | Outcome | Reason |
|---|---|---|
| `e261d2e` fix(security): scope tabular-review document_ids by access (CWE-639) | **N/A** | MikeRust's [`src/routes/tabular_reviews.rs`](../src/routes/tabular_reviews.rs) is a CRUD-only metadata stub (id, title, project_id, workflow_id, columns_config). The vulnerable surface — `PATCH /:id`, `POST /:id/regenerate-cell`, `POST /:id/generate`, and `POST /` accepting `document_ids` in body — does not exist. The full document-by-cell tabular pipeline upstream has, MikeRust does not (yet). When/if the cell-generation flow lands, lift `filterAccessibleDocumentIds` semantics from upstream `backend/src/lib/access.ts:132`. |
| `7062a30` fix project folder boundary checks | **N/A** | MikeRust has no `project_subfolders` table or `/projects/:id/folders` routes. Folder operations live in [`src/routes/sync.rs`](../src/routes/sync.rs) and operate on top-level OS paths under user control. No cross-project folder traversal surface exists. |
| `f39f175` Sync deployment and project page fixes (PR #64) | **Applied (Steps A–H minus header)** | After a meticulous diff (upstream post-refactor `ProjectPage.tsx` is 1689 lines, MikeRust's was 1872 — ~10% divergence, not the 3.6× originally feared), the refactor was ported in 7 sequenced commits on branch `sync/upstream-2026-05-12`:<br>• `b60feda` Steps A+B — layout helpers + `DocVersionHistory` extracted to `ProjectPageParts.tsx` (i18n preserved, no `depth` prop).<br>• `a967dab` Step C — `ProjectPageSkeleton` extracted. `ProjectPageHeader` deferred: MikeRust's header has substantial additions (RAG isolation owner-only toggle, export modal, share modal, members modal, i18n) that diverge from the upstream prop shape.<br>• `41ea283` Steps D+E — `ProjectAssistantTab` + `ProjectReviewsTab` extracted to own files; routes preserved.<br>• `ca4073b` Step G — backend `PATCH /project/:id/documents/:doc_id` + `renameProjectDocument` helper. Scope-reduced for MikeRust's leaner schema (no `documents.updated_at`, no `document_versions.display_name`). 7 Rust unit tests.<br>• `e9f4f4a` Step H — `ProjectsOverview` auth gating + cancel-token + `loadError` with new i18n keys `Projects.loadFailed`.<br>• `0ea5161` Step G part 2 — UI wiring: `Rename` entry added to RowActions on each project document, owner-only gated, inline-input rename pattern matching chats/reviews. Step F (`initialTab` prop) determined N/A: MikeRust already derives the tab from `searchParams.get("tab")` at ProjectPage.tsx:108-113, more flexible than the upstream prop approach.<br>Net effect: `ProjectPage.tsx` 1872 → 1621 lines (-13%), 3 new component files (Parts 281, AssistantTab 156, ReviewsTab 170), Rust route + 7 unit tests, robustness fixes on the overview, doc rename end-to-end. Header extraction left as a future task. Backend suite: 171/171 tests green. tsc --noEmit clean (only pre-existing unrelated errors). |
| `bef75b0` feat: add OpenAI model support | **Already done** | MikeRust supports OpenAI via [`src/llm/`](../src/llm/) (`openai.rs` is a peer of `anthropic.rs` / `gemini.rs`). |
| `91d0c2a` chore: update Next and Cloudflare deps | **Skip** | OpenNext/Cloudflare not used (Tauri shell). Plain Next.js bump is low-priority. |
| `625bca4` fix JSONB shared_with + path-style S3 | **Skip** | Postgres + S3 specific. MikeRust uses SQLite + filesystem; no JSONB; no S3 in the hot path. |
| `eb44140` fix(security): HMAC secret fail-fast | **Skip** | MikeRust does not use the upstream's download-URL HMAC pattern. |
| `ba6f771`, `7f5dd21` Sync security and backend profile updates | **Skip** | Express middleware (helmet, CSP headers, etc.) — axum stack uses different middleware library. Not a direct port. |
| `af5691e` Remove app legal pages | **Skip** | Upstream hosting concern; MikeRust ships as desktop app, no public legal pages. |
| `a84c1cc` docs: improve setup guidance | **Skip** | Upstream deployment docs; MikeRust has its own [`docs/MANUAL.md`](MANUAL.md). |

**Net delta to MikeRust**: zero code changes from this sync pass. The
upstream activity since fork point was overwhelmingly backend/infra
work that doesn't apply to MikeRust's stack, plus one frontend
refactor that's blocked on internal divergence.

The deferred ProjectPage refactor is the only meaningful follow-up to
consider — see the "Apply" path above when scheduling it.
