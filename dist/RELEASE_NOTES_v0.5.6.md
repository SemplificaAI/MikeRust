# MikeRust v0.5.6

End-to-end opt-in **"Modalità sicura locale"**: turns the local LLM
provider into a zero-config, loopback-only, two-curated-models
setup with thinking suppression baked in. Aimed at users who want a
CPU-only, air-gapped Italian-first chat without juggling Modelfiles
or context-window tuning by hand.

## What's new

### Modalità sicura locale — the toggle and what it does

Settings → Modelli LLM → "Locale (compatibile con OpenAI)" gains a
new toggle at the top. When ON:

* The Ollama base URL is locked to `http://localhost:11434`
  (loopback only — no LAN endpoints, no public IPs). Any other URL
  is refused with a clear error message.
* The chat composer's model picker collapses to only two curated
  entries: **Qwen 3.5 4B (rapido)** and **Gemma 4 E2B (rapido)**.
  Cloud providers are hidden from the picker even if their API
  keys are configured. Flipping the toggle off restores the full
  picker.
* Both curated models have **thinking explicitly disabled** via
  per-strategy Modelfile derivations (`/no_think` Qwen-native
  token for Qwen, `<think>` / `<thinking>` / `<reasoning>` stop
  sequences + "rispondi direttamente" preamble for Gemma 4). The
  user gets direct answers without the chain-of-thought block
  models in this size range tend to emit by default.
* The toggle auto-saves the instant you flip it (no separate
  Save button needed).

### Plug-and-play install / uninstall

Each curated entry shows display name, base model id, approximate
size, recommended RAM, and one of four states:

* **Installato** — green badge + a Trash "Rimuovi" button.
* **In corso** — disabled progress button ("Download… 42%",
  "Configurazione…", "Avvio…") + a brand-coloured progress bar
  tracking bytes-completed / bytes-total + a Cancel button.
* **Errore** — red message under the row, Install still clickable.
* **Da installare** — Install button with a download icon.

Click Install → MikeRust streams `ollama pull` progress in real
time, then creates the `mike-…-fast` Modelfile derivation that
bakes in the thinking-suppression configuration. All over the
network, no terminal required.

### Cancel + parallel downloads

* **Cancel button** during downloads: aborts the fetch → SSE drops
  → ollama-rs drops its pull → Ollama treats it as cancelled. The
  partial download stays in Ollama's SHA-256 layer cache so a
  later re-install resumes for free.
* **Parallel installs** already worked because each call carries
  its own fetch + reader + state — the Cancel button is what makes
  them feel symmetric (start any time, stop any time). Click
  Install on Qwen while Gemma is at 70% and both progress bars
  run side by side.

### Persistent install state

The progress map and per-id `AbortController` registry live in a
module-singleton store that survives the user navigating away from
Settings and back. The previous design wiped its state on
component unmount, the Install button re-enabled itself mid-pull,
and a second click fired a parallel ensure stream that raced the
original — fixed.

### "Ollama non rilevato" handling

If the heartbeat fails (Ollama not running on port 11434), the
section surfaces a warning panel with a link to
`ollama.com/download` and a Retry button — no need to leave
Settings to re-check.

## Incidental UX fixes from the same testing session

These landed in the same release because they were caught while
testing the secure-mode flow:

### Doc picker — project scope

"Sfoglia tutti" inside a project-scoped chat now restricts to the
project's documents via `?project_id=…`. Standalone chats keep the
global picker unchanged. Backend filter already existed in
`documents.rs::list_documents` — only the frontend was passing it
through.

### New chat from a project — confirm modal

Clicking `+` in the sidebar while a project-scoped chat is active
now opens a confirm modal:

> "Stai lavorando dentro un progetto. Vuoi mantenere il progetto
> associato alla nuova chat?"

Two action buttons: **Chat indipendente** / **Sì, mantieni il
progetto**, with implicit cancel via the modal X / Esc / backdrop
click. Also fixes the long-standing "chip persists silently"
behaviour where the project chip stuck around after `+` was
clicked without asking.

## Migration notes

* New schema migration **0032_user_local_secure_mode.sql** — adds
  `user_settings.local_secure_mode INTEGER DEFAULT 0`. Existing
  installs keep their custom Ollama URL and free-form model id
  (the toggle is **OFF by default**, retro-compat).
* No data is destroyed by the migration. Downgrade path: drop the
  column or leave it (older mike binaries ignore it).
* `ollama-rs = 0.3` + `async-stream = 0.3` are new direct
  dependencies. Both pure Rust, no native libs added.

## Downloads

Pre-built MSIs for Windows:

- `MikeRust_0.5.6_x64.msi` — Windows x86_64
- `MikeRust_0.5.6_arm64.msi` — Windows ARM64, Snapdragon X Elite
  native

Drop-in replacement for v0.5.5. To use the secure mode after
upgrading:

1. Open Settings → Modelli LLM.
2. Toggle "Modalità sicura locale" ON.
3. Click Install on either curated model (Qwen 3.5 4B is the
   lighter pick at 2.5 GB on disk; Gemma 4 E2B is the smarter one
   at 3.1 GB).
4. Once green-badged, start a new chat — the picker now shows
   the two curated entries; pick one and send.

## License

MikeRust is distributed under **AGPL-3.0-only**. The Semplifica
wordmark and logo are trademarks; see `NOTICE.md`. The full
licence text is available in-app under **Settings → Licenza**.
