//! Zip assembly for the final `.docx` artefact.
//!
//! OOXML expects the archive to carry, at minimum:
//!
//! ```
//! [Content_Types].xml         (root)
//! _rels/.rels                 (root relationships → document.xml)
//! word/document.xml           (the body, produced by document_xml.rs)
//! word/styles.xml             (paragraph + run styles, produced by styles_xml.rs)
//! word/_rels/document.xml.rels (document → styles relationship)
//! ```
//!
//! Both `[Content_Types]` and the two `.rels` files are tiny boilerplate
//! with a fixed shape — kept here as const strings rather than
//! generated, because they never vary between templates.

use anyhow::Result;
use std::io::{Cursor, Write};

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
</Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

const DOCUMENT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
</Relationships>"#;

/// Assemble a complete `.docx` archive from the two generated XML
/// parts. Returns the byte buffer ready to be written to disk or
/// streamed to the client.
pub fn package_docx(document_xml: &str, styles_xml: &str) -> Result<Vec<u8>> {
    let cursor = Cursor::new(Vec::<u8>::new());
    let mut zip = zip::ZipWriter::new(cursor);
    let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("[Content_Types].xml", opts)?;
    zip.write_all(CONTENT_TYPES.as_bytes())?;

    zip.start_file("_rels/.rels", opts)?;
    zip.write_all(ROOT_RELS.as_bytes())?;

    zip.start_file("word/_rels/document.xml.rels", opts)?;
    zip.write_all(DOCUMENT_RELS.as_bytes())?;

    zip.start_file("word/styles.xml", opts)?;
    zip.write_all(styles_xml.as_bytes())?;

    zip.start_file("word/document.xml", opts)?;
    zip.write_all(document_xml.as_bytes())?;

    let cursor = zip.finish()?;
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn package_emits_required_parts() {
        let bytes =
            package_docx("<doc/>", "<styles/>").expect("package");
        // The output starts with the zip local-file header "PK\x03\x04".
        assert_eq!(&bytes[..4], b"PK\x03\x04");

        let cursor = Cursor::new(&bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("open zip");
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
            assert!(
                names.iter().any(|n| n == required),
                "missing zip entry {required}; got {names:?}"
            );
        }
    }

    #[test]
    fn package_round_trips_document_and_styles() {
        let doc = "<doc>hello</doc>";
        let styles = "<styles>world</styles>";
        let bytes = package_docx(doc, styles).expect("package");
        let cursor = Cursor::new(&bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("open");
        let mut got_doc = String::new();
        archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut got_doc)
            .unwrap();
        assert_eq!(got_doc, doc);
        let mut got_styles = String::new();
        archive
            .by_name("word/styles.xml")
            .unwrap()
            .read_to_string(&mut got_styles)
            .unwrap();
        assert_eq!(got_styles, styles);
    }
}
