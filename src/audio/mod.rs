//! Audio transcription via whisper.cpp.
//!
//! Optional. Compiled in only when the `audio-transcription` feature
//! is enabled — without it this module is empty and callers fall
//! through to the "format not supported" branch in
//! `crate::sync::scanner::extract_text_dispatch`.
//!
//! The pipeline (full spec: `docs/whisper-mike-plan.md`):
//!
//! ```text
//! bytes (mp3/ogg/wav/flac/m4a/aac)
//!   → decode::decode_to_pcm_16khz_mono(bytes, ext)
//!         symphonia decode → rubato resample → f32 mono 16 kHz
//!   → transcribe::transcribe(pcm, &model)
//!         whisper-rs WhisperContext (lazily loaded singleton)
//!   → TranscriptionResult { text, segments: [{ start_ms, end_ms, text }] }
//! ```
//!
//! Segments carry `start_ms` / `end_ms` so the document viewer can
//! scrub the `<audio>` element to the cited passage; the chunk text
//! we feed the embedder gets a `[T MM:SS]\n` marker at each segment
//! boundary, mirroring the `[Page N]\n` convention from the PDF
//! extractor. The citation builder strips those markers before
//! showing the quote to the user — same path as PDFs.

#![cfg(feature = "audio-transcription")]

pub mod bootstrap;
pub mod decode;
pub mod model;
pub mod transcribe;

pub use bootstrap::{bootstrap, ensure_default_model, WhisperBootstrap, WhisperStatus};
pub use transcribe::{transcribe_audio, TranscriptionResult, TranscriptSegment};
