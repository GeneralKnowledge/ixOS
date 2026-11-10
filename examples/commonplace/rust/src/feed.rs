//! Reading a feed document (RSS, Atom or JSON Feed) into what Commonplace
//! stores, and turning article HTML into plain text for rows and the
//! reader. No I/O here, so all of it is tested on Linux.

use crate::model::AppError;

/// A feed document, as Commonplace keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFeed {
    pub title: Option<String>,
    pub site_url: Option<String>,
    pub articles: Vec<ParsedArticle>,
}

/// One entry of a feed document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedArticle {
    /// The feed's id for it (`guid`, Atom `id`), else one feed-rs derives
    /// from its link and title: what makes a refetched entry the same article.
    pub guid: String,
    pub title: String,
    pub url: Option<String>,
    pub author: Option<String>,
    /// The fullest body the feed gives (content, else summary), as HTML.
    pub html: String,
    pub published_at_ms: Option<i64>,
}

/// Parses a feed document. Anything that isn't one is `NotAFeed`.
pub fn parse(bytes: &[u8]) -> Result<ParsedFeed, AppError> {
    let feed = feed_rs::parser::parse(bytes).map_err(|e| AppError::NotAFeed {
        reason: e.to_string(),
    })?;
    let site_url = feed
        .links
        .iter()
        .find(|l| l.rel.as_deref().is_none_or(|r| r == "alternate"))
        .map(|l| l.href.clone());
    let articles = feed
        .entries
        .into_iter()
        .map(|entry| {
            let url = entry
                .links
                .iter()
                .find(|l| l.rel.as_deref().is_none_or(|r| r == "alternate"))
                .or(entry.links.first())
                .map(|l| l.href.clone());
            let html = entry
                .content
                .and_then(|c| c.body)
                .filter(|b| !b.trim().is_empty())
                .or_else(|| entry.summary.map(|s| s.content))
                .unwrap_or_default();
            let title = entry
                .title
                .map(|t| plain_line(&t.content))
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| {
                    // Untitled posts (microblogs) are named by their start.
                    let text = summary(&html, 60);
                    if text.is_empty() {
                        "Untitled".into()
                    } else {
                        text
                    }
                });
            ParsedArticle {
                guid: entry.id,
                title,
                url,
                author: entry
                    .authors
                    .first()
                    .map(|a| a.name.trim().to_string())
                    .filter(|a| !a.is_empty()),
                html,
                published_at_ms: entry
                    .published
                    .or(entry.updated)
                    .map(|d| d.timestamp_millis()),
            }
        })
        .collect();
    Ok(ParsedFeed {
        title: feed
            .title
            .map(|t| plain_line(&t.content))
            .filter(|t| !t.is_empty()),
        site_url,
        articles,
    })
}

/// Elements whose start or end ends a paragraph.
const BLOCKS: &[&str] = &[
    "p",
    "div",
    "br",
    "li",
    "ul",
    "ol",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "blockquote",
    "pre",
    "figure",
    "figcaption",
    "table",
    "tr",
    "hr",
    "section",
    "article",
    "header",
    "footer",
];

/// Elements whose content is never text.
const HIDDEN: &[&str] = &["script", "style", "head", "template", "noscript"];

/// Article HTML as plain paragraphs: tags dropped, block elements end a
/// paragraph, entities decoded, whitespace collapsed. Deliberately simple:
/// the native reader will lay out the real structure (images, links,
/// headings), so this is only what a row and the interim reader need.
pub fn paragraphs(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut hidden: Option<String> = None;
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        if hidden.is_none() {
            current.push_str(&rest[..lt]);
        }
        let after = &rest[lt + 1..];
        // `<` not starting a tag (`3 < 4`) is text.
        if !after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!') {
            if hidden.is_none() {
                current.push('<');
            }
            rest = after;
            continue;
        }
        // A comment runs to `-->`, whatever it holds.
        if after.starts_with("!--") {
            rest = after.find("-->").map_or("", |end| &after[end + 3..]);
            continue;
        }
        let Some(gt) = after.find('>') else {
            // A lone `<` is text.
            if hidden.is_none() {
                current.push('<');
            }
            rest = after;
            continue;
        };
        let tag = &after[..gt];
        rest = &after[gt + 1..];
        let closing = tag.starts_with('/');
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if let Some(open) = &hidden {
            if closing && *open == name {
                hidden = None;
            }
            continue;
        }
        if !closing && HIDDEN.contains(&name.as_str()) && !tag.ends_with('/') {
            hidden = Some(name);
            continue;
        }
        if BLOCKS.contains(&name.as_str()) {
            flush(&mut current, &mut out);
        }
    }
    if hidden.is_none() {
        current.push_str(rest);
    }
    flush(&mut current, &mut out);
    out
}

fn flush(current: &mut String, out: &mut Vec<String>) {
    let text = collapse(&decode_entities(current));
    if !text.is_empty() {
        out.push(text);
    }
    current.clear();
}

/// One line of plain text from text that may hold markup or entities (a
/// title).
pub fn plain_line(text: &str) -> String {
    paragraphs(text).join(" ")
}

/// The start of an article as one line of plain text, cut at a word
/// boundary near `max` characters, with "…" when cut.
pub fn summary(html: &str, max: usize) -> String {
    let text = paragraphs(html).join(" ");
    if text.chars().count() <= max {
        return text;
    }
    let cut: String = text.chars().take(max).collect();
    let cut = match cut.rfind(' ') {
        Some(space) if space > max / 2 => &cut[..space],
        _ => cut.as_str(),
    };
    format!("{}…", cut.trim_end_matches([' ', ',', '.', ';', ':']))
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The HTML entities feeds use, numeric ones, and the five XML ones.
fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let end = after.find(';').filter(|&e| e <= 10);
        let decoded = end.and_then(|e| entity(&after[..e]));
        match (end, decoded) {
            (Some(e), Some(c)) => {
                out.push(c);
                rest = &after[e + 1..];
            }
            _ => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "copy" => '©',
        "eacute" => 'é',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const RSS: &str = r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
  <title>Fixture &amp; Friends</title>
  <link>https://example.com/</link>
  <item>
    <title>First post</title>
    <link>https://example.com/1</link>
    <guid>post-1</guid>
    <author>ada@example.com (Ada)</author>
    <pubDate>Mon, 09 Nov 2026 10:00:00 GMT</pubDate>
    <description>&lt;p&gt;Hello &lt;b&gt;world&lt;/b&gt;.&lt;/p&gt;&lt;p&gt;Second&amp;nbsp;paragraph.&lt;/p&gt;</description>
  </item>
  <item>
    <link>https://example.com/2</link>
    <guid>post-2</guid>
    <description>A post with no title, as microblogs write them.</description>
  </item>
</channel></rss>"#;

    #[test]
    fn reads_rss() {
        let feed = parse(RSS.as_bytes()).unwrap();
        assert_eq!(feed.title.as_deref(), Some("Fixture & Friends"));
        assert_eq!(feed.site_url.as_deref(), Some("https://example.com/"));
        assert_eq!(feed.articles.len(), 2);
        let first = &feed.articles[0];
        assert_eq!(first.guid, "post-1");
        assert_eq!(first.title, "First post");
        assert_eq!(first.url.as_deref(), Some("https://example.com/1"));
        assert_eq!(first.published_at_ms, Some(1_794_218_400_000));
        assert_eq!(
            paragraphs(&first.html),
            vec!["Hello world.", "Second paragraph."]
        );
        assert_eq!(
            feed.articles[1].title,
            "A post with no title, as microblogs write them."
        );
    }

    #[test]
    fn reads_atom_and_json_feed() {
        let atom = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Atom Fixture</title>
  <link href="https://atom.example/"/>
  <id>urn:atom</id><updated>2026-11-09T10:00:00Z</updated>
  <entry>
    <title type="html">Caf&amp;eacute; notes</title>
    <id>urn:entry:1</id>
    <link rel="alternate" href="https://atom.example/1"/>
    <updated>2026-11-09T09:00:00Z</updated>
    <author><name>Grace</name></author>
    <content type="html">&lt;p&gt;Body&lt;/p&gt;</content>
  </entry>
</feed>"#;
        let feed = parse(atom.as_bytes()).unwrap();
        assert_eq!(feed.title.as_deref(), Some("Atom Fixture"));
        let entry = &feed.articles[0];
        assert_eq!(entry.title, "Café notes");
        assert_eq!(entry.author.as_deref(), Some("Grace"));
        assert_eq!(entry.url.as_deref(), Some("https://atom.example/1"));

        let json = r#"{"version": "https://jsonfeed.org/version/1.1", "title": "JSON Fixture",
            "items": [{"id": "j1", "url": "https://json.example/1", "title": "J",
                       "content_html": "<p>Hi</p>", "date_published": "2026-11-09T08:00:00Z"}]}"#;
        let feed = parse(json.as_bytes()).unwrap();
        assert_eq!(feed.title.as_deref(), Some("JSON Fixture"));
        assert_eq!(feed.articles[0].guid, "j1");
    }

    #[test]
    fn anything_else_is_not_a_feed() {
        assert!(matches!(
            parse(b"<html><body>A web page</body></html>"),
            Err(AppError::NotAFeed { .. })
        ));
        assert!(matches!(parse(b""), Err(AppError::NotAFeed { .. })));
    }

    #[test]
    fn html_becomes_plain_paragraphs() {
        let html = "<div><h2>Heading</h2><p>One &amp; <a href='x'>two</a>&#8217;s</p>\
                    <script>var x = '<p>no</p>';</script><!-- <p>hidden</p> -->\
                    <ul><li>a</li><li>b</li></ul>3 < 4</div>";
        assert_eq!(
            paragraphs(html),
            vec!["Heading", "One & two’s", "a", "b", "3 < 4"]
        );
        assert!(paragraphs("").is_empty());
        assert_eq!(plain_line("  A\n<em>title</em> "), "A title");
    }

    #[test]
    fn summaries_are_cut_at_a_word() {
        let html = "<p>The quick brown fox jumps over the lazy dog.</p>";
        assert_eq!(
            summary(html, 200),
            "The quick brown fox jumps over the lazy dog."
        );
        assert_eq!(summary(html, 20), "The quick brown fox…");
    }
}
