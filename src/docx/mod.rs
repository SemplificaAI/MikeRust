//! DOCX rendering subsystem.
//!
//! Philosophy (from `docs/TEMPLATE_PRONTUARIO.md`, citing G. Panucci):
//!
//!   > Il Prontuario non serve per generare il contenuto. Quello si
//!   > ottiene dialogando con Claude, iterando, affinando. Il
//!   > Prontuario entra in gioco alla fine, quando il contenuto è
//!   > pronto e devi trasformarlo in un documento stampabile.
//!
//! This module is the *closing formatter* — never the content
//! generator. The LLM produces structured Markdown after iterating
//! with the user; this module's job is to apply the sidecar template
//! (layout + styles + placeholders), and emit a print-ready `.docx`
//! that respects the Italian professional conventions catalogued in
//! the Prontuario.
//!
//! No `.dotx` files anywhere. The sidecar JSON in
//! `config/docx-templates/<domain>/<slug>.json` is the **sole** source
//! of truth: typography, margins, paper size, style names, section
//! skeleton, required metadata. The renderer reads it, produces
//! styles.xml and document.xml from scratch, and zips them into a
//! complete OOXML package. Anyone who wants to customise a template
//! edits the JSON; no Word skills required.

pub mod document_xml;
pub mod it_helpers;
pub mod package;
pub mod placeholders;
pub mod styles_xml;

use anyhow::{anyhow, Result};
use std::collections::HashMap;

use crate::presets::docx_template::DocxTemplate;

/// One-shot render: take a template + the LLM's Markdown body +
/// a metadata bag, and produce a `.docx` byte buffer.
///
/// Pipeline:
///   1. Validate that every `required_metadata` field is supplied
///      (universal fields LUOGO/DATA/MITTENTE/… plus template-specific).
///   2. Render the Markdown body to a WML body string.
///   3. Wrap the body in `word/document.xml` with the template's
///      paper / margins.
///   4. Substitute every `[PLACEHOLDER]` with the metadata bag value.
///   5. Build `word/styles.xml` from the template's typography +
///      style_map.
///   6. Package both XML parts plus the boilerplate `.rels` and
///      `[Content_Types].xml` into the final `.docx` zip.
///
/// Returns the byte buffer ready to stream to the user, plus a vec
/// of any `[PLACEHOLDER]` tokens that were left unfilled — the route
/// layer surfaces this list so missing metadata is loud, not silent.
pub struct RenderOutcome {
    pub bytes: Vec<u8>,
    /// Tokens still present in the output after substitution. Empty
    /// means every `[NAME]` in the body matched a key in `metadata`.
    /// Non-empty means the document was generated anyway (we never
    /// hard-fail on missing data) but the user should fill the gaps.
    pub unresolved_placeholders: Vec<String>,
}

pub fn render(
    template: &DocxTemplate,
    body_md: &str,
    metadata: &HashMap<String, String>,
) -> Result<RenderOutcome> {
    // ── Step 1: validate required_metadata (soft-warn, do not block).
    // The Prontuario lists template-specific required fields per
    // template (e.g. DEBITORE / IMPORTO for Diffida). Missing ones
    // come out as `[DEBITORE]` in the final document — the user sees
    // the gap immediately at proofread time, the loudest possible
    // feedback. Returning an error here would be hostile to the
    // common "LLM forgot a field, regenerate" workflow.
    let missing: Vec<&String> = template
        .required_metadata
        .iter()
        .filter(|k| !metadata.contains_key(k.as_str()))
        .collect();
    if !missing.is_empty() {
        tracing::warn!(
            "[docx] template {}: missing required_metadata fields: {:?}",
            template.id,
            missing
        );
    }

    // ── Step 2: substitute placeholders BEFORE the Markdown parser
    // touches the body. Reason: pulldown-cmark interprets `[X]` as a
    // reference-style link (CommonMark "shortcut reference link") and
    // when no `[X]: url` definition exists it swallows the token —
    // breaking the placeholder system. Substituting first means the
    // parser sees the real value verbatim. Trade-off: metadata values
    // containing Markdown-special chars (`*`, `_`, `[`, `(`) will be
    // reinterpreted by the parser. Documented limitation: bag values
    // should be plain text. Universal-metadata fields (LUOGO, DATA,
    // amounts, PEC) already are; legal text from the user might
    // contain `*` which would become bold — acceptable for the MVP,
    // tracked for Phase 2 (escape MD-special chars in values).
    let (substituted_md, hits) = placeholders::substitute(body_md, metadata);
    tracing::info!(
        "[docx] template {}: substituted {} placeholder occurrence(s)",
        template.id,
        hits
    );

    // ── Step 3-4: Markdown → WML body → document.xml envelope.
    // The WML emitter XML-escapes the text content of every run, so
    // values with `&`, `<`, `>`, `'`, `"` arrive in document.xml as
    // entities and the OOXML stays well-formed.
    let body_xml = document_xml::render_body(&substituted_md);
    let document_xml = document_xml::build_document_xml(&body_xml, template);

    // ── Step 5: styles.xml from typography + style_map.
    let styles_xml = styles_xml::build_styles_xml(template);

    // ── Step 6: zip everything.
    let bytes = package::package_docx(&document_xml, &styles_xml)
        .map_err(|e| anyhow!("docx package: {e}"))?;

    // Surface any `[TOKEN]` still present in the substituted source —
    // these are the fields the user (or the LLM) forgot to provide.
    let unresolved = placeholders::find_remaining_tokens(&substituted_md);
    Ok(RenderOutcome {
        bytes,
        unresolved_placeholders: unresolved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn load_diffida() -> DocxTemplate {
        let dir = crate::presets::config_subdir("docx-templates");
        let templates = crate::presets::docx_template::load_docx_templates(&dir)
            .expect("load templates");
        templates
            .into_iter()
            .find(|t| t.id == "it/diffida-messa-in-mora")
            .expect("diffida template present")
    }

    #[test]
    fn render_diffida_end_to_end_produces_valid_zip() {
        let template = load_diffida();
        let body_md = r#"# Oggetto

[DEBITORE] è diffidato e messo in mora al pagamento di **[IMPORTO]**
entro e non oltre il termine perentorio di *[TERMINE_GG] giorni*.

## Inadempimento

[DESCRIZIONE_INADEMPIMENTO]
"#;
        let mut bag = HashMap::new();
        bag.insert("DEBITORE".into(), "Tizio S.r.l.".into());
        bag.insert("IMPORTO".into(), "€ 12.345,67".into());
        bag.insert("TERMINE_GG".into(), "15".into());
        bag.insert(
            "DESCRIZIONE_INADEMPIMENTO".into(),
            "Mancato pagamento della fattura n. 42 del 1° marzo 2026.".into(),
        );

        let outcome = render(&template, body_md, &bag).expect("render ok");

        // ── Valid zip with the canonical 5 OOXML parts.
        let cursor = std::io::Cursor::new(&outcome.bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip parses");
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        for required in [
            "[Content_Types].xml",
            "_rels/.rels",
            "word/_rels/document.xml.rels",
            "word/styles.xml",
            "word/document.xml",
        ] {
            assert!(names.contains(&required.to_string()), "missing {required}");
        }

        // ── document.xml carries the substituted values, not the
        // raw [TOKENS].
        let mut document_xml = String::new();
        archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut document_xml)
            .unwrap();
        assert!(document_xml.contains("Tizio S.r.l."));
        assert!(document_xml.contains("€ 12.345,67"));
        assert!(document_xml.contains("15"));
        // Empty unresolved list — every required token was supplied.
        assert!(
            outcome.unresolved_placeholders.is_empty(),
            "unresolved tokens: {:?}",
            outcome.unresolved_placeholders
        );

        // ── styles.xml carries the localised baseline style names.
        let mut styles_xml = String::new();
        archive
            .by_name("word/styles.xml")
            .unwrap()
            .read_to_string(&mut styles_xml)
            .unwrap();
        assert!(styles_xml.contains("Corpo testo"));
        assert!(styles_xml.contains("Titolo sezione"));
        // Diffida is Calibri 11pt — 11pt = 22 half-points.
        assert!(styles_xml.contains(r#"w:ascii="Calibri""#));
        assert!(styles_xml.contains(r#"w:val="22""#));
    }

    #[test]
    fn missing_metadata_surfaces_in_unresolved_list() {
        let template = load_diffida();
        // Body references [IMPORTO] but the bag doesn't have it.
        let bag = HashMap::from([("DEBITORE".to_string(), "Caio".to_string())]);
        let outcome = render(&template, "Pay [IMPORTO] to [DEBITORE].", &bag)
            .expect("render still succeeds");
        assert!(outcome.unresolved_placeholders.contains(&"IMPORTO".to_string()));
        // Bytes still valid — render is non-blocking on missing data.
        assert_eq!(&outcome.bytes[..4], b"PK\x03\x04");
    }

    #[test]
    fn xml_special_chars_in_metadata_are_escaped_into_document() {
        // Ampersand and apostrophe in metadata values must reach
        // document.xml as XML entities — otherwise Word rejects the
        // file. Angle brackets `<...>` in a value are a pathological
        // case (pulldown-cmark may interpret as inline HTML); not in
        // scope for Phase 1.A.1, documented in render() comment.
        let template = load_diffida();
        let mut bag = HashMap::new();
        bag.insert("DEBITORE".to_string(), "Rossi & Verdi".to_string());
        bag.insert("IMPORTO".to_string(), "€ 100".to_string());
        bag.insert("TERMINE_GG".to_string(), "15".to_string());
        bag.insert(
            "DESCRIZIONE_INADEMPIMENTO".to_string(),
            "Apostrofo: l'inadempimento è grave.".to_string(),
        );
        let outcome =
            render(&template, "Spett.le [DEBITORE].", &bag).expect("render ok");
        let cursor = std::io::Cursor::new(&outcome.bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip");
        let mut document_xml = String::new();
        archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut document_xml)
            .unwrap();
        // The substituted name made it in (Markdown parser accepts
        // ampersand fine since the substitution happens BEFORE MD
        // parsing — pulldown-cmark sees "Rossi & Verdi" as plain
        // text, emits a Text event, our WML emitter XML-escapes to
        // `Rossi &amp; Verdi`).
        assert!(
            document_xml.contains("Rossi &amp; Verdi"),
            "ampersand must be XML-escaped: {document_xml}"
        );
        // The raw unescaped ampersand must NOT survive in document.xml.
        // (Whitespace check — `& V` would be the dangerous substring.)
        assert!(
            !document_xml.contains("Rossi & Verdi"),
            "raw ampersand leaked into XML: {document_xml}"
        );
    }
}
