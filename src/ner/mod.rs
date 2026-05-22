//! Named-entity recognition + PII detection via GLiNER2.
//!
//! Optional. Compiled in only when the `ner-pii` feature is enabled —
//! without it this module is empty and downstream callers fall
//! through to "feature not compiled" branches.
//!
//! Pipeline (full spec: `docs/gliner2-pii-plan.md`):
//!
//! ```text
//! text + Option<&[label]>
//!   → engine::extract_entities(text, labels)
//!         Gliner2Engine::from_pretrained(SemplificaAI/gliner2-
//!         privacy-filter-PII-multi, Some("fp16_v2"), HF)
//!         singleton, lazy-loaded, fp16 inference on the same
//!         onnxruntime 1.20.0 DLL fastembed already uses
//!   → Vec<Entity { start, end, label, score, text }>
//! ```
//!
//! Default PII label set lives in `labels.rs`; callers can override
//! with their own subset (e.g. only `["fiscal_code","iban"]` for a
//! contract-redaction workflow). The engine is loaded once per
//! process and re-used across requests — the underlying
//! `Gliner2Engine` is `Send + Sync`.

#![cfg(feature = "ner-pii")]

pub mod engine;
pub mod labels;

pub use engine::{
    extract_entities, mask_pii, status, Entity, NerStatus, GLINER2_OVERLAP_CHARS,
    GLINER2_WINDOW_CHARS,
};
pub use labels::default_pii_labels;
