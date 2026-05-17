use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response, Sse},
    response::sse::Event,
    routing::get,
    Json, Router,
};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    auth::middleware::AuthUser,
    llm::{
        self, builtin_tools, LocalConfig, Message, Role, StreamEvent, StreamParams, ToolCall,
        ToolFunction, ToolSchema,
    },
    routes::user::{fetch_llm_settings, fetch_mcp_servers, read_jsonrpc_response, McpServerOut},
    storage::make_storage,
    AppState,
};
use std::collections::HashMap;

/// Build the OpenAI-compatible `LocalConfig` for a model id carrying a
/// `local:`, `openai:` or `mistral:` prefix. Returns `None` for native
/// cloud models (Claude / Gemini) or when the selected provider has no
/// endpoint / key configured.
///
/// Mistral and OpenAI have fixed public endpoints; `local:` reads the
/// user's BYO base URL. The model name is the per-provider stored model
/// field, falling back to the id with its prefix stripped.
pub fn build_local_config(
    model: &str,
    settings: Option<&crate::routes::user::LlmSettings>,
) -> Option<LocalConfig> {
    let s = settings?;
    let (base, key, stored_model) = if model.starts_with("openai:") {
        (
            s.openai_api_key
                .as_ref()
                .map(|_| "https://api.openai.com/v1".to_string())
                .unwrap_or_default(),
            s.openai_api_key.clone(),
            s.openai_model.clone(),
        )
    } else if model.starts_with("mistral:") {
        (
            s.mistral_api_key
                .as_ref()
                .map(|_| "https://api.mistral.ai/v1".to_string())
                .unwrap_or_default(),
            s.mistral_api_key.clone(),
            s.mistral_model.clone(),
        )
    } else if model.starts_with("local:") {
        (
            s.local_base_url.clone().unwrap_or_default(),
            s.local_api_key.clone(),
            s.local_model.clone(),
        )
    } else {
        return None;
    };
    if base.trim().is_empty() {
        return None;
    }
    Some(LocalConfig {
        base_url: base,
        api_key: key.filter(|k| !k.trim().is_empty()),
        model: stored_model
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| llm::strip_model_prefix(model).to_string()),
    })
}

// ---------------------------------------------------------------------------
// MCP capability discovery — surfaces configured servers to the chat model
// ---------------------------------------------------------------------------

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct McpDiscovered {
    config_name: String,
    server_name: Option<String>,
    server_version: Option<String>,
    instructions: Option<String>,
    tools: Vec<(String, String)>,    // (name, description) — for system prompt rendering
    /// Full tool schemas (incl. inputSchema) ready to be passed to the LLM.
    tool_schemas: Vec<ToolSchema>,
    prompts: Vec<(String, String)>,  // (name, description)
    /// Coordinates needed to dispatch a `tools/call` later.
    url: Option<String>,
    api_key: Option<String>,
    extra_headers: serde_json::Map<String, serde_json::Value>,
    session_id: Option<String>,
}

async fn discover_one_mcp(server: McpServerOut) -> Option<McpDiscovered> {
    if server.transport == "stdio" {
        return Some(McpDiscovered {
            config_name: server.name,
            server_name: None,
            server_version: None,
            instructions: Some(format!(
                "(Configured as stdio: command={} args={:?}; runtime spawning is not yet wired in this build.)",
                server.command.as_deref().unwrap_or(""),
                server.args
            )),
            tools: vec![],
            tool_schemas: vec![],
            prompts: vec![],
            url: None,
            api_key: None,
            extra_headers: serde_json::Map::new(),
            session_id: None,
        });
    }
    let url = server.url.as_ref()?.clone();
    if url.trim().is_empty() {
        return None;
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().ok()?);
    headers.insert(
        "Accept",
        "application/json, text/event-stream".parse().ok()?,
    );
    if let Some(k) = server.api_key.as_ref().filter(|k| !k.trim().is_empty()) {
        if let Ok(v) = format!("Bearer {k}").parse() {
            headers.insert("Authorization", v);
        }
    }
    for (k, v) in &server.headers {
        if let Some(s) = v.as_str() {
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                s.parse::<reqwest::header::HeaderValue>(),
            ) {
                headers.insert(name, value);
            }
        }
    }

    // 1) initialize → capture session id
    let init_resp = client
        .post(&url)
        .headers(headers.clone())
        .json(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "MikeRust", "version": "0.1" }
            }
        }))
        .send()
        .await
        .ok()?;

    if !init_resp.status().is_success() {
        tracing::warn!("[mcp/discover] {}: initialize {}", server.name, init_resp.status());
        return None;
    }

    let session_id: Option<String> = init_resp
        .headers()
        .get("mcp-session-id")
        .or_else(|| init_resp.headers().get("Mcp-Session-Id"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let init_value = read_jsonrpc_response(init_resp, 1, 10).await.ok()?;
    let server_name = init_value["result"]["serverInfo"]["name"]
        .as_str()
        .map(|s| s.to_string());
    let server_version = init_value["result"]["serverInfo"]["version"]
        .as_str()
        .map(|s| s.to_string());
    let instructions = init_value["result"]["instructions"]
        .as_str()
        .map(|s| s.to_string());

    // 2) Build session-aware headers
    let mut session_headers = headers.clone();
    if let Some(sid) = &session_id {
        if let Ok(v) = sid.parse() {
            session_headers.insert("Mcp-Session-Id", v);
        }
    }

    // 3) notifications/initialized handshake completion (best-effort)
    let _ = client
        .post(&url)
        .headers(session_headers.clone())
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }))
        .send()
        .await;

    // 4) tools/list — keep the full inputSchema for tool-use, plus a
    // (name, description) summary for the system prompt rendering.
    let raw_tools: Vec<Value> = match client
        .post(&url)
        .headers(session_headers.clone())
        .json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}))
        .send()
        .await
    {
        Ok(r) => read_jsonrpc_response(r, 2, 8)
            .await
            .ok()
            .and_then(|v| v["result"]["tools"].as_array().cloned())
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let tools: Vec<(String, String)> = raw_tools
        .iter()
        .map(|t| (
            t["name"].as_str().unwrap_or("").to_string(),
            t["description"].as_str().unwrap_or("").to_string(),
        ))
        .collect();
    let tool_schemas: Vec<ToolSchema> = raw_tools
        .iter()
        .map(|t| ToolSchema {
            kind: "function".to_string(),
            function: ToolFunction {
                name: t["name"].as_str().unwrap_or("").to_string(),
                description: t["description"].as_str().unwrap_or("").to_string(),
                parameters: t["inputSchema"].clone(),
            },
        })
        .collect();

    // 5) prompts/list
    let prompts = match client
        .post(&url)
        .headers(session_headers.clone())
        .json(&json!({"jsonrpc":"2.0","id":3,"method":"prompts/list","params":{}}))
        .send()
        .await
    {
        Ok(r) => read_jsonrpc_response(r, 3, 8)
            .await
            .ok()
            .and_then(|v| v["result"]["prompts"].as_array().cloned())
            .map(|arr| {
                arr.into_iter()
                    .map(|p| {
                        (
                            p["name"].as_str().unwrap_or("").to_string(),
                            p["description"].as_str().unwrap_or("").to_string(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    Some(McpDiscovered {
        config_name: server.name,
        server_name,
        server_version,
        instructions,
        tools,
        tool_schemas,
        prompts,
        url: Some(url.clone()),
        api_key: server.api_key,
        extra_headers: server.headers,
        session_id,
    })
}

/// Dispatch a tool call to the right MCP server using its session id.
/// Returns a string suitable for `tool` role message content.
///
/// Verbose phase-by-phase logging: every line carries the elapsed-ms
/// since dispatch start so the user can see *exactly* where time
/// goes — useful when an MCP tool requires interactive approval on
/// the server side and the call appears to "hang".
async fn dispatch_mcp_tool(
    servers: &[McpDiscovered],
    tool_name: &str,
    arguments: &Value,
) -> String {
    let dispatch_start = std::time::Instant::now();
    macro_rules! mtrace {
        ($fmt:literal $(, $arg:expr)* $(,)?) => {
            tracing::info!(
                concat!("[mcp/dispatch] tool={} +{}ms — ", $fmt),
                tool_name,
                dispatch_start.elapsed().as_millis()
                $(, $arg)*
            )
        };
    }

    let Some(srv) = servers.iter().find(|s| {
        s.tool_schemas.iter().any(|t| t.function.name == tool_name)
    }) else {
        tracing::warn!(
            "[mcp/dispatch] tool={} +0ms — no MCP server provides this tool (known servers: {:?})",
            tool_name,
            servers.iter().map(|s| s.config_name.as_str()).collect::<Vec<_>>()
        );
        return json!({"error": format!("No MCP server provides tool '{tool_name}'")}).to_string();
    };
    let Some(url) = &srv.url else {
        return json!({"error": "tool's MCP server has no URL"}).to_string();
    };

    let timeout_secs = crate::db::mcp_call_timeout_secs();
    mtrace!(
        "routing to server={} url={} session_id={} timeout={}s",
        srv.config_name,
        url,
        srv.session_id.as_deref().unwrap_or("(none)"),
        timeout_secs
    );

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
    {
        Ok(c) => c,
        Err(e) => return json!({"error": e.to_string()}).to_string(),
    };

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::CONTENT_TYPE, "application/json".parse().unwrap());
    headers.insert(reqwest::header::ACCEPT, "application/json, text/event-stream".parse().unwrap());
    if let Some(k) = srv.api_key.as_ref().filter(|k| !k.trim().is_empty()) {
        if let Ok(v) = format!("Bearer {k}").parse() {
            headers.insert(reqwest::header::AUTHORIZATION, v);
        }
    }
    for (k, v) in &srv.extra_headers {
        if let Some(s) = v.as_str() {
            if let (Ok(name), Ok(val)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                s.parse::<reqwest::header::HeaderValue>(),
            ) {
                headers.insert(name, val);
            }
        }
    }
    if let Some(sid) = &srv.session_id {
        if let Ok(v) = sid.parse() {
            headers.insert("Mcp-Session-Id", v);
        }
    }

    let body = json!({
        "jsonrpc": "2.0",
        "id": 100,
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": arguments,
        }
    });
    let body_bytes = body.to_string().len();
    mtrace!(
        "POST {} (body {} bytes, {} args, headers: {:?})",
        url,
        body_bytes,
        arguments
            .as_object()
            .map(|m| m.len())
            .unwrap_or(0),
        headers
            .keys()
            .map(|k| k.as_str())
            .filter(|k| !k.eq_ignore_ascii_case("authorization")) // never log Bearer tokens
            .collect::<Vec<_>>()
    );

    let resp = match client.post(url).headers(headers).json(&body).send().await {
        Ok(r) => {
            mtrace!(
                "POST returned: status={} content-type={:?}",
                r.status(),
                r.headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|h| h.to_str().ok())
            );
            r
        }
        Err(e) => {
            mtrace!("POST failed: {}", e);
            return json!({"error": format!("network: {e}")}).to_string();
        }
    };

    mtrace!("reading response body / SSE stream (timeout {}s)", timeout_secs);
    // Reader timeout matches the wire-level timeout — otherwise the
    // SSE stream reader could give up earlier than the HTTP client
    // and we'd lose a long but legitimate tool response (e.g. Edge
    // pseudonymising a multi-MB document, or a tool that requires
    // interactive human approval before releasing the response).
    let val = match read_jsonrpc_response(resp, 100, timeout_secs).await {
        Ok(v) => {
            mtrace!("body decoded as JSON-RPC, ~{} chars", v.to_string().len());
            v
        }
        Err(e) => {
            mtrace!("body read failed: {}", e);
            return json!({"error": format!("read: {e}")}).to_string();
        }
    };

    if let Some(rpc_err) = val.get("error") {
        mtrace!("JSON-RPC error in response: {}", rpc_err);
        return json!({"error": rpc_err}).to_string();
    }

    // MCP tools/call result is `{content: [{type:"text", text:"…"}, …], isError?:bool}`
    let content = &val["result"]["content"];
    if let Some(arr) = content.as_array() {
        let joined: Vec<String> = arr
            .iter()
            .filter_map(|c| c["text"].as_str().map(|s| s.to_string()))
            .collect();
        if !joined.is_empty() {
            mtrace!(
                "DONE — returning {} text chunk(s), {} total chars",
                joined.len(),
                joined.iter().map(|s| s.len()).sum::<usize>()
            );
            return joined.join("\n");
        }
    }
    let fallback = val["result"].to_string();
    mtrace!(
        "DONE — content array empty, returning result-as-string ({} chars)",
        fallback.len()
    );
    fallback
}

/// Dispatch an MCP tool, then transparently auto-chain a follow-up
/// `get_*` call when the server returns the async-pending pattern.
///
/// Pattern detection (Edge's pseudonymise flow is the canonical
/// example):
///
///   1. Model calls `request_pseudonymized_documents(ids=[…])`
///   2. Edge returns `{session_id, status:"pending", doc_count:N}`
///      — the actual documents aren't ready yet because Edge wants
///      a human to click "Conferma" in its UI first.
///   3. Without auto-chain, the model receives the pending envelope
///      as the tool result, almost always declares the job done,
///      and never fetches the real documents.
///
/// Auto-chain bridges step 3 by:
///
///   * recognising the `{session_id, status:"pending"}` shape;
///   * deriving the companion tool name (`request_X` → `get_X`);
///   * checking the same MCP server actually exposes that companion;
///   * dispatching it with `{session_id, wait_for_approval: true,
///     wait_timeout_seconds: <our timeout>}` so the long-poll
///     completes server-side;
///   * substituting the get_* result for the original.
///
/// Generic enough to fit any MCP server that uses the same naming
/// convention. Tools that don't follow the pattern (or that already
/// return their full result inline) are unaffected — the function
/// degrades to a passthrough.
async fn dispatch_mcp_tool_with_async_chain(
    servers: &[McpDiscovered],
    tool_name: &str,
    arguments: &Value,
) -> String {
    let primary = dispatch_mcp_tool(servers, tool_name, arguments).await;

    // Only the "request_*" tools can ever trigger a chain — short-
    // circuit otherwise so we don't pay the JSON parse for every
    // tool result (most are already final).
    let companion_name = match tool_name.strip_prefix("request_") {
        Some(rest) => format!("get_{rest}"),
        None => return primary,
    };

    // Try to parse the response as JSON. If it isn't JSON, or the
    // shape doesn't match the pending pattern, just return as-is.
    let parsed: Value = match serde_json::from_str(&primary) {
        Ok(v) => v,
        Err(_) => return primary,
    };
    let session_id = parsed
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let status = parsed
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let is_pending = matches!(
        status,
        "pending" | "queued" | "in_review" | "awaiting_approval"
    );
    let (Some(session_id), true) = (session_id, is_pending) else {
        return primary;
    };

    // The companion tool must exist on the same server that handled
    // the request — calling it on a different server would land in
    // the wrong session-id namespace.
    let server_has_companion = servers.iter().any(|s| {
        s.tool_schemas
            .iter()
            .any(|t| t.function.name == tool_name)
            && s.tool_schemas
                .iter()
                .any(|t| t.function.name == companion_name)
    });
    if !server_has_companion {
        tracing::info!(
            "[mcp/dispatch] auto-chain skipped: {} returned pending session_id={} \
             but companion {} not found on the same server — passing the pending \
             envelope to the model so it can decide what to do",
            tool_name,
            session_id,
            companion_name
        );
        return primary;
    }

    let timeout_secs = crate::db::mcp_call_timeout_secs();
    let chain_args = json!({
        "session_id": session_id,
        // Edge's flag — long-poll until the human clicks Conferma.
        // Other MCP servers using the same naming pattern may
        // ignore this kwarg, which is fine.
        "wait_for_approval": true,
        "wait_timeout_seconds": timeout_secs,
    });
    tracing::info!(
        "[mcp/dispatch] auto-chain {} → {} with session_id={} \
         (wait_for_approval=true, timeout={}s)",
        tool_name,
        companion_name,
        session_id,
        timeout_secs
    );

    let chained = dispatch_mcp_tool(servers, &companion_name, &chain_args).await;
    tracing::info!(
        "[mcp/dispatch] auto-chain done: {} → {} returned {} chars",
        tool_name,
        companion_name,
        chained.len()
    );
    chained
}

async fn discover_mcp_for_user(state: &AppState, user_id: &str) -> Vec<McpDiscovered> {
    let ttl = crate::db::mcp_cache_ttl();

    // Cache hit: deserialise and return without touching the network.
    {
        let cache = state.mcp_discovery_cache.read().await;
        if let Some(entry) = cache.get(user_id) {
            if entry.is_fresh(ttl) {
                if let Ok(parsed) =
                    serde_json::from_str::<Vec<McpDiscovered>>(&entry.payload_json)
                {
                    tracing::info!(
                        "[mcp/discover] cache hit for user={}: {} servers ({} sec old, ttl {}s)",
                        user_id,
                        parsed.len(),
                        entry.stored_at.elapsed().as_secs(),
                        ttl.as_secs(),
                    );
                    return parsed;
                }
                tracing::warn!(
                    "[mcp/discover] cache entry deserialise failed for user={}, re-discovering",
                    user_id
                );
            }
        }
    }

    // Cache miss / stale: do the full handshake.
    let servers = match fetch_mcp_servers(&state.db, user_id).await {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let enabled: Vec<McpServerOut> =
        servers.into_iter().filter(|s| s.enabled).collect();
    if enabled.is_empty() {
        // Drop any prior cached entry — the user just disabled all servers.
        state.mcp_discovery_cache.write().await.remove(user_id);
        return vec![];
    }
    use futures_util::future::join_all;
    let futs = enabled.into_iter().map(discover_one_mcp);
    let discovered: Vec<McpDiscovered> =
        join_all(futs).await.into_iter().flatten().collect();
    tracing::info!(
        "[mcp/discover] cache miss for user={}: discovered {} servers via fresh handshake",
        user_id,
        discovered.len()
    );

    // Store in cache for next request.
    if let Ok(payload_json) = serde_json::to_string(&discovered) {
        let mut g = state.mcp_discovery_cache.write().await;
        g.insert(
            user_id.to_string(),
            crate::db::McpDiscoveryCacheEntry {
                stored_at: std::time::Instant::now(),
                payload_json,
            },
        );
    }

    discovered
}

fn build_mcp_system_prompt(servers: &[McpDiscovered]) -> String {
    if servers.is_empty() {
        return String::new();
    }
    // Minimal MCP awareness: the actual tool definitions are passed to the
    // model via the standard `tools` parameter — we don't need to repeat
    // them in the system prompt. A long verbose listing biases the model
    // into proposing tools for every greeting. Keep the prompt small and
    // assertive about NOT calling tools unless explicitly asked.
    let mut s = String::from(
        "You are a helpful general-purpose chat assistant. Your default behavior \
         is to answer questions directly from the conversation context (including \
         any attached documents). \n\n\
         You have access to optional external tools provided by connected MCP \
         servers (declared via the `tools` parameter). Invoke a tool **only when \
         the user explicitly requests it** (e.g. \"use tool X\", \"call X\", \
         \"run X on this\"). For greetings, generic questions (\"test\", \"hi\", \
         \"explain\", \"summarize\", \"analyze this\"), reply normally — \
         **do not list available tools or propose them proactively**.\n\n\
         Connected MCP servers (don't enumerate them unless asked):\n",
    );
    for srv in servers {
        let display = srv
            .server_name
            .clone()
            .unwrap_or_else(|| srv.config_name.clone());
        let version = srv
            .server_version
            .as_ref()
            .map(|v| format!(" v{v}"))
            .unwrap_or_default();
        // One-line summary: name, version, first sentence of instructions only.
        let summary = srv
            .instructions
            .as_deref()
            .map(|inst| {
                inst.split(|c: char| c == '.' || c == '\n')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .chars()
                    .take(160)
                    .collect::<String>()
            })
            .unwrap_or_default();
        if summary.is_empty() {
            s.push_str(&format!("- `{display}`{version}\n"));
        } else {
            s.push_str(&format!("- `{display}`{version} — {summary}\n"));
        }
    }
    s.push('\n');
    s
}

/// Extract the JSON inside a `<CITATIONS>...</CITATIONS>` block at the end
/// of the assistant response. Tolerant of surrounding whitespace and code
/// fences. Returns the parsed `Value` (an array) or `None`.
pub(crate) fn extract_citations_block(text: &str) -> Option<Value> {
    let lower = text.to_lowercase();
    let open = lower.rfind("<citations>")?;
    let after_open = open + "<citations>".len();
    // Find the matching close tag *after* the open.
    let close_rel = lower[after_open..].find("</citations>")?;
    let inner = text[after_open..after_open + close_rel].trim();
    // Strip optional Markdown fences like ```json … ```
    let inner = inner.trim_start_matches("```json").trim_start_matches("```").trim();
    let inner = inner.trim_end_matches("```").trim();
    serde_json::from_str::<Value>(inner).ok()
}

/// Result of processing one attached document.
pub struct DocPayload {
    pub filename: String,
    /// Extracted plain text (None when only images are usable, e.g. scanned PDF).
    pub text: Option<String>,
    /// `data:image/png;base64,...` URLs for vision-capable models.
    pub images: Vec<String>,
}

const MAX_PDF_IMAGE_PAGES: usize = 8;
const PDF_RENDER_DPI: f32 = 200.0;

#[cfg(feature = "pdf")]
fn pages_to_data_urls(pngs: Vec<Vec<u8>>) -> Vec<String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    pngs.into_iter()
        .map(|bytes| format!("data:image/png;base64,{}", STANDARD.encode(&bytes)))
        .collect()
}

/// Read attached documents from storage and extract their text and/or images.
/// `vision_ok` lets scanned PDFs fall back to rendered page images.
async fn load_attached_docs(
    state: &AppState,
    user_id: &str,
    document_ids: &[String],
    vision_ok: bool,
) -> Vec<DocPayload> {
    let mut out = Vec::new();
    for doc_id in document_ids {
        let row: Option<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT filename, file_type, storage_path, extracted_text_path \
             FROM documents WHERE id = ? AND user_id = ?",
        )
        .bind(doc_id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();

        let Some((filename, file_type, Some(storage_path), extracted_text_path)) = row
        else {
            continue;
        };

        let storage = match make_storage() {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Cache fast path: if the upload pipeline already extracted
        // plain text to data/storage/cache/<hash>.txt, prefer it.
        //  - Text-bearing formats (docx, rtf, xlsx, txt/md/csv): use
        //    the cache directly and skip the per-format dispatch and
        //    even the binary read.
        //  - PDFs: use the cache if non-empty (native PDFs); fall
        //    through if empty (scanned PDFs needing vision rendering).
        //  - Images: never use the cache — they need the binary
        //    base64-encoded for the model.
        let is_image_format = matches!(
            file_type.as_str(),
            "png" | "jpeg" | "jpg" | "tiff" | "tif"
        );
        let mut cached_text: Option<String> = None;
        if !is_image_format {
            if let Some(txt_key) = extracted_text_path.as_ref() {
                if let Ok(txt_bytes) = storage.get(txt_key).await {
                    let text = String::from_utf8_lossy(&txt_bytes).into_owned();
                    if !text.is_empty() {
                        cached_text = Some(text);
                    }
                }
            }
        }
        if let Some(text) = cached_text.take() {
            if file_type != "pdf" || !text.trim().is_empty() {
                tracing::info!(
                    "[chat] using cached text for {filename}: {} chars",
                    text.len()
                );
                out.push(DocPayload {
                    filename: filename.clone(),
                    text: Some(text),
                    images: Vec::new(),
                });
                continue;
            }
        }

        let bytes = match storage.get(&storage_path).await {
            Ok(b) => b,
            Err(_) => continue,
        };

        let mut payload = DocPayload {
            filename: filename.clone(),
            text: None,
            images: Vec::new(),
        };

        match file_type.as_str() {
            "docx" => {
                payload.text = crate::pdf::extract_docx_text(&bytes).ok();
            }
            "rtf" => {
                let raw = String::from_utf8_lossy(&bytes);
                payload.text = rtf_parser::RtfDocument::try_from(raw.as_ref())
                    .map(|d| d.get_text())
                    .ok();
            }
            "xlsx" | "xls" | "xlsb" | "ods" => {
                payload.text = crate::pdf::extract_xlsx_text(&bytes).ok();
            }
            "txt" | "md" | "csv" => {
                payload.text = Some(String::from_utf8_lossy(&bytes).to_string());
            }
            "png" => {
                if vision_ok {
                    use base64::{engine::general_purpose::STANDARD, Engine as _};
                    payload.images.push(format!(
                        "data:image/png;base64,{}",
                        STANDARD.encode(&bytes)
                    ));
                } else {
                    tracing::warn!(
                        "[chat] {filename}: PNG attached but selected model is not vision-capable"
                    );
                }
            }
            "jpeg" | "jpg" => {
                if vision_ok {
                    use base64::{engine::general_purpose::STANDARD, Engine as _};
                    payload.images.push(format!(
                        "data:image/jpeg;base64,{}",
                        STANDARD.encode(&bytes)
                    ));
                } else {
                    tracing::warn!(
                        "[chat] {filename}: JPEG attached but selected model is not vision-capable"
                    );
                }
            }
            "tiff" | "tif" => {
                if vision_ok {
                    match crate::pdf::convert_tiff_to_jpegs(&bytes) {
                        Ok(jpegs) => {
                            tracing::info!(
                                "[chat] {filename}: TIFF converted to {} JPEG frame(s)",
                                jpegs.len()
                            );
                            use base64::{engine::general_purpose::STANDARD, Engine as _};
                            for j in jpegs {
                                payload.images.push(format!(
                                    "data:image/jpeg;base64,{}",
                                    STANDARD.encode(&j)
                                ));
                            }
                        }
                        Err(e) => {
                            tracing::warn!("[chat] {filename}: TIFF conversion failed: {e}");
                        }
                    }
                } else {
                    tracing::warn!(
                        "[chat] {filename}: TIFF attached but selected model is not vision-capable"
                    );
                }
            }
            "pdf" => {
                #[cfg(feature = "pdf")]
                {
                    let tmp = std::env::temp_dir().join(format!("mike-{}.pdf", doc_id));
                    if std::fs::write(&tmp, &bytes).is_ok() {
                        let pages = crate::pdf::extract_text(&tmp).ok();
                        if let Some(pages) = pages {
                            let scanned = crate::pdf::is_scanned_pdf(&pages);
                            let mut full_text = String::new();
                            for p in &pages {
                                full_text.push_str(&format!("[Page {}]\n{}\n", p.page, p.text));
                            }
                            if !scanned {
                                payload.text = Some(full_text);
                            } else if vision_ok {
                                tracing::info!(
                                    "[chat] {filename}: scanned PDF detected, rendering up to {MAX_PDF_IMAGE_PAGES} pages at {PDF_RENDER_DPI} DPI"
                                );
                                match crate::pdf::render_pdf_pages(
                                    &tmp,
                                    PDF_RENDER_DPI,
                                    MAX_PDF_IMAGE_PAGES,
                                ) {
                                    Ok(pngs) => {
                                        payload.images = pages_to_data_urls(pngs);
                                    }
                                    Err(e) => {
                                        tracing::warn!("[chat] render PDF pages failed: {e}");
                                    }
                                }
                            } else {
                                tracing::warn!(
                                    "[chat] {filename}: scanned PDF but the selected model is not vision-capable; sending what little text was extracted"
                                );
                                payload.text = Some(full_text);
                            }
                        }
                        let _ = std::fs::remove_file(&tmp);
                    }
                }
                #[cfg(not(feature = "pdf"))]
                {
                    tracing::warn!("[chat] PDF document {doc_id} skipped: pdf feature not enabled");
                }
            }
            _ => {
                tracing::warn!("[chat] unsupported file_type={file_type} for {filename}");
            }
        }

        let chars = payload.text.as_deref().map(|t| t.len()).unwrap_or(0);
        tracing::info!(
            "[chat] loaded doc {filename}: text={} chars, images={}",
            chars,
            payload.images.len()
        );
        out.push(payload);
    }
    out
}

/// Mike's original legal-assistant system prompt, adapted from upstream
/// (willchen96/mike, `backend/src/lib/chatTools.ts` SYSTEM_PROMPT).
const MRUST_SYSTEM_PROMPT: &str = r#"You are Mike, an AI legal assistant that helps lawyers and legal professionals analyze documents, answer legal questions, and draft legal documents.

DOCUMENT CITATION INSTRUCTIONS:
When you reference specific content from a document, place a numbered marker [1], [2], etc. inline in your prose at the point of reference.

After your complete response, append a <CITATIONS> block containing a JSON array with one entry per marker:

<CITATIONS>
[
  {"ref": 1, "doc_id": "doc-0", "page": 3, "quote": "exact verbatim text from the document"},
  {"ref": 2, "doc_id": "doc-1", "page": "41-42", "quote": "Section 4.2 describes the procedure [[PAGE_BREAK]] in all material respects."}
]
</CITATIONS>

CRITICAL: The number inside the [N] marker in your prose is the "ref" value of a citation entry in the <CITATIONS> block — it is NOT a page number, footnote number, section number, or any other number that appears in the document. The marker [1] refers to the entry with "ref": 1 in the JSON block; [2] refers to "ref": 2; and so on. Refs are simple sequential integers you assign (1, 2, 3, ...) in the order citations appear in your prose. Never use a page number or a document's own numbering as the marker number. Every [N] you write in prose MUST have a matching {"ref": N, ...} entry in the JSON block.

Rules:
- Only cite text that appears verbatim in the provided documents
- In every <CITATIONS> entry, "doc_id" MUST be the exact chat-local document label you were given (for example "doc-0"). Never use a filename, document UUID, or any other identifier in "doc_id"
- Keep quotes short (ideally <= 25 words) and narrowly scoped to the specific claim. Don't reuse one quote to support multiple different claims — give each its own citation
- "page" refers to the sequential [Page N] marker in the text you were given (1-indexed from the first page). IGNORE any page numbers printed inside the document itself (footers, roman numerals, etc.)
- For a single-page quote, set "page" to an integer. If a quote is one continuous sentence that spans two pages, set "page" to "N-M" and insert [[PAGE_BREAK]] in the quote at the page break. Otherwise, use separate citations for text on different pages
- Put the <CITATIONS> block at the very end of the response. Omit it entirely if there are no citations

DOCX GENERATION:
If asked to draft or generate a document, use the generate_docx tool to produce a downloadable Word document. Always use this tool rather than just displaying the document content inline when the user asks for a document to be created.
If the user follows up on a document you just generated and asks for changes (e.g. "make section 3 longer", "add a termination clause", "change the parties"), default to calling edit_document on that newly generated document — do NOT call generate_docx again to regenerate the whole document. Only fall back to generate_docx if the user explicitly asks for a brand-new document or the change is so sweeping that an edit would not be coherent.
After calling generate_docx, do NOT include any download links, URLs, or markdown links to the document in your prose response — the download card is presented automatically by the UI.
After calling generate_docx, you MUST call read_document on the returned doc_id before writing your prose response. Base your description on the generated document's actual text, not on memory of what you intended to generate.
Your prose response MUST include a short description of the generated document: what it is, its structure (key sections/clauses), and — if the draft was informed by any provided source documents — which sources you drew from and how. Keep it concise (typically 3–8 sentences or a short bulleted list). Refer to the document by filename, never by a download link.
When the description makes factual claims about the contents of the newly generated document, cite the generated document with [N] markers and a <CITATIONS> block exactly as specified in the DOCUMENT CITATION INSTRUCTIONS above. If you also make factual claims about provided source documents, cite those source documents separately. Omit the <CITATIONS> block if the description makes no such claims.
Heading hierarchy: always use Heading 1 before introducing Heading 2, Heading 2 before Heading 3, and so on. Never skip levels.
Numbering: all numbering MUST start from 1, never 0. Never duplicate the numbering prefix in heading text — pass "Introduction", never "1. Introduction".
Contracts: when generating a contract or agreement, always include a signatures block at the very end of the document on its own page, with a signature line for each party (party name + "By:", "Name:", "Title:", "Date:"). Contract preambles (recitals, "WHEREAS" clauses, parties block) must NOT be numbered.

DOCUMENT EDITING:
When using edit_document, any edit that adds, removes, or reorders a numbered clause, section, sub-clause, schedule, exhibit, or list item shifts every downstream number. You MUST update all affected numbering AND every cross-reference to those numbers in the same edit_document call:
- Renumber the sibling clauses/sections/sub-clauses that follow the change so the sequence stays contiguous.
- Find every in-document reference to the shifted numbers — e.g. "see Section 5", "pursuant to Clause 4.2(b)", "as set out in Schedule 3", "defined in Section 2.1" — and update them.
- Before issuing the edits, scan the full document (use read_document or find_in_document) to enumerate affected cross-references; do not assume references only appear near the change site.
- If you are uncertain whether a reference points to the shifted number or an unrelated number, err on the side of including it as an edit and explain in the reason field.
- When deleting square brackets, delete both the opening `[` and the closing `]`. Never leave behind an unmatched bracket.

WORKFLOWS:
When a user message begins with a [Workflow: <title> (id: <id>)] marker, the user has selected a workflow and you MUST apply it. Immediately call the read_workflow tool with that exact id to load the workflow's full prompt, then follow those instructions for the current turn. Do this before producing any other output or calling any other tools (aside from any document reads the workflow requires). Do not ask the user to confirm — the selection itself is the instruction to apply the workflow.

DOCX TEMPLATES:
When a user message begins with a [Template: <title> (id: <id>)] marker, the user has selected a DOCX template and you MUST produce a Word document using it. Immediately call describe_docx_template with that exact id to load the authoring contract (layout, section skeleton, required metadata, per-field guidance). Then collect the needed [PLACEHOLDER] values from the conversation, write the document body in Markdown following the section_skeleton, and finally call generate_docx(template_id=..., body_md=..., metadata={...}). Do NOT consider the user's request fulfilled until generate_docx has succeeded and you have presented the download to the user. The Template marker can co-occur with a Workflow marker — if both are present, apply the workflow's instructions while still producing the docx as the closing step.

DOCUMENT NAMING IN PROSE:
The chat-local labels ("doc-0", "doc-1", "doc-N", ...) are internal handles for tool calls and citation JSON ONLY. NEVER write them in your prose response or in any text the user reads — not in body text, not in headings, not in lists, not in tool-activity descriptions. The user does not know what "doc-0" means and seeing it is jarring. When referring to a document in prose, always use its filename. The only places "doc-N" identifiers are allowed are inside tool-call arguments and inside the <CITATIONS> JSON block's "doc_id" field.

GENERAL GUIDANCE:
- Be precise and professional
- Cite the specific document and quote when making claims about document content
- When no documents are provided, answer based on your legal knowledge
- Do not fabricate document content
- Do not use emojis in your responses
"#;

fn build_doc_system_prompt(docs: &[DocPayload]) -> String {
    let with_text: Vec<&DocPayload> = docs.iter().filter(|d| d.text.is_some()).collect();
    let with_imgs: Vec<&DocPayload> = docs.iter().filter(|d| !d.images.is_empty()).collect();
    if with_text.is_empty() && with_imgs.is_empty() { return String::new(); }

    // Use Mike's chat-local doc-N labels so the citation system works.
    let mut s = String::from(
        "The user has attached the following documents. Use them to answer the question. \
         Cite the document name when relevant. The 'doc-N' label is for use in <CITATIONS> JSON only — \
         in prose, refer to documents by their filename.\n\n",
    );
    for (idx, d) in with_text.iter().enumerate() {
        s.push_str(&format!(
            "=== {label} (filename: {fname}) ===\n{body}\n\n",
            label = format!("doc-{idx}"),
            fname = d.filename,
            body = d.text.as_deref().unwrap_or("")
        ));
    }
    let img_offset = with_text.len();
    for (i, d) in with_imgs.iter().enumerate() {
        s.push_str(&format!(
            "=== {label} (filename: {fname}, rendered as {n} page image(s) attached below) ===\n\n",
            label = format!("doc-{}", img_offset + i),
            fname = d.filename,
            n = d.images.len()
        ));
    }
    s
}

fn collect_images(docs: &[DocPayload]) -> Vec<String> {
    docs.iter().flat_map(|d| d.images.clone()).collect()
}

/// One retrieved KB chunk plus the citation tag it was rendered with so
/// the response post-processor can map the model's `[g1]`/`[p1]` text
/// references back to the source path + chunk index.
#[derive(Debug, Clone)]
pub struct RetrievedKbEntry {
    /// Tag used in the system prompt: "g1", "g2", "p1", ... — used by
    /// the citation parser to look the entry up.
    pub tag: String,
    /// "global" | "project". Surfaced in the prompt and copied into
    /// the citation JSON.
    pub scope_label: &'static str,
    pub source_path: String,
    pub document_id: String,
    pub chunk_index: i32,
    pub text: String,
    /// 1-based page number authoritative from the chunker (PDFs only).
    /// `None` for non-PDF formats. Forwarded into the citation JSON so
    /// the DocPanel can scroll directly to the right page instead of
    /// falling back to text-search.
    pub page: Option<i64>,
}

/// Maximum cosine distance accepted for a chunk to be included. Values
/// above this threshold are noise rather than relevant context — but
/// 0.6 was too aggressive for cross-lingual queries (e.g. asking in
/// English about an Italian-language GDPR), where multilingual-e5
/// similarities cluster ~0.05-0.10 lower than monolingual. With an
/// English question against an Italian corpus doc we observed valid
/// matches falling around 0.62-0.68 and getting culled, leading to
/// "no relevant passages found" answers despite the doc being
/// retrievable in principle. 0.75 still excludes cosine-distant
/// noise while admitting cross-lingual paraphrases.
#[cfg(feature = "rag")]
const KB_DISTANCE_THRESHOLD: f32 = 0.75;

/// Run vector retrieval against the user's library and return the
/// chunks ready to be rendered into the system prompt. The scope is
/// inferred from the chat's project_id + the project's isolation_mode.
/// Returns an empty vec when:
///  - the rag feature isn't compiled in
///  - the embedding service isn't initialised
///  - the user has no indexed documents in the relevant pool
///  - all retrieved chunks are above the distance threshold
#[cfg(feature = "rag")]
async fn retrieve_kb_chunks(
    state: &AppState,
    user_id: &str,
    chat_id: &str,
    user_query: &str,
    top_k_target: usize,
) -> Vec<RetrievedKbEntry> {
    let Some(svc) = state.embeddings.as_ref() else {
        return Vec::new();
    };
    if user_query.trim().is_empty() {
        return Vec::new();
    }

    // Resolve scope: chat → project_id → isolation_mode.
    let project_row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT project_id FROM chats WHERE id = ?",
    )
    .bind(chat_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let project_id: Option<String> = project_row.and_then(|(p,)| p);

    use crate::embeddings::service::SearchScope;
    let scope_label: &'static str;
    let chunks_result = match project_id.as_deref() {
        None => {
            scope_label = "global";
            svc.search(user_id, SearchScope::Global, user_query, top_k_target)
                .await
        }
        Some(pid) => {
            let mode: Option<(String,)> = sqlx::query_as(
                "SELECT isolation_mode FROM projects WHERE id = ?",
            )
            .bind(pid)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
            let strict = mode.as_ref().map(|(m,)| m.as_str()) == Some("strict");
            // The scope_label below is per-chunk decided after retrieval
            // (a chunk with project_id NULL is "global", with our pid is
            // "project"); we still set a useful default for the empty
            // path. Real labelling happens below.
            scope_label = "project";
            if strict {
                svc.search(user_id, SearchScope::ProjectStrict(pid), user_query, top_k_target)
                    .await
            } else {
                svc.search(user_id, SearchScope::ProjectShared(pid), user_query, top_k_target)
                    .await
            }
        }
    };
    let _ = scope_label;

    let chunks = match chunks_result {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("[rag] retrieval failed: {e}");
            return Vec::new();
        }
    };

    // Filter by distance + label per-chunk based on whether the row had
    // project_id NULL (global) or a value (project). We can't know the
    // raw project_id from the public RetrievedChunk; instead, we look
    // it up in synced_files via the document_id — cheap and accurate.
    let mut out: Vec<RetrievedKbEntry> = Vec::new();
    let mut g_idx = 0u32;
    let mut p_idx = 0u32;
    for c in chunks.into_iter().filter(|c| c.distance <= KB_DISTANCE_THRESHOLD) {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT project_id FROM synced_files WHERE document_id = ?",
        )
        .bind(&c.document_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
        let is_global = row.and_then(|(p,)| p).is_none();
        let (tag, scope_label) = if is_global {
            g_idx += 1;
            (format!("g{g_idx}"), "global")
        } else {
            p_idx += 1;
            (format!("p{p_idx}"), "project")
        };
        out.push(RetrievedKbEntry {
            tag,
            scope_label,
            source_path: c.source_path,
            document_id: c.document_id,
            chunk_index: c.chunk_index,
            text: c.text,
            page: c.page,
        });
    }
    out
}

#[cfg(not(feature = "rag"))]
async fn retrieve_kb_chunks(
    _state: &AppState,
    _user_id: &str,
    _chat_id: &str,
    _user_query: &str,
    _top_k_target: usize,
) -> Vec<RetrievedKbEntry> {
    Vec::new()
}

/// Lightweight description of a doc in the user's authoritative-corpus
/// library — enough to render the "you have these documents indexed"
/// section of the system prompt without dragging the full text in.
struct CorpusInventoryEntry {
    corpus_id: String,
    identifier: String,
    title: String,
    language: String,
    status: String,
}

/// Pull the list of corpus-sourced documents the user has indexed.
/// Used to seed the library-inventory section of the system prompt
/// so the model orients itself even when the user's question doesn't
/// trigger a semantic-retrieval hit on those documents.
async fn list_indexed_corpus_docs(
    state: &AppState,
    user_id: &str,
) -> Vec<CorpusInventoryEntry> {
    let rows: Vec<(String, String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT corpus_id, corpus_identifier, filename, corpus_language, status \
         FROM documents \
         WHERE user_id = ? AND corpus_id IS NOT NULL AND corpus_identifier IS NOT NULL \
         ORDER BY created_at DESC \
         LIMIT 50",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    rows.into_iter()
        .map(|(corpus_id, identifier, title, language, status)| CorpusInventoryEntry {
            corpus_id,
            identifier,
            title,
            language: language.unwrap_or_default(),
            status,
        })
        .collect()
}

/// Render the library inventory as a system-prompt block. Only docs
/// that have been **fully indexed** (status = "ready") are listed as
/// retrievable; documents in "syncing" or "interrupted" state are
/// surfaced separately so the model can tell the user about them but
/// shouldn't pretend to have their text available.
fn build_library_inventory_prompt(entries: &[CorpusInventoryEntry]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let mut ready: Vec<&CorpusInventoryEntry> = Vec::new();
    let mut other: Vec<&CorpusInventoryEntry> = Vec::new();
    for e in entries {
        if e.status == "ready" {
            ready.push(e);
        } else {
            other.push(e);
        }
    }

    let mut s = String::from(
        "<USER LIBRARY — authoritative corpus documents indexed for this user>\n\
         This is an awareness list ONLY. The documents below are indexed and \
         retrievable. When a question matches one of them, the relevant \
         passages appear in the <KNOWLEDGE BASE> block above tagged \
         [g1]/[g2]/[p1]/...\n\
         \n\
         IF <KNOWLEDGE BASE> CONTAINS [gN]/[pN] TAGS:\n\
           · Use them and cite via the rules in that section. The user's \
             documents are authoritative.\n\
         \n\
         IF <KNOWLEDGE BASE> IS EMPTY OR HAS NO RELEVANT MATCH:\n\
           · The semantic match was below threshold, NOT that the document \
             is missing. Do NOT say \"not currently loaded\" or \"not \
             available for direct querying\" — those phrasings are wrong \
             and confuse the user.\n\
           · You may answer from general knowledge if confident, BUT state \
             plainly that the answer isn't grounded in the user's library, \
             and suggest the user re-phrase or attach the doc directly if \
             they want a citation-backed answer.\n\
         \n\
         CITATION DOC_ID RULES (mandatory):\n\
           · NEVER use the inventory identifiers below (e.g. \"32016R0679\", \
             \"eurlex_32016R0679\") as `doc_id` in <CITATIONS>. Those are \
             corpus references, NOT citation handles.\n\
           · NEVER invent doc-N labels when no files are attached to this \
             chat — only use doc-N if the user actually attached a file.\n\
           · The ONLY valid `doc_id` values are: (a) the [gN]/[pN] tags from \
             <KNOWLEDGE BASE>, or (b) the doc-N labels of files actually \
             attached to this chat. Anything else gets dropped or mis-routed.\n\
         \n\
         If asked \"what do you have?\" or \"do you know X?\", answer based on \
         this list (no citation needed for the meta-answer).\n\n",
    );
    if !ready.is_empty() {
        s.push_str("Indexed and ready:\n");
        for e in &ready {
            s.push_str(&format!(
                "  · [{corpus}] {ident}: {title} ({lang})\n",
                corpus = e.corpus_id,
                ident = e.identifier,
                title = e.title,
                lang = e.language.to_uppercase(),
            ));
        }
    }
    if !other.is_empty() {
        s.push_str("\nIndexing in progress / interrupted (not yet retrievable):\n");
        for e in &other {
            s.push_str(&format!(
                "  · [{corpus}] {ident}: {title} — {status}\n",
                corpus = e.corpus_id,
                ident = e.identifier,
                title = e.title,
                status = e.status,
            ));
        }
    }
    s
}

/// Render retrieved chunks as a `<KNOWLEDGE BASE>` section. Empty
/// string when there are no chunks — the caller skips the section
/// entirely so we don't pollute the prompt with empty headers.
fn build_kb_system_prompt(chunks: &[RetrievedKbEntry]) -> String {
    if chunks.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        "<KNOWLEDGE BASE — retrieved excerpts (not full documents)>\n\
         These are partial passages selected by similarity to the user's question. \
         They come from the user's indexed library; they are NOT authoritative full \
         documents. If you need full context for any of them, either call the \
         `search_kb` tool to fetch more passages from the same area, or ask the \
         user to attach the document via the paperclip.\n\n",
    );
    for c in chunks {
        let basename = std::path::Path::new(&c.source_path)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| c.source_path.clone());
        s.push_str(&format!(
            "[{tag}] {scope} · {fname} (chunk {idx}):\n«{text}»\n\n",
            tag = c.tag,
            scope = c.scope_label,
            fname = basename,
            idx = c.chunk_index,
            text = c.text,
        ));
    }
    s.push_str(
        "CITING THESE PASSAGES (mandatory — read carefully):\n\
         When you cite ANY of the passages above:\n\
           1. Write the [tag] VERBATIM in your prose at the point of \
              reference — for example: \"Articolo 35 GDPR [g1]\".\n\
           2. INCLUDE a matching entry in the <CITATIONS> JSON block at \
              the end of your response. The KB tag IS your citation \
              identifier — these passages count as document references \
              and the <CITATIONS> block applies to them exactly the same \
              way it applies to attached documents.\n\
           3. In the <CITATIONS> entry, set \"doc_id\" to the EXACT tag \
              you used inline (\"g1\", \"g2\", \"p1\", etc.) — NOT a \
              number, NOT \"doc-0\", NOT a filename.\n\
           4. The `quote` field MUST be a verbatim substring of the \
              passage text shown above between «…» — do NOT translate, \
              paraphrase, summarise, or correct typography. Copy the \
              exact characters (including the original language and \
              punctuation). The viewer text-searches the PDF for this \
              quote to highlight it; any deviation breaks the highlight.\n\
              If you want to discuss the passage in the user's language \
              (e.g. translate while answering), do that in your prose, \
              but keep the JSON `quote` in the original.\n\n\
         Example for KB tags only:\n\
         \n\
         Prose: \"L'articolo 35 GDPR richiede una DPIA [g1].\"\n\
         <CITATIONS>\n\
         [\n  {\"doc_id\": \"g1\", \"quote\": \"...\"}\n]\n\
         </CITATIONS>\n\n\
         Skipping the <CITATIONS> block when you used [gN]/[pN] tags is \
         a bug — the UI relies on it to render the clickable pill that \
         opens the source document. The block is REQUIRED whenever any \
         [tag] appears in your prose.\n\
         </KNOWLEDGE BASE>\n",
    );
    s
}

/// Remove the `[Page N]` markers our PDF scanner prepends to each
/// extracted page when it concatenates them. The model often copies
/// these markers verbatim into citation quotes (because they appear
/// inside the chunk text it was given), but they aren't actually
/// present in the underlying PDF — leaving them in breaks the
/// PDF.js text-search highlight in the DocPanel viewer.
///
/// Strategy: drop standalone `[Page N]` tokens (with surrounding
/// whitespace), then collapse any double-spaces / leading newlines
/// the removal might leave behind. Quotes that don't contain a marker
/// pass through unchanged.
fn strip_page_markers(quote: &str) -> String {
    let mut out = String::with_capacity(quote.len());
    let bytes = quote.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        // Detect `[Page <digits>]` at byte i.
        if bytes[i] == b'[' && bytes.get(i..i + 6) == Some(b"[Page ") {
            let num_start = i + 6;
            let mut j = num_start;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > num_start && bytes.get(j) == Some(&b']') {
                // Skip the marker and a single trailing whitespace
                // character (newline or space) if present.
                i = j + 1;
                if i < bytes.len() && (bytes[i] == b'\n' || bytes[i] == b' ') {
                    i += 1;
                }
                continue;
            }
        }
        out.push(quote[i..].chars().next().unwrap());
        i += quote[i..].chars().next().unwrap().len_utf8();
    }
    // Trim and collapse the most common leftover artefact (leading
    // newline that remained when the marker was at the very start).
    out.trim_start().to_string()
}

/// Walk a citations JSON array and rewrite each entry's `quote` field
/// through `strip_page_markers`. Used by the chat-history loader so
/// citations persisted before the strip-on-write fix still render
/// without literal `[Page N]` contamination.
fn sanitise_annotations_quotes(value: Value) -> Value {
    let Value::Array(items) = value else {
        return value;
    };
    let cleaned = items
        .into_iter()
        .map(|item| {
            let Value::Object(mut obj) = item else {
                return item;
            };
            if let Some(q) = obj.get("quote").and_then(|v| v.as_str()) {
                let stripped = strip_page_markers(q);
                if stripped != q {
                    obj.insert("quote".into(), Value::String(stripped));
                }
            }
            Value::Object(obj)
        })
        .collect();
    Value::Array(cleaned)
}

/// Fallback path that synthesises citation entries from the inline
/// `[gN]`/`[pN]` markers in the assistant's response when the model
/// forgot to emit the trailing `<CITATIONS>` JSON block. Each unique
/// tag found in `text` that resolves to a `kb_by_tag` entry produces a
/// `{"doc_id": "<tag>", "quote": "..."}` shape that the downstream
/// resolver then enriches with `source: "kb"`, `path`, `page`, etc.
///
/// Returns `None` when `text` has no resolvable KB markers — caller
/// should treat that as "no citations" and ship an empty array.
fn synthesise_kb_citations_from_markers(
    text: &str,
    kb_by_tag: &HashMap<String, RetrievedKbEntry>,
) -> Option<Value> {
    use std::collections::BTreeSet;
    let re_iter = text.match_indices('[');
    let mut tags = BTreeSet::<String>::new();
    for (i, _) in re_iter {
        // Simple state machine: after `[` we accept `g|p` then digits then `]`.
        let bytes = text.as_bytes();
        if let Some(&b) = bytes.get(i + 1) {
            if b == b'g' || b == b'p' || b == b'G' || b == b'P' {
                let mut j = i + 2;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > i + 2 && bytes.get(j) == Some(&b']') {
                    let tag = text[i + 1..j].to_ascii_lowercase();
                    if kb_by_tag.contains_key(&tag) {
                        tags.insert(tag);
                    }
                }
            }
        }
    }
    if tags.is_empty() {
        return None;
    }
    let arr: Vec<Value> = tags
        .into_iter()
        .map(|tag| {
            // Use a short prefix of the chunk text as the synthesized
            // quote so the DocPanel still has something to highlight.
            // The resolver further down stamps the authoritative page
            // and source path so the click-to-open path still works.
            let quote = kb_by_tag
                .get(&tag)
                .map(|e| {
                    let t = e.text.trim();
                    let cap = 200.min(t.len());
                    let mut end = cap;
                    while end < t.len() && !t.is_char_boundary(end) {
                        end -= 1;
                    }
                    t[..end].to_string()
                })
                .unwrap_or_default();
            json!({ "doc_id": tag, "quote": quote })
        })
        .collect();
    tracing::info!(
        "[chat] no <CITATIONS> block in response — synthesised {} citation(s) from inline KB markers",
        arr.len()
    );
    Some(Value::Array(arr))
}

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"detail": msg})))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_chats).post(post_chat_root))
        .route("/{id}", get(get_chat).patch(patch_chat).delete(delete_chat))
        .route("/{id}/messages", get(get_messages))
        .route("/{id}/message", axum::routing::post(post_message))
        .route("/{id}/generate-title", axum::routing::post(generate_title))
}

// ---------------------------------------------------------------------------
// GET /chat  — list chats for user
// ---------------------------------------------------------------------------
async fn list_chats(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let rows: Vec<(String, String, Option<String>, Option<String>, String)> =
        sqlx::query_as(
            "SELECT id, user_id, project_id, title, updated_at \
             FROM chats WHERE user_id = ? ORDER BY updated_at DESC",
        )
        .bind(&auth.user_id)
        .fetch_all(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let chats: Vec<Value> = rows
        .into_iter()
        .map(|(id, user_id, project_id, title, updated_at)| {
            json!({
                "id": id,
                "user_id": user_id,
                "project_id": project_id,
                "title": title,
                "updated_at": updated_at,
            })
        })
        .collect();

    Ok(Json(json!({ "chats": chats })))
}

// ---------------------------------------------------------------------------
// POST /chat — dispatched by body shape
//   - { messages: [...], chat_id?, model? }     → SSE streaming
//   - { project_id?, title? } (no messages)    → create chat record (JSON)
// ---------------------------------------------------------------------------
async fn post_chat_root(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<Value>,
) -> Response {
    let has_messages = body
        .get("messages")
        .and_then(|v| v.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    tracing::info!("[chat] POST / dispatch: has_messages={has_messages}, user={}", auth.username);

    if has_messages {
        return stream_chat_root(state, auth, body).await;
    }
    create_chat_record(state, auth, body).await
}

async fn create_chat_record(
    state: Arc<AppState>,
    auth: AuthUser,
    body: Value,
) -> Response {
    let project_id = body.get("project_id").and_then(|v| v.as_str()).map(|s| s.to_string());
    let title = body.get("title").and_then(|v| v.as_str()).map(|s| s.to_string());

    let id = uuid::Uuid::new_v4().to_string();
    if let Err(e) = sqlx::query(
        "INSERT INTO chats (id, user_id, project_id, title) VALUES (?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .bind(&project_id)
    .bind(&title)
    .execute(&state.db)
    .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"detail": e.to_string()})),
        )
            .into_response();
    }

    (StatusCode::OK, Json(json!({ "id": id }))).into_response()
}

/// SSE handler for the upstream-Mike `POST /chat` shape.
/// Body: { messages: [{role, content}], chat_id?, model? }
/// Emits `data: {type: ...}` events that useAssistantChat parses.
async fn stream_chat_root(
    state: Arc<AppState>,
    auth: AuthUser,
    body: Value,
) -> Response {
    let model_request = body.get("model").and_then(|v| v.as_str()).map(|s| s.to_string());
    let chat_id_in = body.get("chat_id").and_then(|v| v.as_str()).map(|s| s.to_string());

    // Resolve / create chat row
    let (chat_id, is_new_chat) = match chat_id_in.clone() {
        Some(id) => {
            let exists: Option<(String,)> = sqlx::query_as(
                "SELECT id FROM chats WHERE id = ? AND user_id = ?",
            )
            .bind(&id)
            .bind(&auth.user_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
            if exists.is_none() {
                return (StatusCode::NOT_FOUND, Json(json!({"detail": "Chat not found"}))).into_response();
            }
            (id, false)
        }
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            if let Err(e) = sqlx::query(
                "INSERT INTO chats (id, user_id, project_id, title) VALUES (?, ?, NULL, NULL)",
            )
            .bind(&id)
            .bind(&auth.user_id)
            .execute(&state.db)
            .await
            {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"detail": e.to_string()}))).into_response();
            }
            (id, true)
        }
    };

    // Parse messages from the request body. The frontend sends the entire
    // running history; persist only the *last* user message.
    //
    // Each message carries optional structured `workflow` and `template`
    // chips set by the chat composer. The system prompt instructs the
    // LLM to look for `[Workflow: <title> (id: <id>)]` and
    // `[Template: <title> (id: <id>)]` markers at the start of the user
    // message — but the markers aren't sent over the wire as inline
    // text, they're carried as JSON fields. We materialise them into
    // the content here so the LLM observes them where its instructions
    // expect them to be.
    type ParsedMessage = (
        String,             // role
        String,             // content (with markers prepended)
        Option<String>,     // template_id if any, used downstream by chat handler
    );
    let messages_in: Vec<ParsedMessage> = body
        .get("messages")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let role = m.get("role").and_then(|r| r.as_str())?.to_string();
                    let content = m
                        .get("content")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string();
                    let wf_marker = m.get("workflow").and_then(|wf| {
                        let id = wf.get("id").and_then(|v| v.as_str())?;
                        let title =
                            wf.get("title").and_then(|v| v.as_str()).unwrap_or("");
                        Some(format!("[Workflow: {title} (id: {id})]"))
                    });
                    let template_id_for_chat = m
                        .get("template")
                        .and_then(|t| t.get("id"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let tpl_marker = m.get("template").and_then(|tpl| {
                        let id = tpl.get("id").and_then(|v| v.as_str())?;
                        let title =
                            tpl.get("title").and_then(|v| v.as_str()).unwrap_or("");
                        Some(format!("[Template: {title} (id: {id})]"))
                    });
                    // Compose: markers first, then a blank line, then
                    // the user's actual text. Markers don't apply to
                    // assistant or tool messages — only user picks chips.
                    let augmented = if role == "user" && (wf_marker.is_some() || tpl_marker.is_some()) {
                        let mut prefix = String::new();
                        if let Some(m) = wf_marker {
                            prefix.push_str(&m);
                            prefix.push('\n');
                        }
                        if let Some(m) = tpl_marker {
                            prefix.push_str(&m);
                            prefix.push('\n');
                        }
                        format!("{prefix}\n{content}")
                    } else {
                        content
                    };
                    Some((role, augmented, template_id_for_chat))
                })
                .collect()
        })
        .unwrap_or_default();

    // Collect document_ids from message-level attachments.
    let mut doc_ids: Vec<String> = Vec::new();
    if let Some(arr) = body.get("messages").and_then(|v| v.as_array()) {
        for m in arr {
            if let Some(files) = m.get("files").and_then(|v| v.as_array()) {
                for f in files {
                    if let Some(id) = f.get("document_id").and_then(|v| v.as_str()) {
                        if !doc_ids.iter().any(|x| x == id) {
                            doc_ids.push(id.to_string());
                        }
                    }
                }
            }
        }
    }

    // Stamp this chat onto any newly attached cache documents so
    // chat-deletion can sweep their on-disk files (see migration
    // 0013). Restrictions:
    //   - chat_id IS NULL  → don't reroute a doc already linked to
    //     another chat (its cleanup belongs there).
    //   - content_hash IS NOT NULL  → only true for cache uploads.
    //     Project-scoped or pre-cache docs must NOT inherit chat_id,
    //     otherwise deleting the chat would cascade them away even
    //     though they live in a project library.
    if !doc_ids.is_empty() {
        let placeholders = doc_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "UPDATE documents SET chat_id = ? \
             WHERE user_id = ? \
               AND chat_id IS NULL \
               AND content_hash IS NOT NULL \
               AND id IN ({})",
            placeholders
        );
        let mut q = sqlx::query(&sql).bind(&chat_id).bind(&auth.user_id);
        for id in &doc_ids {
            q = q.bind(id);
        }
        match q.execute(&state.db).await {
            Ok(res) => tracing::info!(
                "[chat] linked {}/{} attached cache doc(s) to chat {}",
                res.rows_affected(),
                doc_ids.len(),
                chat_id
            ),
            Err(e) => tracing::warn!(
                "[chat] failed to link attached docs to chat {}: {}",
                chat_id,
                e
            ),
        }
    }

    // Also pull in every document already linked to this chat from an
    // earlier turn. On a reopened chat the frontend no longer carries the
    // attachment in the message payload, so without this a follow-up turn
    // would drop the document from context and — worse — its `[N]`
    // citations would resolve to no `document_id`, leaving the viewer with
    // a "document not found" on a document that was never actually lost.
    // Appended after the payload ids so existing `doc-N` indices stay
    // stable when the payload does still carry the files.
    if let Ok(rows) = sqlx::query_as::<_, (String,)>(
        "SELECT id FROM documents WHERE chat_id = ? AND user_id = ? ORDER BY created_at ASC",
    )
    .bind(&chat_id)
    .bind(&auth.user_id)
    .fetch_all(&state.db)
    .await
    {
        for (id,) in rows {
            if !doc_ids.iter().any(|x| x == &id) {
                doc_ids.push(id);
            }
        }
    }

    // Persist the *last* user message. We store:
    //   - the ORIGINAL content (raw user-typed text), not the
    //     marker-augmented form that goes to the LLM — markers like
    //     `[Workflow: ...]` are an LLM-side hint, putting them in the
    //     replayable history would surface as literal text on chat
    //     reopen.
    //   - the structured `files` / `workflow` / `template` JSON blobs
    //     so the composer pills come back when the chat is reopened
    //     (see migration 0021).
    let last_user_msg_json = body
        .get("messages")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter().rev().find(|m| {
                m.get("role").and_then(|r| r.as_str()) == Some("user")
            })
        });
    if let Some(msg) = last_user_msg_json {
        let raw_content = msg
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        if !raw_content.trim().is_empty() {
            // Serialise structured metadata to JSON strings, NULL when
            // absent/empty so the column matches the "no metadata"
            // shape every existing row already carries.
            let files_json = msg
                .get("files")
                .filter(|v| v.is_array() && !v.as_array().map(|a| a.is_empty()).unwrap_or(true))
                .map(|v| v.to_string());
            let workflow_json = msg
                .get("workflow")
                .filter(|v| v.is_object())
                .map(|v| v.to_string());
            let template_json = msg
                .get("template")
                .filter(|v| v.is_object())
                .map(|v| v.to_string());

            let user_msg_id = uuid::Uuid::new_v4().to_string();
            let _ = sqlx::query(
                "INSERT INTO messages (id, chat_id, role, content, files, workflow, template) \
                 VALUES (?, ?, 'user', ?, ?, ?, ?)",
            )
            .bind(&user_msg_id)
            .bind(&chat_id)
            .bind(&raw_content)
            .bind(&files_json)
            .bind(&workflow_json)
            .bind(&template_json)
            .execute(&state.db)
            .await;
        }
    }

    let messages: Vec<Message> = messages_in
        .iter()
        .filter_map(|(role, content, _template_id)| {
            let r = match role.as_str() {
                "user" => Role::User,
                "assistant" => Role::Assistant,
                "tool" => Role::Tool,
                _ => return None,
            };
            Some(Message {
                role: r,
                content: content.clone(),
                images: vec![],
                tool_calls: vec![],
                tool_call_id: None,
                tool_name: None,
            })
        })
        .collect();

    // Resolve LLM config from the user's saved settings
    let user_settings = fetch_llm_settings(&state.db, &auth.user_id).await.ok();
    let raw_model = model_request
        .or_else(|| user_settings.as_ref().and_then(|s| s.main_model.clone()))
        .unwrap_or_else(|| "gemini-3-flash-preview".to_string());

    let local_config = build_local_config(&raw_model, user_settings.as_ref());

    let vision_ok = llm::is_vision_capable(&raw_model);

    // Last user message is what we embed for retrieval. We deliberately
    // skip the conversation history because cosine on the running
    // history smears across topics; the latest turn captures intent
    // best. See the strategy doc for the rationale.
    let last_user_query: String = messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, Role::User))
        .map(|m| m.content.clone())
        .unwrap_or_default();
    let kb_top_k = if doc_ids.is_empty() { 8 } else { 6 };

    // Discover MCP, load attached docs, retrieve KB chunks, and pull
    // a library inventory in parallel. The inventory is what tells the
    // model "the user has the GDPR and AI Act in their indexed library"
    // even when the user's question doesn't surface those documents
    // via semantic match — without it, the model defaults to "I don't
    // have access to your synced documents."
    let (attached_docs, mcp_servers, kb_chunks, library_inventory) = tokio::join!(
        load_attached_docs(&state, &auth.user_id, &doc_ids, vision_ok),
        discover_mcp_for_user(&state, &auth.user_id),
        retrieve_kb_chunks(&state, &auth.user_id, &chat_id, &last_user_query, kb_top_k),
        list_indexed_corpus_docs(&state, &auth.user_id),
    );

    // Compose: Mike base + library inventory + KB excerpts + attached
    // full-text + MCP. Library inventory comes near the top so the
    // model orients itself before the semantic-retrieval block —
    // which may have missed documents the user has but didn't trigger.
    let inventory_prompt = build_library_inventory_prompt(&library_inventory);
    let mcp_prompt = build_mcp_system_prompt(&mcp_servers);
    let docs_prompt = build_doc_system_prompt(&attached_docs);
    let kb_prompt = build_kb_system_prompt(&kb_chunks);
    let mut sections: Vec<String> = vec![MRUST_SYSTEM_PROMPT.trim().to_string()];
    if !inventory_prompt.is_empty() {
        sections.push(inventory_prompt);
    }
    if !kb_prompt.is_empty() {
        sections.push(kb_prompt);
    }
    if !docs_prompt.is_empty() {
        sections.push(docs_prompt);
    }
    if !mcp_prompt.is_empty() {
        sections.push(mcp_prompt);
    }
    let system_prompt = sections.join("\n\n---\n\n");
    let images = if vision_ok { collect_images(&attached_docs) } else { Vec::new() };

    let mut messages = messages;
    if !images.is_empty() {
        // Attach the rendered page images to the *last* user message, which is
        // the one the model is replying to. Falls through silently if there is
        // no user message in the history.
        if let Some(last_user) = messages.iter_mut().rev().find(|m| matches!(m.role, Role::User)) {
            last_user.images = images.clone();
        }
    }

    tracing::info!(
        "[chat] stream_chat_root: chat_id={chat_id}, model={raw_model}, vision_ok={vision_ok}, local_config_present={}, docs={}, mcp_servers={}, kb_chunks={} (sys_prompt={} chars, images={})",
        local_config.is_some(),
        attached_docs.len(),
        mcp_servers.len(),
        kb_chunks.len(),
        system_prompt.len(),
        images.len()
    );

    // ─── Tools available to the model ────────────────────────────────
    // Builtin Mike tools first (read_document, find_in_document,
    // read_workflow, generate_docx stub, edit_document stub).
    let mut all_tools: Vec<ToolSchema> = builtin_tools::schemas();

    // MCP tools: injected ONLY for models that handle large tool
    // schemas reliably (see `llm::supports_mcp_tools` for the gate).
    // Smaller local models keep the previous behaviour — the MCP
    // servers stay visible via the system-prompt summary
    // (`build_mcp_system_prompt`) but their tool schemas don't go
    // into the schema list. The system prompt structure is unchanged
    // either way; the only thing this gate decides is whether the
    // model receives the additional `tools` schemas at the wire
    // protocol level.
    let mcp_tools_enabled = llm::supports_mcp_tools(&raw_model);
    let mcp_tool_count: usize = mcp_servers
        .iter()
        .map(|s| s.tool_schemas.len())
        .sum();
    if mcp_tools_enabled {
        for srv in &mcp_servers {
            all_tools.extend(srv.tool_schemas.iter().cloned());
        }
    }

    // Map chat-local labels (`doc-0`, `doc-1`, …) to real document UUIDs so
    // builtin tools (read_document, find_in_document) can resolve them.
    let mut doc_label_map: HashMap<String, String> = HashMap::new();
    for (idx, doc_id) in doc_ids.iter().enumerate() {
        doc_label_map.insert(format!("doc-{idx}"), doc_id.clone());
    }

    tracing::info!(
        "[chat] tool-use: {} total tools (builtin + {} MCP, mcp_enabled={}), labels={:?}",
        all_tools.len(),
        mcp_tool_count,
        mcp_tools_enabled,
        doc_label_map.keys().collect::<Vec<_>>()
    );
    // Verbose dump of the MCP tool names actually being shipped in the
    // request — invaluable when a user reports "the model never calls
    // my MCP tool". If this log shows the tool name, the schema is on
    // the wire; if not, either the gate dropped it (model-not-supported)
    // or discovery never returned it (server-side handshake failure).
    if mcp_tools_enabled && mcp_tool_count > 0 {
        let mcp_tool_names: Vec<&str> = mcp_servers
            .iter()
            .flat_map(|s| s.tool_schemas.iter().map(|t| t.function.name.as_str()))
            .collect();
        tracing::info!(
            "[chat] MCP tools shipped to model: {:?}",
            mcp_tool_names
        );
    } else if mcp_tool_count > 0 {
        let server_names: Vec<&str> = mcp_servers
            .iter()
            .map(|s| s.config_name.as_str())
            .collect();
        tracing::info!(
            "[chat] MCP servers discovered ({} tools total) but NOT shipped — model {:?} not in supports_mcp_tools allowlist. Servers: {:?}. Set MRUST_FORCE_MCP_TOOLS=1 to override.",
            mcp_tool_count,
            raw_model,
            server_names
        );
    }

    let claude_key = user_settings.as_ref().and_then(|s| s.claude_api_key.clone());
    let gemini_key = user_settings.as_ref().and_then(|s| s.gemini_api_key.clone());
    let gemini_region = user_settings.as_ref().and_then(|s| s.gemini_region.clone());

    // Compress older turns when the running history starts to crowd the
    // model's context window. The threshold (70%) leaves room for the
    // system prompt + RAG block + attached docs + reply. Failing-open:
    // if the summarizer LLM call errors, the original messages are
    // returned and the dispatch continues unchanged.
    let summarizer_creds = llm::summarize::SummarizerCreds {
        local_config: local_config.clone(),
        claude_api_key: claude_key.clone(),
        gemini_api_key: gemini_key.clone(),
        gemini_region: gemini_region.clone(),
    };
    let messages =
        llm::summarize::maybe_compress_history(messages, &raw_model, &summarizer_creds).await;

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);
    let state_clone = state.clone();
    let chat_id_clone = chat_id.clone();
    // Move retrieved KB chunks into the spawned task so the post-stream
    // citation parser can map model-emitted [g1]/[p1] tags back to the
    // source path + chunk index.
    let kb_chunks_for_citations = kb_chunks.clone();

    tokio::spawn(async move {
        if is_new_chat {
            let chat_id_event = json!({ "type": "chat_id", "chatId": &chat_id_clone });
            let _ = tx
                .send(Ok(Event::default().data(chat_id_event.to_string())))
                .await;
        }

        const MAX_TOOL_ITERATIONS: u32 = 5;
        let mut full_response = String::new();
        // Per-message persistent events. Today this collects the
        // `doc_created` envelopes so reopening the chat re-shows the
        // download card; the stored shape mirrors what the live SSE
        // stream sends so the frontend renders identically in both
        // paths. Other event types (tool_call_start, citations, …) are
        // streaming-only and deliberately not persisted.
        let mut persistent_events: Vec<Value> = Vec::new();
        let mut current_messages = messages;
        let mut iteration: u32 = 0;
        let mut errored = false;
        // Some models (e.g. gemma3 on Ollama) refuse the `tools` parameter
        // entirely. We detect that on the first call and disable tool-use
        // for the rest of the conversation, falling back to the system-prompt
        // listing (the model still "knows" the servers exist, just can't call them).
        // Persisted in AppState so we don't pay the retry on every message.
        let already_known_unsupported = state_clone
            .no_tools_models
            .read()
            .await
            .contains(&raw_model);
        let mut tools_supported = !all_tools.is_empty() && !already_known_unsupported;

        // If we already know this model does not support tools but there ARE
        // MCP servers configured, prepend an explicit warning to the response
        // so the user sees it in chat (not just in the backend log).
        let mut tool_warning_emitted = false;
        if !all_tools.is_empty() && already_known_unsupported {
            let warning = format!(
                "> ⚠️ **Tool-use non supportato dal modello selezionato** (`{}`). I {} \
                 server MCP configurati sono visibili nel mio contesto, ma non posso \
                 invocare direttamente i loro tools. Per il tool-use reale usa un \
                 modello compatibile: Claude, Gemini, GPT-4o, Qwen 2.5, Llama 3.1+, \
                 Mistral Small.\n\n---\n\n",
                raw_model,
                mcp_servers.len()
            );
            full_response.push_str(&warning);
            let payload = json!({ "type": "content_delta", "text": warning });
            let _ = tx.send(Ok(Event::default().data(payload.to_string()))).await;
            tool_warning_emitted = true;
        }

        loop {
            iteration += 1;
            let params = StreamParams {
                model: raw_model.clone(),
                system_prompt: system_prompt.clone(),
                messages: current_messages.clone(),
                tools: if tools_supported { all_tools.clone() } else { vec![] },
                max_iterations: 1,
                enable_thinking: false,
                local_config: local_config.clone(),
                claude_api_key: claude_key.clone(),
                gemini_api_key: gemini_key.clone(),
                gemini_region: gemini_region.clone(),
            };

            let stream = llm::stream_chat(params).await;
            match stream {
                Err(e) => {
                    let msg = e.to_string();
                    // Be precise: only treat as "model can't do tools" if the
                    // upstream explicitly says so. A generic 400 with "tool"
                    // in the body usually means a malformed schema, not a
                    // model limitation — surfacing the error is more useful.
                    let lower = msg.to_lowercase();
                    let unsupported = lower.contains("does not support tools")
                        || lower.contains("tools not supported")
                        || lower.contains("does not support tool use")
                        || lower.contains("tool use is not supported")
                        || lower.contains("functioncalling is not supported")
                        || lower.contains("function calling is not supported");
                    if tools_supported && unsupported {
                        tracing::warn!(
                            "[chat] model {raw_model}: tools rejected — \
                             retrying without tool-use. Original error: {}",
                            msg.chars().take(500).collect::<String>()
                        );
                        state_clone
                            .no_tools_models
                            .write()
                            .await
                            .insert(raw_model.clone());
                        tools_supported = false;
                        if !tool_warning_emitted && !all_tools.is_empty() {
                            let warning = format!(
                                "> ⚠️ **Tool-use non supportato dal modello selezionato** (`{}`). I {} \
                                 server MCP configurati sono visibili nel mio contesto, ma non posso \
                                 invocare direttamente i loro tools. Per il tool-use reale usa un \
                                 modello compatibile: Claude, Gemini, GPT-4o, Qwen 2.5, Llama 3.1+, \
                                 Mistral Small.\n\n---\n\n",
                                raw_model,
                                mcp_servers.len()
                            );
                            full_response.push_str(&warning);
                            let payload = json!({ "type": "content_delta", "text": warning });
                            let _ = tx.send(Ok(Event::default().data(payload.to_string()))).await;
                            tool_warning_emitted = true;
                        }
                        iteration -= 1; // don't count this as a real iteration
                        continue;
                    }
                    tracing::error!("[chat] stream_chat error (iter {iteration}): {e}");
                    let payload = json!({ "type": "error", "message": e.to_string() });
                    let _ = tx.send(Ok(Event::default().data(payload.to_string()))).await;
                    errored = true;
                    break;
                }
                Ok(mut s) => {
                    let mut iter_text = String::new();
                    let mut iter_tool_calls: Vec<ToolCall> = Vec::new();
                    let mut got_done = false;
                    let mut got_err: Option<String> = None;
                    while let Some(event) = s.next().await {
                        match event {
                            Ok(StreamEvent::ContentDelta(text)) => {
                                iter_text.push_str(&text);
                                full_response.push_str(&text);
                                let payload = json!({ "type": "content_delta", "text": text });
                                if tx
                                    .send(Ok(Event::default().data(payload.to_string())))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Ok(StreamEvent::ToolCalls(calls)) => {
                                iter_tool_calls = calls;
                            }
                            Ok(StreamEvent::Done) => { got_done = true; break; }
                            Err(e) => { got_err = Some(e.to_string()); break; }
                            _ => {}
                        }
                    }
                    tracing::info!(
                        "[chat] iter {iteration}: text={}, tool_calls={}, done={}, err={:?}",
                        iter_text.len(),
                        iter_tool_calls.len(),
                        got_done,
                        got_err
                    );

                    if iter_tool_calls.is_empty() {
                        // No more tools requested → final answer reached.
                        break;
                    }
                    if iteration >= MAX_TOOL_ITERATIONS {
                        tracing::warn!("[chat] hit MAX_TOOL_ITERATIONS, stopping");
                        let payload = json!({
                            "type": "content_delta",
                            "text": "\n\n_(stopped: too many tool iterations)_"
                        });
                        let _ = tx.send(Ok(Event::default().data(payload.to_string()))).await;
                        break;
                    }

                    // Replay the assistant's tool_calls in the next round, then
                    // dispatch each call and append its result as a `tool` message.
                    current_messages.push(Message::assistant_tool_calls(iter_tool_calls.clone()));
                    for call in &iter_tool_calls {
                        let payload = json!({ "type": "tool_call_start", "name": call.name });
                        let _ = tx.send(Ok(Event::default().data(payload.to_string()))).await;

                        // Race the dispatch against a 5-s ticker that
                        // emits `tool_call_progress` SSE events to the
                        // browser. Without this, slow MCP tools (e.g.
                        // Edge's pseudonymise-with-human-approval flow
                        // that can hold the connection for minutes
                        // while a user clicks Conferma in the Edge UI)
                        // looked silent in the chat — the user thought
                        // Mike had died. Now the chat shows
                        // "Sto eseguendo X (37s)…" so the wait is
                        // visibly progressing.
                        let dispatch_start_ts = std::time::Instant::now();
                        let tool_name_for_progress = call.name.clone();
                        let tx_progress = tx.clone();
                        let progress_task = tokio::spawn(async move {
                            // First tick at 5 s, then every 5 s after.
                            let mut ticker = tokio::time::interval(
                                std::time::Duration::from_secs(5),
                            );
                            // Skip the immediate first tick that
                            // tokio::interval fires.
                            ticker.tick().await;
                            loop {
                                ticker.tick().await;
                                let elapsed_secs =
                                    dispatch_start_ts.elapsed().as_secs();
                                let payload = json!({
                                    "type": "tool_call_progress",
                                    "name": tool_name_for_progress,
                                    "elapsed_secs": elapsed_secs,
                                });
                                if tx_progress
                                    .send(Ok(Event::default()
                                        .data(payload.to_string())))
                                    .await
                                    .is_err()
                                {
                                    // Receiver gone — stop ticking.
                                    return;
                                }
                            }
                        });

                        let result = if builtin_tools::is_builtin(&call.name) {
                            tracing::info!("[chat] dispatching builtin tool: {}", call.name);
                            builtin_tools::dispatch(
                                &state_clone,
                                &auth.user_id,
                                Some(&chat_id_clone),
                                &doc_label_map,
                                &call.name,
                                &call.input,
                            )
                            .await
                        } else {
                            tracing::info!("[chat] dispatching MCP tool: {}", call.name);
                            // Goes through the auto-chain wrapper so
                            // `request_*` calls that return a pending
                            // session_id automatically follow up with
                            // `get_*` instead of returning the pending
                            // envelope to the model.
                            dispatch_mcp_tool_with_async_chain(
                                &mcp_servers,
                                &call.name,
                                &call.input,
                            )
                            .await
                        };
                        progress_task.abort();
                        // For diagnostics: when a tool result is short
                        // it's almost always an error envelope or a
                        // pointer to async work. Log the body verbatim
                        // so we can tell at a glance whether the model
                        // is going to refuse vs proceed.
                        if result.len() <= 200 {
                            tracing::info!(
                                "[chat] tool {} result ({} chars): {}",
                                call.name,
                                result.len(),
                                result
                            );
                        } else {
                            tracing::info!(
                                "[chat] tool {} result: {} chars",
                                call.name,
                                result.len()
                            );
                        }
                        // If this tool produced a downloadable document
                        // (envelope `{ "doc_id": "...", "filename": "..." }`,
                        // no error), surface it to the chat UI as a
                        // `doc_created` event so the assistant message
                        // grows a download card. Today this fires for
                        // `generate_docx`; the same envelope is what any
                        // future generator (xlsx, pdf-export, …) will
                        // return, so the detection is on the shape rather
                        // than the tool name.
                        if let Ok(envelope) = serde_json::from_str::<Value>(&result)
                            && envelope.get("error").is_none()
                            && let (Some(doc_id), Some(filename)) = (
                                envelope.get("doc_id").and_then(|v| v.as_str()),
                                envelope.get("filename").and_then(|v| v.as_str()),
                            )
                        {
                            let payload = json!({
                                "type": "doc_created",
                                "filename": filename,
                                "download_url": format!("/document/{doc_id}/download"),
                                "document_id": doc_id,
                            });
                            let _ = tx
                                .send(Ok(Event::default().data(payload.to_string())))
                                .await;
                            // Persist so the card re-renders on chat
                            // reopen (see migration 0020 + get_chat
                            // hydration). Stored as the same shape the
                            // SSE event uses, plus `isStreaming: false`
                            // because by the time we serialise it the
                            // generation has finished.
                            persistent_events.push(json!({
                                "type": "doc_created",
                                "filename": filename,
                                "download_url": format!("/document/{doc_id}/download"),
                                "document_id": doc_id,
                                "isStreaming": false,
                            }));
                        }
                        current_messages.push(Message::tool_result(&call.id, &call.name, &result));
                    }
                }
            }
        }

        let got_done = !errored;
        let got_error: Option<String> = if errored { Some("see backend log".into()) } else { None };
        tracing::info!(
            "[chat] stream finished: chars={}, done={}, error={:?}",
            full_response.len(),
            got_done,
            got_error
        );

        // We hold the assistant-message id outside the if-block so the
        // citations-resolution step below can update the same row with
        // the parsed annotations JSON. Without that link the chat
        // history loses citations on reload (`get_messages` returns
        // content but not annotations) and `[g1]`/`[p1]` pills render
        // as plain text on old turns.
        // Persist the assistant turn whenever there's prose OR a
        // doc_created (or any other persistent event). The empty-prose
        // case happens when a tool call (e.g. `generate_docx`) is the
        // only thing the LLM produced this turn — without persistence
        // the download card would silently vanish on reopen.
        let asst_msg_id: Option<String> =
            if !full_response.is_empty() || !persistent_events.is_empty() {
                let id = uuid::Uuid::new_v4().to_string();
                let events_json = if persistent_events.is_empty() {
                    None
                } else {
                    Some(Value::Array(persistent_events.clone()).to_string())
                };
                let _ = sqlx::query(
                    "INSERT INTO messages (id, chat_id, role, content, events) \
                     VALUES (?, ?, 'assistant', ?, ?)",
                )
                .bind(&id)
                .bind(&chat_id_clone)
                .bind(&full_response)
                .bind(&events_json)
                .execute(&state_clone.db)
                .await;

                let _ = sqlx::query(
                    "UPDATE chats SET updated_at = datetime('now') WHERE id = ?",
                )
                .bind(&chat_id_clone)
                .execute(&state_clone.db)
                .await;
                Some(id)
            } else {
                None
            };

        // Parse the trailing <CITATIONS>…</CITATIONS> JSON block the model
        // is instructed to emit (see MRUST_SYSTEM_PROMPT). Resolve each
        // citation's `doc_id` (a chat-local label like "doc-0") back to the
        // real document UUID + filename so the frontend viewer can fetch
        // and highlight it.
        let mut id_by_label: HashMap<String, String> = HashMap::new();
        for (label, uuid) in &doc_label_map {
            id_by_label.insert(label.clone(), uuid.clone());
        }
        // Also fetch filenames so the citation entry contains it.
        let mut name_by_id: HashMap<String, String> = HashMap::new();
        for uuid in id_by_label.values() {
            if let Ok(Some((fname,))) = sqlx::query_as::<_, (String,)>(
                "SELECT filename FROM documents WHERE id = ? AND user_id = ?",
            )
            .bind(uuid)
            .bind(&auth.user_id)
            .fetch_optional(&state_clone.db)
            .await
            {
                name_by_id.insert(uuid.clone(), fname);
            }
        }

        // Build a tag → KB-entry index so we can resolve [g1]/[p1] back
        // to the source path the user-side viewer needs.
        let mut kb_by_tag: HashMap<String, RetrievedKbEntry> = HashMap::new();
        for entry in &kb_chunks_for_citations {
            kb_by_tag.insert(entry.tag.clone(), entry.clone());
        }

        // Build a corpus-identifier → tag fallback index so the citation
        // resolver can recover when the model invents a doc_id from the
        // <USER LIBRARY> inventory (e.g. "eurlex_32016R0679" or just
        // "32016R0679") instead of using the [gN] tag from the
        // <KNOWLEDGE BASE> section as instructed. Without this fallback
        // those citations get tagged source="attached", point at no
        // real document, and render as a 404 in the viewer.
        //
        // We index the same chunk under several normalised keys so a
        // model emitting any of "eurlex_32016R0679", "EUR-Lex/32016R0679",
        // "32016R0679", or "eurlex:32016R0679" still resolves.
        let mut corpus_ref_to_tag: HashMap<String, String> = HashMap::new();
        if !kb_by_tag.is_empty() {
            let doc_ids: std::collections::HashSet<String> = kb_chunks_for_citations
                .iter()
                .map(|e| e.document_id.clone())
                .collect();
            if !doc_ids.is_empty() {
                let placeholders = std::iter::repeat("?")
                    .take(doc_ids.len())
                    .collect::<Vec<_>>()
                    .join(",");
                let q = format!(
                    "SELECT id, corpus_id, corpus_identifier FROM documents \
                     WHERE user_id = ? AND id IN ({}) \
                       AND corpus_id IS NOT NULL AND corpus_identifier IS NOT NULL",
                    placeholders
                );
                let mut query = sqlx::query_as::<_, (String, String, String)>(&q)
                    .bind(&auth.user_id);
                for did in &doc_ids {
                    query = query.bind(did);
                }
                if let Ok(rows) = query.fetch_all(&state_clone.db).await {
                    // Build a doc_id → tag lookup once, then map every
                    // alias of (corpus_id, corpus_identifier) to it.
                    let mut tag_by_doc: HashMap<String, String> = HashMap::new();
                    for entry in &kb_chunks_for_citations {
                        tag_by_doc
                            .entry(entry.document_id.clone())
                            .or_insert_with(|| entry.tag.clone());
                    }
                    for (doc_uuid, corpus_id, ident) in rows {
                        let Some(tag) = tag_by_doc.get(&doc_uuid) else { continue };
                        let ident_lower = ident.to_ascii_lowercase();
                        let corpus_lower = corpus_id.to_ascii_lowercase();
                        for key in [
                            ident.clone(),
                            ident_lower.clone(),
                            format!("{corpus_id}_{ident}"),
                            format!("{corpus_lower}_{ident_lower}"),
                            format!("{corpus_id}:{ident}"),
                            format!("{corpus_lower}:{ident_lower}"),
                            format!("{corpus_id}/{ident}"),
                            format!("{corpus_lower}/{ident_lower}"),
                        ] {
                            corpus_ref_to_tag
                                .entry(key)
                                .or_insert_with(|| tag.clone());
                        }
                    }
                    if !corpus_ref_to_tag.is_empty() {
                        tracing::info!(
                            "[chat] built corpus-ref → tag fallback with {} aliases",
                            corpus_ref_to_tag.len()
                        );
                    }
                }
            }
        }

        let citations_json = extract_citations_block(&full_response).or_else(|| {
            // Fallback: model wrote [gN]/[pN] inline but skipped the
            // <CITATIONS> JSON block. Synthesise from markers so the
            // pills still render.
            synthesise_kb_citations_from_markers(&full_response, &kb_by_tag)
        });
        let citations_array: Vec<Value> = match citations_json {
            Some(v) => v
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|c| {
                    let label = c.get("doc_id").and_then(|x| x.as_str()).unwrap_or("");
                    let mut obj = c.as_object().cloned().unwrap_or_default();
                    obj.insert("type".into(), Value::String("citation_data".to_string()));

                    // Three resolution paths:
                    //  - "doc-N"           → attached document, lookup in id_by_label
                    //  - "g1" / "p1" / ... → KB chunk, lookup in kb_by_tag
                    //  - corpus identifier → KB chunk, via corpus_ref_to_tag
                    // Plus normalisation passes for variations the model
                    // produces in practice: "[g1]" (with brackets),
                    // "G1" (uppercase), "1" (just the number), and even
                    // "doc-0" emitted as a generic placeholder when no
                    // attached docs exist. The last fallback is the
                    // most robust: quote-based content matching against
                    // the kb chunks we actually fed to the model.
                    let original_label = label.to_string();
                    let normalised = original_label
                        .trim()
                        .trim_start_matches('[')
                        .trim_end_matches(']')
                        .to_ascii_lowercase();
                    let mut resolved_label = original_label.clone();
                    if !kb_by_tag.contains_key(&resolved_label)
                        && !id_by_label.contains_key(&resolved_label)
                    {
                        // Try the normalised form first.
                        if kb_by_tag.contains_key(&normalised) {
                            resolved_label = normalised.clone();
                        } else if id_by_label.contains_key(&normalised) {
                            resolved_label = normalised.clone();
                        } else if let Some(tag) = corpus_ref_to_tag
                            .get(&original_label)
                            .or_else(|| corpus_ref_to_tag.get(&normalised))
                        {
                            tracing::info!(
                                "[chat] citation doc_id {:?} not a known label/tag; \
                                 retro-resolving via corpus alias to KB tag {:?}",
                                original_label,
                                tag
                            );
                            resolved_label = tag.clone();
                        } else if normalised.chars().all(|c| c.is_ascii_digit())
                            && !normalised.is_empty()
                        {
                            // Bare number like "1": if there's exactly
                            // one [gN] in kb_by_tag, that's almost
                            // certainly what the model meant.
                            let g_keys: Vec<&String> = kb_by_tag
                                .keys()
                                .filter(|k| k.starts_with('g'))
                                .collect();
                            if g_keys.len() == 1 {
                                tracing::info!(
                                    "[chat] citation doc_id {:?} is bare number; \
                                     mapping to sole KB tag {:?}",
                                    original_label,
                                    g_keys[0]
                                );
                                resolved_label = g_keys[0].clone();
                            } else {
                                let candidate = format!("g{normalised}");
                                if kb_by_tag.contains_key(&candidate) {
                                    resolved_label = candidate;
                                }
                            }
                        }

                        // Quote-based content match: when the model
                        // copied a verbatim excerpt of a chunk into the
                        // citation quote, we can find the chunk it
                        // came from and use that tag. Cheaper than the
                        // single-doc fallback below, and more accurate
                        // when chunks span multiple corpus docs.
                        // Requires ≥25-char prefix so a short phrase
                        // doesn't accidentally match every chunk.
                        if resolved_label == original_label
                            && !kb_by_tag.contains_key(&resolved_label)
                            && !id_by_label.contains_key(&resolved_label)
                        {
                            if let Some(quote) = obj.get("quote").and_then(|v| v.as_str()) {
                                let needle = quote
                                    .split_whitespace()
                                    .collect::<Vec<_>>()
                                    .join(" ")
                                    .to_lowercase();
                                let needle_prefix: String =
                                    needle.chars().take(120).collect();
                                if needle_prefix.chars().count() >= 25 {
                                    let mut hit: Option<&str> = None;
                                    for (tag, kb) in &kb_by_tag {
                                        let hay = kb
                                            .text
                                            .split_whitespace()
                                            .collect::<Vec<_>>()
                                            .join(" ")
                                            .to_lowercase();
                                        if hay.contains(&needle_prefix) {
                                            hit = Some(tag.as_str());
                                            break;
                                        }
                                    }
                                    if let Some(tag) = hit {
                                        tracing::info!(
                                            "[chat] citation doc_id {:?} resolved by \
                                             quote-content match to KB tag {:?}",
                                            original_label,
                                            tag
                                        );
                                        resolved_label = tag.to_string();
                                    }
                                }
                            }
                        }

                        // Single-corpus-doc fallback: when every KB
                        // chunk we surfaced for this turn points at
                        // the same underlying corpus document, all
                        // citations almost certainly mean that one
                        // doc — even a paraphrased quote with a
                        // hallucinated page is "talking about GDPR".
                        // Map the unresolved label to any tag from
                        // that doc so the citation pill at least
                        // opens the right viewer. Not safe when KB
                        // chunks span multiple docs (we'd guess).
                        if resolved_label == original_label
                            && !kb_by_tag.contains_key(&resolved_label)
                            && !id_by_label.contains_key(&resolved_label)
                            && !kb_by_tag.is_empty()
                        {
                            let mut doc_ids: std::collections::HashSet<&str> =
                                std::collections::HashSet::new();
                            for kb in kb_by_tag.values() {
                                doc_ids.insert(kb.document_id.as_str());
                            }
                            if doc_ids.len() == 1 {
                                // Pick the lowest-numbered g-tag if any,
                                // otherwise the first tag we see.
                                let mut keys: Vec<&String> =
                                    kb_by_tag.keys().collect();
                                keys.sort();
                                let chosen = keys
                                    .iter()
                                    .find(|k| k.starts_with('g'))
                                    .copied()
                                    .or_else(|| keys.first().copied());
                                if let Some(tag) = chosen {
                                    tracing::info!(
                                        "[chat] citation doc_id {:?} unresolvable; \
                                         all KB chunks share one corpus doc — \
                                         routing to KB tag {:?} (page may be \
                                         hallucinated, viewer still opens correct file)",
                                        original_label,
                                        tag
                                    );
                                    resolved_label = tag.clone();
                                    // The model's page is likely
                                    // hallucinated when it invented
                                    // the doc_id — drop it so the
                                    // viewer falls back to opening
                                    // page 1 / using PDF.js text
                                    // search on the quote.
                                    obj.remove("page");
                                }
                            }
                        }

                        if resolved_label != original_label {
                            obj.insert(
                                "doc_id".into(),
                                Value::String(resolved_label.clone()),
                            );
                        }
                    }
                    let label = resolved_label.as_str();
                    if let Some(kb) = kb_by_tag.get(label) {
                        // Strip our scanner's `[Page N]` markers from
                        // the quote — the model often copies them
                        // verbatim from the chunk text we fed it, but
                        // they don't exist in the underlying PDF, so
                        // PDF.js text-search can't match.
                        if let Some(q) = obj.get("quote").and_then(|v| v.as_str()) {
                            let cleaned = strip_page_markers(q);
                            if cleaned != q {
                                obj.insert("quote".into(), Value::String(cleaned));
                            }
                        }
                        obj.insert("source".into(), Value::String("kb".to_string()));
                        obj.insert("scope".into(), Value::String(kb.scope_label.to_string()));
                        obj.insert("path".into(), Value::String(kb.source_path.clone()));
                        obj.insert("chunk_index".into(), Value::Number(kb.chunk_index.into()));
                        // document_id here points to the synced_files entry,
                        // not the upload-flow `documents` row — same field name
                        // for frontend simplicity.
                        obj.insert(
                            "document_id".into(),
                            Value::String(kb.document_id.clone()),
                        );
                        let basename = std::path::Path::new(&kb.source_path)
                            .file_name()
                            .map(|f| f.to_string_lossy().to_string())
                            .unwrap_or_else(|| kb.source_path.clone());
                        obj.insert("filename".into(), Value::String(basename));
                        // Page assignment: prefer the page the model
                        // emitted in <CITATIONS> if present. The model
                        // can see the literal `[Page N]` markers we
                        // prepend to each PDF page in the chunk text,
                        // and is more accurate per-quote than the
                        // chunker's coarse "page where this chunk
                        // STARTS" assignment — that one is wrong
                        // whenever a chunk spans multiple pages OR
                        // when the model picks a quote from the
                        // chunk's leading overlap section (which
                        // came from the previous chunk and may
                        // belong to a different page than the chunk
                        // is tagged with).
                        // Only stamp `kb.page` as a fallback when the
                        // model didn't provide a usable page.
                        let model_page_ok = obj
                            .get("page")
                            .map(|v| v.is_i64() || v.is_string())
                            .unwrap_or(false);
                        if !model_page_ok {
                            if let Some(p) = kb.page {
                                obj.insert("page".into(), Value::Number(p.into()));
                            }
                        }
                    } else {
                        obj.insert("source".into(), Value::String("attached".to_string()));
                        let uuid = id_by_label.get(label).cloned();
                        let filename = uuid
                            .as_ref()
                            .and_then(|u| name_by_id.get(u))
                            .cloned()
                            .unwrap_or_default();
                        if let Some(uuid) = uuid {
                            obj.insert("document_id".into(), Value::String(uuid));
                        }
                        if !filename.is_empty() {
                            obj.insert("filename".into(), Value::String(filename));
                        }
                    }
                    Value::Object(obj)
                })
                .collect(),
            None => Vec::new(),
        };
        tracing::info!("[chat] parsed {} citations from response", citations_array.len());

        // Persist the citation annotations on the assistant message so
        // GET /chat/:id/messages can hand them back when the user
        // reopens this chat from the sidebar.
        if let Some(id) = &asst_msg_id {
            let annotations_json = if citations_array.is_empty() {
                None
            } else {
                Some(Value::Array(citations_array.clone()).to_string())
            };
            match sqlx::query("UPDATE messages SET annotations = ? WHERE id = ?")
                .bind(&annotations_json)
                .bind(id)
                .execute(&state_clone.db)
                .await
            {
                Ok(r) => tracing::info!(
                    "[chat] annotations persisted on message id={} rows_affected={} payload_bytes={}",
                    id,
                    r.rows_affected(),
                    annotations_json.as_ref().map(|s| s.len()).unwrap_or(0),
                ),
                Err(e) => tracing::error!(
                    "[chat] FAILED to persist annotations on id={}: {e}",
                    id
                ),
            }
        }

        // Diagnostic: log the doc_id/source/page of each parsed citation
        // so we can tell whether the model emitted attached-style numeric
        // refs vs KB-style g1/p1 tags, and whether kb_by_tag matched.
        for (i, c) in citations_array.iter().enumerate() {
            tracing::info!(
                "[chat]   citation #{i}: doc_id={:?} source={:?} page={:?} ref={:?}",
                c.get("doc_id").and_then(|v| v.as_str()),
                c.get("source").and_then(|v| v.as_str()),
                c.get("page"),
                c.get("ref"),
            );
        }

        let done_payload = json!({ "type": "citations", "citations": citations_array });
        let _ = tx
            .send(Ok(Event::default().data(done_payload.to_string())))
            .await;
    });

    let sse_stream = ReceiverStream::new(rx);
    Sse::new(sse_stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

// ---------------------------------------------------------------------------
// GET /chat/:id
// ---------------------------------------------------------------------------
async fn get_chat(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let row: Option<(String, String, Option<String>, Option<String>, String)> =
        sqlx::query_as(
            "SELECT id, user_id, project_id, title, updated_at \
             FROM chats WHERE id = ? AND user_id = ?",
        )
        .bind(&id)
        .bind(&auth.user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (chat_id, user_id, project_id, title, updated_at) =
        row.ok_or_else(|| err(StatusCode::NOT_FOUND, "Chat not found"))?;

    type MsgRow = (
        String,         // id
        String,         // role
        Option<String>, // content
        String,         // created_at
        Option<String>, // annotations (assistant)
        Option<String>, // events (assistant)
        Option<String>, // files (user)
        Option<String>, // workflow (user)
        Option<String>, // template (user)
    );
    let msg_rows: Vec<MsgRow> = sqlx::query_as(
        "SELECT id, role, content, created_at, annotations, events, files, workflow, template \
         FROM messages WHERE chat_id = ? ORDER BY created_at ASC",
    )
    .bind(&chat_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let with_annot = msg_rows
        .iter()
        .filter(|r| r.1 == "assistant" && r.4.is_some())
        .count();
    let with_events = msg_rows
        .iter()
        .filter(|r| r.1 == "assistant" && r.5.is_some())
        .count();
    let with_files = msg_rows
        .iter()
        .filter(|r| r.1 == "user" && r.6.is_some())
        .count();
    tracing::info!(
        "[chat] GET /chat/{}: {} messages total, \
         {} assistant rows w/ annotations, {} w/ persistent events, \
         {} user rows w/ persisted files",
        chat_id,
        msg_rows.len(),
        with_annot,
        with_events,
        with_files,
    );

    let messages: Vec<Value> = msg_rows
        .into_iter()
        .map(|(mid, role, content, created_at, annotations, events, files, workflow, template)| {
            let content_value = if role == "assistant" {
                let mut arr = vec![json!({
                    "type": "content",
                    "text": content.unwrap_or_default(),
                })];
                // Append persisted non-text events (today: `doc_created`)
                // so the frontend's getChat path picks them up exactly
                // like the live SSE stream — see mikeApi.ts where the
                // events array is derived from `m.content`.
                if let Some(stored) = events.as_deref()
                    && let Ok(Value::Array(items)) = serde_json::from_str::<Value>(stored)
                {
                    arr.extend(items);
                }
                Value::Array(arr)
            } else {
                json!(content.unwrap_or_default())
            };
            // Hydrate annotations the same way the live SSE event does,
            // so the chat-history loader path delivers identical shape.
            // Re-apply `strip_page_markers` to each KB quote: rows
            // persisted before that fix landed contain the literal
            // `[Page N]` markers that PDF.js can't match — sanitising
            // on read makes old chats render correctly without a
            // destructive migration.
            let annotations_value = annotations
                .as_deref()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .map(sanitise_annotations_quotes)
                .unwrap_or_else(|| Value::Array(Vec::new()));
            // User-side metadata — files / workflow / template (see
            // migration 0021). Parse-on-read: if the stored JSON is
            // corrupt the field is dropped silently, the user message
            // still renders as plain text.
            let mut entry = json!({
                "id": mid,
                "role": role,
                "content": content_value,
                "created_at": created_at,
                "annotations": annotations_value,
            });
            if role == "user" {
                if let Some(v) = files
                    .as_deref()
                    .and_then(|s| serde_json::from_str::<Value>(s).ok())
                {
                    entry["files"] = v;
                }
                if let Some(v) = workflow
                    .as_deref()
                    .and_then(|s| serde_json::from_str::<Value>(s).ok())
                {
                    entry["workflow"] = v;
                }
                if let Some(v) = template
                    .as_deref()
                    .and_then(|s| serde_json::from_str::<Value>(s).ok())
                {
                    entry["template"] = v;
                }
            }
            entry
        })
        .collect();

    Ok(Json(json!({
        "chat": {
            "id": chat_id,
            "user_id": user_id,
            "project_id": project_id,
            "title": title,
            "updated_at": updated_at,
        },
        "messages": messages,
    })))
}

// ---------------------------------------------------------------------------
// PATCH /chat/:id  — update title
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct PatchChatBody {
    title: Option<String>,
}

async fn patch_chat(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<PatchChatBody>,
) -> ApiResult {
    let result = sqlx::query(
        "UPDATE chats SET title = COALESCE(?, title), updated_at = datetime('now') \
         WHERE id = ? AND user_id = ?",
    )
    .bind(&body.title)
    .bind(&id)
    .bind(&auth.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    if result.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "Chat not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// DELETE /chat/:id
// ---------------------------------------------------------------------------
async fn delete_chat(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    // Snapshot the cache-keyed paths of every doc linked to this chat
    // BEFORE the FK cascade (migration 0013) wipes the rows. We need
    // both storage_path (binary) and extracted_text_path so the
    // ref-count check can free the right files.
    let docs_to_check: Vec<(String, Option<String>, Option<String>, Option<String>)> =
        sqlx::query_as(
            "SELECT id, storage_path, extracted_text_path, content_hash \
             FROM documents WHERE chat_id = ? AND user_id = ?",
        )
        .bind(&id)
        .bind(&auth.user_id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

    let result = sqlx::query("DELETE FROM chats WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    if result.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "Chat not found"));
    }

    // FK cascade has already removed every documents row that pointed
    // at this chat. Two cleanup paths now:
    //
    //   - **Hash-keyed cache uploads** (`content_hash IS NOT NULL`,
    //     used by chat composer attachments) — ref-count check before
    //     deleting bytes: a hash shared with another chat keeps its
    //     files alive.
    //   - **Generated documents** (`content_hash IS NULL`, written by
    //     `exec_generate_docx`) — each has a unique
    //     `documents/<user_id>/<doc_id>` storage path, no dedup is
    //     possible, so the file is freed unconditionally.
    //
    // Without this second branch generated `.docx`s would be left
    // dangling on disk when the originating chat is deleted, slowly
    // bloating `data/storage/`.
    if !docs_to_check.is_empty() {
        if let Ok(storage) = make_storage() {
            let mut seen_hashes: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let mut generated_swept = 0usize;
            for (doc_id, sp, txt, hash) in &docs_to_check {
                match hash.as_ref() {
                    Some(hash) => {
                        if !seen_hashes.insert(hash.clone()) {
                            continue;
                        }
                        let still_referenced: Option<(i64,)> = sqlx::query_as(
                            "SELECT 1 FROM documents WHERE content_hash = ? LIMIT 1",
                        )
                        .bind(hash)
                        .fetch_optional(&state.db)
                        .await
                        .unwrap_or(None);
                        if still_referenced.is_some() {
                            tracing::info!(
                                "[chat] keeping cache files for hash {} \
                                 (still referenced by another doc)",
                                hash
                            );
                            continue;
                        }
                        if let Some(key) = sp.as_ref() {
                            if let Err(e) = storage.delete(key).await {
                                tracing::warn!(
                                    "[chat] failed to delete cache binary {} (doc {}): {}",
                                    key,
                                    doc_id,
                                    e
                                );
                            }
                        }
                        if let Some(key) = txt.as_ref() {
                            if let Err(e) = storage.delete(key).await {
                                tracing::warn!(
                                    "[chat] failed to delete cache text {} (doc {}): {}",
                                    key,
                                    doc_id,
                                    e
                                );
                            }
                        }
                    }
                    None => {
                        // Generated doc — storage path is unique per
                        // doc_id, no other row points at it, free
                        // unconditionally.
                        if let Some(key) = sp.as_ref() {
                            if let Err(e) = storage.delete(key).await {
                                tracing::warn!(
                                    "[chat] failed to delete generated doc binary {} (doc {}): {}",
                                    key,
                                    doc_id,
                                    e
                                );
                            } else {
                                generated_swept += 1;
                            }
                        }
                    }
                }
            }
            tracing::info!(
                "[chat] delete chat={} swept {} doc row(s) \
                 ({} unique cache hash(es), {} generated doc(s))",
                id,
                docs_to_check.len(),
                seen_hashes.len(),
                generated_swept,
            );
        }
    }

    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// GET /chat/:id/messages
// ---------------------------------------------------------------------------
/// Back-fill `document_id`/`filename` on citation annotations whose
/// `doc_id` is a chat-local `doc-N` label but whose real document link
/// was never resolved at write time. This happens on follow-up turns of
/// a reopened chat: the frontend no longer carries the attachment in the
/// payload, so the live citation enrichment had no label→UUID map and
/// persisted `document_id: null`. The document itself is never lost —
/// it stays linked to the chat via `documents.chat_id` — so resolving it
/// on read makes the viewer work again. `chat_docs` is the chat's
/// attached documents ordered as `doc-0`, `doc-1`, ….
fn enrich_doc_citations(mut value: Value, chat_docs: &[(String, String)]) -> Value {
    if chat_docs.is_empty() {
        return value;
    }
    // Annotations are persisted either as a bare array of citation
    // objects or as an object with a `citations` array — handle both.
    let cits = if value.is_array() {
        value.as_array_mut()
    } else {
        value.get_mut("citations").and_then(|v| v.as_array_mut())
    };
    let Some(cits) = cits else {
        return value;
    };
    for c in cits.iter_mut() {
        let Some(obj) = c.as_object_mut() else { continue };
        let resolved = obj
            .get("document_id")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty());
        if resolved {
            continue;
        }
        let idx = obj
            .get("doc_id")
            .and_then(|v| v.as_str())
            .and_then(|s| s.strip_prefix("doc-"))
            .and_then(|n| n.parse::<usize>().ok());
        let Some((doc_id, filename)) = idx.and_then(|i| chat_docs.get(i)) else {
            continue;
        };
        obj.insert("document_id".to_string(), Value::String(doc_id.clone()));
        let has_name = obj
            .get("filename")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty());
        if !has_name {
            obj.insert("filename".to_string(), Value::String(filename.clone()));
        }
    }
    value
}

async fn get_messages(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    // Verify ownership
    let exists: Option<(String,)> =
        sqlx::query_as("SELECT id FROM chats WHERE id = ? AND user_id = ?")
            .bind(&id)
            .bind(&auth.user_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    exists.ok_or_else(|| err(StatusCode::NOT_FOUND, "Chat not found"))?;

    // Documents attached to this chat, ordered as doc-0, doc-1, … so a
    // `doc-N` label maps back to a real document even on turns whose
    // citation links were never resolved at write time.
    let chat_docs: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, filename FROM documents WHERE chat_id = ? AND user_id = ? \
         ORDER BY created_at ASC",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let rows: Vec<(String, String, Option<String>, String, Option<String>)> = sqlx::query_as(
        "SELECT id, role, content, created_at, annotations FROM messages \
         WHERE chat_id = ? ORDER BY created_at ASC",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let with_annot = rows
        .iter()
        .filter(|(_, role, _, _, ann)| role == "assistant" && ann.is_some())
        .count();
    tracing::info!(
        "[chat] GET /chat/{}/messages: {} rows total, {} assistant rows with persisted annotations",
        id,
        rows.len(),
        with_annot,
    );

    let messages: Vec<Value> = rows
        .into_iter()
        .map(|(id, role, content, created_at, annotations)| {
            // Hydrate annotations from the stored JSON so the chat-history
            // path delivers the same shape as the live SSE event. Falls
            // back to an empty array when the column is NULL (older
            // assistant turns from before migration 0012).
            let annotations_value = annotations
                .as_deref()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .map(|v| enrich_doc_citations(v, &chat_docs))
                .unwrap_or_else(|| Value::Array(Vec::new()));
            json!({
                "id": id,
                "role": role,
                "content": content,
                "created_at": created_at,
                "annotations": annotations_value,
            })
        })
        .collect();

    Ok(Json(json!({ "messages": messages })))
}

// ---------------------------------------------------------------------------
// POST /chat/:id/message  — SSE streaming
// Body: { content, model?, system_prompt? }
// Response: text/event-stream with delta/done events
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct PostMessageBody {
    content: String,
    model: Option<String>,
    system_prompt: Option<String>,
}

async fn post_message(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(chat_id): Path<String>,
    Json(body): Json<PostMessageBody>,
) -> Response {
    // Verify ownership
    let exists: Option<(String,)> =
        sqlx::query_as("SELECT id FROM chats WHERE id = ? AND user_id = ?")
            .bind(&chat_id)
            .bind(&auth.user_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();

    if exists.is_none() {
        return (StatusCode::NOT_FOUND, Json(json!({"detail": "Chat not found"}))).into_response();
    }

    // Persist user message
    let user_msg_id = uuid::Uuid::new_v4().to_string();
    if let Err(e) = sqlx::query(
        "INSERT INTO messages (id, chat_id, role, content) VALUES (?, ?, 'user', ?)",
    )
    .bind(&user_msg_id)
    .bind(&chat_id)
    .bind(&body.content)
    .execute(&state.db)
    .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"detail": e.to_string()})),
        )
            .into_response();
    }

    // Load conversation history (last 50 messages)
    let history: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT role, content FROM messages WHERE chat_id = ? ORDER BY created_at ASC LIMIT 50")
            .bind(&chat_id)
            .fetch_all(&state.db)
            .await
            .unwrap_or_default();

    let messages: Vec<Message> = history
        .into_iter()
        .filter_map(|(role, content)| {
            let r = match role.as_str() {
                "user" => Role::User,
                "assistant" => Role::Assistant,
                "tool" => Role::Tool,
                _ => return None,
            };
            Some(Message { role: r, content: content.unwrap_or_default(), images: vec![], tool_calls: vec![], tool_call_id: None, tool_name: None })
        })
        .collect();

    // Resolve model from request or user settings
    let user_settings = fetch_llm_settings(&state.db, &auth.user_id)
        .await
        .ok();

    let raw_model = body.model.clone().unwrap_or_else(|| {
        user_settings
            .as_ref()
            .and_then(|s| s.main_model.clone())
            .unwrap_or_else(|| "gemini-3-flash-preview".to_string())
    });
    let model = raw_model.clone();

    // Build per-provider config from saved settings.
    let local_config = build_local_config(&model, user_settings.as_ref());

    let system_prompt = body.system_prompt.unwrap_or_default();

    let params = StreamParams {
        model: model.clone(),
        system_prompt,
        messages,
        tools: vec![],
        max_iterations: 1,
        enable_thinking: false,
        local_config,
        claude_api_key: user_settings.as_ref().and_then(|s| s.claude_api_key.clone()),
        gemini_api_key: user_settings.as_ref().and_then(|s| s.gemini_api_key.clone()),
        gemini_region: user_settings.as_ref().and_then(|s| s.gemini_region.clone()),
    };

    // SSE stream
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);
    let state_clone = state.clone();
    let chat_id_clone = chat_id.clone();

    tokio::spawn(async move {
        let mut full_response = String::new();

        match llm::stream_chat(params).await {
            Err(e) => {
                let _ = tx
                    .send(Ok(Event::default().event("error").data(e.to_string())))
                    .await;
            }
            Ok(mut stream) => {
                while let Some(event) = stream.next().await {
                    match event {
                        Ok(StreamEvent::ContentDelta(text)) => {
                            full_response.push_str(&text);
                            let data = serde_json::to_string(&json!({ "delta": text }))
                                .unwrap_or_default();
                            if tx.send(Ok(Event::default().event("delta").data(data))).await.is_err() {
                                break;
                            }
                        }
                        Ok(StreamEvent::Done) | Err(_) => break,
                        _ => {}
                    }
                }

                // Persist assistant message
                let asst_msg_id = uuid::Uuid::new_v4().to_string();
                let _ = sqlx::query(
                    "INSERT INTO messages (id, chat_id, role, content) VALUES (?, ?, 'assistant', ?)",
                )
                .bind(&asst_msg_id)
                .bind(&chat_id_clone)
                .bind(&full_response)
                .execute(&state_clone.db)
                .await;

                // Update chat timestamp
                let _ = sqlx::query(
                    "UPDATE chats SET updated_at = datetime('now') WHERE id = ?",
                )
                .bind(&chat_id_clone)
                .execute(&state_clone.db)
                .await;

                let done_data = serde_json::to_string(&json!({ "message_id": asst_msg_id }))
                    .unwrap_or_default();
                let _ = tx.send(Ok(Event::default().event("done").data(done_data))).await;
            }
        }
    });

    let sse_stream = ReceiverStream::new(rx);
    Sse::new(sse_stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

// ---------------------------------------------------------------------------
// POST /chat/:id/generate-title — short title from first user message
// ---------------------------------------------------------------------------
async fn generate_title(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(chat_id): Path<String>,
) -> ApiResult {
    let owns: Option<(String,)> = sqlx::query_as("SELECT id FROM chats WHERE id = ? AND user_id = ?")
        .bind(&chat_id)
        .bind(&auth.user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    if owns.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "Chat not found"));
    }

    let first: Option<(String,)> = sqlx::query_as(
        "SELECT content FROM messages WHERE chat_id = ? AND role = 'user' \
         ORDER BY created_at ASC LIMIT 1",
    )
    .bind(&chat_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let Some((first_msg,)) = first else {
        return Ok(Json(json!({ "title": null })));
    };

    let user_settings = fetch_llm_settings(&state.db, &auth.user_id).await.ok();
    // Pick a model from user settings — prefer the active provider, then any
    // configured one. Falling back to Gemini default fails when the user only
    // has a Local/OpenAI key set, so try to match what the chat is actually using.
    //
    // Crucially every candidate model must have its endpoint/key configured —
    // otherwise we'd happily pick `local:llama3.2:3b` only to 502 because the
    // user never wrote a localBaseUrl.
    let is_usable = |m: &str, s: &crate::routes::user::LlmSettings| -> bool {
        if let Some(rest) = m.strip_prefix("local:") {
            return !rest.is_empty()
                && s.local_base_url
                    .as_deref()
                    .map(|x| !x.trim().is_empty())
                    .unwrap_or(false);
        }
        if let Some(rest) = m.strip_prefix("openai:") {
            return !rest.is_empty()
                && s.openai_api_key
                    .as_deref()
                    .map(|x| !x.trim().is_empty())
                    .unwrap_or(false);
        }
        if let Some(rest) = m.strip_prefix("mistral:") {
            return !rest.is_empty()
                && s.mistral_api_key
                    .as_deref()
                    .map(|x| !x.trim().is_empty())
                    .unwrap_or(false);
        }
        if m.starts_with("claude") {
            return s
                .claude_api_key
                .as_deref()
                .map(|x| !x.trim().is_empty())
                .unwrap_or(false);
        }
        if m.starts_with("gemini") {
            return s
                .gemini_api_key
                .as_deref()
                .map(|x| !x.trim().is_empty())
                .unwrap_or(false);
        }
        false
    };
    let title_model = user_settings
        .as_ref()
        .and_then(|s| s.title_model.clone().filter(|m| is_usable(m, s)))
        .or_else(|| {
            user_settings
                .as_ref()
                .and_then(|s| s.main_model.clone().filter(|m| is_usable(m, s)))
        })
        .or_else(|| {
            user_settings.as_ref().and_then(|s| match s.active_provider.as_deref() {
                // For local/openai also require the corresponding endpoint
                // / API key to be configured — otherwise we'd pick a model
                // that has no way to be reached and the title generation
                // would 502.
                Some("local") => match (&s.local_model, &s.local_base_url) {
                    (Some(m), Some(b)) if !b.trim().is_empty() => Some(format!("local:{m}")),
                    _ => None,
                },
                Some("openai") => match (&s.openai_model, &s.openai_api_key) {
                    (Some(m), Some(k)) if !k.trim().is_empty() => Some(format!("openai:{m}")),
                    _ => None,
                },
                Some("mistral") => match (&s.mistral_model, &s.mistral_api_key) {
                    (Some(m), Some(k)) if !k.trim().is_empty() => Some(format!("mistral:{m}")),
                    _ => None,
                },
                Some("claude") => s
                    .claude_api_key
                    .as_ref()
                    .filter(|k| !k.trim().is_empty())
                    .map(|_| "claude-sonnet-4-6".to_string()),
                Some("gemini") => s
                    .gemini_api_key
                    .as_ref()
                    .filter(|k| !k.trim().is_empty())
                    .map(|_| "gemini-3-flash-preview".to_string()),
                _ => None,
            })
        })
        .or_else(|| {
            // No active_provider — pick first configured.
            let s = user_settings.as_ref()?;
            if let Some(m) = &s.local_model {
                if s.local_base_url.is_some() {
                    return Some(format!("local:{m}"));
                }
            }
            if let Some(m) = &s.openai_model {
                if s.openai_api_key.is_some() {
                    return Some(format!("openai:{m}"));
                }
            }
            if let Some(m) = &s.mistral_model {
                if s.mistral_api_key.is_some() {
                    return Some(format!("mistral:{m}"));
                }
            }
            if s.claude_api_key.is_some() { return Some("claude-sonnet-4-6".to_string()); }
            if s.gemini_api_key.is_some() { return Some("gemini-3-flash-preview".to_string()); }
            None
        })
        .unwrap_or_else(|| "gemini-3-flash-preview".to_string());

    tracing::info!("[chat] generate_title using model={title_model}");

    let local_config = build_local_config(&title_model, user_settings.as_ref());

    let prompt = format!(
        "Generate a concise 3-5 word title (no quotes, no punctuation) for a chat that begins with this user message:\n\n{}",
        first_msg.chars().take(500).collect::<String>()
    );

    let params = StreamParams {
        model: title_model.clone(),
        system_prompt: String::new(),
        messages: vec![Message::user(prompt)],
        tools: vec![],
        max_iterations: 1,
        enable_thinking: false,
        local_config,
        claude_api_key: user_settings.as_ref().and_then(|s| s.claude_api_key.clone()),
        gemini_api_key: user_settings.as_ref().and_then(|s| s.gemini_api_key.clone()),
        gemini_region: user_settings.as_ref().and_then(|s| s.gemini_region.clone()),
    };

    let title_text = match llm::provider_for_model(&title_model) {
        llm::Provider::Claude => llm::claude::complete(params).await,
        llm::Provider::OpenAI => llm::local::complete(params).await,
        llm::Provider::Gemini => llm::gemini::complete(params).await,
    }
    .map_err(|e| err(StatusCode::BAD_GATEWAY, &e.to_string()))?;

    let title: String = title_text
        .lines()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| c == '"' || c == '\'' || c.is_whitespace())
        .chars()
        .take(80)
        .collect();

    sqlx::query("UPDATE chats SET title = ?, updated_at = datetime('now') WHERE id = ?")
        .bind(&title)
        .bind(&chat_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({ "title": title })))
}

#[cfg(test)]
mod tests {
    use super::{
        enrich_doc_citations, extract_citations_block, sanitise_annotations_quotes,
        strip_page_markers,
    };
    use serde_json::{json, Value};

    #[test]
    fn sanitise_annotations_quotes_strips_each_entry() {
        let input = json!([
            { "doc_id": "g1", "quote": "[Page 1]\nFirst quote", "page": 1 },
            { "doc_id": "g2", "quote": "Plain quote", "page": 2 },
            { "doc_id": "g3", "quote": "[Page 3] Mid [Page 5] tail", "page": 3 },
        ]);
        let out = sanitise_annotations_quotes(input);
        let arr = out.as_array().expect("array");
        assert_eq!(arr[0]["quote"], "First quote");
        assert_eq!(arr[1]["quote"], "Plain quote");
        assert_eq!(arr[2]["quote"], "Mid tail");
    }

    #[test]
    fn sanitise_annotations_quotes_passes_non_array_through() {
        let v = json!({ "not": "array" });
        assert_eq!(sanitise_annotations_quotes(v.clone()), v);
    }

    #[test]
    fn sanitise_annotations_quotes_preserves_other_fields() {
        let input = json!([{
            "doc_id": "g1",
            "quote": "[Page 1]\ntext",
            "page": 1,
            "source": "kb",
            "scope": "global",
            "filename": "a.pdf",
        }]);
        let out = sanitise_annotations_quotes(input);
        let obj = out.as_array().unwrap()[0].as_object().unwrap();
        assert_eq!(obj["quote"], Value::String("text".to_string()));
        assert_eq!(obj["source"], "kb");
        assert_eq!(obj["scope"], "global");
        assert_eq!(obj["filename"], "a.pdf");
        assert_eq!(obj["page"], 1);
    }

    #[test]
    fn strip_page_markers_drops_leading_marker() {
        let q = "[Page 1]\nModello [2026] per la Valutazione…";
        assert_eq!(
            strip_page_markers(q),
            "Modello [2026] per la Valutazione…"
        );
    }

    #[test]
    fn strip_page_markers_drops_inline_marker() {
        let q = "qualcosa qui [Page 5] e qualcosa lì";
        assert_eq!(
            strip_page_markers(q),
            "qualcosa qui e qualcosa lì"
        );
    }

    #[test]
    fn strip_page_markers_handles_multi_digit() {
        let q = "[Page 123]\ntesto pagina centoventitré";
        assert_eq!(strip_page_markers(q), "testo pagina centoventitré");
    }

    #[test]
    fn strip_page_markers_preserves_other_brackets() {
        // Real document brackets like [2026] or [art. 5] must survive.
        let q = "Articolo [art. 5] del 2026 [2026]";
        assert_eq!(strip_page_markers(q), q);
    }

    #[test]
    fn strip_page_markers_preserves_non_marker_text() {
        let q = "Plain quote with no markers at all.";
        assert_eq!(strip_page_markers(q), q);
    }

    #[test]
    fn strip_page_markers_handles_multiple_markers() {
        let q = "[Page 1]\nfoo [Page 2]\nbar";
        assert_eq!(strip_page_markers(q), "foo bar");
    }

    #[test]
    fn extracts_plain_block() {
        let text = "Some answer.\n<CITATIONS>[{\"doc\":\"a\",\"page\":1}]</CITATIONS>";
        let v = extract_citations_block(text).unwrap();
        assert_eq!(v, json!([{"doc":"a","page":1}]));
    }

    #[test]
    fn extracts_block_with_code_fence() {
        let text = "Answer.\n<CITATIONS>\n```json\n[{\"x\":1}]\n```\n</CITATIONS>";
        let v = extract_citations_block(text).unwrap();
        assert_eq!(v, json!([{"x":1}]));
    }

    #[test]
    fn case_insensitive_tag() {
        let text = "<citations>[]</citations>";
        let v = extract_citations_block(text).unwrap();
        assert_eq!(v, json!([]));
    }

    #[test]
    fn returns_none_for_no_block() {
        assert!(extract_citations_block("plain text").is_none());
    }

    #[test]
    fn returns_none_for_unclosed_block() {
        assert!(extract_citations_block("<CITATIONS>[1,2,3]").is_none());
    }

    #[test]
    fn returns_none_for_invalid_json() {
        assert!(extract_citations_block("<CITATIONS>not json</CITATIONS>").is_none());
    }

    #[test]
    fn picks_last_block_when_multiple() {
        // rfind on "<citations>" → last opening tag wins.
        let text = "<CITATIONS>[1]</CITATIONS> ... <CITATIONS>[2]</CITATIONS>";
        let v = extract_citations_block(text).unwrap();
        assert_eq!(v, json!([2]));
    }

    // ── enrich_doc_citations ────────────────────────────────────────

    #[test]
    fn enrich_backfills_missing_document_id_from_chat_docs() {
        let docs = vec![
            ("uuid-a".to_string(), "first.pdf".to_string()),
            ("uuid-b".to_string(), "second.pdf".to_string()),
        ];
        let input = json!({
            "citations": [
                { "ref": 1, "doc_id": "doc-0", "document_id": null, "source": "attached" },
                { "ref": 2, "doc_id": "doc-1", "filename": "kept.pdf" },
            ]
        });
        let out = enrich_doc_citations(input, &docs);
        let cits = out["citations"].as_array().unwrap();
        assert_eq!(cits[0]["document_id"], "uuid-a");
        assert_eq!(cits[0]["filename"], "first.pdf");
        // An already-present filename is not overwritten.
        assert_eq!(cits[1]["document_id"], "uuid-b");
        assert_eq!(cits[1]["filename"], "kept.pdf");
    }

    #[test]
    fn enrich_leaves_resolved_citations_untouched() {
        let docs = vec![("uuid-a".to_string(), "first.pdf".to_string())];
        let input = json!({
            "citations": [
                { "ref": 1, "doc_id": "doc-0", "document_id": "real-uuid" },
            ]
        });
        let out = enrich_doc_citations(input.clone(), &docs);
        assert_eq!(out["citations"][0]["document_id"], "real-uuid");
    }

    #[test]
    fn enrich_handles_bare_array_annotations() {
        // Annotations are persisted as a bare array, not a wrapper object.
        let docs = vec![("uuid-a".to_string(), "first.pdf".to_string())];
        let input = json!([{ "ref": 1, "doc_id": "doc-0", "page": 4 }]);
        let out = enrich_doc_citations(input, &docs);
        assert_eq!(out[0]["document_id"], "uuid-a");
        assert_eq!(out[0]["filename"], "first.pdf");
    }

    #[test]
    fn enrich_is_a_noop_without_chat_docs() {
        let empty: Vec<(String, String)> = vec![];
        let v = json!([{ "doc_id": "doc-0" }]);
        assert_eq!(enrich_doc_citations(v.clone(), &empty), v);
    }

    #[test]
    fn enrich_skips_doc_label_out_of_range() {
        let docs = vec![("uuid-a".to_string(), "first.pdf".to_string())];
        let input = json!({ "citations": [{ "doc_id": "doc-5" }] });
        let out = enrich_doc_citations(input, &docs);
        assert!(out["citations"][0].get("document_id").is_none());
    }
}
