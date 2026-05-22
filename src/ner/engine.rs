//! GLiNER2 engine singleton + entity extraction entry point.
//!
//! Pattern mirrors `crate::audio::transcribe::get_or_load_context`:
//! a process-wide `OnceLock<Mutex<Option<Arc<Gliner2Engine>>>>` so
//! the first call pays the ~500 MB model load + the FP16
//! WhisperContext-equivalent warmup, and every later call jumps
//! straight to inference. Heavy work runs on
//! `tokio::task::spawn_blocking` so the tokio worker pool stays
//! responsive while long documents are scanned.
//!
//! Cache layout: `hf-hub` (gliner2-rs transitive dep) drops the
//! weights under `$HF_HOME/...`. We set `HF_HOME` to
//! `%USERPROFILE%/mikerust-data/gliner2/` at server startup so the
//! ~500 MB model lives next to the other heavy artefacts (fastembed,
//! whisper) and the Tauri watcher never sees it.

use anyhow::{anyhow, Context, Result};
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;

use gliner2_inference::{Gliner2Engine, ModelType, SchemaTask};

use super::labels::default_pii_labels;

/// A single PII / named-entity span. `start` / `end` are byte
/// offsets into the original text (`text[start..end]` is verbatim
/// equal to `text` below).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Entity {
    pub start: usize,
    pub end: usize,
    pub label: String,
    pub score: f32,
    pub text: String,
}

/// Default HF model id for the privacy / PII task. Pinned here so a
/// future variant swap is a one-line change.
const PII_MODEL_ID: &str = "SemplificaAI/gliner2-privacy-filter-PII-multi";
const PII_MODEL_VARIANT: &str = "fp16_v2";

/// Top-level extraction entry point. `labels = None` uses the
/// canonical PII set in `labels.rs`; `Some(&[...])` lets the caller
/// scope detection (e.g. only banking identifiers for a contract
/// redaction workflow).
pub async fn extract_entities(
    text: &str,
    labels: Option<&[&str]>,
) -> Result<Vec<Entity>> {
    let engine = ensure_engine().await?;
    let owned_labels: Vec<String> = labels
        .map(|l| l.iter().map(|s| s.to_string()).collect())
        .unwrap_or_else(|| {
            default_pii_labels()
                .iter()
                .map(|s| s.to_string())
                .collect()
        });
    let text_owned = text.to_string();

    tokio::task::spawn_blocking(move || run_pass(engine, &text_owned, owned_labels))
        .await
        .map_err(|e| anyhow!("ner task join: {e:?}"))?
}

fn run_pass(
    engine: Arc<Gliner2Engine>,
    text: &str,
    labels: Vec<String>,
) -> Result<Vec<Entity>> {
    let tasks = vec![SchemaTask::Entities(labels)];
    let (entities, _relations, _classifications) = engine
        .extract(text, &tasks)
        .map_err(|e| anyhow!("gliner2 extract failed: {e:?}"))?;

    // `gliner2_inference::Entity` carries char offsets + label +
    // score; we re-emit it with a verbatim `text` slice for
    // ergonomics on the wire. The crate's field names may evolve;
    // when v0.6 lands, adjust this mapping and nothing else.
    let out: Vec<Entity> = entities
        .into_iter()
        .map(|e| Entity {
            start: e.start,
            end: e.end,
            label: e.label,
            score: e.score,
            text: text
                .get(e.start..e.end)
                .map(|s| s.to_string())
                .unwrap_or_default(),
        })
        .collect();
    Ok(out)
}

/// Lazy-loaded process-wide engine. Wraps the underlying
/// `Gliner2Engine` in an `Arc` so the worker closure can move a
/// clone into `spawn_blocking` without holding the mutex across the
/// await. The mutex itself only protects the *creation* path; once
/// the engine exists, every call goes through a cheap read.
async fn ensure_engine() -> Result<Arc<Gliner2Engine>> {
    static CELL: OnceLock<Mutex<Option<Arc<Gliner2Engine>>>> = OnceLock::new();
    let cell = CELL.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().await;
    if let Some(engine) = guard.as_ref() {
        return Ok(engine.clone());
    }
    tracing::info!(
        "[ner] loading GLiNER2 engine — model={} variant={}",
        PII_MODEL_ID,
        PII_MODEL_VARIANT
    );
    // `from_pretrained` is sync (downloads via hf-hub + builds the
    // ort session). We hold the mutex for the load — it's a one-time
    // cost per process; concurrent first-callers naturally serialise
    // and the second one finds the engine already cached.
    let engine = Gliner2Engine::from_pretrained(
        PII_MODEL_ID,
        Some(PII_MODEL_VARIANT),
        ModelType::HuggingFace,
    )
    .with_context(|| {
        format!("loading GLiNER2 model {PII_MODEL_ID} ({PII_MODEL_VARIANT})")
    })?;
    let arc = Arc::new(engine);
    *guard = Some(arc.clone());
    Ok(arc)
}
