# MikeRust v0.6.4 — Mistral 1-RPS spacing + global cursor-pointer

Two issues addressed.

## 1. Mistral 429 cascade — true rate-limiter, not just concurrency

The semaphore added in v0.6.2 caps **concurrency** (at most one
Mistral request in flight at a time) but doesn't cap
**throughput**. A fast call to Ministral 3B (~0.3s) followed by
the next gives ~2 RPS in the same wall-clock second, and Mistral
Experiment's token-bucket limiter (nominally 1 RPS) 429s the
second one. User logs from a 24-cell tabular review on v0.6.3
confirmed: cells fired sequentially through the semaphore but
~half still 429'd because they were spaced ~0.5s apart, not ~1s.

v0.6.4 adds a **minimum-spacing rate limiter** on top of the
v0.6.2 semaphore: the wall-clock instant of the last issued
Mistral request is tracked under a Mutex, and before each new
request the code sleeps until at least 1100ms have passed since
the previous one.

The 1100ms (vs nominal 1000ms) gives a 10% safety margin against
Mistral's bucket-refill granularity. Worst-case effective
throughput drops from 1.00 RPS to 0.91 RPS — irrelevant for the
chat composer (already pays multi-second LLM latency) and a
~10% overhead for tabular extraction (24 cells × 1.1s = 26s vs
24 × 1.0s = 24s) in exchange for essentially zero 429s.

Combined with the three retry layers shipped in v0.6.1 (backend
exponential backoff), v0.6.2 (process-global semaphore) and
v0.6.3 (frontend per-cell linear backoff), Experiment-tier 429s
should now be essentially invisible under normal use.

## 2. Cursor-pointer on every clickable element

Tailwind v4's preflight removed the default `cursor: pointer`
from `<button>` in some browser combinations. For a Tauri shell
aimed at non-technical legal users that's the wrong default —
the pointing-finger is the universal "this is clickable" signal
that survived 30 years of UX research.

v0.6.4 restores it globally via a single CSS rule in
`frontend/src/app.css` covering `<button>`, `<a href>`,
`<label for>`, `summary`, and all standard ARIA roles for
clickable surfaces (`role="button"`, `"link"`, `"menuitem"`,
`"tab"`, `"option"`). Excluded: `:disabled` + `[aria-disabled="true"]`
keep their `not-allowed` cursor.

## Tests

One new pinning test: `mistral_min_interval_is_at_least_one_second`
prevents a future PR from accidentally dropping below the 1 RPS
floor. 25/25 `llm::mistral` green. The cursor change is a single
CSS rule, manually verified across the sidebar, model picker,
tabs, file pills, attach menu, and settings toggles.

## Downloads

Pre-built MSIs for Windows:

- `MikeRust_0.6.4_x64.msi` — Windows x86_64
- `MikeRust_0.6.4_arm64.msi` — Windows ARM64, Snapdragon X Elite

Drop-in replacement for v0.6.3.

## License

MikeRust is distributed under **AGPL-3.0-only**. The Semplifica
wordmark and logo are trademarks; see `NOTICE.md`. The full
licence text is available in-app under **Settings → Licenza**.
