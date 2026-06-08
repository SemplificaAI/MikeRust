# MikeRust v0.6.2 — Mistral concurrency semaphore + JS console diagnostics

Hotfix on top of v0.6.1. The retry-with-backoff added there
addressed one-off 429 spikes but didn't fix the root cause:
MikeRust fires several Mistral calls per turn (main chat + title
generation + HyDE retrieval + tabular cell extraction), and on
the free Experiment tier (1 req/s) they all 429 simultaneously.
Retrying doesn't help when 8 callers retry at the same time —
they race again on the next attempt.

v0.6.2 adds the proper fix.

## What's new

### Process-global Mistral concurrency cap

A `tokio::sync::Semaphore` with **1 permit** now gates every
Mistral request. Every Mistral call across the whole MikeRust
process (chat / tabular / HyDE / title gen) acquires the permit
before issuing the HTTP request and releases it on completion.
Combined with v0.6.1's retry-with-backoff this ensures we never
exceed 1 RPS to Mistral regardless of how many MikeRust subsystems
try to call it concurrently.

The cap is **fair (FIFO)** so tabular cells process row-by-row in
the order the worker pool fires them — the per-row pill rendering
in the UI reads better in deterministic order than arbitrary
reorder.

The default of 1 is safe for the Experiment tier. For paid
Scale-tier users (4-8 RPS) it's mildly conservative; those users
rarely 429 anyway so the simpler "always 1" behaviour is the
right product trade-off for now. A future point release will
expose a `MISTRAL_MAX_CONCURRENT` knob for power users on paid
tiers.

### JS console diagnostics

Two `console.warn` hooks added to surface LLM errors in the
DevTools console alongside the existing UI banners:

* `[chat] LLM error: …` with `{ activeModel, chatId }` —
  fires from the chat SSE error callback.
* `[tabular] stream error event: …` with `{ reviewId, raw }`
  + a network-level twin `[tabular] generate stream failed: …`
  — fires from the tabular extraction stream loop.

Press **F12 → Console** while reproducing a 429 storm to see
the underlying Mistral error string. The visual treatment (chat
banner / per-cell red pill) is purposely terse for normal users;
the console hook gives power users the actual cause.

## Tests

Two new unit tests in `src/llm/mistral.rs`:
* `mistral_concurrency_cap_is_one` — pins the constant.
* `mistral_gate_serialises_acquirers` — verifies the semaphore
  actually blocks a second acquirer while the first holds the
  permit (uses `try_acquire` to keep the test deterministic).

24/24 `llm::mistral` green. 112/112 across the `llm::` tree.
svelte-check 0 errors.

## Downloads

Pre-built MSIs for Windows:

- `MikeRust_0.6.2_x64.msi` — Windows x86_64
- `MikeRust_0.6.2_arm64.msi` — Windows ARM64, Snapdragon X Elite

Drop-in replacement for v0.6.1.

## License

MikeRust is distributed under **AGPL-3.0-only**. The Semplifica
wordmark and logo are trademarks; see `NOTICE.md`. The full
licence text is available in-app under **Settings → Licenza**.
