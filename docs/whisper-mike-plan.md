# Whisper integration — implementation plan (`whisper-mike` branch)

**Status**: design + scaffolding committed; full implementation pending sign-off on the open questions in §6.

## 1. Scope

**In scope (Phase 1, this branch)**

- Accept audio files (`wav`, `mp3`, `ogg`, `flac`, `m4a`) as chat
  attachments AND in local-folder sync.
- Transcribe via `whisper-rs` (whisper.cpp Rust binding) on first
  ingestion; persist transcript as the document's extracted text so
  it flows through the existing RAG / chat / citation pipeline
  unchanged.
- Render audio docs in the document viewer with an HTML5
  `<audio>` element + the transcript text alongside.

**Out of scope (Phase 2, separate branch)**

- Real-time microphone recording from the browser / Tauri webview.
- "Input device + volume" configuration modal.
- Voice activity detection, diarization (who-said-what).
- Streaming partial transcripts during transcription.

Rationale: the device picker + permission flow + browser audio
encoding triples the surface for a v1 whose primary value is "this
deposition / hearing / voice note is now searchable and citable in
chat." Files arrive as files in legal workflows anyway.

## 2. Backend pipeline

```
upload (.mp3 / .wav / .ogg / .flac / .m4a)
   │
   ▼
src/audio/decode.rs::decode_to_pcm_16khz_mono(bytes, ext)
   │       symphonia (mp3/ogg/wav/flac) → PCM s16/f32 → resample to
   │       16 kHz mono f32 (whisper.cpp requires this exact format)
   ▼
src/audio/transcribe.rs::transcribe(pcm, model_path)
   │       whisper-rs `WhisperContext::new(...)` (lazy-loaded once
   │       per process) → `state.full(params, pcm)` → collect
   │       per-segment text + timestamps
   ▼
TranscriptionResult { text, segments: [{ start_ms, end_ms, text }] }
   │
   ▼
extract_text_dispatch() in src/sync/scanner.rs
   │       new branch: "wav"|"mp3"|"ogg"|"flac"|"m4a" → call
   │       audio::transcribe, render text with `[T 14:32]`
   │       segment markers analogous to the existing `[Page N]`
   │       markers for PDFs
   ▼
existing pipeline: chunker → embedder → doc_chunks → RAG → citations
```

The segment markers (`[T MM:SS]` at the start of each whisper
segment) play the same role as `[Page N]` in PDFs: the chunker
preserves them in the chunk text, the citation builder strips them
from quotes via `strip_page_markers` (rename to
`strip_locality_markers`?), and the viewer uses them to scrub the
`<audio>` element to the cited timestamp.

## 2.1 Build prerequisite on Windows ARM64

`whisper-rs-sys` uses `bindgen` at build time, which needs a
`libclang.dll` matching the **target** architecture. On a Snapdragon
X Elite dev machine the default LLVM install (`C:\Program Files\
LLVM\`) ships **x86-64** binaries — bindgen rejects it for an ARM64
target with:

```
Unable to find libclang: invalid DLL (x86-64)
```

Options to unblock `cargo build --features audio-transcription` on
ARM64:

1. **Recommended** — install the ARM64 LLVM build from
   <https://github.com/llvm/llvm-project/releases> (`LLVM-<ver>-
   woa64.exe`). Set `LIBCLANG_PATH=C:\Program Files\LLVM-ARM64\bin`
   before the cargo invocation. Side-by-side with the x64 install
   is fine — different folders.
2. **Workaround** — target `x86_64-pc-windows-msvc` and rely on
   Prism emulation at runtime. Builds work, transcription works,
   but x64 native code under emulation costs ~25 % CPU per pass.
3. Until either is in place, the default build (no feature flag)
   stays clean — no consumer is impacted.

## 3. Dependencies (Cargo.toml)

Add behind a new `audio-transcription` feature so the 142 MB+ model
isn't pulled in by default builds:

```toml
[features]
audio-transcription = ["whisper-rs", "symphonia", "rubato"]

[dependencies]
whisper-rs = { version = "0.13", optional = true, default-features = false, features = ["log_backend"] }
symphonia  = { version = "0.5",  optional = true, default-features = false, features = ["mp3", "vorbis", "wav", "flac", "isomp4", "aac"] }
rubato     = { version = "0.16", optional = true }   # sample-rate conversion to 16 kHz
```

The `whisper-rs` crate vendors whisper.cpp via `bindgen` + `cc`.
On first build this compiles the C++ — adds ~30s to a clean build.
GPU acceleration features (`cuda`, `metal`, `coreml`, `hipblas`,
`vulkan`) gate behind further sub-features; CPU works everywhere.

## 4. Model file management

GGML model (whisper.cpp's format). Three sizes worth shipping for:

| Model       | Size   | Quality      | Use case                          |
|-------------|--------|--------------|-----------------------------------|
| `ggml-tiny` | 75 MB  | low          | quick draft / latency-sensitive   |
| `ggml-base` | 142 MB | usable       | default for legal / compliance    |
| `ggml-small`| 466 MB | good         | optional upgrade                  |

Bootstrap pattern (mirrors fastembed):

- Default model: `ggml-base.bin` (multilingual, handles `auto`
  language detection across our six locales).
- Cache path: `%USERPROFILE%/mikerust-data/whisper/<model>.bin`
  (out-of-tree like fastembed, to avoid Tauri watcher chatter).
- Download URL: `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-<name>.bin`
  (the canonical mirror).
- First transcription triggers download with progress events on
  `/sync/model-status` (extend the existing endpoint with a
  `whisper` key alongside `embedding`).
- Setting override: `WHISPER_MODEL` env var picks the model;
  `WHISPER_MODEL_PATH` lets advanced users point at a custom file.

## 5. Frontend changes

**ChatInput.svelte / sync picker**

- `UPLOAD_ACCEPT` gains `.wav,.mp3,.ogg,.flac,.m4a`.
- Attachment chip shows a 🎙️ icon when the file is audio; the chip
  tooltip notes the duration once the backend reports it.

**DocViewerPanel.svelte**

- New renderer kind `'audio'`. `rendererFor(blob, filename)`
  detects audio via mime / extension.

**AudioView.svelte** (new)

- Two-pane layout:
  - top: `<audio controls>` with the blob URL
  - bottom: transcript text in monospace, segments are clickable
    spans that seek the `<audio>` element to `start_ms`. The cited
    quote highlights via the existing `highlightCitation` helper
    (segment markers `[T MM:SS]` filter out exactly like
    `[Page N]` does — they're stripped from the haystack and quote
    via `strip_locality_markers`).

**Citation pill behaviour**

- Clicking a `[c3]` pill on an audio citation opens AudioView,
  scrubs the player to the segment's `start_ms`, and highlights the
  segment's text in the transcript pane. Same UX as a PDF citation
  scrubbing to a page, modulo audio plays instead of pages render.

## 6. Open questions (need sign-off)

These choices change the implementation non-trivially. Confirm
defaults below or override:

1. **Auto-download model on first use?**
   - DEFAULT: yes — same UX as fastembed (download ~142 MB once,
     progress bar). Alternative: ship the user `whisper/README.md`
     with manual placement instructions and hard-fail until the
     file is there.
2. **Default model size?**
   - DEFAULT: `ggml-base.bin` (142 MB, multilingual). Alternative:
     `ggml-small.bin` (466 MB, better quality) — adds latency on
     CPU but useful for hearings with cross-talk.
3. **Long-file handling**.
   - whisper.cpp processes 30s windows internally; a 60-minute
     deposition takes ≈ 5-10 min on M1 CPU and longer on x64. Two
     options:
     - DEFAULT: synchronous transcription with a progress event
       per 30s segment; the user sees motion and the upload row is
       in state `transcribing` until done.
     - Alternative: chunk the audio by silence (VAD) into 5-10
       minute pieces and transcribe in parallel — much faster but
       worse quality at chunk boundaries.
4. **Segment marker syntax in chunk text**.
   - DEFAULT: `[T MM:SS]\n` at the start of each whisper segment
     (mirrors the `[Page N]\n` convention so the existing
     citation/chunker code mostly carries over).
   - Alternative: keep markers internal and only expose the
     timestamp when the frontend opens the citation (cleaner chunk
     text, more code in the citation builder).
5. **GPU acceleration**.
   - DEFAULT for v1: CPU only (works on every Windows/Mac/Linux
     install, no extra DLL). Phase 2 can add the Vulkan/CUDA/Metal
     sub-features behind a build flag, similar to how `rag-directml`
     / `rag-qnn` are opt-in for embeddings.

## 7. Out-of-scope items deferred to Phase 2

- Live microphone recording (Tauri's webview supports
  `getUserMedia` with the right permissions in `tauri.conf.json`,
  but the encoder choice — Opus/WebM via `MediaRecorder`,
  re-decoded server-side via symphonia — is a non-trivial design).
- Input device picker + volume meter UI.
- Push-to-talk shortcut and chat-composer voice memos.
- Diarization (speaker labels).

The Phase 2 design is sketched in §1 above so when we tackle it the
audio pipeline already exists — we'll just feed live-captured PCM
into the same `transcribe()` entry point.

## 8. Implementation order (when sign-off lands)

1. Cargo deps + feature flag (smallest commit, validates the build
   compiles whisper.cpp end-to-end on Windows MSVC + ARM64).
2. `src/audio/decode.rs` + `src/audio/transcribe.rs` + a CLI smoke
   test (`cargo run --features audio-transcription --bin
   audio-smoke -- sample.mp3`).
3. Wire into `extract_text_dispatch` and the upload route MIME
   allow-list.
4. Frontend `UPLOAD_ACCEPT` + AudioView.svelte + `rendererFor`.
5. Model bootstrap + `/sync/model-status` extension.
6. Citation marker / viewer scrubbing wiring.
7. End-to-end test on a real audio file from the user's corpus.
