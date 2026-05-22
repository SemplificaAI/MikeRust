use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;

use crate::{auth::middleware::AuthUser, storage::make_storage, AppState};

fn storage_root() -> PathBuf {
    PathBuf::from(
        std::env::var("STORAGE_PATH").unwrap_or_else(|_| "./data/storage".to_string()),
    )
}

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"detail": msg})))
}

pub fn router() -> Router<Arc<AppState>> {
    // axum's DefaultBodyLimit caps multipart bodies at 2 MB out of the box.
    // A handful of docx/pdf docs uploaded together blow past that and the
    // connection is reset mid-stream — the browser surfaces it as
    // `TypeError: Failed to fetch`, not as an HTTP 413, which is why the
    // backend log shows nothing when concurrent uploads fail. 100 MB is
    // safely above any realistic legal document we expect.
    Router::new()
        .route("/", get(list_documents).post(upload_document))
        .route("/{id}", get(get_document).delete(delete_document))
        // Display endpoint used by the in-app viewer (DocView.tsx / DocxView.tsx).
        // Returns the file bytes with the appropriate Content-Type so the
        // frontend can pick PDF.js or docx-preview based on it.
        .route("/{id}/display", get(display_document))
        .route("/{id}/docx", get(display_document))
        .route("/{id}/text", get(display_document))
        .route("/{id}/download", get(download_document))
        .route("/{id}/url", get(get_document_url))
        .route("/{id}/transcript", get(get_document_transcript))
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
}

// ---------------------------------------------------------------------------
// GET /document?project_id=…
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct ListQuery {
    project_id: Option<String>,
    /// Optional `?domain=…` filter added with migration 0018. Useful
    /// for the global-documents (project_id IS NULL) view in the UI
    /// where users want to slice their personal pool by vertical.
    domain: Option<String>,
}

async fn list_documents(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<ListQuery>,
) -> ApiResult {
    let domain_filter: Option<&str> = q
        .domain
        .as_deref()
        .filter(|s| !s.is_empty() && crate::domain::is_valid(s));

    let mut sql = String::from(
        "SELECT id, filename, file_type, size_bytes, status, created_at, domain, \
                project_folder_id \
         FROM documents WHERE user_id = ?",
    );
    if q.project_id.is_some() {
        sql.push_str(" AND project_id = ?");
    }
    if domain_filter.is_some() {
        sql.push_str(" AND domain = ?");
    }
    sql.push_str(" ORDER BY created_at DESC");

    let mut query = sqlx::query_as::<
        _,
        (String, String, String, i64, Option<String>, String, String, Option<String>),
    >(&sql)
    .bind(&auth.user_id);
    if let Some(pid) = &q.project_id {
        query = query.bind(pid);
    }
    if let Some(d) = domain_filter {
        query = query.bind(d);
    }
    let rows = query
        .fetch_all(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let docs: Vec<Value> = rows
        .into_iter()
        .map(|(id, filename, file_type, size, status, created_at, domain, project_folder_id)| {
            json!({ "id": id, "filename": filename, "file_type": file_type,
                    "size_bytes": size, "status": status,
                    "domain": domain, "created_at": created_at,
                    "project_folder_id": project_folder_id })
        })
        .collect();

    Ok(Json(json!({ "documents": docs })))
}

// ---------------------------------------------------------------------------
// POST /document  — multipart upload
// Fields: file (binary), project_id? (text)
// ---------------------------------------------------------------------------
async fn upload_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    mut multipart: Multipart,
) -> ApiResult {
    tracing::info!("[upload] POST /document user={}", auth.user_id);
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut filename: Option<String> = None;
    let mut project_id: Option<String> = None;
    // `cache=true` is the chat-composer signal: store the binary +
    // extracted text under data/storage/cache, keyed by SHA-256 of the
    // bytes. The chat row may not exist at upload time (the composer
    // materialises the chat on first send), so chat_id is wired up
    // later by the /chat send handler — and the chat-delete handler
    // ref-counts by content_hash before unlinking the on-disk files.
    let mut cache = false;
    // Optional `domain` multipart field — when omitted, project-scoped
    // uploads inherit the project's domain below, and standalone
    // uploads fall back to the schema default ('legal') via the
    // INSERT (no need to bind a value).
    let mut domain: Option<String> = None;
    while let Some(field) = multipart.next_field().await.map_err(|e| {
        tracing::warn!("[upload] multipart parse error: {e}");
        err(StatusCode::BAD_REQUEST, &e.to_string())
    })? {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "file" => {
                filename = field.file_name().map(|s| s.to_string());
                let bytes = field.bytes().await.map_err(|e| {
                    tracing::warn!(
                        "[upload] failed reading file field (filename={:?}): {e}",
                        filename
                    );
                    err(StatusCode::BAD_REQUEST, &e.to_string())
                })?;
                tracing::info!(
                    "[upload] received file field name={:?} size={} bytes",
                    filename,
                    bytes.len()
                );
                file_bytes = Some(bytes.to_vec());
            }
            "project_id" => {
                let text = field.text().await.map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
                if !text.trim().is_empty() {
                    project_id = Some(text.trim().to_string());
                }
            }
            "cache" => {
                let text = field.text().await.map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
                cache = matches!(text.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes");
            }
            "domain" => {
                let text = field.text().await.map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
                let trimmed = text.trim();
                if !trimmed.is_empty() && crate::domain::is_valid(trimmed) {
                    domain = Some(trimmed.to_string());
                }
            }
            _ => {}
        }
    }

    let data = file_bytes.ok_or_else(|| err(StatusCode::BAD_REQUEST, "No file field in multipart"))?;
    let fname = filename.unwrap_or_else(|| "upload".to_string());
    let ext = fname.rsplit('.').next().unwrap_or("").to_lowercase();
    let file_type = match ext.as_str() {
        "pdf" => "pdf",
        "docx" => "docx",
        "rtf" => "rtf",
        "xlsx" => "xlsx",
        "xls" => "xls",
        "xlsb" => "xlsb",
        "ods" => "ods",
        "csv" => "csv",
        "txt" => "txt",
        "md" => "md",
        "png" => "png",
        "jpg" | "jpeg" => "jpeg",
        "tif" | "tiff" => "tiff",
        _ => "other",
    };

    let doc_id = uuid::Uuid::new_v4().to_string();
    let storage = make_storage().map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let size = data.len() as i64;

    // Cache uploads (chat-attached): key files by SHA-256 of the
    // binary so re-uploads of identical content dedupe and same
    // user-facing filename across different chats can't collide on
    // disk. We also extract plain text once per unique hash so the
    // chat send handler doesn't re-parse a 200-page PDF on every
    // turn. Skip extraction silently if the binary or text already
    // exist on disk — same hash means identical bytes.
    let (storage_key, content_hash, extracted_text_path) = if cache {
        let hash = {
            let mut hasher = Sha256::new();
            hasher.update(&data);
            format!("{:x}", hasher.finalize())
        };
        let bin_ext = if ext.is_empty() { "bin".to_string() } else { ext.clone() };
        let bin_key = format!("cache/{}.{}", hash, bin_ext);
        let txt_key = format!("cache/{}.txt", hash);

        let root = storage_root();
        let bin_abs = root.join(bin_key.replace('/', std::path::MAIN_SEPARATOR_STR));
        let txt_abs = root.join(txt_key.replace('/', std::path::MAIN_SEPARATOR_STR));

        if !bin_abs.exists() {
            storage
                .put(&bin_key, &data, "application/octet-stream")
                .await
                .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
            tracing::info!("[upload] cache binary written: {} ({} bytes)", bin_key, data.len());
        } else {
            tracing::info!("[upload] cache binary already exists, reusing: {}", bin_key);
        }

        if !txt_abs.exists() {
            // extract_text_dispatch keys off the path's extension, so
            // the absolute path of the binary we just wrote is the
            // right thing to feed it (pdfium also needs an on-disk
            // path for PDFs).
            match crate::sync::scanner::extract_text_dispatch(&bin_abs, &data).await {
                Ok((text, skip_reason)) => {
                    if let Some(reason) = skip_reason {
                        tracing::info!(
                            "[upload] cache text extraction skipped for {} ({}): {}",
                            fname,
                            hash,
                            reason
                        );
                    }
                    storage
                        .put(&txt_key, text.as_bytes(), "text/plain; charset=utf-8")
                        .await
                        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
                    tracing::info!(
                        "[upload] cache text written: {} ({} chars)",
                        txt_key,
                        text.len()
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "[upload] cache text extraction failed for {} ({}): {}",
                        fname,
                        hash,
                        e
                    );
                    // Drop a marker so we don't retry on every reload —
                    // an empty .txt is a valid "we tried" signal.
                    let _ = storage
                        .put(&txt_key, b"", "text/plain; charset=utf-8")
                        .await;
                }
            }
        } else {
            tracing::info!("[upload] cache text already exists, reusing: {}", txt_key);
        }

        (bin_key, Some(hash), Some(txt_key))
    } else {
        // Legacy (non-cache) layout: per-user, per-doc-id. No hashing,
        // no text extraction — the existing pipeline handles those
        // documents on demand.
        let key = format!("documents/{}/{}", auth.user_id, doc_id);
        storage
            .put(&key, &data, "application/octet-stream")
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
        (key, None, None)
    };

    // Resolve domain: explicit field wins; otherwise inherit from the
    // parent project; otherwise fall back to schema default ('legal').
    let resolved_domain: Option<String> = if let Some(d) = domain {
        Some(d)
    } else if let Some(pid) = &project_id {
        sqlx::query_as::<_, (String,)>(
            "SELECT domain FROM projects WHERE id = ? AND user_id = ?",
        )
        .bind(pid)
        .bind(&auth.user_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .map(|(d,)| d)
    } else {
        None
    };

    sqlx::query(
        "INSERT INTO documents (id, user_id, project_id, filename, file_type, size_bytes, storage_path, status, content_hash, extracted_text_path, domain) \
         VALUES (?, ?, ?, ?, ?, ?, ?, 'ready', ?, ?, COALESCE(?, 'legal'))",
    )
    .bind(&doc_id)
    .bind(&auth.user_id)
    .bind(&project_id)
    .bind(&fname)
    .bind(file_type)
    .bind(size)
    .bind(&storage_key)
    .bind(&content_hash)
    .bind(&extracted_text_path)
    .bind(&resolved_domain)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({
        "id": doc_id,
        "filename": fname,
        "file_type": file_type,
        "size_bytes": size,
        "domain": resolved_domain.unwrap_or_else(|| "legal".to_string()),
        "status": "ready"
    })))
}

// ---------------------------------------------------------------------------
// GET /document/:id
// ---------------------------------------------------------------------------
async fn get_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let row: Option<(String, String, String, i64, Option<String>, Option<String>, String)> =
        sqlx::query_as(
            "SELECT id, filename, file_type, size_bytes, storage_path, status, created_at \
             FROM documents WHERE id = ? AND user_id = ?",
        )
        .bind(&id)
        .bind(&auth.user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (id, filename, file_type, size, storage_path, status, created_at) =
        row.ok_or_else(|| err(StatusCode::NOT_FOUND, "Document not found"))?;

    Ok(Json(json!({
        "id": id,
        "filename": filename,
        "file_type": file_type,
        "size_bytes": size,
        "storage_path": storage_path,
        "status": status,
        "created_at": created_at,
    })))
}

// ---------------------------------------------------------------------------
// GET /document/:id/display, /docx, /text — stream raw bytes for the viewer
// ---------------------------------------------------------------------------
async fn display_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Response {
    let row: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT filename, file_type, storage_path FROM documents WHERE id = ? AND user_id = ?",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    let Some((filename, file_type, Some(storage_path))) = row else {
        return (StatusCode::NOT_FOUND, "Document not found").into_response();
    };

    let storage = match crate::storage::make_storage() {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let bytes = match storage.get(&storage_path).await {
        Ok(b) => b,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let content_type = match file_type.as_str() {
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "rtf" => "application/rtf",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xls" => "application/vnd.ms-excel",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "csv" => "text/csv; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "png" => "image/png",
        "jpeg" | "jpg" => "image/jpeg",
        "tiff" | "tif" => "image/tiff",
        _ => "application/octet-stream",
    };

    let mut resp = Response::new(Body::from(bytes));
    resp.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    if let Ok(disp) = HeaderValue::from_str(&format!("inline; filename=\"{filename}\"")) {
        resp.headers_mut().insert(header::CONTENT_DISPOSITION, disp);
    }
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=60"),
    );
    resp
}

// ---------------------------------------------------------------------------
// GET /document/:id/download — same bytes as /display but with
// `Content-Disposition: attachment` so a plain browser navigation
// triggers the OS save dialog. Used by the chat's download card to
// hand a generated .docx / .xlsx / … out of MikeRust as a normal file
// the user can keep, mail, archive, etc. The /display sibling stays
// `inline` because the in-app PDF.js / docx-preview viewers need the
// webview to render the bytes in place rather than save them.
// ---------------------------------------------------------------------------
async fn download_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Response {
    let row: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT filename, file_type, storage_path FROM documents WHERE id = ? AND user_id = ?",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    let Some((filename, file_type, Some(storage_path))) = row else {
        return (StatusCode::NOT_FOUND, "Document not found").into_response();
    };

    let storage = match crate::storage::make_storage() {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let bytes = match storage.get(&storage_path).await {
        Ok(b) => b,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let content_type = mime_for_extension(&file_type);
    let mut resp = Response::new(Body::from(bytes));
    resp.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    // RFC 6266: filename* with UTF-8 encoding for non-ASCII filenames,
    // plus a quoted ASCII fallback so older clients still pick something
    // sensible. The fallback strips any character outside the printable
    // ASCII range so a curly-quote in the filename can't break the
    // Content-Disposition header parser.
    let ascii_fallback: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii() && c != '"' && !c.is_control() {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded =
        percent_encoding::utf8_percent_encode(&filename, percent_encoding::NON_ALPHANUMERIC)
            .to_string();
    let disp = format!(
        "attachment; filename=\"{ascii_fallback}\"; filename*=UTF-8''{encoded}"
    );
    if let Ok(value) = HeaderValue::from_str(&disp) {
        resp.headers_mut().insert(header::CONTENT_DISPOSITION, value);
    }
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    resp
}

/// Map our `documents.file_type` column to a MIME string. Kept in sync
/// with the `display_document` handler so download and inline-display
/// return identical Content-Type for the same row.
fn mime_for_extension(ext: &str) -> &'static str {
    match ext {
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "rtf" => "application/rtf",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xls" => "application/vnd.ms-excel",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "csv" => "text/csv; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "png" => "image/png",
        "jpeg" | "jpg" => "image/jpeg",
        "tiff" | "tif" => "image/tiff",
        _ => "application/octet-stream",
    }
}

// ---------------------------------------------------------------------------
// GET /document/:id/url — frontend convenience: returns a URL the viewer
// can fetch later. In MikeRust it's just an absolute /display URL because
// storage is local; remote-storage backends could return a presigned URL.
// ---------------------------------------------------------------------------
async fn get_document_url(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let owns: Option<(String,)> =
        sqlx::query_as("SELECT id FROM documents WHERE id = ? AND user_id = ?")
            .bind(&id)
            .bind(&auth.user_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    if owns.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "Document not found"));
    }
    let api_base = std::env::var("API_BASE_URL")
        .unwrap_or_else(|_| "http://localhost:3001".to_string());
    Ok(Json(json!({
        "url": format!("{api_base}/document/{id}/display"),
    })))
}

// ---------------------------------------------------------------------------
// DELETE /document/:id
// ---------------------------------------------------------------------------
async fn delete_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT storage_path FROM documents WHERE id = ? AND user_id = ?")
            .bind(&id)
            .bind(&auth.user_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (storage_path,) = row.ok_or_else(|| err(StatusCode::NOT_FOUND, "Document not found"))?;

    // Delete from storage
    if let Some(key) = storage_path {
        if let Ok(storage) = make_storage() {
            let _ = storage.delete(&key).await;
        }
    }

    sqlx::query("DELETE FROM documents WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// GET /document/:id/transcript
//
// Joins the indexed chunks of a document back into a single body of
// text. For audio documents transcribed via whisper.cpp, each whisper
// segment is stamped with a `[T MM:SS]\n` marker (mirror of the
// `[Page N]\n` PDF marker) at index time; we parse those markers out,
// dedupe by start_ms (chunks overlap by ~200 tokens, so the same
// segment appears in multiple chunks), and return both the
// concatenated text and a structured segment list with timestamps.
//
// Non-audio documents return `segments: []` and `text` = chunks joined
// in chunk_index order with overlap stripped via longest-suffix /
// longest-prefix overlap heuristic. Useful for "show me the indexed
// content of this PDF" preview too, not just audio.
//
// Permissions: enforced via the `user_id` predicate on doc_chunks.
// ---------------------------------------------------------------------------
async fn get_document_transcript(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    // Ownership guard: confirm the document exists and is owned by
    // this user before touching doc_chunks. The chunks table is
    // partition-keyed by user_id so the SELECT below is already safe,
    // but the explicit check gives us a clean 404 vs an empty body.
    let owner: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM documents WHERE id = ? AND user_id = ?",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    if owner.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "Document not found"));
    }

    // Chunks ordered by chunk_index. We project just the text + page
    // because that's all the transcript join needs; embeddings stay
    // server-side.
    let rows: Vec<(i64, String, Option<i64>)> = sqlx::query_as(
        "SELECT chunk_index, text, page FROM doc_chunks \
         WHERE user_id = ? AND document_id = ? \
         ORDER BY chunk_index ASC",
    )
    .bind(&auth.user_id)
    .bind(&id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    if rows.is_empty() {
        return Ok(Json(json!({
            "text": "",
            "segments": Vec::<Value>::new(),
            "duration_ms": 0u64,
        })));
    }

    // Parse `[T MM:SS]` or `[T HH:MM:SS]` segment markers out of each
    // chunk's text. Returns Vec<(start_ms, segment_text)>. Markers we
    // can't parse are skipped silently — the surrounding text just
    // becomes part of the previous segment.
    let mut segments: std::collections::BTreeMap<u64, String> =
        std::collections::BTreeMap::new();
    let mut any_audio_marker = false;
    for (_idx, text, _page) in &rows {
        for seg in extract_audio_segments(text) {
            any_audio_marker = true;
            // First occurrence wins; later overlapping chunks usually
            // truncate the segment at their window edge.
            segments.entry(seg.start_ms).or_insert(seg.text);
        }
    }

    if any_audio_marker {
        // Audio document — return segments with sequential end_ms
        // computed from the next segment's start (last segment has
        // end = start + 0 fallback; the viewer treats end_ms <=
        // start_ms as "play to end of file").
        let entries: Vec<(u64, String)> = segments.into_iter().collect();
        let mut segs_json = Vec::with_capacity(entries.len());
        let mut joined = String::new();
        for (i, (start_ms, text)) in entries.iter().enumerate() {
            let end_ms = entries
                .get(i + 1)
                .map(|(next_start, _)| *next_start)
                .unwrap_or(*start_ms);
            segs_json.push(json!({
                "start_ms": start_ms,
                "end_ms": end_ms,
                "text": text,
            }));
            if !joined.is_empty() {
                joined.push('\n');
            }
            joined.push_str(&format!(
                "[T {:02}:{:02}]\n{}",
                start_ms / 60_000,
                (start_ms / 1000) % 60,
                text
            ));
        }
        let duration_ms = entries.last().map(|(s, _)| *s).unwrap_or(0);
        Ok(Json(json!({
            "text": joined,
            "segments": segs_json,
            "duration_ms": duration_ms,
        })))
    } else {
        // Non-audio document — join chunks in chunk_index order with
        // a simple longest-common-overlap trim so the ~200-token
        // overlap between adjacent chunks doesn't duplicate paragraphs
        // in the rendered preview.
        let mut joined = String::new();
        for (_idx, text, _page) in rows {
            if joined.is_empty() {
                joined.push_str(&text);
            } else {
                let overlap = longest_overlap(&joined, &text, 256);
                if overlap > 0 {
                    joined.push_str(&text[overlap..]);
                } else {
                    joined.push('\n');
                    joined.push_str(&text);
                }
            }
        }
        Ok(Json(json!({
            "text": joined,
            "segments": Vec::<Value>::new(),
            "duration_ms": 0u64,
        })))
    }
}

/// A single whisper segment parsed from a chunk's text.
struct AudioSegment {
    start_ms: u64,
    text: String,
}

/// Scan `chunk_text` for `[T MM:SS]` or `[T HH:MM:SS]` markers and
/// return the segment after each (up to the next marker or end of
/// string). Tolerant of CRLF, surrounding whitespace, and the marker
/// being mid-chunk (the chunker's overlap can place a marker anywhere).
fn extract_audio_segments(chunk_text: &str) -> Vec<AudioSegment> {
    let bytes = chunk_text.as_bytes();
    // Find every `[T ` occurrence; for each, parse `(\d{1,2}:)?\d{1,2}:\d{2}\]`.
    let mut markers: Vec<(usize, usize, u64)> = Vec::new(); // (marker_start, marker_end_exclusive, start_ms)
    let mut i = 0;
    while i + 4 <= bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'T' && bytes[i + 2] == b' ' {
            let inner_start = i + 3;
            let mut j = inner_start;
            // Read digits and colons up to ']'.
            while j < bytes.len() && bytes[j] != b']' {
                let c = bytes[j];
                if !(c.is_ascii_digit() || c == b':' || c == b' ') {
                    break;
                }
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b']' {
                if let Ok(time_str) = std::str::from_utf8(&bytes[inner_start..j]) {
                    if let Some(ms) = parse_ts(time_str.trim()) {
                        markers.push((i, j + 1, ms));
                    }
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    if markers.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(markers.len());
    for (k, (_start, end, ms)) in markers.iter().enumerate() {
        let next = markers.get(k + 1).map(|(s, _, _)| *s).unwrap_or(bytes.len());
        let text = chunk_text[*end..next].trim().to_string();
        if !text.is_empty() {
            out.push(AudioSegment {
                start_ms: *ms,
                text,
            });
        }
    }
    out
}

/// Parse `MM:SS` or `HH:MM:SS` into milliseconds. Returns None on
/// malformed input.
fn parse_ts(s: &str) -> Option<u64> {
    let parts: Vec<&str> = s.split(':').collect();
    let (h, m, sec) = match parts.len() {
        2 => (0u64, parts[0].parse::<u64>().ok()?, parts[1].parse::<u64>().ok()?),
        3 => (
            parts[0].parse::<u64>().ok()?,
            parts[1].parse::<u64>().ok()?,
            parts[2].parse::<u64>().ok()?,
        ),
        _ => return None,
    };
    Some(((h * 3600) + (m * 60) + sec) * 1000)
}

/// Longest suffix of `a` that is a prefix of `b`, capped at `cap`. Used
/// to deduplicate chunk-overlap when joining non-audio chunks for the
/// preview. Linear-ish on the overlap window — fine for our chunk
/// sizes (~800 tokens, overlap ~200).
///
/// Anything below `MIN_OVERLAP` is treated as a no-match: a real chunk
/// seam shares hundreds of characters (200-token overlap ≈ 800 bytes
/// for typical Italian/English text), while a single-letter "match"
/// is just an accidental seam of unrelated text and would garble the
/// preview if we trusted it.
fn longest_overlap(a: &str, b: &str, cap: usize) -> usize {
    const MIN_OVERLAP: usize = 16;
    let limit = cap.min(a.len()).min(b.len());
    if limit < MIN_OVERLAP {
        return 0;
    }
    let a_tail = &a[a.len() - limit..];
    let a_bytes = a_tail.as_bytes();
    let b_bytes = b.as_bytes();
    for k in (MIN_OVERLAP..=limit).rev() {
        if a_bytes[limit - k..] == b_bytes[..k]
            && b.is_char_boundary(k)
            && a_tail.is_char_boundary(limit - k)
        {
            return k;
        }
    }
    0
}

#[cfg(test)]
mod transcript_tests {
    use super::*;

    #[test]
    fn parse_ts_handles_mm_ss() {
        assert_eq!(parse_ts("14:32"), Some(14 * 60_000 + 32_000));
    }

    #[test]
    fn parse_ts_handles_hh_mm_ss() {
        assert_eq!(parse_ts("01:14:32"), Some(3600_000 + 14 * 60_000 + 32_000));
    }

    #[test]
    fn extract_segments_simple() {
        let chunk = "[T 00:00]\nBuongiorno.\n[T 00:05]\nIl mio nome è Mario.";
        let segs = extract_audio_segments(chunk);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].start_ms, 0);
        assert_eq!(segs[0].text, "Buongiorno.");
        assert_eq!(segs[1].start_ms, 5_000);
        assert_eq!(segs[1].text, "Il mio nome è Mario.");
    }

    #[test]
    fn extract_segments_handles_hms() {
        let chunk = "[T 01:00:05]\nSecondo capitolo.";
        let segs = extract_audio_segments(chunk);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].start_ms, 3600_000 + 5_000);
    }

    #[test]
    fn extract_segments_skips_non_audio() {
        let chunk = "[Page 3]\nNormal PDF text without any time markers.";
        assert!(extract_audio_segments(chunk).is_empty());
    }

    #[test]
    fn longest_overlap_finds_join_seam() {
        let a = "the quick brown fox jumps over the lazy dog";
        let b = "over the lazy dog and runs away";
        assert_eq!(longest_overlap(a, b, 64), "over the lazy dog".len());
    }

    #[test]
    fn longest_overlap_no_match_returns_zero() {
        // Accidental single-char tail/head match ("…d" / "d…") is
        // below the 16-byte minimum and must not be reported as a
        // seam — otherwise the preview would silently merge
        // unrelated text.
        let a = "completely unrelated";
        let b = "different content";
        assert_eq!(longest_overlap(a, b, 64), 0);
    }

    #[test]
    fn longest_overlap_ignores_short_accidental_match() {
        // 8-char common substring at the boundary is still below the
        // 16-byte floor — should be treated as no seam.
        let a = "lorem ipsum abcdefg X";
        let b = "abcdefg Y next paragraph";
        assert_eq!(longest_overlap(a, b, 64), 0);
    }
}
