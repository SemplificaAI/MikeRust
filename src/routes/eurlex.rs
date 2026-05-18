//! EUR-Lex routes — V1.
//!
//! Three endpoints back the settings panel:
//!
//!   GET  /eurlex/config           → user's enabled flag + reference language
//!   PUT  /eurlex/config           → save same
//!   POST /eurlex/fetch            → fetch a CELEX, persist + index it
//!
//! V1 lookups are CELEX-based only (no full-text search — see
//! `corpora::eurlex` for the rationale). Once the user has the docs in
//! their personal index, they appear in chat retrieval like any other
//! document in the global RAG partition.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;

use crate::{
    auth::middleware::AuthUser,
    corpora::{eurlex::EurlexAdapter, CorpusHit, LegalCorpusAdapter},
    storage::make_storage,
    AppState,
};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"detail": msg})))
}

fn storage_root() -> PathBuf {
    PathBuf::from(
        std::env::var("STORAGE_PATH").unwrap_or_else(|_| "./data/storage".to_string()),
    )
}

const CORPUS_ID: &str = "eurlex";

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/config", get(get_config).put(put_config))
        .route("/search", post(search))
        .route("/fetch", post(fetch_celex))
        .route("/documents", get(list_documents))
        .route("/documents/{id}", delete(delete_document))
        .route("/documents/{id}/resync", post(resync_document))
        .route("/embed-progress", get(embed_progress))
}

// ---------------------------------------------------------------------------
// GET /eurlex/embed-progress — current chunk/total of the active embed job
// ---------------------------------------------------------------------------
//
// Returns `null` between jobs. While a sync is running, returns
// `{ document_id, current, total, percent }`. The frontend polls this
// every ~500ms when at least one row is in 'syncing' state.

async fn embed_progress(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> ApiResult {
    #[cfg(feature = "rag")]
    if let Some(emb) = state.embeddings.clone() {
        if let Some(p) = emb.embed_progress().await {
            let percent = if p.total == 0 {
                0
            } else {
                ((p.current as f64 * 100.0) / p.total as f64).round() as i64
            };
            return Ok(Json(json!({
                "document_id": p.document_id,
                "current": p.current,
                "total": p.total,
                "percent": percent,
            })));
        }
    }
    let _ = state;
    Ok(Json(json!(null)))
}

// ---------------------------------------------------------------------------
// GET /eurlex/config
// ---------------------------------------------------------------------------

async fn get_config(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let row: Option<(i64, Option<String>, i64)> = sqlx::query_as(
        "SELECT enabled, language, fallback_en FROM corpus_settings \
         WHERE user_id = ? AND corpus_id = ?",
    )
    .bind(&auth.user_id)
    .bind(CORPUS_ID)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (enabled, language, fallback_en) = row
        .map(|(e, l, f)| (e != 0, l, f != 0))
        .unwrap_or((false, Some("en".to_string()), true));

    Ok(Json(json!({
        "enabled": enabled,
        "language": language.unwrap_or_else(|| "en".to_string()),
        "fallback_en": fallback_en,
    })))
}

// ---------------------------------------------------------------------------
// PUT /eurlex/config
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ConfigPayload {
    enabled: bool,
    language: Option<String>,
    fallback_en: Option<bool>,
}

async fn put_config(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<ConfigPayload>,
) -> ApiResult {
    let language = body
        .language
        .as_deref()
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "en".to_string());
    let fallback_en = body.fallback_en.unwrap_or(true);

    sqlx::query(
        "INSERT INTO corpus_settings (user_id, corpus_id, enabled, language, fallback_en, updated_at) \
         VALUES (?, ?, ?, ?, ?, datetime('now')) \
         ON CONFLICT(user_id, corpus_id) DO UPDATE SET \
           enabled = excluded.enabled, \
           language = excluded.language, \
           fallback_en = excluded.fallback_en, \
           updated_at = excluded.updated_at",
    )
    .bind(&auth.user_id)
    .bind(CORPUS_ID)
    .bind(body.enabled as i64)
    .bind(&language)
    .bind(fallback_en as i64)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({
        "enabled": body.enabled,
        "language": language,
        "fallback_en": fallback_en,
    })))
}

// ---------------------------------------------------------------------------
// POST /eurlex/search — { query, language? } → list of probed hits
// ---------------------------------------------------------------------------
//
// Takes whatever the user typed (CELEX, "Direttiva 2014/24/UE", or just
// "2014/24") and returns the list of candidate documents that EUR-Lex
// actually serves in the requested language. The frontend renders each
// hit with a "Sync" button that calls /eurlex/fetch on the chosen CELEX.

#[derive(Deserialize)]
struct SearchPayload {
    query: String,
    language: Option<String>,
}

#[derive(serde::Serialize)]
struct SearchResponse {
    hits: Vec<CorpusHit>,
    note: Option<String>,
}

async fn search(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<SearchPayload>,
) -> ApiResult {
    let query = body.query.trim();
    if query.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Query vuota."));
    }

    let stored_lang = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT language FROM corpus_settings WHERE user_id = ? AND corpus_id = ?",
    )
    .bind(&auth.user_id)
    .bind(CORPUS_ID)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .and_then(|(l,)| l)
    .unwrap_or_else(|| "en".to_string());
    let lang = body
        .language
        .clone()
        .unwrap_or(stored_lang)
        .to_ascii_lowercase();

    let adapter = EurlexAdapter::new();

    // Auto-detect intent. If the query matches a CELEX / year-number /
    // ELI shape, treat it as an identifier lookup (probe candidates,
    // confirm in-language). Otherwise fall back to a keyword search
    // on EUR-Lex's public search page — which returns CELEX hits we
    // can then surface for the user to pick from.
    let candidates = EurlexAdapter::enumerate_celex_candidates(query);
    let mode_label = if candidates.is_empty() { "keyword" } else { "identifier" };
    tracing::info!(
        "[eurlex] /search query={:?} lang={} mode={}",
        query,
        lang,
        mode_label
    );

    let hits = if !candidates.is_empty() {
        adapter
            .search_by_id(query, Some(&lang))
            .await
            .map_err(|e| err(StatusCode::BAD_GATEWAY, &e.to_string()))?
    } else {
        adapter
            .search_by_keyword(query, Some(&lang), 20)
            .await
            .map_err(|e| err(StatusCode::BAD_GATEWAY, &e.to_string()))?
    };

    let note = if hits.is_empty() {
        Some(if mode_label == "identifier" {
            format!(
                "Nessun atto EUR-Lex trovato per '{}' in {}. Verifica CELEX o anno/numero.",
                query,
                lang.to_uppercase()
            )
        } else {
            format!(
                "Nessun risultato EUR-Lex per '{}' in {}. Prova parole più specifiche \
                 o usa direttamente un CELEX se lo conosci.",
                query,
                lang.to_uppercase()
            )
        })
    } else {
        None
    };

    let resp = SearchResponse { hits, note };
    Ok(Json(serde_json::to_value(resp).unwrap()))
}

// ---------------------------------------------------------------------------
// POST /eurlex/fetch — { celex, language? } → fetched + indexed document
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct FetchPayload {
    celex: String,
    language: Option<String>,
    date: Option<String>,
}

async fn fetch_celex(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<FetchPayload>,
) -> ApiResult {
    // Pick up user's stored config so a missing `language` in the
    // request body falls back to whatever the user picked in settings.
    let cfg: Option<(Option<String>, i64)> = sqlx::query_as(
        "SELECT language, fallback_en FROM corpus_settings \
         WHERE user_id = ? AND corpus_id = ?",
    )
    .bind(&auth.user_id)
    .bind(CORPUS_ID)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let stored_lang = cfg
        .as_ref()
        .and_then(|(l, _)| l.clone())
        .unwrap_or_else(|| "en".to_string());
    let stored_fallback = cfg.map(|(_, f)| f != 0).unwrap_or(true);
    let lang = body
        .language
        .clone()
        .unwrap_or(stored_lang)
        .to_ascii_lowercase();

    // Dedupe by (corpus_id, identifier) — if the user already indexed
    // this CELEX (in any language), surface the existing row instead
    // of re-fetching. Re-fetching the same CELEX in a *different*
    // language is a separate concern handled by upserting based on
    // (corpus_id, identifier, corpus_language).
    let existing: Option<(String, String, Option<String>, Option<String>, Option<String>, i64)> =
        sqlx::query_as(
            "SELECT id, filename, corpus_language, corpus_date, storage_path, fetched_with_fallback \
             FROM documents \
             WHERE user_id = ? AND corpus_id = ? AND corpus_identifier = ? AND corpus_language = ?",
        )
        .bind(&auth.user_id)
        .bind(CORPUS_ID)
        .bind(&body.celex)
        .bind(&lang)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
    if let Some((id, filename, language, date, _, fb)) = existing {
        if body.date.as_ref().is_some_and(|d| !d.trim().is_empty()) {
            let _ = sqlx::query(
                "UPDATE documents SET corpus_date = COALESCE(corpus_date, ?) \
                 WHERE id = ? AND user_id = ?",
            )
            .bind(body.date.as_deref().map(str::trim))
            .bind(&id)
            .bind(&auth.user_id)
            .execute(&state.db)
            .await;
        }
        return Ok(Json(json!({
            "id": id,
            "filename": filename,
            "corpus_id": CORPUS_ID,
            "corpus_identifier": body.celex,
            "corpus_language": language,
            "corpus_date": date,
            "fetched_with_fallback": fb != 0,
            "already_indexed": true,
        })));
    }

    // Fetch + scrape via the adapter.
    let adapter = EurlexAdapter::new();
    let fetched = adapter
        .fetch(&body.celex, Some(&lang), stored_fallback)
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, &e.to_string()))?;

    let corpus_date = body
        .date
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(str::to_string)
        .or(fetched.date.clone());

    // Refuse to persist a phantom document. A real CELEX served in
    // any language is at least several thousand chars (even short
    // Court orders run past 1 KB); anything below 1 KB is almost
    // certainly an EUR-Lex stub page that slipped past the
    // extractor's stub-marker check (we've seen this happen with
    // older fetches before the multi-URL fallback chain landed).
    // Without this guard, the row goes in at size=0, the user sees
    // a "indicizzato" badge, and the next time the model tries to
    // read the doc it gets a "not found" because there's nothing
    // to read.
    if fetched.bytes.len() < 1024 {
        tracing::warn!(
            "[eurlex] refusing to persist {} ({}, {} bytes) — too small to be a real act, \
             likely an EUR-Lex stub. The user can retry; subsequent fetches \
             often succeed once EUR-Lex's caches warm.",
            fetched.identifier,
            fetched.language,
            fetched.bytes.len()
        );
        return Err(err(
            StatusCode::BAD_GATEWAY,
            &format!(
                "EUR-Lex ha restituito un body troppo piccolo per essere il testo dell'atto \
                 ({} byte). Spesso è una pagina di fallback intermedia — riprova tra qualche \
                 secondo.",
                fetched.bytes.len()
            ),
        ));
    }

    // Hash the bytes so two users / two languages of the same act
    // dedupe on disk. We reuse the same `cache/` layout as chat
    // attachments (see docs/CACHE.md) so the chat retrieval fast-path
    // works uniformly: storage_path = cache/<hash>.txt,
    // extracted_text_path = same.
    let hash = {
        let mut h = Sha256::new();
        h.update(&fetched.bytes);
        format!("{:x}", h.finalize())
    };
    let bin_key = format!("cache/{}.txt", hash);

    let storage = make_storage()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let bin_abs =
        storage_root().join(bin_key.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !bin_abs.exists() {
        storage
            .put(&bin_key, &fetched.bytes, "text/plain; charset=utf-8")
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    }

    let doc_id = uuid::Uuid::new_v4().to_string();
    let filename = format!("{} ({}).txt", fetched.title, fetched.language.to_uppercase());
    let size = fetched.bytes.len() as i64;

    // Insert with status='syncing' so the UI can surface progress.
    // Once chunking + embedding completes, we UPDATE to 'ready'. If
    // anything below errors, status stays 'syncing' (we don't roll
    // back) and the resync endpoint can pick it back up — but to make
    // recovery explicit, the catch-block at the end of this function
    // flips it to 'interrupted' on failure.
    sqlx::query(
        "INSERT INTO documents \
           (id, user_id, project_id, filename, file_type, size_bytes, \
            storage_path, status, content_hash, extracted_text_path, \
            corpus_id, corpus_identifier, corpus_language, corpus_date, fetched_with_fallback) \
         VALUES (?, ?, NULL, ?, 'txt', ?, ?, 'syncing', ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&doc_id)
    .bind(&auth.user_id)
    .bind(&filename)
    .bind(size)
    .bind(&bin_key)
    .bind(&hash)
    .bind(&bin_key) // extracted_text_path = same: the source IS plain text
    .bind(CORPUS_ID)
    .bind(&fetched.identifier)
    .bind(&fetched.language)
    .bind(&corpus_date)
    .bind(fetched.fetched_with_fallback as i64)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    tracing::info!(
        "[eurlex] indexed CELEX {} ({}, fallback={}) as doc {}",
        fetched.identifier,
        fetched.language,
        fetched.fetched_with_fallback,
        doc_id
    );

    // Chunk + embed synchronously, flipping the status on success.
    // On failure we mark 'interrupted' so the resync endpoint and the
    // UI can recover without losing the row (which would re-trigger a
    // fetch from EUR-Lex).
    //
    // The chunk's `source_path` MUST be the local filesystem path the
    // DocPanel can read back via `/sync/kb-doc?path=...`. Storing the
    // EUR-Lex URL here (the obvious-looking choice) made the
    // citation-pill viewer fail with "Failed to load document":
    // `kb-doc` does `std::fs::read(path)`, which can't take a URL.
    // Use the absolute path of the cache file we just wrote.
    let text = String::from_utf8_lossy(&fetched.bytes).into_owned();
    let chunk_source_path = bin_abs.to_string_lossy().to_string();
    let (chunks_indexed, indexing_error, final_status) =
        run_indexing(&state, &auth.user_id, &doc_id, &chunk_source_path, &text).await;

    sqlx::query("UPDATE documents SET status = ? WHERE id = ?")
        .bind(&final_status)
        .bind(&doc_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({
        "id": doc_id,
        "filename": filename,
        "corpus_id": CORPUS_ID,
        "corpus_identifier": fetched.identifier,
        "corpus_language": fetched.language,
        "corpus_date": corpus_date,
        "fetched_with_fallback": fetched.fetched_with_fallback,
        "source_url": fetched.source_url,
        "size_bytes": size,
        "already_indexed": false,
        "chunks_indexed": chunks_indexed,
        "indexing_error": indexing_error,
        "status": final_status,
    })))
}

/// Run chunking + embedding for a document. Returns
/// `(chunks_indexed, error_message, final_status)` where
/// `final_status` is one of `"ready"` or `"interrupted"`.
///
/// When the rag feature is compiled in and an embedding service is
/// available, we treat `chunks_indexed == 0` as an error — a row with
/// zero chunks is unreachable to RAG retrieval, so flipping it to
/// 'ready' would be a silent failure: the inventory line says the doc
/// is available but every query about it comes back empty. The user
/// can recover by Riavvia (re-runs `index_document` against the same
/// cached text) or Delete + re-fetch.
async fn run_indexing(
    state: &AppState,
    user_id: &str,
    doc_id: &str,
    source_path: &str,
    text: &str,
) -> (usize, Option<String>, String) {
    #[cfg(feature = "rag")]
    {
        if let Some(emb) = state.embeddings.clone() {
            return match emb
                .index_document(user_id, None, doc_id, source_path, text)
                .await
            {
                Ok(0) => {
                    // Text was on disk and >= 1024 bytes (fetch_celex's
                    // guard already enforced that), yet the chunker
                    // produced nothing. This usually means the body
                    // turned out to be markup we couldn't parse cleanly
                    // — leaving status=ready would silently mislead
                    // the model into citing a doc with no retrievable
                    // content.
                    tracing::warn!(
                        "[eurlex] {}: indexer produced 0 chunks despite valid text \
                         ({} chars). Marking interrupted so the user can resync.",
                        doc_id,
                        text.len()
                    );
                    let msg = format!(
                        "Indicizzazione completata ma nessun chunk creato \
                         (testo: {} caratteri). Probabile problema con il chunker; \
                         usa Riavvia per ritentare.",
                        text.len()
                    );
                    (0, Some(msg), "interrupted".to_string())
                }
                Ok(n) => {
                    tracing::info!(
                        "[eurlex] indexed {} into {} chunk(s)",
                        doc_id,
                        n
                    );
                    (n, None, "ready".to_string())
                }
                Err(e) => {
                    tracing::warn!(
                        "[eurlex] embedding for {} failed: {}",
                        doc_id,
                        e
                    );
                    (0, Some(e.to_string()), "interrupted".to_string())
                }
            };
        }
    }
    let _ = (state, user_id, doc_id, source_path, text);
    // No rag feature: nothing to index, but we're still "ready" since
    // the doc body is on disk and the chat handler can serve it via
    // the cache fast-path.
    (0, None, "ready".to_string())
}

// ---------------------------------------------------------------------------
// POST /eurlex/documents/:id/resync — restart embedding for an interrupted doc
// ---------------------------------------------------------------------------
//
// Picks up a documents row whose status is 'interrupted' (or 'syncing'
// — covers the case where the backend was killed mid-embed and the
// row never made it to a terminal state), reads the cached text from
// `extracted_text_path`, re-runs `index_document`, and updates status.

async fn resync_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let row: Option<(Option<String>, Option<String>, String)> = sqlx::query_as(
        "SELECT extracted_text_path, corpus_identifier, status FROM documents \
         WHERE id = ? AND user_id = ? AND corpus_id = ?",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .bind(CORPUS_ID)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (text_path, _identifier, prev_status) = row.ok_or_else(|| {
        err(StatusCode::NOT_FOUND, "Documento EUR-Lex non trovato")
    })?;
    let text_key = text_path.ok_or_else(|| {
        err(
            StatusCode::CONFLICT,
            "Documento senza testo estratto: re-fetch necessario",
        )
    })?;

    // Mark as syncing immediately so concurrent /list calls show the
    // right state.
    let _ = sqlx::query("UPDATE documents SET status = 'syncing' WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await;

    let storage = make_storage()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let bytes = storage
        .get(&text_key)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    // Same constraint as fetch_celex: the chunk source_path must be a
    // local filesystem path so /sync/kb-doc can read it back. Resolve
    // text_key (relative `cache/<hash>.txt`) to its absolute form.
    let source_path = storage_root()
        .join(text_key.replace('/', std::path::MAIN_SEPARATOR_STR))
        .to_string_lossy()
        .to_string();

    let (chunks_indexed, indexing_error, final_status) =
        run_indexing(&state, &auth.user_id, &id, &source_path, &text).await;

    sqlx::query("UPDATE documents SET status = ? WHERE id = ?")
        .bind(&final_status)
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({
        "id": id,
        "previous_status": prev_status,
        "status": final_status,
        "chunks_indexed": chunks_indexed,
        "indexing_error": indexing_error,
    })))
}

// ---------------------------------------------------------------------------
// GET /eurlex/documents — list all EUR-Lex docs the user has indexed
// ---------------------------------------------------------------------------

async fn list_documents(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    // LEFT JOIN onto a per-doc chunk-count subquery so the UI can
    // surface "0 chunk indicizzati" even on rows the backend marked
    // 'ready' — defends against silent regressions where the chunker
    // pipeline goes empty for a class of inputs.
    let rows: Vec<(
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        i64,
        String,
        String,
        i64,
    )> = sqlx::query_as(
        "SELECT d.id, d.filename, d.corpus_identifier, d.corpus_language, \
                d.corpus_date, d.fetched_with_fallback, d.size_bytes, d.created_at, d.status, \
                COALESCE(c.n, 0) AS chunks \
         FROM documents d \
         LEFT JOIN ( \
             SELECT document_id, COUNT(*) AS n FROM doc_chunks GROUP BY document_id \
         ) c ON c.document_id = d.id \
         WHERE d.user_id = ? AND d.corpus_id = ? \
         ORDER BY d.created_at DESC",
    )
    .bind(&auth.user_id)
    .bind(CORPUS_ID)
    .fetch_all(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let docs: Vec<Value> = rows
        .into_iter()
        .map(|(id, filename, ident, lang, date, fb, size, created, status, chunks)| {
            json!({
                "id": id,
                "filename": filename,
                "corpus_identifier": ident,
                "corpus_language": lang,
                "corpus_date": date,
                "fetched_with_fallback": fb != 0,
                "size_bytes": size,
                "created_at": created,
                "status": status,
                "chunks_indexed": chunks,
                "source_url": ident.as_ref().zip(lang.as_ref()).map(|(c, l)| {
                    format!(
                        "https://eur-lex.europa.eu/legal-content/{}/TXT/?uri=CELEX:{}",
                        l.to_uppercase(),
                        c
                    )
                }),
            })
        })
        .collect();

    Ok(Json(json!({ "documents": docs })))
}

// ---------------------------------------------------------------------------
// DELETE /eurlex/documents/:id — drop a synced EUR-Lex doc
// ---------------------------------------------------------------------------
//
// Removes the documents row + its embedding chunks. The cache files
// (cache/<hash>.txt) are ref-counted across users / chats — we only
// delete the on-disk text if no other documents row still references
// the same hash. Same policy as chat-attachment cleanup.

async fn delete_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT storage_path, content_hash FROM documents \
         WHERE id = ? AND user_id = ? AND corpus_id = ?",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .bind(CORPUS_ID)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (storage_path, content_hash) = row.ok_or_else(|| {
        err(StatusCode::NOT_FOUND, "Documento EUR-Lex non trovato")
    })?;

    // Drop embedding chunks first so RAG queries don't return stale
    // hits for a doc the user just removed.
    let _ = sqlx::query("DELETE FROM doc_chunks WHERE document_id = ?")
        .bind(&id)
        .execute(&state.db)
        .await;

    sqlx::query("DELETE FROM documents WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    // Ref-count check on the content hash: only delete the on-disk
    // file if no other documents row still references it.
    if let Some(hash) = content_hash {
        let still_referenced: Option<(i64,)> = sqlx::query_as(
            "SELECT 1 FROM documents WHERE content_hash = ? LIMIT 1",
        )
        .bind(&hash)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
        if still_referenced.is_none() {
            if let (Ok(storage), Some(key)) = (make_storage(), storage_path) {
                if let Err(e) = storage.delete(&key).await {
                    tracing::warn!(
                        "[eurlex] failed to delete cache file {} for doc {}: {}",
                        key,
                        id,
                        e
                    );
                }
            }
        } else {
            tracing::info!(
                "[eurlex] keeping cache file for hash {} (still referenced)",
                hash
            );
        }
    }

    Ok(Json(json!({ "ok": true, "id": id })))
}
