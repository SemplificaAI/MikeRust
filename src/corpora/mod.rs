//! Authoritative legal-corpus connectors (EUR-Lex, Retsinformation, ...).
//!
//! Each corpus implements `LegalCorpusAdapter`. The routes layer takes
//! the resulting `CorpusDocument`, runs it through the same hash-keyed
//! cache layout that chat-attachments use (`data/storage/cache/<sha256>.<ext>`
//! + `<sha256>.txt`), and indexes it via the existing embedding service.
//!
//! V1 ships with the EUR-Lex adapter only — see `eurlex` submodule.
//! The trait is shared so adding Retsinformation / Légifrance / etc.
//! later is a matter of dropping a new file in this module.

use anyhow::Result;
use async_trait::async_trait;

pub mod dila_bulk;
pub mod eurlex;
pub mod fedlex;
pub mod italian_legal;
pub mod limits;
pub mod manifest_adapter;
pub mod plugin;

/// Search hit returned by a corpus adapter — enough to render in a UI
/// list and round-trip back into `fetch()`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CorpusHit {
    /// Corpus-native identifier (CELEX, ELI, NOR, BOE-A-…). Opaque to
    /// us; the adapter that produced it is the one that resolves it.
    pub identifier: String,
    pub title: String,
    /// ISO-8601 date when known (publication date for legislation).
    pub date: Option<String>,
    /// Best canonical URL on the source site — what we'd link back to
    /// from the UI for "open on the original site".
    pub url: String,
    /// Languages the source claims this document is available in.
    /// Populated when cheap (EUR-Lex SOAP includes it); may be empty
    /// for adapters that don't surface it in search results.
    pub languages_available: Vec<String>,
}

/// Fully-fetched document, ready to be hashed + cached + embedded.
pub struct CorpusDocument {
    pub identifier: String,
    pub title: String,
    /// Source-provided date (publication/act date) when available.
    pub date: Option<String>,
    /// Language actually fetched. May differ from the user's request
    /// when the document wasn't available in the requested language
    /// and the adapter fell back to English (or a corpus-native
    /// fallback). `fetched_with_fallback` makes that explicit.
    pub language: String,
    pub fetched_with_fallback: bool,
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub source_url: String,
}

/// Common shape for any external legal-corpus connector.
#[async_trait]
pub trait LegalCorpusAdapter: Send + Sync {
    /// Stable corpus key written to `documents.corpus_id`. Lower-case,
    /// no spaces — used as a routing key and persisted in the DB.
    fn id(&self) -> &'static str;

    /// Languages the corpus serves, as ISO-639-1 lowercase codes.
    /// Single-language corpora (Retsinformation = `["da"]`) return
    /// a one-element slice.
    fn languages(&self) -> &[&'static str];

    /// Resolve a corpus-native identifier (CELEX, ELI, ...) to a hit
    /// we can fetch. Implementations may optionally probe the source
    /// to validate the identifier.
    async fn search_by_id(
        &self,
        identifier: &str,
        language: Option<&str>,
    ) -> Result<Vec<CorpusHit>>;

    /// Full-text search. May be unavailable on adapters that don't
    /// expose a search endpoint without authentication — those return
    /// an `Err` with a human-readable explanation.
    async fn search_by_keyword(
        &self,
        query: &str,
        language: Option<&str>,
        limit: usize,
    ) -> Result<Vec<CorpusHit>>;

    /// Fetch the full document. The adapter is responsible for the
    /// language-fallback logic — when the user's preferred language
    /// isn't available and `fallback_en` is true, the adapter tries
    /// English and sets `fetched_with_fallback = true` on the result.
    async fn fetch(
        &self,
        identifier: &str,
        language: Option<&str>,
        fallback_en: bool,
    ) -> Result<CorpusDocument>;
}
