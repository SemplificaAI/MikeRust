/// OpenAI-compatible streaming endpoint (vLLM, Infomaniak AI Tools, etc.)
/// Mirrors the logic in the TypeScript localllm.ts.
use anyhow::{anyhow, Result};
use futures_util::stream;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::types::{Message, Role, StreamEvent, StreamParams, ToolCall};
use crate::llm::{strip_model_prefix, BoxStream};

/// Normalize a base URL for OpenAI-compatible requests.
/// Accepts both "https://host" and "https://host/v1" forms — appends `/v1`
/// when the user-supplied URL doesn't already include a versioned suffix.
/// Adds `http://` when the user typed a host:port without scheme (typical
/// for local Ollama endpoints like `127.0.0.1:11434/v1`).
fn normalize_base(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/').to_string();
    let with_scheme = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed
    } else {
        format!("http://{trimmed}")
    };
    if with_scheme.ends_with("/v1") || with_scheme.contains("/v1/") {
        with_scheme
    } else {
        format!("{with_scheme}/v1")
    }
}

fn resolve_endpoint(params: &StreamParams) -> Result<(String, String, String)> {
    if let Some(cfg) = &params.local_config {
        let base = normalize_base(&cfg.base_url);
        let key = cfg.api_key.clone().unwrap_or_else(|| "local".to_string());
        let model = if cfg.model.is_empty() {
            strip_model_prefix(&params.model).to_string()
        } else {
            cfg.model.clone()
        };
        return Ok((base, key, model));
    }
    // Legacy env-var path
    let base = std::env::var("VLLM_BASE_URL")
        .map_err(|_| anyhow!("Local model not configured: set it in Account → Models, or set VLLM_BASE_URL"))?;
    let base = normalize_base(&base);
    let key = std::env::var("VLLM_API_KEY").unwrap_or_else(|_| "local".to_string());
    let model = if params.model == "localllm-light" {
        std::env::var("VLLM_LIGHT_MODEL").unwrap_or_else(|_| params.model.clone())
    } else if params.model.starts_with("localllm") {
        std::env::var("VLLM_MAIN_MODEL").unwrap_or_else(|_| params.model.clone())
    } else {
        strip_model_prefix(&params.model).to_string()
    };
    Ok((base, key, model))
}

#[derive(Deserialize)]
struct StreamChunk {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    delta: Delta,
}

#[derive(Deserialize, Default)]
struct Delta {
    content: Option<String>,
    tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Deserialize)]
struct ToolCallDelta {
    index: usize,
    id: Option<String>,
    function: Option<FunctionDelta>,
}

#[derive(Deserialize)]
struct FunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<serde_json::Value>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

fn to_wire_messages(system: &str, messages: &[Message]) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    if !system.is_empty() {
        out.push(json!({ "role": "system", "content": system }));
    }
    for m in messages {
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
            Role::System => "system",
        };
        // Tool result message — needs `tool_call_id`.
        if matches!(m.role, Role::Tool) {
            out.push(json!({
                "role": "tool",
                "tool_call_id": m.tool_call_id.clone().unwrap_or_default(),
                "content": m.content,
            }));
            continue;
        }
        // Assistant message that previously emitted tool_calls — replay them.
        if !m.tool_calls.is_empty() {
            let calls: Vec<serde_json::Value> = m.tool_calls.iter().map(|c| {
                json!({
                    "id": c.id,
                    "type": "function",
                    "function": {
                        "name": c.name,
                        "arguments": serde_json::to_string(&c.input).unwrap_or_else(|_| "{}".into()),
                    }
                })
            }).collect();
            let mut obj = serde_json::Map::new();
            obj.insert("role".into(), json!("assistant"));
            if !m.content.is_empty() {
                obj.insert("content".into(), json!(m.content));
            }
            obj.insert("tool_calls".into(), json!(calls));
            out.push(serde_json::Value::Object(obj));
            continue;
        }
        if m.images.is_empty() {
            out.push(json!({ "role": role, "content": m.content }));
        } else {
            // OpenAI vision content array: text + image_url parts.
            let mut parts: Vec<serde_json::Value> = Vec::new();
            if !m.content.is_empty() {
                parts.push(json!({ "type": "text", "text": m.content }));
            }
            for url in &m.images {
                parts.push(json!({
                    "type": "image_url",
                    "image_url": { "url": url }
                }));
            }
            out.push(json!({ "role": role, "content": parts }));
        }
    }
    out
}

pub async fn stream(
    params: StreamParams,
) -> Result<BoxStream> {
    let (base, api_key, model) = resolve_endpoint(&params)?;
    tracing::info!("[llm/local] stream → base={base}, model={model}, key_present={}", !api_key.is_empty() && api_key != "local");
    let client = reqwest::Client::new();

    let messages = to_wire_messages(&params.full_system(), &params.messages);
    let tools = if params.tools.is_empty() {
        None
    } else {
        Some(serde_json::to_value(&params.tools)?)
    };

    let body = ChatRequest {
        model,
        messages,
        tools,
        stream: true,
        // Ollama defaults to num_predict: 128 — too short for real answers.
        // OpenAI-compatible servers ignore this if their own limit is lower.
        max_tokens: Some(4096),
    };

    let resp = client
        .post(format!("{}/chat/completions", base.trim_end_matches('/')))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Local LLM error {status}: {text}"));
    }

    // Parse SSE stream
    let byte_stream = resp.bytes_stream();
    let event_stream = stream::unfold(
        (byte_stream, String::new()),
        |(mut bs, mut buf)| async move {
            loop {
                use futures_util::StreamExt;
                match bs.next().await {
                    None => {
                        // Connection closed by upstream. If anything remains
                        // in the buffer, try to parse it as a final event;
                        // otherwise close the stream cleanly. Do NOT emit a
                        // synthetic error for unparseable trailing data —
                        // that would be reported back to the client as a
                        // failed stream when the response is actually fine.
                        let trimmed = buf.trim().to_string();
                        buf.clear();
                        if trimmed.is_empty() {
                            return None;
                        }
                        for line in trimmed.lines() {
                            if let Some(ev) = parse_sse_line_opt(line.trim()) {
                                return Some((Ok(ev), (bs, buf)));
                            }
                        }
                        return None;
                    }
                    Some(Err(e)) => {
                        return Some((Err(anyhow!("stream error: {e}")), (bs, buf)));
                    }
                    Some(Ok(bytes)) => {
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                        if let Some(pos) = buf.find('\n') {
                            let line = buf[..pos].trim().to_string();
                            buf.drain(..=pos);
                            if let Some(ev) = parse_sse_line_opt(&line) {
                                return Some((Ok(ev), (bs, buf)));
                            }
                        }
                    }
                }
            }
        },
    );

    Ok(Box::pin(event_stream))
}

fn parse_sse_line(line: &str) -> Result<StreamEvent> {
    parse_sse_line_opt(line)
        .ok_or_else(|| anyhow!("empty SSE line"))
}

fn parse_sse_line_opt(line: &str) -> Option<StreamEvent> {
    if !line.starts_with("data: ") { return None; }
    let data = line[6..].trim();
    if data == "[DONE]" { return Some(StreamEvent::Done); }
    let chunk: StreamChunk = serde_json::from_str(data).ok()?;
    let delta = chunk.choices.into_iter().next()?.delta;
    if let Some(text) = delta.content {
        return Some(StreamEvent::ContentDelta(text));
    }
    if let Some(tcs) = delta.tool_calls {
        let calls: Vec<ToolCall> = tcs
            .into_iter()
            .filter_map(|tc| {
                let f = tc.function?;
                Some(ToolCall {
                    id: tc.id.unwrap_or_else(|| format!("tool-{}", tc.index)),
                    name: f.name.unwrap_or_default(),
                    input: serde_json::from_str(f.arguments.as_deref().unwrap_or("{}"))
                        .unwrap_or(json!({})),
                })
            })
            .collect();
        if !calls.is_empty() {
            return Some(StreamEvent::ToolCalls(calls));
        }
    }
    None
}

pub async fn complete(params: StreamParams) -> Result<String> {
    let (base, api_key, model) = resolve_endpoint(&params)?;
    tracing::info!("[llm/local] complete → base={base}, model={model}, key_present={}", !api_key.is_empty() && api_key != "local");
    let client = reqwest::Client::new();

    let messages = to_wire_messages(&params.full_system(), &params.messages);
    let body = json!({
        "model": model,
        "messages": messages,
        "stream": false,
        "max_tokens": 512,
    });

    let resp = client
        .post(format!("{}/chat/completions", base.trim_end_matches('/')))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        tracing::error!("[llm/local] complete non-success {status}: {text}");
        return Err(anyhow!("Local LLM error {status}: {text}"));
    }

    #[derive(Deserialize)]
    struct Resp { choices: Vec<RespChoice> }
    #[derive(Deserialize)]
    struct RespChoice { message: RespMessage }
    #[derive(Deserialize)]
    struct RespMessage { content: Option<String> }

    let raw = resp.text().await
        .map_err(|e| { tracing::error!("[llm/local] complete read body: {e}"); anyhow!(e) })?;
    let data: Resp = serde_json::from_str(&raw)
        .map_err(|e| {
            tracing::error!("[llm/local] complete parse error: {e} (body: {})", raw.chars().take(400).collect::<String>());
            anyhow!("Local LLM body parse error: {e}")
        })?;
    Ok(data.choices.into_iter().next()
        .and_then(|c| c.message.content)
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::StreamEvent;

    #[test]
    fn parses_done_marker() {
        match parse_sse_line_opt("data: [DONE]") {
            Some(StreamEvent::Done) => {}
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn ignores_non_data_lines() {
        assert!(parse_sse_line_opt(": comment").is_none());
        assert!(parse_sse_line_opt("event: message").is_none());
        assert!(parse_sse_line_opt("").is_none());
    }

    #[test]
    fn parses_content_delta() {
        let line = r#"data: {"id":"x","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"content":"hello"},"finish_reason":null}]}"#;
        match parse_sse_line_opt(line) {
            Some(StreamEvent::ContentDelta(s)) => assert_eq!(s, "hello"),
            other => panic!("expected ContentDelta, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_calls() {
        let line = r#"data: {"id":"x","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"read_document","arguments":"{\"doc_id\":\"doc-0\"}"}}]},"finish_reason":null}]}"#;
        match parse_sse_line_opt(line) {
            Some(StreamEvent::ToolCalls(calls)) => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].id, "call_1");
                assert_eq!(calls[0].name, "read_document");
                assert_eq!(calls[0].input["doc_id"], "doc-0");
            }
            other => panic!("expected ToolCalls, got {other:?}"),
        }
    }

    #[test]
    fn malformed_json_returns_none() {
        assert!(parse_sse_line_opt("data: not json").is_none());
    }

    #[test]
    fn empty_delta_returns_none() {
        let line = r#"data: {"id":"x","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{},"finish_reason":null}]}"#;
        assert!(parse_sse_line_opt(line).is_none());
    }
}
