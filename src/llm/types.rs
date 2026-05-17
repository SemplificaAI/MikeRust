use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub enum Provider {
    Claude,
    Gemini,
    OpenAI, // any OpenAI-compatible endpoint (vLLM, Infomaniak, etc.)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    /// Optional list of `data:image/png;base64,...` URLs to attach.
    /// Only honored when the target model is vision-capable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
    /// For `assistant` messages that requested tool calls in a previous turn.
    /// Replayed to the model as `tool_calls` in OpenAI-compatible format.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// For `tool` messages: the id of the call this result belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// For `tool` messages: the name of the invoked function. OpenAI keys
    /// tool results by id, Gemini keys them by function name — so we keep both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            images: vec![],
            tool_calls: vec![],
            tool_call_id: None,
            tool_name: None,
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            images: vec![],
            tool_calls: vec![],
            tool_call_id: None,
            tool_name: None,
        }
    }
    /// Synthetic system-role message — used by the summarizer to inject
    /// the compressed-history block. Most providers map this onto a
    /// `system` role, but Gemini doesn't have one; the gemini.rs
    /// adapter folds system content into the first user message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            images: vec![],
            tool_calls: vec![],
            tool_call_id: None,
            tool_name: None,
        }
    }
    pub fn tool_result(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            images: vec![],
            tool_calls: vec![],
            tool_call_id: Some(tool_call_id.into()),
            tool_name: Some(tool_name.into()),
        }
    }
    pub fn assistant_tool_calls(calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: String::new(),
            images: vec![],
            tool_calls: calls,
            tool_call_id: None,
            tool_name: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    #[serde(rename = "type")]
    pub kind: String, // "function"
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
}

#[derive(Debug)]
pub enum StreamEvent {
    ContentDelta(String),
    ReasoningDelta(String),
    ReasoningEnd,
    ToolCallStart { name: String },
    ToolCalls(Vec<ToolCall>),
    Done,
}

/// Per-user OpenAI-compatible endpoint config (Ollama / vLLM / Cloud Run / etc.)
/// When set, supersedes VLLM_BASE_URL / VLLM_API_KEY / VLLM_MAIN_MODEL env vars.
#[derive(Debug, Clone)]
pub struct LocalConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
}

pub struct StreamParams {
    pub model: String,
    /// Stable system-prompt prefix — identical across the turns of a chat
    /// (model instructions, attached-document text, tool descriptions).
    /// Sent as a cacheable block so providers that support prompt caching
    /// (Anthropic explicit, Gemini implicit) don't re-bill it every turn.
    pub system_prompt: String,
    /// Volatile system-prompt tail — content that changes per turn (today:
    /// per-query knowledge-base retrieval). Kept out of the cached prefix
    /// so it never invalidates the cache. Empty for non-chat callers.
    pub system_volatile: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSchema>,
    pub max_iterations: u32,
    pub enable_thinking: bool,
    pub local_config: Option<LocalConfig>,
    pub claude_api_key: Option<String>,
    pub gemini_api_key: Option<String>,
    /// Optional Google Cloud region (e.g. "europe-west1", "us-central1") for
    /// Gemini API calls. None or "global" → public endpoint. Preview models
    /// always force global; the `gemini::base_url` builder enforces this.
    pub gemini_region: Option<String>,
}

impl StreamParams {
    /// The full system prompt — stable prefix plus volatile tail — for
    /// providers that send it as one opaque string (Gemini, local).
    pub fn full_system(&self) -> String {
        if self.system_volatile.is_empty() {
            self.system_prompt.clone()
        } else if self.system_prompt.is_empty() {
            self.system_volatile.clone()
        } else {
            format!("{}\n\n---\n\n{}", self.system_prompt, self.system_volatile)
        }
    }
}
