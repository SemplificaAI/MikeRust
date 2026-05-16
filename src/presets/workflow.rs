//! Workflow preset registry — JSON files under `workflow-presets/<domain>/`.
//!
//! Each file declares one system-shipped workflow (the in-app
//! equivalent of the upstream-Mike "built-in" entries that used to
//! live in `frontend/.../builtinWorkflows.ts`). Drop a JSON into the
//! right domain subfolder and restart the app — the registry will
//! merge it into `/workflow` responses with `is_system: true` and a
//! `null` user_id so the existing UI grey-out logic kicks in.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// Single column definition inside a tabular workflow's
/// `columns_config`. Mirrors `ColumnConfig` in the frontend.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowColumn {
    pub index: i64,
    pub name: String,
    /// Per-cell extraction prompt. Free-form Markdown.
    pub prompt: String,
    /// One of: `text` | `bulleted_list` | `number` | `currency` |
    /// `monetary_amount` | `percentage` | `yes_no` | `date` | `tag`.
    /// Optional — defaults to `text` at consumer side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// One workflow preset loaded from JSON. The shape projects directly
/// into the MikeRust workflow JSON the `/workflow` endpoint emits — the
/// only extra fields on the wire (`is_system`, `created_at`,
/// `user_id`) are synthesised by the route handler when it merges
/// presets into a list response, so they don't appear here.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowPreset {
    /// Stable identifier. Convention: `builtin-<slug>`. Used as the
    /// row id on the wire and by the frontend to detect built-ins.
    pub id: String,
    pub title: String,
    /// One of `assistant` | `tabular`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Professional vertical — see `crate::domain::DOMAINS`. Required.
    pub domain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub practice: Option<String>,
    /// Free-form Markdown system prompt. Required for assistant
    /// workflows; for tabular workflows it sets the overall posture
    /// while each `columns_config` entry carries the per-cell prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_md: Option<String>,
    /// Tabular column schema. Optional (assistant workflows omit it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns_config: Option<Vec<WorkflowColumn>>,
    /// Optional DocxTemplate id this workflow's output should be
    /// formatted with. When set on an `assistant`-type workflow, the
    /// chat handler:
    ///   1. Injects the template's authoring prompt (auto-generated
    ///      from its sidecar fields) into the system message.
    ///   2. Encourages the LLM to finish the conversation by calling
    ///      `generate_docx(template_id=..., body_md=..., metadata=...)`.
    /// Omitted → workflow produces plain chat output; the user can
    /// still ask for a docx after the fact via the bare
    /// `generate_docx` tool. The wiring is opt-in per template.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_output_template: Option<String>,
}

impl WorkflowPreset {
    /// Render the preset as the JSON shape the `/workflow` endpoint
    /// serves to the frontend. Adds the synthesised fields (`is_system`,
    /// `user_id`, `created_at`, `is_owner`) so consumers see the same
    /// schema as user-created rows from the DB.
    pub fn to_api_json(&self) -> Value {
        let columns = self
            .columns_config
            .as_ref()
            .map(|cols| serde_json::to_value(cols).unwrap_or(serde_json::json!([])))
            .unwrap_or(serde_json::json!([]));
        serde_json::json!({
            "id": self.id,
            "user_id": null,
            "title": self.title,
            "type": self.kind,
            "prompt_md": self.prompt_md,
            "columns_config": columns,
            "practice": self.practice,
            "domain": self.domain,
            "default_output_template": self.default_output_template,
            "created_at": "",
            "is_system": true,
            "is_owner": false,
        })
    }
}

/// Walk every JSON file in `dir` (one level of subdirectory recursion
/// for domain folders), parse each as a `WorkflowPreset`, validate
/// minimal invariants. Broken files are skipped with a `tracing::warn`
/// — one bad preset doesn't take down the rest.
pub fn load_workflow_presets(dir: &Path) -> Result<Vec<WorkflowPreset>> {
    let mut out: Vec<WorkflowPreset> = Vec::new();
    let files = match super::collect_json_files(dir) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!(
                "[workflow-presets] directory {} not found; no presets loaded",
                dir.display()
            );
            return Ok(out);
        }
        Err(e) => return Err(anyhow::anyhow!("read {}: {}", dir.display(), e)),
    };

    for path in files {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(
                    "[workflow-presets] skip {} (read error): {}",
                    path.display(),
                    e
                );
                continue;
            }
        };
        match serde_json::from_slice::<WorkflowPreset>(&bytes) {
            Ok(p) => {
                if p.id.is_empty() || p.title.is_empty() {
                    tracing::warn!(
                        "[workflow-presets] skip {} (id/title empty)",
                        path.display()
                    );
                    continue;
                }
                if p.kind != "assistant" && p.kind != "tabular" {
                    tracing::warn!(
                        "[workflow-presets] skip {} (type {} not in [assistant, tabular])",
                        path.display(),
                        p.kind
                    );
                    continue;
                }
                if !crate::domain::is_valid(&p.domain) {
                    tracing::warn!(
                        "[workflow-presets] skip {} (domain {} not in canonical set)",
                        path.display(),
                        p.domain
                    );
                    continue;
                }
                tracing::info!(
                    "[workflow-presets] loaded {} ({}, {}, domain={})",
                    p.id,
                    p.kind,
                    p.title,
                    p.domain
                );
                out.push(p);
            }
            Err(e) => {
                tracing::warn!(
                    "[workflow-presets] skip {} (parse error): {}",
                    path.display(),
                    e
                );
            }
        }
    }

    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}
