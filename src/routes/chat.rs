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
    let requested_model = llm::strip_model_prefix(model).trim().to_string();
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
        model: if requested_model.is_empty() {
            stored_model
                .filter(|m| !m.trim().is_empty())
                .unwrap_or_default()
        } else {
            requested_model
        },
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

/// Reduce a corpus identifier (or any model-emitted `doc_id` variant)
/// to its alphanumeric-only, lowercase canonical form. Used by the
/// citation resolver as a last-resort lookup key against the user's
/// full corpus library so that bracket/space/separator/case variants
/// the model produces (e.g. `[italian-legal] corte_costituzionale_1990_241`,
/// `Italian-Legal_corte_costituzionale_1990_241`, or even
/// `italianlegal:cortecostituzionale1990/241`) all collapse onto the
/// same key as the canonical `<corpus_id><corpus_identifier>` we index.
fn canonical_corpus_key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Repair the invalid backslash escapes LLMs routinely emit inside the
/// `<CITATIONS>` JSON. The model copies verbatim quotes and over-escapes
/// them — most commonly an apostrophe as `\'`, which is NOT a legal JSON
/// escape and makes `serde_json` reject the whole block. JSON only
/// allows `\` before `" \ / b f n r t u`; for any other follower the
/// backslash is spurious, so we drop it and keep the character. This
/// only ever turns an unparseable block into a parseable one.
fn repair_json_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some(&n) if matches!(n, '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' | 'u') => {
                out.push('\\');
                out.push(n);
                chars.next();
            }
            // Spurious escape (e.g. `\'`): drop the backslash, keep the char.
            Some(&n) => {
                out.push(n);
                chars.next();
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Extract the JSON inside a `<CITATIONS>...</CITATIONS>` block at the end
/// of the assistant response. Tolerant of:
/// * surrounding whitespace and `` ```json `` code fences,
/// * the invalid backslash escapes LLMs commonly emit (`\'` etc.),
/// * a **missing closing tag** — the model ran out of output tokens
///   before writing `</CITATIONS>` (observed when emitting 30+ citations
///   for a full report). In that case we take everything from the open
///   tag to end-of-text.
/// * a **truncated JSON array** — if the array itself was cut mid-entry,
///   recover the longest complete prefix (`[ {…}, {…}, …, {…} ]`) so
///   we surface the entries the model managed to finish.
///
/// Returns the parsed `Value` (an array) or `None`.
pub(crate) fn extract_citations_block(text: &str) -> Option<Value> {
    let lower = text.to_lowercase();
    let open = lower.rfind("<citations>")?;
    let after_open = open + "<citations>".len();
    let inner_raw = if let Some(close_rel) = lower[after_open..].find("</citations>") {
        text[after_open..after_open + close_rel].trim()
    } else {
        // No closing tag — model output was truncated before it
        // finished. Take everything that came through.
        text[after_open..].trim()
    };
    // Strip optional Markdown fences like ```json … ```
    let inner = inner_raw
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim();
    let inner = inner.trim_end_matches("```").trim();
    if let Ok(v) = serde_json::from_str::<Value>(inner) {
        return Some(v);
    }
    // Clean parse failed — most often an over-escaped apostrophe (`\'`).
    // Retry once with the escapes repaired.
    let repaired = repair_json_escapes(inner);
    if repaired != inner {
        if let Ok(v) = serde_json::from_str::<Value>(&repaired) {
            tracing::info!(
                "[chat] <CITATIONS> block parsed after repairing invalid JSON escapes"
            );
            return Some(v);
        }
    }
    // Last resort: truncation recovery. Walk the (repaired) prefix as
    // a JSON array, keep every complete top-level entry, drop the
    // partial one at the tail, close with `]`.
    if let Some(recovered) = recover_truncated_citations_array(&repaired) {
        let n = recovered.as_array().map(|a| a.len()).unwrap_or(0);
        tracing::warn!(
            "[chat] <CITATIONS> block was truncated — recovered first {n} entries from the JSON prefix"
        );
        return Some(recovered);
    }
    tracing::warn!("[chat] <CITATIONS> block found but is not valid JSON — citations dropped");
    None
}

/// Recover the longest valid `[…]` prefix from a truncated citations
/// JSON array. The input is the body that should have been a full
/// array; we walk it character-by-character respecting string scope
/// (so a quote-contained `}` doesn't fool us) and remember the offset
/// of the most recent `}` that closed a top-level array entry. Cutting
/// there and appending `]` gives a syntactically valid prefix.
///
/// Returns `None` when the prefix doesn't even start with `[` or no
/// complete entry was emitted.
fn recover_truncated_citations_array(inner: &str) -> Option<Value> {
    let s = inner.trim();
    if !s.starts_with('[') {
        return None;
    }
    let bytes = s.as_bytes();
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut esc = false;
    let mut last_top_level_entry_end: Option<usize> = None;
    for (i, &b) in bytes.iter().enumerate() {
        let c = b as char;
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                if c == '}' && depth == 2 {
                    // Closes an entry inside the outer array.
                    last_top_level_entry_end = Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    let cut = last_top_level_entry_end?;
    let recovered = format!("{}]", &s[..=cut]);
    serde_json::from_str::<Value>(&recovered).ok()
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
/// Emit a `doc_extract_*` SSE event so the chat UI can render a
/// "Estraendo testo da X" step before the PII pass kicks in. Without
/// it the user sees a multi-second block on long PDFs with no
/// feedback. `start = true` for the leading event, `false` for the
/// terminal one (carries `chars` so the UI can report doc size).
fn emit_doc_extract(
    tx: &tokio::sync::mpsc::Sender<Result<axum::response::sse::Event, std::convert::Infallible>>,
    filename: &str,
    chars: Option<usize>,
    done: bool,
) {
    use axum::response::sse::Event;
    let payload = if done {
        serde_json::json!({
            "type": "doc_extract_done",
            "filename": filename,
            "chars": chars.unwrap_or(0),
        })
    } else {
        serde_json::json!({
            "type": "doc_extract_start",
            "filename": filename,
        })
    };
    // try_send: a stuck client mustn't block the loader. Dropping a
    // progress tick is acceptable; the chat continues regardless.
    let _ = tx.try_send(Ok(Event::default().data(payload.to_string())));
}

async fn load_attached_docs(
    state: &AppState,
    user_id: &str,
    document_ids: &[String],
    vision_ok: bool,
    pii_protected_ids: &std::collections::HashSet<String>,
    sse_tx: &tokio::sync::mpsc::Sender<Result<axum::response::sse::Event, std::convert::Infallible>>,
) -> Vec<DocPayload> {
    let mut out = Vec::new();
    for doc_id in document_ids {
        let row: Option<(String, String, Option<String>, Option<String>, i64, String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT filename, file_type, storage_path, extracted_text_path, pii_protected, \
                    decision, decision_reason, decision_summary \
             FROM documents WHERE id = ? AND user_id = ?",
        )
        .bind(doc_id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();

        let Some((
            filename,
            file_type,
            Some(storage_path),
            extracted_text_path,
            persisted_pii,
            decision,
            decision_reason,
            decision_summary,
        )) = row
        else {
            continue;
        };

        // Rejected docs (per-chat decision, migration 0029) never load
        // their full text into the prompt. Instead we synthesise a
        // short note from the user-provided reason and the one-shot
        // LLM summary captured at reject-time, so the model on a
        // subsequent turn knows what was rejected and why — without
        // re-seeing the bytes the user already vetoed.
        if decision == "rejected" {
            let reason = decision_reason.as_deref().unwrap_or("(motivo non registrato)");
            let summary = decision_summary
                .as_deref()
                .unwrap_or("(riassunto non disponibile)");
            let stub = format!(
                "[Documento rifiutato dall'utente]\n\
                 Filename: {filename}\n\
                 Motivo del rifiuto: {reason}\n\
                 Riassunto della versione rifiutata: {summary}\n\
                 Indicazione operativa: non riprodurre questa versione \
                 così com'è; correggi tenendo conto del motivo sopra."
            );
            tracing::info!(
                "[chat] doc {filename} (id={doc_id}) rejected — substituting full \
                 text with reason+summary stub ({} chars)",
                stub.len()
            );
            out.push(DocPayload {
                filename: filename.clone(),
                text: Some(stub),
                images: Vec::new(),
            });
            continue;
        }

        // Effective protection = persisted column (set by an earlier
        // opt-in turn) OR the per-request set (set by this turn). The
        // OR-logic guarantees a follow-up text-only turn still
        // redacts a document the user opted-in to earlier, even if
        // the current payload doesn't repeat the flag.
        let pii_on = persisted_pii != 0 || pii_protected_ids.contains(doc_id);

        // Emit the leading step event so the UI shows "Estraendo
        // testo — file" the moment we start touching the file —
        // before any storage read, before PII. Closes via
        // `doc_extract_done` either in the cached fast path or
        // after the per-format extractor runs.
        emit_doc_extract(sse_tx, &filename, None, false);

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
                let chars = text.len();
                tracing::info!(
                    "[chat] using cached text for {filename}: {} chars",
                    chars
                );
                emit_doc_extract(sse_tx, &filename, Some(chars), true);
                let final_text = maybe_redact_pii(
                    text,
                    pii_on,
                    doc_id,
                    &filename,
                    sse_tx,
                )
                .await;
                out.push(DocPayload {
                    filename: filename.clone(),
                    text: Some(final_text),
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

        // Close the doc_extract step with the raw character count
        // BEFORE we apply PII redaction. The UI then transitions
        // into the pii_redact step (if protected) — two distinct
        // visual phases instead of one undifferentiated wait.
        let pre_redact_chars =
            payload.text.as_deref().map(|t| t.len()).unwrap_or(0);
        emit_doc_extract(sse_tx, &filename, Some(pre_redact_chars), true);

        // Apply PII redaction before the chars count log so the
        // log line reflects what actually goes to the LLM.
        if let Some(t) = payload.text.take() {
            payload.text = Some(
                maybe_redact_pii(
                    t,
                    pii_on,
                    doc_id,
                    &filename,
                    sse_tx,
                )
                .await,
            );
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

/// If `protected` is true AND the `ner-pii` feature is built in,
/// run `text` through GLiNER2 and return the redacted copy.
/// Emits `pii_redact_*` SSE events on `sse_tx` so the chat UI can
/// render a tool step with `n / N` chunk progress while a long
/// document is being processed (the engine is single-threaded per
/// session and a 100-page PDF can take 10-30 s).
/// On failure (model load, inference) the original text flows
/// through and the failure is logged — the user already saw the
/// blackbox disclaimer; breaking the chat over an inference glitch
/// would be worse.
/// Storage key for the cached PII-anonymised text of a document.
///
/// PII inference is the most expensive single step in `load_attached_docs`
/// (22-35 s per 2000-char chunk on CPU; a 9-page PDF is ~3-5 min). Rerunning
/// it on every chat turn for the same document is wasteful — once a
/// `[LABEL]`-redacted copy exists, the same masking is byte-for-byte stable
/// (the labels and threshold are pinned in `crate::ner::default_pii_labels`
/// / `crate::ner::PII_THRESHOLD`). We persist the redacted text alongside
/// the raw extracted text and consult it before any inference. The key
/// space is flat under `cache/pii/` so a future garbage-collection sweep
/// over deleted documents is a single prefix walk.
fn pii_cache_key(doc_id: &str) -> String {
    format!("cache/pii/{doc_id}.txt")
}

async fn maybe_redact_pii(
    text: String,
    protected: bool,
    doc_id: &str,
    filename: &str,
    #[allow(unused_variables)] sse_tx: &tokio::sync::mpsc::Sender<
        Result<axum::response::sse::Event, std::convert::Infallible>,
    >,
) -> String {
    tracing::info!(
        "[chat] maybe_redact_pii({filename}, doc_id={doc_id}) — protected={} ner-pii-built-in={}",
        protected,
        cfg!(feature = "ner-pii")
    );
    if !protected {
        return text;
    }

    // Cache fast-path. The redacted output is deterministic for a given
    // input + fixed labels + fixed threshold, so a previous turn's
    // result is reusable. We still emit the start/done SSE pair so the
    // UI stays consistent (and the user sees the "cache hit" is fast).
    let cache_key = pii_cache_key(doc_id);
    if let Ok(storage) = crate::storage::make_storage() {
        if let Ok(bytes) = storage.get(&cache_key).await {
            let cached = String::from_utf8_lossy(&bytes).into_owned();
            if !cached.is_empty() {
                tracing::info!(
                    "[chat] PII cache hit for {filename} (doc_id={doc_id}): {} chars",
                    cached.len()
                );
                use axum::response::sse::Event;
                // Two-event burst: start (with total=1 so the UI doesn't
                // try to render N/N progress) immediately followed by done.
                // try_send because the events are best-effort; if the
                // client already disconnected we shouldn't pay anything.
                let _ = sse_tx.try_send(Ok(Event::default().data(
                    json!({
                        "type": "pii_redact_start",
                        "filename": filename,
                        "total": 1,
                    })
                    .to_string(),
                )));
                let _ = sse_tx.try_send(Ok(Event::default().data(
                    json!({
                        "type": "pii_redact_done",
                        "filename": filename,
                    })
                    .to_string(),
                )));
                return cached;
            }
        }
    }

    #[cfg(feature = "ner-pii")]
    {
        use axum::response::sse::Event;
        // Bridge channel: spawn_blocking worker → tokio task that
        // forwards each (current, total) tick as a pii_redact_progress
        // SSE event. Bounded(16) so a stuck client never bloats memory.
        let (prog_tx, mut prog_rx) =
            tokio::sync::mpsc::channel::<(usize, usize)>(16);
        let filename_owned = filename.to_string();
        let sse_tx_clone = sse_tx.clone();
        let forwarder = tokio::spawn(async move {
            // First tick that arrives carries `total`; we emit the
            // `start` event from it (we don't know the total until
            // chunks are computed inside the blocking pass).
            let mut sent_start = false;
            while let Some((current, total)) = prog_rx.recv().await {
                if !sent_start {
                    let _ = sse_tx_clone
                        .send(Ok(Event::default().data(
                            json!({
                                "type": "pii_redact_start",
                                "filename": filename_owned,
                                "total": total,
                            })
                            .to_string(),
                        )))
                        .await;
                    sent_start = true;
                }
                let _ = sse_tx_clone
                    .send(Ok(Event::default().data(
                        json!({
                            "type": "pii_redact_progress",
                            "filename": filename_owned,
                            "current": current,
                            "total": total,
                        })
                        .to_string(),
                    )))
                    .await;
            }
            // Final done event whatever the outcome — even if no
            // progress arrived (e.g. spawn_blocking failed before
            // the first chunk), so the UI doesn't leave a dangling
            // "loading" step.
            let _ = sse_tx_clone
                .send(Ok(Event::default().data(
                    json!({
                        "type": "pii_redact_done",
                        "filename": filename_owned,
                    })
                    .to_string(),
                )))
                .await;
        });

        let progress_cb: crate::ner::ProgressFn = std::sync::Arc::new(
            move |current: usize, total: usize| {
                // try_send so a slow consumer never blocks the
                // blocking worker; dropped progress ticks aren't
                // critical — the UI smooths over them.
                let _ = prog_tx.try_send((current, total));
            },
        );
        let result = crate::ner::mask_pii(&text, None, Some(progress_cb)).await;
        // Drop the closure (and with it `prog_tx`) so the forwarder
        // sees EOF and emits the `done` event.
        let _ = forwarder.await;

        match result {
            Ok(masked) => {
                tracing::info!(
                    "[chat] PII redaction applied to {filename}: {} → {} chars",
                    text.len(),
                    masked.len()
                );
                // Persist for the next chat turn that references the
                // same document. Failure here is non-fatal — the user
                // still gets a correctly anonymised payload this turn;
                // only the *next* turn pays the inference cost again.
                if let Ok(storage) = crate::storage::make_storage() {
                    if let Err(e) = storage
                        .put(&cache_key, masked.as_bytes(), "text/plain")
                        .await
                    {
                        tracing::warn!(
                            "[chat] failed to write PII cache for {filename} ({cache_key}): {e:#}"
                        );
                    } else {
                        tracing::info!(
                            "[chat] PII cache stored for {filename} → {cache_key}"
                        );
                    }
                }
                return masked;
            }
            Err(e) => {
                tracing::warn!(
                    "[chat] PII redaction failed for {filename}: {e:#} — \
                     sending ORIGINAL text to LLM"
                );
            }
        }
    }
    #[cfg(not(feature = "ner-pii"))]
    {
        tracing::warn!(
            "[chat] {filename} flagged for PII protection but `ner-pii` feature \
             is not compiled — sending ORIGINAL text to LLM"
        );
    }
    text
}

/// Mike's original legal-assistant system prompt, adapted from upstream
/// (willchen96/mike, `backend/src/lib/chatTools.ts` SYSTEM_PROMPT).
const MRUST_SYSTEM_PROMPT: &str = r#"You are Mike, an AI legal assistant that helps lawyers and legal professionals analyze documents, answer legal questions, and draft legal documents.

RESPONSE STYLE:
- Always respond in the same language as the user's last message.
- Keep answers concise and well-structured (short paragraphs and/or bullets).
- Do not repeat the same filename multiple times in a row.
- When referring to provided documents, list each filename at most once.
- Avoid verbose meta-reasoning or restating the whole workflow unless explicitly requested.

DOCUMENT CITATION INSTRUCTIONS:
When you reference specific content from an attached or project document, place a marker [c1], [c2], [c3], etc. inline in your prose at the point of reference. The marker ALWAYS begins with the lowercase letter "c" (for "chat document") followed by a sequential number. NEVER write a bare bracketed number like [1] — without the "c" prefix it is read as ordinary text (a clause or page number), not a citation, and no source pill is rendered.

After your complete response, append a <CITATIONS> block containing a JSON array with one entry per marker:

<CITATIONS>
[
  {"ref": "c1", "doc_id": "doc-1", "page": 3, "quote": "exact verbatim text from the document"},
  {"ref": "c2", "doc_id": "doc-2", "page": "41-42", "quote": "Section 4.2 describes the procedure [[PAGE_BREAK]] in all material respects."}
]
</CITATIONS>

CRITICAL: "ref" MUST be the exact marker text you wrote in prose, without the brackets — the marker [c1] pairs with {"ref": "c1", ...}, [c2] with {"ref": "c2", ...}, and so on. "ref" is a string, NOT a page number, footnote number, section number, or any other number printed inside the document. Assign refs as "c1", "c2", "c3", ... in the order citations first appear in your prose. Never use a page number or a document's own numbering as the marker. Every [cN] you write in prose MUST have a matching {"ref": "cN", ...} entry in the JSON block, and every entry MUST have a [cN] marker somewhere in the prose.

Rules:
- Only cite text that appears verbatim in the provided documents
- In every <CITATIONS> entry, "doc_id" MUST be the exact chat-local document label you were given (for example "doc-1"). Never use a filename, document UUID, or any other identifier in "doc_id". "doc_id" is separate from "ref": "ref" is the prose marker ("c1", "c2", ...) and "doc_id" is the document handle ("doc-1", "doc-2", ...) — they are not interchangeable
- Keep quotes short (ideally <= 25 words) and narrowly scoped to the specific claim. Don't reuse one quote to support multiple different claims — give each its own citation
- "page" refers to the sequential [Page N] marker in the text you were given (1-indexed from the first page). IGNORE any page numbers printed inside the document itself (footers, roman numerals, etc.)
- For a single-page quote, set "page" to an integer. If a quote is one continuous sentence that spans two pages, set "page" to "N-M" and insert [[PAGE_BREAK]] in the quote at the page break. Otherwise, use separate citations for text on different pages
- Put the <CITATIONS> block at the very end of the response. Omit it entirely if there are no citations
- DO NOT write free-form references like "[doc-id: <uuid>, page N]", "[doc-id: doc-0, page 1]", "(see doc-0, p. 3)", or any other ad-hoc bracketed format in your prose. The ONLY recognised inline marker is "[cN]" (paired with a "ref": "cN" entry in the <CITATIONS> block). Free-form references render as plain text — the user cannot click them

DOCX GENERATION:
If asked to draft or generate a document, use the generate_docx tool to produce a downloadable Word document. Always use this tool rather than just displaying the document content inline when the user asks for a document to be created.
If the user follows up on a document you just generated and asks for changes (e.g. "make section 3 longer", "add a termination clause", "change the parties"), default to calling edit_document on that newly generated document — do NOT call generate_docx again to regenerate the whole document. Only fall back to generate_docx if the user explicitly asks for a brand-new document or the change is so sweeping that an edit would not be coherent.
After calling generate_docx, do NOT include any download links, URLs, or markdown links to the document in your prose response — the download card is presented automatically by the UI.
After calling generate_docx, you MUST call read_document on the returned doc_id before writing your prose response. Base your description on the generated document's actual text, not on memory of what you intended to generate.
Your prose response MUST include a short description of the generated document: what it is, its structure (key sections/clauses), and — if the draft was informed by any provided source documents — which sources you drew from and how. Keep it concise (typically 3–8 sentences or a short bulleted list). Refer to the document by filename, never by a download link.
When the description makes factual claims about the contents of the newly generated document, cite the generated document with [cN] markers and a <CITATIONS> block exactly as specified in the DOCUMENT CITATION INSTRUCTIONS above. If you also make factual claims about provided source documents, cite those source documents separately. Omit the <CITATIONS> block if the description makes no such claims.
Heading hierarchy: always use Heading 1 before introducing Heading 2, Heading 2 before Heading 3, and so on. Never skip levels.
Numbering: all numbering MUST start from 1, never 0. Never duplicate the numbering prefix in heading text — pass "Introduction", never "1. Introduction".
Contracts: when generating a contract or agreement, always include a signatures block at the very end of the document on its own page, with a signature line for each party (party name + "By:", "Name:", "Title:", "Date:"). Contract preambles (recitals, "WHEREAS" clauses, parties block) must NOT be numbered.

SPREADSHEET / EXCEL GENERATION:
When the user asks for an Excel file, a spreadsheet, an .xlsx, or to export tabular data to Excel, use the generate_xlsx tool. Pass the column labels in `headers` and one array of cell strings per row in `rows`. If you have just shown the user a Markdown table, reuse exactly those columns and rows. NEVER reply that you cannot create Excel files — generate_xlsx is available for exactly this. After calling generate_xlsx, do NOT include any download link or URL in your prose; the download card is presented automatically by the UI. Briefly state what the spreadsheet contains (sheet, number of rows/columns).

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
When a user message begins with a [Template: <title> (id: <id>)] marker, the user has selected a DOCX template and you MUST produce a Word document using it. Immediately call describe_docx_template with that exact id to load the authoring contract (layout, section skeleton, required metadata, per-field guidance). Then collect the needed [PLACEHOLDER] values from the conversation, write the document body in Markdown following the section_skeleton, and finally call generate_docx(template_id=..., body="<your Markdown here>", metadata={...}). The argument MUST be named `body` (a string of Markdown) — `body_md` does NOT exist and an empty / missing `body` makes the call fail. Do NOT consider the user's request fulfilled until generate_docx has succeeded and you have presented the download to the user. The Template marker can co-occur with a Workflow marker — if both are present, apply the workflow's instructions while still producing the docx as the closing step.

DOCUMENT NAMING IN PROSE:
The chat-local labels ("doc-0", "doc-1", "doc-N", ...) are internal handles for tool calls and citation JSON ONLY. NEVER write them in your prose response or in any text the user reads — not in body text, not in headings, not in lists, not in tool-activity descriptions. The user does not know what "doc-0" means and seeing it is jarring. When referring to a document in prose, always use its filename. The only places "doc-N" identifiers are allowed are inside tool-call arguments and inside the <CITATIONS> JSON block's "doc_id" field.

GENERAL GUIDANCE:
- Be precise and professional
- Cite the specific document and quote when making claims about document content
- When no documents are provided, answer based on your legal knowledge
- Do not fabricate document content
- Do not use emojis in your responses
"#;

/// System-prompt block listing a project's indexed documents. They are
/// exposed to the assistant as `doc-N` labels (read on demand via
/// `read_document`) — NOT loaded inline, to keep the context small.
/// `base` offsets the labels past the inline-attached documents so the
/// two label ranges never collide.
fn build_project_docs_prompt(base: usize, docs: &[(String, String)]) -> String {
    if docs.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        "PROJECT DOCUMENTS — this chat belongs to a project and the \
         documents below are already part of it. They are available to \
         you right now: open one in full with the `read_document` tool \
         using its label, or search it with `find_in_document`. NEVER \
         tell the user to attach these — they are already attached. \
         When asked which documents the project has, list exactly \
         these:\n",
    );
    for (i, (_, filename)) in docs.iter().enumerate() {
        s.push_str(&format!("  - doc-{} : {}\n", base + i, filename));
    }
    s
}

/// System-prompt block stating the project this chat belongs to. Without
/// it the assistant has no notion of the project's identity and, when
/// asked "what is the project name?", guesses it from an attached
/// document's filename or title.
fn build_project_context_prompt(name: &str, domain: &str) -> String {
    format!(
        "PROJECT CONTEXT — this chat belongs to a project. The project's \
         name is exactly \"{name}\" and its professional domain is \
         \"{domain}\". Whenever the user asks about \"the project\" — its \
         name, subject, or scope — this is what they mean. NEVER infer or \
         guess the project name from a document's filename, title, or \
         contents: the authoritative project name is \"{name}\"."
    )
}

fn build_doc_system_prompt(docs: &[DocPayload]) -> String {
    let with_text: Vec<&DocPayload> = docs.iter().filter(|d| d.text.is_some()).collect();
    let with_imgs: Vec<&DocPayload> = docs.iter().filter(|d| !d.images.is_empty()).collect();
    if with_text.is_empty() && with_imgs.is_empty() { return String::new(); }

    // Use Mike's chat-local doc-N labels so the citation system works.
    // Labels are 1-indexed (`doc-1`, `doc-2`, …) — the previous 0-indexed
    // scheme produced an off-by-one bug when filename numbering was
    // 1-based (e.g. CARTELLA_TEST_001..010): the model would refer to
    // "the 10th doc" as `doc-10` instead of `doc-9` and `read_document`
    // returned "not found". 1-indexed labels match human counting and
    // typical filename conventions.
    let mut s = String::from(
        "The user has attached the following documents. Use them to answer the question. \
         Cite the document name when relevant. The 'doc-N' label is for use in <CITATIONS> JSON only — \
         in prose, refer to documents by their filename.\n\n",
    );
    for (idx, d) in with_text.iter().enumerate() {
        s.push_str(&format!(
            "=== {label} (filename: {fname}) ===\n{body}\n\n",
            label = format!("doc-{}", idx + 1),
            fname = d.filename,
            body = d.text.as_deref().unwrap_or("")
        ));
    }
    let img_offset = with_text.len();
    for (i, d) in with_imgs.iter().enumerate() {
        s.push_str(&format!(
            "=== {label} (filename: {fname}, rendered as {n} page image(s) attached below) ===\n\n",
            label = format!("doc-{}", img_offset + i + 1),
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
    // We ALSO drop any chunk whose source document is flagged
    // `pii_protected = 1` (migration 0028). KB chunks come from the
    // raw indexed text, not the redacted PII cache — leaving them in
    // would re-expose the very entities the inline-attached path is
    // careful to mask. The user still gets the document content via
    // the parallel `load_attached_docs` path, which serves the
    // anonymised cache from `cache/pii/<doc_id>.txt`. Citations
    // referencing those passages move with the doc, so the answer
    // can still ground itself on the redacted version.
    let mut out: Vec<RetrievedKbEntry> = Vec::new();
    let mut g_idx = 0u32;
    let mut p_idx = 0u32;
    for c in chunks.into_iter().filter(|c| c.distance <= KB_DISTANCE_THRESHOLD) {
        // Per-doc PII-protection lookup. Cheap because the chunk batch
        // typically points to at most a handful of distinct documents,
        // and SQLite has a 100 ns hot-cache lookup.
        let prot: Option<(i64,)> = sqlx::query_as(
            "SELECT pii_protected FROM documents WHERE id = ?",
        )
        .bind(&c.document_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
        if prot.map(|(p,)| p != 0).unwrap_or(false) {
            tracing::info!(
                "[rag] dropping KB chunk from PII-protected doc_id={} \
                 (chunk_index={}, source={}) — content available via \
                 redacted cache instead",
                c.document_id,
                c.chunk_index,
                c.source_path,
            );
            continue;
        }

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
           3. In the <CITATIONS> entry, set BOTH \"ref\" and \"doc_id\" \
              to the EXACT tag you used inline (\"g1\", \"g2\", \"p1\", \
              etc.) — NOT a bare number, NOT \"doc-0\", NOT a filename. \
              \"ref\" must equal the marker text in your prose.\n\
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
         [\n  {\"ref\": \"g1\", \"doc_id\": \"g1\", \"quote\": \"...\"}\n]\n\
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
/// Compact a string to its ASCII-alphanumeric, lower-cased projection.
/// Counterpart of `frontend/src/lib/utils/highlight.ts::onlyLetters`
/// (the frontend version additionally NFD-normalises to handle the
/// case where the document on disk and the persisted quote use
/// different accent encodings; server-side both come from the same
/// retrieval pipeline so we skip that step). Used by the citation
/// validator below to decide whether the model's emitted quote is a
/// substring of the chunk text we actually retrieved — if not, the
/// quote is a hallucination and we replace it with the chunk's real
/// opening before persisting the citation.
fn letters_only(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

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

/// One inline `[doc-id: <handle>, page <N>]`-style reference found in
/// the assistant's prose. `handle` is either a `doc-N` chat-local label
/// or a 36-char UUID; `page` is the digit (or `N-M` range) the model
/// emitted (may be `None` for a `[doc-id: <handle>]` without page).
#[derive(Debug, Clone, PartialEq, Eq)]
struct InlineDocIdRef {
    start: usize,
    end: usize,
    handle: String,
    page: Option<String>,
}

/// Scan `text` for every `[doc-id: <handle>[, page[s] <N|N-M>]]`
/// occurrence the model emits as a free-form citation when it ignores
/// the `[cN]` + `<CITATIONS>` contract (most common with verbose
/// `generate_docx` follow-up descriptions). Returns the matches in
/// order of appearance — the caller assigns sequential `cN` refs and
/// rewrites the prose. Tolerant of variable whitespace, `page` vs
/// `pages`, capital `Doc-ID:` / `DOC-ID:`, and missing-page form.
fn extract_inline_docid_refs(text: &str) -> Vec<InlineDocIdRef> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let lower = text.to_ascii_lowercase();
    let needle = "[doc-id:";
    let mut search_from = 0;
    while let Some(off) = lower[search_from..].find(needle) {
        let start = search_from + off;
        let after_prefix = start + needle.len();
        let mut i = after_prefix;
        // skip whitespace after the colon
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        // read the handle: UUID hex+dash characters or doc-\d+
        let handle_start = i;
        while i < bytes.len() {
            let c = bytes[i] as char;
            if c.is_ascii_alphanumeric() || c == '-' {
                i += 1;
            } else {
                break;
            }
        }
        let handle = text[handle_start..i].to_string();
        if handle.is_empty() {
            search_from = after_prefix;
            continue;
        }
        // optional `, page[s] N` clause
        let mut page: Option<String> = None;
        let mut j = i;
        while j < bytes.len() && (bytes[j] as char).is_whitespace() {
            j += 1;
        }
        if j < bytes.len() && bytes[j] == b',' {
            j += 1;
            while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                j += 1;
            }
            // accept `page` or `pages`, case-insensitive
            let rest_lower = &lower[j..];
            let page_word = if rest_lower.starts_with("pages") {
                Some("pages")
            } else if rest_lower.starts_with("page") {
                Some("page")
            } else {
                None
            };
            if let Some(w) = page_word {
                j += w.len();
                while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                    j += 1;
                }
                let digits_start = j;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > digits_start {
                    // optional `-N` range
                    let mut k = j;
                    if k < bytes.len() && bytes[k] == b'-' {
                        k += 1;
                        let r = k;
                        while k < bytes.len() && bytes[k].is_ascii_digit() {
                            k += 1;
                        }
                        if k > r {
                            j = k;
                        }
                    }
                    page = Some(text[digits_start..j].to_string());
                    while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                        j += 1;
                    }
                }
            }
        }
        if j < bytes.len() && bytes[j] == b']' {
            out.push(InlineDocIdRef {
                start,
                end: j + 1,
                handle,
                page,
            });
            search_from = j + 1;
        } else {
            // Not a well-formed marker — keep scanning from after the colon.
            search_from = after_prefix;
        }
    }
    out
}

/// Rewrite an assistant response that cites attached documents through
/// the free-form `[doc-id: <handle>, page <N>]` pattern into the
/// canonical `[cN]` markers + `<CITATIONS>` block format. `resolve`
/// returns `Some((document_uuid, filename))` for handles that point to
/// a real document the user can access — handles that resolve to
/// `None` are left untouched in the prose (they continue to render as
/// plain text, which is the safest fallback).
///
/// Returns `Some((rewritten_text, citations_array))` when at least one
/// reference was successfully rewritten; `None` means the text had no
/// such references (or none resolved). Two references with the same
/// `(uuid, page)` share a `cN` ref so the `<CITATIONS>` block stays
/// compact.
/// Second-shape scanner: the model also writes free-form citations in
/// parenthesised prose using the Italian convention
/// `(... doc-N, pag. <N>[-<M>])`, with no surrounding `[doc-id: …]`
/// brackets and "pag" instead of "page". Observed in the wild on the
/// `Inventario beni assicurati` workflow's docx-description follow-up.
///
/// Only the `doc-N, pag X` substring is captured — the prose context
/// (e.g. `(Polizza n. 449435502/39,`) stays untouched, so the rewriter
/// can fold the marker into a `[cN]` pill without eating meaningful
/// content from the user's view.
///
/// Recognises: `doc-N`, optional comma + whitespace, then a page word
/// (`pag`, `pag.`, `pagina`, `pagine`, `page`, `pages`, `p.`), then
/// digits with optional `-`/`–` range.
fn extract_inline_paren_doc_refs(text: &str) -> Vec<InlineDocIdRef> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let lower = text.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(off) = lower[search_from..].find("doc-") {
        let start = search_from + off;
        let after_doc = start + 4;
        // Read the digit suffix: `doc-1`, `doc-12`, …
        let mut k = after_doc;
        while k < bytes.len() && bytes[k].is_ascii_digit() {
            k += 1;
        }
        if k == after_doc {
            search_from = after_doc;
            continue;
        }
        let handle_end = k;

        // Optional `, ` between handle and the page word.
        let mut j = handle_end;
        if j < bytes.len() && bytes[j] == b',' {
            j += 1;
        }
        while j < bytes.len() && (bytes[j] as char).is_whitespace() {
            j += 1;
        }

        // Page word, case-insensitive. Order matters: longer prefixes
        // before shorter ones so `pagine` isn't truncated to `pag`.
        let rest = &lower[j..];
        let page_word_len = if rest.starts_with("pagine") { 6 }
            else if rest.starts_with("pagina") { 6 }
            else if rest.starts_with("pages") { 5 }
            else if rest.starts_with("pag.") { 4 }
            else if rest.starts_with("page") { 4 }
            else if rest.starts_with("pag") { 3 }
            else if rest.starts_with("p.") { 2 }
            else { 0 };
        if page_word_len == 0 {
            search_from = handle_end;
            continue;
        }
        j += page_word_len;
        while j < bytes.len() && (bytes[j] as char).is_whitespace() {
            j += 1;
        }

        // Digits + optional range `-N` or `–N` (em-dash).
        let page_digits_start = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j == page_digits_start {
            search_from = handle_end;
            continue;
        }
        let mut end = j;
        if end < bytes.len() {
            let c = bytes[end] as char;
            let dash_len = if c == '-' {
                Some(1usize)
            } else if text[end..].starts_with('–') {
                Some('–'.len_utf8())
            } else {
                None
            };
            if let Some(d) = dash_len {
                let after_dash = end + d;
                let mut r = after_dash;
                while r < bytes.len() && bytes[r].is_ascii_digit() {
                    r += 1;
                }
                if r > after_dash {
                    end = r;
                }
            }
        }

        out.push(InlineDocIdRef {
            start,
            end,
            handle: text[start..handle_end].to_string(),
            page: Some(text[page_digits_start..end].to_string()),
        });
        search_from = end;
    }
    out
}

fn rewrite_inline_docid_citations<F>(text: &str, mut resolve: F) -> Option<(String, Value)>
where
    F: FnMut(&str) -> Option<(String, String)>,
{
    // Both shapes feed the same rewriter. The bracketed `[doc-id: …]`
    // form and the parenthesised `... doc-N, pag …` form are gathered
    // into a single list, then sorted by start offset so substitution
    // happens in left-to-right document order — and the rewrite loop
    // applies them in reverse so earlier byte offsets stay valid.
    let mut refs = extract_inline_docid_refs(text);
    refs.extend(extract_inline_paren_doc_refs(text));
    refs.sort_by_key(|r| r.start);
    // Drop overlapping captures (e.g. an inner paren scan landing on
    // bytes already inside an outer `[doc-id: …]` bracket capture).
    let mut deduped: Vec<InlineDocIdRef> = Vec::with_capacity(refs.len());
    for r in refs {
        if let Some(prev) = deduped.last() {
            if r.start < prev.end {
                continue;
            }
        }
        deduped.push(r);
    }
    let refs = deduped;
    if refs.is_empty() {
        return None;
    }
    let mut citations: Vec<Value> = Vec::new();
    let mut key_to_ref: HashMap<(String, Option<String>), String> = HashMap::new();
    let mut rewrites: Vec<(usize, usize, String)> = Vec::new();
    for r in &refs {
        let Some((uuid, filename)) = resolve(&r.handle) else {
            continue; // unresolved → keep the original text as-is
        };
        let key = (uuid.clone(), r.page.clone());
        let ref_id = match key_to_ref.get(&key) {
            Some(id) => id.clone(),
            None => {
                let id = format!("c{}", citations.len() + 1);
                let mut obj = serde_json::Map::new();
                obj.insert("ref".into(), Value::String(id.clone()));
                // Keep both `doc_id` and `document_id` — the downstream
                // citation enrichment looks at both names.
                obj.insert("doc_id".into(), Value::String(uuid.clone()));
                obj.insert("document_id".into(), Value::String(uuid.clone()));
                obj.insert("filename".into(), Value::String(filename.clone()));
                if let Some(p) = &r.page {
                    if let Ok(n) = p.parse::<i64>() {
                        obj.insert("page".into(), Value::Number(n.into()));
                    } else {
                        obj.insert("page".into(), Value::String(p.clone()));
                    }
                }
                obj.insert("source".into(), Value::String("attached".to_string()));
                citations.push(Value::Object(obj));
                key_to_ref.insert(key, id.clone());
                id
            }
        };
        rewrites.push((r.start, r.end, ref_id));
    }
    if citations.is_empty() {
        return None;
    }
    // Rewrite in REVERSE order so earlier byte offsets stay valid as
    // later replacements change the string length.
    let mut out = text.to_string();
    for (start, end, ref_id) in rewrites.iter().rev() {
        out.replace_range(*start..*end, &format!("[{ref_id}]"));
    }
    Some((out, Value::Array(citations)))
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
        .route("/{id}/documents", get(get_chat_documents))
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

    // Collect document_ids from message-level attachments. Also
    // record which attachments the user flagged with PII protection
    // (the per-file checkbox in the chat composer). When the
    // `ner-pii` feature is built in we'll run those docs' text
    // through `crate::ner::mask_pii` before stuffing it into the
    // LLM payload.
    let mut doc_ids: Vec<String> = Vec::new();
    let mut pii_protected_ids: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    if let Some(arr) = body.get("messages").and_then(|v| v.as_array()) {
        for m in arr {
            if let Some(files) = m.get("files").and_then(|v| v.as_array()) {
                for f in files {
                    if let Some(id) = f.get("document_id").and_then(|v| v.as_str()) {
                        if !doc_ids.iter().any(|x| x == id) {
                            doc_ids.push(id.to_string());
                        }
                        let pii = f
                            .get("pii_protected")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        if pii {
                            pii_protected_ids.insert(id.to_string());
                        }
                    }
                }
            }
        }
    }
    tracing::info!(
        "[chat] payload parsed — attachments={} pii_protected={} (ner-pii built-in: {})",
        doc_ids.len(),
        pii_protected_ids.len(),
        cfg!(feature = "ner-pii"),
    );

    // Persist the per-document PII-protection flag (migration 0028).
    // Before this column existed, the flag lived only on the current
    // request payload — fine for the upload turn, but follow-up
    // text-only turns re-fetched the document from chat history
    // without the flag and sent the raw text to the LLM. With the
    // column, once a user has opted-in on a file it stays protected
    // for every subsequent turn of every chat that references it.
    for id in &pii_protected_ids {
        let _ = sqlx::query(
            "UPDATE documents SET pii_protected = 1 WHERE id = ? AND user_id = ?",
        )
        .bind(id)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await;
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
    // would drop the document from context and — worse — its `[cN]`
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

    // Project documents — when this chat belongs to a project, the
    // project's indexed documents are made available to the assistant
    // as labelled, read-on-demand entries (so it never tells the user
    // to attach documents that are already in the project). They are
    // NOT appended to `doc_ids`: that would load every project doc
    // inline on every turn. Instead they get `doc-N` labels after the
    // inline attachments and an inventory line in the system prompt.
    let chat_project_id: Option<String> = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT project_id FROM chats WHERE id = ?",
    )
    .bind(&chat_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .and_then(|(p,)| p);
    let project_documents: Vec<(String, String)> = if let Some(pid) = &chat_project_id {
        sqlx::query_as::<_, (String, String)>(
            "SELECT id, filename FROM documents \
             WHERE project_id = ? AND user_id = ? AND status = 'ready' \
             ORDER BY created_at ASC",
        )
        .bind(pid)
        .bind(&auth.user_id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|(id, _)| !doc_ids.contains(id))
        .collect()
    } else {
        Vec::new()
    };

    // Project identity (name + domain) — surfaced in the system prompt
    // so the assistant answers "what is the project name?" with the
    // actual project, not a filename guessed from an attached document.
    let project_meta: Option<(String, String)> = if let Some(pid) = &chat_project_id {
        sqlx::query_as::<_, (String, String)>(
            "SELECT name, domain FROM projects WHERE id = ? AND user_id = ?",
        )
        .bind(pid)
        .bind(&auth.user_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
    } else {
        None
    };

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
        .unwrap_or_else(|| "gemini-3.5-flash".to_string());

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

    // SSE channel + spawn the whole pipeline as a background task so the
    // HTTP response can start streaming immediately. Before this change
    // load_attached_docs (PDF text extraction + PII redaction) and the
    // history summarizer call ran INSIDE the handler — `Sse::new` only
    // returned after all that finished, so the browser saw no body bytes
    // for the multi-minute window during which doc_extract / pii_redact
    // events were being emitted. They just buffered in the channel and
    // flushed all at once when the response finally went out. Moving the
    // setup work inside the spawn lets the client connect as soon as
    // the handler returns and observe each event the moment it fires.
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);
    let state_clone = state.clone();
    let chat_id_clone = chat_id.clone();

    tokio::spawn(async move {
        if is_new_chat {
            let chat_id_event = json!({ "type": "chat_id", "chatId": &chat_id_clone });
            let _ = tx
                .send(Ok(Event::default().data(chat_id_event.to_string())))
                .await;
        }

        // PII redaction inside load_attached_docs emits per-chunk
        // progress events on the same channel that carries the rest
        // of the stream (rendered text deltas, citations, tool calls).
        let tx_for_redact = tx.clone();

        // Discover MCP, load attached docs, retrieve KB chunks, and pull
        // a library inventory in parallel. The inventory is what tells the
        // model "the user has the GDPR and AI Act in their indexed library"
        // even when the user's question doesn't surface those documents
        // via semantic match — without it, the model defaults to "I don't
        // have access to your synced documents."
        let (attached_docs, mcp_servers, kb_chunks, library_inventory) = tokio::join!(
            load_attached_docs(
                &state_clone,
                &auth.user_id,
                &doc_ids,
                vision_ok,
                &pii_protected_ids,
                &tx_for_redact,
            ),
            discover_mcp_for_user(&state_clone, &auth.user_id),
            retrieve_kb_chunks(&state_clone, &auth.user_id, &chat_id_clone, &last_user_query, kb_top_k),
            list_indexed_corpus_docs(&state_clone, &auth.user_id),
        );

        // Compose: Mike base + library inventory + KB excerpts + attached
        // full-text + MCP. Library inventory comes near the top so the
        // model orients itself before the semantic-retrieval block —
        // which may have missed documents the user has but didn't trigger.
        let inventory_prompt = build_library_inventory_prompt(&library_inventory);
        let mcp_prompt = build_mcp_system_prompt(&mcp_servers);
        let docs_prompt = build_doc_system_prompt(&attached_docs);
        let kb_prompt = build_kb_system_prompt(&kb_chunks);
        // Stable prefix — identical across the turns of a chat. Sent as a
        // cacheable block (see StreamParams::system_prompt). The per-query
        // KB retrieval is deliberately NOT joined here: it changes every
        // turn and would invalidate the cache, so it travels separately as
        // the volatile tail.
        // Domain-aware prologue (see crate::presets::system_prompt).
        // The domain resolves from the chat's project (if any) →
        // user_settings.default_domain → "others". The locale comes
        // from user_settings.locale → "it" (MikeRust's primary
        // language). Prepended FIRST so it sets the role before the
        // generic Mike tool-use / citation rules.
        let domain_locale: (String, String) = {
            let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
                "SELECT locale, default_domain FROM user_settings WHERE user_id = ?",
            )
            .bind(&auth.user_id)
            .fetch_optional(&state_clone.db)
            .await
            .ok()
            .flatten();
            let (locale_opt, default_domain_opt) = row.unwrap_or((None, None));
            let locale = locale_opt
                .as_deref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .unwrap_or("it")
                .to_string();
            // Project domain wins over user default — the project is
            // the more specific scope and almost always carries the
            // authoritative vertical for everything that happens in
            // its chats.
            let domain = project_meta
                .as_ref()
                .map(|(_, pdomain)| pdomain.clone())
                .or(default_domain_opt)
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "others".to_string());
            (locale, domain)
        };
        let domain_prologue = crate::presets::system_prompt::assemble_prologue(
            &domain_locale.0,
            &domain_locale.1,
        );

        let mut sections: Vec<String> = Vec::new();
        sections.push(domain_prologue);
        sections.push(MRUST_SYSTEM_PROMPT.trim().to_string());
        if !inventory_prompt.is_empty() {
            sections.push(inventory_prompt);
        }
        if !docs_prompt.is_empty() {
            sections.push(docs_prompt);
        }
        if let Some((pname, pdomain)) = &project_meta {
            sections.push(build_project_context_prompt(pname, pdomain));
        }
        let project_docs_prompt =
            build_project_docs_prompt(doc_ids.len(), &project_documents);
        if !project_docs_prompt.is_empty() {
            sections.push(project_docs_prompt);
        }
        if !mcp_prompt.is_empty() {
            sections.push(mcp_prompt);
        }
        let system_prompt = sections.join("\n\n---\n\n");
        // Volatile tail — knowledge-base hits for *this* query.
        let system_volatile = kb_prompt;
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
            "[chat] stream_chat_root: chat_id={chat_id_clone}, model={raw_model}, vision_ok={vision_ok}, local_config_present={}, docs={}, mcp_servers={}, kb_chunks={} (sys_prompt={} chars cacheable + {} volatile, images={})",
            local_config.is_some(),
            attached_docs.len(),
            mcp_servers.len(),
            kb_chunks.len(),
            system_prompt.len(),
            system_volatile.len(),
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

        // Map chat-local labels (`doc-1`, `doc-2`, …) to real document UUIDs so
        // builtin tools (read_document, find_in_document) can resolve them.
        // 1-indexed (see build_doc_system_prompt above for the off-by-one
        // story that motivated the switch).
        let mut doc_label_map: HashMap<String, String> = HashMap::new();
        for (idx, doc_id) in doc_ids.iter().enumerate() {
            doc_label_map.insert(format!("doc-{}", idx + 1), doc_id.clone());
        }
        // Project documents continue the label sequence after the inline
        // attachments so read_document / find_in_document resolve them too.
        for (i, (id, _)) in project_documents.iter().enumerate() {
            doc_label_map.insert(format!("doc-{}", doc_ids.len() + i + 1), id.clone());
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

        // Compress older turns once the whole prompt — system prefix
        // (instructions + attached-document text), the volatile KB block and
        // the conversation history — fills past 80% of the model's context
        // window. The system prefix is measured here and passed in, because
        // in a document-heavy chat it dwarfs the turns and a history-only
        // trigger would never fire. Failing-open: if the summarizer LLM call
        // errors, the original messages are returned unchanged.
        let summarizer_creds = llm::summarize::SummarizerCreds {
            local_config: local_config.clone(),
            claude_api_key: claude_key.clone(),
            gemini_api_key: gemini_key.clone(),
            gemini_region: gemini_region.clone(),
        };
        let system_overhead = llm::summarize::estimate_tokens(&system_prompt)
            + llm::summarize::estimate_tokens(&system_volatile);
        let messages = llm::summarize::maybe_compress_history(
            messages,
            &raw_model,
            &summarizer_creds,
            system_overhead,
        )
        .await;

        // Move retrieved KB chunks into the post-stream citation parser
        // so model-emitted [g1]/[p1] tags can be mapped back to the
        // source path + chunk index.
        let kb_chunks_for_citations = kb_chunks.clone();

        // Bumped from 5 in v0.3.2 after a user hit the cap on a
        // medical-anamnesis workflow with 10 attached PDFs: the model
        // legitimately needs to `read_document` (or `find_in_document`)
        // several times to compile a multi-source summary, and each
        // tool call burns one iteration. 5 was an early-debug ceiling
        // tuned against single-doc reviews; 20 covers ten-doc-anamnesis
        // / due-diligence-document-pack flows while still bounding a
        // truly stuck loop (per-iteration cost is one LLM round-trip,
        // so 20 caps a runaway at ~20× the per-turn latency budget).
        const MAX_TOOL_ITERATIONS: u32 = 20;
        // How many times to nudge the model when it ends a turn with a
        // completely empty answer (no text, no tool call) — a flaky
        // behaviour seen mostly with Gemini right after a tool result.
        const MAX_EMPTY_ANSWER_RETRIES: u32 = 2;
        let mut empty_answer_retries: u32 = 0;
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
                system_volatile: system_volatile.clone(),
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
                            // Model reasoning / "thinking" — forwarded as
                            // its own SSE event so the UI can show it in a
                            // separate collapsible block rather than mixing
                            // it into the answer text. Not appended to
                            // `full_response` (it is not the answer).
                            Ok(StreamEvent::ReasoningDelta(text)) => {
                                let payload = json!({
                                    "type": "reasoning_delta", "text": text
                                });
                                if tx
                                    .send(Ok(Event::default().data(payload.to_string())))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Ok(StreamEvent::ReasoningEnd) => {
                                let payload = json!({ "type": "reasoning_done" });
                                let _ = tx
                                    .send(Ok(Event::default().data(payload.to_string())))
                                    .await;
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
                        // Empty final turn — no text and no further tool
                        // call. Seen mostly with Gemini right after a
                        // tool result (e.g. read_workflow), and it leaves
                        // the user with a blank reply. Nudge the model to
                        // actually produce its answer before giving up.
                        if iter_text.trim().is_empty()
                            && full_response.trim().is_empty()
                            && empty_answer_retries < MAX_EMPTY_ANSWER_RETRIES
                        {
                            empty_answer_retries += 1;
                            tracing::warn!(
                                "[chat] empty answer at iter {iteration}; \
                                 nudging model (retry {empty_answer_retries}/{MAX_EMPTY_ANSWER_RETRIES})"
                            );
                            current_messages.push(Message::user(
                                "Non hai prodotto alcuna risposta. Completa \
                                 ora la richiesta seguendo le istruzioni \
                                 date: se ti serve il contenuto di un \
                                 documento caricalo con read_document, poi \
                                 fornisci direttamente l'output richiesto.",
                            ));
                            continue;
                        }
                        // No more tools requested → final answer reached.
                        if full_response.trim().is_empty() && !errored {
                            // Retries exhausted (or none warranted) and
                            // still nothing: surface a visible note so the
                            // turn doesn't render as an empty bubble.
                            let note = "_(Il modello non ha prodotto una \
                                        risposta. Riprova a inviare il \
                                        messaggio.)_";
                            full_response.push_str(note);
                            let payload =
                                json!({ "type": "content_delta", "text": note });
                            let _ = tx
                                .send(Ok(Event::default().data(payload.to_string())))
                                .await;
                        }
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

                    // The model produced at least one tool call — proof of
                    // life. Reset the empty-answer retry counter so a future
                    // stall (e.g. blank turn AFTER a tool result later in
                    // the conversation) gets its own fresh nudges instead
                    // of inheriting a budget already burned earlier.
                    empty_answer_retries = 0;

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
                        // Tell the UI this tool finished, so its
                        // "running…" line resolves to a check right away.
                        // Without it the step stays spinning until the
                        // *next* tool starts — and on a docx generation
                        // that gap is the whole body-writing phase, so it
                        // looked stuck on `describe_docx_template`.
                        let _ = tx
                            .send(Ok(Event::default().data(
                                json!({
                                    "type": "tool_call_done",
                                    "name": call.name,
                                })
                                .to_string(),
                            )))
                            .await;
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
                        // Typed step events for the document / workflow
                        // builtin tools. Name-based (not shape-based):
                        // read_document also returns {doc_id,filename,…},
                        // so a shape match used to mis-fire a bogus
                        // download card for a plain read.
                        //   generate_docx    → doc_created (download card,
                        //                      persisted so it survives reload)
                        //   generate_xlsx    → doc_created (same card)
                        //   read_document    → doc_read
                        //   find_in_document → doc_find
                        //   read_workflow    → workflow_applied
                        // A future generator (pdf-export, …) adds its
                        // own arm here.
                        if let Ok(rv) = serde_json::from_str::<Value>(&result)
                            && rv.get("error").is_none()
                        {
                            let s = |k: &str| {
                                rv.get(k).and_then(|v| v.as_str()).unwrap_or("")
                            };
                            match call.name.as_str() {
                                "generate_docx" | "generate_xlsx" => {
                                    let doc_id = s("doc_id");
                                    let filename = s("filename");
                                    if !doc_id.is_empty() && !filename.is_empty() {
                                        let ev = json!({
                                            "type": "doc_created",
                                            "filename": filename,
                                            "download_url": format!("/document/{doc_id}/download"),
                                            "document_id": doc_id,
                                        });
                                        let _ = tx
                                            .send(Ok(Event::default().data(ev.to_string())))
                                            .await;
                                        // Persisted so the download card
                                        // re-renders when the chat reopens.
                                        persistent_events.push(json!({
                                            "type": "doc_created",
                                            "filename": filename,
                                            "download_url": format!("/document/{doc_id}/download"),
                                            "document_id": doc_id,
                                            "isStreaming": false,
                                        }));
                                    }
                                }
                                "read_document" => {
                                    let ev = json!({
                                        "type": "doc_read",
                                        "doc_id": s("doc_id"),
                                        "filename": s("filename"),
                                    });
                                    let _ = tx
                                        .send(Ok(Event::default().data(ev.to_string())))
                                        .await;
                                }
                                "find_in_document" => {
                                    let ev = json!({
                                        "type": "doc_find",
                                        "doc_id": s("doc_id"),
                                        "filename": s("filename"),
                                        "query": s("query"),
                                        "match_count": rv.get("match_count")
                                            .and_then(|v| v.as_u64())
                                            .unwrap_or(0),
                                    });
                                    let _ = tx
                                        .send(Ok(Event::default().data(ev.to_string())))
                                        .await;
                                }
                                "read_workflow" => {
                                    let ev = json!({
                                        "type": "workflow_applied",
                                        "workflow_id": s("workflow_id"),
                                        "title": s("title"),
                                    });
                                    let _ = tx
                                        .send(Ok(Event::default().data(ev.to_string())))
                                        .await;
                                }
                                _ => {}
                            }
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

        // Rewrite free-form `[doc-id: <handle>, page <N>]` references the
        // model occasionally writes (ignoring the `[cN]` + <CITATIONS>
        // contract — observed routinely on verbose generate_docx
        // descriptions) into the canonical `[cN]` markers and synthesize
        // the matching citations array. Done BEFORE persistence so the
        // stored body has the right markers and pills render on reload
        // too, and a `content_replace` SSE event swaps the live view.
        let mut prebuilt_citations: Option<Value> = None;
        if extract_citations_block(&full_response).is_none() {
            let inline_refs = extract_inline_docid_refs(&full_response);
            if !inline_refs.is_empty() {
                // Collect every distinct handle, resolve doc-N labels
                // locally, and validate raw UUIDs against the user's
                // documents in one batch query.
                let mut handles: Vec<String> =
                    inline_refs.iter().map(|r| r.handle.clone()).collect();
                handles.sort();
                handles.dedup();
                let mut uuids_to_validate: Vec<String> = Vec::new();
                for h in &handles {
                    if let Some(uuid) = doc_label_map.get(h) {
                        uuids_to_validate.push(uuid.clone());
                    } else if h.len() == 36
                        && h.chars().filter(|c| *c == '-').count() == 4
                    {
                        uuids_to_validate.push(h.clone());
                    }
                }
                uuids_to_validate.sort();
                uuids_to_validate.dedup();
                let mut filename_by_uuid: HashMap<String, String> = HashMap::new();
                if !uuids_to_validate.is_empty() {
                    let placeholders = std::iter::repeat("?")
                        .take(uuids_to_validate.len())
                        .collect::<Vec<_>>()
                        .join(",");
                    let q = format!(
                        "SELECT id, filename FROM documents \
                         WHERE user_id = ? AND id IN ({})",
                        placeholders
                    );
                    let mut query = sqlx::query_as::<_, (String, String)>(&q)
                        .bind(&auth.user_id);
                    for u in &uuids_to_validate {
                        query = query.bind(u);
                    }
                    if let Ok(rows) = query.fetch_all(&state_clone.db).await {
                        for (id, fname) in rows {
                            filename_by_uuid.insert(id, fname);
                        }
                    }
                }
                let mut handle_to_doc: HashMap<String, (String, String)> = HashMap::new();
                for h in &handles {
                    let real_uuid = doc_label_map
                        .get(h)
                        .cloned()
                        .unwrap_or_else(|| h.clone());
                    if let Some(filename) = filename_by_uuid.get(&real_uuid) {
                        handle_to_doc.insert(
                            h.clone(),
                            (real_uuid, filename.clone()),
                        );
                    }
                }
                if let Some((new_body, citations_array)) =
                    rewrite_inline_docid_citations(&full_response, |h| {
                        handle_to_doc.get(h).cloned()
                    })
                {
                    let n_refs = citations_array
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0);
                    tracing::info!(
                        "[chat] rewrote {n_refs} inline [doc-id: …] reference(s) to [cN] markers"
                    );
                    full_response = new_body;
                    prebuilt_citations = Some(citations_array);
                    // Live view: replace the message body wholesale so
                    // pills render immediately for this turn.
                    let payload = json!({
                        "type": "content_replace",
                        "text": full_response,
                    });
                    let _ = tx
                        .send(Ok(Event::default().data(payload.to_string())))
                        .await;
                }
            }
        }

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

        // Canonical-key index over the user's FULL corpus library. Catches
        // the case where the model copies a verbatim inventory line
        // (e.g. `[italian-legal] corte_costituzionale_1990_241`, with
        // bracket and whitespace) as `doc_id` instead of the [gN]/[pN]
        // tag — and where this turn produced no KB chunks at all, so
        // `corpus_ref_to_tag` above stays empty. The canonical form
        // strips every non-alphanumeric character and lowercases, so
        // any separator / case / bracket variant collapses to the same
        // key. Resolution sets `document_id` + `filename` directly and
        // marks the citation as a viewable attached document.
        let mut library_corpus_index: HashMap<String, (String, String)> = HashMap::new();
        if let Ok(rows) = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT id, filename, corpus_id, corpus_identifier FROM documents \
             WHERE user_id = ? AND corpus_id IS NOT NULL AND corpus_identifier IS NOT NULL",
        )
        .bind(&auth.user_id)
        .fetch_all(&state_clone.db)
        .await
        {
            for (doc_uuid, filename, corpus_id, ident) in rows {
                // Two canonical variants per row: the bare identifier
                // (model wrote just the ident) and the combined
                // corpus_id+ident (model copied the full inventory
                // prefix). Both collapse to alphanumeric-only,
                // lowercase under `canonical_corpus_key`.
                for key_source in [ident.clone(), format!("{corpus_id} {ident}")] {
                    let canon = canonical_corpus_key(&key_source);
                    if !canon.is_empty() {
                        library_corpus_index
                            .entry(canon)
                            .or_insert_with(|| (doc_uuid.clone(), filename.clone()));
                    }
                }
            }
            if !library_corpus_index.is_empty() {
                tracing::info!(
                    "[chat] built library corpus canonical index with {} entries",
                    library_corpus_index.len()
                );
            }
        }

        // Pre-fetch a `document_id → absolute local storage path` map
        // for this user's corpus docs. Used below to defensively remap
        // any KB chunk whose `source_path` is the upstream URL (older
        // `doc_chunks` rows, or any code path that stored the URL
        // instead of the cached file path) back to the local file
        // `/sync/kb-doc` can actually `std::fs::read`. The fix lives at
        // citation-build time, not as a DB migration, so it covers
        // both pre-existing rows and future regressions uniformly.
        let mut corpus_local_path_by_docid: HashMap<String, String> = HashMap::new();
        {
            let storage_root = std::path::PathBuf::from(
                std::env::var("STORAGE_PATH")
                    .unwrap_or_else(|_| "./data/storage".to_string()),
            );
            if let Ok(rows) = sqlx::query_as::<_, (String, Option<String>)>(
                "SELECT id, storage_path FROM documents \
                 WHERE user_id = ? AND corpus_id IS NOT NULL AND storage_path IS NOT NULL",
            )
            .bind(&auth.user_id)
            .fetch_all(&state_clone.db)
            .await
            {
                for (doc_uuid, sp_opt) in rows {
                    if let Some(sp) = sp_opt {
                        let abs = storage_root
                            .join(sp.replace('/', std::path::MAIN_SEPARATOR_STR));
                        corpus_local_path_by_docid
                            .insert(doc_uuid, abs.to_string_lossy().to_string());
                    }
                }
            }
        }

        // Resolution order: the inline `[doc-id: …]` rewriter wins if it
        // synthesised any citations earlier (the body is already rewritten
        // and has `[cN]` markers); otherwise parse a model-emitted block;
        // otherwise synthesise from inline `[gN]`/`[pN]` KB markers.
        let citations_json = prebuilt_citations.or_else(|| {
            extract_citations_block(&full_response).or_else(|| {
                synthesise_kb_citations_from_markers(&full_response, &kb_by_tag)
            })
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
                        // Hallucinated-quote safety net: validate the
                        // model's quote against the chunk text we
                        // actually retrieved. The frontend's highlight
                        // is letters-only-and-lower-cased; mirror that
                        // here. If the projection of the quote isn't a
                        // substring of the projection of the chunk
                        // text, the model invented the quote (most
                        // often by citing one of its own section
                        // headings instead of the source) — replace
                        // it with the chunk's real opening so the
                        // viewer at least lands on the cited passage.
                        if let Some(q) = obj.get("quote").and_then(|v| v.as_str()) {
                            let chunk_clean = strip_page_markers(&kb.text);
                            let needle = letters_only(q);
                            let haystack = letters_only(&chunk_clean);
                            if needle.len() >= 4 && !haystack.contains(&needle) {
                                let trimmed = chunk_clean.trim();
                                let cap = 200.min(trimmed.len());
                                let mut end = cap;
                                while end < trimmed.len()
                                    && !trimmed.is_char_boundary(end)
                                {
                                    end += 1;
                                }
                                let fallback = trimmed[..end].to_string();
                                tracing::warn!(
                                    "[chat] citation quote not found in chunk for tag {label:?} \
                                     (doc {:?}, chunk {}): model emitted {:?}; \
                                     substituting first {} chars of chunk text",
                                    kb.document_id,
                                    kb.chunk_index,
                                    q.chars().take(80).collect::<String>(),
                                    fallback.len()
                                );
                                obj.insert("quote".into(), Value::String(fallback));
                            }
                        }
                        obj.insert("source".into(), Value::String("kb".to_string()));
                        obj.insert("scope".into(), Value::String(kb.scope_label.to_string()));
                        // Remap URL-shaped source_path back to the local
                        // cache file. EUR-Lex (and any corpus that stored
                        // the upstream URL in older indexing runs) needs
                        // this — /sync/kb-doc does std::fs::read on the
                        // value and can't take a URL.
                        let mut path_value = kb.source_path.clone();
                        if path_value.starts_with("http://")
                            || path_value.starts_with("https://")
                        {
                            if let Some(local) =
                                corpus_local_path_by_docid.get(&kb.document_id)
                            {
                                tracing::info!(
                                    "[chat] remapping URL source_path → local storage path \
                                     for doc {:?} (was {:?})",
                                    kb.document_id,
                                    path_value
                                );
                                path_value = local.clone();
                            } else {
                                tracing::warn!(
                                    "[chat] citation source_path is a URL ({:?}) but no \
                                     local storage_path is registered under documents \
                                     for doc_id {:?} — viewer will 404",
                                    path_value,
                                    kb.document_id
                                );
                            }
                        }
                        obj.insert("path".into(), Value::String(path_value));
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
                        let mut uuid = id_by_label.get(label).cloned();
                        let mut filename = uuid
                            .as_ref()
                            .and_then(|u| name_by_id.get(u))
                            .cloned()
                            .unwrap_or_default();
                        // Last-resort: canonical-key match against the
                        // user's full corpus library. Catches the model
                        // copying an inventory line verbatim (bracket +
                        // space + identifier) as `doc_id` even when no
                        // KB chunks were retrieved this turn.
                        if uuid.is_none() {
                            let canon = canonical_corpus_key(label);
                            if !canon.is_empty()
                                && let Some((corp_uuid, corp_filename)) =
                                    library_corpus_index.get(&canon)
                            {
                                tracing::info!(
                                    "[chat] citation doc_id {:?} resolved to corpus document {:?} via canonical-key match",
                                    label,
                                    corp_filename
                                );
                                uuid = Some(corp_uuid.clone());
                                if filename.is_empty() {
                                    filename = corp_filename.clone();
                                }
                                // The model invented the doc_id from the
                                // inventory line — page (if any) is
                                // almost certainly hallucinated. Drop it
                                // so the viewer text-searches the quote
                                // instead of jumping to a fake page.
                                obj.remove("page");
                            }
                        }
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
/// Rewrite annotations whose `path` is the upstream URL of a corpus
/// document back to the local cache-file path the viewer can fetch via
/// `/sync/kb-doc`. Older indexing runs (pre eurlex.rs:462 fix) stored
/// the EUR-Lex URL as `doc_chunks.source_path`; that URL got persisted
/// into `messages.annotations[].path`. The hot fix at write time
/// remaps new citations, but persisted ones from old chats still carry
/// the URL — so we apply the same remap on read.
///
/// `corpus_local_path_by_docid` maps a `document_id` to the absolute
/// on-disk path of its cached binary (storage root joined with the
/// row's `storage_path`). Built once per `get_messages` call from
/// `documents` rows that belong to a corpus.
fn remap_url_annotation_paths(
    mut value: Value,
    corpus_local_path_by_docid: &HashMap<String, String>,
) -> Value {
    if corpus_local_path_by_docid.is_empty() {
        return value;
    }
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
        let path = obj.get("path").and_then(|v| v.as_str()).unwrap_or("");
        if !(path.starts_with("http://") || path.starts_with("https://")) {
            continue;
        }
        let doc_uuid = obj
            .get("document_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if doc_uuid.is_empty() {
            continue;
        }
        if let Some(local) = corpus_local_path_by_docid.get(doc_uuid) {
            tracing::info!(
                "[chat] get_messages: remapping URL path → local storage path for doc {:?}",
                doc_uuid
            );
            obj.insert("path".to_string(), Value::String(local.clone()));
        }
    }
    value
}

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

    // Mapping `document_id → absolute local path` for the user's corpus
    // docs. Used to rewrite annotations whose persisted `path` is the
    // upstream URL — old chats stored those before the eurlex.rs fix.
    let mut corpus_local_path_by_docid: HashMap<String, String> = HashMap::new();
    {
        let storage_root = std::path::PathBuf::from(
            std::env::var("STORAGE_PATH")
                .unwrap_or_else(|_| "./data/storage".to_string()),
        );
        if let Ok(rows) = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT id, storage_path FROM documents \
             WHERE user_id = ? AND corpus_id IS NOT NULL AND storage_path IS NOT NULL",
        )
        .bind(&auth.user_id)
        .fetch_all(&state.db)
        .await
        {
            for (doc_uuid, sp_opt) in rows {
                if let Some(sp) = sp_opt {
                    let abs = storage_root
                        .join(sp.replace('/', std::path::MAIN_SEPARATOR_STR));
                    corpus_local_path_by_docid
                        .insert(doc_uuid, abs.to_string_lossy().to_string());
                }
            }
        }
    }

    let rows: Vec<(
        String,         // id
        String,         // role
        Option<String>, // content
        String,         // created_at
        Option<String>, // annotations
        Option<String>, // events (assistant — persisted doc_created etc.)
    )> = sqlx::query_as(
        "SELECT id, role, content, created_at, annotations, events FROM messages \
         WHERE chat_id = ? ORDER BY created_at ASC",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let with_annot = rows
        .iter()
        .filter(|(_, role, _, _, ann, _)| role == "assistant" && ann.is_some())
        .count();
    let with_events = rows
        .iter()
        .filter(|(_, role, _, _, _, ev)| role == "assistant" && ev.is_some())
        .count();
    tracing::info!(
        "[chat] GET /chat/{}/messages: {} rows total, {} w/ annotations, {} w/ events",
        id,
        rows.len(),
        with_annot,
        with_events,
    );

    let messages: Vec<Value> = rows
        .into_iter()
        .map(|(id, role, content, created_at, annotations, events)| {
            // Hydrate annotations from the stored JSON. When the column
            // is NULL — older turns from before migration 0012, or
            // turns where the live pass dropped the citations (e.g.
            // <CITATIONS> block truncated without `</CITATIONS>` before
            // the truncation-tolerant parser shipped) — re-parse the
            // persisted content with the current (smarter) extractor.
            // This retroactively re-renders pills on chats that broke
            // silently in earlier builds.
            let annotations_value = annotations
                .as_deref()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .or_else(|| {
                    content.as_deref().and_then(extract_citations_block)
                })
                .map(|v| enrich_doc_citations(v, &chat_docs))
                .map(|v| remap_url_annotation_paths(v, &corpus_local_path_by_docid))
                .unwrap_or_else(|| Value::Array(Vec::new()));
            // Persisted non-text events (today: `doc_created`). Without
            // hydrating them here the download cards for generated
            // docx/xlsx vanish on chat reload — they only existed in
            // the live SSE stream, never in the prose. Returning the
            // raw array lets the frontend turn them into steps in the
            // same shape the live `onDocCreated` callback produces.
            let events_value = events
                .as_deref()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .unwrap_or_else(|| Value::Array(Vec::new()));
            json!({
                "id": id,
                "role": role,
                "content": content,
                "created_at": created_at,
                "annotations": annotations_value,
                "events": events_value,
            })
        })
        .collect();

    Ok(Json(json!({ "messages": messages })))
}

// ---------------------------------------------------------------------------
// GET /chat/:id/documents
// ---------------------------------------------------------------------------
/// Enumerate every document linked to this chat — uploads the user
/// attached through the composer plus docs synthesised by tools
/// (today: `generate_docx`). Both flows set `documents.chat_id` at
/// creation time (migration 0013), so a single query against that
/// column is the source of truth.
///
/// The chat-files popover in the composer footer (v0.4.3+) relies on
/// this endpoint instead of walking the in-memory `messages` array,
/// because `messages.files` is not echoed back by GET /chat/:id/messages
/// — so on a fresh chat select the frontend would otherwise lose every
/// upload reference and only see the persisted `doc_created` events
/// (generated docs).
///
/// Decision columns ride along so the UI can paint the strikethrough +
/// `Rifiutato` badge on rejected rows without a per-doc round-trip.
async fn get_chat_documents(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    // Ownership check up-front so a forged chat id doesn't leak other
    // users' rows even when the JOIN would have come up empty anyway.
    let owned: Option<(String,)> =
        sqlx::query_as("SELECT id FROM chats WHERE id = ? AND user_id = ?")
            .bind(&id)
            .bind(&auth.user_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    if owned.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "chat not found"));
    }

    let rows: Vec<(
        String,         // id
        String,         // filename
        String,         // file_type
        String,         // decision ('accepted' | 'rejected')
        Option<String>, // decision_reason
        Option<String>, // decision_summary
    )> = sqlx::query_as(
        "SELECT id, filename, file_type, decision, decision_reason, decision_summary \
         FROM documents \
         WHERE chat_id = ? AND user_id = ? \
         ORDER BY created_at ASC",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let documents: Vec<Value> = rows
        .into_iter()
        .map(
            |(doc_id, filename, file_type, decision, decision_reason, decision_summary)| {
                json!({
                    "id": doc_id,
                    "filename": filename,
                    "file_type": file_type,
                    "decision": decision,
                    "decision_reason": decision_reason,
                    "decision_summary": decision_summary,
                })
            },
        )
        .collect();

    Ok(Json(json!({ "documents": documents })))
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
            .unwrap_or_else(|| "gemini-3.5-flash".to_string())
    });
    let model = raw_model.clone();

    // Build per-provider config from saved settings.
    let local_config = build_local_config(&model, user_settings.as_ref());

    let system_prompt = body.system_prompt.unwrap_or_default();

    let params = StreamParams {
        model: model.clone(),
        system_prompt,
        system_volatile: String::new(),
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
                    .map(|_| "gemini-3.5-flash".to_string()),
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
            if s.gemini_api_key.is_some() { return Some("gemini-3.5-flash".to_string()); }
            None
        })
        .unwrap_or_else(|| "gemini-3.5-flash".to_string());

    tracing::info!("[chat] generate_title using model={title_model}");

    let local_config = build_local_config(&title_model, user_settings.as_ref());

    let prompt = format!(
        "Generate a concise 3-5 word title (no quotes, no punctuation) for a chat that begins with this user message:\n\n{}",
        first_msg.chars().take(500).collect::<String>()
    );

    let params = StreamParams {
        model: title_model.clone(),
        system_prompt: String::new(),
        system_volatile: String::new(),
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

    let mut title: String = title_text
        .lines()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| c == '"' || c == '\'' || c.is_whitespace())
        .chars()
        .take(80)
        .collect();

    if title.trim().is_empty() {
        let fallback_words: Vec<&str> = first_msg
            .split_whitespace()
            .take(5)
            .collect();
        let fallback = fallback_words.join(" ");
        title = fallback
            .trim_matches(|c: char| c == '"' || c == '\'' || c.is_whitespace())
            .chars()
            .take(80)
            .collect();
        if title.trim().is_empty() {
            title = "New chat".to_string();
        }
    }

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
        canonical_corpus_key, enrich_doc_citations, extract_citations_block,
        extract_inline_docid_refs, extract_inline_paren_doc_refs,
        rewrite_inline_docid_citations, sanitise_annotations_quotes,
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
    fn unclosed_block_still_parses_when_inner_is_valid_json() {
        // Truncation tolerance: a JSON array that arrived without its
        // closing tag must still parse — this is the recovery path the
        // citation-rendering UI depends on for long reports.
        let v = extract_citations_block("<CITATIONS>[1,2,3]").expect("parses");
        assert_eq!(v, json!([1, 2, 3]));
    }

    #[test]
    fn returns_none_for_invalid_json() {
        assert!(extract_citations_block("<CITATIONS>not json</CITATIONS>").is_none());
    }

    #[test]
    fn extract_inline_docid_refs_picks_up_the_observed_pattern() {
        // The exact shape the model emitted on the NIS2 report turn that
        // surfaced this bug: UUID + comma + `page N` (or `page N-M`).
        let text = "**Introduzione** [doc-id: cdbe5ce0-36f1-4574-a818-64e06826e632, page 1]. \
                    Continua [doc-id: cdbe5ce0-36f1-4574-a818-64e06826e632, page 1-2] eccetera.";
        let refs = extract_inline_docid_refs(text);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].handle, "cdbe5ce0-36f1-4574-a818-64e06826e632");
        assert_eq!(refs[0].page.as_deref(), Some("1"));
        assert_eq!(refs[1].handle, "cdbe5ce0-36f1-4574-a818-64e06826e632");
        assert_eq!(refs[1].page.as_deref(), Some("1-2"));
    }

    #[test]
    fn extract_inline_docid_refs_handles_doc_n_and_pages_and_no_page() {
        // Three legitimate variants: doc-N label, `pages` plural, and no
        // page at all (model cited the document without a page).
        let text = "A [doc-id: doc-0, page 3] B [doc-id: doc-1, pages 4] C [doc-id: doc-2] D";
        let refs = extract_inline_docid_refs(text);
        assert_eq!(refs.len(), 3);
        assert_eq!((refs[0].handle.as_str(), refs[0].page.as_deref()), ("doc-0", Some("3")));
        assert_eq!((refs[1].handle.as_str(), refs[1].page.as_deref()), ("doc-1", Some("4")));
        assert_eq!(refs[2].handle.as_str(), "doc-2");
        assert!(refs[2].page.is_none());
    }

    #[test]
    fn extract_inline_docid_refs_ignores_malformed_and_unrelated_brackets() {
        // Real bracket-rich prose must not produce false positives.
        let text = "Vedi [Page 3] e [art. 5] e [doc-id]: testo. \
                    Anche [doc-id: ] vuoto e [doc-id: foo, page abc] malformato.";
        let refs = extract_inline_docid_refs(text);
        assert!(refs.is_empty(), "expected no matches, got {refs:?}");
    }

    #[test]
    fn extract_inline_paren_doc_refs_picks_up_italian_paren_form() {
        // The exact shape from the Inventario beni assicurati workflow:
        // a parenthesised italian reference, no [doc-id:] bracket.
        let text = "Veicolo coperto (Polizza n. 449435502/39, doc-1, pag. 99-101). \
                    Cfr. anche doc-0, pag 5 e doc-2, pagina 12.";
        let refs = extract_inline_paren_doc_refs(text);
        assert_eq!(refs.len(), 3);
        assert_eq!((refs[0].handle.as_str(), refs[0].page.as_deref()), ("doc-1", Some("99-101")));
        assert_eq!((refs[1].handle.as_str(), refs[1].page.as_deref()), ("doc-0", Some("5")));
        assert_eq!((refs[2].handle.as_str(), refs[2].page.as_deref()), ("doc-2", Some("12")));
        // The match must NOT consume the closing paren or trailing
        // prose — only the `doc-N, pag …` substring.
        let m0 = &text[refs[0].start..refs[0].end];
        assert_eq!(m0, "doc-1, pag. 99-101");
    }

    #[test]
    fn extract_inline_paren_doc_refs_ignores_bare_doc_n_without_page_word() {
        // `doc-1` on its own is NOT a citation marker — many prompts
        // mention the label in passing. Must require a page word.
        let text = "Il file doc-1 contiene la polizza. Cfr. doc-1 per dettagli.";
        assert!(extract_inline_paren_doc_refs(text).is_empty());
    }

    #[test]
    fn rewrite_inline_docid_citations_handles_both_shapes_together() {
        // Mixed prose: one bracket shape + two paren shapes pointing at
        // the same doc on different pages. All three must collapse onto
        // distinct cN refs and rewrite in document order.
        let text = "Cfr. [doc-id: doc-0, page 1] e poi (Polizza, doc-0, pag. 7) \
                    nonché doc-1, pag. 3.";
        let (out, cits) = rewrite_inline_docid_citations(text, |h| match h {
            "doc-0" => Some(("uuid-A".into(), "polizza.pdf".into())),
            "doc-1" => Some(("uuid-B".into(), "schedule.pdf".into())),
            _ => None,
        })
        .expect("rewrites");
        assert_eq!(out, "Cfr. [c1] e poi (Polizza, [c2]) nonché [c3].");
        let arr = cits.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["page"], 1);
        assert_eq!(arr[1]["page"], 7);
        assert_eq!(arr[1]["filename"], "polizza.pdf");
        assert_eq!(arr[2]["page"], 3);
        assert_eq!(arr[2]["filename"], "schedule.pdf");
    }

    #[test]
    fn rewrite_inline_docid_citations_collapses_repeats_into_one_ref() {
        // Two references to the same (uuid, page) MUST share a single
        // c1 in the <CITATIONS> array — keeps the block compact.
        let text = "Vedi [doc-id: doc-0, page 1] e di nuovo [doc-id: doc-0, page 1].";
        let (out, cits) = rewrite_inline_docid_citations(text, |h| {
            if h == "doc-0" {
                Some(("uuid-abc".into(), "Report.docx".into()))
            } else {
                None
            }
        })
        .expect("at least one rewrite");
        assert_eq!(out, "Vedi [c1] e di nuovo [c1].");
        let arr = cits.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["ref"], "c1");
        assert_eq!(arr[0]["doc_id"], "uuid-abc");
        assert_eq!(arr[0]["document_id"], "uuid-abc");
        assert_eq!(arr[0]["filename"], "Report.docx");
        assert_eq!(arr[0]["page"], 1);
        assert_eq!(arr[0]["source"], "attached");
    }

    #[test]
    fn rewrite_inline_docid_citations_distinct_pages_get_distinct_refs() {
        let text = "Sez. A [doc-id: doc-0, page 1]; sez. B [doc-id: doc-0, page 2].";
        let (out, cits) = rewrite_inline_docid_citations(text, |h| {
            (h == "doc-0").then(|| ("uuid-abc".into(), "Report.docx".into()))
        })
        .expect("rewrite");
        assert_eq!(out, "Sez. A [c1]; sez. B [c2].");
        let arr = cits.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["page"], 1);
        assert_eq!(arr[1]["page"], 2);
    }

    #[test]
    fn rewrite_inline_docid_citations_leaves_unresolved_handles_alone() {
        // Security: handles that don't resolve through the user's
        // documents MUST NOT be rewritten — keep the model's text and
        // emit no citation for them, so we never reveal an arbitrary
        // UUID through the viewer.
        let text = "[doc-id: doc-0, page 1] e [doc-id: fake-uuid, page 2].";
        let (out, cits) = rewrite_inline_docid_citations(text, |h| {
            (h == "doc-0").then(|| ("uuid-abc".into(), "Report.docx".into()))
        })
        .expect("rewrite");
        assert_eq!(out, "[c1] e [doc-id: fake-uuid, page 2].");
        let arr = cits.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["ref"], "c1");
    }

    #[test]
    fn rewrite_inline_docid_citations_returns_none_when_nothing_resolves() {
        let text = "Nessun riferimento qui, solo prosa.";
        assert!(rewrite_inline_docid_citations(text, |_| None).is_none());
        // Even with a [doc-id: ...] marker — if every handle is unknown
        // we must return None and let the original body stand.
        let text = "[doc-id: ignoto, page 1]";
        assert!(rewrite_inline_docid_citations(text, |_| None).is_none());
    }

    #[test]
    fn canonical_corpus_key_collapses_inventory_variants_onto_one_key() {
        // The bug we are guarding against: the model copies the
        // `<USER LIBRARY>` line `[italian-legal] corte_costituzionale_1990_241`
        // (with bracket + space) as `doc_id` instead of the [gN] tag.
        // The canonical form must match whatever we index on the lookup
        // side — `<corpus_id> <corpus_identifier>` — and tolerate every
        // other punctuation / case variant.
        let canon = canonical_corpus_key("[italian-legal] corte_costituzionale_1990_241");
        assert_eq!(canon, "italianlegalcortecostituzionale1990241");
        // Every reasonable alternative form must collapse to the same key.
        for variant in [
            "italian-legal corte_costituzionale_1990_241",
            "Italian-Legal_corte_costituzionale_1990_241",
            "italianlegal:cortecostituzionale1990/241",
            "[ITALIAN-LEGAL] corte_costituzionale_1990_241",
        ] {
            assert_eq!(
                canonical_corpus_key(variant),
                canon,
                "variant {variant:?} did not collapse to the canonical key"
            );
        }
        // Sanity: bare ASCII passes through lowercase.
        assert_eq!(canonical_corpus_key("EurLex_32016R0679"), "eurlex32016r0679");
        // Empty / whitespace-only / punctuation-only inputs canonicalise
        // to the empty string, which the resolver must skip.
        assert_eq!(canonical_corpus_key(""), "");
        assert_eq!(canonical_corpus_key("   "), "");
        assert_eq!(canonical_corpus_key("[ ]"), "");
    }

    #[test]
    fn recovers_block_without_closing_tag() {
        // Exact shape observed in the wild: model wrote the JSON array
        // fully but ran out of output tokens before emitting the
        // `</CITATIONS>` tag. We must still surface the citations.
        let text = "Prose.\n<CITATIONS>\n[\n  {\"ref\":\"c1\",\"doc_id\":\"doc-0\",\"page\":1,\"quote\":\"hi\"},\n  {\"ref\":\"c2\",\"doc_id\":\"doc-0\",\"page\":2,\"quote\":\"bye\"}\n]";
        let v = extract_citations_block(text).expect("recovers without close tag");
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["ref"], "c1");
        assert_eq!(arr[1]["ref"], "c2");
    }

    #[test]
    fn recovers_truncated_array_with_partial_last_entry() {
        // The model's output was cut off mid-entry: the last
        // {"ref":"c3", … is missing its closing `}` and quote string.
        // We must recover c1 and c2 and drop the partial c3.
        let text = "<CITATIONS>\n[\n  {\"ref\":\"c1\",\"doc_id\":\"doc-0\",\"page\":1,\"quote\":\"hi\"},\n  {\"ref\":\"c2\",\"doc_id\":\"doc-0\",\"page\":2,\"quote\":\"bye\"},\n  {\"ref\":\"c3\",\"doc_id\":\"doc-0\",\"page\":3,\"quote\":\"truncated mid-stri";
        let v = extract_citations_block(text).expect("recovers truncated array");
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2, "expected first two complete entries");
        assert_eq!(arr[0]["ref"], "c1");
        assert_eq!(arr[1]["ref"], "c2");
    }

    #[test]
    fn truncation_recovery_handles_quote_with_brace_inside() {
        // A quote containing `}` must NOT trick the depth tracker — the
        // `}` inside the string is part of content, not structure.
        let text = "<CITATIONS>\n[\n  {\"ref\":\"c1\",\"quote\":\"value has } brace\"},\n  {\"ref\":\"c2\",\"quote\":\"second\"";
        let v = extract_citations_block(text).expect("recovers despite brace-in-string");
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["ref"], "c1");
    }

    #[test]
    fn recovers_block_without_closing_tag_via_escape_repair() {
        // Missing `</CITATIONS>` AND an over-escaped apostrophe inside
        // a quote — both must be tolerated, and the array must parse.
        let text = "<CITATIONS>\n[{\"ref\":\"c1\",\"quote\":\"SOCIETA\\' ALBA\"}]";
        let v = extract_citations_block(text).expect("parses after both repairs");
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0]["quote"], "SOCIETA' ALBA");
    }

    #[test]
    fn repairs_over_escaped_apostrophe() {
        // LLMs copy verbatim quotes and emit `\'`, which is not legal
        // JSON — the block must still parse after the escape repair.
        let text = "Answer [c1].\n<CITATIONS>\n\
            [{\"ref\": \"c1\", \"doc_id\": \"doc-0\", \
              \"quote\": \"SOCIETA\\' ALBA LEASING S.P.A.\"}]\n\
            </CITATIONS>";
        let v = extract_citations_block(text).unwrap();
        assert_eq!(v[0]["ref"], "c1");
        assert_eq!(v[0]["quote"], "SOCIETA' ALBA LEASING S.P.A.");
    }

    #[test]
    fn repair_leaves_valid_escapes_intact() {
        // A clean block with legitimate \n / \" escapes must round-trip
        // unchanged (it parses on the first attempt, no repair applied).
        let text = "<CITATIONS>[{\"quote\":\"line one\\nsays \\\"hi\\\"\"}]</CITATIONS>";
        let v = extract_citations_block(text).unwrap();
        assert_eq!(v[0]["quote"], "line one\nsays \"hi\"");
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
