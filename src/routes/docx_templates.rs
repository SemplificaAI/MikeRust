//! `/docx-templates` — read-only view onto the DOCX template registry
//! loaded from `config/docx-templates/<domain>/<slug>.json` at startup.
//!
//! Each entry is a "closing formatter" template — sidecar JSON metadata
//! plus a companion `.dotx` Word file. The frontend consumes this
//! endpoint to populate the Settings → Templates DOCX picker and the
//! "Default output template" combo in the workflow editor.
//!
//! No write endpoint today — to add or modify a template, drop the
//! `.dotx` + sidecar pair into the right folder and restart. Follows
//! the same JSON-driven pattern as workflows / column-presets / models.

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
use std::sync::Arc;

use crate::{auth::middleware::AuthUser, AppState};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

pub fn router() -> Router<Arc<AppState>> {
    // Template ids contain `/` (e.g. "it/diffida-messa-in-mora") so
    // we pass them in JSON body parameters rather than URL path
    // segments — sidesteps URL-encoding pitfalls in clients.
    Router::new()
        .route("/", get(list_docx_templates))
        .route("/describe", post(describe_docx_template))
        .route("/render", post(render_docx_template))
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
    let items: Vec<Value> = state
        .docx_templates
        .iter()
        .filter(|t| q.domain.as_deref().is_none_or(|d| t.domain == d))
        .filter(|t| {
            q.locale
                .as_deref()
                .is_none_or(|l| t.locale.starts_with(l))
        })
        .map(|t| t.to_api_json())
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
    let Some(template) = state
        .docx_templates
        .iter()
        .find(|t| t.id == body.template_id)
    else {
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
        "sidecar": template.to_api_json(),
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
    let Some(template) = state
        .docx_templates
        .iter()
        .find(|t| t.id == body.template_id)
    else {
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
    use super::sanitize_filename;

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
