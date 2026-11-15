use super::*;

struct Saved;

impl HtmlResources for Saved {
    fn image(&mut self, src: &str) -> Option<String> {
        Some(format!("/saved/{src}"))
    }
}

fn blocks(xhtml: &str) -> Vec<ReaderBlock> {
    blocks_from_html(xhtml, &mut Saved)
}

fn plain(text: &str) -> ReaderSpan {
    ReaderSpan::plain(text)
}

fn texts(blocks: &[ReaderBlock]) -> Vec<String> {
    blocks.iter().map(ReaderBlock::text).collect()
}

#[test]
fn paragraphs_headings_and_styles() {
    let html = r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Skip me</title><style>p { color: red }</style></head>
<body>
  <h1 class="chapter">Chapter <br/>One</h1>
  <p>It was a <em>dark</em> and
     <strong>stormy</strong> night.</p>
  <p>   </p>
  <div><p>Nested&#160;text &amp; more</p></div>
  <p>Line one<br/>line two</p>
</body></html>"#;
    let blocks = blocks(html);
    assert_eq!(
        texts(&blocks),
        vec![
            "Chapter\nOne",
            "It was a dark and stormy night.",
            "Nested\u{a0}text & more",
            "Line one\nline two"
        ]
    );
    assert_eq!(blocks[0].kind, ReaderBlockKind::Heading { level: 1 });
    assert_eq!(blocks[1].kind, ReaderBlockKind::Paragraph);
    assert_eq!(
        blocks[1].spans,
        vec![
            plain("It was a "),
            ReaderSpan {
                italic: true,
                ..plain("dark")
            },
            plain(" and "),
            ReaderSpan {
                bold: true,
                ..plain("stormy")
            },
            plain(" night."),
        ]
    );
}

#[test]
fn quotes_lists_pre_breaks_and_images() {
    let html = r#"<html><body>
<blockquote><p>Quoted.</p></blockquote>
<ol start="3"><li>Three</li><li><p>Four</p><p>more of four</p></li></ol>
<ul><li>Dot</li></ul>
<pre>  keep
    this</pre>
<p class="center">* * *</p>
<hr/>
<p>After the break</p>
<img src="../images/map.png" alt="A  map"/>
<p>Text with <code>code</code>.</p>
</body></html>"#;
    let blocks = blocks(html);
    let kinds: Vec<_> = blocks.iter().map(|b| b.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![
            ReaderBlockKind::Quote,
            ReaderBlockKind::ListItem {
                marker: "3.".into()
            },
            ReaderBlockKind::ListItem {
                marker: "4.".into()
            },
            ReaderBlockKind::Paragraph,
            ReaderBlockKind::ListItem {
                marker: "•".into()
            },
            ReaderBlockKind::Preformatted,
            ReaderBlockKind::Separator,
            ReaderBlockKind::Paragraph,
            ReaderBlockKind::Image {
                source: "/saved/../images/map.png".into(),
                alt: "A map".into()
            },
            ReaderBlockKind::Paragraph,
        ]
    );
    assert_eq!(texts(&blocks)[5], "  keep\n    this");
    assert!(blocks[9].spans[1].monospace);
}

#[test]
fn html_style_void_tags_and_broken_markup_still_read() {
    let html = "<html><body><p>One<br>two</p><p>Unclosed <i>italic</p><p>Next</body></html>";
    assert_eq!(
        texts(&blocks(html)),
        vec!["One\ntwo", "Unclosed italic", "Next"]
    );
}

#[test]
fn every_block_has_a_stable_id_and_source_ids_win() {
    let html = r#"<body><h2 id="intro">Intro</h2><p>Same</p><p>Same</p><p><a id="note-1"></a>Note</p></body>"#;
    let blocks = blocks(html);
    assert_eq!(blocks[0].id, "intro");
    assert_eq!(blocks[2].id, format!("{}-2", blocks[1].id));
    assert_eq!(blocks[3].id, "note-1");
    // Parsing again gives the same ids.
    assert_eq!(blocks, super::blocks_from_html(html, &mut Saved));
}

#[test]
fn feed_html_fragments_with_links_and_web_images() {
    let html = r#"<p>Read <a href="/post/2">the next post</a> or <a href="javascript:alert(1)">this</a>.</p>
<p>Caf&eacute; &mdash; <b>bold <a href="https://example.org/x">linked</a></b></p>
<img src="//cdn.example.com/pic.jpg" alt="Pic">
<img src="data:image/png;base64,AAAA">
<script>ignored()</script>
<p>Fish &amp; chips < 3 R&D</p>"#;
    let blocks = blocks_from_html(
        html,
        &mut WebResources::new(Some("https://blog.example.com/post/1")),
    );
    assert_eq!(
        blocks[0].spans,
        vec![
            plain("Read "),
            ReaderSpan {
                link: Some("https://blog.example.com/post/2".into()),
                ..plain("the next post")
            },
            plain(" or this."),
        ]
    );
    assert_eq!(blocks[1].text(), "Café — bold linked");
    assert_eq!(
        blocks[1].spans.last().unwrap(),
        &ReaderSpan {
            bold: true,
            link: Some("https://example.org/x".into()),
            ..plain("linked")
        }
    );
    assert_eq!(
        blocks[2].kind,
        ReaderBlockKind::Image {
            source: "https://cdn.example.com/pic.jpg".into(),
            alt: "Pic".into()
        }
    );
    // The data: image is left out; the script's text never shows.
    assert!(blocks.iter().all(|b| !b.text().contains("ignored")));
    assert_eq!(blocks.len(), 4, "{blocks:#?}");
}

#[test]
fn plain_text_without_tags_is_one_paragraph() {
    let blocks = blocks("Just some text\nover two lines.");
    assert_eq!(texts(&blocks), vec!["Just some text over two lines."]);
}
