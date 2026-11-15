//! Reads an EPUB's text for the reader: the chapters in reading order with
//! their titles, and each chapter as a ForgeReader section of styled blocks
//! (`forge_reader::blocks_from_html` turns its XHTML into them) for the
//! host to lay out natively.
//!
//! An [`EpubText`] keeps the zip open, so turning to a chapter reads only
//! that chapter.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use forge_reader::{HtmlResources, ReaderBlockKind, ReaderSection};
use forge_storage::FileStore;
use quick_xml::events::{BytesStart, Event};

use crate::epub::{
    attribute, count_words, folder_of, image_extension, open_package, read_entry, read_text,
    resolve, resolve_entity, unsupported, xml_reader, Archive, MAX_COVER_BYTES, MAX_TEXT_BYTES,
};
use crate::error::LibraryError;
use crate::model::ChapterSummary;

/// One chapter is at most this much XHTML (a whole novel is ~1 MB).
const MAX_CHAPTER_BYTES: u64 = 16 * 1024 * 1024;
/// At most this many images are saved per chapter.
const MAX_IMAGES_PER_CHAPTER: usize = 200;

/// An open EPUB: its chapters, and the zip to read them from.
pub struct EpubText<R: Read + Seek = File> {
    archive: Archive<R>,
    /// Zip paths of the reading order's XHTML files.
    paths: Vec<String>,
    chapters: Vec<ChapterSummary>,
}

impl EpubText<File> {
    pub fn open_file(path: &Path) -> Result<Self, LibraryError> {
        let file = File::open(path).map_err(|e| LibraryError::NotReadable {
            reason: format!("the saved EPUB could not be opened ({e}); import it again"),
        })?;
        Self::open(file)
    }
}

impl<R: Read + Seek> EpubText<R> {
    /// Reads the package, the table of contents and every chapter once (for
    /// titles and word counts).
    pub fn open(reader: R) -> Result<Self, LibraryError> {
        let mut archive = zip::ZipArchive::new(reader)
            .map_err(|e| unsupported(format!("not an EPUB (zip): {e}")))?;
        let (package, base) = open_package(&mut archive)?;
        let paths: Vec<String> = package
            .spine_hrefs()
            .map(|href| resolve(&base, href))
            .collect();
        if paths.is_empty() {
            return Err(unsupported("the EPUB has no chapters"));
        }

        let mut toc = HashMap::new();
        if let Some(href) = package.nav_href() {
            let path = resolve(&base, href);
            if let Some(nav) = read_text(&mut archive, &path, 4 * 1024 * 1024)? {
                toc = nav_titles(&nav, folder_of(&path));
            }
        }
        if toc.is_empty() {
            if let Some(href) = package.ncx_href() {
                let path = resolve(&base, href);
                if let Some(ncx) = read_text(&mut archive, &path, 4 * 1024 * 1024)? {
                    toc = ncx_titles(&ncx, folder_of(&path));
                }
            }
        }

        let mut chapters = Vec::with_capacity(paths.len());
        let mut budget = MAX_TEXT_BYTES;
        let mut previous_title: Option<String> = None;
        for (index, path) in paths.iter().enumerate() {
            let text = if budget > 0 {
                read_text(&mut archive, path, budget.min(MAX_CHAPTER_BYTES))?
            } else {
                None
            };
            let (words, heading) = match &text {
                Some(text) => {
                    budget = budget.saturating_sub(text.len() as u64);
                    (count_words(text), first_heading(text))
                }
                None => (0, None),
            };
            let listed = toc.get(path).cloned();
            let title = listed
                .clone()
                .or(heading)
                .or_else(|| previous_title.clone())
                .unwrap_or_else(|| format!("Section {}", index + 1));
            previous_title = Some(title.clone());
            chapters.push(ChapterSummary {
                index: index as u32,
                title,
                in_contents: listed.is_some(),
                words: u32::try_from(words).unwrap_or(u32::MAX),
            });
        }
        Ok(Self {
            archive,
            paths,
            chapters,
        })
    }

    pub fn chapters(&self) -> &[ChapterSummary] {
        &self.chapters
    }

    pub fn chapter_words(&self) -> Vec<u32> {
        self.chapters.iter().map(|c| c.words).collect()
    }

    /// Chapter `index` as a reader section (its id is the chapter's path
    /// in the zip). Its images are saved in `images` and named by absolute
    /// path.
    pub fn chapter(
        &mut self,
        index: u32,
        images: &FileStore,
    ) -> Result<ReaderSection, LibraryError> {
        let path = self
            .paths
            .get(index as usize)
            .ok_or_else(|| LibraryError::invalid(format!("the book has no chapter {index}")))?
            .clone();
        let xhtml = read_text(&mut self.archive, &path, MAX_CHAPTER_BYTES)?.ok_or_else(|| {
            LibraryError::NotReadable {
                reason: format!("{path} is missing from the EPUB"),
            }
        })?;
        let folder = folder_of(&path).to_string();
        let mut saver = ImageSaver {
            archive: &mut self.archive,
            folder: &folder,
            images,
            saved: HashMap::new(),
        };
        let blocks = forge_reader::blocks_from_html(&xhtml, &mut saver);
        Ok(ReaderSection {
            id: path,
            title: self.chapters[index as usize].title.clone(),
            blocks,
        })
    }
}

impl<R: Read + Seek> EpubText<R> {
    /// The paragraphs of chapter `index` as plain text (headings, quotes
    /// and list items included; images and scene breaks left out), for
    /// summaries. Saves no images.
    pub fn chapter_text(&mut self, index: u32) -> Result<Vec<String>, LibraryError> {
        let path = self
            .paths
            .get(index as usize)
            .ok_or_else(|| LibraryError::invalid(format!("the book has no chapter {index}")))?
            .clone();
        let Some(xhtml) = read_text(&mut self.archive, &path, MAX_CHAPTER_BYTES)? else {
            return Ok(Vec::new());
        };
        Ok(forge_reader::blocks_from_html(&xhtml, &mut NoImages)
            .into_iter()
            .filter_map(|block| match &block.kind {
                ReaderBlockKind::Image { .. } | ReaderBlockKind::Separator => None,
                ReaderBlockKind::ListItem { marker } => Some(format!("{marker} {}", block.text())),
                _ => Some(block.text()),
            })
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .collect())
    }
}

/// Leaves pictures out (summaries read text only).
struct NoImages;

impl HtmlResources for NoImages {
    fn image(&mut self, _src: &str) -> Option<String> {
        None
    }
}

impl<R: Read + Seek> HtmlResources for ImageSaver<'_, R> {
    fn image(&mut self, src: &str) -> Option<String> {
        self.save(src)
    }
}

/// Copies images a chapter refers to out of the zip, once each.
struct ImageSaver<'a, R: Read + Seek> {
    archive: &'a mut Archive<R>,
    folder: &'a str,
    images: &'a FileStore,
    saved: HashMap<String, Option<PathBuf>>,
}

impl<R: Read + Seek> ImageSaver<'_, R> {
    fn save(&mut self, src: &str) -> Option<String> {
        if src.starts_with("data:") || src.contains("://") {
            return None;
        }
        let path = resolve(self.folder, src);
        if !self.saved.contains_key(&path) {
            let file = if self.saved.len() >= MAX_IMAGES_PER_CHAPTER {
                None
            } else {
                self.copy(&path)
            };
            self.saved.insert(path.clone(), file);
        }
        self.saved[&path]
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
    }

    fn copy(&mut self, path: &str) -> Option<PathBuf> {
        let bytes = read_entry(self.archive, path, MAX_COVER_BYTES + 1).ok()??;
        if bytes.len() as u64 > MAX_COVER_BYTES {
            return None;
        }
        let extension = image_extension(&bytes)?;
        let stem: String = path
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let name = format!("{stem}.{extension}");
        self.images.write(&name, &bytes).ok().map(|file| file.path)
    }
}

// MARK: Table of contents

/// EPUB 3: the `<nav epub:type="toc">` list (or the first `<nav>`).
/// Returns zip path → title; the first entry for a file wins.
fn nav_titles(xhtml: &str, folder: &str) -> HashMap<String, String> {
    let mut reader = xml_reader(xhtml);
    let mut titles = HashMap::new();
    // Depth inside the chosen nav; None before it, Some(0) after it ends.
    let mut nav_depth: Option<u32> = None;
    let mut done = false;
    let mut link: Option<(String, String)> = None; // (href, text so far)
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.local_name();
                match (name.as_ref(), nav_depth) {
                    ("nav", None) if !done => {
                        let kind = attribute(&e, "type").unwrap_or_default();
                        if kind.is_empty() || kind.split_whitespace().any(|k| k == "toc") {
                            nav_depth = Some(1);
                        }
                    }
                    ("nav", Some(depth)) => nav_depth = Some(depth + 1),
                    ("a", Some(_)) => {
                        link = attribute(&e, "href").map(|href| (href, String::new()));
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => match (e.local_name().as_ref(), nav_depth) {
                ("nav", Some(1)) => {
                    nav_depth = None;
                    done = true;
                }
                ("nav", Some(depth)) => nav_depth = Some(depth - 1),
                ("a", Some(_)) => {
                    if let Some((href, text)) = link.take() {
                        add_title(&mut titles, folder, &href, &text);
                    }
                }
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if let Some((_, text)) = &mut link {
                    text.push_str(&t.xml10_content());
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if let Some((_, text)) = &mut link {
                    text.push_str(&resolve_entity(
                        &r.xml10_content(),
                        r.resolve_char_ref().ok().flatten(),
                    ));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        if done {
            break;
        }
    }
    titles
}

/// EPUB 2: each `<navPoint>`'s `<navLabel><text>` and `<content src>`.
fn ncx_titles(ncx: &str, folder: &str) -> HashMap<String, String> {
    let mut reader = xml_reader(ncx);
    let mut titles = HashMap::new();
    let mut in_text = false;
    let mut label = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                "navLabel" => label.clear(),
                "text" => in_text = true,
                "content" => content_src(&e, &mut titles, folder, &label),
                _ => {}
            },
            Ok(Event::Empty(e)) if e.local_name().as_ref() == "content" => {
                content_src(&e, &mut titles, folder, &label)
            }
            Ok(Event::End(e)) if e.local_name().as_ref() == "text" => in_text = false,
            Ok(Event::Text(t)) if in_text => label.push_str(&t.xml10_content()),
            Ok(Event::GeneralRef(r)) if in_text => label.push_str(&resolve_entity(
                &r.xml10_content(),
                r.resolve_char_ref().ok().flatten(),
            )),
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    titles
}

fn content_src(
    e: &BytesStart<'_>,
    titles: &mut HashMap<String, String>,
    folder: &str,
    label: &str,
) {
    if let Some(src) = attribute(e, "src") {
        add_title(titles, folder, &src, label);
    }
}

fn add_title(titles: &mut HashMap<String, String>, folder: &str, href: &str, text: &str) {
    let text = collapse(text);
    if !text.is_empty() {
        titles.entry(resolve(folder, href)).or_insert(text);
    }
}

/// The text of a chapter's first `<h1>`–`<h3>`, for chapters the table of
/// contents does not name.
fn first_heading(xhtml: &str) -> Option<String> {
    let mut reader = xml_reader(xhtml);
    let mut depth = 0u32;
    let mut text = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                if matches!(e.local_name().as_ref(), "h1" | "h2" | "h3") {
                    depth += 1;
                } else if depth > 0 && e.local_name().as_ref() == "br" {
                    text.push(' ');
                }
            }
            Ok(Event::Empty(e)) if depth > 0 && e.local_name().as_ref() == "br" => text.push(' '),
            Ok(Event::End(e)) => {
                if depth > 0 && matches!(e.local_name().as_ref(), "h1" | "h2" | "h3") {
                    depth -= 1;
                    if depth == 0 {
                        let title = collapse(&text);
                        if !title.is_empty() {
                            return Some(title);
                        }
                        text.clear();
                    }
                }
            }
            Ok(Event::Text(t)) if depth > 0 => text.push_str(&t.xml10_content()),
            Ok(Event::GeneralRef(r)) if depth > 0 => text.push_str(&resolve_entity(
                &r.xml10_content(),
                r.resolve_char_ref().ok().flatten(),
            )),
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epub::tests::sample_epub;
    use forge_reader::ReaderBlock;
    use std::io::{Cursor, Write};

    fn texts(blocks: &[ReaderBlock]) -> Vec<String> {
        blocks.iter().map(ReaderBlock::text).collect()
    }

    /// An EPUB 2 book (NCX contents) whose first chapter is split across
    /// two files, with a cover page that is only an image.
    fn split_epub() -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        let mut add = |name: &str, body: &[u8]| {
            zip.start_file(name, options).unwrap();
            zip.write_all(body).unwrap();
        };
        add("mimetype", b"application/epub+zip");
        add(
            "META-INF/container.xml",
            br#"<container><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#,
        );
        add(
            "content.opf",
            br#"<package><metadata><dc:title xmlns:dc="dc">Split</dc:title></metadata><manifest>
<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
<item id="cover" href="cover.xhtml" media-type="application/xhtml+xml"/>
<item id="a" href="text/one.xhtml" media-type="application/xhtml+xml"/>
<item id="b" href="text/one_split.xhtml" media-type="application/xhtml+xml"/>
<item id="c" href="text/two.xhtml" media-type="application/xhtml+xml"/>
<item id="img" href="images/c.jpg" media-type="image/jpeg"/>
</manifest><spine toc="ncx"><itemref idref="cover"/><itemref idref="a"/><itemref idref="b"/><itemref idref="c"/></spine></package>"#,
        );
        add(
            "toc.ncx",
            br#"<ncx><navMap>
<navPoint id="n1"><navLabel><text>The  Beginning</text></navLabel><content src="text/one.xhtml"/></navPoint>
<navPoint id="n2"><navLabel><text>Middle</text></navLabel><content src="text/two.xhtml#start"/></navPoint>
</navMap></ncx>"#,
        );
        add(
            "cover.xhtml",
            br#"<html><body><svg><image href="images/c.jpg"/></svg></body></html>"#,
        );
        add(
            "text/one.xhtml",
            b"<html><body><h1>One</h1><p>alpha beta</p></body></html>",
        );
        add(
            "text/one_split.xhtml",
            b"<html><body><p>gamma delta epsilon</p></body></html>",
        );
        add(
            "text/two.xhtml",
            b"<html><body><h2>Two</h2><p>zeta</p></body></html>",
        );
        add("images/c.jpg", &[0xFF, 0xD8, 0xFF]);
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn chapters_take_titles_from_the_ncx_and_follow_on() {
        let mut book = EpubText::open(Cursor::new(split_epub())).unwrap();
        let titles: Vec<_> = book.chapters().iter().map(|c| c.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["Section 1", "The Beginning", "The Beginning", "Middle"]
        );
        let listed: Vec<_> = book.chapters().iter().map(|c| c.in_contents).collect();
        assert_eq!(listed, vec![false, true, false, true]);
        assert_eq!(book.chapter_words(), vec![0, 3, 3, 2]);

        let dir = tempfile::tempdir().unwrap();
        let chapter = book.chapter(2, &FileStore::new(dir.path())).unwrap();
        assert_eq!(texts(&chapter.blocks), vec!["gamma delta epsilon"]);
        assert_eq!(chapter.title, "The Beginning");
        assert!(book.chapter(4, &FileStore::new(dir.path())).is_err());
    }

    #[test]
    fn cover_page_images_are_saved_for_the_host() {
        let mut book = EpubText::open(Cursor::new(split_epub())).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let cover = book.chapter(0, &FileStore::new(dir.path())).unwrap();
        // The 3-byte "jpg" is a real JPEG signature, so it is saved…
        let [ReaderBlock {
            kind: ReaderBlockKind::Image { source: file, .. },
            ..
        }] = cover.blocks.as_slice()
        else {
            panic!("{:?}", cover.blocks)
        };
        assert!(Path::new(file).starts_with(dir.path()));
        assert!(Path::new(file).exists());
    }

    #[test]
    fn epub3_nav_titles_name_the_chapters() {
        // sample_epub has no nav document: titles come from the headings.
        let book = EpubText::open(Cursor::new(sample_epub("Nav", 5, true))).unwrap();
        let titles: Vec<_> = book.chapters().iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, vec!["Chapter", "Chapter"]);

        let nav = r#"<html xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="landmarks"><ol><li><a href="text/ch1.xhtml">Wrong</a></li></ol></nav>
<nav epub:type="toc"><ol><li><a href="text/ch1.xhtml">First &amp; Foremost</a>
<ol><li><a href="text/ch1.xhtml#s2">Later</a></li></ol></li>
<li><a href="text/ch2.xhtml"><span>Second</span></a></li></ol></nav></body></html>"#;
        let titles = nav_titles(nav, "OEBPS/");
        assert_eq!(titles["OEBPS/text/ch1.xhtml"], "First & Foremost");
        assert_eq!(titles["OEBPS/text/ch2.xhtml"], "Second");
    }
}
