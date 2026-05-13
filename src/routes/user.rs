use axum::{
    extract::State,
    http::StatusCode,
    routing::{delete, get},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::{auth::middleware::AuthUser, AppState};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"detail": msg})))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/profile", get(get_profile).put(update_profile))
        .route("/llm-settings", get(get_llm_settings).put(update_llm_settings))
        .route("/locale", get(get_locale).put(update_locale))
        .route("/account", delete(delete_account))
        .route("/mcp-servers", get(list_mcp_servers).post(upsert_mcp_server))
        .route("/mcp-servers/probe", axum::routing::post(probe_mcp_server))
        .route("/mcp-servers/{name}", delete(delete_mcp_server).put(upsert_mcp_server_named))
}

// ---------------------------------------------------------------------------
// GET /user/locale  →  { locale: "it" | "en" | "fr" | null }
// PUT /user/locale  body { locale: "it" | "en" | "fr" }
//
// Persists the UI locale in user_settings so the choice follows the data
// folder rather than living only in the browser. The Next.js frontend
// keeps a cookie for SSR, but on profile load it reconciles that cookie
// with this value.
// ---------------------------------------------------------------------------
async fn get_locale(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT locale FROM user_settings WHERE user_id = ?")
            .bind(&auth.user_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let locale = row.and_then(|(l,)| l);
    Ok(Json(json!({ "locale": locale })))
}

#[derive(Deserialize)]
struct UpdateLocaleBody {
    locale: String,
}

async fn update_locale(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<UpdateLocaleBody>,
) -> ApiResult {
    let normalized = match body.locale.as_str() {
        "it" | "en" | "fr" => body.locale,
        _ => return Err(err(StatusCode::BAD_REQUEST, "unsupported locale")),
    };
    sqlx::query(
        "INSERT INTO user_settings (user_id, locale, updated_at) \
         VALUES (?, ?, datetime('now')) \
         ON CONFLICT(user_id) DO UPDATE SET locale = excluded.locale, \
             updated_at = datetime('now')",
    )
    .bind(&auth.user_id)
    .bind(&normalized)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    Ok(Json(json!({ "ok": true, "locale": normalized })))
}

// ---------------------------------------------------------------------------
// GET /user/profile
// ---------------------------------------------------------------------------
async fn get_profile(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let row: Option<(String, String, Option<String>, String)> =
        sqlx::query_as(
            "SELECT id, username, display_name, created_at FROM user_profiles WHERE id = ?",
        )
        .bind(&auth.user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (id, username, display_name, created_at) =
        row.ok_or_else(|| err(StatusCode::NOT_FOUND, "Profile not found"))?;

    Ok(Json(json!({
        "id": id,
        "username": username,
        "display_name": display_name,
        "created_at": created_at,
    })))
}

// ---------------------------------------------------------------------------
// PUT /user/profile
// Body: { display_name? }
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct UpdateProfileBody {
    display_name: Option<String>,
}

async fn update_profile(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<UpdateProfileBody>,
) -> ApiResult {
    sqlx::query("UPDATE user_profiles SET display_name = ? WHERE id = ?")
        .bind(&body.display_name)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// GET /user/llm-settings
// ---------------------------------------------------------------------------
#[derive(Default, Serialize, Deserialize)]
pub struct LlmSettings {
    pub main_model: Option<String>,
    pub title_model: Option<String>,
    pub tabular_model: Option<String>,
    pub claude_api_key: Option<String>,
    pub gemini_api_key: Option<String>,
    pub gemini_region: Option<String>,
    pub gemini_model: Option<String>,
    pub openai_api_key: Option<String>,
    pub openai_model: Option<String>,
    pub local_base_url: Option<String>,
    pub local_api_key: Option<String>,
    pub local_model: Option<String>,
    pub active_provider: Option<String>,
}

type LlmRow = (
    Option<String>, // main_model
    Option<String>, // title_model
    Option<String>, // tabular_model
    Option<String>, // claude_api_key
    Option<String>, // gemini_api_key
    Option<String>, // gemini_region
    Option<String>, // gemini_model
    Option<String>, // openai_api_key
    Option<String>, // openai_model
    Option<String>, // local_base_url
    Option<String>, // local_api_key
    Option<String>, // local_model
    Option<String>, // active_provider
);

const SELECT_COLUMNS: &str =
    "main_model, title_model, tabular_model, claude_api_key, gemini_api_key, gemini_region, \
     gemini_model, openai_api_key, openai_model, local_base_url, local_api_key, local_model, active_provider";

pub async fn fetch_llm_settings(
    db: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<LlmSettings, sqlx::Error> {
    let row: Option<LlmRow> = sqlx::query_as(&format!(
        "SELECT {SELECT_COLUMNS} FROM user_settings WHERE user_id = ?"
    ))
    .bind(user_id)
    .fetch_optional(db)
    .await?;

    Ok(row.map(row_to_settings).unwrap_or_default())
}

fn row_to_settings(r: LlmRow) -> LlmSettings {
    LlmSettings {
        main_model: r.0,
        title_model: r.1,
        tabular_model: r.2,
        claude_api_key: r.3,
        gemini_api_key: r.4,
        gemini_region: r.5,
        gemini_model: r.6,
        openai_api_key: r.7,
        openai_model: r.8,
        local_base_url: r.9,
        local_api_key: r.10,
        local_model: r.11,
        active_provider: r.12,
    }
}

async fn get_llm_settings(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let settings = fetch_llm_settings(&state.db, &auth.user_id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    tracing::info!(
        "[user] GET /llm-settings user={} has_gemini_key={} gemini_model={:?} gemini_region={:?} active={:?}",
        auth.user_id,
        settings.gemini_api_key.is_some(),
        settings.gemini_model,
        settings.gemini_region,
        settings.active_provider,
    );
    Ok(Json(serde_json::to_value(settings).unwrap()))
}

// ---------------------------------------------------------------------------
// PUT /user/llm-settings
//
// Patch semantics: every Option<String> field is "unchanged when absent
// or null". This is critical for API keys — the client must be able to
// save other settings (e.g. region, model) without the user re-typing
// the API key. The UPDATE branch uses COALESCE(?, column) so a NULL
// bind keeps the existing value; the INSERT branch (first save) writes
// whatever was provided. To explicitly *clear* a key the client sends
// an empty string (which we treat as null at read time).
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct UpdateLlmSettingsBody {
    #[serde(default)] main_model: Option<String>,
    #[serde(default)] title_model: Option<String>,
    #[serde(default)] tabular_model: Option<String>,
    #[serde(default)] claude_api_key: Option<String>,
    #[serde(default)] gemini_api_key: Option<String>,
    #[serde(default)] gemini_region: Option<String>,
    #[serde(default)] gemini_model: Option<String>,
    #[serde(default)] openai_api_key: Option<String>,
    #[serde(default)] openai_model: Option<String>,
    #[serde(default)] local_base_url: Option<String>,
    #[serde(default)] local_api_key: Option<String>,
    #[serde(default)] local_model: Option<String>,
    #[serde(default)] active_provider: Option<String>,
}

async fn update_llm_settings(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<UpdateLlmSettingsBody>,
) -> ApiResult {
    tracing::info!(
        "[user] PUT /llm-settings user={} dirty={{openai_key:{},claude_key:{},gemini_key:{},gemini_model:{:?},gemini_region:{:?},local_base_url:{:?},local_model:{:?},active_provider:{:?}}}",
        auth.user_id,
        body.openai_api_key.is_some(),
        body.claude_api_key.is_some(),
        body.gemini_api_key.is_some(),
        body.gemini_model,
        body.gemini_region,
        body.local_base_url,
        body.local_model,
        body.active_provider,
    );
    // Two-step upsert:
    //  1. INSERT OR IGNORE → seeds an empty row for first-time users
    //     without clobbering existing values.
    //  2. UPDATE with COALESCE(?, col) → only writes the columns the
    //     client actually sent; absent fields retain whatever was there.
    sqlx::query("INSERT OR IGNORE INTO user_settings (user_id, updated_at) VALUES (?, datetime('now'))")
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    sqlx::query(
        "UPDATE user_settings SET \
            main_model      = COALESCE(?, main_model), \
            title_model     = COALESCE(?, title_model), \
            tabular_model   = COALESCE(?, tabular_model), \
            claude_api_key  = COALESCE(?, claude_api_key), \
            gemini_api_key  = COALESCE(?, gemini_api_key), \
            gemini_region   = COALESCE(?, gemini_region), \
            gemini_model    = COALESCE(?, gemini_model), \
            openai_api_key  = COALESCE(?, openai_api_key), \
            openai_model    = COALESCE(?, openai_model), \
            local_base_url  = COALESCE(?, local_base_url), \
            local_api_key   = COALESCE(?, local_api_key), \
            local_model     = COALESCE(?, local_model), \
            active_provider = COALESCE(?, active_provider), \
            updated_at      = datetime('now') \
         WHERE user_id = ?",
    )
    .bind(&body.main_model)
    .bind(&body.title_model)
    .bind(&body.tabular_model)
    .bind(&body.claude_api_key)
    .bind(&body.gemini_api_key)
    .bind(&body.gemini_region)
    .bind(&body.gemini_model)
    .bind(&body.openai_api_key)
    .bind(&body.openai_model)
    .bind(&body.local_base_url)
    .bind(&body.local_api_key)
    .bind(&body.local_model)
    .bind(&body.active_provider)
    .bind(&auth.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// MCP servers — per-user configurations
//
// Schema mirrors Anthropic's `claude_desktop_config.json`:
//   stdio servers → { command, args, env } (transport: "stdio")
//   remote (HTTP/SSE) → { url, headers, api_key } (transport: "http"|"sse")
// ---------------------------------------------------------------------------
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct McpServerOut {
    pub name: String,
    pub transport: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: serde_json::Map<String, Value>,
    #[serde(default)]
    pub headers: serde_json::Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    pub enabled: bool,
}

fn row_to_server(
    name: String,
    transport: String,
    url: Option<String>,
    command: Option<String>,
    args_json: String,
    env_json: String,
    headers_json: String,
    api_key: Option<String>,
    enabled: i64,
) -> McpServerOut {
    let args: Vec<String> = serde_json::from_str(&args_json).unwrap_or_default();
    let env: serde_json::Map<String, Value> =
        serde_json::from_str(&env_json).unwrap_or_default();
    let headers: serde_json::Map<String, Value> =
        serde_json::from_str(&headers_json).unwrap_or_default();
    McpServerOut {
        name,
        transport,
        url,
        command,
        args,
        env,
        headers,
        api_key,
        enabled: enabled != 0,
    }
}

pub async fn fetch_mcp_servers(
    db: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<Vec<McpServerOut>, sqlx::Error> {
    let rows: Vec<(String, String, Option<String>, Option<String>, String, String, String, Option<String>, i64)> =
        sqlx::query_as(
            "SELECT name, transport, url, command, args_json, env_json, headers_json, api_key, enabled \
             FROM mcp_servers WHERE user_id = ? ORDER BY name ASC",
        )
        .bind(user_id)
        .fetch_all(db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(n, t, u, c, a, e, h, k, en)| row_to_server(n, t, u, c, a, e, h, k, en))
        .collect())
}

async fn list_mcp_servers(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let servers = fetch_mcp_servers(&state.db, &auth.user_id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    Ok(Json(json!({ "servers": servers })))
}

#[derive(Deserialize)]
struct UpsertMcpBody {
    name: String,
    #[serde(default = "default_transport")]
    transport: String,
    url: Option<String>,
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: serde_json::Map<String, Value>,
    #[serde(default)]
    headers: serde_json::Map<String, Value>,
    api_key: Option<String>,
    #[serde(default = "default_enabled")]
    enabled: bool,
}
fn default_transport() -> String { "http".to_string() }
fn default_enabled() -> bool { true }

async fn upsert_mcp_server(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<UpsertMcpBody>,
) -> ApiResult {
    upsert_mcp_inner(state, auth, None, body).await
}

async fn upsert_mcp_server_named(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    axum::extract::Path(path_name): axum::extract::Path<String>,
    Json(body): Json<UpsertMcpBody>,
) -> ApiResult {
    upsert_mcp_inner(state, auth, Some(path_name), body).await
}

async fn upsert_mcp_inner(
    state: Arc<AppState>,
    auth: AuthUser,
    rename_from: Option<String>,
    body: UpsertMcpBody,
) -> ApiResult {
    let target_name = body.name.trim().to_string();
    if target_name.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Server name cannot be empty"));
    }
    let transport = match body.transport.as_str() {
        "http" | "sse" | "stdio" => body.transport.clone(),
        other => return Err(err(StatusCode::BAD_REQUEST, &format!("Unsupported transport: {other}"))),
    };
    if transport == "stdio" {
        if body.command.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true) {
            return Err(err(StatusCode::BAD_REQUEST, "stdio server requires `command`"));
        }
    } else {
        if body.url.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true) {
            return Err(err(StatusCode::BAD_REQUEST, "http/sse server requires `url`"));
        }
    }

    let args_json = serde_json::to_string(&body.args).unwrap_or_else(|_| "[]".into());
    let env_json = serde_json::to_string(&body.env).unwrap_or_else(|_| "{}".into());
    let headers_json = serde_json::to_string(&body.headers).unwrap_or_else(|_| "{}".into());

    // If renaming, drop the old row first.
    if let Some(old) = rename_from.as_ref().filter(|n| n != &&target_name) {
        let _ = sqlx::query("DELETE FROM mcp_servers WHERE user_id = ? AND name = ?")
            .bind(&auth.user_id)
            .bind(old)
            .execute(&state.db)
            .await;
    }

    sqlx::query(
        "INSERT INTO mcp_servers (\
            user_id, name, transport, url, command, args_json, env_json, headers_json, api_key, enabled, updated_at\
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now')) \
         ON CONFLICT(user_id, name) DO UPDATE SET \
           transport    = excluded.transport, \
           url          = excluded.url, \
           command      = excluded.command, \
           args_json    = excluded.args_json, \
           env_json     = excluded.env_json, \
           headers_json = excluded.headers_json, \
           api_key      = excluded.api_key, \
           enabled      = excluded.enabled, \
           updated_at   = excluded.updated_at",
    )
    .bind(&auth.user_id)
    .bind(&target_name)
    .bind(&transport)
    .bind(&body.url)
    .bind(&body.command)
    .bind(&args_json)
    .bind(&env_json)
    .bind(&headers_json)
    .bind(&body.api_key)
    .bind(if body.enabled { 1 } else { 0 })
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    // Drop the chat handler's MCP discovery cache for this user — the
    // server config just changed, the cached `tools/list` snapshot is
    // probably stale (different URL, different auth, possibly disabled).
    state.invalidate_mcp_cache_for_user(&auth.user_id).await;

    Ok(Json(json!({ "ok": true, "name": target_name })))
}

// ---------------------------------------------------------------------------
// POST /user/mcp-servers/probe
// Body: { url, api_key?, headers? }
// Performs the MCP `initialize` handshake and `tools/list` discovery.
// Auto-detects transport: tries HTTP POST first, falls back to SSE on 405.
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct ProbeBody {
    url: String,
    api_key: Option<String>,
    #[serde(default)]
    headers: serde_json::Map<String, Value>,
}

async fn probe_mcp_server(
    _state: State<Arc<AppState>>,
    _auth: AuthUser,
    Json(body): Json<ProbeBody>,
) -> ApiResult {
    let url = body.url.trim().to_string();
    if url.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "url is required"));
    }

    let mut req_headers = reqwest::header::HeaderMap::new();
    req_headers.insert("Content-Type", "application/json".parse().unwrap());
    req_headers.insert("Accept", "application/json, text/event-stream".parse().unwrap());
    if let Some(key) = body.api_key.as_ref().filter(|k| !k.trim().is_empty()) {
        if let Ok(v) = format!("Bearer {key}").parse() {
            req_headers.insert("Authorization", v);
        }
    }
    for (k, v) in &body.headers {
        if let (Ok(name), Some(val_str), Ok(val_hv)) = (
            reqwest::header::HeaderName::from_bytes(k.as_bytes()),
            v.as_str(),
            v.as_str().unwrap_or("").parse::<reqwest::header::HeaderValue>(),
        ) {
            let _ = val_str;
            req_headers.insert(name, val_hv);
        }
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let init_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "MikeRust", "version": "0.1" }
        }
    });

    // Try a base URL plus a small set of well-known MCP suffixes (some
    // servers mount the JSON-RPC handler on `/mcp` or `/messages` rather
    // than the root). Returns the first URL that yields a 2xx initialize.
    async fn try_initialize_with_fallback(
        client: &reqwest::Client,
        base_url: &str,
        headers: &reqwest::header::HeaderMap,
        init_body: &Value,
    ) -> (String, Result<reqwest::Response, reqwest::Error>) {
        let trimmed = base_url.trim_end_matches('/').to_string();
        // Original URL first; only try fallbacks if root path is "/" or empty.
        let url_obj = url::Url::parse(base_url).ok();
        let path_is_root = url_obj
            .as_ref()
            .map(|u| u.path().is_empty() || u.path() == "/")
            .unwrap_or(false);

        let candidates: Vec<String> = if path_is_root {
            vec![
                base_url.to_string(),
                format!("{trimmed}/mcp"),
                format!("{trimmed}/messages"),
                format!("{trimmed}/api/mcp"),
            ]
        } else {
            vec![base_url.to_string()]
        };

        let mut last_resp: Option<(String, Result<reqwest::Response, reqwest::Error>)> = None;
        for candidate in candidates {
            let resp = client
                .post(&candidate)
                .headers(headers.clone())
                .json(init_body)
                .send()
                .await;
            match &resp {
                Ok(r) if r.status().is_success() => {
                    return (candidate, resp);
                }
                Ok(r) if r.status().as_u16() == 401 || r.status().as_u16() == 403 => {
                    // Auth required at this path — that's a strong signal it's the right path.
                    return (candidate, resp);
                }
                _ => {
                    last_resp = Some((candidate, resp));
                }
            }
        }
        last_resp.expect("at least one candidate")
    }

    let (matched_url, resp) =
        try_initialize_with_fallback(&client, &url, &req_headers, &init_body).await;
    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Err(err(StatusCode::BAD_GATEWAY, &format!("connection failed: {e}"))),
    };

    let status = resp.status();
    // Capture the session id the server returned — Streamable HTTP MCP
    // requires it on every subsequent request, otherwise the server replies
    // with "Unexpected message, expect initialize request".
    let session_id = resp
        .headers()
        .get("mcp-session-id")
        .or_else(|| resp.headers().get("Mcp-Session-Id"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let detected_transport: &str;
    let init_value: Value;

    let suggested_url = if matched_url != url {
        Some(matched_url.clone())
    } else {
        None
    };

    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(err(
            status,
            &format!(
                "MCP server requires authentication{}. Configure API key or headers and retry.",
                suggested_url
                    .as_ref()
                    .map(|u| format!(" (path discovered: {u})"))
                    .unwrap_or_default()
            ),
        ));
    }
    if status.as_u16() == 405 {
        return Ok(Json(json!({
            "ok": false,
            "transport_detected": "sse",
            "suggested_url": suggested_url,
            "hint": "POST returned 405; this URL appears to use the legacy HTTP+SSE transport (GET /sse + POST messages). Save it with transport=\"sse\" and the tools list will be loaded at runtime."
        })));
    }
    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        return Err(err(
            status,
            &format!(
                "MCP error {status} at {matched_url}: {}",
                body_text.chars().take(300).collect::<String>()
            ),
        ));
    }
    detected_transport = "http";

    init_value = read_jsonrpc_response(resp, 1, 8)
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, &e.to_string()))?;

    if let Some(rpc_err) = init_value.get("error") {
        return Err(err(
            StatusCode::BAD_GATEWAY,
            &format!("MCP initialize error: {rpc_err}"),
        ));
    }

    let server_info = init_value["result"]["serverInfo"].clone();
    let capabilities = init_value["result"]["capabilities"].clone();
    // The MCP `initialize` response can include a free-form `instructions`
    // field — the spec analogue of a `skill.md` body: a Markdown explanation
    // of what the server does and how to use it.
    let instructions = init_value["result"]["instructions"].as_str().map(|s| s.to_string());

    // Build the headers used for follow-up requests: same as initialize
    // plus `Mcp-Session-Id` so the server recognises the session.
    let mut session_headers = req_headers.clone();
    if let Some(sid) = session_headers.clone().get("Mcp-Session-Id") {
        // already set by user — leave it
        let _ = sid;
    } else if let Some(sid) = &session_id {
        if let Ok(v) = sid.parse() {
            session_headers.insert("Mcp-Session-Id", v);
        }
    }

    // 2) Send the `notifications/initialized` handshake (no id = notification,
    // server returns 202 Accepted with empty body). Required by the spec
    // before any other request on the same session.
    let _ = client
        .post(&matched_url)
        .headers(session_headers.clone())
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }))
        .send()
        .await;

    // Helper that calls a JSON-RPC method against the URL we just initialized
    // and returns the array under `result.{key}`, ignoring failures.
    async fn list_method(
        client: &reqwest::Client,
        url: &str,
        headers: &reqwest::header::HeaderMap,
        method: &str,
        result_key: &str,
        id: u64,
    ) -> Vec<Value> {
        let resp = client
            .post(url)
            .headers(headers.clone())
            .json(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": {}
            }))
            .send()
            .await;
        let Ok(r) = resp else { return Vec::new() };
        let Ok(v) = read_jsonrpc_response(r, id, 8).await else { return Vec::new() };
        v["result"][result_key].as_array().cloned().unwrap_or_default()
    }

    // 3) Discover tools, prompts (skill-like templates), resources — all on
    // the same session.
    let raw_tools = list_method(&client, &matched_url, &session_headers, "tools/list", "tools", 2).await;
    let raw_prompts = list_method(&client, &matched_url, &session_headers, "prompts/list", "prompts", 3).await;
    let raw_resources = list_method(&client, &matched_url, &session_headers, "resources/list", "resources", 4).await;

    let tools: Vec<Value> = raw_tools
        .into_iter()
        .map(|t| json!({
            "name": t["name"].as_str().unwrap_or(""),
            "description": t["description"].as_str().unwrap_or(""),
        }))
        .collect();

    let prompts: Vec<Value> = raw_prompts
        .into_iter()
        .map(|p| {
            // Each prompt may declare `arguments: [{name, description, required}]`
            let args: Vec<Value> = p["arguments"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|a| json!({
                    "name": a["name"].as_str().unwrap_or(""),
                    "description": a["description"].as_str().unwrap_or(""),
                    "required": a["required"].as_bool().unwrap_or(false),
                }))
                .collect();
            json!({
                "name": p["name"].as_str().unwrap_or(""),
                "description": p["description"].as_str().unwrap_or(""),
                "arguments": args,
            })
        })
        .collect();

    let resources: Vec<Value> = raw_resources
        .into_iter()
        .map(|r| json!({
            "uri": r["uri"].as_str().unwrap_or(""),
            "name": r["name"].as_str().unwrap_or(""),
            "description": r["description"].as_str().unwrap_or(""),
            "mimeType": r["mimeType"].as_str().unwrap_or(""),
        }))
        .collect();

    Ok(Json(json!({
        "ok": true,
        "transport_detected": detected_transport,
        "suggested_url": suggested_url,
        "server_info": server_info,
        "capabilities": capabilities,
        "instructions": instructions,
        "tools": tools,
        "tool_count": tools.len(),
        "prompts": prompts,
        "prompt_count": prompts.len(),
        "resources": resources,
        "resource_count": resources.len(),
    })))
}

/// Read a JSON-RPC response from a Streamable-HTTP MCP endpoint.
///
/// MCP servers using the Streamable HTTP transport often respond with an SSE
/// stream that may begin with keep-alive frames before the actual JSON-RPC
/// payload, and may stay open afterwards to push server→client notifications.
/// This means `Response::text()` would block until either the connection is
/// closed or the global request timeout elapses — too slow for an interactive
/// "Test & detect" probe.
///
/// Instead we incrementally read chunks, look for `data: {…}` lines, and
/// return as soon as we find a JSON-RPC envelope whose `id` matches the
/// expected request id. Pure-JSON (non-SSE) responses are also handled.
pub async fn read_jsonrpc_response(
    resp: reqwest::Response,
    expect_id: u64,
    max_secs: u64,
) -> Result<Value, anyhow::Error> {
    use futures_util::StreamExt;
    use tokio::time::{timeout, Duration, Instant};

    let deadline = Instant::now() + Duration::from_secs(max_secs);
    let started_at = Instant::now();
    // Periodic "still waiting" log when an SSE stream is silent for a
    // while — useful for tools like Edge's pseudonymise-with-approval
    // where the server holds the connection open while a human clicks
    // "Conferma" in their UI. Without this log, the dispatch appeared
    // to hang silently for minutes; now it's clear we're alive and
    // waiting on the server.
    let mut next_heartbeat = started_at + Duration::from_secs(15);
    let mut chunk_count = 0usize;
    let mut bytes_received = 0usize;

    let mut buf = String::new();
    let mut stream = resp.bytes_stream();

    loop {
        let now = Instant::now();
        if now >= deadline { break; }
        let remaining = deadline.duration_since(now);

        // Wake every 15 s (or remaining, whichever is shorter) so we
        // can emit a heartbeat log even if the stream is silent.
        let wait = std::cmp::min(
            remaining,
            next_heartbeat.saturating_duration_since(now).max(Duration::from_millis(1)),
        );

        match timeout(wait, stream.next()).await {
            Err(_) => {
                // Wait timed out — but is it the heartbeat or the
                // overall deadline? If we still have time left, log
                // a heartbeat and keep going.
                if Instant::now() < deadline {
                    let elapsed_secs = started_at.elapsed().as_secs();
                    tracing::info!(
                        "[mcp/sse] still waiting on response… ({}s elapsed, {} chunks, {} bytes received so far, deadline at {}s)",
                        elapsed_secs, chunk_count, bytes_received, max_secs
                    );
                    next_heartbeat = Instant::now() + Duration::from_secs(15);
                    continue;
                }
                break;                                  // real overall timeout
            }
            Ok(None) => break,                          // stream ended
            Ok(Some(Err(e))) => return Err(anyhow::anyhow!("stream error: {e}")),
            Ok(Some(Ok(bytes))) => {
                chunk_count += 1;
                bytes_received += bytes.len();
                tracing::debug!(
                    "[mcp/sse] chunk #{}: +{} bytes (total {} bytes, {}s elapsed)",
                    chunk_count, bytes.len(), bytes_received, started_at.elapsed().as_secs()
                );
                buf.push_str(&String::from_utf8_lossy(&bytes));

                // Pure-JSON response (e.g. when server doesn't use SSE).
                let trimmed = buf.trim_start();
                if trimmed.starts_with('{') {
                    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
                        return Ok(v);
                    }
                }

                // SSE: scan all complete `data:` lines we have so far.
                for line in buf.lines() {
                    let l = line.trim();
                    if let Some(rest) = l.strip_prefix("data:") {
                        let data = rest.trim();
                        if data.is_empty() || data == "[DONE]" { continue; }
                        if let Ok(v) = serde_json::from_str::<Value>(data) {
                            // Match by id when one is present; otherwise
                            // accept the first parseable payload (some
                            // servers omit the id on errors).
                            let id_match = v
                                .get("id")
                                .and_then(|i| i.as_u64())
                                .map(|i| i == expect_id)
                                .unwrap_or(true);
                            if id_match {
                                return Ok(v);
                            }
                        }
                    }
                }
            }
        }
    }

    // Final attempt on whatever we've accumulated.
    parse_jsonrpc_payload(&buf)
}

/// Parse a Streamable-HTTP MCP response which may be either:
///   - a single JSON object: `{"jsonrpc":"2.0","id":...,"result":...}`
///   - an SSE stream with `data: {...}` lines
fn parse_jsonrpc_payload(raw: &str) -> Result<Value, anyhow::Error> {
    let trimmed = raw.trim_start();
    if trimmed.starts_with('{') {
        return serde_json::from_str(trimmed)
            .map_err(|e| anyhow::anyhow!("invalid JSON: {e}"));
    }
    // SSE: pick the first `data:` line that parses as JSON.
    for line in raw.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("data:") {
            let data = rest.trim();
            if data.is_empty() || data == "[DONE]" { continue; }
            if let Ok(v) = serde_json::from_str::<Value>(data) {
                return Ok(v);
            }
        }
    }
    Err(anyhow::anyhow!("no parseable JSON-RPC payload in response"))
}

async fn delete_mcp_server(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> ApiResult {
    sqlx::query("DELETE FROM mcp_servers WHERE user_id = ? AND name = ?")
        .bind(&auth.user_id)
        .bind(&name)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    state.invalidate_mcp_cache_for_user(&auth.user_id).await;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// DELETE /user/account  — irreversible, deletes all user data via CASCADE
// ---------------------------------------------------------------------------
async fn delete_account(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    sqlx::query("DELETE FROM user_profiles WHERE id = ?")
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({ "ok": true })))
}
