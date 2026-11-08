//! Exercise and glossary text links terms as `[[term-id]]` or
//! `[[term-id|shown text]]`. [`parse_term_links`] splits such text into runs
//! so a host can show linked terms its own way (a tappable link on iOS).

use super::content::Glossary;

/// A piece of text, linked to a glossary term or not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRun {
    pub text: String,
    /// The glossary term this run names, when it is a published term.
    pub term_id: Option<String>,
}

/// Splits `text` into plain and linked runs. A link's shown text is its
/// `|text` part, else the term's preferred name, else the id with dashes as
/// spaces. Links to unknown or unpublished terms become plain text. Adjacent
/// plain runs are merged; unclosed `[[` is kept as written.
pub fn parse_term_links(text: &str, glossary: &Glossary) -> Vec<TextRun> {
    let mut runs: Vec<TextRun> = Vec::new();
    let mut push = |text: &str, term_id: Option<String>| {
        if text.is_empty() {
            return;
        }
        match runs.last_mut() {
            Some(last) if last.term_id.is_none() && term_id.is_none() => last.text.push_str(text),
            _ => runs.push(TextRun {
                text: text.to_string(),
                term_id,
            }),
        }
    };

    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let Some(len) = rest[start + 2..].find("]]") else {
            break;
        };
        push(&rest[..start], None);
        let inner = &rest[start + 2..start + 2 + len];
        let (id, shown) = match inner.split_once('|') {
            Some((id, shown)) => (id.trim(), Some(shown)),
            None => (inner.trim(), None),
        };
        let term = glossary.term(id);
        let shown = shown.map(str::to_string).unwrap_or_else(|| {
            term.map_or_else(|| id.replace('-', " "), |t| t.preferred_term.to_lowercase())
        });
        push(&shown, term.map(|t| t.id.clone()));
        rest = &rest[start + 2 + len + 2..];
    }
    push(rest, None);
    runs
}

/// `text` with links replaced by their shown text.
pub fn plain_text(text: &str, glossary: &Glossary) -> String {
    parse_term_links(text, glossary)
        .into_iter()
        .map(|r| r.text)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dog_training::bundled_glossary;

    #[test]
    fn splits_links_into_runs() {
        let g = bundled_glossary();
        let runs = parse_term_links("Say [[cue]] then [[marker-word|“Yes!”]].", g);
        assert_eq!(
            runs,
            vec![
                TextRun {
                    text: "Say ".into(),
                    term_id: None
                },
                TextRun {
                    text: "cue".into(),
                    term_id: Some("cue".into())
                },
                TextRun {
                    text: " then ".into(),
                    term_id: None
                },
                TextRun {
                    text: "“Yes!”".into(),
                    term_id: Some("marker-word".into())
                },
                TextRun {
                    text: ".".into(),
                    term_id: None
                },
            ]
        );
    }

    #[test]
    fn unknown_terms_and_broken_markup_stay_readable() {
        let g = bundled_glossary();
        assert_eq!(plain_text("a [[no-such-term]] b", g), "a no such term b");
        assert_eq!(
            parse_term_links("a [[no-such-term]] b", g),
            vec![TextRun {
                text: "a no such term b".into(),
                term_id: None
            }]
        );
        assert_eq!(
            plain_text("open [[ never closed", g),
            "open [[ never closed"
        );
        assert_eq!(plain_text("", g), "");
    }

    #[test]
    fn every_link_in_the_bundled_exercises_names_a_published_term() {
        let g = bundled_glossary();
        for e in crate::dog_training::bundled_exercises() {
            let texts = [&e.purpose, &e.preparation, &e.look_for, &e.if_difficult]
                .into_iter()
                .chain(&e.steps);
            for text in texts {
                let mut rest = text.as_str();
                while let Some(start) = rest.find("[[") {
                    let end = rest[start..].find("]]").unwrap() + start;
                    let id = rest[start + 2..end].split('|').next().unwrap().trim();
                    assert!(g.term(id).is_some(), "{}: unknown term {id}", e.core.id);
                    rest = &rest[end + 2..];
                }
            }
        }
    }
}
