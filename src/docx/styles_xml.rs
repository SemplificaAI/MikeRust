//! Build `word/styles.xml` from a `DocxTemplate` sidecar.
//!
//! Every paragraph in the generated document references its style by
//! `w:styleId` (a stable, ASCII-only Word-internal id). The renderer
//! defines one `<w:style>` per canonical English ID (`body_text`,
//! `section_heading`, `citation`, `footnote`, …); the localised
//! human-readable `w:name` (e.g. "Corpo testo") comes from the
//! sidecar's `style_map_baseline` / `style_map`. Word's UI shows the
//! `w:name`; the document.xml engine references the `w:styleId`.
//!
//! This is the place where every OOXML unit conversion happens:
//!   - point sizes → half-points (`12 pt` → `"24"`)
//!   - line spacing multiplier → 240-based line value
//!     (`1.5` → `"360"` with line_rule="auto")
//!   - cm margins → twips (`3.5 cm` → `"1984"`, see `cm_to_twips`)
//!
//! Pure function: no I/O, no global state. Output is a complete,
//! self-contained styles.xml ready to drop into the zip.

use crate::presets::docx_template::{DocxTemplate, Typography};

const W_NS: &str =
    r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

/// Canonical English style IDs every template inherits. Anything in
/// the sidecar's `style_map_baseline` or `style_map` map keyed on one
/// of these names becomes a `<w:style>` with the corresponding
/// styleId.
///
/// Kept in this fixed order so styles.xml output is deterministic —
/// golden-file tests can diff byte-for-byte across runs.
pub const BASELINE_STYLE_IDS: &[&str] =
    &["body_text", "section_heading", "citation", "footnote"];

/// Emit `word/styles.xml` for the given template.
///
/// The returned string is a complete, well-formed XML document. The
/// renderer does no further wrapping — it goes straight into the zip.
pub fn build_styles_xml(template: &DocxTemplate) -> String {
    let mut out = String::with_capacity(4096);
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    out.push('\n');
    out.push_str(&format!("<w:styles {W_NS}>"));
    out.push('\n');

    // ── Document defaults (font + size + base line spacing). Acts as
    // the inheritance root for every <w:style> below. Without
    // <w:docDefaults> Word falls back to "Calibri 11pt" hard-coded,
    // which would silently override our typography choices.
    push_doc_defaults(&mut out, &template.typography);

    // ── body_text first (it's the `w:default="1"` paragraph style).
    push_body_text_style(&mut out, template);

    // ── section_heading, citation, footnote
    push_section_heading_style(&mut out, template);
    push_citation_style(&mut out, template);
    push_footnote_style(&mut out, template);

    // ── Template-specific extras from `style_map` (anything that
    // isn't a baseline ID is rendered as a paragraph style based on
    // body_text, so authors can drop extras like `DiffidaIntestazione`
    // and the engine just makes them inherit sane defaults).
    for (id, name) in template.style_map.iter() {
        if BASELINE_STYLE_IDS.contains(&id.as_str()) {
            continue;
        }
        push_extra_style(&mut out, id, name);
    }

    out.push_str("</w:styles>");
    out
}

fn push_doc_defaults(out: &mut String, t: &Typography) {
    let sz_half_pts = pt_to_half_points(t.body_size_pt);
    let line_240 = line_spacing_to_240(t.line_spacing);
    let font = xml_escape(&t.body_font);
    out.push_str(&format!(
        r#"  <w:docDefaults>
    <w:rPrDefault>
      <w:rPr>
        <w:rFonts w:ascii="{font}" w:hAnsi="{font}" w:cs="{font}"/>
        <w:sz w:val="{sz_half_pts}"/>
        <w:szCs w:val="{sz_half_pts}"/>
      </w:rPr>
    </w:rPrDefault>
    <w:pPrDefault>
      <w:pPr>
        <w:spacing w:line="{line_240}" w:lineRule="auto" w:after="{after}"/>
        <w:jc w:val="{jc}"/>
      </w:pPr>
    </w:pPrDefault>
  </w:docDefaults>
"#,
        after = pt_to_twentieths(t.paragraph_after_pt),
        jc = alignment_to_jc(&t.alignment),
    ));
}

fn push_body_text_style(out: &mut String, template: &DocxTemplate) {
    let style_id = "BodyText";
    let style_name = resolve_style_name(template, "body_text");
    let indent = cm_to_twips(template.typography.first_line_indent_cm);
    let indent_xml = if indent > 0 {
        format!(r#"<w:ind w:firstLine="{indent}"/>"#)
    } else {
        String::new()
    };
    out.push_str(&format!(
        r#"  <w:style w:type="paragraph" w:default="1" w:styleId="{style_id}">
    <w:name w:val="{name}"/>
    <w:qFormat/>
    <w:pPr>{indent_xml}</w:pPr>
  </w:style>
"#,
        name = xml_escape(&style_name),
    ));
}

fn push_section_heading_style(out: &mut String, template: &DocxTemplate) {
    let style_name = resolve_style_name(template, "section_heading");
    // +2pt over body, bold, uppercase via `<w:caps/>`. Outline lvl 0
    // so it shows up in Word's navigation pane / TOC commands.
    let bump_sz = pt_to_half_points(template.typography.body_size_pt + 2.0);
    out.push_str(&format!(
        r#"  <w:style w:type="paragraph" w:styleId="SectionHeading">
    <w:name w:val="{name}"/>
    <w:basedOn w:val="BodyText"/>
    <w:next w:val="BodyText"/>
    <w:qFormat/>
    <w:pPr>
      <w:spacing w:before="240" w:after="120"/>
      <w:outlineLvl w:val="0"/>
      <w:keepNext/>
    </w:pPr>
    <w:rPr>
      <w:b/>
      <w:caps/>
      <w:sz w:val="{bump_sz}"/>
      <w:szCs w:val="{bump_sz}"/>
    </w:rPr>
  </w:style>
"#,
        name = xml_escape(&style_name),
    ));
}

fn push_citation_style(out: &mut String, template: &DocxTemplate) {
    let style_name = resolve_style_name(template, "citation");
    // -1pt body, italic, left-indent 1.5cm — matches the Prontuario
    // convention for legal citations and stralci giurisprudenziali.
    let drop_sz = pt_to_half_points((template.typography.body_size_pt - 1.0).max(8.0));
    let indent_15cm = cm_to_twips(1.5);
    out.push_str(&format!(
        r#"  <w:style w:type="paragraph" w:styleId="Citation">
    <w:name w:val="{name}"/>
    <w:basedOn w:val="BodyText"/>
    <w:next w:val="BodyText"/>
    <w:qFormat/>
    <w:pPr>
      <w:ind w:left="{indent_15cm}"/>
      <w:spacing w:line="240" w:lineRule="auto"/>
    </w:pPr>
    <w:rPr>
      <w:i/>
      <w:sz w:val="{drop_sz}"/>
      <w:szCs w:val="{drop_sz}"/>
    </w:rPr>
  </w:style>
"#,
        name = xml_escape(&style_name),
    ));
}

fn push_footnote_style(out: &mut String, template: &DocxTemplate) {
    let style_name = resolve_style_name(template, "footnote");
    let (font, size_pt, line_spacing) = template
        .footnotes
        .as_ref()
        .map(|f| (f.font.clone(), f.size_pt, f.line_spacing))
        .unwrap_or_else(|| {
            // Sensible default — Prontuario §1.2 says 10pt single
            // line for all categories.
            (template.typography.body_font.clone(), 10.0, 1.0)
        });
    let sz = pt_to_half_points(size_pt);
    let line_240 = line_spacing_to_240(line_spacing);
    let font_xml = xml_escape(&font);
    out.push_str(&format!(
        r#"  <w:style w:type="paragraph" w:styleId="Footnote">
    <w:name w:val="{name}"/>
    <w:basedOn w:val="BodyText"/>
    <w:next w:val="BodyText"/>
    <w:pPr>
      <w:spacing w:line="{line_240}" w:lineRule="auto" w:after="60"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="{font_xml}" w:hAnsi="{font_xml}" w:cs="{font_xml}"/>
      <w:sz w:val="{sz}"/>
      <w:szCs w:val="{sz}"/>
    </w:rPr>
  </w:style>
"#,
        name = xml_escape(&style_name),
    ));
}

fn push_extra_style(out: &mut String, id: &str, name: &str) {
    let style_id = sanitize_style_id(id);
    out.push_str(&format!(
        r#"  <w:style w:type="paragraph" w:styleId="{style_id}">
    <w:name w:val="{name}"/>
    <w:basedOn w:val="BodyText"/>
    <w:qFormat/>
  </w:style>
"#,
        name = xml_escape(name),
    ));
}

/// Pick the localised Word style name for a baseline ID, falling back
/// to `style_map_baseline` defaults if the template author didn't
/// override the value. The `style_map` table wins over
/// `style_map_baseline` when both define the same id (per-template
/// overrides take precedence over inherited defaults).
fn resolve_style_name(template: &DocxTemplate, id: &str) -> String {
    template
        .style_map
        .get(id)
        .or_else(|| template.style_map_baseline.get(id))
        .cloned()
        .unwrap_or_else(|| id.to_string())
}

// ─────────────────────────────────────────────────────────────────────
// Conversion helpers
// ─────────────────────────────────────────────────────────────────────

/// Convert points to half-points — OOXML's unit for `<w:sz>`.
/// `12pt` → `24`, `11pt` → `22`, `10pt` → `20`. Rounds to nearest.
pub fn pt_to_half_points(pt: f32) -> u32 {
    (pt * 2.0).round() as u32
}

/// Convert points to twentieths — used by `<w:spacing w:after>` and
/// page-margin twips. 1pt = 20 twips.
pub fn pt_to_twentieths(pt: f32) -> u32 {
    (pt * 20.0).round() as u32
}

/// Convert centimetres to twips for `<w:pgMar>`, `<w:ind w:left=...>`.
/// 1 cm = 567 twips approximately (1 inch / 2.54 × 1440). We compute
/// it precisely to avoid drift on small margins (3.5cm → 1984, not
/// 1980).
pub fn cm_to_twips(cm: f32) -> u32 {
    ((cm / 2.54) * 1440.0).round() as u32
}

/// Convert the sidecar's `line_spacing` multiplier (e.g. 1.0, 1.15,
/// 1.5, 2.0) to OOXML's 240-based `<w:spacing w:line=...>` value
/// when combined with `w:lineRule="auto"`.
pub fn line_spacing_to_240(multiplier: f32) -> u32 {
    (multiplier * 240.0).round() as u32
}

/// Translate sidecar alignment string into OOXML `<w:jc>` value.
/// Supports `"justify"` (legal/forense), `"left"` (blocco americano),
/// `"center"`, `"right"`. Unknown → `"left"`.
pub fn alignment_to_jc(alignment: &str) -> &'static str {
    match alignment {
        "justify" => "both",
        "right" => "right",
        "center" => "center",
        _ => "left",
    }
}

/// Word styleId grammar: ASCII letters + digits only. Underscores are
/// rejected by some Word versions; we map our snake_case canonical
/// IDs to PascalCase. `"body_text"` → `"BodyText"`.
pub fn sanitize_style_id(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    let mut capitalise_next = true;
    for ch in id.chars() {
        if ch.is_ascii_alphanumeric() {
            if capitalise_next {
                out.extend(ch.to_uppercase());
                capitalise_next = false;
            } else {
                out.push(ch);
            }
        } else {
            // _ - . space etc. — split point.
            capitalise_next = true;
        }
    }
    if out.is_empty() { "Style".to_string() } else { out }
}

/// Minimal XML attribute / text escape. Preserves the `&apos;` form
/// instead of `&#39;` — Word reads both, but the apos entity keeps
/// styles.xml identical to what manual authoring in Word produces.
pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn minimal_template() -> DocxTemplate {
        // Hand-roll a minimal template instead of going through the
        // parser — the unit under test is the XML emitter, decoupled
        // from sidecar parsing.
        let mut display = std::collections::HashMap::new();
        display.insert("it".to_string(), "Test".to_string());
        let mut baseline = BTreeMap::new();
        baseline.insert("body_text".to_string(), "Corpo testo".to_string());
        baseline.insert("section_heading".to_string(), "Titolo sezione".to_string());
        baseline.insert("citation".to_string(), "Citazione".to_string());
        baseline.insert("footnote".to_string(), "Note piè pagina".to_string());
        DocxTemplate {
            schema_version: 1,
            id: "it/test".to_string(),
            display_name: display,
            category: "test".to_string(),
            domain: "legal".to_string(),
            also_applicable_to: Vec::new(),
            locale: "it-IT".to_string(),
            automation_level: "L1".to_string(),
            placeholder_syntax: "square_brackets".to_string(),
            source_reference: None,
            paper: crate::presets::docx_template::Paper {
                size: "A4".to_string(),
                orientation: "portrait".to_string(),
                format: "standard".to_string(),
            },
            uso_bollo: None,
            margins_cm: crate::presets::docx_template::MarginsCm {
                top: 2.5,
                right: 2.5,
                bottom: 2.5,
                left: 3.5,
            },
            typography: Typography {
                body_font: "Times New Roman".to_string(),
                body_size_pt: 12.0,
                line_spacing: 1.5,
                paragraph_after_pt: 6.0,
                alignment: "justify".to_string(),
                first_line_indent_cm: 0.75,
            },
            footnotes: Some(crate::presets::docx_template::Footnotes {
                font: "Times New Roman".to_string(),
                size_pt: 10.0,
                line_spacing: 1.0,
            }),
            style_map_baseline: baseline,
            style_map: BTreeMap::new(),
            directives_supported: vec![],
            header_block: None,
            footer_block: None,
            section_numbering: "manual".to_string(),
            section_skeleton: vec![],
            field_prompts: BTreeMap::new(),
            required_metadata: vec![],
            character_limits: None,
            few_shot_examples: vec![],
            prompt_md_extra: None,
        }
    }

    #[test]
    fn pt_to_half_points_rounds_to_nearest() {
        assert_eq!(pt_to_half_points(12.0), 24);
        assert_eq!(pt_to_half_points(11.0), 22);
        assert_eq!(pt_to_half_points(10.5), 21);
        assert_eq!(pt_to_half_points(9.0), 18);
    }

    #[test]
    fn cm_to_twips_matches_word_canonical_values() {
        // Word's own twips values for round cm — observed by opening
        // a hand-set page in Word and dumping pgMar.
        assert_eq!(cm_to_twips(2.5), 1417);
        assert_eq!(cm_to_twips(3.0), 1701);
        assert_eq!(cm_to_twips(3.5), 1984);
    }

    #[test]
    fn line_spacing_to_240_handles_common_multipliers() {
        assert_eq!(line_spacing_to_240(1.0), 240);
        assert_eq!(line_spacing_to_240(1.15), 276);
        assert_eq!(line_spacing_to_240(1.5), 360);
        assert_eq!(line_spacing_to_240(2.0), 480);
    }

    #[test]
    fn alignment_translates_justify_to_both() {
        // OOXML uses `both` for full justification, not `justify`.
        assert_eq!(alignment_to_jc("justify"), "both");
        assert_eq!(alignment_to_jc("left"), "left");
        assert_eq!(alignment_to_jc("right"), "right");
        assert_eq!(alignment_to_jc("center"), "center");
        assert_eq!(alignment_to_jc("garbage"), "left"); // unknown → left
    }

    #[test]
    fn sanitize_style_id_produces_pascal_case() {
        assert_eq!(sanitize_style_id("body_text"), "BodyText");
        assert_eq!(sanitize_style_id("section_heading"), "SectionHeading");
        assert_eq!(sanitize_style_id("diffida_verb"), "DiffidaVerb");
        assert_eq!(sanitize_style_id("a-b.c d"), "ABCD");
    }

    #[test]
    fn sanitize_style_id_rejects_pure_garbage() {
        assert_eq!(sanitize_style_id("___"), "Style");
        assert_eq!(sanitize_style_id(""), "Style");
    }

    #[test]
    fn xml_escape_handles_ampersand_lt_gt_quotes_apos() {
        assert_eq!(xml_escape("A & B"), "A &amp; B");
        assert_eq!(xml_escape("<b>"), "&lt;b&gt;");
        assert_eq!(xml_escape(r#""hello""#), "&quot;hello&quot;");
        assert_eq!(xml_escape("L'avvocato"), "L&apos;avvocato");
    }

    // ── Full styles.xml emission ────────────────────────────────────

    #[test]
    fn styles_xml_starts_with_declaration_and_styles_root() {
        let xml = build_styles_xml(&minimal_template());
        assert!(xml.starts_with(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#));
        assert!(xml.contains(r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#));
        assert!(xml.ends_with("</w:styles>"));
    }

    #[test]
    fn styles_xml_includes_all_four_baseline_styles() {
        let xml = build_styles_xml(&minimal_template());
        // Each baseline style declared by both styleId and localised name.
        assert!(xml.contains(r#"w:styleId="BodyText""#));
        assert!(xml.contains(r#"w:val="Corpo testo""#));
        assert!(xml.contains(r#"w:styleId="SectionHeading""#));
        assert!(xml.contains(r#"w:val="Titolo sezione""#));
        assert!(xml.contains(r#"w:styleId="Citation""#));
        assert!(xml.contains(r#"w:val="Citazione""#));
        assert!(xml.contains(r#"w:styleId="Footnote""#));
        assert!(xml.contains(r#"w:val="Note pi&#232; pagina""#) || xml.contains("Note piè pagina"));
    }

    #[test]
    fn styles_xml_applies_typography_font_and_size() {
        let xml = build_styles_xml(&minimal_template());
        assert!(xml.contains(r#"w:ascii="Times New Roman""#));
        // 12pt = 24 half-points
        assert!(xml.contains(r#"w:val="24""#));
    }

    #[test]
    fn styles_xml_applies_first_line_indent_when_nonzero() {
        let xml = build_styles_xml(&minimal_template());
        // 0.75 cm = 425 twips
        let want = cm_to_twips(0.75);
        assert!(
            xml.contains(&format!(r#"w:firstLine="{want}""#)),
            "expected firstLine={want} in styles.xml"
        );
    }

    #[test]
    fn styles_xml_section_heading_is_bold_caps_and_bumped() {
        let xml = build_styles_xml(&minimal_template());
        // body 12pt → heading 14pt = 28 half-points
        let pos = xml.find(r#"w:styleId="SectionHeading""#).expect("heading present");
        let after = &xml[pos..];
        assert!(after.contains("<w:b/>"));
        assert!(after.contains("<w:caps/>"));
        assert!(after.contains(r#"w:val="28""#));
    }

    #[test]
    fn styles_xml_includes_extra_style_from_style_map() {
        let mut t = minimal_template();
        t.style_map.insert(
            "diffida_verb".to_string(),
            "Verbo centrale".to_string(),
        );
        let xml = build_styles_xml(&t);
        assert!(xml.contains(r#"w:styleId="DiffidaVerb""#));
        assert!(xml.contains(r#"w:val="Verbo centrale""#));
        // Extra style inherits from BodyText.
        let pos = xml.find(r#"w:styleId="DiffidaVerb""#).unwrap();
        let after = &xml[pos..];
        assert!(after.contains(r#"w:basedOn w:val="BodyText""#));
    }

    #[test]
    fn styles_xml_uses_blocco_americano_when_alignment_left() {
        let mut t = minimal_template();
        t.typography.alignment = "left".to_string();
        let xml = build_styles_xml(&t);
        assert!(xml.contains(r#"<w:jc w:val="left"/>"#));
    }
}
