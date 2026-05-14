//! Builtin tools that ship with Mike's legal-assistant identity.
//!
//! Mirror the OpenAI/Anthropic tool schemas declared by upstream Mike
//! (`backend/src/lib/chatTools.ts`):
//!
//! * `read_document` — fetch full text of a chat-attached document by `doc-N` label
//! * `find_in_document` — case-insensitive search within a document
//! * `read_workflow` — load the Markdown body of a saved workflow by id
//! * `generate_docx` — produce a downloadable .docx (stub for now)
//! * `edit_document` — modify an existing .docx (stub for now)
//!
//! The model is expected to call these tools to ground its answers. The
//! dispatch fn returns plain-string results that get fed back as `tool`
//! messages in the next iteration, exactly like MCP tool results.

use crate::llm::types::{ToolFunction, ToolSchema};
use crate::AppState;
use serde_json::{json, Value};
use std::collections::HashMap;

const READ_DOCUMENT: &str = "read_document";
const FIND_IN_DOCUMENT: &str = "find_in_document";
const READ_WORKFLOW: &str = "read_workflow";
const GENERATE_DOCX: &str = "generate_docx";
const EDIT_DOCUMENT: &str = "edit_document";
const LIST_DOCX_TEMPLATES: &str = "list_docx_templates";
const DESCRIBE_DOCX_TEMPLATE: &str = "describe_docx_template";

pub fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        READ_DOCUMENT
            | FIND_IN_DOCUMENT
            | READ_WORKFLOW
            | GENERATE_DOCX
            | EDIT_DOCUMENT
            | LIST_DOCX_TEMPLATES
            | DESCRIBE_DOCX_TEMPLATE
    )
}

pub fn schemas() -> Vec<ToolSchema> {
    fn fun(name: &str, description: &str, parameters: Value) -> ToolSchema {
        ToolSchema {
            kind: "function".to_string(),
            function: ToolFunction {
                name: name.to_string(),
                description: description.to_string(),
                parameters,
            },
        }
    }

    vec![
        fun(
            READ_DOCUMENT,
            "Read the full text content of a document attached by the user. Always call this before answering questions about, summarising, or citing from a document.",
            json!({
                "type": "object",
                "properties": {
                    "doc_id": {
                        "type": "string",
                        "description": "The document ID to read (e.g. 'doc-0', 'doc-1')"
                    }
                },
                "required": ["doc_id"]
            }),
        ),
        fun(
            FIND_IN_DOCUMENT,
            "Search for specific strings inside a document — a Ctrl+F equivalent. Returns each match with surrounding context. Matching is case-insensitive and whitespace-tolerant.",
            json!({
                "type": "object",
                "properties": {
                    "doc_id": { "type": "string", "description": "The document ID to search (e.g. 'doc-0')." },
                    "query":  { "type": "string", "description": "The string to search for (case-insensitive)." },
                    "max_results": { "type": "integer", "description": "Maximum matches to return (default 20).", "minimum": 1, "maximum": 200 }
                },
                "required": ["doc_id", "query"]
            }),
        ),
        fun(
            READ_WORKFLOW,
            "Read the full instructions (prompt) of a workflow by its ID. Call this after a workflow marker has been mentioned.",
            json!({
                "type": "object",
                "properties": {
                    "workflow_id": { "type": "string", "description": "The workflow ID to read." }
                },
                "required": ["workflow_id"]
            }),
        ),
        fun(
            GENERATE_DOCX,
            "Produce a downloadable .docx document. Two modes:\n\
             • **Template mode** (preferred): pass `template_id` (e.g. 'it/diffida-messa-in-mora') and `metadata` (the values for the template's `[PLACEHOLDERS]`). The body Markdown is rendered through the template's layout — typography, margins, styles all come from the template sidecar. Use `list_docx_templates` to discover templates and `describe_docx_template` to see the required metadata fields for a specific one.\n\
             • **Plain mode** (legacy): omit `template_id`. Falls back to a minimal generic layout. Pass `title` for the filename.\n\
             Returns the new document id and filename.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Document title / base filename (no extension)." },
                    "body":  { "type": "string", "description": "Document content in Markdown. Headings, bullet lists, bold/italic honoured. With a template_id, `[PLACEHOLDER]` tokens in the body are substituted against the metadata map." },
                    "template_id": { "type": "string", "description": "Optional. The id of a docx-template from list_docx_templates (e.g. 'it/diffida-messa-in-mora'). When omitted, falls back to the plain renderer." },
                    "metadata": {
                        "type": "object",
                        "description": "Map of [PLACEHOLDER] name → value, e.g. { \"DEBITORE\": \"Tizio S.r.l.\", \"IMPORTO\": \"€ 12.345,67\" }. Required when template_id is supplied. Universal fields (LUOGO, DATA, MITTENTE, OGGETTO, RIF_PRATICA, ...) should always be filled.",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["body"]
            }),
        ),
        fun(
            LIST_DOCX_TEMPLATES,
            "List the DOCX templates available to the closing formatter. Returns id, display_name, category, domain, automation_level, and required_metadata for each. Filter by `domain` (e.g. 'legal', 'finance', 'real_estate', 'compliance'). Call this FIRST when the user asks to produce a structured document (atto, diffida, parcella, contratto, ...) to pick the right template, then call describe_docx_template to see how to fill it, then generate_docx.",
            json!({
                "type": "object",
                "properties": {
                    "domain": { "type": "string", "description": "Optional canonical domain filter: 'legal' | 'medical' | 'finance' | 'real_estate' | 'hr' | 'insurance' | 'ip' | 'compliance' | 'others'." },
                    "locale": { "type": "string", "description": "Optional locale filter, e.g. 'it' to see only Italian templates." }
                },
                "required": []
            }),
        ),
        fun(
            DESCRIBE_DOCX_TEMPLATE,
            "Get the full authoring contract for a specific DOCX template: auto-generated system-prompt block (layout + section skeleton + required fields + per-field guidance), source reference into the Prontuario, and raw sidecar JSON. Call this AFTER list_docx_templates and BEFORE generate_docx — the returned `prompt_md` is what teaches you how to write a correct body for this template.",
            json!({
                "type": "object",
                "properties": {
                    "template_id": { "type": "string", "description": "The template id from list_docx_templates, e.g. 'it/diffida-messa-in-mora'." }
                },
                "required": ["template_id"]
            }),
        ),
        fun(
            EDIT_DOCUMENT,
            "Apply minimal substitutions to an existing .docx document attached to the chat. Pass `doc_id` (e.g. 'doc-0') and an array of `edits`, each with `find` and `replace` strings. The find string MUST appear verbatim in the document.",
            json!({
                "type": "object",
                "properties": {
                    "doc_id": { "type": "string", "description": "The document ID to edit (e.g. 'doc-0')." },
                    "edits": {
                        "type": "array",
                        "description": "List of substitutions to apply atomically.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "find":    { "type": "string" },
                                "replace": { "type": "string" }
                            },
                            "required": ["find", "replace"]
                        }
                    }
                },
                "required": ["doc_id", "edits"]
            }),
        ),
    ]
}

/// `doc_label_map` maps the chat-local label (`doc-0`, `doc-1`, …) to the
/// real `documents.id` UUID stored in SQLite. Built by the chat dispatcher
/// from the message's attached files.
pub async fn dispatch(
    state: &AppState,
    user_id: &str,
    doc_label_map: &HashMap<String, String>,
    name: &str,
    arguments: &Value,
) -> String {
    match name {
        READ_DOCUMENT => exec_read_document(state, user_id, doc_label_map, arguments).await,
        FIND_IN_DOCUMENT => exec_find_in_document(state, user_id, doc_label_map, arguments).await,
        READ_WORKFLOW => exec_read_workflow(state, user_id, arguments).await,
        GENERATE_DOCX => exec_generate_docx(state, user_id, arguments).await,
        EDIT_DOCUMENT => exec_edit_document(state, user_id, doc_label_map, arguments).await,
        LIST_DOCX_TEMPLATES => exec_list_docx_templates(state, arguments).await,
        DESCRIBE_DOCX_TEMPLATE => exec_describe_docx_template(state, arguments).await,
        other => json!({"error": format!("unknown builtin tool: {other}")}).to_string(),
    }
}

async fn resolve_doc(
    state: &AppState,
    user_id: &str,
    doc_label_map: &HashMap<String, String>,
    label_or_id: &str,
) -> Option<(String, String, Option<String>)> {
    let real_id = doc_label_map
        .get(label_or_id)
        .cloned()
        .unwrap_or_else(|| label_or_id.to_string());
    let row: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT filename, file_type, storage_path FROM documents WHERE id = ? AND user_id = ?",
    )
    .bind(&real_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    row
}

async fn exec_read_document(
    state: &AppState,
    user_id: &str,
    doc_label_map: &HashMap<String, String>,
    arguments: &Value,
) -> String {
    let doc_label = arguments.get("doc_id").and_then(|v| v.as_str()).unwrap_or("");
    if doc_label.is_empty() {
        return json!({"error": "doc_id is required"}).to_string();
    }
    let Some((filename, file_type, Some(storage_path))) =
        resolve_doc(state, user_id, doc_label_map, doc_label).await
    else {
        return json!({"error": format!("document {doc_label} not found")}).to_string();
    };
    let bytes = match crate::storage::make_storage()
        .ok()
        .and_then(|s| Some(s))
    {
        Some(s) => match s.get(&storage_path).await {
            Ok(b) => b,
            Err(e) => return json!({"error": format!("storage read: {e}")}).to_string(),
        },
        None => return json!({"error": "storage backend unavailable"}).to_string(),
    };
    let text = extract_text(&file_type, &filename, &bytes);
    json!({
        "doc_id": doc_label,
        "filename": filename,
        "file_type": file_type,
        "text": text,
    })
    .to_string()
}

async fn exec_find_in_document(
    state: &AppState,
    user_id: &str,
    doc_label_map: &HashMap<String, String>,
    arguments: &Value,
) -> String {
    let doc_label = arguments.get("doc_id").and_then(|v| v.as_str()).unwrap_or("");
    let query = arguments.get("query").and_then(|v| v.as_str()).unwrap_or("");
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(20)
        .min(200) as usize;
    if doc_label.is_empty() || query.is_empty() {
        return json!({"error": "doc_id and query are required"}).to_string();
    }
    let Some((filename, file_type, Some(storage_path))) =
        resolve_doc(state, user_id, doc_label_map, doc_label).await
    else {
        return json!({"error": format!("document {doc_label} not found")}).to_string();
    };
    let bytes = match crate::storage::make_storage()
        .ok()
        .and_then(|s| Some(s))
    {
        Some(s) => match s.get(&storage_path).await {
            Ok(b) => b,
            Err(e) => return json!({"error": format!("storage read: {e}")}).to_string(),
        },
        None => return json!({"error": "storage backend unavailable"}).to_string(),
    };
    let text = extract_text(&file_type, &filename, &bytes);

    // Case-insensitive, whitespace-tolerant search.
    let needle: String = query.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let haystack_norm: String = text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();

    let mut matches = Vec::new();
    let mut start = 0usize;
    while let Some(idx) = haystack_norm[start..].find(&needle) {
        let abs = start + idx;
        let ctx_lo = abs.saturating_sub(60);
        let ctx_hi = (abs + needle.len() + 60).min(haystack_norm.len());
        let snippet = &haystack_norm[ctx_lo..ctx_hi];
        matches.push(json!({
            "offset": abs,
            "snippet": snippet,
        }));
        if matches.len() >= max_results { break; }
        start = abs + needle.len();
    }
    json!({
        "doc_id": doc_label,
        "query": query,
        "match_count": matches.len(),
        "matches": matches,
    })
    .to_string()
}

async fn exec_read_workflow(state: &AppState, user_id: &str, arguments: &Value) -> String {
    let id = arguments.get("workflow_id").and_then(|v| v.as_str()).unwrap_or("");
    if id.is_empty() {
        return json!({"error": "workflow_id is required"}).to_string();
    }

    // ── First check the in-memory preset registry. System workflows
    // (`builtin-*`) live there, not in the DB. Without this branch
    // every preset workflow would 404 on read_workflow.
    if let Some(preset) = state.workflow_presets.iter().find(|p| p.id == id) {
        return build_read_workflow_response(
            id,
            &preset.title,
            preset.prompt_md.as_deref().unwrap_or(""),
            preset.default_output_template.as_deref(),
            &state.docx_templates,
        )
        .to_string();
    }

    let row: Option<(String, String)> =
        sqlx::query_as("SELECT title, prompt_md FROM workflows WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    let Some((title, prompt_md)) = row else {
        return json!({"error": format!("workflow {id} not found")}).to_string();
    };
    // User-created workflows don't carry default_output_template yet
    // — that's a system-preset-only field for now. Phase 2 may surface
    // it in the workflow editor UI and add the DB column.
    build_read_workflow_response(id, &title, &prompt_md, None, &state.docx_templates)
        .to_string()
}

/// Bundle a workflow's prompt with the linked DOCX template's
/// authoring contract (if any) into the JSON payload `read_workflow`
/// returns. Pure function — takes only the inputs it needs, no
/// AppState — so unit tests can drive it with hand-rolled templates.
fn build_read_workflow_response(
    workflow_id: &str,
    title: &str,
    prompt_md: &str,
    default_output_template: Option<&str>,
    docx_templates: &[crate::presets::docx_template::DocxTemplate],
) -> Value {
    let mut payload = json!({
        "workflow_id": workflow_id,
        "title": title,
        "prompt_md": prompt_md,
    });

    if let Some(tpl_id) = default_output_template {
        if let Some(template) = docx_templates.iter().find(|t| t.id == tpl_id) {
            payload["default_output_template"] = json!({
                "template_id": template.id,
                "display_name": template.display_name_for("it"),
                "automation_level": template.automation_level,
                "required_metadata": template.required_metadata,
                "prompt_md": template.auto_generated_prompt_md("it"),
                "source_reference": template.source_reference,
            });
            payload["closing_instruction"] = json!(format!(
                "This workflow produces a Word document. Once the user's request is clear, write the body in Markdown and call generate_docx(template_id=\"{}\", body_md=..., metadata=...). The template's authoring contract above (default_output_template.prompt_md) tells you what sections to emit and which fields to collect.",
                template.id
            ));
        } else {
            payload["default_output_template_missing"] = json!(format!(
                "Workflow references template_id={tpl_id} but it isn't loaded. The user should drop the sidecar into config/docx-templates/. Continue without docx output."
            ));
        }
    }
    payload
}

async fn exec_generate_docx(state: &AppState, user_id: &str, arguments: &Value) -> String {
    let body = arguments.get("body").and_then(|v| v.as_str()).unwrap_or("");
    if body.is_empty() {
        return json!({"error": "body (Markdown) is required"}).to_string();
    }
    let template_id = arguments
        .get("template_id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let explicit_title = arguments
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // ── Branch A: template-driven render via src/docx/ pipeline.
    let (bytes, default_title, unresolved): (Vec<u8>, String, Vec<String>) =
        if let Some(template_id) = template_id {
            let template = state
                .docx_templates
                .iter()
                .find(|t| t.id == template_id);
            let Some(template) = template else {
                return json!({
                    "error": format!("template_id {template_id} not found"),
                    "hint": "Call list_docx_templates to see available ids."
                })
                .to_string();
            };

            // metadata map[String→String]. Coerce numeric / bool
            // values to string so the LLM can be sloppy.
            let metadata: std::collections::HashMap<String, String> = arguments
                .get("metadata")
                .and_then(|v| v.as_object())
                .map(|obj| {
                    obj.iter()
                        .map(|(k, v)| {
                            let s = match v {
                                Value::String(s) => s.clone(),
                                Value::Number(n) => n.to_string(),
                                Value::Bool(b) => b.to_string(),
                                Value::Null => String::new(),
                                other => other.to_string(),
                            };
                            (k.clone(), s)
                        })
                        .collect()
                })
                .unwrap_or_default();

            let outcome = match crate::docx::render(template, body, &metadata) {
                Ok(o) => o,
                Err(e) => return json!({"error": format!("docx render: {e}")}).to_string(),
            };
            let title_default = template
                .display_name_for("it")
                .replace('/', "-");
            (outcome.bytes, title_default, outcome.unresolved_placeholders)
        } else {
            // ── Branch B: backwards-compat plain render.
            let title_for_legacy = explicit_title
                .clone()
                .unwrap_or_else(|| "Untitled".to_string());
            let bytes =
                match crate::pdf::docx_writer::markdown_to_docx(&title_for_legacy, body) {
                    Ok(b) => b,
                    Err(e) => {
                        return json!({"error": format!("docx build: {e}")}).to_string();
                    }
                };
            (bytes, title_for_legacy, Vec::new())
        };

    let title = explicit_title.unwrap_or(default_title);
    let safe_title = sanitize_filename(&title);
    let filename = format!("{safe_title}.docx");
    let doc_id = uuid::Uuid::new_v4().to_string();
    let storage_path = format!("documents/{user_id}/{doc_id}");

    let storage = match crate::storage::make_storage() {
        Ok(s) => s,
        Err(e) => return json!({"error": format!("storage: {e}")}).to_string(),
    };
    if let Err(e) = storage
        .put(
            &storage_path,
            &bytes,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        )
        .await
    {
        return json!({"error": format!("storage write: {e}")}).to_string();
    }

    let size = bytes.len() as i64;
    if let Err(e) = sqlx::query(
        "INSERT INTO documents (id, user_id, project_id, filename, file_type, size_bytes, storage_path, status) \
         VALUES (?, ?, NULL, ?, 'docx', ?, ?, 'ready')",
    )
    .bind(&doc_id)
    .bind(user_id)
    .bind(&filename)
    .bind(size)
    .bind(&storage_path)
    .execute(&state.db)
    .await
    {
        return json!({"error": format!("db: {e}")}).to_string();
    }

    let mut payload = json!({
        "doc_id": doc_id,
        "filename": filename,
        "size_bytes": size,
        "note": "Document persisted as a standalone document. Call read_document with this doc_id to verify content before describing it to the user."
    });
    if !unresolved.is_empty() {
        payload["unresolved_placeholders"] = json!(unresolved);
        payload["warning"] = json!(format!(
            "{} placeholder(s) still present in the document — these metadata fields were not supplied: {}. The document is generated but the user will see the gaps. Consider regenerating with the missing fields filled.",
            unresolved.len(),
            unresolved.join(", ")
        ));
    }
    payload.to_string()
}

async fn exec_list_docx_templates(state: &AppState, arguments: &Value) -> String {
    let domain_filter = arguments
        .get("domain")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let locale_filter = arguments
        .get("locale")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let items: Vec<Value> = state
        .docx_templates
        .iter()
        .filter(|t| t.matches_domain(domain_filter.as_deref()))
        .filter(|t| {
            locale_filter
                .as_deref()
                .is_none_or(|l| t.locale.starts_with(l))
        })
        .map(|t| {
            json!({
                "id": t.id,
                "display_name": t.display_name_for("it"),
                "category": t.category,
                "domain": t.domain,
                "also_applicable_to": t.also_applicable_to,
                "locale": t.locale,
                "automation_level": t.automation_level,
                "required_metadata": t.required_metadata,
                "source_reference": t.source_reference,
            })
        })
        .collect();
    json!({ "templates": items, "count": items.len() }).to_string()
}

async fn exec_describe_docx_template(state: &AppState, arguments: &Value) -> String {
    let id = arguments
        .get("template_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if id.is_empty() {
        return json!({"error": "template_id is required"}).to_string();
    }
    let Some(template) = state.docx_templates.iter().find(|t| t.id == id) else {
        return json!({
            "error": format!("template {id} not found"),
            "hint": "Call list_docx_templates to see available ids."
        })
        .to_string();
    };
    // Return both the auto-generated authoring prompt AND the raw
    // sidecar so the model can introspect any field it wants. The
    // prompt_md is the load-bearing payload — it's what the model
    // injects into its own working context to know HOW to write a
    // body for this template.
    json!({
        "template_id": template.id,
        "display_name": template.display_name_for("it"),
        "prompt_md": template.auto_generated_prompt_md("it"),
        "sidecar": template.to_api_json(),
    })
    .to_string()
}

async fn exec_edit_document(
    state: &AppState,
    user_id: &str,
    doc_label_map: &HashMap<String, String>,
    arguments: &Value,
) -> String {
    let label = arguments.get("doc_id").and_then(|v| v.as_str()).unwrap_or("");
    let edits_val = arguments.get("edits").and_then(|v| v.as_array());
    let Some(edits_val) = edits_val else {
        return json!({"error": "edits array is required"}).to_string();
    };
    let edits: Vec<crate::pdf::docx_writer::DocxEdit> = edits_val
        .iter()
        .filter_map(|e| {
            let find = e.get("find").and_then(|v| v.as_str())?.to_string();
            let replace = e.get("replace").and_then(|v| v.as_str())?.to_string();
            Some(crate::pdf::docx_writer::DocxEdit { find, replace })
        })
        .collect();
    if edits.is_empty() {
        return json!({"error": "no valid edit entries"}).to_string();
    }

    let Some((filename, file_type, Some(storage_path))) =
        resolve_doc(state, user_id, doc_label_map, label).await
    else {
        return json!({"error": format!("document {label} not found")}).to_string();
    };
    if file_type != "docx" {
        return json!({"error": format!("edit_document only supports .docx files (got {file_type})")}).to_string();
    }

    let storage = match crate::storage::make_storage() {
        Ok(s) => s,
        Err(e) => return json!({"error": format!("storage: {e}")}).to_string(),
    };
    let bytes = match storage.get(&storage_path).await {
        Ok(b) => b,
        Err(e) => return json!({"error": format!("storage read: {e}")}).to_string(),
    };

    let (new_bytes, hits) = match crate::pdf::docx_writer::apply_text_edits(&bytes, &edits) {
        Ok(x) => x,
        Err(e) => return json!({"error": format!("docx edit: {e}")}).to_string(),
    };

    if let Err(e) = storage
        .put(
            &storage_path,
            &new_bytes,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        )
        .await
    {
        return json!({"error": format!("storage write: {e}")}).to_string();
    }
    let new_size = new_bytes.len() as i64;
    let real_id = doc_label_map
        .get(label)
        .cloned()
        .unwrap_or_else(|| label.to_string());
    let _ = sqlx::query("UPDATE documents SET size_bytes = ? WHERE id = ? AND user_id = ?")
        .bind(new_size)
        .bind(&real_id)
        .bind(user_id)
        .execute(&state.db)
        .await;

    let summary: Vec<Value> = edits
        .iter()
        .zip(hits.iter())
        .map(|(e, h)| json!({"find": e.find, "replace": e.replace, "hits": h}))
        .collect();
    json!({
        "doc_id": label,
        "filename": filename,
        "edits_applied": summary,
    })
    .to_string()
}

fn sanitize_filename(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.is_empty() { return "Untitled".to_string(); }
    let cleaned: String = trimmed
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    cleaned.chars().take(60).collect::<String>().trim().to_string()
}

fn extract_text(file_type: &str, filename: &str, bytes: &[u8]) -> String {
    match file_type {
        "docx" => crate::pdf::extract_docx_text(bytes).unwrap_or_default(),
        "rtf" => {
            // Same path the sync scanner uses — RtfDocument::get_text()
            // returns the body without control words / fonts / pictures.
            let raw = String::from_utf8_lossy(bytes);
            rtf_parser::RtfDocument::try_from(raw.as_ref())
                .map(|d| d.get_text())
                .unwrap_or_default()
        }
        "xlsx" | "xls" | "xlsb" | "ods" => {
            crate::pdf::extract_xlsx_text(bytes).unwrap_or_default()
        }
        "txt" | "md" | "csv" => String::from_utf8_lossy(bytes).to_string(),
        "pdf" => {
            #[cfg(feature = "pdf")]
            {
                let tmp = std::env::temp_dir().join(format!("mike-builtin-{filename}"));
                if std::fs::write(&tmp, bytes).is_ok() {
                    let out = crate::pdf::extract_full_text(&tmp).unwrap_or_default();
                    let _ = std::fs::remove_file(&tmp);
                    out
                } else {
                    String::new()
                }
            }
            #[cfg(not(feature = "pdf"))]
            {
                let _ = filename;
                String::new()
            }
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_builtin_recognises_each_tool() {
        for name in [
            "read_document",
            "find_in_document",
            "read_workflow",
            "generate_docx",
            "edit_document",
            "list_docx_templates",
            "describe_docx_template",
        ] {
            assert!(is_builtin(name), "{name} should be builtin");
        }
        assert!(!is_builtin("unknown_tool"));
        assert!(!is_builtin(""));
    }

    #[test]
    fn schemas_have_required_fields() {
        let s = schemas();
        assert_eq!(s.len(), 7);
        for sch in &s {
            assert_eq!(sch.kind, "function");
            assert!(!sch.function.name.is_empty());
            assert!(!sch.function.description.is_empty());
            assert_eq!(sch.function.parameters["type"], "object");
        }
        let names: Vec<&str> = s.iter().map(|t| t.function.name.as_str()).collect();
        assert!(names.contains(&"read_document"));
        assert!(names.contains(&"find_in_document"));
        assert!(names.contains(&"read_workflow"));
        assert!(names.contains(&"generate_docx"));
        assert!(names.contains(&"list_docx_templates"));
        assert!(names.contains(&"describe_docx_template"));
        assert!(names.contains(&"edit_document"));
    }

    #[test]
    fn schema_required_arrays_are_consistent() {
        let s = schemas();
        for sch in &s {
            let p = &sch.function.parameters;
            let required = p["required"].as_array().expect("required must be array");
            let props = p["properties"].as_object().expect("properties must be object");
            for r in required {
                let key = r.as_str().unwrap();
                assert!(props.contains_key(key), "{} requires {key} but property not declared", sch.function.name);
            }
        }
    }

    // ── build_read_workflow_response ────────────────────────────────

    fn diffida_template() -> crate::presets::docx_template::DocxTemplate {
        let dir = crate::presets::config_subdir("docx-templates");
        let templates =
            crate::presets::docx_template::load_docx_templates(&dir).expect("load");
        templates
            .into_iter()
            .find(|t| t.id == "it/diffida-messa-in-mora")
            .expect("diffida present")
    }

    #[test]
    fn read_workflow_response_without_template_carries_only_core_fields() {
        let payload = build_read_workflow_response(
            "wf-id",
            "Test workflow",
            "Sei un assistente. Aiuta l'utente.",
            None,
            &[],
        );
        assert_eq!(payload["workflow_id"], json!("wf-id"));
        assert_eq!(payload["title"], json!("Test workflow"));
        assert!(payload.get("default_output_template").is_none());
        assert!(payload.get("default_output_template_missing").is_none());
        assert!(payload.get("closing_instruction").is_none());
    }

    #[test]
    fn read_workflow_response_with_template_bundles_authoring_contract() {
        let templates = vec![diffida_template()];
        let payload = build_read_workflow_response(
            "builtin-redazione-diffida",
            "Redazione diffida",
            "Sei un avvocato. Redigi una diffida.",
            Some("it/diffida-messa-in-mora"),
            &templates,
        );

        // Core fields
        assert_eq!(payload["workflow_id"], json!("builtin-redazione-diffida"));

        // default_output_template object present and well-formed
        let dot = payload["default_output_template"].as_object().expect("dot present");
        assert_eq!(dot["template_id"], json!("it/diffida-messa-in-mora"));
        assert_eq!(dot["automation_level"], json!("L1"));
        // required_metadata is a non-empty array
        assert!(dot["required_metadata"].as_array().unwrap().len() >= 5);
        // prompt_md contains the layout description (anchored to a
        // stable substring from the sidecar)
        assert!(dot["prompt_md"].as_str().unwrap().contains("Calibri"));
        // source_reference points back to the Prontuario
        assert!(dot["source_reference"]
            .as_str()
            .unwrap()
            .contains("TEMPLATE_PRONTUARIO"));

        // closing_instruction explicitly names generate_docx with the id
        let ci = payload["closing_instruction"].as_str().expect("closing");
        assert!(ci.contains("generate_docx"));
        assert!(ci.contains(r#"template_id="it/diffida-messa-in-mora""#));
    }

    #[test]
    fn read_workflow_response_with_unknown_template_emits_missing_field() {
        let payload = build_read_workflow_response(
            "wf-id",
            "Workflow rotto",
            "...",
            Some("it/non-esiste"),
            &[diffida_template()],
        );
        let missing = payload["default_output_template_missing"]
            .as_str()
            .expect("missing field present");
        assert!(missing.contains("it/non-esiste"));
        assert!(missing.contains("config/docx-templates/"));
        // No legit template object — the wiring failed.
        assert!(payload.get("default_output_template").is_none());
        // No closing_instruction either — there's no template to wrap up against.
        assert!(payload.get("closing_instruction").is_none());
    }

    #[test]
    fn read_workflow_response_with_template_id_but_empty_registry() {
        // Edge case: server starts with the docx-templates dir empty
        // (or removed). A workflow that references a template should
        // still produce a usable response — just with the
        // _missing field set.
        let payload = build_read_workflow_response(
            "wf-id",
            "Test",
            "...",
            Some("it/diffida-messa-in-mora"),
            &[],
        );
        assert!(payload["default_output_template_missing"].is_string());
    }

    #[test]
    fn sanitize_filename_default_when_empty() {
        assert_eq!(sanitize_filename(""), "Untitled");
        assert_eq!(sanitize_filename("    "), "Untitled");
    }

    #[test]
    fn sanitize_filename_replaces_unsafe_chars() {
        let s = sanitize_filename("foo/bar:baz?\\<>|*\"");
        assert!(!s.contains('/'));
        assert!(!s.contains('\\'));
        assert!(!s.contains(':'));
        assert!(!s.contains('?'));
        assert!(!s.contains('*'));
        assert!(!s.contains('"'));
        assert!(!s.contains('<'));
        assert!(!s.contains('>'));
        assert!(!s.contains('|'));
    }

    #[test]
    fn sanitize_filename_truncates_to_60_chars() {
        let long = "a".repeat(120);
        let out = sanitize_filename(&long);
        // 60-char max via `take(60)`. The trim() at the end may yield ≤60.
        assert!(out.chars().count() <= 60);
    }

    #[test]
    fn sanitize_filename_keeps_safe_chars() {
        assert_eq!(sanitize_filename("Contract Draft 2025-Q1"), "Contract Draft 2025-Q1");
        assert_eq!(sanitize_filename("invoice_#42"), "invoice_#42".replace('#', "_"));
    }

    #[test]
    fn extract_text_handles_text_formats() {
        assert_eq!(extract_text("txt", "x.txt", b"hello"), "hello");
        assert_eq!(extract_text("md", "x.md", b"# title"), "# title");
        assert_eq!(extract_text("csv", "x.csv", b"a,b,c\n1,2,3"), "a,b,c\n1,2,3");
    }

    #[test]
    fn extract_text_unknown_format_returns_empty() {
        assert_eq!(extract_text("zip", "x.zip", b"PK\x03\x04"), "");
        assert_eq!(extract_text("", "x", b"data"), "");
    }
}
