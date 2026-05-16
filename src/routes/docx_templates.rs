//! `/docx-templates` — the DOCX template registry.
//!
//! Each entry is a "closing formatter" template: structured JSON
//! metadata the renderer turns into a print-ready `.docx`. Two sources
//! are merged:
//!
//!   * **System templates** — shipped under `config/docx-templates/`
//!     and loaded once at startup into `state.docx_templates`. Read-only.
//!   * **User templates** — JSON files under `config/docx-templates/user/`,
//!     created and edited through the template editor. Writable.
//!
//! User-template ids carry a `user/` prefix; the file basename is the
//! single path segment after that prefix. Read endpoints re-scan the
//! `user/` folder per call so edits show up without an app restart.
//!
//! Template ids contain `/` (e.g. `it/diffida-messa-in-mora`,
//! `user/contratto-mio`), so every id travels in a JSON body parameter
//! rather than a URL path segment — sidesteps client URL-encoding pitfalls.

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::presets::docx_template::{load_docx_templates, validate, DocxTemplate};
use crate::{auth::middleware::AuthUser, AppState};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_docx_templates))
        .route("/describe", post(describe_docx_template))
        .route("/render", post(render_docx_template))
        .route("/save", post(save_docx_template))
        .route("/delete", post(delete_docx_template))
        .route("/hidden", get(list_hidden_templates).post(hide_template))
        .route("/unhide", post(unhide_template))
}

// ─────────────────────────────────────────────────────────────────────
// User-template helpers
// ─────────────────────────────────────────────────────────────────────

/// `config/docx-templates/user/` — where writable user templates live.
fn user_templates_dir() -> PathBuf {
    crate::presets::config_subdir("docx-templates").join("user")
}

/// User templates are identified by a `user/` id prefix.
fn is_user_id(id: &str) -> bool {
    id.starts_with("user/")
}

/// Validate and extract the single safe path segment after `user/`.
/// Rejects anything that could escape the folder (`..`, separators) or
/// produce an awkward filename — the slug is lowercase alphanumerics
/// plus `-`/`_`, must start alphanumeric, max 80 chars.
fn user_slug(id: &str) -> Option<String> {
    let slug = id.strip_prefix("user/")?;
    if slug.is_empty() || slug.len() > 80 {
        return None;
    }
    let mut chars = slug.chars();
    let first_ok = chars
        .next()
        .map(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .unwrap_or(false);
    let rest_ok = slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    (first_ok && rest_ok).then(|| slug.to_string())
}

/// System templates (immutable, startup-loaded) merged with the current
/// on-disk user templates. The `user/` folder is tiny and re-reading it
/// per call keeps the API consistent with editor changes without an app
/// restart. Sorted by id for a stable listing.
fn merged_templates(state: &AppState) -> Vec<DocxTemplate> {
    let mut out: Vec<DocxTemplate> = state
        .docx_templates
        .iter()
        .filter(|t| !is_user_id(&t.id))
        .cloned()
        .collect();
    if let Ok(user) = load_docx_templates(&user_templates_dir()) {
        out.extend(user);
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Render one template as API JSON, picking the system / user flags.
fn template_to_api_json(t: &DocxTemplate) -> Value {
    if is_user_id(&t.id) {
        t.to_api_json_user()
    } else {
        t.to_api_json()
    }
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    /// Optional domain filter — `?domain=legal` returns only templates
    /// in the canonical-domain "legal" bucket. Omit to get all.
    domain: Option<String>,
    /// Optional locale filter — `?locale=it` returns templates whose
    /// `locale` field starts with `"it"`. Omit to get all.
    locale: Option<String>,
}

async fn list_docx_templates(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(q): Query<ListQuery>,
) -> ApiResult {
    // Cross-domain matching: a template matches the filter when its
    // primary `domain` equals the query OR when the query is listed
    // in `also_applicable_to`. Lets a Parcella (primary finance)
    // surface for users with default_domain=legal.
    let items: Vec<Value> = merged_templates(&state)
        .iter()
        .filter(|t| t.matches_domain(q.domain.as_deref()))
        .filter(|t| {
            q.locale
                .as_deref()
                .is_none_or(|l| t.locale.starts_with(l))
        })
        .map(template_to_api_json)
        .collect();
    Ok(Json(json!({ "docx_templates": items })))
}

#[derive(Debug, Deserialize)]
struct DescribeBody {
    template_id: String,
    #[serde(default)]
    locale: Option<String>,
}

/// `POST /docx-templates/describe` — returns the full authoring
/// contract for a template (auto-generated prompt_md + sidecar).
/// Same payload as the `describe_docx_template` LLM tool, exposed to
/// the frontend so the workflow editor can preview the contract.
async fn describe_docx_template(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(body): Json<DescribeBody>,
) -> ApiResult {
    let templates = merged_templates(&state);
    let Some(template) = templates.iter().find(|t| t.id == body.template_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("template {} not found", body.template_id) })),
        ));
    };
    let locale = body.locale.as_deref().unwrap_or("it");
    Ok(Json(json!({
        "template_id": template.id,
        "display_name": template.display_name_for(locale),
        "prompt_md": template.auto_generated_prompt_md(locale),
        "sidecar": template_to_api_json(template),
    })))
}

#[derive(Debug, Deserialize)]
struct RenderBody {
    template_id: String,
    body_md: String,
    #[serde(default)]
    metadata: HashMap<String, String>,
    /// Optional override for the download filename. When omitted, the
    /// template's `display_name_for("it")` is sanitised and `.docx`
    /// appended.
    #[serde(default)]
    filename: Option<String>,
}

/// `POST /docx-templates/render` — render a template + body + metadata
/// to a `.docx`, stream the bytes back with the right Content-Type
/// and `Content-Disposition: attachment; filename=...`.
///
/// This is the frontend-facing equivalent of the `generate_docx` LLM
/// tool, but it does NOT persist the document — bytes go straight to
/// the response. The UI uses this for "anteprima" / "scarica subito"
/// flows without polluting the user's documents list.
async fn render_docx_template(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(body): Json<RenderBody>,
) -> Result<axum::response::Response, (StatusCode, Json<Value>)> {
    let templates = merged_templates(&state);
    let Some(template) = templates.iter().find(|t| t.id == body.template_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("template {} not found", body.template_id) })),
        ));
    };

    if body.body_md.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "body_md is required and must be non-empty" })),
        ));
    }

    let outcome = crate::docx::render(template, &body.body_md, &body.metadata)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("render: {e}") })),
            )
        })?;

    let title = body.filename.unwrap_or_else(|| template.display_name_for("it"));
    let safe = sanitize_filename(&title);
    let filename = format!("{safe}.docx");

    // Surface unresolved placeholders in a custom header so the
    // frontend can warn the user without parsing the binary.
    let mut response = (
        [(
            header::CONTENT_TYPE,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        )],
        outcome.bytes,
    )
        .into_response();
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{filename}\"")
            .parse()
            .unwrap(),
    );
    if !outcome.unresolved_placeholders.is_empty() {
        response.headers_mut().insert(
            "X-Unresolved-Placeholders",
            outcome
                .unresolved_placeholders
                .join(",")
                .parse()
                .unwrap_or_else(|_| "?".parse().unwrap()),
        );
    }
    Ok(response)
}

/// `POST /docx-templates/save` — create or update a user template.
///
/// The request body is a full `DocxTemplate` definition. The `id` must
/// carry the `user/` prefix (system templates are never writable here);
/// the slug after the prefix becomes the JSON filename under
/// `config/docx-templates/user/`. The definition is validated against
/// the same invariants the startup loader enforces before it touches
/// disk. Re-saving an existing id overwrites it (the editor's update
/// path); a fresh id creates a new file.
async fn save_docx_template(
    _auth: AuthUser,
    Json(template): Json<DocxTemplate>,
) -> ApiResult {
    let Some(slug) = user_slug(&template.id) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "template id must be 'user/<slug>' — slug is lowercase \
                          alphanumerics with '-'/'_', starting alphanumeric. \
                          System templates cannot be edited."
            })),
        ));
    };

    if let Err(reason) = validate(&template) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": reason })),
        ));
    }

    let dir = user_templates_dir();
    std::fs::create_dir_all(&dir).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("create user templates dir: {e}") })),
        )
    })?;

    let pretty = serde_json::to_string_pretty(&template).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("serialize template: {e}") })),
        )
    })?;

    let path = dir.join(format!("{slug}.json"));
    std::fs::write(&path, pretty).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("write template: {e}") })),
        )
    })?;

    tracing::info!("[docx-templates] saved user template {}", template.id);
    Ok(Json(json!({
        "saved": true,
        "template": template.to_api_json_user(),
    })))
}

#[derive(Debug, Deserialize)]
struct DeleteBody {
    template_id: String,
}

/// `POST /docx-templates/delete` — delete a user template by id.
/// System templates are rejected; the `user/` slug is resolved to a
/// single file under `config/docx-templates/user/`.
async fn delete_docx_template(
    _auth: AuthUser,
    Json(body): Json<DeleteBody>,
) -> ApiResult {
    let Some(slug) = user_slug(&body.template_id) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "only user templates ('user/<slug>') can be deleted" })),
        ));
    };

    let path = user_templates_dir().join(format!("{slug}.json"));
    if !path.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("template {} not found", body.template_id) })),
        ));
    }
    std::fs::remove_file(&path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("delete template: {e}") })),
        )
    })?;

    tracing::info!("[docx-templates] deleted user template {}", body.template_id);
    Ok(Json(json!({ "deleted": true })))
}

/// `GET /docx-templates/hidden` — ids of templates the user has hidden.
async fn list_hidden_templates(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT template_id FROM docx_template_hidden WHERE user_id = ?")
            .bind(&auth.user_id)
            .fetch_all(&state.db)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?;
    let ids: Vec<String> = rows.into_iter().map(|(id,)| id).collect();
    Ok(Json(json!(ids)))
}

#[derive(Debug, Deserialize)]
struct HideBody {
    template_id: String,
}

/// `POST /docx-templates/hidden` — hide a template (system or user) from
/// the user's listing. The id is free text — no foreign key.
async fn hide_template(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<HideBody>,
) -> ApiResult {
    sqlx::query("INSERT OR IGNORE INTO docx_template_hidden (user_id, template_id) VALUES (?, ?)")
        .bind(&auth.user_id)
        .bind(&body.template_id)
        .execute(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    Ok(Json(json!({ "ok": true })))
}

/// `POST /docx-templates/unhide` — restore a previously hidden template.
async fn unhide_template(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<HideBody>,
) -> ApiResult {
    sqlx::query("DELETE FROM docx_template_hidden WHERE user_id = ? AND template_id = ?")
        .bind(&auth.user_id)
        .bind(&body.template_id)
        .execute(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    Ok(Json(json!({ "ok": true })))
}

/// Strip characters that are problematic in filenames on either
/// Windows or POSIX, collapse whitespace, cap length.
fn sanitize_filename(input: &str) -> String {
    let cleaned: String = input
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c => c,
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim_matches(|c: char| c == '.' || c == '-');
    let s: String = trimmed.chars().take(80).collect();
    if s.is_empty() { "Documento".to_string() } else { s }
}

#[cfg(test)]
mod tests {
    use super::{is_user_id, sanitize_filename, user_slug};

    #[test]
    fn is_user_id_only_matches_the_user_prefix() {
        assert!(is_user_id("user/contratto-mio"));
        assert!(!is_user_id("it/diffida-messa-in-mora"));
        assert!(!is_user_id("compliance/procedura-iso-sgi"));
        assert!(!is_user_id("userish/thing"));
    }

    #[test]
    fn user_slug_accepts_a_clean_segment() {
        assert_eq!(user_slug("user/contratto-mio").as_deref(), Some("contratto-mio"));
        assert_eq!(user_slug("user/atto_2026").as_deref(), Some("atto_2026"));
        assert_eq!(user_slug("user/t1").as_deref(), Some("t1"));
    }

    #[test]
    fn user_slug_rejects_system_and_malformed_ids() {
        // No user/ prefix → not a writable template.
        assert_eq!(user_slug("it/diffida"), None);
        // Path traversal attempts.
        assert_eq!(user_slug("user/../secret"), None);
        assert_eq!(user_slug("user/sub/nested"), None);
        // Empty slug.
        assert_eq!(user_slug("user/"), None);
        // Must start alphanumeric.
        assert_eq!(user_slug("user/-leading"), None);
        assert_eq!(user_slug("user/_leading"), None);
        // No uppercase / spaces / dots.
        assert_eq!(user_slug("user/MyTemplate"), None);
        assert_eq!(user_slug("user/my template"), None);
        assert_eq!(user_slug("user/my.template"), None);
    }

    #[test]
    fn user_slug_caps_length_at_80() {
        let ok = format!("user/{}", "a".repeat(80));
        assert!(user_slug(&ok).is_some());
        let too_long = format!("user/{}", "a".repeat(81));
        assert_eq!(user_slug(&too_long), None);
    }

    #[test]
    fn sanitize_replaces_each_unsafe_char_with_dash() {
        // Every char that's reserved on Windows (`\/:*?"<>|`) plus
        // forward-slash (POSIX path separator we don't want either).
        for c in r#"\/:*?"<>|"#.chars() {
            let input = format!("foo{c}bar");
            let out = sanitize_filename(&input);
            assert!(
                !out.contains(c),
                "char {c:?} survived in {out:?} (from {input:?})"
            );
            assert!(out.contains('-'), "expected dash replacement, got {out:?}");
        }
    }

    #[test]
    fn sanitize_collapses_internal_whitespace() {
        assert_eq!(
            sanitize_filename("  Atto    difensivo\t\tCaio"),
            "Atto difensivo Caio"
        );
    }

    #[test]
    fn sanitize_trims_leading_and_trailing_dots_and_dashes() {
        assert_eq!(sanitize_filename("...Documento..."), "Documento");
        assert_eq!(sanitize_filename("---Atto---"), "Atto");
        // Trailing slashes turn into dashes first then trim.
        assert_eq!(sanitize_filename("/Atto/"), "Atto");
    }

    #[test]
    fn sanitize_caps_at_80_chars() {
        let huge = "a".repeat(200);
        let out = sanitize_filename(&huge);
        assert_eq!(out.chars().count(), 80);
    }

    #[test]
    fn sanitize_falls_back_to_documento_on_empty_or_garbage() {
        assert_eq!(sanitize_filename(""), "Documento");
        assert_eq!(sanitize_filename("   "), "Documento");
        assert_eq!(sanitize_filename("..."), "Documento");
        assert_eq!(sanitize_filename("---"), "Documento");
        // Only unsafe chars → all turn to dashes → trimmed to nothing.
        assert_eq!(sanitize_filename(r#"\\\:::"""#), "Documento");
    }

    #[test]
    fn sanitize_preserves_italian_accents_and_apostrophes() {
        // Italian display names commonly carry accents; they're safe
        // on every modern filesystem and must survive sanitisation.
        let out = sanitize_filename("Perizia d'ufficio – è urgente");
        assert!(out.contains("d'ufficio"));
        assert!(out.contains("è"));
        assert!(out.contains("–"));
    }

    #[test]
    fn sanitize_caps_at_80_chars_when_truncating_template_display_name() {
        // Real-world case: a long template display name shouldn't blow
        // up Windows' 255-char limit on the full path.
        let out =
            sanitize_filename("Contratto di locazione ad uso abitativo per immobile sito in Cremona Via Roma 1 piano terzo interno 4");
        assert!(out.chars().count() <= 80);
        // We didn't accidentally cut in the middle of an accented
        // codepoint (chars().take prevents that).
        assert!(out.is_char_boundary(out.len()));
    }
}
