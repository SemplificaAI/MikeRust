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
        // Document folder tree (per project).
        .route("/{id}/folders", get(list_folders).post(create_folder))
        .route(
            "/{id}/folders/{folder_id}",
            axum::routing::patch(update_folder).delete(delete_folder),
        )
        .route(
            "/{id}/documents/{doc_id}/folder",
            axum::routing::patch(move_project_document),
        )
}

// ---------------------------------------------------------------------------
// Project document folders — a per-project tree.
// ---------------------------------------------------------------------------

/// 404 unless `project_id` exists and is owned by `user_id`.
async fn verify_project_owner(
    state: &AppState,
    user_id: &str,
    project_id: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM projects WHERE id = ? AND user_id = ?")
            .bind(project_id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    if row.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "Project not found"));
    }
    Ok(())
}

/// 404 unless `folder_id` exists inside `project_id`.
async fn folder_in_project(
    state: &AppState,
    project_id: &str,
    folder_id: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM project_folders WHERE id = ? AND project_id = ?",
    )
    .bind(folder_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    if row.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "Folder not found"));
    }
    Ok(())
}

/// True when re-parenting `folder_id` under `new_parent` would create a
/// cycle — i.e. `new_parent` is `folder_id` itself or one of its
/// descendants. Walks the parent chain over a one-shot snapshot.
async fn would_create_cycle(
    state: &AppState,
    project_id: &str,
    folder_id: &str,
    new_parent: &str,
) -> Result<bool, (StatusCode, Json<Value>)> {
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT id, parent_id FROM project_folders WHERE project_id = ?",
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let parent_of: std::collections::HashMap<String, Option<String>> =
        rows.into_iter().collect();

    let mut cur = Some(new_parent.to_string());
    let mut hops = 0usize;
    while let Some(c) = cur {
        if c == folder_id {
            return Ok(true);
        }
        hops += 1;
        if hops > 100_000 {
            // Defensive: a pre-existing cycle would loop forever.
            return Ok(true);
        }
        cur = parent_of.get(&c).cloned().flatten();
    }
    Ok(false)
}

async fn list_folders(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(project_id): Path<String>,
) -> ApiResult {
    verify_project_owner(&state, &auth.user_id, &project_id).await?;
    let rows: Vec<(String, Option<String>, String, String)> = sqlx::query_as(
        "SELECT id, parent_id, name, created_at FROM project_folders \
         WHERE project_id = ? ORDER BY name COLLATE NOCASE",
    )
    .bind(&project_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let folders: Vec<Value> = rows
        .into_iter()
        .map(|(id, parent_id, name, created_at)| {
            json!({ "id": id, "parent_id": parent_id, "name": name, "created_at": created_at })
        })
        .collect();
    Ok(Json(json!({ "folders": folders })))
}

#[derive(Deserialize)]
struct CreateFolderBody {
    name: String,
    parent_id: Option<String>,
}

async fn create_folder(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(project_id): Path<String>,
    Json(body): Json<CreateFolderBody>,
) -> ApiResult {
    verify_project_owner(&state, &auth.user_id, &project_id).await?;
    let name = body.name.trim();
    if name.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "folder name is required"));
    }
    if let Some(pid) = &body.parent_id {
        folder_in_project(&state, &project_id, pid).await?;
    }
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO project_folders (id, project_id, parent_id, name) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&project_id)
    .bind(&body.parent_id)
    .bind(name)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    Ok(Json(json!({ "id": id, "parent_id": body.parent_id, "name": name })))
}

/// PATCH a folder — rename (`name`) and/or move (`parent_id`). A raw
/// `Value` body lets us tell "move to root" (`parent_id: null`) apart
/// from "leave parent unchanged" (field absent).
async fn update_folder(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((project_id, folder_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    verify_project_owner(&state, &auth.user_id, &project_id).await?;
    folder_in_project(&state, &project_id, &folder_id).await?;

    if let Some(name) = body.get("name").and_then(|v| v.as_str()) {
        let name = name.trim();
        if name.is_empty() {
            return Err(err(StatusCode::BAD_REQUEST, "folder name is required"));
        }
        sqlx::query("UPDATE project_folders SET name = ? WHERE id = ?")
            .bind(name)
            .bind(&folder_id)
            .execute(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    }

    if let Some(pv) = body.get("parent_id") {
        let new_parent = pv.as_str(); // None when JSON null → move to root
        if let Some(pid) = new_parent {
            if pid == folder_id {
                return Err(err(
                    StatusCode::BAD_REQUEST,
                    "a folder cannot be its own parent",
                ));
            }
            folder_in_project(&state, &project_id, pid).await?;
            if would_create_cycle(&state, &project_id, &folder_id, pid).await? {
                return Err(err(
                    StatusCode::BAD_REQUEST,
                    "cannot move a folder into its own subtree",
                ));
            }
        }
        sqlx::query("UPDATE project_folders SET parent_id = ? WHERE id = ?")
            .bind(new_parent)
            .bind(&folder_id)
            .execute(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    }
    Ok(Json(json!({ "id": folder_id })))
}

async fn delete_folder(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((project_id, folder_id)): Path<(String, String)>,
) -> ApiResult {
    verify_project_owner(&state, &auth.user_id, &project_id).await?;
    folder_in_project(&state, &project_id, &folder_id).await?;
    // Subfolders cascade (parent_id FK); documents fall back to the
    // project root (project_folder_id FK SET NULL).
    sqlx::query("DELETE FROM project_folders WHERE id = ? AND project_id = ?")
        .bind(&folder_id)
        .bind(&project_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    Ok(Json(json!({ "ok": true, "id": folder_id })))
}

#[derive(Deserialize)]
struct MoveDocumentBody {
    /// Target folder; `null` / absent moves the document to the project root.
    folder_id: Option<String>,
}

async fn move_project_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((project_id, doc_id)): Path<(String, String)>,
    Json(body): Json<MoveDocumentBody>,
) -> ApiResult {
    verify_project_owner(&state, &auth.user_id, &project_id).await?;
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM documents \
         WHERE id = ? AND project_id = ? AND user_id = ?",
    )
    .bind(&doc_id)
    .bind(&project_id)
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    if row.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "Document not found"));
    }
    if let Some(fid) = &body.folder_id {
        folder_in_project(&state, &project_id, fid).await?;
    }
    sqlx::query(
        "UPDATE documents SET project_folder_id = ? \
         WHERE id = ? AND project_id = ?",
    )
    .bind(&body.folder_id)
    .bind(&doc_id)
    .bind(&project_id)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    Ok(Json(json!({ "id": doc_id, "folder_id": body.folder_id })))
}

// ---------------------------------------------------------------------------
// GET /project  — list all projects for the authenticated user
// ---------------------------------------------------------------------------
async fn list_projects(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> ApiResult {
    let domain_filter: Option<&str> = params
        .get("domain")
        .map(|s| s.as_str())
        .filter(|s| !s.is_empty() && crate::domain::is_valid(s));

    let rows: Vec<(String, String, Option<String>, String, String, String)> = if let Some(d) =
        domain_filter
    {
        sqlx::query_as(
            "SELECT id, name, description, created_at, updated_at, domain \
             FROM projects WHERE user_id = ? AND domain = ? ORDER BY updated_at DESC",
        )
        .bind(&auth.user_id)
        .bind(d)
        .fetch_all(&state.db)
        .await
    } else {
        sqlx::query_as(
            "SELECT id, name, description, created_at, updated_at, domain \
             FROM projects WHERE user_id = ? ORDER BY updated_at DESC",
        )
        .bind(&auth.user_id)
        .fetch_all(&state.db)
        .await
    }
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let projects: Vec<Value> = rows
        .into_iter()
        .map(|(id, name, desc, created_at, updated_at, domain)| {
            json!({ "id": id, "name": name, "description": desc,
                    "domain": domain,
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
    /// Professional vertical — see `crate::domain::DOMAINS`. Falls back
    /// to `legal` (schema default) when omitted/invalid.
    #[serde(default)]
    domain: Option<String>,
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
    let dom = crate::domain::normalise_or_default(body.domain.as_deref());
    sqlx::query(
        "INSERT INTO projects (id, user_id, name, description, domain) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(dom)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({ "id": id, "name": body.name.trim(), "domain": dom })))
}

// ---------------------------------------------------------------------------
// GET /project/:id
// ---------------------------------------------------------------------------
async fn get_project(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let row: Option<(String, String, Option<String>, String, String, String, String)> =
        sqlx::query_as(
            "SELECT id, name, description, created_at, updated_at, isolation_mode, domain \
             FROM projects WHERE id = ? AND user_id = ?",
        )
        .bind(&id)
        .bind(&auth.user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (id, name, desc, created_at, updated_at, isolation_mode, domain) =
        row.ok_or_else(|| err(StatusCode::NOT_FOUND, "Project not found"))?;

    Ok(Json(json!({
        "id": id, "name": name, "description": desc,
        "created_at": created_at, "updated_at": updated_at,
        "isolation_mode": isolation_mode,
        "domain": domain,
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
    domain: Option<String>,
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
    if let Some(ref d) = body.domain {
        if !crate::domain::is_valid(d) {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "domain must be a canonical value (legal, medical, finance, real_estate, hr, insurance, ip, compliance, others)",
            ));
        }
    }

    let result = sqlx::query(
        "UPDATE projects SET \
           name = COALESCE(?, name), \
           description = COALESCE(?, description), \
           isolation_mode = COALESCE(?, isolation_mode), \
           domain = COALESCE(?, domain), \
           updated_at = datetime('now') \
         WHERE id = ? AND user_id = ?",
    )
    .bind(&body.name)
    .bind(&body.description)
    .bind(&body.isolation_mode)
    .bind(&body.domain)
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

    // Create the new project under the importer's account. v0.5.4
    // amendment: domain + isolation_mode now travel and are honoured
    // here (with sensible fallbacks via COALESCE so older archives
    // that lacked these fields still land on the schema defaults).
    let new_project_id = uuid::Uuid::new_v4().to_string();
    let project_domain = payload
        .project
        .domain
        .as_deref()
        .filter(|s| crate::domain::is_valid(s))
        .unwrap_or("legal");
    let isolation_mode = payload
        .project
        .isolation_mode
        .as_deref()
        .filter(|s| *s == "shared" || *s == "strict")
        .unwrap_or("shared");
    sqlx::query(
        "INSERT INTO projects \
            (id, user_id, name, cm_number, created_at, updated_at, domain, isolation_mode) \
         VALUES (?, ?, ?, ?, datetime('now'), datetime('now'), ?, ?)",
    )
    .bind(&new_project_id)
    .bind(&auth.user_id)
    .bind(&payload.project.name)
    .bind(payload.project.cm_number.as_deref())
    .bind(project_domain)
    .bind(isolation_mode)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    // Documents: write each blob into the importer's storage with a
    // fresh document_id, then row in `documents`.
    //
    // v0.5.4 amendment: per-doc domain (falls back to the project's
    // domain when absent), decision tuple (migration 0029), and the
    // content_hash (re-derived from sha256 in the manifest so the
    // recipient's tabular dedup-by-hash works immediately, not only
    // after a manual re-upload).
    //
    // `project_folder_id` is intentionally left NULL on import: the
    // original folder tree isn't reconstructed in this round —
    // rebuilding `project_folders` from scratch and remapping every
    // doc's parent id is its own change; future work tracked
    // alongside this one.
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
        let doc_domain = doc
            .domain
            .as_deref()
            .filter(|s| crate::domain::is_valid(s))
            .unwrap_or(project_domain);
        let decision = doc
            .decision
            .as_deref()
            .filter(|s| *s == "accepted" || *s == "rejected")
            .unwrap_or("accepted");
        sqlx::query(
            "INSERT INTO documents \
             (id, user_id, project_id, filename, file_type, size_bytes, storage_path, \
              status, created_at, domain, content_hash, decision, decision_reason, decision_summary) \
             VALUES (?, ?, ?, ?, ?, ?, ?, 'ready', datetime('now'), ?, ?, ?, ?, ?)",
        )
        .bind(&new_doc_id)
        .bind(&auth.user_id)
        .bind(&new_project_id)
        .bind(&doc.filename)
        .bind(doc.file_type.as_deref().unwrap_or("bin"))
        .bind(doc.size_bytes.unwrap_or(bytes.len() as u64) as i64)
        .bind(&storage_key)
        .bind(doc_domain)
        .bind(&doc.sha256)
        .bind(decision)
        .bind(doc.decision_reason.as_deref())
        .bind(doc.decision_summary.as_deref())
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
        doc_id_remap.insert(doc.id.clone(), new_doc_id);
    }

    // Tabular reviews: just config, fresh UUIDs. v0.5.4 amendment:
    // the review-level domain travels and is honoured here.
    for tr in &payload.tabular_reviews {
        let new_id = uuid::Uuid::new_v4().to_string();
        let cfg_str = serde_json::to_string(&tr.columns_config)
            .unwrap_or_else(|_| "[]".to_string());
        let tr_domain = tr
            .domain
            .as_deref()
            .filter(|s| crate::domain::is_valid(s))
            .unwrap_or(project_domain);
        let _ = sqlx::query(
            "INSERT INTO tabular_reviews \
             (id, user_id, project_id, title, columns_config, status, created_at, updated_at, domain) \
             VALUES (?, ?, ?, ?, ?, 'pending', datetime('now'), datetime('now'), ?)",
        )
        .bind(&new_id)
        .bind(&auth.user_id)
        .bind(&new_project_id)
        .bind(tr.title.as_deref().unwrap_or("Untitled Review"))
        .bind(&cfg_str)
        .bind(tr_domain)
        .execute(&state.db)
        .await;
    }

    // Custom workflows: recreate with fresh UUIDs. v0.5.4 amendment:
    // the workflow-level domain travels so a `medical` custom
    // workflow exported from a medical project lands as `medical` on
    // the recipient too (instead of falling back to schema-default
    // `legal`, which made it invisible to medical-domain pickers).
    for wf in &payload.workflows {
        let new_id = uuid::Uuid::new_v4().to_string();
        let cols_text = wf
            .columns_config
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "[]".to_string());
        let wf_domain = wf
            .domain
            .as_deref()
            .filter(|s| crate::domain::is_valid(s))
            .unwrap_or(project_domain);
        let _ = sqlx::query(
            "INSERT INTO workflows \
             (id, user_id, title, prompt_md, type, practice, columns_config, domain) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&new_id)
        .bind(&auth.user_id)
        .bind(&wf.title)
        .bind(wf.prompt_md.as_deref().unwrap_or(""))
        .bind(&wf.r#type)
        .bind(&wf.practice)
        .bind(&cols_text)
        .bind(wf_domain)
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
