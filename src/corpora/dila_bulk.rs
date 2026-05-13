//! `dila-bulk-xml` strategy — bulk download of DILA OPENDATA archives.
//!
//! DILA (Direction de l'information légale et administrative) publishes
//! the entirety of several French legal-corpus fondi as static
//! tar.gz archives at <https://echanges.dila.gouv.fr/OPENDATA/>:
//!
//!   - **CNIL** — CNIL deliberations (sanctions, recommendations,
//!     opinions). Full snapshot ~18 MB.
//!   - **LEGI** — French legislation (Code civil, Code pénal, …).
//!     ~9 GB; streaming is mandatory.
//!   - **JORF** — Journal officiel quotidien.
//!   - **CASS** — Cour de cassation decisions.
//!   - **KALI** — collective bargaining agreements.
//!
//! All fondi use the **same XML schema** (the "DILA Akoma Ntoso"-like
//! format, though the real Akoma Ntoso is unrelated): root `<TEXTE_*>`,
//! `<META><META_COMMUN><ID>` for the canonical identifier
//! (`CNILTEXTxxxxxxx`, `LEGITEXTxxxxxxx`, …), `<META><META_SPEC>` for
//! fonds-specific fields, `<BLOC_TEXTUEL><CONTENU>` for the body
//! (with inline `<p>`, `<strong>`, `<em>` etc.).
//!
//! Today this module ships:
//!   - The `DilaBulkXmlSpec` manifest fragment (what the JSON
//!     declares — fonds id, base URL, archive name patterns).
//!   - `parse_dila_xml` that takes one XML file and returns a
//!     `DilaDocument` with all extracted fields + plain-text body
//!     (markup stripped).
//!
//! The bulk-import flow (download, extract, walk, insert into the
//! `corpus_documents` FTS5 table) lives in `routes/corpora.rs` as
//! a generic `/corpora/:id/import` handler — same shape Italian
//! Legal's parquet importer uses for HF datasets.
//!
//! The DILA schema is **opinionated by design**: every French
//! open-data fonds shares it, so we don't expose per-fonds field
//! mappings in the manifest. A new fonds = a new `fonds` key in
//! the manifest, same parser.

use anyhow::{anyhow, Context, Result};
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};

/// Manifest fragment for the `dila-bulk-xml` strategy. Only the
/// archive-discovery fields belong in the JSON — the XML schema is
/// fixed.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DilaBulkXmlSpec {
    /// Fonds id (`CNIL`, `LEGI`, `JORF`, `CASS`, …). Used to filter
    /// the directory listing and as the `corpus_documents.subcorpus`
    /// label when multiple DILA-backed corpora ever share a DB.
    pub fonds: String,

    /// Directory URL the importer scans for the latest full archive
    /// and any incrementals.
    ///
    /// Example: <https://echanges.dila.gouv.fr/OPENDATA/CNIL/>
    pub archive_index_url: String,

    /// File-name prefix of the full snapshot ("baseline") archives.
    /// CNIL uses `"Freemium_cnil_global_"`; LEGI uses
    /// `"Freemium_legi_global_"`. The importer picks the
    /// most-recently-dated file whose name starts with this prefix.
    pub global_archive_prefix: String,

    /// File-name prefix of the daily/weekly incremental archives.
    /// CNIL uses `"CNIL_"`; LEGI uses `"LEGI_"`. The importer applies
    /// all incrementals whose timestamp is strictly newer than the
    /// global snapshot, in order.
    #[serde(default)]
    pub incremental_archive_prefix: Option<String>,

    /// File extension used by DILA. `tar.gz` everywhere today, but
    /// they've experimented with `tar.zst` for very large fondi —
    /// kept as a manifest knob for forward-compat.
    #[serde(default = "default_archive_suffix")]
    pub archive_suffix: String,
}

fn default_archive_suffix() -> String {
    ".tar.gz".to_string()
}

/// One DILA document, parsed from one XML file inside the bulk
/// archive. Field naming matches the DILA element names so the
/// mapping is obvious to anyone reading the XML side-by-side.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DilaDocument {
    /// `<META><META_COMMUN><ID>`. Persistent identifier (e.g.
    /// `CNILTEXT000054047151`). What the `documents.corpus_identifier`
    /// column stores.
    pub id: String,

    /// `<META><META_COMMUN><NATURE>`. Top-level type:
    /// "DELIBERATION", "DECISION", "ARRETE", "DECRET", etc.
    pub nature: String,

    /// `<META><META_COMMUN><ORIGINE>`. Source authority — "CNIL",
    /// "JURI", "LEGI"… mostly redundant with the manifest's `fonds`,
    /// but stored for cross-corpus sanity.
    pub origine: String,

    /// `<META><META_SPEC><META_*><TITRE>`. Short title (one line).
    pub titre: Option<String>,

    /// `<META><META_SPEC><META_*><TITREFULL>`. Long-form title (often
    /// the only one populated for legislation). When both are set we
    /// prefer this one for display.
    pub titre_full: Option<String>,

    /// `<META><META_SPEC><META_*><NUMERO>`. Human-friendly reference
    /// (e.g. CNIL "2026-047", a "loi n° 78-17"). Distinct from `id`
    /// (which is the opaque persistent key).
    pub numero: Option<String>,

    /// `<META><META_SPEC><META_*><NOR>`. NOR identifier — used by
    /// some French legal cross-references.
    pub nor: Option<String>,

    /// `<META><META_SPEC><META_CNIL><NATURE_DELIB>` and equivalents
    /// for other fondi: sub-category that drives source filtering
    /// (e.g. for CNIL: "Sanction", "Mise en demeure",
    /// "Recommandation/Lignes directrices", "Avis"). None for fondi
    /// that don't categorise.
    pub nature_delib: Option<String>,

    /// `<META><META_SPEC><META_*><DATE_TEXTE>`. ISO date the act was
    /// adopted.
    pub date_texte: Option<String>,

    /// `<META><META_SPEC><META_*><DATE_PUBLI>`. ISO date the act was
    /// published to the corpus.
    pub date_publi: Option<String>,

    /// `<META><META_SPEC><META_*><ETAT_JURIDIQUE>`. Legal status:
    /// "VIGUEUR" (in force), "ABROGE" (repealed), "MODIFIE"
    /// (amended), etc. UI surfaces this so a user citing an act
    /// knows whether it's current.
    pub etat: Option<String>,

    /// Plain-text body extracted from `<BLOC_TEXTUEL><CONTENU>`.
    /// Inline `<p>`, `<strong>`, `<em>`, `<br/>` etc. stripped.
    /// Paragraphs joined with double newlines so the chunker can
    /// segment cleanly. Empty when the XML has no body.
    pub body: String,
}

impl DilaDocument {
    /// Best display title — prefer the full title, fall back to the
    /// short one, finally to the numero, finally to the id.
    pub fn display_title(&self) -> &str {
        self.titre_full
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(self.titre.as_deref().filter(|s| !s.is_empty()))
            .or(self.numero.as_deref().filter(|s| !s.is_empty()))
            .unwrap_or(self.id.as_str())
    }
}

/// Parse one DILA XML file into a `DilaDocument`.
///
/// Forgiving by design: missing/empty optional fields produce
/// `None`. Only the `id` is required; absent ID → error (a doc
/// without an id is unaddressable, so refusing to silently produce
/// a placeholder is the safer choice).
///
/// Schema specifics:
///   - We DON'T validate the root element name (`TEXTE_CNIL` vs
///     `TEXTE_LEGI` vs `TEXTE_VERSION` for JORF): different fondi
///     use different root names but the same internal layout.
///   - `<CONTENU>` body extraction strips all inline elements and
///     converts `<p>...</p>` boundaries to double newlines so the
///     downstream chunker can split on blank lines.
pub fn parse_dila_xml(xml: &[u8]) -> Result<DilaDocument> {
    let mut reader = Reader::from_reader(xml);
    // Intentionally do NOT enable `trim_text` here. Inside
    // <CONTENU> we need to preserve inter-element whitespace so
    // "First <strong>bold</strong> word" doesn't collapse to
    // "Firstboldword" once the markup is stripped. We trim at the
    // meta-field assignment site instead.

    let mut doc = DilaDocument::default();

    // State for tracking which element we're inside. We collect
    // PathStack-style breadcrumbs as element names so we can match
    // on a slice of "ancestor names" — much cleaner than nested
    // booleans for the META → META_COMMUN/META_SPEC tree.
    let mut path: Vec<String> = Vec::new();

    // Accumulator for body content; we serialise the inner XML of
    // <CONTENU> back to text, paragraph-aware. State is two-pronged:
    // we copy character data when we're inside <CONTENU>, and we
    // insert paragraph breaks when </p> closes.
    let mut body_buf = String::new();
    let mut in_contenu = false;

    // Single buffer reused for every iteration — quick-xml's
    // recommended pattern.
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .with_context(|| format!("at byte position {}", reader.buffer_position()))?
        {
            Event::Start(e) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                path.push(name.clone());
                if name == "CONTENU" {
                    in_contenu = true;
                }
            }
            Event::End(e) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if name == "CONTENU" {
                    in_contenu = false;
                }
                if in_contenu && (name == "p" || name == "div") {
                    // End of a paragraph → blank line so the chunker
                    // segments naturally.
                    if !body_buf.is_empty() && !body_buf.ends_with("\n\n") {
                        body_buf.push_str("\n\n");
                    }
                }
                let popped = path.pop();
                debug_assert_eq!(popped.as_deref(), Some(name.as_str()));
            }
            Event::Empty(e) => {
                // Self-closing tags (e.g. `<br/>`, `<ANCIEN_ID/>`).
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if in_contenu && (name == "br" || name == "p") {
                    body_buf.push('\n');
                }
            }
            Event::Text(t) => {
                // `unescape()` resolves XML entities (`&amp;`,
                // `&#xE9;`, etc.) and returns Cow<str>. DILA XML is
                // UTF-8 with a few entity uses inside CONTENU text.
                let text = t
                    .unescape()
                    .with_context(|| {
                        format!("text unescape at byte {}", reader.buffer_position())
                    })?
                    .into_owned();
                if in_contenu {
                    body_buf.push_str(&text);
                    continue;
                }
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    continue;
                }
                assign_meta_field(&path, trimmed, &mut doc);
            }
            Event::CData(c) => {
                // CONTENU can contain CDATA blocks; treat them as
                // text in body context.
                if in_contenu {
                    let s =
                        std::str::from_utf8(c.as_ref()).unwrap_or("").to_string();
                    body_buf.push_str(&s);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    if doc.id.is_empty() {
        return Err(anyhow!(
            "DILA XML missing required <META><META_COMMUN><ID> element"
        ));
    }

    doc.body = collapse_body(&body_buf);
    Ok(doc)
}

/// Map an XML element under `<META>` to the corresponding
/// `DilaDocument` field. The path slice is the ancestor chain
/// MINUS the root element — we just look at the tail. Mappings are
/// shared across DILA fondi (META_CNIL / META_LEGI / META_JORF all
/// expose the same field names where applicable).
fn assign_meta_field(path: &[String], value: &str, doc: &mut DilaDocument) {
    let Some(last) = path.last() else { return };
    match last.as_str() {
        "ID" => doc.id = value.to_string(),
        "NATURE" => doc.nature = value.to_string(),
        "ORIGINE" => doc.origine = value.to_string(),
        "TITRE" => doc.titre = Some(value.to_string()),
        "TITREFULL" => doc.titre_full = Some(value.to_string()),
        "NUMERO" => doc.numero = Some(value.to_string()),
        "NOR" => doc.nor = Some(value.to_string()),
        "NATURE_DELIB" => doc.nature_delib = Some(value.to_string()),
        "DATE_TEXTE" => doc.date_texte = Some(value.to_string()),
        "DATE_PUBLI" => doc.date_publi = Some(value.to_string()),
        "ETAT_JURIDIQUE" => doc.etat = Some(value.to_string()),
        _ => {}
    }
}

/// Collapse the raw body buffer into a clean plain-text form:
///   - normalise consecutive whitespace inside a paragraph to a
///     single space (matches what every legal-corpus chunker
///     wants);
///   - keep double newlines as paragraph separators;
///   - trim leading/trailing whitespace from the whole body.
fn collapse_body(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_was_space = false;
    let mut newline_run = 0usize;
    for ch in raw.chars() {
        if ch == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                out.push('\n');
            }
            last_was_space = false;
        } else if ch.is_whitespace() {
            if last_was_space || newline_run > 0 {
                continue;
            }
            out.push(' ');
            last_was_space = true;
        } else {
            out.push(ch);
            last_was_space = false;
            newline_run = 0;
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real DILA CNIL XML downloaded from the OPENDATA bulk archive.
    /// Captured 2026-05-13 from
    /// echanges.dila.gouv.fr/OPENDATA/CNIL/CNIL_20260507-213433.tar.gz.
    /// One full deliberation: CNIL Délibération n° 2026-047 (the
    /// "octroi de crédit" recommandation).
    const REAL_CNIL_XML: &[u8] =
        include_bytes!("fixtures/dila_cnil_recommandation.xml");

    #[test]
    fn parses_real_cnil_recommandation() {
        let d = parse_dila_xml(REAL_CNIL_XML).expect("parse succeeds");
        assert_eq!(d.id, "CNILTEXT000054047151");
        assert_eq!(d.nature, "DELIBERATION");
        assert_eq!(d.origine, "CNIL");
        assert_eq!(d.numero.as_deref(), Some("2026-047"));
        assert_eq!(d.nor.as_deref(), Some("CNIS2611643X"));
        assert_eq!(
            d.nature_delib.as_deref(),
            Some("Recommandation/Lignes directrices")
        );
        assert_eq!(d.date_texte.as_deref(), Some("2026-04-02"));
        assert_eq!(d.date_publi.as_deref(), Some("2026-05-08"));
        assert_eq!(d.etat.as_deref(), Some("VIGUEUR"));
        // Title fields populated.
        assert!(d
            .titre_full
            .as_deref()
            .unwrap()
            .starts_with("Délibération n° 2026-047"));
        // Body has substantive content from the act, paragraph
        // separators preserved.
        assert!(d.body.contains("règlement (UE) 2016/679"));
        assert!(d.body.contains("octroi de crédit"));
        assert!(d.body.contains("\n\n")); // paragraph break preserved
                                          // No inline markup leaks through.
        assert!(!d.body.contains("<p"));
        assert!(!d.body.contains("</p"));
        assert!(!d.body.contains("<strong"));
    }

    #[test]
    fn display_title_prefers_full_then_titre_then_numero_then_id() {
        let mut d = DilaDocument {
            id: "X".to_string(),
            ..Default::default()
        };
        assert_eq!(d.display_title(), "X");
        d.numero = Some("123".to_string());
        assert_eq!(d.display_title(), "123");
        d.titre = Some("Short".to_string());
        assert_eq!(d.display_title(), "Short");
        d.titre_full = Some("Long full title".to_string());
        assert_eq!(d.display_title(), "Long full title");
        // Empty strings are ignored.
        d.titre_full = Some(String::new());
        assert_eq!(d.display_title(), "Short");
    }

    #[test]
    fn rejects_xml_without_id_element() {
        let body = br#"<?xml version="1.0"?>
            <TEXTE_CNIL><META><META_COMMUN><NATURE>X</NATURE></META_COMMUN></META>
            <BLOC_TEXTUEL><CONTENU><p>body</p></CONTENU></BLOC_TEXTUEL>
            </TEXTE_CNIL>"#;
        let err = parse_dila_xml(body).unwrap_err();
        assert!(err.to_string().contains("required"));
    }

    #[test]
    fn body_extraction_strips_inline_markup() {
        let body = br#"<?xml version="1.0"?>
            <TEXTE_CNIL>
                <META>
                    <META_COMMUN><ID>X1</ID></META_COMMUN>
                </META>
                <BLOC_TEXTUEL>
                    <CONTENU>
                        <p>First <strong>bold</strong> para.</p>
                        <p>Second <em>italic</em> para with <a href="x">link</a>.</p>
                    </CONTENU>
                </BLOC_TEXTUEL>
            </TEXTE_CNIL>"#;
        let d = parse_dila_xml(body).unwrap();
        // Paragraphs separated; inline tags gone.
        assert!(d.body.contains("First bold para."));
        assert!(d.body.contains("Second italic para with link."));
        // Tags really gone, not just visually:
        assert!(!d.body.contains('<'));
        assert!(!d.body.contains('>'));
        // Paragraph separator present.
        let between = d
            .body
            .find("Second")
            .expect("second paragraph present in extracted body");
        assert!(d.body[..between].ends_with("\n\n"));
    }

    #[test]
    fn body_handles_cdata() {
        let body = b"<?xml version=\"1.0\"?>
            <TEXTE_CNIL>
                <META><META_COMMUN><ID>X2</ID></META_COMMUN></META>
                <BLOC_TEXTUEL>
                    <CONTENU><p><![CDATA[escaped <special> chars & ampers]]></p></CONTENU>
                </BLOC_TEXTUEL>
            </TEXTE_CNIL>";
        let d = parse_dila_xml(body).unwrap();
        assert!(d.body.contains("escaped <special> chars & ampers"));
    }

    #[test]
    fn collapse_body_normalises_whitespace_inside_paragraphs() {
        // Within a paragraph: collapse multiple spaces / single
        // newlines to one space. Across paragraphs: keep the double
        // newline.
        let raw = "  first   line\nwith   wrap\n\nsecond  para   ";
        let out = collapse_body(raw);
        assert_eq!(out, "first line\nwith wrap\n\nsecond para");
    }

    #[test]
    fn collapse_body_caps_blank_line_runs_at_two_newlines() {
        // 4 newlines collapse to 2 (one paragraph break, not three).
        let raw = "a\n\n\n\nb";
        let out = collapse_body(raw);
        assert_eq!(out, "a\n\nb");
    }

    #[test]
    fn manifest_spec_round_trips_via_serde() {
        let json = r#"{
            "fonds": "CNIL",
            "archive_index_url": "https://echanges.dila.gouv.fr/OPENDATA/CNIL/",
            "global_archive_prefix": "Freemium_cnil_global_",
            "incremental_archive_prefix": "CNIL_"
        }"#;
        let spec: DilaBulkXmlSpec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.fonds, "CNIL");
        // Default suffix kicks in when not declared.
        assert_eq!(spec.archive_suffix, ".tar.gz");
        // Round-trip back to JSON keeps the structure intact.
        let back = serde_json::to_value(&spec).unwrap();
        assert_eq!(back["fonds"], "CNIL");
        assert_eq!(back["archive_suffix"], ".tar.gz");
    }
}
