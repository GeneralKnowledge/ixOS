//! Text helpers for small language models: estimating tokens, splitting a
//! long text into parts that fit a context window, and checking answers for
//! what small models add (a preamble, a refusal in words, an echo of the
//! prompt, a sentence cut off by the length cap, runs copied from the
//! source).
//!
//! Written for Pocket Library's chapter summaries on Apple's on-device model
//! (phone runs, 2026-11-14), and general: nothing here knows about books.

/// A rough token count that errs high: about four characters a token for
/// ASCII text, and a token for every other character (accented letters,
/// other scripts), whose real cost is between a third and two.
pub fn estimate_tokens(text: &str) -> usize {
    let (ascii, other) = text.chars().fold((0usize, 0usize), |(a, o), c| {
        if c.is_ascii() {
            (a + 1, o)
        } else {
            (a, o + 1)
        }
    });
    ascii.div_ceil(4) + other
}

/// Packs paragraphs into parts of at most `budget` estimated tokens, in
/// order and evenly sized, so the last part is not a scrap. A paragraph
/// longer than `budget` is split at sentence ends (or, failing that, at
/// spaces).
pub fn split_into_parts(paragraphs: &[String], budget: usize) -> Vec<String> {
    let paragraphs: Vec<&str> = paragraphs
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    let total: usize = paragraphs.iter().map(|p| estimate_tokens(p) + 1).sum();
    if total == 0 {
        return Vec::new();
    }
    let count = total.div_ceil(budget);
    let target = total.div_ceil(count);
    let limit = (target + target / 5).min(budget);
    let pieces: Vec<String> = paragraphs
        .into_iter()
        .flat_map(|p| split_long(p, limit))
        .collect();
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut size = 0;
    for piece in pieces {
        let cost = estimate_tokens(&piece) + 1;
        // A short start (a heading) stays with what follows if the part
        // still fits the budget.
        let full = size + cost > budget || (size + cost > limit && size >= limit / 4);
        if !current.is_empty() && full {
            parts.push(std::mem::take(&mut current));
            size = 0;
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(&piece);
        size += cost;
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// A paragraph in pieces of at most `budget` tokens.
fn split_long(text: &str, budget: usize) -> Vec<String> {
    if estimate_tokens(text) <= budget {
        return vec![text.to_string()];
    }
    let mut pieces = Vec::new();
    let mut current = String::new();
    for sentence in sentences(text) {
        let candidate = if current.is_empty() {
            sentence.to_string()
        } else {
            format!("{current} {sentence}")
        };
        if estimate_tokens(&candidate) <= budget {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            pieces.push(std::mem::take(&mut current));
        }
        if estimate_tokens(sentence) <= budget {
            current = sentence.to_string();
        } else {
            // One enormous "sentence": cut at spaces, or anywhere.
            for word in sentence.split_whitespace() {
                let candidate = if current.is_empty() {
                    word.to_string()
                } else {
                    format!("{current} {word}")
                };
                if estimate_tokens(&candidate) <= budget {
                    current = candidate;
                } else {
                    if !current.is_empty() {
                        pieces.push(std::mem::take(&mut current));
                    }
                    let mut chunk = String::new();
                    for c in word.chars() {
                        chunk.push(c);
                        if estimate_tokens(&chunk) >= budget {
                            pieces.push(std::mem::take(&mut chunk));
                        }
                    }
                    current = chunk;
                }
            }
        }
    }
    if !current.is_empty() {
        pieces.push(current);
    }
    pieces
}

/// Sentences, each with its closing punctuation.
pub fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let ends = matches!(c, '.' | '!' | '?' | '。' | '！' | '？' | '\n');
        let next_is_space = chars.peek().is_none_or(|(_, n)| n.is_whitespace());
        if ends && next_is_space {
            let end = i + c.len_utf8();
            let s = text[start..end].trim();
            if !s.is_empty() {
                out.push(s);
            }
            start = end;
        }
    }
    let rest = text[start..].trim();
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

/// How many words in a row a summary may share with the book before it
/// counts as copying. Names and stock phrases ("at the end of the day")
/// stay well under it.
pub const COPY_RUN_WORDS: usize = 7;

fn plain_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase().replace('\u{2019}', "'"))
        .collect()
}

/// The phrases of `summary` that appear word for word in the book's text
/// (runs of at least [`COPY_RUN_WORDS`] words, case and punctuation
/// ignored), longest first, at most three.
pub fn copied_phrases(summary: &str, paragraphs: &[String]) -> Vec<String> {
    let n = COPY_RUN_WORDS;
    let mut seen = std::collections::HashSet::new();
    for paragraph in paragraphs {
        let words = plain_words(paragraph);
        for window in words.windows(n) {
            seen.insert(window.join(" "));
        }
    }
    let words = plain_words(summary);
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i + n <= words.len() {
        if seen.contains(&words[i..i + n].join(" ")) {
            let mut end = i + n;
            while end < words.len() && seen.contains(&words[end + 1 - n..=end].join(" ")) {
                end += 1;
            }
            runs.push((i, end));
            i = end;
        } else {
            i += 1;
        }
    }
    runs.sort_by_key(|(a, b)| std::cmp::Reverse(b - a));
    runs.into_iter()
        .take(3)
        .map(|(a, b)| words[a..b].join(" "))
        .collect()
}

/// Halves a part at a paragraph or sentence boundary near its middle.
pub fn halve(part: &str) -> Option<(String, String)> {
    let paragraphs: Vec<String> = part.split("\n\n").map(str::to_string).collect();
    let pieces = if paragraphs.len() >= 2 {
        paragraphs
    } else {
        sentences(part).into_iter().map(str::to_string).collect()
    };
    if pieces.len() < 2 {
        return None;
    }
    let total: usize = pieces.iter().map(|p| estimate_tokens(p)).sum();
    let mut size = 0;
    let mut cut = 1;
    for (i, piece) in pieces.iter().enumerate() {
        size += estimate_tokens(piece);
        if size * 2 >= total {
            cut = (i + 1).clamp(1, pieces.len() - 1);
            break;
        }
    }
    let join = |p: &[String]| p.join("\n\n");
    Some((join(&pieces[..cut]), join(&pieces[cut..])))
}

/// The model's answer without the things small models add: a "Here is a
/// summary:" line, Markdown emphasis, surrounding quotes, extra blank
/// lines.
pub fn clean(text: &str) -> String {
    let mut lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    if let Some(first) = lines.first() {
        let lower = first.trim().to_lowercase();
        // "Certainly! Here is a summary of the requested text:" and the
        // like: a short opening line that only introduces the answer.
        let preamble = lower.ends_with(':') && lower.chars().count() <= 100;
        if preamble || lower.starts_with('#') {
            lines.remove(0);
        }
    }
    // "Certainly! Anna leaves." keeps "Anna leaves."
    if let Some(first) = lines.first_mut() {
        let lower = first.trim_start().to_lowercase();
        for filler in [
            "certainly!",
            "sure!",
            "of course!",
            "certainly.",
            "sure.",
            "of course.",
        ] {
            if lower.starts_with(filler) {
                *first = first.trim_start()[filler.len()..].trim_start();
                break;
            }
        }
    }
    let mut out = String::new();
    let mut blank = false;
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            blank = true;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank { "\n\n" } else { "\n" });
        }
        blank = false;
        out.push_str(line);
    }
    let mut out = out.replace("**", "").replace("__", "");
    for prefix in ["Summary:", "Notes:", "summary:"] {
        if let Some(rest) = out.strip_prefix(prefix) {
            out = rest.trim_start().to_string();
        }
    }
    let quoted = out.len() >= 2
        && ((out.starts_with('"') && out.ends_with('"'))
            || (out.starts_with('\u{201C}') && out.ends_with('\u{201D}')));
    if quoted && out.matches(['"', '\u{201C}', '\u{201D}']).count() == 2 {
        let mut chars = out.chars();
        chars.next();
        chars.next_back();
        out = chars.as_str().trim().to_string();
    }
    out
}

/// Openings with which a small model declines in words instead of raising
/// its refusal error ("I'm sorry, but I cannot fulfill that request.").
const DECLINES: &[&str] = &[
    "i'm sorry",
    "i am sorry",
    "sorry, i",
    "sorry, but",
    "i cannot",
    "i can't",
    "i can not",
    "i'm unable",
    "i am unable",
    "i'm not able",
    "i am not able",
    "unfortunately, i",
    "as an ai",
    "i apologize",
    "i apologise",
];

/// Whether a paragraph is the model declining or talking about itself
/// rather than answering.
pub fn declines(paragraph: &str) -> bool {
    let lower = paragraph
        .trim()
        .trim_start_matches(['"', '\u{201C}', '-', '*', ' '])
        .to_lowercase()
        .replace('\u{2019}', "'");
    DECLINES.iter().any(|d| lower.starts_with(d))
}

/// The answer without paragraphs in which the model declines. Empty when
/// it declined altogether.
pub fn without_declines(text: &str) -> String {
    text.split("\n\n")
        .filter(|p| !declines(p))
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim()
        .to_string()
}

/// How much of a paragraph repeats the prompt word for word (the share of
/// its words inside runs of [`COPY_RUN_WORDS`] found in the prompt).
fn echoed_share(paragraph: &str, prompt: &std::collections::HashSet<String>) -> f64 {
    let n = COPY_RUN_WORDS;
    let words = plain_words(paragraph);
    if words.len() < n {
        return 0.0;
    }
    let mut covered = vec![false; words.len()];
    for i in 0..=words.len() - n {
        if prompt.contains(&words[i..i + n].join(" ")) {
            covered[i..i + n].iter_mut().for_each(|c| *c = true);
        }
    }
    covered.iter().filter(|c| **c).count() as f64 / words.len() as f64
}

/// The answer without paragraphs that mostly repeat the prompt: a small
/// model given a long prompt sometimes copies its input or its
/// instructions back before (or instead of) answering. Empty when every
/// paragraph is an echo.
pub fn without_echoes(text: &str, prompt: &str) -> String {
    let n = COPY_RUN_WORDS;
    let words = plain_words(prompt);
    let seen: std::collections::HashSet<String> = words.windows(n).map(|w| w.join(" ")).collect();
    text.split("\n\n")
        .filter(|p| echoed_share(p, &seen) < 0.6)
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim()
        .to_string()
}

/// Prose cut off by the length cap ends mid-sentence ("The names are
/// sealed,"): keep it up to its last full sentence, dropping a last
/// paragraph that has none.
pub fn whole_sentences(text: &str) -> String {
    let text = text.trim();
    let ends = |s: &str| {
        s.trim_end_matches(['"', '\u{201D}', '\'', '\u{2019}', ')'])
            .ends_with(['.', '!', '?', '\u{3002}', '\u{FF01}', '\u{FF1F}'])
    };
    if text.is_empty() || ends(text) {
        return text.to_string();
    }
    let (before, last) = match text.rfind("\n\n") {
        Some(i) => (&text[..i], &text[i + 2..]),
        None => ("", text),
    };
    let kept: Vec<&str> = sentences(last).into_iter().filter(|s| ends(s)).collect();
    let last = kept.join(" ");
    [before.trim(), last.trim()]
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}
