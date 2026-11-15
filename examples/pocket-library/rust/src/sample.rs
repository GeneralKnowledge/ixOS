//! A short book to read: UI tests start on an empty library, and the reader,
//! read aloud and the summaries need an EPUB in it. The host writes this one
//! to a temporary file and imports it like any other (`write_sample_epub`).

use std::io::{Cursor, Write};

use crate::error::LibraryError;

pub const SAMPLE_TITLE: &str = "The Lighthouse Keeper";

const CHAPTERS: [(&str, &[&str]); 2] = [
    (
        "The Storm",
        &[
            "The storm arrived an hour after sunset, as the old keeper had said it would.",
            "Wind pressed against the tower and rain ran down the glass in sheets.",
            "She climbed the stairs, trimmed the wick and watched the beam turn over the water.",
        ],
    ),
    (
        "Morning",
        &[
            "By morning the sea was calm and the sky had cleared to a pale blue.",
            "A small boat came round the point with the week's supplies and a letter.",
            "She read it on the gallery in the sun, and then she read it again.",
        ],
    ),
];

/// The sample EPUB's bytes: two short chapters, no cover.
pub fn sample_epub() -> Result<Vec<u8>, LibraryError> {
    build().map_err(|e| LibraryError::invalid(format!("could not write the sample book: {e}")))
}

fn build() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("mimetype", stored)?;
    zip.write_all(b"application/epub+zip")?;
    zip.start_file("META-INF/container.xml", deflated)?;
    zip.write_all(br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#)?;
    let manifest: String = (1..=CHAPTERS.len())
        .map(|n| {
            format!(r#"<item id="c{n}" href="c{n}.xhtml" media-type="application/xhtml+xml"/>"#)
        })
        .collect();
    let spine: String = (1..=CHAPTERS.len())
        .map(|n| format!(r#"<itemref idref="c{n}"/>"#))
        .collect();
    let opf = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="id">urn:uuid:7b1e4c1a-pocket-library-sample</dc:identifier>
    <dc:title>{SAMPLE_TITLE}</dc:title>
    <dc:creator>Pocket Library</dc:creator>
    <dc:language>en</dc:language>
    <dc:description>A short sample book for trying the reader.</dc:description>
  </metadata>
  <manifest>{manifest}</manifest>
  <spine>{spine}</spine>
</package>"#
    );
    zip.start_file("OEBPS/content.opf", deflated)?;
    zip.write_all(opf.as_bytes())?;
    for (n, (title, paragraphs)) in CHAPTERS.iter().enumerate() {
        let body: String = paragraphs.iter().map(|p| format!("<p>{p}</p>")).collect();
        let xhtml = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>{title}</title></head><body><h1>{title}</h1>{body}</body></html>"#
        );
        zip.start_file(format!("OEBPS/c{}.xhtml", n + 1), deflated)?;
        zip.write_all(xhtml.as_bytes())?;
    }
    Ok(zip.finish()?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_book_reads_like_any_epub() {
        let bytes = sample_epub().unwrap();
        let meta = crate::epub::read_metadata(bytes.clone()).unwrap();
        assert_eq!(meta.title, SAMPLE_TITLE);
        assert_eq!(meta.cover, None);
        crate::reader::EpubText::open(Cursor::new(&bytes)).unwrap();
    }
}
