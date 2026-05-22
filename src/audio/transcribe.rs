//! Whisper-rs transcription pass.
//!
//! Singleton `WhisperContext` per model file: the GGML weights are
//! lazy-loaded once and reused across requests (loading takes ≥1 s
//! and 142 MB+ resident, doing it per file would be wasteful and
//! would also fragment the heap). A `OnceCell<Mutex<HashMap<PathBuf,
//! Arc<WhisperContext>>>>` keys the cache by absolute path so a
//! future "switch model" UI can hold both at once.

use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::bootstrap::ensure_default_model;
use super::decode::{decode_to_pcm_16khz_mono, DecodedAudio};

/// Per-segment slice of the transcript with millisecond timestamps —
/// what the document viewer needs to scrub the `<audio>` element to
/// the cited passage. Public so the extractor can stash segments
/// alongside the joined text for the citation layer.
#[derive(Debug, Clone)]
pub struct TranscriptSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Result of a full transcription pass.
#[derive(Debug, Clone)]
pub struct TranscriptionResult {
    /// Concatenation of every segment's text with `[T MM:SS]\n`
    /// markers at each segment start — mirrors the `[Page N]\n`
    /// convention from the PDF extractor so the chunker and the
    /// citation builder treat audio segments and PDF pages
    /// uniformly.
    pub text: String,
    pub segments: Vec<TranscriptSegment>,
    pub duration_ms: u64,
    /// IETF-style language tag the model auto-detected on the first
    /// 30s window (`it`, `en`, …). Useful for downstream UI hints.
    pub language: Option<String>,
}

/// Top-level entry point used by `crate::sync::scanner::extract_text_dispatch`.
/// Decodes the audio, runs whisper.cpp, returns the marker-annotated
/// transcript ready for the existing chunker/embedder pipeline.
///
/// First-use path triggers a HuggingFace download of the GGML weights
/// via `bootstrap::ensure_default_model`; subsequent calls find the
/// file in `<cache_dir>/<model_id>.bin` and return immediately. The
/// download status is observable on `GET /sync/whisper-status` for
/// the UI's progress bar.
pub async fn transcribe_audio(bytes: &[u8], ext: &str) -> Result<TranscriptionResult> {
    let model_path = ensure_default_model()
        .await
        .context("whisper model bootstrap")?;

    let bytes_owned = bytes.to_vec();
    let ext_owned = ext.to_string();
    // Heavy CPU work — keep it off the tokio runtime threads so other
    // requests stay responsive while a long file transcribes.
    tokio::task::spawn_blocking(move || run_pass(&bytes_owned, &ext_owned, &model_path))
        .await
        .map_err(|e| anyhow!("transcription task join: {e:?}"))?
}

fn run_pass(bytes: &[u8], ext: &str, model_path: &PathBuf) -> Result<TranscriptionResult> {
    let DecodedAudio {
        samples,
        sample_rate: _,
        duration_ms,
    } = decode_to_pcm_16khz_mono(bytes, ext)
        .context("audio decode failed")?;

    let ctx = get_or_load_context(model_path)
        .with_context(|| format!("loading whisper model {}", model_path.display()))?;

    let mut state = ctx.create_state().context("whisper state init failed")?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(None); // auto-detect — we want IT/EN/FR/DE/ES/PT to all work
    params.set_translate(false);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    // 4 threads is a safe baseline. We don't want to peg the user's
    // laptop while a chat is in flight — whisper.cpp scales sub-
    // linearly past 4 anyway. Make it configurable later.
    params.set_n_threads(4);

    state
        .full(params, &samples)
        .map_err(|e| anyhow!("whisper full pass failed: {e:?}"))?;

    let num_segments = state.full_n_segments();
    let mut segments = Vec::with_capacity(num_segments as usize);
    let mut text_buf = String::new();

    for i in 0..num_segments {
        let seg_text = state
            .full_get_segment_text(i)
            .map_err(|e| anyhow!("segment {i} text: {e:?}"))?;
        // whisper.cpp timestamps are in 10 ms units (centiseconds).
        let start_cs = state
            .full_get_segment_t0(i)
            .map_err(|e| anyhow!("segment {i} t0: {e:?}"))?;
        let end_cs = state
            .full_get_segment_t1(i)
            .map_err(|e| anyhow!("segment {i} t1: {e:?}"))?;
        let start_ms = (start_cs.max(0) as u64) * 10;
        let end_ms = (end_cs.max(0) as u64) * 10;
        let trimmed = seg_text.trim().to_string();
        if !text_buf.is_empty() {
            text_buf.push('\n');
        }
        text_buf.push_str(&format_marker(start_ms));
        text_buf.push('\n');
        text_buf.push_str(&trimmed);
        segments.push(TranscriptSegment {
            start_ms,
            end_ms,
            text: trimmed,
        });
    }

    // Language: whisper-rs exposes it via the state's full_lang_id.
    let language = state
        .full_lang_id()
        .map(|id| whisper_rs::get_lang_str(id).unwrap_or("").to_string())
        .filter(|s| !s.is_empty());

    Ok(TranscriptionResult {
        text: text_buf,
        segments,
        duration_ms,
        language,
    })
}

/// `[T MM:SS]` for the chunker / citation layer. Hours collapse into
/// minutes for any practical legal audio (a 4-hour hearing reads as
/// `[T 240:00]`); we keep this simple — fixed width keeps the
/// chunker's offsetting predictable.
fn format_marker(ms: u64) -> String {
    let total_secs = ms / 1000;
    let m = total_secs / 60;
    let s = total_secs % 60;
    format!("[T {m:02}:{s:02}]")
}

/// Singleton per (resolved) model path. WhisperContext is `Send +
/// Sync` (whisper.cpp guards internal state; whisper-rs reflects that)
/// so an `Arc` is enough.
fn get_or_load_context(path: &PathBuf) -> Result<Arc<WhisperContext>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<WhisperContext>>>> = OnceLock::new();
    let map = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let guard = map.lock().expect("whisper ctx cache poisoned");
        if let Some(ctx) = guard.get(path) {
            return Ok(ctx.clone());
        }
    }
    let path_str = path.to_string_lossy();
    tracing::info!("[whisper] loading model {}", path_str);
    let ctx = WhisperContext::new_with_params(&path_str, WhisperContextParameters::default())
        .map_err(|e| anyhow!("WhisperContext::new failed: {e:?}"))?;
    let arc = Arc::new(ctx);
    let mut guard = map.lock().expect("whisper ctx cache poisoned");
    guard.insert(path.clone(), arc.clone());
    Ok(arc)
}
