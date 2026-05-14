//! DOCX template registry — sidecar JSON files under
//! `config/docx-templates/<domain>/<slug>.json`, each paired with a
//! Word `.dotx` template file of the same stem.
//!
//! Authoritative spec: [`docs/TEMPLATE_PRONTUARIO.md`]. Each entry
//! here corresponds to one of the schede in the Prontuario and is
//! referenced back via the optional `source_reference` field.
//!
//! Philosophy (from Panucci, restated in the Prontuario):
//!
//!   > Il Prontuario non serve per generare il contenuto. Quello si
//!   > ottiene dialogando con Claude, iterando, affinando. Il
//!   > Prontuario entra in gioco alla fine, quando il contenuto è
//!   > pronto e devi trasformarlo in un documento stampabile.
//!
//! The template is the **closing formatter**, never the content
//! generator. The LLM produces structured Markdown after iterating
//! with the user; the renderer applies the right `.dotx`, binds
//! `[PLACEHOLDERS]`, and emits a print-ready `.docx`.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

// ─────────────────────────────────────────────────────────────────────
// Layout primitives
// ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Paper {
    pub size: String,
    #[serde(default = "default_orientation")]
    pub orientation: String,
    /// `"standard"` for A4 ordinary, `"uso_bollo"` for notarial deeds.
    /// When `uso_bollo`, the sibling `uso_bollo` block becomes required.
    #[serde(default = "default_paper_format")]
    pub format: String,
}

fn default_orientation() -> String {
    "portrait".to_string()
}

fn default_paper_format() -> String {
    "standard".to_string()
}

/// Special paper rules for notarial "uso bollo" deeds. Only present
/// when `paper.format == "uso_bollo"`. Captures the constraints listed
/// in Prontuario scheda 6: 25 lines per facciata, mirror margins,
/// no blank lines allowed, marginal signature on every page except
/// the last.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UsoBollo {
    /// Line spacing in typographic points (not `1.5` multiplier).
    /// Standard is `28.35` for the canonical 25-lines/facciata layout.
    pub line_spacing_pt_exact: f32,
    pub lines_per_facciata: u32,
    pub facciate_per_foglio: u32,
    #[serde(default)]
    pub mirror_margins: bool,
    #[serde(default)]
    pub duplex: bool,
    #[serde(default)]
    pub forbid_empty_lines: bool,
    #[serde(default)]
    pub marginal_signature_required: bool,
    #[serde(default)]
    pub signature_exclude_last_page: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MarginsCm {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Typography {
    pub body_font: String,
    pub body_size_pt: f32,
    pub line_spacing: f32,
    #[serde(default)]
    pub paragraph_after_pt: f32,
    /// `"justify"` (legal/forense) or `"left"` (PA blocco americano).
    #[serde(default = "default_alignment")]
    pub alignment: String,
    #[serde(default)]
    pub first_line_indent_cm: f32,
}

fn default_alignment() -> String {
    "justify".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Footnotes {
    pub font: String,
    pub size_pt: f32,
    pub line_spacing: f32,
}

// ─────────────────────────────────────────────────────────────────────
// Authoring contract
// ─────────────────────────────────────────────────────────────────────

/// One step in the document's expected structure. The `id` is the
/// canonical English snake_case identifier (memory: English IDs,
/// localised display). The `title` is the heading text rendered into
/// the Word document — typically Italian for `it/` templates, but the
/// field carries whatever the template author wrote.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SectionSkeletonEntry {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Literal text to render in place of a heading — e.g. `"* * *"`
    /// for the inter-block separator used in atto difensivo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guidance: Option<String>,
    /// When `true`, this section is a *repeating block* (Prontuario
    /// L3 automation) — e.g. one quesito → one risposta in CTU, one
    /// process card → one row in ISO. Renderer expects the LLM to
    /// produce a list under this section in the Markdown.
    #[serde(default)]
    pub repeating: bool,
}

/// Character-count vincoli, used by atto difensivo (D.M. 110/2023).
/// Map of `atto_type` → max chars. Renderer warns when body exceeds.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CharacterLimits {
    /// Map preserved verbatim so future `atto_type` values can be
    /// added without recompiling.
    #[serde(flatten)]
    pub by_atto_type: std::collections::HashMap<String, u64>,
}

/// Few-shot example pointing at a Markdown file in the same template
/// directory. Loaded lazily when the LLM asks for examples via
/// `describe_docx_template`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FewShotExample {
    pub label: String,
    /// Path relative to the template's sidecar JSON.
    pub path: String,
}

// ─────────────────────────────────────────────────────────────────────
// DocxTemplate root
// ─────────────────────────────────────────────────────────────────────

/// One template as parsed from disk. Every field except the bare
/// minimum (`id`, `display_name`, `paper`, `margins_cm`, `typography`)
/// is optional so a new template can be drafted incrementally.
///
/// Serialisation back to JSON via `to_api_json()` adds the synthesised
/// fields (`is_system: true`) that the route returns to the frontend.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DocxTemplate {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,

    pub id: String,
    /// Map of locale code → display name. Renderer picks the entry
    /// matching the user's UI locale; falls back to `en` then to the
    /// first available entry.
    pub display_name: std::collections::HashMap<String, String>,

    pub category: String,
    /// Canonical domain enum value — see `crate::domain::DOMAINS`.
    pub domain: String,
    pub locale: String,

    #[serde(default = "default_automation_level")]
    pub automation_level: String,
    #[serde(default = "default_placeholder_syntax")]
    pub placeholder_syntax: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_reference: Option<String>,

    // ── layout ──────────────────────────────────────────────────────
    pub paper: Paper,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uso_bollo: Option<UsoBollo>,
    pub margins_cm: MarginsCm,
    pub typography: Typography,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footnotes: Option<Footnotes>,

    /// Universal baseline (4 styles from Prontuario Parte III.1).
    /// Keys are canonical English IDs; values are the Word style names
    /// embedded in the companion `.dotx` (localised per template).
    #[serde(default = "default_style_map_baseline")]
    pub style_map_baseline: std::collections::BTreeMap<String, String>,

    /// Template-specific style overrides on top of the baseline.
    #[serde(default)]
    pub style_map: std::collections::BTreeMap<String, String>,

    #[serde(default)]
    pub directives_supported: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header_block: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer_block: Option<String>,
    /// `"manual"` (the LLM writes `1.`, `2.` in the heading text) or
    /// `"auto"` (the template uses Word numbering definitions).
    #[serde(default = "default_section_numbering")]
    pub section_numbering: String,

    // ── authoring contract ──────────────────────────────────────────
    #[serde(default)]
    pub section_skeleton: Vec<SectionSkeletonEntry>,

    /// Per-field micro-prompts for the LLM, telling it how to extract
    /// each required_metadata field from a chat conversation.
    #[serde(default)]
    pub field_prompts: std::collections::BTreeMap<String, String>,

    /// Names of metadata fields the LLM must collect before the
    /// renderer runs. Universal fields (LUOGO, DATA, MITTENTE, …) are
    /// always implicitly required and don't need to be listed here.
    #[serde(default)]
    pub required_metadata: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_limits: Option<CharacterLimits>,

    #[serde(default)]
    pub few_shot_examples: Vec<FewShotExample>,

    /// Optional author override appended to the auto-generated
    /// `prompt_md`. Use for jurisdiction-specific tone notes that
    /// don't fit cleanly in `field_prompts`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_md_extra: Option<String>,
}

fn default_schema_version() -> u32 {
    1
}

fn default_automation_level() -> String {
    "L1".to_string()
}

fn default_placeholder_syntax() -> String {
    "square_brackets".to_string()
}

fn default_section_numbering() -> String {
    "manual".to_string()
}

/// The 4 canonical paragraph styles every template inherits. Keys are
/// the IDs the renderer references; values are what the companion
/// `.dotx` actually defines (will be `"Corpo testo"` etc. for IT
/// templates, `"Body Text"` for future EN ones).
fn default_style_map_baseline() -> std::collections::BTreeMap<String, String> {
    let mut m = std::collections::BTreeMap::new();
    m.insert("body_text".to_string(), "Corpo testo".to_string());
    m.insert("section_heading".to_string(), "Titolo sezione".to_string());
    m.insert("citation".to_string(), "Citazione".to_string());
    m.insert("footnote".to_string(), "Note piè pagina".to_string());
    m
}

impl DocxTemplate {
    /// Resolve a display name for the given locale, with English
    /// fallback and last-resort first-entry pickup.
    pub fn display_name_for(&self, locale: &str) -> String {
        if let Some(name) = self.display_name.get(locale) {
            return name.clone();
        }
        if let Some(name) = self.display_name.get("en") {
            return name.clone();
        }
        self.display_name
            .values()
            .next()
            .cloned()
            .unwrap_or_else(|| self.id.clone())
    }

    /// Render the template as the JSON shape the `/docx-templates`
    /// endpoint serves. Adds synthesised fields (`is_system: true`,
    /// `is_owner: false`) so consumers see the same schema as future
    /// user-created rows from the DB.
    pub fn to_api_json(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or(serde_json::json!({}));
        if let Value::Object(ref mut map) = v {
            map.insert("is_system".to_string(), Value::Bool(true));
            map.insert("is_owner".to_string(), Value::Bool(false));
        }
        v
    }

    /// Compose the system-prompt block that teaches an LLM how to
    /// write a document for this specific template. Derived
    /// **entirely** from the structured sidecar fields — margins,
    /// typography, section skeleton, required metadata — so a change
    /// to the JSON propagates to the prompt at the next restart
    /// without manual edits. The author can still append a free-form
    /// `prompt_md_extra` block for jurisdiction-specific tone notes.
    ///
    /// Output mirrors the "Schema del prompt di formattazione" in
    /// `docs/TEMPLATE_PRONTUARIO.md` Parte V — same shape, same
    /// section headers, same placeholder-syntax instruction. The LLM
    /// reads this as the closing-formatter contract for the
    /// document type it's about to produce.
    pub fn auto_generated_prompt_md(&self, locale: &str) -> String {
        let mut out = String::with_capacity(2048);
        let kind = self.display_name_for(locale);
        out.push_str(&format!("Generate a Word (.docx) document for: **{kind}**.\n\n"));

        // ── Source reference (so the LLM knows where the spec lives).
        if let Some(src) = &self.source_reference {
            out.push_str(&format!("Authoritative spec: {src}\n\n"));
        }

        // ── Layout block.
        out.push_str("LAYOUT (rendered automatically by the docx engine — do NOT include manual page-setup instructions in your output):\n");
        out.push_str(&format!(
            "- Paper: {} {}\n",
            self.paper.size, self.paper.orientation
        ));
        if self.paper.format != "standard" {
            out.push_str(&format!("- Special format: {}\n", self.paper.format));
        }
        out.push_str(&format!(
            "- Margins (cm): top {} / right {} / bottom {} / left {}\n",
            self.margins_cm.top,
            self.margins_cm.right,
            self.margins_cm.bottom,
            self.margins_cm.left,
        ));
        out.push_str(&format!(
            "- Body font: {} {}pt, line spacing {}, alignment {}\n",
            self.typography.body_font,
            self.typography.body_size_pt,
            self.typography.line_spacing,
            self.typography.alignment,
        ));
        if let Some(f) = &self.footnotes {
            out.push_str(&format!(
                "- Footnotes: {} {}pt, line spacing {}\n",
                f.font, f.size_pt, f.line_spacing
            ));
        }
        out.push('\n');

        // ── Placeholder convention (always square brackets per the
        //    Prontuario, but we read it from the field so a future
        //    template variant can opt into Jinja or DOCPROPERTY).
        out.push_str(&format!(
            "PLACEHOLDERS: use the `{}` convention — e.g. `[NOME]`, \
             `[DATA]`, `[PARTE_ASSISTITA.CF]`. Tokens are uppercase \
             with `_` and `.` allowed. The docx engine substitutes \
             every `[NAME]` against a metadata bag at render time. \
             Tokens that don't match a bag key are left verbatim in \
             the final document so the user sees the gap during \
             proofread.\n\n",
            self.placeholder_syntax,
        ));

        // ── Required metadata that the call to generate_docx must
        //    carry. Universal fields (LUOGO, DATA, MITTENTE, etc.)
        //    are inherited and always required — they're not listed
        //    here because every template needs them.
        if !self.required_metadata.is_empty() {
            out.push_str("REQUIRED METADATA (must be present in the `metadata` argument when calling `generate_docx`):\n");
            for field in &self.required_metadata {
                if let Some(hint) = self.field_prompts.get(field) {
                    out.push_str(&format!("- `{field}` — {hint}\n"));
                } else {
                    out.push_str(&format!("- `{field}`\n"));
                }
            }
            out.push('\n');
        }

        // ── Section skeleton (the structural blueprint).
        if !self.section_skeleton.is_empty() {
            out.push_str("SECTION SKELETON (emit sections in this order, using Markdown headings for titles):\n");
            for entry in &self.section_skeleton {
                let title = entry.title.as_deref().unwrap_or("");
                let render = entry.render.as_deref().unwrap_or("");
                let rep = if entry.repeating { " [REPEATING BLOCK]" } else { "" };
                let label = if !title.is_empty() {
                    format!("**{title}**{rep}")
                } else if !render.is_empty() {
                    format!("literal: `{render}`{rep}")
                } else {
                    format!("`{}`{rep}", entry.id)
                };
                out.push_str(&format!("- {label}"));
                if let Some(g) = &entry.guidance {
                    out.push_str(&format!(" — {g}"));
                }
                out.push('\n');
            }
            out.push('\n');
        }

        // ── Character limits (D.M. 110/2023 — atti difensivi only,
        //    but the field is generic).
        if let Some(limits) = &self.character_limits {
            out.push_str("CHARACTER LIMITS (apply by `atto_type` value):\n");
            let mut entries: Vec<(&String, &u64)> = limits.by_atto_type.iter().collect();
            entries.sort_by_key(|(k, _)| k.as_str());
            for (k, v) in entries {
                out.push_str(&format!("- `{k}`: max {v} characters\n"));
            }
            out.push_str("Exceeding the limit produces no invalidity but may be sanctioned by the judge — self-moderate.\n\n");
        }

        // ── Author override block (free-form tone notes).
        if let Some(extra) = &self.prompt_md_extra {
            out.push_str("ADDITIONAL AUTHOR NOTES:\n");
            out.push_str(extra.trim());
            out.push_str("\n\n");
        }

        out.push_str(&format!(
            "When ready, call `generate_docx(template_id=\"{}\", body_md=..., metadata=...)`. \
             Do NOT include layout instructions in the body — the engine handles them. \
             Write Markdown body content, no front-matter.\n",
            self.id,
        ));

        out
    }
}

// ─────────────────────────────────────────────────────────────────────
// Loader
// ─────────────────────────────────────────────────────────────────────

/// Walk every JSON file in `dir` (one level of subdirectory recursion
/// for domain folders), parse each as a `DocxTemplate`, validate
/// minimal invariants. Broken files are skipped with a `tracing::warn`
/// — one bad template doesn't take down the rest.
pub fn load_docx_templates(dir: &Path) -> Result<Vec<DocxTemplate>> {
    let mut out: Vec<DocxTemplate> = Vec::new();
    let files = match super::collect_json_files(dir) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!(
                "[docx-templates] directory {} not found; no templates loaded",
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
                    "[docx-templates] skip {} (read error): {}",
                    path.display(),
                    e
                );
                continue;
            }
        };
        match serde_json::from_slice::<DocxTemplate>(&bytes) {
            Ok(t) => {
                if let Err(reason) = validate(&t) {
                    tracing::warn!(
                        "[docx-templates] skip {}: {reason}",
                        path.display(),
                    );
                    continue;
                }
                tracing::info!(
                    "[docx-templates] loaded {} (domain={}, locale={}, L={})",
                    t.id,
                    t.domain,
                    t.locale,
                    t.automation_level,
                );
                out.push(t);
            }
            Err(e) => {
                tracing::warn!(
                    "[docx-templates] skip {} (parse error): {}",
                    path.display(),
                    e
                );
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Minimum invariants every loaded template must satisfy.
fn validate(t: &DocxTemplate) -> Result<(), String> {
    if t.id.is_empty() {
        return Err("empty id".into());
    }
    if t.display_name.is_empty() {
        return Err(format!("template {}: display_name map is empty", t.id));
    }
    if !crate::domain::is_valid(&t.domain) {
        return Err(format!(
            "template {}: domain {} not in canonical set",
            t.id, t.domain
        ));
    }
    if !matches!(t.automation_level.as_str(), "L1" | "L2" | "L3" | "L4") {
        return Err(format!(
            "template {}: automation_level {} not in [L1,L2,L3,L4]",
            t.id, t.automation_level
        ));
    }
    if t.paper.format == "uso_bollo" && t.uso_bollo.is_none() {
        return Err(format!(
            "template {}: paper.format=uso_bollo requires uso_bollo block",
            t.id
        ));
    }
    if !matches!(t.placeholder_syntax.as_str(), "square_brackets" | "docproperty" | "jinja") {
        return Err(format!(
            "template {}: placeholder_syntax {} unsupported",
            t.id, t.placeholder_syntax
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_template_json(id: &str, domain: &str) -> String {
        format!(
            r#"{{
                "id": "{id}",
                "display_name": {{ "it": "Test {id}", "en": "Test {id}" }},
                "category": "legal",
                "domain": "{domain}",
                "locale": "it-IT",
                "paper": {{ "size": "A4" }},
                "margins_cm": {{ "top": 2.5, "right": 2.5, "bottom": 2.5, "left": 3.0 }},
                "typography": {{ "body_font": "Times New Roman", "body_size_pt": 12.0, "line_spacing": 1.5 }}
            }}"#
        )
    }

    #[test]
    fn parses_minimal_template() {
        let json = minimal_template_json("it/test", "legal");
        let t: DocxTemplate = serde_json::from_str(&json).expect("parse minimal");
        assert_eq!(t.id, "it/test");
        assert_eq!(t.domain, "legal");
        assert_eq!(t.automation_level, "L1"); // default
        assert_eq!(t.placeholder_syntax, "square_brackets"); // default
        assert_eq!(t.paper.format, "standard"); // default
        assert_eq!(t.style_map_baseline.len(), 4); // baseline always present
        assert!(t.style_map_baseline.contains_key("body_text"));
        assert!(t.style_map_baseline.contains_key("section_heading"));
        assert!(t.style_map_baseline.contains_key("citation"));
        assert!(t.style_map_baseline.contains_key("footnote"));
    }

    #[test]
    fn validate_rejects_invalid_domain() {
        let mut t: DocxTemplate =
            serde_json::from_str(&minimal_template_json("it/test", "legal")).unwrap();
        t.domain = "made_up_domain".into();
        let err = validate(&t).unwrap_err();
        assert!(err.contains("domain"));
    }

    #[test]
    fn validate_rejects_invalid_automation_level() {
        let mut t: DocxTemplate =
            serde_json::from_str(&minimal_template_json("it/test", "legal")).unwrap();
        t.automation_level = "L9".into();
        let err = validate(&t).unwrap_err();
        assert!(err.contains("automation_level"));
    }

    #[test]
    fn validate_rejects_uso_bollo_without_block() {
        let mut t: DocxTemplate =
            serde_json::from_str(&minimal_template_json("it/test", "legal")).unwrap();
        t.paper.format = "uso_bollo".into();
        // uso_bollo block missing
        let err = validate(&t).unwrap_err();
        assert!(err.contains("uso_bollo"));
    }

    #[test]
    fn validate_accepts_uso_bollo_with_block() {
        let mut t: DocxTemplate =
            serde_json::from_str(&minimal_template_json("it/test", "legal")).unwrap();
        t.paper.format = "uso_bollo".into();
        t.uso_bollo = Some(UsoBollo {
            line_spacing_pt_exact: 28.35,
            lines_per_facciata: 25,
            facciate_per_foglio: 4,
            mirror_margins: true,
            duplex: true,
            forbid_empty_lines: true,
            marginal_signature_required: true,
            signature_exclude_last_page: true,
        });
        assert!(validate(&t).is_ok());
    }

    #[test]
    fn display_name_falls_back_to_english_then_first() {
        let t: DocxTemplate =
            serde_json::from_str(&minimal_template_json("it/test", "legal")).unwrap();
        assert_eq!(t.display_name_for("it"), "Test it/test");
        assert_eq!(t.display_name_for("en"), "Test it/test");
        // Locale we don't have → fallback to en
        assert_eq!(t.display_name_for("ja"), "Test it/test");
    }

    #[test]
    fn to_api_json_marks_system() {
        let t: DocxTemplate =
            serde_json::from_str(&minimal_template_json("it/test", "legal")).unwrap();
        let v = t.to_api_json();
        assert_eq!(v["is_system"], serde_json::json!(true));
        assert_eq!(v["is_owner"], serde_json::json!(false));
        assert_eq!(v["id"], serde_json::json!("it/test"));
    }

    #[test]
    fn shipped_templates_all_load_cleanly() {
        // Integration test: every JSON under config/docx-templates/
        // that ships with the repo must parse + validate. Catches
        // typos and schema drift on every CI run.
        let dir = crate::presets::config_subdir("docx-templates");
        if !dir.exists() {
            // Skip silently when running from a stripped checkout
            // without the config tree (e.g. `cargo publish` package).
            return;
        }
        let templates =
            load_docx_templates(&dir).expect("shipped templates must load");

        // Phase 1.A acceptance: the 4 ★★★★★ templates from the Prontuario
        // must all be present and valid after a fresh build.
        let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
        for expected in [
            "it/diffida-messa-in-mora",
            "it/parcella-professionale",
            "it/contratto-locazione",
            "compliance/procedura-iso-sgi",
        ] {
            assert!(
                ids.contains(&expected),
                "missing shipped template {expected} (found: {ids:?})"
            );
        }
    }

    #[test]
    fn auto_generated_prompt_md_contains_layout_and_skeleton() {
        // Use the actual shipped Diffida template — anchors the test
        // to a representative real-world sidecar.
        let dir = crate::presets::config_subdir("docx-templates");
        let templates = load_docx_templates(&dir).expect("load");
        let diffida = templates
            .iter()
            .find(|t| t.id == "it/diffida-messa-in-mora")
            .expect("diffida present");
        let prompt = diffida.auto_generated_prompt_md("it");

        // Layout invariants come straight from the sidecar fields.
        assert!(prompt.contains("Diffida"), "display name missing");
        assert!(prompt.contains("Paper: A4"), "paper missing");
        assert!(prompt.contains("Calibri"), "font missing");
        assert!(prompt.contains("11pt") || prompt.contains("11 pt"));
        // Authoritative spec back-reference resolves to the Prontuario.
        assert!(prompt.contains("TEMPLATE_PRONTUARIO.md"));
        // Required metadata listed with field_prompts as hints.
        assert!(prompt.contains("`DEBITORE`"));
        assert!(prompt.contains("`IMPORTO`"));
        assert!(prompt.contains("`TERMINE_GG`"));
        // Section skeleton present.
        assert!(prompt.contains("DIFFIDA E METTE IN MORA"));
        // Closing instruction telling the LLM how to call the tool.
        assert!(prompt.contains("generate_docx"));
        assert!(prompt.contains(r#"template_id="it/diffida-messa-in-mora""#));
        // Placeholder convention echoed.
        assert!(prompt.contains("square_brackets"));
    }

    #[test]
    fn auto_generated_prompt_md_marks_repeating_blocks() {
        // Parcella has a `voci_onorario` repeating section.
        let dir = crate::presets::config_subdir("docx-templates");
        let templates = load_docx_templates(&dir).expect("load");
        let parcella = templates
            .iter()
            .find(|t| t.id == "it/parcella-professionale")
            .expect("parcella present");
        let prompt = parcella.auto_generated_prompt_md("it");
        assert!(prompt.contains("[REPEATING BLOCK]"));
    }

    // ── auto_generated_prompt_md edge cases on hand-rolled fixtures ──

    /// Build a maximally-minimal template — only the fields parse()
    /// would require — to test the "no optionals" path of the prompt
    /// generator.
    fn bare_template() -> DocxTemplate {
        let json = minimal_template_json("it/bare", "legal");
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn prompt_md_omits_sections_for_empty_optionals() {
        // A bare template has no source_reference, no
        // character_limits, no section_skeleton, no required_metadata,
        // no prompt_md_extra. The generator must omit those headers,
        // not emit empty ones.
        let t = bare_template();
        let prompt = t.auto_generated_prompt_md("it");
        // Headers absent
        assert!(!prompt.contains("Authoritative spec:"));
        assert!(!prompt.contains("REQUIRED METADATA"));
        assert!(!prompt.contains("SECTION SKELETON"));
        assert!(!prompt.contains("CHARACTER LIMITS"));
        assert!(!prompt.contains("ADDITIONAL AUTHOR NOTES"));
        // The closing instruction is always present.
        assert!(prompt.contains("generate_docx"));
    }

    #[test]
    fn prompt_md_emits_character_limits_sorted() {
        let mut t = bare_template();
        let mut limits = std::collections::HashMap::new();
        limits.insert("note_udienza".to_string(), 10000u64);
        limits.insert("atto_di_citazione".to_string(), 80000u64);
        limits.insert("memoria_ex_art_183_cpc".to_string(), 50000u64);
        t.character_limits = Some(CharacterLimits { by_atto_type: limits });

        let prompt = t.auto_generated_prompt_md("it");
        assert!(prompt.contains("CHARACTER LIMITS"));
        assert!(prompt.contains("`atto_di_citazione`: max 80000"));
        assert!(prompt.contains("`memoria_ex_art_183_cpc`: max 50000"));
        assert!(prompt.contains("`note_udienza`: max 10000"));
        // Alphabetic order: 'a' < 'm' < 'n'. Find each substring and
        // assert their relative position.
        let pos_a = prompt.find("atto_di_citazione").unwrap();
        let pos_m = prompt.find("memoria_ex_art_183_cpc").unwrap();
        let pos_n = prompt.find("note_udienza").unwrap();
        assert!(pos_a < pos_m && pos_m < pos_n, "limits must be alphabetically sorted");
    }

    #[test]
    fn prompt_md_appends_author_override_when_present() {
        let mut t = bare_template();
        t.prompt_md_extra =
            Some("Use 'all'Ill.mo Tribunale adito' without 'contrariis reiectis'.".into());
        let prompt = t.auto_generated_prompt_md("it");
        assert!(prompt.contains("ADDITIONAL AUTHOR NOTES"));
        assert!(prompt.contains("all'Ill.mo Tribunale adito"));
    }

    #[test]
    fn prompt_md_handles_section_skeleton_with_literal_render() {
        // A section with `render` (literal text, e.g. "* * *") and no
        // title should be rendered as `literal: ...` so the LLM
        // knows to emit the verbatim string.
        let mut t = bare_template();
        t.section_skeleton = vec![
            SectionSkeletonEntry {
                id: "in_fatto".into(),
                title: Some("IN FATTO".into()),
                render: None,
                guidance: Some("Esposizione fatti.".into()),
                repeating: false,
            },
            SectionSkeletonEntry {
                id: "separator".into(),
                title: None,
                render: Some("* * *".into()),
                guidance: None,
                repeating: false,
            },
        ];
        let prompt = t.auto_generated_prompt_md("it");
        assert!(prompt.contains("**IN FATTO**"));
        assert!(prompt.contains("Esposizione fatti."));
        assert!(prompt.contains("literal: `* * *`"));
    }

    #[test]
    fn prompt_md_field_prompts_attached_to_required_metadata() {
        let mut t = bare_template();
        t.required_metadata = vec!["DEBITORE".into(), "IMPORTO".into()];
        t.field_prompts.insert(
            "DEBITORE".into(),
            "Nome o ragione sociale del debitore.".into(),
        );
        // IMPORTO without a field_prompts entry — should still appear
        // in the prompt, just without the hint.
        let prompt = t.auto_generated_prompt_md("it");
        assert!(prompt.contains("`DEBITORE` — Nome o ragione sociale del debitore."));
        // IMPORTO line: id present, no em-dash hint.
        let importo_line = prompt
            .lines()
            .find(|l| l.contains("`IMPORTO`"))
            .expect("IMPORTO listed");
        assert!(!importo_line.contains(" — "), "IMPORTO line should not carry a hint dash");
    }

    #[test]
    fn prompt_md_uses_locale_for_display_name() {
        let mut t = bare_template();
        t.display_name.insert("en".to_string(), "Bare template (EN)".to_string());
        let it = t.auto_generated_prompt_md("it");
        // "it" → Italian display name from minimal_template_json
        assert!(it.contains("Test it/bare"));
        let en = t.auto_generated_prompt_md("en");
        assert!(en.contains("Bare template (EN)"));
    }

    #[test]
    fn prompt_md_mentions_uso_bollo_special_format() {
        let mut t = bare_template();
        t.paper.format = "uso_bollo".into();
        t.uso_bollo = Some(UsoBollo {
            line_spacing_pt_exact: 28.35,
            lines_per_facciata: 25,
            facciate_per_foglio: 4,
            mirror_margins: true,
            duplex: true,
            forbid_empty_lines: true,
            marginal_signature_required: true,
            signature_exclude_last_page: true,
        });
        let prompt = t.auto_generated_prompt_md("it");
        // Special-format line surfaces the variant name.
        assert!(prompt.contains("Special format: uso_bollo"));
    }

    #[test]
    fn character_limits_parses_flexible_map() {
        let json = r#"{
            "id": "it/atto",
            "display_name": { "it": "Atto" },
            "category": "legal",
            "domain": "legal",
            "locale": "it-IT",
            "paper": { "size": "A4" },
            "margins_cm": { "top": 3.0, "right": 2.0, "bottom": 2.5, "left": 3.5 },
            "typography": { "body_font": "Times New Roman", "body_size_pt": 12.0, "line_spacing": 1.5 },
            "character_limits": {
                "atto_di_citazione": 80000,
                "memoria_ex_art_183_cpc": 50000,
                "note_udienza": 10000
            }
        }"#;
        let t: DocxTemplate = serde_json::from_str(json).expect("parse");
        let limits = t.character_limits.expect("has limits");
        assert_eq!(limits.by_atto_type["atto_di_citazione"], 80000);
        assert_eq!(limits.by_atto_type["memoria_ex_art_183_cpc"], 50000);
    }
}
