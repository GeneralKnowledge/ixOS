//! Syntax colours for Python Lab's editor (docs/python-lab-workspace-plan.md,
//! PL1c): a small Python tokenizer that finds keywords, strings, comments,
//! numbers, `def` and `class` names and decorators, and returns their
//! ranges for Swift to colour. Everything else is left plain.
//!
//! Ranges are in UTF-16 code units, which is what `NSAttributedString`
//! counts in, so Swift can use them without converting. The tokenizer never
//! fails: broken code (an unclosed string, a stray quote) still gets
//! colours up to where it stops making sense, because it is what the
//! person is in the middle of typing.
//!
//! It agrees with Python's own `tokenize` on strings, comments and numbers
//! for the samples (the tests check that when `python3` is installed).
//! Soft keywords (`match`, `case`, `type`, `_`) stay plain: telling them
//! from names needs a parser, and colouring every variable called `match`
//! would be worse than colouring none.

/// The largest source coloured; bigger files are shown plain (the editor
/// refuses files over 1 MB anyway).
pub const HIGHLIGHT_MAX_BYTES: usize = 1024 * 1024;

const KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum HighlightKind {
    Keyword,
    String,
    Comment,
    Number,
    /// The name after `def` or `class`.
    Definition,
    /// `@` and the dotted name after it, at the start of a line.
    Decorator,
}

/// One coloured range, in UTF-16 code units from the start of the source.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HighlightSpan {
    pub start: u32,
    pub length: u32,
    pub kind: HighlightKind,
}

/// The coloured ranges in `source`, in order and never overlapping.
#[uniffi::export]
pub fn python_highlight(source: String) -> Vec<HighlightSpan> {
    if source.len() > HIGHLIGHT_MAX_BYTES {
        return Vec::new();
    }
    let chars: Vec<char> = source.chars().collect();
    // UTF-16 offset of each char index (and one past the end).
    let mut utf16 = Vec::with_capacity(chars.len() + 1);
    let mut at = 0u32;
    for c in &chars {
        utf16.push(at);
        at += c.len_utf16() as u32;
    }
    utf16.push(at);
    tokens(&chars)
        .into_iter()
        .map(|t| HighlightSpan {
            start: utf16[t.start],
            length: utf16[t.end] - utf16[t.start],
            kind: t.kind,
        })
        .collect()
}

/// A coloured range in char indexes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Token {
    pub start: usize,
    pub end: usize,
    pub kind: HighlightKind,
}

pub(crate) fn tokens(s: &[char]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0;
    // Only spaces and tabs since the last newline: an `@` here is a decorator.
    let mut line_start = true;
    // The last word was `def` or `class`, so the next name is a definition.
    let mut naming = false;
    while i < s.len() {
        let c = s[i];
        if c == '\n' || c == '\r' {
            line_start = true;
            naming = false;
            i += 1;
            continue;
        }
        if c == ' ' || c == '\t' || c == '\x0c' {
            i += 1;
            continue;
        }
        if c == '\\' && matches!(s.get(i + 1), Some('\n' | '\r')) {
            // Explicit line joining: the next line continues this one.
            i += 2;
            continue;
        }
        let at_line_start = line_start;
        line_start = false;
        if c == '#' {
            let end = line_end(s, i);
            out.push(Token {
                start: i,
                end,
                kind: HighlightKind::Comment,
            });
            i = end;
            continue;
        }
        if let Some((quote_at, formatted)) = string_prefix(s, i) {
            let end = string_end(s, quote_at, formatted, 0);
            out.push(Token {
                start: i,
                end,
                kind: HighlightKind::String,
            });
            naming = false;
            i = end;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && s.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let end = number_end(s, i);
            out.push(Token {
                start: i,
                end,
                kind: HighlightKind::Number,
            });
            naming = false;
            i = end;
            continue;
        }
        if is_name_start(c) {
            let end = name_end(s, i);
            let word: String = s[i..end].iter().collect();
            if naming {
                out.push(Token {
                    start: i,
                    end,
                    kind: HighlightKind::Definition,
                });
                naming = false;
            } else if KEYWORDS.contains(&word.as_str()) {
                out.push(Token {
                    start: i,
                    end,
                    kind: HighlightKind::Keyword,
                });
                naming = word == "def" || word == "class";
            }
            i = end;
            continue;
        }
        if c == '@' && at_line_start {
            // `@name.attr`, without any call arguments after it.
            let mut end = i + 1;
            while end < s.len() && (s[end] == ' ' || s[end] == '\t') {
                end += 1;
            }
            while end < s.len() && is_name_start(s[end]) {
                end = name_end(s, end);
                if s.get(end) == Some(&'.') && s.get(end + 1).is_some_and(|&c| is_name_start(c)) {
                    end += 1;
                } else {
                    break;
                }
            }
            out.push(Token {
                start: i,
                end,
                kind: HighlightKind::Decorator,
            });
            i = end;
            continue;
        }
        naming = false;
        i += 1;
    }
    out
}

fn line_end(s: &[char], mut i: usize) -> usize {
    while i < s.len() && s[i] != '\n' && s[i] != '\r' {
        i += 1;
    }
    i
}

fn is_name_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

fn name_end(s: &[char], mut i: usize) -> usize {
    while i < s.len() && (s[i] == '_' || s[i].is_alphanumeric()) {
        i += 1;
    }
    i
}

/// If a string starts at `i` (an optional prefix such as `rb` or `f`, then a
/// quote), where its quote is and whether it is formatted.
fn string_prefix(s: &[char], i: usize) -> Option<(usize, bool)> {
    let mut j = i;
    let (mut formatted, mut raw, mut bytes, mut unicode) = (false, false, false, false);
    while j < s.len() && j - i < 3 {
        match s[j].to_ascii_lowercase() {
            '\'' | '"' => {
                // `u` goes with nothing; `b` not with `f` or `t`.
                let valid = !(unicode && (raw || bytes || formatted)) && !(bytes && formatted);
                return valid.then_some((j, formatted));
            }
            'r' if !raw => raw = true,
            'b' if !bytes => bytes = true,
            'u' if !unicode => unicode = true,
            'f' | 't' if !formatted => formatted = true,
            _ => return None,
        }
        j += 1;
    }
    None
}

/// One past the end of the string whose opening quote is at `q`. An
/// unclosed single-quoted string ends at the end of its line; an unclosed
/// triple-quoted one at the end of the source.
/// Strings nested inside f-strings deeper than this are not followed (the
/// rest of the outer string is still coloured), so a pathological file
/// can't recurse deep enough to overflow the stack.
const NESTING_MAX: usize = 16;

fn string_end(s: &[char], q: usize, formatted: bool, depth: usize) -> usize {
    let quote = s[q];
    let triple = s.get(q + 1) == Some(&quote) && s.get(q + 2) == Some(&quote);
    let mut i = if triple { q + 3 } else { q + 1 };
    while i < s.len() {
        let c = s[i];
        if c == '\\' {
            // In a raw string the backslash still keeps the quote after it
            // from closing the string; it just stays in the text.
            i += 2;
            continue;
        }
        if !triple && (c == '\n' || c == '\r') {
            return i;
        }
        if c == quote {
            if !triple {
                return i + 1;
            }
            if s.get(i + 1) == Some(&quote) && s.get(i + 2) == Some(&quote) {
                return i + 3;
            }
        }
        if formatted && c == '{' {
            if s.get(i + 1) == Some(&'{') {
                i += 2;
                continue;
            }
            i = replacement_end(s, i + 1, triple, depth);
            continue;
        }
        i += 1;
    }
    s.len()
}

/// One past the `}` that closes a replacement field starting at `i`,
/// stepping over brackets and strings inside it (since Python 3.12 those
/// may reuse the outer quote: `f"{d["key"]}"`).
fn replacement_end(s: &[char], mut i: usize, triple: bool, depth: usize) -> usize {
    let mut brackets = 0usize;
    while i < s.len() {
        let c = s[i];
        if !triple && (c == '\n' || c == '\r') {
            return i;
        }
        if depth < NESTING_MAX {
            if let Some((quote_at, formatted)) = string_prefix(s, i) {
                i = string_end(s, quote_at, formatted, depth + 1);
                continue;
            }
        }
        match c {
            '(' | '[' | '{' => brackets += 1,
            ')' | ']' => brackets = brackets.saturating_sub(1),
            '}' if brackets == 0 => return i + 1,
            '}' => brackets -= 1,
            _ => {}
        }
        if is_name_start(c) {
            i = name_end(s, i);
            continue;
        }
        i += 1;
    }
    i
}

/// One past the end of the number starting at `i`: decimal, hex, octal or
/// binary, with underscores, a fraction, an exponent and `j` for imaginary.
fn number_end(s: &[char], mut i: usize) -> usize {
    let radix = s[i] == '0' && matches!(s.get(i + 1), Some('x' | 'X' | 'o' | 'O' | 'b' | 'B'));
    if radix {
        i += 2;
        while i < s.len() && (s[i].is_ascii_hexdigit() || s[i] == '_') {
            i += 1;
        }
        return i;
    }
    let digits = |s: &[char], mut i: usize| {
        while i < s.len() && (s[i].is_ascii_digit() || s[i] == '_') {
            i += 1;
        }
        i
    };
    i = digits(s, i);
    if s.get(i) == Some(&'.') {
        i = digits(s, i + 1);
    }
    if matches!(s.get(i), Some('e' | 'E')) {
        let mut j = i + 1;
        if matches!(s.get(j), Some('+' | '-')) {
            j += 1;
        }
        if s.get(j).is_some_and(char::is_ascii_digit) {
            i = digits(s, j);
        }
    }
    if matches!(s.get(i), Some('j' | 'J')) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests;
