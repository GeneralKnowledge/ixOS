//! OPML: subscriptions in and out of Commonplace.
//!
//! OPML is an interchange format here, never the library's model. Reading a
//! file gives a list of [`Entry`]s (an address, a name, the folder path it
//! sat in, the options Commonplace understands, and the rest of its
//! attributes kept as text); the library decides what to do with them.
//! Writing walks the library's own folders and feeds.
//!
//! Feeder's OPML (from its public 2.5.0 notes, namespace
//! `https://nononsenseapps.com/feeder`) carries per-feed `notify` and
//! `fullTextByDefault`, other per-feed attributes, global `setting`s and
//! block-list `blocked` patterns. Commonplace maps the two per-feed options
//! it has, keeps every other namespaced feed attribute so an export writes
//! it back, imports block patterns as plain text, and names everything it
//! can't use so the import screen can say so.
//!
//! An imported file is untrusted: its size, the number of outlines, folder
//! depth and every name and pattern are limited, nothing is fetched while
//! reading it, no entity beyond XML's five is expanded (quick-xml expands
//! none), and a filter pattern is text that is never run.

use std::collections::BTreeSet;

use quick_xml::escape::escape;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use quick_xml::{NsReader, XmlVersion};
use serde::{Deserialize, Serialize};

/// Feeder's namespace (from its public OPML notes).
pub const FEEDER_NS: &str = "https://nononsenseapps.com/feeder";
/// Commonplace's own namespace for what OPML has no word for.
pub const COMMONPLACE_NS: &str = "https://github.com/GeneralKnowledge/ixOS/commonplace";

/// The largest OPML file read. A subscription list is a few hundred KB at
/// most; this keeps a wrong file from filling memory.
pub const MAX_BYTES: usize = 5 * 1024 * 1024;
/// The most outlines read from one file.
pub const MAX_OUTLINES: usize = 10_000;
/// Folder levels kept; feeds deeper go into the deepest kept folder.
pub const MAX_DEPTH: usize = 8;
/// The longest folder or feed name, in characters (longer is cut).
pub const MAX_NAME_CHARS: usize = 200;
/// The most filter patterns imported, and the longest one.
pub const MAX_FILTERS: usize = 500;
pub const MAX_FILTER_CHARS: usize = 200;

/// Why a file couldn't be read as OPML. `reason` is a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpmlError {
    TooLarge,
    NotOpml { reason: String },
}

/// One subscription found in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// `xmlUrl`, as written.
    pub url: String,
    /// `text` (else `title`), cleaned; none if blank.
    pub title: Option<String>,
    /// `htmlUrl`.
    pub site_url: Option<String>,
    /// The folders it sits in, outermost first.
    pub folder: Vec<String>,
    pub options: FeedOptions,
    /// Namespaced attributes Commonplace doesn't use, kept for export.
    pub extras: Vec<Extra>,
    /// Names of the settings above, and of values it couldn't read.
    pub not_used: Vec<String>,
}

/// The per-feed options Commonplace has. None: the file didn't say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeedOptions {
    pub notify: Option<bool>,
    pub full_text: Option<bool>,
}

/// A namespaced attribute kept as text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extra {
    pub namespace: String,
    pub name: String,
    pub value: String,
}

/// What a file holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Document {
    pub entries: Vec<Entry>,
    /// Filter patterns that passed the checks, without repeats.
    pub filters: Vec<String>,
    /// Patterns refused (blank, too long, control characters, too many).
    pub rejected_filters: u32,
    /// Global settings in the file Commonplace doesn't carry over.
    pub settings_not_used: Vec<String>,
}

/// Reads an OPML file. Never touches the network or the library.
pub fn parse(text: &str) -> Result<Document, OpmlError> {
    if text.len() > MAX_BYTES {
        return Err(OpmlError::TooLarge);
    }
    let mut reader = NsReader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut doc = Document::default();
    let mut filters = BTreeSet::new();
    let mut settings = BTreeSet::new();
    // One per open outline: the folder name it adds, if it is a folder.
    let mut open: Vec<Option<String>> = Vec::new();
    let mut seen_root = false;
    let mut outlines = 0usize;
    loop {
        let event = reader.read_event().map_err(|e| OpmlError::NotOpml {
            reason: e.to_string(),
        })?;
        let (start, has_children) = match &event {
            Event::Start(e) => (e, true),
            Event::Empty(e) => (e, false),
            Event::End(e) => {
                if e.local_name().as_ref() == "outline"
                    && namespace_of_element(&reader, e.name()).is_none()
                {
                    open.pop();
                }
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let element_ns = namespace_of_element(&reader, start.name());
        let local = start.local_name().as_ref().to_string();
        if !seen_root {
            if local != "opml" || element_ns.is_some() {
                return Err(OpmlError::NotOpml {
                    reason: format!("the root element is <{local}>"),
                });
            }
            seen_root = true;
            continue;
        }
        match (element_ns.as_deref(), local.as_str()) {
            (None, "outline") => {
                outlines += 1;
                if outlines > MAX_OUTLINES {
                    return Err(OpmlError::NotOpml {
                        reason: format!("more than {MAX_OUTLINES} outlines"),
                    });
                }
                let attributes = read_attributes(&reader, start)?;
                let pushed = outline(&attributes, &open, &mut doc.entries);
                if has_children {
                    open.push(pushed);
                }
            }
            (Some(FEEDER_NS), "blocked") | (Some(COMMONPLACE_NS), "filter") => {
                let attributes = read_attributes(&reader, start)?;
                if let Some(pattern) = attributes
                    .iter()
                    .find(|a| a.namespace.is_none() && a.name == "pattern")
                {
                    match clean_filter(&pattern.value) {
                        Some(p) if filters.len() < MAX_FILTERS => {
                            filters.insert(p);
                        }
                        _ => doc.rejected_filters += 1,
                    }
                }
            }
            (Some(FEEDER_NS), "setting") => {
                let attributes = read_attributes(&reader, start)?;
                if let Some(key) = attributes
                    .iter()
                    .find(|a| a.namespace.is_none() && a.name == "key")
                {
                    settings.insert(clean_name(&key.value).unwrap_or_else(|| "setting".into()));
                }
            }
            _ => {}
        }
    }
    if !seen_root {
        return Err(OpmlError::NotOpml {
            reason: "the file is empty".into(),
        });
    }
    doc.filters = filters.into_iter().collect();
    doc.settings_not_used = settings.into_iter().collect();
    Ok(doc)
}

/// An attribute with its namespace resolved.
struct Attribute {
    namespace: Option<String>,
    name: String,
    value: String,
}

fn namespace_of_element(
    reader: &NsReader<&[u8]>,
    name: quick_xml::name::QName<'_>,
) -> Option<String> {
    match reader.resolver().resolve_element(name).0 {
        ResolveResult::Bound(ns) => Some(ns.into_inner().to_string()),
        _ => None,
    }
}

fn read_attributes(
    reader: &NsReader<&[u8]>,
    start: &BytesStart<'_>,
) -> Result<Vec<Attribute>, OpmlError> {
    let mut out = Vec::new();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|e| OpmlError::NotOpml {
            reason: e.to_string(),
        })?;
        let key = attribute.key.into_inner();
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let (resolved, local) = reader.resolver().resolve_attribute(attribute.key);
        let namespace = match resolved {
            ResolveResult::Bound(ns) => Some(ns.into_inner().to_string()),
            // A prefix nobody declared: kept under its prefix's name.
            ResolveResult::Unknown(prefix) => Some(prefix),
            ResolveResult::Unbound => None,
        };
        let value = attribute
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(|e| OpmlError::NotOpml {
                reason: e.to_string(),
            })?
            .into_owned();
        out.push(Attribute {
            namespace,
            name: local.into_inner().to_string(),
            value,
        });
    }
    Ok(out)
}

/// Reads one outline: a feed is added to `entries`; a folder returns its
/// name (for the outlines inside it).
fn outline(
    attributes: &[Attribute],
    open: &[Option<String>],
    entries: &mut Vec<Entry>,
) -> Option<String> {
    let plain = |name: &str| {
        attributes
            .iter()
            .find(|a| a.namespace.is_none() && a.name.eq_ignore_ascii_case(name))
            .map(|a| a.value.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let name = plain("text")
        .or_else(|| plain("title"))
        .and_then(|n| clean_name(&n));
    let Some(url) = plain("xmlUrl") else {
        // A folder (or an outline with nothing to subscribe to).
        let depth = open.iter().filter(|f| f.is_some()).count();
        return name.filter(|_| depth < MAX_DEPTH);
    };
    let mut entry = Entry {
        url,
        title: name,
        site_url: plain("htmlUrl"),
        folder: open.iter().flatten().cloned().collect(),
        options: FeedOptions::default(),
        extras: Vec::new(),
        not_used: Vec::new(),
    };
    let mut feeder = FeedOptions::default();
    for a in attributes {
        let Some(ns) = a.namespace.as_deref() else {
            continue;
        };
        let target = match (ns, a.name.as_str()) {
            (COMMONPLACE_NS, "notify") => Some(&mut entry.options.notify),
            (COMMONPLACE_NS, "fullText") => Some(&mut entry.options.full_text),
            (FEEDER_NS, "notify") => Some(&mut feeder.notify),
            (FEEDER_NS, "fullTextByDefault") => Some(&mut feeder.full_text),
            _ => None,
        };
        match target {
            Some(slot) => match parse_bool(&a.value) {
                Some(on) => *slot = Some(on),
                None => entry.not_used.push(a.name.clone()),
            },
            None => {
                entry.extras.push(Extra {
                    namespace: ns.to_string(),
                    name: a.name.clone(),
                    value: a.value.clone(),
                });
                entry.not_used.push(a.name.clone());
            }
        }
    }
    // Commonplace's own words win where both are written.
    entry.options.notify = entry.options.notify.or(feeder.notify);
    entry.options.full_text = entry.options.full_text.or(feeder.full_text);
    entries.push(entry);
    // A feed outline with outlines inside it adds no folder.
    None
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

/// A name for the screen: control characters out, spaces collapsed, cut to
/// `MAX_NAME_CHARS`. None if nothing is left.
pub fn clean_name(raw: &str) -> Option<String> {
    let collapsed: String = raw
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let cut: String = collapsed.chars().take(MAX_NAME_CHARS).collect();
    (!cut.is_empty()).then_some(cut)
}

/// A filter pattern as Commonplace keeps it: trimmed text of at most
/// `MAX_FILTER_CHARS` with no control characters. It is matched as text
/// later, never compiled or run.
pub fn clean_filter(raw: &str) -> Option<String> {
    let pattern = raw.trim();
    let ok = !pattern.is_empty()
        && pattern.chars().count() <= MAX_FILTER_CHARS
        && !pattern.chars().any(char::is_control);
    ok.then(|| pattern.to_string())
}

/// What an export writes.
pub struct ExportFolder {
    pub name: String,
    pub folders: Vec<ExportFolder>,
    pub feeds: Vec<ExportFeed>,
}

pub struct ExportFeed {
    pub title: String,
    pub url: String,
    pub site_url: Option<String>,
    pub notify: bool,
    pub full_text: bool,
    pub extras: Vec<Extra>,
}

/// Writes OPML 2.0: folders as outlines holding their feeds; each feed with
/// Commonplace's options (also in Feeder's words, so Feeder reads them),
/// and the attributes an import kept, written back in their namespaces.
pub fn write(top: &ExportFolder, filters: &[String]) -> String {
    // Prefixes for every namespace used.
    let mut namespaces: Vec<String> = vec![COMMONPLACE_NS.into(), FEEDER_NS.into()];
    collect_namespaces(top, &mut namespaces);
    let prefix = |ns: &str| -> String {
        match ns {
            COMMONPLACE_NS => "commonplace".into(),
            FEEDER_NS => "feeder".into(),
            other => format!(
                "ns{}",
                namespaces.iter().position(|n| n == other).unwrap_or(0)
            ),
        }
    };
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<opml version=\"2.0\"");
    for ns in &namespaces {
        out.push_str(&format!(
            " xmlns:{}=\"{}\"",
            prefix(ns),
            escape(ns.as_str())
        ));
    }
    out.push_str(">\n  <head>\n    <title>Commonplace subscriptions</title>\n");
    if !filters.is_empty() {
        out.push_str("    <commonplace:filters>\n");
        for f in filters {
            out.push_str(&format!(
                "      <commonplace:filter pattern=\"{}\"/>\n",
                escape(f.as_str())
            ));
        }
        out.push_str("    </commonplace:filters>\n");
    }
    out.push_str("  </head>\n  <body>\n");
    write_folder_contents(top, 2, &prefix, &mut out);
    out.push_str("  </body>\n</opml>\n");
    out
}

fn collect_namespaces(folder: &ExportFolder, namespaces: &mut Vec<String>) {
    for feed in &folder.feeds {
        for extra in &feed.extras {
            if !namespaces.contains(&extra.namespace) {
                namespaces.push(extra.namespace.clone());
            }
        }
    }
    for sub in &folder.folders {
        collect_namespaces(sub, namespaces);
    }
}

fn write_folder_contents(
    folder: &ExportFolder,
    depth: usize,
    prefix: &dyn Fn(&str) -> String,
    out: &mut String,
) {
    let indent = "  ".repeat(depth);
    for feed in &folder.feeds {
        let title = escape(feed.title.as_str());
        out.push_str(&format!(
            "{indent}<outline type=\"rss\" text=\"{title}\" title=\"{title}\" xmlUrl=\"{}\"",
            escape(feed.url.as_str())
        ));
        if let Some(site) = &feed.site_url {
            out.push_str(&format!(" htmlUrl=\"{}\"", escape(site.as_str())));
        }
        out.push_str(&format!(
            " commonplace:notify=\"{n}\" commonplace:fullText=\"{f}\" feeder:notify=\"{n}\" feeder:fullTextByDefault=\"{f}\"",
            n = feed.notify,
            f = feed.full_text
        ));
        for extra in &feed.extras {
            // The options above are written from the library, not kept text.
            let mapped = matches!(
                (extra.namespace.as_str(), extra.name.as_str()),
                (COMMONPLACE_NS, "notify" | "fullText")
                    | (FEEDER_NS, "notify" | "fullTextByDefault")
            );
            if !mapped && is_xml_name(&extra.name) {
                out.push_str(&format!(
                    " {}:{}=\"{}\"",
                    prefix(&extra.namespace),
                    extra.name,
                    escape(extra.value.as_str())
                ));
            }
        }
        out.push_str("/>\n");
    }
    for sub in &folder.folders {
        let name = escape(sub.name.as_str());
        out.push_str(&format!(
            "{indent}<outline text=\"{name}\" title=\"{name}\">\n"
        ));
        write_folder_contents(sub, depth + 1, prefix, out);
        out.push_str(&format!("{indent}</outline>\n"));
    }
}

/// Whether a kept attribute name is safe to write as one.
fn is_xml_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}
