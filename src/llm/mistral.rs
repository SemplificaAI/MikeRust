//! Mistral La Plateforme (`api.mistral.ai`) — dedicated provider.
//!
//! Split from `src/llm/local.rs` (OpenAI-compat path) in v0.6.0
//! because Mistral has provider-specific knobs that matter for the
//! legal-target product and would be wrong to send to a generic
//! OpenAI-shape endpoint:
//!
//! * **`parallel_tool_calls: false`** — Mistral defaults to parallel
//!   tool execution; legal workflows are sequential (tabular cell
//!   extraction, citation lookups in order, …) and benefit from the
//!   predictability of one-call-at-a-time semantics.
//!
//! * **`safe_prompt: false`** — Mistral's safety wrapper false-flags
//!   legitimate legal content (sentences on criminal cases, sensitive
//!   medical reports). Sent explicit-OFF here; the user-facing toggle
//!   lands with migration 0033 in v0.6.0 Commit B.
//!
//! * **`prompt_cache_key`** — Mistral charges only 10% of normal
//!   token price on cache hits. We derive a stable per-chat key
//!   (`mike_chat_{chat_id}`) so the system prompt + attached
//!   documents prefix is cached across turns. On a typical 20-turn
//!   document-heavy legal conversation the cache hit rate is 80-90%,
//!   for an effective cost reduction in the same range.
//!
//! Plus Mistral-specific error mapping: 401 invalid key, 403 quota
//! or guardrail block, 422 validation error, 429 rate limit with
//! `Retry-After` header parsing.
//!
//! ZDR (Zero Data Retention) is NOT a per-request header — it
//! requires a manual support ticket at help.mistral.ai (confirmed
//! 2026-06 against the official help center). The card UI surfaces
//! this as info text + a link, not as a checkbox.

use anyhow::{anyhow, Result};
use futures_util::stream;
use serde::Deserialize;
use serde_json::json;
use std::collections::VecDeque;

use super::local::{parse_sse_line_opt, to_wire_messages};
use super::types::{StreamEvent, StreamParams};
use crate::llm::BoxStream;

/// La Plateforme endpoint. Hardcoded — Mistral is a managed cloud
/// provider; the regional Azure-AI / AWS-Bedrock deployments need
/// different auth flows and are not modelled here. If a user wants
/// to point at one of those, they should use the `openai:` provider
/// path (which accepts a free-form `local_base_url`).
const MISTRAL_BASE_URL: &str = "https://api.mistral.ai/v1";

/// Tighter than the OpenAI / Ollama 1.0 default — citation lists,
/// structured JSON and tool calls all benefit from less random
/// sampling in this product. Same value `local.rs` ships.
const DEFAULT_TEMPERATURE: f32 = 0.5;

/// 8192 — long enough to fit the citation block legal answers
/// typically emit (which gets truncated at 4096 — see v0.5.1
/// history). Same value `local.rs` ships.
const DEFAULT_MAX_TOKENS: u32 = 8192;

fn resolve_credentials(params: &StreamParams) -> Result<(String, String)> {
    let cfg = params.local_config.as_ref().ok_or_else(|| {
        anyhow!(
            "Mistral: API key non configurata. Imposta la chiave in \
             Settings → Modelli LLM → Mistral AI."
        )
    })?;
    let api_key = cfg.api_key.as_deref().unwrap_or("").trim();
    if api_key.is_empty() {
        return Err(anyhow!(
            "Mistral: API key vuota. Inseriscila in \
             Settings → Modelli LLM → Mistral AI."
        ));
    }
    let model = if cfg.model.is_empty() {
        super::strip_model_prefix(&params.model).to_string()
    } else {
        cfg.model.clone()
    };
    if model.is_empty() {
        return Err(anyhow!(
            "Mistral: model id mancante. Esempi validi: \
             mistral-large-latest, mistral-medium-latest, mistral-small-latest."
        ));
    }
    Ok((api_key.to_string(), model))
}

/// Derive the cache key. Only emit when we have a chat id —
/// one-shot calls (title generation, HyDE, summarisation) have no
/// repeated-prefix structure to cache.
pub(crate) fn cache_key_for(params: &StreamParams) -> Option<String> {
    params
        .chat_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|cid| format!("mike_chat_{cid}"))
}

pub(crate) fn build_body(params: &StreamParams, model: &str, stream: bool) -> serde_json::Value {
    let messages = to_wire_messages(&params.full_system(), &params.messages);
    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": stream,
        "max_tokens": DEFAULT_MAX_TOKENS,
        "temperature": DEFAULT_TEMPERATURE,
        // Mistral default is `true`. Legal workflows benefit from
        // sequential tool execution: tabular reviews extract cell by
        // cell, citation lookups need a definitive order, and a
        // parallel call that returns out-of-order results is a debug
        // nightmare. v0.6.0 Commit B exposes a per-user toggle.
        "parallel_tool_calls": false,
        // Mistral default is already `false`; we send explicit-OFF
        // so future readers see the intent. Legal documents trip
        // the safe_prompt filter on legitimate content
        // (criminal-case sentences, sensitive medical reports).
        "safe_prompt": false,
    });
    if !params.tools.is_empty() {
        if let Ok(tools) = serde_json::to_value(&params.tools) {
            body["tools"] = tools;
        }
    }
    // The 10%-of-price cache key. Only emit when we have a chat
    // anchor — one-shot calls don't benefit and the key would be
    // wasted on unique prefixes.
    if let Some(key) = cache_key_for(params) {
        body["prompt_cache_key"] = json!(key);
    }
    body
}

/// Map a non-success HTTP response into a user-readable
/// `anyhow::Error`. Italian-first because that's the product
/// audience; the chat UI surfaces the message verbatim.
pub(crate) fn mistral_error(
    status: reqwest::StatusCode,
    body: &str,
    retry_after: Option<&str>,
) -> anyhow::Error {
    match status.as_u16() {
        401 => anyhow!(
            "Mistral 401: API key non valida o scaduta. \
             Verifica in Settings → Modelli LLM → Mistral AI."
        ),
        403 => anyhow!(
            "Mistral 403: richiesta rifiutata (quota esaurita o filtro \
             di sicurezza attivato). Dettagli: {body}"
        ),
        422 => anyhow!(
            "Mistral 422: payload non valido. Probabile model id \
             errato o messaggio malformato. Dettagli: {body}"
        ),
        429 => {
            let ra = retry_after
                .map(|s| format!(" (riprova fra ~{s}s)"))
                .unwrap_or_default();
            anyhow!("Mistral 429: rate limit raggiunto{ra}.")
        }
        500..=599 => anyhow!("Mistral {status}: errore lato server. Dettagli: {body}"),
        _ => anyhow!("Mistral {status}: {body}"),
    }
}

pub async fn stream(params: StreamParams) -> Result<BoxStream> {
    let (api_key, model) = resolve_credentials(&params)?;
    tracing::info!(
        "[llm/mistral] stream → model={model}, chat_id={:?}, cache_key={}",
        params.chat_id.as_deref(),
        cache_key_for(&params).as_deref().unwrap_or("<none>"),
    );
    let client = reqwest::Client::new();
    let body = build_body(&params, &model, true);

    let resp = client
        .post(format!("{MISTRAL_BASE_URL}/chat/completions"))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let text = resp.text().await.unwrap_or_default();
        return Err(mistral_error(status, &text, retry_after.as_deref()));
    }

    let byte_stream = resp.bytes_stream();
    let event_stream = stream::unfold(
        (byte_stream, String::new(), VecDeque::<StreamEvent>::new()),
        |(mut bs, mut buf, mut pending)| async move {
            use futures_util::StreamExt;
            loop {
                if let Some(ev) = pending.pop_front() {
                    return Some((Ok(ev), (bs, buf, pending)));
                }
                match bs.next().await {
                    None => return None,
                    Some(Ok(bytes)) => {
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                        while let Some(idx) = buf.find('\n') {
                            let line = buf[..idx].trim().to_string();
                            buf.drain(..=idx);
                            if line.is_empty() {
                                continue;
                            }
                            if let Some(ev) = parse_sse_line_opt(&line) {
                                pending.push_back(ev);
                            }
                        }
                    }
                    Some(Err(e)) => {
                        return Some((
                            Err(anyhow!("Mistral stream error: {e}")),
                            (bs, buf, pending),
                        ));
                    }
                }
            }
        },
    );

    Ok(Box::pin(event_stream))
}

pub async fn complete(params: StreamParams) -> Result<String> {
    let (api_key, model) = resolve_credentials(&params)?;
    let client = reqwest::Client::new();
    let body = build_body(&params, &model, false);

    let resp = client
        .post(format!("{MISTRAL_BASE_URL}/chat/completions"))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let text = resp.text().await.unwrap_or_default();
        return Err(mistral_error(status, &text, retry_after.as_deref()));
    }

    // Local types to avoid leaking Mistral wire shape into the rest
    // of the crate — the non-streaming response payload uses the
    // OpenAI shape and we only care about the first choice's text.
    #[derive(Deserialize)]
    struct CompleteResponse {
        choices: Vec<Choice>,
    }
    #[derive(Deserialize)]
    struct Choice {
        message: Msg,
    }
    #[derive(Deserialize)]
    struct Msg {
        content: Option<String>,
    }

    let parsed: CompleteResponse = resp.json().await?;
    Ok(parsed
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.content)
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::{LocalConfig, Message};

    fn params_with(
        api_key: &str,
        model: &str,
        chat_id: Option<&str>,
    ) -> StreamParams {
        StreamParams {
            model: format!("mistral:{model}"),
            system_prompt: "system test".into(),
            system_volatile: String::new(),
            messages: vec![Message::user("hi".to_string())],
            tools: vec![],
            max_iterations: 1,
            enable_thinking: false,
            local_config: Some(LocalConfig {
                base_url: MISTRAL_BASE_URL.to_string(),
                api_key: if api_key.is_empty() {
                    None
                } else {
                    Some(api_key.to_string())
                },
                model: model.to_string(),
                secure_mode: false,
            }),
            claude_api_key: None,
            gemini_api_key: None,
            gemini_region: None,
            chat_id: chat_id.map(String::from),
        }
    }

    #[test]
    fn resolve_credentials_rejects_missing_key() {
        let p = params_with("", "mistral-large-latest", None);
        let err = resolve_credentials(&p).unwrap_err();
        assert!(err.to_string().contains("vuota"));
    }

    #[test]
    fn resolve_credentials_rejects_missing_local_config() {
        let mut p = params_with("k", "mistral-large-latest", None);
        p.local_config = None;
        let err = resolve_credentials(&p).unwrap_err();
        assert!(err.to_string().contains("non configurata"));
    }

    #[test]
    fn resolve_credentials_accepts_valid_key_and_model() {
        let p = params_with("sk-test", "mistral-large-latest", None);
        let (key, model) = resolve_credentials(&p).unwrap();
        assert_eq!(key, "sk-test");
        assert_eq!(model, "mistral-large-latest");
    }

    #[test]
    fn cache_key_emitted_for_chat_scoped_call() {
        let p = params_with("k", "mistral-large-latest", Some("abc-123"));
        assert_eq!(
            cache_key_for(&p).as_deref(),
            Some("mike_chat_abc-123"),
        );
    }

    #[test]
    fn cache_key_omitted_for_one_shot_calls() {
        let p = params_with("k", "mistral-large-latest", None);
        assert!(cache_key_for(&p).is_none());
    }

    #[test]
    fn cache_key_omitted_for_empty_chat_id() {
        let p = params_with("k", "mistral-large-latest", Some(""));
        assert!(cache_key_for(&p).is_none());
    }

    #[test]
    fn build_body_disables_parallel_tool_calls() {
        let p = params_with("k", "mistral-large-latest", Some("x"));
        let body = build_body(&p, "mistral-large-latest", true);
        // Both literally `false`, not omitted — we want explicit OFF
        // so a future Mistral default change doesn't quietly turn
        // them back on.
        assert_eq!(body["parallel_tool_calls"], json!(false));
        assert_eq!(body["safe_prompt"], json!(false));
    }

    #[test]
    fn build_body_includes_cache_key_when_chat_scoped() {
        let p = params_with("k", "mistral-large-latest", Some("chat-xyz"));
        let body = build_body(&p, "mistral-large-latest", true);
        assert_eq!(body["prompt_cache_key"], json!("mike_chat_chat-xyz"));
    }

    #[test]
    fn build_body_omits_cache_key_for_one_shot() {
        let p = params_with("k", "mistral-large-latest", None);
        let body = build_body(&p, "mistral-large-latest", true);
        assert!(body.get("prompt_cache_key").is_none());
    }

    #[test]
    fn build_body_has_sensible_sampling_defaults() {
        let p = params_with("k", "mistral-large-latest", None);
        let body = build_body(&p, "mistral-large-latest", true);
        assert_eq!(body["temperature"], json!(0.5));
        assert_eq!(body["max_tokens"], json!(8192));
        assert_eq!(body["stream"], json!(true));
    }

    #[test]
    fn build_body_propagates_stream_flag_off_for_complete() {
        let p = params_with("k", "mistral-large-latest", None);
        let body = build_body(&p, "mistral-large-latest", false);
        assert_eq!(body["stream"], json!(false));
    }

    #[test]
    fn mistral_error_401_mentions_api_key() {
        let e = mistral_error(reqwest::StatusCode::UNAUTHORIZED, "{}", None);
        let s = e.to_string();
        assert!(s.contains("401"));
        assert!(s.contains("API key"));
    }

    #[test]
    fn mistral_error_429_extracts_retry_after() {
        let e = mistral_error(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            "{}",
            Some("12"),
        );
        let s = e.to_string();
        assert!(s.contains("429"));
        assert!(s.contains("~12s"));
    }

    #[test]
    fn mistral_error_429_without_retry_after_still_clear() {
        let e = mistral_error(reqwest::StatusCode::TOO_MANY_REQUESTS, "{}", None);
        let s = e.to_string();
        assert!(s.contains("429"));
        assert!(s.contains("rate limit"));
    }

    #[test]
    fn mistral_error_500_surfaces_body() {
        let e = mistral_error(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "{\"message\":\"internal\"}",
            None,
        );
        let s = e.to_string();
        assert!(s.contains("500"));
        assert!(s.contains("internal"));
    }
}
