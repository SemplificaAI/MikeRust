-- Opt-in HyDE (Hypothetical Document Embeddings) for the chat-time
-- vector retrieval pipeline.
--
-- When enabled, `retrieve_kb_chunks` (src/routes/chat.rs) asks the
-- user's currently-active LLM to draft a 3-4 sentence pseudo-answer
-- in the same register the corpus is written in — driven by the
-- domain-aware prologue introduced in v0.4.0
-- (config/system-prompts/<locale>/<domain>.md). Both the original
-- query AND the hypothesis are embedded; the two KNN result sets are
-- merged via Reciprocal Rank Fusion (k=60) and truncated to the
-- usual top-K.
--
-- Default OFF because the technique adds one extra LLM call per chat
-- turn — that cost is paid on the user's chosen provider (Anthropic /
-- Gemini / OpenAI / local), not local CPU compute, but it's still
-- material on a high-volume conversation. Users opt in via
-- Settings → Recupero documenti.

ALTER TABLE user_settings ADD COLUMN hyde_enabled INTEGER NOT NULL DEFAULT 0;
