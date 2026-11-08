//! `forge ui focus`: which screens and controls a change touches, worked
//! out from the source (docs/forge-m6.md, M6.2). `forge ui explore --since`
//! uses it to aim the walk.
//!
//! Every link it makes points at a line of reasoning a person can check, and
//! it says when it has none:
//!
//! - **direct**: a changed Swift file declares the identifier
//!   (`.accessibilityIdentifier("record.save")`). An identifier built with
//!   interpolation (`"record.result.\(key)"`) is kept as its prefix.
//! - **entry**: a Swift file that uses a type the change declares (a view
//!   that presents `ExerciseView`), or that calls into changed Rust through
//!   the UniFFI bindings, possibly through a model method; its identifiers
//!   are the ways into the changed code.
//! - A changed file with no such link is listed under `unmapped` with the
//!   reason: a file name alone never ties code to a screen.
//!
//! The matching is textual (names as whole words, Swift and Rust function
//! bodies found by their braces), not a compiler's: it can link too much (a
//! common method name) and never sees identifiers built at run time. It
//! guides where the walk goes first; it proves nothing about the app.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use forge_uitest::focus::{Focus, Link, Strength};
use serde_json::{json, Value};

/// A source file of the app.
struct Source {
    /// Repository-relative (or app-relative outside git).
    path: String,
    text: String,
}

impl Source {
    fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

/// What a change touches.
#[derive(Debug, Default)]
pub struct Impact {
    pub since: Option<String>,
    pub changed: Vec<String>,
    pub links: Vec<Link>,
    /// Changed files with no link, and why.
    pub unmapped: Vec<(String, String)>,
    /// UI test files that use a linked identifier.
    pub tests: Vec<(String, Vec<String>)>,
    /// Hints from the changed code: persistence, navigation.
    pub signals: BTreeMap<String, Vec<String>>,
}

impl Impact {
    pub fn focus(&self) -> Focus {
        Focus {
            links: self.links.clone(),
            routes: Vec::new(),
        }
    }

    pub fn to_json(&self) -> Value {
        let by = |s: Strength| -> Vec<String> {
            let set: BTreeSet<String> = self
                .links
                .iter()
                .filter(|l| l.strength == s)
                .map(|l| {
                    if l.prefix {
                        format!("{}*", l.id)
                    } else {
                        l.id.clone()
                    }
                })
                .collect();
            set.into_iter().collect()
        };
        let direct = by(Strength::Direct);
        let screens: BTreeSet<&str> = self
            .links
            .iter()
            .filter(|l| l.strength == Strength::Direct)
            .map(|l| forge_uitest::focus::screen_part(&l.id))
            .filter(|s| !s.is_empty())
            .collect();
        let links: Vec<Value> = self
            .links
            .iter()
            .map(|l| {
                let mut v = serde_json::to_value(l).unwrap_or(Value::Null);
                v["basis"] = json!(l.strength.basis());
                v
            })
            .collect();
        json!({
            // Read first: what every link below rests on.
            "method": "textual",
            "proves": "nothing about the running app: these are leads for where to look first",
            "basis_note": forge_uitest::focus::BASIS_NOTE,
            "since": self.since,
            "changed": self.changed,
            "direct": direct,
            // Ways in that the change doesn't itself declare.
            "entry": by(Strength::Entry).into_iter().filter(|e| !direct.contains(e)).collect::<Vec<_>>(),
            "screens": screens,
            "screens_note": "Screen names come from the identifiers' `screen.element` naming rule (AGENTS.md), not from the running app.",
            "links": links,
            "unmapped": self.unmapped.iter().map(|(f, why)| json!({"file": f, "why": why})).collect::<Vec<_>>(),
            "tests": self.tests.iter().map(|(f, ids)| json!({"file": f, "uses": ids})).collect::<Vec<_>>(),
            "signals": self.signals,
            "limits": [
                "Textual matching: whole-word names and brace-matched bodies, not the compiler's view. It can over-link (a common method name) and misses identifiers built at run time.",
                "Persistence across a relaunch can't be explored on the phone yet: test-data mode starts fresh on every launch (docs/forge-m6.md, M6.2).",
            ],
        })
    }
}

/// The files changed since `since` (a commit; `HEAD` means uncommitted
/// work), repository-relative, plus untracked ones. The repository's top
/// folder and the app's folder within it come back too.
pub fn changed_since(app: &Path, since: &str) -> Result<(PathBuf, String, Vec<String>), String> {
    let git = |args: &[&str]| -> Result<Vec<String>, String> {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(app)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("git: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect())
    };
    let top = git(&["rev-parse", "--show-toplevel"]).map_err(|e| {
        format!(
            "{} is not in a git checkout ({e}); pass --changed FILE instead",
            app.display()
        )
    })?;
    let top = PathBuf::from(top.into_iter().next().unwrap_or_default());
    let prefix = git(&["rev-parse", "--show-prefix"])?
        .into_iter()
        .next()
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_string();
    let mut files = git(&["diff", "--name-only", since, "--", ":/"])
        .map_err(|e| format!("git diff {since}: {e}"))?;
    files.extend(git(&[
        "ls-files",
        "--others",
        "--exclude-standard",
        "--full-name",
        "--",
        ":/",
    ])?);
    files.sort();
    files.dedup();
    Ok((top, prefix, files))
}

/// Links the `changed` files (repository-relative) to identifiers of the
/// app at `repo/app_rel`.
pub fn impact(repo: &Path, app_rel: &str, changed: &[String]) -> Impact {
    let app_dir = repo.join(app_rel);
    let rel = |p: &Path| {
        p.strip_prefix(repo)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| p.display().to_string())
    };
    let swift: Vec<Source> = files_under(&app_dir.join("Sources"), "swift")
        .into_iter()
        .filter_map(|p| {
            Some(Source {
                path: rel(&p),
                text: std::fs::read_to_string(&p).ok()?,
            })
        })
        .collect();
    let rust: Vec<Source> = files_under(&app_dir.join("rust/src"), "rs")
        .into_iter()
        .filter_map(|p| {
            Some(Source {
                path: rel(&p),
                text: std::fs::read_to_string(&p).ok()?,
            })
        })
        .collect();
    let mut impact = Impact {
        changed: changed.to_vec(),
        ..Impact::default()
    };
    let app_prefix = if app_rel.is_empty() {
        String::new()
    } else {
        format!("{app_rel}/")
    };
    for file in changed {
        let before = impact.links.len();
        let in_app = file.starts_with(&app_prefix);
        let text = std::fs::read_to_string(repo.join(file)).unwrap_or_default();
        signals(&text, file, &mut impact.signals);
        let why = if !in_app {
            Some("outside the app: shared or unrelated code; no link to this app's screens is established".to_string())
        } else if file.contains("/Generated/") {
            Some("generated bindings: follow the Rust change instead".into())
        } else if file.ends_with(".swift") && file.contains("/Sources/") {
            swift_links(file, &text, &swift, &mut impact.links);
            None
        } else if file.ends_with(".rs") && file.contains("/rust/src/") {
            rust_links(file, &rust, &swift, &mut impact.links);
            None
        } else if file.ends_with(".swift") {
            Some("Swift outside Sources/ (tests or tooling): not part of the app's screens".into())
        } else {
            Some("not app source (configuration, docs, tests or data)".into())
        };
        if impact.links.len() == before {
            let why = why.unwrap_or_else(|| {
                if text.is_empty() {
                    "deleted or unreadable: nothing left to read identifiers from".into()
                } else {
                    "no identifier, used type or exported function connects it to a screen".into()
                }
            });
            impact.unmapped.push((file.clone(), why));
        }
    }
    // One link per (identifier, file), the strongest.
    impact
        .links
        .sort_by(|a, b| (&a.id, &a.file, b.strength).cmp(&(&b.id, &b.file, a.strength)));
    impact
        .links
        .dedup_by(|b, a| a.id == b.id && a.file == b.file && a.prefix == b.prefix);
    impact.tests = tests_using(&app_dir.join("uitests"), &impact.links, repo);
    impact
}

/// Links from a changed Swift file: its own identifiers (direct), and the
/// identifiers of the files that use the types it declares (entry).
fn swift_links(file: &str, text: &str, swift: &[Source], links: &mut Vec<Link>) {
    let name = file.rsplit('/').next().unwrap_or(file).to_string();
    for (id, prefix) in identifiers(text) {
        links.push(Link {
            id,
            prefix,
            strength: Strength::Direct,
            file: file.into(),
            how: format!("declared in {name}"),
        });
    }
    let types = declared_types(text);
    for other in swift.iter().filter(|s| s.path != file) {
        if let Some(t) = types.iter().find(|t| mentions(&other.text, t)) {
            for (id, prefix) in identifiers(&other.text) {
                links.push(Link {
                    id,
                    prefix,
                    strength: Strength::Entry,
                    file: file.into(),
                    how: format!("{} uses {t} (declared in {name})", other.name()),
                });
            }
        }
    }
    // A model file (no identifiers of its own): the views calling the
    // methods it declares.
    if identifiers(text).is_empty() {
        for func in swift_funcs(text) {
            for other in swift.iter().filter(|s| s.path != file) {
                if calls(&other.text, &func.name) {
                    for (id, prefix) in identifiers(&other.text) {
                        links.push(Link {
                            id,
                            prefix,
                            strength: Strength::Entry,
                            file: file.into(),
                            how: format!(
                                "{} calls {}() (declared in {name})",
                                other.name(),
                                func.name
                            ),
                        });
                    }
                }
            }
        }
    }
}

/// Links from a changed Rust file of the app crate: the UniFFI exports it
/// declares, or the exports in other files that call what it declares;
/// then the Swift that calls those exports, directly or through a model
/// method.
fn rust_links(file: &str, rust: &[Source], swift: &[Source], links: &mut Vec<Link>) {
    let name = file.rsplit('/').next().unwrap_or(file).to_string();
    let Some(this) = rust.iter().find(|r| r.path == file) else {
        return;
    };
    // Exported names (Swift spelling), whether they are functions (used
    // only when called), and how each is tied to the change.
    let mut exports: Vec<(String, bool, String)> = Vec::new();
    for item in rust_exports(&this.text) {
        exports.push((
            item.swift_name(),
            !item.is_type,
            format!("{} is exported from {name}", item.name),
        ));
    }
    if exports.is_empty() {
        let declared = rust_declared(&this.text);
        for other in rust.iter().filter(|r| r.path != file) {
            for item in rust_exports(&other.text) {
                if let Some(d) = declared.iter().find(|d| mentions(&item.body, d)) {
                    exports.push((
                        item.swift_name(),
                        !item.is_type,
                        format!("{} ({}) uses {d} from {name}", item.name, other.name()),
                    ));
                }
            }
        }
    }
    // A function counts where it is called (`core.today(`), not where a
    // property of the same name is read (`model.today`).
    let used = |text: &str, export: &str, func: bool| {
        if func {
            calls(text, export)
        } else {
            mentions(text, export)
        }
    };
    for (export, func, chain) in &exports {
        for view in swift {
            let own = identifiers(&view.text);
            if !own.is_empty() {
                if used(&view.text, export, *func) {
                    for (id, prefix) in own {
                        links.push(Link {
                            id,
                            prefix,
                            strength: Strength::Entry,
                            file: file.into(),
                            how: format!("{} uses {export}; {chain}", view.name()),
                        });
                    }
                }
                continue;
            }
            // A model: its methods that call the export, and the views
            // calling those.
            for method in swift_funcs(&view.text) {
                if !used(&method.body, export, *func) {
                    continue;
                }
                let func = method;
                for caller in swift.iter().filter(|s| s.path != view.path) {
                    if calls(&caller.text, &func.name) {
                        for (id, prefix) in identifiers(&caller.text) {
                            links.push(Link {
                                id,
                                prefix,
                                strength: Strength::Entry,
                                file: file.into(),
                                how: format!(
                                    "{} calls {}() ({}), which calls {export}; {chain}",
                                    caller.name(),
                                    func.name,
                                    view.name()
                                ),
                            });
                        }
                    }
                }
            }
        }
    }
}

/// `.accessibilityIdentifier("…")` literals: the identifier, or for an
/// interpolated one its literal prefix (kept only when it ends at a dot,
/// so a prefix never covers a whole screen by accident).
fn identifiers(text: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let needle = ".accessibilityIdentifier(\"";
    let mut rest = text;
    while let Some(at) = rest.find(needle) {
        rest = &rest[at + needle.len()..];
        let Some(end) = string_end(rest) else { break };
        let literal = &rest[..end];
        match literal.find("\\(") {
            None if !literal.is_empty() => out.push((literal.to_string(), false)),
            Some(cut) => {
                let prefix = &literal[..cut];
                if prefix.ends_with('.') && prefix.len() > 1 {
                    out.push((prefix.to_string(), true));
                }
            }
            _ => {}
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Where a Swift string literal starting at `s` ends (its closing quote),
/// stepping over escapes and `\( … )` interpolations.
fn string_end(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut depth = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if depth == 0 && bytes.get(i + 1) == Some(&b'(') => {
                depth = 1;
                i += 2;
                continue;
            }
            b'\\' if depth == 0 => {
                i += 2;
                continue;
            }
            b'(' if depth > 0 => depth += 1,
            b')' if depth > 0 => depth -= 1,
            b'"' if depth == 0 => return Some(i),
            b'\n' => return None,
            _ => {}
        }
        i += 1;
    }
    None
}

/// Types a Swift file declares or extends.
fn declared_types(text: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let words: Vec<&str> = line
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter(|w| !w.is_empty())
            .collect();
        for pair in words.windows(2) {
            if matches!(pair[0], "struct" | "class" | "enum" | "actor" | "protocol")
                && pair[1].chars().next().is_some_and(char::is_uppercase)
            {
                out.insert(pair[1].to_string());
            }
        }
    }
    out.into_iter().collect()
}

struct Func {
    name: String,
    body: String,
}

/// Swift `func`s with their bodies. Names shorter than four letters and
/// SwiftUI's own (`body`) are left out: too common to tie anything.
fn swift_funcs(text: &str) -> Vec<Func> {
    let mut out = Vec::new();
    let mut rest = text;
    let mut offset = 0;
    while let Some(at) = find_word(rest, "func") {
        let after = &rest[at + 4..];
        let name: String = after
            .trim_start()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        let start = offset + at + 4;
        let body = text[start..]
            .find('{')
            .and_then(|open| braced(&text[start + open..]).map(|b| b.to_string()))
            .unwrap_or_default();
        if name.len() >= 4 && name != "body" {
            out.push(Func { name, body });
        }
        offset = start;
        rest = &text[start..];
    }
    out
}

/// One Rust item of the app crate.
struct RustItem {
    name: String,
    body: String,
    /// A type rather than a function.
    is_type: bool,
}

impl RustItem {
    /// What UniFFI calls it in Swift: `record_session` → `recordSession`.
    fn swift_name(&self) -> String {
        if self.is_type {
            return self.name.clone();
        }
        let mut out = String::new();
        let mut upper = false;
        for c in self.name.chars() {
            if c == '_' {
                upper = true;
            } else if upper {
                out.extend(c.to_uppercase());
                upper = false;
            } else {
                out.push(c);
            }
        }
        out
    }
}

/// UniFFI exports in a Rust file: `#[uniffi::export]` functions, the `pub
/// fn`s of `#[uniffi::export] impl` blocks, and `uniffi::Record`, `Enum`,
/// `Object` and `Error` types.
fn rust_exports(text: &str) -> Vec<RustItem> {
    let mut out = Vec::new();
    let mut search = 0;
    while let Some(at) = text[search..].find("#[uniffi::export") {
        let start = search + at;
        let after = &text[start..];
        let item_start = after.find(']').map(|e| e + 1).unwrap_or(after.len());
        let item = after[item_start..].trim_start();
        if item.starts_with("impl") {
            if let Some(open) = item.find('{') {
                if let Some(block) = braced(&item[open..]) {
                    out.extend(rust_fns(block, true));
                }
            }
        } else if let Some(f) = rust_fns(item, false).into_iter().next() {
            out.push(f);
        }
        search = start + 1;
    }
    for derive in [
        "uniffi::Record",
        "uniffi::Enum",
        "uniffi::Object",
        "uniffi::Error",
    ] {
        let mut search = 0;
        while let Some(at) = text[search..].find(derive) {
            let start = search + at;
            if let Some(name) = text[start..]
                .split_whitespace()
                .skip_while(|w| !matches!(*w, "struct" | "enum"))
                .nth(1)
            {
                let name: String = name
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    out.push(RustItem {
                        name,
                        body: String::new(),
                        is_type: true,
                    });
                }
            }
            search = start + derive.len();
        }
    }
    out
}

/// `fn`s in `text` (only `pub` ones when `public`), with their bodies.
fn rust_fns(text: &str, public: bool) -> Vec<RustItem> {
    let mut out = Vec::new();
    let mut search = 0;
    while let Some(at) = find_word(&text[search..], "fn") {
        let start = search + at;
        let line_start = text[..start].rfind('\n').map(|n| n + 1).unwrap_or(0);
        let head = &text[line_start..start];
        let name: String = text[start + 2..]
            .trim_start()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        let body = text[start..]
            .find('{')
            .and_then(|o| braced(&text[start + o..]))
            .unwrap_or("")
            .to_string();
        if !name.is_empty() && (!public || head.contains("pub")) {
            out.push(RustItem {
                name,
                body: body.clone(),
                is_type: false,
            });
        }
        search = start + 2 + body.len().max(1);
        if search >= text.len() {
            break;
        }
    }
    out
}

/// Functions and types a Rust file declares (any visibility): what other
/// files of the crate may use from it.
fn rust_declared(text: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let words: Vec<&str> = line
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter(|w| !w.is_empty())
            .collect();
        for pair in words.windows(2) {
            if matches!(
                pair[0],
                "fn" | "struct" | "enum" | "trait" | "const" | "type"
            ) && pair[1].len() >= 4
                && pair[1] != "main"
            {
                out.insert(pair[1].to_string());
            }
        }
    }
    out.into_iter().collect()
}

/// The text between the `{` at the start of `s` and its matching `}`.
fn braced(s: &str) -> Option<&str> {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(&s[1..i]);
                }
            }
            _ => {}
        }
    }
    None
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where `word` appears as a whole word.
fn find_word(text: &str, word: &str) -> Option<usize> {
    let mut search = 0;
    while let Some(at) = text[search..].find(word) {
        let start = search + at;
        let end = start + word.len();
        let before = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !is_word(c));
        let after = text[end..].chars().next().is_none_or(|c| !is_word(c));
        if before && after {
            return Some(start);
        }
        search = end;
    }
    None
}

fn mentions(text: &str, word: &str) -> bool {
    find_word(text, word).is_some()
}

/// `name(` or `.name(` as a call, not its declaration.
fn calls(text: &str, name: &str) -> bool {
    let mut search = 0;
    while let Some(at) = find_word(&text[search..], name) {
        let start = search + at;
        let end = start + name.len();
        let declared = text[..start].trim_end().ends_with("func");
        if !declared && text[end..].starts_with('(') {
            return true;
        }
        search = end;
    }
    false
}

/// Hints from a changed file's text, for the report.
fn signals(text: &str, file: &str, out: &mut BTreeMap<String, Vec<String>>) {
    const PERSISTENCE: &[&str] = &[
        "JsonFile",
        "forge_storage",
        "SqliteStore",
        "UserDefaults",
        "FileManager",
        "SwiftData",
        "persist",
        "STORE_VERSION",
    ];
    const NAVIGATION: &[&str] = &[
        "NavigationStack",
        "NavigationLink",
        "navigationDestination",
        ".sheet(",
        "fullScreenCover",
        "TabView",
        "dismiss()",
    ];
    for (kind, markers) in [("persistence", PERSISTENCE), ("navigation", NAVIGATION)] {
        let hits: Vec<&str> = markers
            .iter()
            .copied()
            .filter(|m| text.contains(m))
            .collect();
        if !hits.is_empty() {
            out.entry(kind.into())
                .or_default()
                .push(format!("{file}: {}", hits.join(", ")));
        }
    }
}

/// `uitests/*.toml` files that name a linked identifier.
fn tests_using(dir: &Path, links: &[Link], repo: &Path) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for file in files_under(dir, "toml") {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let mut used = BTreeSet::new();
        for link in links.iter().filter(|l| l.strength >= Strength::Entry) {
            let quoted = if link.prefix {
                format!("\"{}", link.id)
            } else {
                format!("\"{}\"", link.id)
            };
            if text.contains(&quoted) {
                used.insert(if link.prefix {
                    format!("{}*", link.id)
                } else {
                    link.id.clone()
                });
            }
        }
        if !used.is_empty() {
            let rel = file
                .strip_prefix(repo)
                .map(|r| r.display().to_string())
                .unwrap_or_else(|_| file.display().to_string());
            out.push((rel, used.into_iter().collect()));
        }
    }
    out
}

fn files_under(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == ext) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// What to change: `--changed` files (relative to the current folder, the
/// app or the repository) or git's changes since `--since`.
pub fn read(
    app: &Path,
    since: Option<&str>,
    changed: &[String],
) -> Result<(PathBuf, String, Impact), String> {
    let app = std::fs::canonicalize(app).map_err(|e| format!("{}: {e}", app.display()))?;
    let (repo, app_rel, files) = if changed.is_empty() {
        let since = since.unwrap_or("HEAD");
        changed_since(&app, since)?
    } else {
        let (repo, app_rel) = match changed_since(&app, "HEAD") {
            Ok((repo, rel, _)) => (repo, rel),
            // Outside git: paths are taken relative to the app's parent.
            Err(_) => (
                app.parent().unwrap_or(&app).to_path_buf(),
                app.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
        };
        let mut files = Vec::new();
        for c in changed {
            let candidates = [
                std::env::current_dir()
                    .map(|d| d.join(c))
                    .unwrap_or_default(),
                app.join(c),
                repo.join(c),
            ];
            let found = candidates
                .iter()
                .filter_map(|p| std::fs::canonicalize(p).ok())
                .find_map(|p| p.strip_prefix(&repo).ok().map(Path::to_path_buf))
                .ok_or_else(|| {
                    format!("--changed {c}: no such file in the app or the repository")
                })?;
            files.push(found.to_string_lossy().replace('\\', "/"));
        }
        (repo, app_rel, files)
    };
    let mut impact = impact(&repo, &app_rel, &files);
    impact.since = if changed.is_empty() {
        Some(since.unwrap_or("HEAD").to_string())
    } else {
        None
    };
    Ok((repo, app_rel, impact))
}

/// `forge ui focus`.
pub fn show(
    app: &Path,
    since: Option<&str>,
    changed: &[String],
    out: &mut dyn std::io::Write,
) -> Result<(), String> {
    let (_, _, impact) = read(app, since, changed)?;
    let value = impact.to_json();
    crate::report::set_data(value.clone());
    let err = |e: std::io::Error| e.to_string();
    if impact.changed.is_empty() {
        writeln!(
            out,
            "No changed files{}.",
            impact
                .since
                .as_deref()
                .map(|s| format!(" since {s}"))
                .unwrap_or_default()
        )
        .map_err(err)?;
        return Ok(());
    }
    writeln!(
        out,
        "Read from the source as text, not compiled: leads for where to look, not proven dependencies."
    )
    .map_err(err)?;
    writeln!(out, "Changed: {}", impact.changed.join(", ")).map_err(err)?;
    let direct: Vec<&str> = value["direct"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !direct.is_empty() {
        writeln!(
            out,
            "Declared in the changed files (their code changed; the screen they show on is not proven): {}",
            direct.join(", ")
        )
        .map_err(err)?;
    }
    // Inferred ways in, grouped by the reason, so each can be checked.
    let mut entry: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for link in impact
        .links
        .iter()
        .filter(|l| l.strength == Strength::Entry)
    {
        let id = if link.prefix {
            format!("{}*", link.id)
        } else {
            link.id.clone()
        };
        if !direct.contains(&id.as_str()) {
            entry.entry(link.how.as_str()).or_default().insert(id);
        }
    }
    if !entry.is_empty() {
        writeln!(
            out,
            "Inferred ways into it (names matched as words; not every control of such a view leads there):"
        )
        .map_err(err)?;
        for (how, ids) in &entry {
            let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
            writeln!(out, "  {}: {how}", ids.join(", ")).map_err(err)?;
        }
    }
    for (file, why) in &impact.unmapped {
        writeln!(out, "No link from {file}: {why}").map_err(err)?;
    }
    for (file, ids) in &impact.tests {
        writeln!(out, "UI test using them: {file} ({})", ids.join(", ")).map_err(err)?;
    }
    for (kind, hits) in &impact.signals {
        writeln!(out, "{kind} code changed: {}", hits.join("; ")).map_err(err)?;
    }
    writeln!(
        out,
        "Explore with: forge ui explore --project {} {}",
        app.display(),
        if changed.is_empty() {
            format!("--since {}", impact.since.as_deref().unwrap_or("HEAD"))
        } else {
            changed
                .iter()
                .map(|c| format!("--changed {c}"))
                .collect::<Vec<_>>()
                .join(" ")
        }
    )
    .map_err(err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn identifiers_and_interpolated_prefixes() {
        let text = r#"
            Button("Save") {}.accessibilityIdentifier("record.save")
            Text(x).accessibilityIdentifier("record.result.\(option.key)")
            Text(y).accessibilityIdentifier("\(name)")
            Text(z).accessibilityIdentifier("a\"b")
        "#;
        assert_eq!(
            identifiers(text),
            vec![
                ("a\\\"b".to_string(), false),
                ("record.result.".to_string(), true),
                ("record.save".to_string(), false),
            ]
        );
    }

    #[test]
    fn rust_exports_and_their_swift_names() {
        let text = r#"
#[uniffi::export]
impl AppCore {
    pub fn record_session(&self) -> u32 { helper(1) }
    fn private_one(&self) {}
}
#[uniffi::export]
pub fn version() -> String { String::new() }
#[derive(Debug, uniffi::Record)]
pub struct SessionFeedback { pub text: String }
"#;
        let names: Vec<String> = rust_exports(text)
            .iter()
            .map(RustItem::swift_name)
            .collect();
        assert_eq!(names, vec!["recordSession", "version", "SessionFeedback"]);
    }

    #[test]
    fn a_changed_view_links_its_own_identifiers_and_the_views_that_present_it() {
        let impact = impact(
            &repo(),
            "examples/good-dog",
            &["examples/good-dog/Sources/AppHost/ExerciseView.swift".into()],
        );
        let v = impact.to_json();
        let direct: Vec<&str> = v["direct"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(direct.contains(&"record.save"), "{direct:?}");
        assert!(direct.contains(&"record.result.*"), "{direct:?}");
        let entry: Vec<&str> = v["entry"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        // TodayView presents ExerciseView; its rows are the way in.
        assert!(entry.contains(&"today.activity.*"), "{entry:?}");
        let how = impact
            .links
            .iter()
            .find(|l| l.id == "today.activity.")
            .unwrap()
            .how
            .clone();
        assert!(how.contains("TodayView.swift uses ExerciseView"), "{how}");
        // Every link says what it rests on, and the result says first that
        // none of it is proof.
        assert_eq!(v["method"], "textual");
        assert!(v["basis_note"]
            .as_str()
            .unwrap()
            .contains("not a proven dependency"));
        let basis = |id: &str| {
            v["links"]
                .as_array()
                .unwrap()
                .iter()
                .find(|l| l["id"] == id)
                .unwrap()["basis"]
                .clone()
        };
        assert_eq!(basis("record.save"), "declared");
        assert_eq!(basis("today.activity."), "inferred");
        let mut text = Vec::new();
        show(
            &repo().join("examples/good-dog"),
            None,
            &["Sources/AppHost/ExerciseView.swift".into()],
            &mut text,
        )
        .unwrap();
        let text = String::from_utf8(text).unwrap();
        assert!(
            text.starts_with("Read from the source as text, not compiled"),
            "{text}"
        );
        assert!(
            text.contains("  today.activity.*, today.progress, today.shorten: TodayView.swift uses ExerciseView"),
            "{text}"
        );
        // The journey test taps record.save.
        assert!(impact
            .tests
            .iter()
            .any(|(f, ids)| f.ends_with("uitests/journey.toml")
                && ids.contains(&"record.save".to_string())));
        assert!(impact.unmapped.is_empty());
    }

    #[test]
    fn a_rust_change_reaches_the_views_through_the_model() {
        let impact = impact(
            &repo(),
            "examples/good-dog",
            &["examples/good-dog/rust/src/training.rs".into()],
        );
        // training.rs exports nothing itself: lib.rs's exports use it, the
        // Swift model calls them, and views call the model.
        assert!(
            impact.links.iter().any(|l| l.strength == Strength::Entry
                && l.how.contains("training.rs")
                && l.how.contains("calls")),
            "{:#?}",
            impact.links.iter().take(5).collect::<Vec<_>>()
        );
        assert!(impact.links.iter().all(|l| l.strength != Strength::Direct));
    }

    #[test]
    fn files_without_a_link_say_why() {
        let impact = impact(
            &repo(),
            "examples/good-dog",
            &[
                "examples/good-dog/README.md".into(),
                "crates/forge-runtime/src/lib.rs".into(),
                "examples/good-dog/Sources/AppHost/Gone.swift".into(),
            ],
        );
        assert!(impact.links.is_empty());
        let why: BTreeMap<&str, &str> = impact
            .unmapped
            .iter()
            .map(|(f, w)| (f.as_str(), w.as_str()))
            .collect();
        assert!(why["examples/good-dog/README.md"].contains("not app source"));
        assert!(why["crates/forge-runtime/src/lib.rs"].contains("outside the app"));
        assert!(why["examples/good-dog/Sources/AppHost/Gone.swift"].contains("deleted"));
    }

    #[test]
    fn whole_words_and_calls() {
        assert!(mentions("let x = ExerciseView(model)", "ExerciseView"));
        assert!(!mentions("let x = ExerciseViewModel()", "ExerciseView"));
        assert!(calls("await model.record(exerciseId: 1)", "record"));
        assert!(!calls("public func record(", "record"));
        assert!(!calls("let recorded = 1", "record"));
    }
}
