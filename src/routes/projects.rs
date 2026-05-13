use axum::{
    extract::{Multipart, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::{auth::middleware::AuthUser, AppState};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"detail": msg})))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_projects).post(create_project))
        .route("/{id}", get(get_project).put(update_project).delete(delete_project))
        .route("/{id}/export", post(export_project))
        .route("/import", post(import_project))
        .route("/{id}/documents/{doc_id}", axum::routing::patch(rename_project_document))
}

// ---------------------------------------------------------------------------
// GET /project  — list all projects for the authenticated user
// ---------------------------------------------------------------------------
async fn list_projects(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let rows: Vec<(String, String, Option<String>, String, String)> =
        sqlx::query_as(
            "SELECT id, name, description, created_at, updated_at \
             FROM projects WHERE user_id = ? ORDER BY updated_at DESC",
        )
        .bind(&auth.user_id)
        .fetch_all(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let projects: Vec<Value> = rows
        .into_iter()
        .map(|(id, name, desc, created_at, updated_at)| {
            json!({ "id": id, "name": name, "description": desc,
                    "created_at": created_at, "updated_at": updated_at })
        })
        .collect();

    Ok(Json(json!({ "projects": projects })))
}

// ---------------------------------------------------------------------------
// POST /project
// Body: { name, description? }
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct CreateProjectBody {
    name: String,
    description: Option<String>,
}

async fn create_project(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<CreateProjectBody>,
) -> ApiResult {
    if body.name.trim().is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Project name cannot be empty"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO projects (id, user_id, name, description) VALUES (?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .bind(body.name.trim())
    .bind(&body.description)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({ "id": id, "name": body.name.trim() })))
}

// ---------------------------------------------------------------------------
// GET /project/:id
// ---------------------------------------------------------------------------
async fn get_project(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let row: Option<(String, String, Option<String>, String, String, String)> =
        sqlx::query_as(
            "SELECT id, name, description, created_at, updated_at, isolation_mode \
             FROM projects WHERE id = ? AND user_id = ?",
        )
        .bind(&id)
        .bind(&auth.user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (id, name, desc, created_at, updated_at, isolation_mode) =
        row.ok_or_else(|| err(StatusCode::NOT_FOUND, "Project not found"))?;

    Ok(Json(json!({
        "id": id, "name": name, "description": desc,
        "created_at": created_at, "updated_at": updated_at,
        "isolation_mode": isolation_mode
    })))
}

// ---------------------------------------------------------------------------
// PUT /project/:id
// Body: { name?, description?, isolation_mode? }
// `isolation_mode` controls how RAG retrieval behaves inside this
// project's chats:
//   - "shared" (default): chats see global pool + this project's pool
//   - "strict":           chats see ONLY this project's pool
// Defended at the SQL layer in `EmbeddingService::search`, so a
// strict project can't leak global excerpts even via the search_kb tool.
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct UpdateProjectBody {
    name: Option<String>,
    description: Option<String>,
    isolation_mode: Option<String>,
}

async fn update_project(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateProjectBody>,
) -> ApiResult {
    // Reject unknown isolation values up front rather than letting them
    // sneak into the DB and confuse the chat dispatcher.
    if let Some(mode) = body.isolation_mode.as_deref() {
        if mode != "shared" && mode != "strict" {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "isolation_mode must be 'shared' or 'strict'",
            ));
        }
    }

    let result = sqlx::query(
        "UPDATE projects SET \
           name = COALESCE(?, name), \
           description = COALESCE(?, description), \
           isolation_mode = COALESCE(?, isolation_mode), \
           updated_at = datetime('now') \
         WHERE id = ? AND user_id = ?",
    )
    .bind(&body.name)
    .bind(&body.description)
    .bind(&body.isolation_mode)
    .bind(&id)
    .bind(&auth.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    if result.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "Project not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// DELETE /project/:id
// ---------------------------------------------------------------------------
async fn delete_project(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let result = sqlx::query("DELETE FROM projects WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    if result.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "Project not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// POST /project/:id/export
// Body: { recipient_email: string, include_chats?: bool }
// Response: binary `.mikeprj` (encrypted zip)
//
// The recipient_email is the address that will be used to derive the
// AES key — only a MikeRust install where the active user's account is
// registered with the same email can open the file. See `mikeprj/mod.rs`
// for the (intentionally-weak) sharing model.
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct ExportBody {
    recipient_email: String,
    #[serde(default)]
    include_chats: bool,
}

async fn export_project(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<ExportBody>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    if body.recipient_email.trim().is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "recipient_email is required"));
    }

    // Storage handle — used by build_payload's closure to read each
    // document's bytes. We share one handle for the whole export so the
    // local-fs case doesn't keep re-creating the storage instance.
    let storage = crate::storage::make_storage()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let storage: std::sync::Arc<Box<dyn crate::storage::Storage>> =
        std::sync::Arc::new(storage);

    let payload = crate::mikeprj::io::build_payload(
        &state.db,
        &auth.user_id,
        &id,
        crate::mikeprj::io::ExportOptions {
            include_chats: body.include_chats,
        },
        |key| {
            let s = storage.clone();
            let k = key.to_string();
            Box::pin(async move { s.get(&k).await })
        },
    )
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let project_basename = sanitize_filename(&payload.project.name);

    let zip_bytes = crate::mikeprj::io::zip_payload(&payload)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let sealed = crate::mikeprj::crypto::seal(&body.recipient_email, &zip_bytes)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let filename = format!("{project_basename}.mikeprj");
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        sealed,
    )
        .into_response())
}

/// Strip path-unsafe characters from a project name so it survives use
/// as a download filename. Falls back to "project" when the result is
/// empty (e.g. a name made entirely of slashes or control chars).
fn sanitize_filename(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = s.trim().trim_matches('.');
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed.to_string()
    }
}

// ---------------------------------------------------------------------------
// POST /project/import   (multipart)
//   - field `file`             : the .mikeprj bytes
//   - field `recipient_email`  : the email to derive the AES key with —
//                                must match the one used at export time
//
// On success returns the new project_id. The caller can then navigate
// to /projects/<new_id>. Documents are copied into the importer's
// local storage; tabular reviews and custom workflows are recreated
// with fresh UUIDs (so we don't collide with existing rows). Chats are
// imported only if the original export included them.
// ---------------------------------------------------------------------------
async fn import_project(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    mut multipart: Multipart,
) -> ApiResult {
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut recipient_email: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
                file_bytes = Some(bytes.to_vec());
            }
            "recipient_email" => {
                let s = field
                    .text()
                    .await
                    .map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
                recipient_email = Some(s);
            }
            _ => {} // ignore unknown fields
        }
    }

    let file_bytes = file_bytes
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "missing 'file' field"))?;
    let recipient_email = recipient_email
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "missing 'recipient_email' field"))?;

    // Decrypt + unzip
    let zip_bytes = crate::mikeprj::crypto::open(&recipient_email, &file_bytes)
        .map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
    let payload = crate::mikeprj::io::unzip_payload(&zip_bytes)
        .map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;

    // Create the new project under the importer's account.
    let new_project_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO projects (id, user_id, name, cm_number, created_at, updated_at) \
         VALUES (?, ?, ?, ?, datetime('now'), datetime('now'))",
    )
    .bind(&new_project_id)
    .bind(&auth.user_id)
    .bind(&payload.project.name)
    .bind(payload.project.cm_number.as_deref())
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    // Documents: write each blob into the importer's storage with a
    // fresh document_id, then row in `documents`.
    let storage = crate::storage::make_storage()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let mut doc_id_remap: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for (doc, bytes) in &payload.documents {
        let new_doc_id = uuid::Uuid::new_v4().to_string();
        let storage_key = format!("documents/{}/{}", auth.user_id, new_doc_id);
        let _ = storage
            .put(&storage_key, bytes, "application/octet-stream")
            .await;
        sqlx::query(
            "INSERT INTO documents \
             (id, user_id, project_id, filename, file_type, size_bytes, storage_path, status, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, 'ready', datetime('now'))",
        )
        .bind(&new_doc_id)
        .bind(&auth.user_id)
        .bind(&new_project_id)
        .bind(&doc.filename)
        .bind(doc.file_type.as_deref().unwrap_or("bin"))
        .bind(doc.size_bytes.unwrap_or(bytes.len() as u64) as i64)
        .bind(&storage_key)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
        doc_id_remap.insert(doc.id.clone(), new_doc_id);
    }

    // Tabular reviews: just config, fresh UUIDs.
    for tr in &payload.tabular_reviews {
        let new_id = uuid::Uuid::new_v4().to_string();
        let cfg_str = serde_json::to_string(&tr.columns_config)
            .unwrap_or_else(|_| "[]".to_string());
        let _ = sqlx::query(
            "INSERT INTO tabular_reviews \
             (id, user_id, project_id, title, columns_config, status, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, 'pending', datetime('now'), datetime('now'))",
        )
        .bind(&new_id)
        .bind(&auth.user_id)
        .bind(&new_project_id)
        .bind(tr.title.as_deref().unwrap_or("Untitled Review"))
        .bind(&cfg_str)
        .execute(&state.db)
        .await;
    }

    // Custom workflows: recreate with fresh UUIDs. Restore the rich
    // shape (type/practice/columns_config) shipped by 0010_workflows_extend.
    for wf in &payload.workflows {
        let new_id = uuid::Uuid::new_v4().to_string();
        let cols_text = wf
            .columns_config
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "[]".to_string());
        let _ = sqlx::query(
            "INSERT INTO workflows \
             (id, user_id, title, prompt_md, type, practice, columns_config) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&new_id)
        .bind(&auth.user_id)
        .bind(&wf.title)
        .bind(wf.prompt_md.as_deref().unwrap_or(""))
        .bind(&wf.r#type)
        .bind(&wf.practice)
        .bind(&cols_text)
        .execute(&state.db)
        .await;
    }

    // Chats (only when the export included them).
    let mut chat_count = 0u32;
    for c in &payload.chats {
        let new_chat_id = uuid::Uuid::new_v4().to_string();
        if sqlx::query(
            "INSERT INTO chats (id, user_id, project_id, title, created_at, updated_at) \
             VALUES (?, ?, ?, ?, datetime('now'), datetime('now'))",
        )
        .bind(&new_chat_id)
        .bind(&auth.user_id)
        .bind(&new_project_id)
        .bind(c.title.as_deref())
        .execute(&state.db)
        .await
        .is_ok()
        {
            chat_count += 1;
            for m in &c.messages {
                let role = m
                    .get("role")
                    .and_then(|r| r.as_str())
                    .unwrap_or("user");
                let content = m
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
                let _ = sqlx::query(
                    "INSERT INTO messages (id, chat_id, role, content) VALUES (?, ?, ?, ?)",
                )
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(&new_chat_id)
                .bind(role)
                .bind(content)
                .execute(&state.db)
                .await;
            }
        }
    }

    Ok(Json(json!({
        "ok": true,
        "project_id": new_project_id,
        "document_count": doc_id_remap.len(),
        "chat_count": chat_count,
    })))
}

// ---------------------------------------------------------------------------
// PATCH /project/:id/documents/:doc_id  — rename a project document
// ---------------------------------------------------------------------------
//
// Mirror of upstream willchen96/mike `f39f175` endpoint
// PATCH /projects/:projectId/documents/:documentId. Scope-reduced for
// MikeRust's leaner schema:
//   - Upstream also bumps documents.updated_at and propagates
//     document_versions.display_name on current_version_id. MikeRust's
//     documents table has no updated_at column and document_versions
//     has no display_name; both are upstream-only additions to a
//     larger version-tracking pipeline we haven't ported. The rename
//     here updates only `documents.filename`.
//   - Ownership is enforced via (id, project_id, user_id) on the
//     UPDATE so a caller can't rename someone else's doc by guessing
//     UUIDs (the same defense MikeRust uses for project-level edits).

#[derive(Deserialize)]
struct RenameDocumentBody {
    filename: String,
}

/// Normalise a user-supplied filename:
///   - trim whitespace, cap at 200 chars
///   - reject empty after trim
///   - preserve the current extension when the new name has no
///     extension (avoids accidental "report" overwriting "report.pdf")
fn normalize_document_filename(
    next_name: &str,
    current_name: &str,
) -> Option<String> {
    let trimmed: String = next_name.trim().chars().take(200).collect();
    if trimmed.is_empty() {
        return None;
    }
    // Has its own extension? e.g. "report.pdf" or "X.docx"
    let has_ext = trimmed
        .rsplit_once('.')
        .map(|(_, ext)| {
            !ext.is_empty()
                && ext.len() <= 6
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
        })
        .unwrap_or(false);
    if has_ext {
        return Some(trimmed);
    }
    // Append current extension if any.
    let cur_ext = current_name
        .rsplit_once('.')
        .filter(|(_, e)| {
            !e.is_empty()
                && e.len() <= 6
                && e.chars().all(|c| c.is_ascii_alphanumeric())
        })
        .map(|(_, e)| format!(".{e}"))
        .unwrap_or_default();
    Some(format!("{trimmed}{cur_ext}"))
}

async fn rename_project_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((project_id, doc_id)): Path<(String, String)>,
    Json(body): Json<RenameDocumentBody>,
) -> ApiResult {
    // Confirm the doc belongs to this project + this user before we
    // accept the new name. Returns the current filename so the
    // normaliser can preserve its extension if the user only typed a
    // bare name.
    let current: Option<(String,)> = sqlx::query_as(
        "SELECT filename FROM documents \
         WHERE id = ? AND project_id = ? AND user_id = ?",
    )
    .bind(&doc_id)
    .bind(&project_id)
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (current_filename,) = current
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "Document not found"))?;

    let new_filename = normalize_document_filename(&body.filename, &current_filename)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "filename is required"))?;

    sqlx::query(
        "UPDATE documents SET filename = ? \
         WHERE id = ? AND project_id = ? AND user_id = ?",
    )
    .bind(&new_filename)
    .bind(&doc_id)
    .bind(&project_id)
    .bind(&auth.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({
        "id": doc_id,
        "filename": new_filename,
        "project_id": project_id,
    })))
}

#[cfg(test)]
mod tests {
    use super::normalize_document_filename;

    #[test]
    fn normalize_keeps_user_supplied_extension() {
        assert_eq!(
            normalize_document_filename("report-final.pdf", "old.pdf").as_deref(),
            Some("report-final.pdf"),
        );
    }

    #[test]
    fn normalize_appends_current_extension_when_missing() {
        assert_eq!(
            normalize_document_filename("report-final", "old.pdf").as_deref(),
            Some("report-final.pdf"),
        );
    }

    #[test]
    fn normalize_appends_docx_extension() {
        assert_eq!(
            normalize_document_filename("Notes", "draft.docx").as_deref(),
            Some("Notes.docx"),
        );
    }

    #[test]
    fn normalize_trims_whitespace_and_caps_at_200() {
        let huge: String = std::iter::repeat('a').take(250).collect();
        let out = normalize_document_filename(&format!("   {huge}  "), "x.pdf")
            .unwrap();
        // 200 'a's plus ".pdf" appended (input has no extension).
        assert_eq!(out.len(), 204);
        assert!(out.starts_with('a'));
        assert!(out.ends_with(".pdf"));
    }

    #[test]
    fn normalize_rejects_empty() {
        assert_eq!(normalize_document_filename("", "x.pdf"), None);
        assert_eq!(normalize_document_filename("   ", "x.pdf"), None);
    }

    #[test]
    fn normalize_handles_no_current_extension() {
        // No source extension → user name kept as-is even if bare.
        assert_eq!(
            normalize_document_filename("untitled", "blob").as_deref(),
            Some("untitled"),
        );
    }

    #[test]
    fn normalize_distinguishes_dot_in_middle_from_extension() {
        // "my.file.v2" → ext is ".v2"; recognised as having extension.
        assert_eq!(
            normalize_document_filename("my.file.v2", "old.pdf").as_deref(),
            Some("my.file.v2"),
        );
    }
}
