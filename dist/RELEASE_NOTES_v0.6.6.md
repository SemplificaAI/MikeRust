# MikeRust v0.6.6 — Mistral profile picker auto-saves on click

Hotfix on top of v0.6.5. The Mistral profile picker
(Veloce / Equilibrato / Premium) mutated only the local form
state — persistence required the user to click "Salva modifiche"
at the bottom of the section. Confusing because the highlight on
the picked profile *suggested* the choice was committed, but
re-entering Settings (or a fresh chat turn) would still use the
previously-saved role models.

## What's new

### Click-to-save

Clicking any of the three Mistral profile buttons now:

1. Updates the form state for immediate visual feedback (the
   "Attivo" badge moves to the new profile).
2. Calls `modelsStore.save({ main_model, title_model,
   tabular_model })` to persist all three role assignments in
   one PUT to `/user/llm-settings`.
3. Shows the success toast ("Profilo Veloce applicato" etc.).

On a save failure (network drop, backend 5xx) the form reverts
to the previous snapshot so the "Attivo" highlight keeps
pointing at what's actually persisted — no UI lying about state.

### Persistent highlight across sessions

Side benefit: because role assignments are now committed on every
profile click, when the user re-enters
**Settings → Modelli LLM** (or restarts MikeRust entirely) the
picker correctly highlights the last-chosen profile. The
`activeMistralProfile` derived state reads from `form.main_model`
etc., which initialises from `modelsStore.settings` on every
mount, which itself comes from the DB via
`GET /user/llm-settings`. So state survives:

* In-session navigation (close + reopen the Settings drawer)
* MikeRust restart
* Re-installation that preserves the data folder

## Tests

Single behaviour change, no schema migration, no API contract
change. svelte-check 0 errors. Backend untouched — v0.6.5's
Mistral stack carries over.

## Downloads

Pre-built MSIs for Windows:

- `MikeRust_0.6.6_x64.msi` — Windows x86_64
- `MikeRust_0.6.6_arm64.msi` — Windows ARM64, Snapdragon X Elite

Drop-in replacement for v0.6.5.

## License

MikeRust is distributed under **AGPL-3.0-only**. The Semplifica
wordmark and logo are trademarks; see `NOTICE.md`. The full
licence text is available in-app under **Settings → Licenza**.
