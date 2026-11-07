//! Reads the metadata of an EPUB file: title, author, description, cover and
//! an estimated page count. It never keeps the book's text.
//!
//! An EPUB is a zip. `META-INF/container.xml` names the package file (OPF),
//! whose `<metadata>` has Dublin Core fields, whose `<manifest>` lists every
//! file, and whose `<spine>` gives the reading order.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};

use crate::error::LibraryError;

/// Words per printed page used for the page estimate (a typical paperback).
pub const WORDS_PER_PAGE: u32 = 275;
/// Larger files are refused before unzipping anything.
pub const MAX_EPUB_BYTES: usize = 100 * 1024 * 1024;
/// Stop reading text after this much, so a hostile file cannot make us
/// inflate gigabytes. Plenty for any real book.
const MAX_TEXT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_COVER_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpubCover {
    pub bytes: Vec<u8>,
    /// `jpg`, `png`, `gif` or `webp`.
    pub extension: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpubMetadata {
    pub title: String,
    pub author: String,
    pub description: String,
    pub estimated_pages: Option<u32>,
    pub cover: Option<EpubCover>,
}

fn unsupported(reason: impl Into<String>) -> LibraryError {
    LibraryError::UnsupportedFile {
        reason: reason.into(),
    }
}

type Archive = zip::ZipArchive<Cursor<Vec<u8>>>;

/// Parses `bytes` as an EPUB. Fails with `UnsupportedFile` for anything that
/// is not a readable EPUB with a title.
pub fn read_metadata(bytes: Vec<u8>) -> Result<EpubMetadata, LibraryError> {
    if bytes.len() > MAX_EPUB_BYTES {
        return Err(unsupported("the file is larger than 100 MB"));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| unsupported(format!("not an EPUB (zip): {e}")))?;

    let container = read_text(&mut archive, "META-INF/container.xml", 1024 * 1024)?
        .ok_or_else(|| unsupported("not an EPUB: META-INF/container.xml is missing"))?;
    let opf_path = rootfile_path(&container)?;
    let opf = read_text(&mut archive, &opf_path, 8 * 1024 * 1024)?
        .ok_or_else(|| unsupported(format!("the package file {opf_path} is missing")))?;
    let package = parse_package(&opf)?;
    let base = match opf_path.rfind('/') {
        Some(i) => &opf_path[..=i],
        None => "",
    };

    let title = package
        .title
        .clone()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| unsupported("the EPUB has no title"))?;
    let author = if package.creators.is_empty() {
        String::new()
    } else {
        package.creators.join(", ")
    };

    let cover = match package.cover_href() {
        Some(href) => read_cover(&mut archive, &resolve(base, href))?,
        None => None,
    };

    let mut words: u64 = 0;
    let mut budget = MAX_TEXT_BYTES;
    for href in package.spine_hrefs() {
        if budget == 0 {
            break;
        }
        let path = resolve(base, href);
        if let Some(text) = read_text(&mut archive, &path, budget)? {
            budget = budget.saturating_sub(text.len() as u64);
            words += count_words(&text);
        }
    }
    let estimated_pages = estimate_pages(words);

    Ok(EpubMetadata {
        title,
        author,
        description: package.description.unwrap_or_default(),
        estimated_pages,
        cover,
    })
}

/// Pages for `words` words at [`WORDS_PER_PAGE`], rounded up; `None` for a
/// book with no text.
pub fn estimate_pages(words: u64) -> Option<u32> {
    if words == 0 {
        return None;
    }
    let pages = words.div_ceil(u64::from(WORDS_PER_PAGE));
    Some(u32::try_from(pages).unwrap_or(u32::MAX))
}

/// Reads one entry as UTF-8 text, at most `limit` bytes. `None` if absent.
fn read_text(
    archive: &mut Archive,
    name: &str,
    limit: u64,
) -> Result<Option<String>, LibraryError> {
    let Some(bytes) = read_entry(archive, name, limit)? else {
        return Ok(None);
    };
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

fn read_entry(
    archive: &mut Archive,
    name: &str,
    limit: u64,
) -> Result<Option<Vec<u8>>, LibraryError> {
    let mut entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(unsupported(format!("{name}: {e}"))),
    };
    let mut bytes = Vec::new();
    (&mut entry)
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|e| unsupported(format!("{name}: {e}")))?;
    Ok(Some(bytes))
}

fn read_cover(archive: &mut Archive, path: &str) -> Result<Option<EpubCover>, LibraryError> {
    let Some(bytes) = read_entry(archive, path, MAX_COVER_BYTES + 1)? else {
        return Ok(None);
    };
    if bytes.len() as u64 > MAX_COVER_BYTES {
        return Ok(None);
    }
    Ok(image_extension(&bytes).map(|extension| EpubCover { bytes, extension }))
}

/// The file type of an image from its first bytes, or `None` if it is not
/// one the app shows.
pub fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(b"GIF8") {
        Some("gif")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// Joins `href` (relative to the package file, possibly percent-encoded) to
/// the package's folder, resolving `..`.
fn resolve(base: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or(href);
    let decoded = percent_decode(href);
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for part in decoded.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 3 <= bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn xml_reader(xml: &str) -> Reader<&[u8]> {
    let mut reader = Reader::from_reader(xml.as_bytes());
    reader.config_mut().check_end_names = false;
    reader
}

fn attribute(e: &quick_xml::events::BytesStart<'_>, local: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        (a.key.local_name().as_ref() == local).then(|| {
            a.normalized_value(XmlVersion::Implicit1_0)
                .map(|v| v.into_owned())
                .unwrap_or_else(|_| a.value.to_string())
        })
    })
}

fn rootfile_path(container: &str) -> Result<String, LibraryError> {
    let mut reader = xml_reader(container);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.local_name().as_ref() == "rootfile" => {
                if let Some(path) = attribute(&e, "full-path") {
                    return Ok(path);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(unsupported(format!("container.xml: {e}"))),
            _ => {}
        }
    }
    Err(unsupported("container.xml names no package file"))
}

#[derive(Debug, Default)]
struct Package {
    title: Option<String>,
    creators: Vec<String>,
    description: Option<String>,
    cover_meta_id: Option<String>,
    /// id → (href, media type, properties)
    manifest: HashMap<String, (String, String, String)>,
    spine: Vec<String>,
}

impl Package {
    fn cover_href(&self) -> Option<&str> {
        let is_image = |media: &str| media.starts_with("image/");
        // EPUB 3: properties="cover-image".
        if let Some((href, _, _)) = self.manifest.values().find(|(_, media, props)| {
            is_image(media) && props.split_whitespace().any(|p| p == "cover-image")
        }) {
            return Some(href);
        }
        // EPUB 2: <meta name="cover" content="item-id"/>.
        if let Some(id) = &self.cover_meta_id {
            if let Some((href, media, _)) = self.manifest.get(id) {
                if is_image(media) {
                    return Some(href);
                }
            }
        }
        // Last resort: an image whose id or file name says "cover".
        let mut candidates: Vec<_> = self
            .manifest
            .iter()
            .filter(|(id, (href, media, _))| {
                is_image(media)
                    && (id.to_lowercase().contains("cover")
                        || href.to_lowercase().contains("cover"))
            })
            .map(|(_, (href, _, _))| href.as_str())
            .collect();
        candidates.sort_unstable();
        candidates.first().copied()
    }

    fn spine_hrefs(&self) -> impl Iterator<Item = &str> {
        self.spine.iter().filter_map(|id| {
            self.manifest
                .get(id)
                .filter(|(_, media, _)| media.contains("html"))
                .map(|(href, _, _)| href.as_str())
        })
    }
}

fn parse_package(opf: &str) -> Result<Package, LibraryError> {
    let mut reader = xml_reader(opf);
    let mut package = Package::default();
    // The Dublin Core element whose text we are collecting, if any.
    let mut collecting: Option<&'static str> = None;
    let mut text = String::new();
    loop {
        let event = reader
            .read_event()
            .map_err(|e| unsupported(format!("package file: {e}")))?;
        match event {
            Event::Start(e) => match e.local_name().as_ref() {
                "title" if package.title.is_none() => collecting = Some("title"),
                "creator" => collecting = Some("creator"),
                "description" if package.description.is_none() => collecting = Some("description"),
                _ => {}
            },
            Event::Empty(e) => match e.local_name().as_ref() {
                "meta" => {
                    if attribute(&e, "name").as_deref() == Some("cover") {
                        package.cover_meta_id = attribute(&e, "content");
                    }
                }
                "item" => {
                    if let (Some(id), Some(href)) = (attribute(&e, "id"), attribute(&e, "href")) {
                        let media = attribute(&e, "media-type").unwrap_or_default();
                        let props = attribute(&e, "properties").unwrap_or_default();
                        package.manifest.insert(id, (href, media, props));
                    }
                }
                "itemref" => {
                    if let Some(id) = attribute(&e, "idref") {
                        package.spine.push(id);
                    }
                }
                _ => {}
            },
            Event::Text(t) if collecting.is_some() => text.push_str(&t.xml10_content()),
            Event::CData(t) if collecting.is_some() => text.push_str(&t.xml10_content()),
            Event::GeneralRef(r) if collecting.is_some() => text.push_str(&resolve_entity(
                &r.xml10_content(),
                r.resolve_char_ref().ok().flatten(),
            )),
            Event::End(e) => {
                if let Some(field) = collecting {
                    let name = e.local_name();
                    let ends = matches!(
                        (field, name.as_ref()),
                        ("title", "title")
                            | ("creator", "creator")
                            | ("description", "description")
                    );
                    if ends {
                        let value = collapse_whitespace(&text);
                        match field {
                            "title" => package.title = Some(value),
                            "creator" if !value.is_empty() => package.creators.push(value),
                            "description" => package.description = Some(strip_markup(&value)),
                            _ => {}
                        }
                        text.clear();
                        collecting = None;
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(package)
}

fn resolve_entity(name: &str, char_ref: Option<char>) -> String {
    if let Some(c) = char_ref {
        return c.to_string();
    }
    match name {
        "lt" => "<",
        "gt" => ">",
        "amp" => "&",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => " ",
        "mdash" => "—",
        "ndash" => "–",
        "hellip" => "…",
        "rsquo" => "’",
        "lsquo" => "‘",
        "ldquo" => "“",
        "rdquo" => "”",
        _ => " ",
    }
    .to_string()
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Descriptions are often escaped HTML (`&lt;p&gt;…`): drop the tags.
fn strip_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    collapse_whitespace(&out)
}

/// Counts the words in an XHTML document's `<body>`, ignoring markup.
fn count_words(xhtml: &str) -> u64 {
    let mut reader = xml_reader(xhtml);
    let mut in_body = false;
    let mut skip_depth = 0u32; // inside <script>/<style>
    let mut words = 0u64;
    let mut mid_word = false;
    let mut count = |text: &str, mid_word: &mut bool| {
        for c in text.chars() {
            if c.is_alphanumeric() {
                if !*mid_word {
                    words += 1;
                    *mid_word = true;
                }
            } else if c.is_whitespace() {
                *mid_word = false;
            }
        }
    };
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                "body" => in_body = true,
                "script" | "style" => skip_depth += 1,
                // Block elements separate words even without whitespace.
                _ => mid_word = false,
            },
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                "body" => in_body = false,
                "script" | "style" => skip_depth = skip_depth.saturating_sub(1),
                _ => mid_word = false,
            },
            Ok(Event::Empty(_)) => mid_word = false,
            Ok(Event::Text(t)) if in_body && skip_depth == 0 => {
                count(&t.xml10_content(), &mut mid_word)
            }
            Ok(Event::GeneralRef(r)) if in_body && skip_depth == 0 => {
                let text = resolve_entity(&r.xml10_content(), r.resolve_char_ref().ok().flatten());
                count(&text, &mut mid_word)
            }
            Ok(Event::Eof) => break,
            // A malformed chapter still counts for what was read so far.
            Err(_) => break,
            _ => {}
        }
    }
    words
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// A small EPUB 3 built in memory: two chapters and a PNG cover.
    pub(crate) fn sample_epub(title: &str, words_per_chapter: usize, with_cover: bool) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        zip.start_file("META-INF/container.xml", deflated).unwrap();
        zip.write_all(br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        let cover_item = if with_cover {
            r#"<item id="cover-img" href="images/cover%20art.png" media-type="image/png" properties="cover-image"/>"#
        } else {
            ""
        };
        let opf = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="id">urn:uuid:1</dc:identifier>
    <dc:title>{title}</dc:title>
    <dc:creator>Ada Writer</dc:creator>
    <dc:creator>Bo Coauthor</dc:creator>
    <dc:description>&lt;p&gt;A &lt;b&gt;tale&lt;/b&gt; of two chapters &amp; more.&lt;/p&gt;</dc:description>
  </metadata>
  <manifest>
    {cover_item}
    <item id="c1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="text/ch2.xhtml" media-type="application/xhtml+xml"/>
    <item id="css" href="style.css" media-type="text/css"/>
  </manifest>
  <spine><itemref idref="c1"/><itemref idref="c2"/><itemref idref="css"/></spine>
</package>"#
        );
        zip.start_file("OEBPS/content.opf", deflated).unwrap();
        zip.write_all(opf.as_bytes()).unwrap();
        for chapter in ["ch1", "ch2"] {
            let body = vec!["word"; words_per_chapter].join(" ");
            let xhtml = format!(
                r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>Not counted here</title><style>p {{ x: y }}</style></head><body><h1>Chapter</h1><p>{body}</p></body></html>"#
            );
            zip.start_file(format!("OEBPS/text/{chapter}.xhtml"), deflated)
                .unwrap();
            zip.write_all(xhtml.as_bytes()).unwrap();
        }
        if with_cover {
            zip.start_file("OEBPS/images/cover art.png", stored)
                .unwrap();
            zip.write_all(b"\x89PNG\r\n\x1a\nnot really a png").unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn reads_title_authors_description_cover_and_pages() {
        let meta = read_metadata(sample_epub("The Test Book", 1000, true)).unwrap();
        assert_eq!(meta.title, "The Test Book");
        assert_eq!(meta.author, "Ada Writer, Bo Coauthor");
        assert_eq!(meta.description, "A tale of two chapters & more.");
        // 2 × (1 heading word + 1000 words) = 2002 words → 8 pages at 275/page.
        assert_eq!(meta.estimated_pages, Some(8));
        let cover = meta.cover.unwrap();
        assert_eq!(cover.extension, "png");
        assert!(cover.bytes.starts_with(b"\x89PNG"));
    }

    #[test]
    fn a_book_without_a_cover_still_imports() {
        let meta = read_metadata(sample_epub("No Cover", 10, false)).unwrap();
        assert_eq!(meta.cover, None);
        assert_eq!(meta.estimated_pages, Some(1));
    }

    #[test]
    fn not_an_epub_is_unsupported() {
        assert!(matches!(
            read_metadata(b"plain text".to_vec()),
            Err(LibraryError::UnsupportedFile { .. })
        ));
        // A zip without container.xml.
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("hello.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"hi").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let err = read_metadata(bytes).unwrap_err();
        assert!(
            matches!(err, LibraryError::UnsupportedFile { ref reason } if reason.contains("container.xml")),
            "{err:?}"
        );
    }

    #[test]
    fn untitled_epub_is_refused() {
        let bytes = sample_epub("", 10, false);
        assert!(matches!(
            read_metadata(bytes),
            Err(LibraryError::UnsupportedFile { .. })
        ));
    }

    #[test]
    fn paths_resolve_relative_to_the_package() {
        assert_eq!(resolve("OEBPS/", "text/ch1.xhtml"), "OEBPS/text/ch1.xhtml");
        assert_eq!(
            resolve("OEBPS/text/", "../images/a%20b.jpg"),
            "OEBPS/images/a b.jpg"
        );
        assert_eq!(resolve("", "ch.xhtml#frag"), "ch.xhtml");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn page_estimate_rounds_up() {
        assert_eq!(estimate_pages(0), None);
        assert_eq!(estimate_pages(1), Some(1));
        assert_eq!(estimate_pages(275), Some(1));
        assert_eq!(estimate_pages(276), Some(2));
    }

    #[test]
    fn image_types_are_sniffed() {
        assert_eq!(image_extension(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(image_extension(b"GIF89a"), Some("gif"));
        assert_eq!(image_extension(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(image_extension(b"<html>"), None);
    }
}
