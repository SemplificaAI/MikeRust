# MikeRust v0.6.3 — Tabular per-cell rate-limit retry + hourglass UI

Hotfix on top of v0.6.2. The process-global Mistral semaphore
landed in v0.6.2 capped concurrency to 1 RPS but didn't help when
a tabular review queues 24+ cells: cell #24 has been waiting 24s
by the time its turn comes, and if Mistral's quota window shifts
during that wait the backend's retry budget exhausts. The cell
lands as a red exclamation — visually identical to a permanent
error.

v0.6.3 adds a **frontend-side retry loop** on top of the two
existing backend layers.

## What's new

### Three-layer 429 protection

| Layer | Where | What it does |
|---|---|---|
| v0.6.1 | `src/llm/mistral.rs::post_with_retry` | Per-call retry-with-backoff (1s/2s/4s, 3 attempts, ~7s) |
| v0.6.2 | `src/llm/mistral.rs::MISTRAL_GATE` | Process-global Semaphore (1 permit, serialises all Mistral calls) |
| **v0.6.3** | `frontend/src/lib/api/tabular.ts::scheduleRateLimitRetry` | **Per-cell frontend retry, linear backoff** (5s × attempt, up to 10 attempts) |

When a `cell_update` SSE event arrives with `status: "error"` and
the content matches a 429-class signature (`\b429\b|rate[\s_]?limit`),
the frontend:

1. Rewrites the status to `rate_limited` (new transient state).
2. Schedules a retry via `POST /tabular-review/{id}/regenerate-cell`
   after `N × 5s` (attempt 1 = 5s, attempt 2 = 10s, attempt 3 =
   15s, …).
3. On 429 again → loops with the next backoff.
4. After 10 attempts (~275s cumulative) → surfaces the error
   permanently as red pill.

### Hourglass UI for transient state

Tabular cells in the new `rate_limited` state render as an
**Hourglass icon** in `--color-warning-700` (amber) instead of the
red `AlertCircle` for permanent errors. The cell's `title`
tooltip shows the retry countdown ("Tentativo 3/10 fra 15s") so
the user knows where they are in the recovery without having to
open the DevTools console.

### Diagnostic coverage

v0.6.2's `console.warn` hook only covered stream-level `error`
events. Per-cell errors come through `cell_update` events — the
user reported "non ho ricevuto alcun errore" in the console
despite seeing red pills, which is exactly this gap. v0.6.3
fixes it: every `cell_update` with `status === "error"` now logs
`[tabular] cell error:` + `{reviewId, rowId, columnKey}` to the
DevTools console.

Every scheduled retry also logs `[tabular] cell rate-limited,
scheduling retry` + attempt count + delay — the user can watch
the recovery happen in real time.

### Why linear (not exponential) backoff

Predictable mental model: "attempt N waits N × 5s, gives up after
~5 minutes total." Exponential (1s/2s/4s/8s/16s/32s/…) doubles
fast enough to feel like a hang after the first few retries.

## Tests

No new backend tests (this release is frontend-only). 24/24
`llm::mistral` from v0.6.2 stay green; the frontend changes are
covered by svelte-check (0 errors).

## Downloads

Pre-built MSIs for Windows:

- `MikeRust_0.6.3_x64.msi` — Windows x86_64
- `MikeRust_0.6.3_arm64.msi` — Windows ARM64, Snapdragon X Elite

Drop-in replacement for v0.6.2.

## License

MikeRust is distributed under **AGPL-3.0-only**. The Semplifica
wordmark and logo are trademarks; see `NOTICE.md`. The full
licence text is available in-app under **Settings → Licenza**.
