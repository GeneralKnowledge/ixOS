//! Bonsai's answer while it streams in, read field by field so the screen
//! can show the thought, then the file being written, as they arrive.
//!
//! The grammar fixes the shape (`{"thought": "…", "tool": "…", "path": "…",
//! "content": "…"}`, arguments in the tool's order), so a forgiving scan of
//! the partial text is enough: each string is decoded up to its closing
//! quote, or up to the end of the text while it is still being written.

/// The answer so far, for the live view.
#[derive(uniffi::Record, Clone, Debug, Default, PartialEq, Eq)]
pub struct HarnessLive {
    /// The reasoning line, as far as it has been written.
    pub thought: String,
    /// True once the thought's closing quote has arrived.
    pub thought_done: bool,
    /// The tool, once its name is complete.
    pub tool: Option<String>,
    /// The file the step is about (`path`), once known.
    pub path: Option<String>,
    /// The text being written into the file: `content` for write_file,
    /// `new` for edit_file. Empty for other tools.
    pub code: String,
    /// The lines being replaced (`old`, edit_file only).
    pub replacing: String,
    /// True while `code` is still being written.
    pub writing: bool,
}

/// Reads the partial answer `text`.
pub fn read(text: &str) -> HarnessLive {
    let mut live = HarnessLive::default();
    if let Some((thought, done)) = field(text, "thought") {
        live.thought = thought;
        live.thought_done = done;
    }
    live.tool = field(text, "tool").and_then(|(name, done)| done.then_some(name));
    live.path = field(text, "path").and_then(|(path, done)| done.then_some(path));
    if let Some((old, _)) = field(text, "old") {
        live.replacing = old;
    }
    for key in ["content", "new"] {
        if let Some((code, done)) = field(text, key) {
            live.code = code;
            live.writing = !done;
        }
    }
    live
}

/// The string value of `"key": "…` in `text`, decoded, and whether its
/// closing quote has arrived. Keys appear once each, in the grammar's order;
/// a key inside a value is escaped (`\"`), so the raw `"key": "` marker can
/// only be the real one.
fn field(text: &str, key: &str) -> Option<(String, bool)> {
    let marker = format!("\"{key}\": \"");
    let start = find_marker(text, &marker)? + marker.len();
    let mut out = String::new();
    let mut chars = text[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some((out, true)),
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('b') => out.push('\u{8}'),
                Some('f') => out.push('\u{c}'),
                Some('u') => {
                    let Some(unit) = hex4(&mut chars) else { break };
                    if (0xD800..0xDC00).contains(&unit) {
                        // A high surrogate: wait for its low half.
                        let mut ahead = chars.clone();
                        match (ahead.next(), ahead.next()) {
                            (None, _) | (Some('\\'), None) => break,
                            (Some('\\'), Some('u')) => {
                                // cut: only counts whether four characters of the escape have arrived; nothing is cut from the text.
                                if ahead.as_str().chars().take(4).count() < 4 {
                                    break;
                                }
                                match hex4(&mut ahead) {
                                    Some(low) if (0xDC00..0xE000).contains(&low) => {
                                        let code =
                                            0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                                        out.extend(char::from_u32(code));
                                        chars = ahead;
                                    }
                                    _ => out.push(char::REPLACEMENT_CHARACTER),
                                }
                            }
                            // Something else follows: a lone high surrogate.
                            _ => out.push(char::REPLACEMENT_CHARACTER),
                        }
                    } else {
                        out.push(char::from_u32(unit).unwrap_or(char::REPLACEMENT_CHARACTER));
                    }
                }
                Some(other) => out.push(other),
                None => break,
            },
            other => out.push(other),
        }
    }
    Some((out, false))
}

/// Four hex digits after `\u`, or None when the text ends first.
fn hex4(chars: &mut std::str::Chars<'_>) -> Option<u32> {
    let hex: String = chars.by_ref().take(4).collect();
    if hex.len() < 4 {
        return None;
    }
    u32::from_str_radix(&hex, 16).ok()
}

/// The first `marker` that isn't inside a string value: scans the text
/// tracking quotes and escapes.
fn find_marker(text: &str, marker: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut inside = false;
    let mut i = 0;
    while i < bytes.len() {
        if !inside && bytes[i..].starts_with(marker.as_bytes()) {
            return Some(i);
        }
        match bytes[i] {
            b'\\' if inside => i += 1,
            b'"' => inside = !inside,
            _ => {}
        }
        i += 1;
    }
    None
}
