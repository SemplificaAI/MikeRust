//! Markdown → `word/document.xml` body emitter.
//!
//! Consumes events from `pulldown_cmark` and produces WordprocessingML
//! paragraphs (`<w:p>`) and runs (`<w:r>`). References styles by the
//! ASCII styleIds emitted by `styles_xml::build_styles_xml`, never by
//! the localised `w:name` — Word resolves by id.
//!
//! What this MVP covers (matches the shipped Phase-1.A templates):
//!   - Headings 1/2/3 → `SectionHeading` style (the sole baseline
//!     heading slot; deeper levels reuse it)
//!   - Paragraphs → `BodyText`
//!   - Lists (ordered + unordered) → `BodyText` with manual marker
//!   - Strong / Emphasis → `<w:b/>` / `<w:i/>` on the run
//!   - Code spans → monospace run (Courier New)
//!   - Hard / soft breaks
//!   - **GFM tables** → `<w:tbl>` with explicit single-line borders.
//!     Required by `it/inventario-beni-assicurati` (the LLM emits a
//!     9-column passages table) and any future tabular template.
//!
//! Deferred (Phase 2 — footnotes, page breaks, citation blockquote
//! `>`, multi-column-width hints from `:--:` alignment markers): the
//! templates don't depend on them yet.

use pulldown_cmark::{Alignment, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use super::styles_xml::xml_escape;

/// Wrap the body XML produced by [`render_body`] into a complete
/// `word/document.xml`. Handles the `<w:document>` envelope, page
/// size from `paper`, margins from `margins_cm`, and the `<w:sectPr>`
/// at end-of-body that Word needs to mount a section.
pub fn build_document_xml(
    body_xml: &str,
    template: &crate::presets::docx_template::DocxTemplate,
) -> String {
    let (pg_w, pg_h) = paper_dimensions_twips(&template.paper.size, &template.paper.orientation);
    let margins = &template.margins_cm;
    let top = super::styles_xml::cm_to_twips(margins.top);
    let right = super::styles_xml::cm_to_twips(margins.right);
    let bottom = super::styles_xml::cm_to_twips(margins.bottom);
    let left = super::styles_xml::cm_to_twips(margins.left);

    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
{body_xml}    <w:sectPr>
      <w:pgSz w:w="{pg_w}" w:h="{pg_h}"/>
      <w:pgMar w:top="{top}" w:right="{right}" w:bottom="{bottom}" w:left="{left}" w:header="708" w:footer="708" w:gutter="0"/>
    </w:sectPr>
  </w:body>
</w:document>"#
    )
}

/// Render the body — every `<w:p>` between `<w:body>` open/close —
/// from a Markdown string. The output is a sequence of paragraph
/// elements separated by newlines for readability.
pub fn render_body(markdown: &str) -> String {
    let mut state = RenderState::default();
    // GFM tables are the only non-CommonMark feature we need today.
    // Strikethrough / task lists / footnotes can be opted in later
    // without changing the table machinery below.
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(markdown, options);
    for ev in parser {
        state.handle(ev);
    }
    state.flush_paragraph();
    state.out
}

#[derive(Default)]
struct RenderState {
    out: String,
    /// Runs accumulated for the paragraph currently being built.
    current_runs: Vec<String>,
    /// Style for the current paragraph (None until first text event).
    /// Stable styleIds emitted by `styles_xml::sanitize_style_id`.
    current_style: Option<&'static str>,
    bold: bool,
    italic: bool,
    in_code_block: bool,
    list_marker_stack: Vec<ListMarker>,
    /// Active table context. None outside any table.
    table: Option<TableState>,
}

/// State accumulated while traversing a GFM table. We can't stream
/// the `<w:tbl>` to `out` cell-by-cell because OOXML needs `<w:tblGrid>`
/// up front (Word's renderer is sloppy about missing tblGrid but other
/// consumers like LibreOffice reject it) — so we buffer cells and rows
/// and emit the whole table on `TagEnd::Table`.
struct TableState {
    /// Per-column alignment as the GFM separator row specified
    /// (`:---`, `---:`, `:---:`). Length == column count.
    alignments: Vec<Alignment>,
    /// All rows seen so far, each already serialised as a `<w:tr>…</w:tr>`.
    rows: Vec<String>,
    /// Cells of the row currently being built — each is a complete
    /// `<w:tc>…</w:tc>` blob.
    current_row_cells: Vec<String>,
    /// Runs belonging to the cell currently being built.
    current_cell_runs: Vec<String>,
    /// True while inside the header row (`<w:r>` runs get bold).
    in_head: bool,
}

#[derive(Clone, Copy)]
enum ListMarker {
    Bullet,
    Ordered(u64),
}

impl RenderState {
    fn handle(&mut self, ev: Event) {
        match ev {
            Event::Start(Tag::Heading { level, .. }) => {
                self.flush_paragraph();
                // All headings map to SectionHeading — the Prontuario's
                // baseline has only one heading slot. Deeper levels
                // (h2, h3) inherit the same style for now; templates
                // can add `heading_2` / `heading_3` style_map entries
                // when they need differentiation.
                self.current_style = Some(match level {
                    HeadingLevel::H1 => "SectionHeading",
                    HeadingLevel::H2 => "SectionHeading",
                    HeadingLevel::H3 => "SectionHeading",
                    _ => "SectionHeading",
                });
            }
            Event::End(TagEnd::Heading(_)) => {
                self.flush_paragraph();
            }
            Event::Start(Tag::Paragraph) => {
                if self.current_style.is_none() {
                    self.current_style = Some("BodyText");
                }
            }
            Event::End(TagEnd::Paragraph) => {
                self.flush_paragraph();
            }
            Event::Start(Tag::List(start)) => {
                self.list_marker_stack.push(match start {
                    Some(n) => ListMarker::Ordered(n),
                    None => ListMarker::Bullet,
                });
            }
            Event::End(TagEnd::List(_)) => {
                self.list_marker_stack.pop();
            }
            Event::Start(Tag::Item) => {
                self.flush_paragraph();
                self.current_style = Some("BodyText");
                // Prepend the marker as the first run — pure-text
                // bullet "• " or "1. ". Numbering definitions in
                // OOXML are heavy machinery we don't need yet; this
                // gives Word a clean visible list.
                let marker = match self.list_marker_stack.last_mut() {
                    Some(ListMarker::Bullet) => "•  ".to_string(),
                    Some(ListMarker::Ordered(n)) => {
                        let s = format!("{n}.  ");
                        *n += 1;
                        s
                    }
                    None => String::new(),
                };
                self.current_runs.push(run_text(&marker, false, false, false));
            }
            Event::End(TagEnd::Item) => {
                self.flush_paragraph();
            }
            Event::Start(Tag::Strong) => self.bold = true,
            Event::End(TagEnd::Strong) => self.bold = false,
            Event::Start(Tag::Emphasis) => self.italic = true,
            Event::End(TagEnd::Emphasis) => self.italic = false,
            Event::Start(Tag::CodeBlock(_)) => {
                self.in_code_block = true;
                self.flush_paragraph();
                self.current_style = Some("BodyText");
            }
            Event::End(TagEnd::CodeBlock) => {
                self.flush_paragraph();
                self.in_code_block = false;
            }
            Event::Text(t) => {
                let bold_eff = self.bold || self.table.as_ref().is_some_and(|t| t.in_head);
                let run = run_text(&t, bold_eff, self.italic, self.in_code_block);
                if let Some(table) = self.table.as_mut() {
                    table.current_cell_runs.push(run);
                } else {
                    self.current_runs.push(run);
                }
            }
            Event::Code(t) => {
                let bold_eff = self.bold || self.table.as_ref().is_some_and(|t| t.in_head);
                let run = run_text(&t, bold_eff, self.italic, true);
                if let Some(table) = self.table.as_mut() {
                    table.current_cell_runs.push(run);
                } else {
                    self.current_runs.push(run);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                let run = r#"<w:r><w:br/></w:r>"#.to_string();
                if let Some(table) = self.table.as_mut() {
                    table.current_cell_runs.push(run);
                } else {
                    self.current_runs.push(run);
                }
            }
            Event::Start(Tag::Table(alignments)) => {
                self.flush_paragraph();
                self.table = Some(TableState {
                    alignments,
                    rows: Vec::new(),
                    current_row_cells: Vec::new(),
                    current_cell_runs: Vec::new(),
                    in_head: false,
                });
            }
            Event::Start(Tag::TableHead) => {
                if let Some(table) = self.table.as_mut() {
                    table.in_head = true;
                    table.current_row_cells.clear();
                }
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(table) = self.table.as_mut() {
                    let row_xml = build_row_xml(&table.current_row_cells, true);
                    table.rows.push(row_xml);
                    table.current_row_cells.clear();
                    table.in_head = false;
                }
            }
            Event::Start(Tag::TableRow) => {
                if let Some(table) = self.table.as_mut() {
                    table.current_row_cells.clear();
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(table) = self.table.as_mut() {
                    let row_xml = build_row_xml(&table.current_row_cells, false);
                    table.rows.push(row_xml);
                    table.current_row_cells.clear();
                }
            }
            Event::Start(Tag::TableCell) => {
                if let Some(table) = self.table.as_mut() {
                    table.current_cell_runs.clear();
                }
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(table) = self.table.as_mut() {
                    let cell_xml = build_cell_xml(&table.current_cell_runs);
                    table.current_row_cells.push(cell_xml);
                    table.current_cell_runs.clear();
                }
            }
            Event::End(TagEnd::Table) => {
                if let Some(table) = self.table.take() {
                    self.out.push_str(&build_table_xml(&table));
                }
            }
            // Footnotes, blockquote, HTML — deferred.
            _ => {}
        }
    }

    fn flush_paragraph(&mut self) {
        if self.current_runs.is_empty() {
            return;
        }
        let style = self.current_style.unwrap_or("BodyText");
        self.out.push_str("    <w:p>");
        self.out.push_str(&format!(
            r#"<w:pPr><w:pStyle w:val="{style}"/></w:pPr>"#
        ));
        for r in self.current_runs.drain(..) {
            self.out.push_str(&r);
        }
        self.out.push_str("</w:p>\n");
        self.current_style = None;
    }
}

/// Emit a single `<w:r>` run with the right formatting properties.
fn run_text(text: &str, bold: bool, italic: bool, monospace: bool) -> String {
    let mut props = String::new();
    if bold {
        props.push_str("<w:b/>");
    }
    if italic {
        props.push_str("<w:i/>");
    }
    if monospace {
        props.push_str(r#"<w:rFonts w:ascii="Courier New" w:hAnsi="Courier New"/>"#);
    }
    let rpr = if props.is_empty() {
        String::new()
    } else {
        format!("<w:rPr>{props}</w:rPr>")
    };
    format!(
        r#"<w:r>{rpr}<w:t xml:space="preserve">{}</w:t></w:r>"#,
        xml_escape(text)
    )
}

/// Build a `<w:tc>` cell wrapping the accumulated runs in a single
/// paragraph. Cells must always contain at least one `<w:p>` — Word
/// rejects empty `<w:tc>` — so we synthesise an empty run when the
/// LLM produced no cell content.
fn build_cell_xml(runs: &[String]) -> String {
    let body: String = if runs.is_empty() {
        String::new()
    } else {
        runs.join("")
    };
    format!(
        r#"<w:tc><w:tcPr><w:tcW w:w="0" w:type="auto"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="BodyText"/></w:pPr>{body}</w:p></w:tc>"#
    )
}

/// Build a `<w:tr>` row from its cell xml blobs. The header row gets
/// `<w:tblHeader/>` so Word repeats it on every page break when the
/// table spans multiple pages — useful for inventories long enough
/// to overflow a landscape A4.
fn build_row_xml(cells: &[String], is_header: bool) -> String {
    let pr = if is_header {
        r#"<w:trPr><w:tblHeader/></w:trPr>"#
    } else {
        ""
    };
    let cells_xml: String = cells.join("");
    format!("<w:tr>{pr}{cells_xml}</w:tr>")
}

/// Build the complete `<w:tbl>` from buffered rows. Uses explicit
/// single-line borders so the document is portable to consumers that
/// don't ship Word's built-in `TableGrid` style (LibreOffice, online
/// previewers, docx-preview in our own DocxView).
fn build_table_xml(state: &TableState) -> String {
    let cols = state.alignments.len().max(1);
    let grid: String = (0..cols).map(|_| r#"<w:gridCol w:w="0"/>"#).collect();
    let rows: String = state.rows.join("");
    format!(
        r#"<w:tbl><w:tblPr><w:tblW w:w="5000" w:type="pct"/><w:tblLayout w:type="autofit"/><w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:left w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:bottom w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:right w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:insideH w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:insideV w:val="single" w:sz="4" w:space="0" w:color="auto"/></w:tblBorders></w:tblPr><w:tblGrid>{grid}</w:tblGrid>{rows}</w:tbl>"#
    )
}

/// Page dimensions in twips. A4 portrait = 11906 × 16838.
/// Currently only A4 is shipped — other sizes default to A4.
pub fn paper_dimensions_twips(size: &str, orientation: &str) -> (u32, u32) {
    let (w, h) = match size {
        "A4" => (11906u32, 16838u32),
        "Letter" => (12240, 15840),
        _ => (11906, 16838),
    };
    if orientation == "landscape" { (h, w) } else { (w, h) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_empty_body() {
        assert_eq!(render_body(""), "");
    }

    #[test]
    fn plain_paragraph_uses_body_text_style() {
        let xml = render_body("Hello world.");
        assert!(xml.contains(r#"<w:pStyle w:val="BodyText"/>"#));
        assert!(xml.contains("Hello world."));
    }

    #[test]
    fn h1_h2_h3_all_use_section_heading_style() {
        for md in ["# Title", "## Sub", "### Subsub"] {
            let xml = render_body(md);
            assert!(
                xml.contains(r#"<w:pStyle w:val="SectionHeading"/>"#),
                "heading style missing for {md:?}: {xml}"
            );
        }
    }

    #[test]
    fn strong_emits_bold_run() {
        let xml = render_body("Hello **bold** world.");
        // The 'bold' run must carry a <w:b/> inside its <w:rPr>.
        let bold_idx = xml.find("bold").expect("text present");
        let before = &xml[..bold_idx];
        // Look back up to 80 chars — the run open is right before
        // the text.
        assert!(before[before.len().saturating_sub(80)..].contains("<w:b/>"));
    }

    #[test]
    fn emphasis_emits_italic_run() {
        let xml = render_body("Hello *italic* world.");
        let italic_idx = xml.find("italic").expect("text present");
        let before = &xml[..italic_idx];
        assert!(before[before.len().saturating_sub(80)..].contains("<w:i/>"));
    }

    #[test]
    fn code_span_emits_courier_run() {
        let xml = render_body("Try `let x = 1`.");
        assert!(xml.contains(r#"<w:rFonts w:ascii="Courier New" w:hAnsi="Courier New"/>"#));
        assert!(xml.contains("let x = 1"));
    }

    #[test]
    fn bullet_list_prepends_dot_marker() {
        let xml = render_body("- alpha\n- beta\n- gamma");
        assert!(xml.contains("•"));
        // Three paragraphs.
        let p_count = xml.matches("<w:p>").count();
        assert_eq!(p_count, 3, "expected 3 paragraphs, got {p_count}: {xml}");
    }

    #[test]
    fn ordered_list_uses_incrementing_marker() {
        let xml = render_body("1. alpha\n2. beta\n3. gamma");
        assert!(xml.contains("1."));
        assert!(xml.contains("2."));
        assert!(xml.contains("3."));
    }

    #[test]
    fn xml_escape_applies_to_text() {
        let xml = render_body("A & B");
        assert!(xml.contains("A &amp; B"));
        assert!(!xml.contains("A & B"));
    }

    #[test]
    fn gfm_table_emits_wtbl_with_borders_and_grid() {
        let md = "\
| # | Categoria | Valore |
|---|-----------|--------|
| 1 | Veicolo   | 14000  |
| 2 | Immobile  | 200000 |
";
        let xml = render_body(md);
        assert!(xml.contains("<w:tbl>"), "table element missing: {xml}");
        assert!(xml.contains("<w:tblGrid>"), "tblGrid missing");
        // 3 columns → 3 gridCol entries.
        assert_eq!(xml.matches("<w:gridCol").count(), 3);
        // Borders required for portability across LibreOffice / docx-preview.
        assert!(xml.contains("<w:tblBorders>"));
        // Three rows total (one header + two body).
        assert_eq!(xml.matches("<w:tr>").count(), 3);
        // Header row must repeat on page break.
        assert!(xml.contains("<w:tblHeader/>"));
        // Header cells render bold.
        let header_idx = xml.find("Categoria").expect("header text");
        let before = &xml[..header_idx];
        assert!(before[before.len().saturating_sub(120)..].contains("<w:b/>"));
        // Data cells must NOT have the body-text style stripped — every
        // <w:tc> contains a <w:p> with BodyText.
        let cell_paragraphs = xml.matches(r#"<w:pStyle w:val="BodyText"/>"#).count();
        assert!(
            cell_paragraphs >= 9,
            "expected at least 9 BodyText paragraphs (3x3 cells), got {cell_paragraphs}: {xml}"
        );
    }

    #[test]
    fn gfm_table_with_empty_cell_still_emits_paragraph() {
        // Word rejects empty <w:tc>, so a blank cell must still carry
        // a <w:p>. Mirrors the "no value" rows the LLM emits for assets
        // with unknown identificativo.
        let md = "\
| col1 | col2 |
|------|------|
| a    |      |
";
        let xml = render_body(md);
        // 2 cells × 2 rows = 4 <w:tc> tags. Every one needs a <w:p>.
        assert_eq!(xml.matches("<w:tc>").count(), 4);
        assert_eq!(
            xml.matches("<w:tc><w:tcPr").count(),
            xml.matches("<w:p>").count() - xml.matches("<w:p><w:pPr><w:pStyle w:val=\"SectionHeading\"").count(),
            "every <w:tc> must wrap a <w:p>"
        );
    }

    #[test]
    fn paragraph_after_table_renders_correctly() {
        // Regression guard: state machine must drop back to non-table
        // mode cleanly after `End(Table)`.
        let md = "\
| col |
|-----|
| x   |

Closing prose here.
";
        let xml = render_body(md);
        assert!(xml.contains("<w:tbl>"));
        assert!(xml.contains("</w:tbl>"));
        assert!(xml.contains("Closing prose here."));
        // The closing prose must be a normal BodyText paragraph,
        // NOT trapped inside the table.
        let prose_idx = xml.find("Closing prose").unwrap();
        let tbl_close = xml.find("</w:tbl>").unwrap();
        assert!(
            prose_idx > tbl_close,
            "closing prose must come after </w:tbl>: tbl_close={tbl_close} prose={prose_idx}"
        );
    }

    #[test]
    fn build_document_xml_wraps_body_with_a4_dimensions() {
        let template = make_test_template();
        let xml = build_document_xml("    <w:p>...</w:p>\n", &template);
        assert!(xml.contains(r#"w:w="11906""#));
        assert!(xml.contains(r#"w:h="16838""#));
        // Margins translated to twips
        // top 2.5cm → 1417, sx 3.5cm → 1984
        assert!(xml.contains(r#"w:top="1417""#));
        assert!(xml.contains(r#"w:left="1984""#));
    }

    #[test]
    fn build_document_xml_swaps_dimensions_in_landscape() {
        let mut t = make_test_template();
        t.paper.orientation = "landscape".to_string();
        let xml = build_document_xml("", &t);
        // Swapped.
        assert!(xml.contains(r#"w:w="16838""#));
        assert!(xml.contains(r#"w:h="11906""#));
    }

    fn make_test_template() -> crate::presets::docx_template::DocxTemplate {
        use crate::presets::docx_template::*;
        let mut display = std::collections::HashMap::new();
        display.insert("it".to_string(), "T".to_string());
        DocxTemplate {
            schema_version: 1,
            id: "it/test".into(),
            display_name: display,
            category: "test".into(),
            domain: "legal".into(),
            also_applicable_to: Vec::new(),
            locale: "it-IT".into(),
            automation_level: "L1".into(),
            placeholder_syntax: "square_brackets".into(),
            source_reference: None,
            paper: Paper {
                size: "A4".into(),
                orientation: "portrait".into(),
                format: "standard".into(),
            },
            uso_bollo: None,
            margins_cm: MarginsCm { top: 2.5, right: 2.5, bottom: 2.5, left: 3.5 },
            typography: Typography {
                body_font: "Times New Roman".into(),
                body_size_pt: 12.0,
                line_spacing: 1.5,
                paragraph_after_pt: 0.0,
                alignment: "justify".into(),
                first_line_indent_cm: 0.0,
            },
            footnotes: None,
            style_map_baseline: std::collections::BTreeMap::new(),
            style_map: std::collections::BTreeMap::new(),
            directives_supported: vec![],
            header_block: None,
            footer_block: None,
            section_numbering: "manual".into(),
            section_skeleton: vec![],
            field_prompts: std::collections::BTreeMap::new(),
            required_metadata: vec![],
            character_limits: None,
            few_shot_examples: vec![],
            prompt_md_extra: None,
        }
    }
}
