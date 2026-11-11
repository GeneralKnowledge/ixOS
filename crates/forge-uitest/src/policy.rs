//! What Forge may do in apps it didn't build (docs/research/general-device-control.md#safety).
//!
//! A test file can name any installed app, and WebDriverAgent will drive it.
//! For the developer's own apps nothing changes. Every other app is behind
//! three checks, all on the Linux side, so an agent that read something
//! persuasive on screen can't talk its way past them:
//!
//! 1. **`--any-app`.** Without it, Forge only acts in the developer's own
//!    apps ([`Gate::own`]).
//! 2. **A per-app level** from a file the person writes,
//!    `~/.config/forge/device-control.toml` ([`Policy`]): `deny`, `observe`,
//!    `operate`, `full`, or `ask`. It lives outside every project, so an
//!    agent editing a project can't loosen it.
//! 3. **The commit gate.** At `operate`, a tap on something whose label or
//!    identifier says send, post, pay, delete and the like ([`is_commit`]),
//!    or typing that presses Return, needs the person's approval each time.
//!    With nobody at a terminal to ask, it is refused.
//!
//! Some things are refused at every level ([`never`]): Settings, Wallet,
//! Passwords, the App Store, Find My, Phone, FaceTime, payment and Face ID
//! sheets, and typing into secure fields.
//!
//! Each decision in another app is written to a journal (time, app, action,
//! target, decision; typed text only as its length).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// What Forge may do in one app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Nothing: not launched, not read.
    Deny,
    /// Launch, read the tree, screenshots, scroll, expectations.
    Observe,
    /// Also tap, type, clear, stop. Commits ask each time.
    Operate,
    /// Everything except the hard refusals; no commit gate.
    Full,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Deny => "deny",
            Level::Observe => "observe",
            Level::Operate => "operate",
            Level::Full => "full",
        }
    }
}

/// A line of the policy file: a level, or ask the person once per run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rule {
    Deny,
    Observe,
    Operate,
    Full,
    Ask,
}

impl Rule {
    pub fn level(self) -> Option<Level> {
        match self {
            Rule::Deny => Some(Level::Deny),
            Rule::Observe => Some(Level::Observe),
            Rule::Operate => Some(Level::Operate),
            Rule::Full => Some(Level::Full),
            Rule::Ask => None,
        }
    }
}

/// `~/.config/forge/device-control.toml`:
///
/// ```toml
/// default = "ask"                     # apps not listed
///
/// [apps]
/// "com.apple.weather" = "operate"
/// "com.example.bank" = "observe"
/// "org.forgemobile.agentlab" = "full"
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    #[serde(default = "ask")]
    pub default: Rule,
    #[serde(default)]
    pub apps: BTreeMap<String, Rule>,
    /// Where it was read from, for messages; `None` when there is no file.
    #[serde(skip)]
    pub source: Option<PathBuf>,
}

fn ask() -> Rule {
    Rule::Ask
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            default: Rule::Ask,
            apps: BTreeMap::new(),
            source: None,
        }
    }
}

/// The policy file's place: `$XDG_CONFIG_HOME/forge/device-control.toml`,
/// else `~/.config/forge/device-control.toml`.
pub fn policy_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|h| PathBuf::from(h).join(".config"))
        })?;
    Some(base.join("forge").join("device-control.toml"))
}

/// Where runs in other apps keep their screens and journal:
/// `$XDG_CACHE_HOME/forge/any-app/`, else `~/.cache/forge/any-app/`.
/// Outside every project, so nothing from the person's apps is committed.
pub fn session_root() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|h| PathBuf::from(h).join(".cache"))
        })?;
    Some(base.join("forge").join("any-app"))
}

impl Policy {
    /// Reads the person's policy. No file means every other app is `ask`.
    pub fn load() -> Result<Policy, String> {
        match policy_path() {
            Some(path) if path.is_file() => Policy::load_from(&path),
            _ => Ok(Policy::default()),
        }
    }

    pub fn load_from(path: &Path) -> Result<Policy, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut policy = Policy::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        policy.source = Some(path.to_path_buf());
        Ok(policy)
    }

    pub fn parse(text: &str) -> Result<Policy, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// The rule for an app: its own line (by installed id, or the id before
    /// xtool's `XTL-<team>.` prefix), else the default.
    pub fn rule_for(&self, bundle_id: &str) -> Rule {
        let original = original_id(bundle_id);
        self.apps
            .get(bundle_id)
            .or_else(|| self.apps.get(original))
            .copied()
            .unwrap_or(self.default)
    }

    /// Where the person can change it, for messages.
    pub fn where_(&self) -> String {
        self.source
            .clone()
            .or_else(policy_path)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "~/.config/forge/device-control.toml".into())
    }
}

/// The bundle id without xtool's free-team `XTL-<team>.` prefix.
pub fn original_id(bundle_id: &str) -> &str {
    bundle_id
        .strip_prefix("XTL-")
        .and_then(|rest| rest.split_once('.'))
        .map(|(_, original)| original)
        .unwrap_or(bundle_id)
}

/// The home screen. Reading it shows app icons, nothing inside an app, and
/// it is what is in front after a test stops its own app.
pub const SPRINGBOARD: &str = "com.apple.springboard";

/// Apps and system screens Forge never reads or touches, at any level.
const NEVER: &[(&str, &str)] = &[
    ("com.apple.Preferences", "Settings"),
    ("com.apple.Passbook", "Wallet"),
    ("com.apple.PassbookUIService", "the Apple Pay sheet"),
    ("com.apple.Passwords", "Passwords"),
    ("com.apple.AppStore", "the App Store"),
    (
        "com.apple.ios.StoreKitUIService",
        "the App Store purchase sheet",
    ),
    ("com.apple.findmy", "Find My"),
    ("com.apple.mobilephone", "Phone"),
    ("com.apple.facetime", "FaceTime"),
    ("com.apple.CoreAuthUI", "the Face ID and passcode prompt"),
];

/// Why Forge refuses this app at every level, if it does.
pub fn never(bundle_id: &str) -> Option<String> {
    NEVER
        .iter()
        .find(|(id, _)| id.eq_ignore_ascii_case(bundle_id))
        .map(|(id, name)| {
            format!("Forge never reads or operates {name} ({id}), whatever the policy says")
        })
}

/// Words that mark a commit: something that leaves the phone or can't be
/// undone. Matched against the words of a control's label and identifier.
/// It will miss some, which is why money and messaging apps should stay at
/// `observe`.
const COMMIT_WORDS: &[&str] = &[
    "send",
    "post",
    "reply",
    "repost",
    "retweet",
    "tweet",
    "publish",
    "share",
    "submit",
    "pay",
    "buy",
    "purchase",
    "order",
    "checkout",
    "subscribe",
    "transfer",
    "donate",
    "delete",
    "remove",
    "erase",
    "block",
    "report",
    "call",
    "invite",
    "follow",
    "unfollow",
    "like",
    "allow",
];

/// The words of a label or identifier: lowercase, split at anything that
/// isn't a letter or digit and at camelCase humps (`sendButton` → send,
/// button).
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut prev_lower = false;
    for ch in text.chars() {
        if !ch.is_alphanumeric() {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            prev_lower = false;
            continue;
        }
        if ch.is_uppercase() && prev_lower && !word.is_empty() {
            out.push(std::mem::take(&mut word));
        }
        prev_lower = ch.is_lowercase();
        word.extend(ch.to_lowercase());
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// Whether `word` is `base` or a plain inflection of it (sends, sent is not
/// covered, posted, posting, deleting).
fn inflects(word: &str, base: &str) -> bool {
    if word == base {
        return true;
    }
    let Some(rest) = word.strip_prefix(base) else {
        // delete → deleting, deleted
        return base
            .strip_suffix('e')
            .and_then(|stem| word.strip_prefix(stem))
            .is_some_and(|rest| matches!(rest, "ing" | "ed"));
    };
    matches!(rest, "s" | "es" | "ed" | "d" | "ing" | "now")
}

/// The commit word in a control's label or identifier, if any.
pub fn is_commit(label: Option<&str>, identifier: Option<&str>) -> Option<String> {
    for text in [label, identifier].into_iter().flatten() {
        for word in words(text) {
            if let Some(base) = COMMIT_WORDS.iter().find(|base| inflects(&word, base)) {
                return Some((*base).to_owned());
            }
        }
    }
    None
}

/// Kinds a tap only focuses: never a commit by themselves.
fn is_text_input(kind: &str) -> bool {
    matches!(
        kind.strip_prefix("XCUIElementType").unwrap_or(kind),
        "TextField" | "SearchField" | "TextView"
    )
}

fn is_secure(kind: &str) -> bool {
    kind.strip_prefix("XCUIElementType").unwrap_or(kind) == "SecureTextField"
}

/// The element an action is about, as the tree describes it.
#[derive(Debug, Clone, Default)]
pub struct Target {
    pub label: Option<String>,
    pub identifier: Option<String>,
    /// `Button`, `TextField`… (with or without `XCUIElementType`).
    pub kind: Option<String>,
}

impl Target {
    /// Neither a label nor an identifier: nothing says what it does.
    fn is_unidentified(&self) -> bool {
        let blank = |t: &Option<String>| t.as_deref().is_none_or(|t| t.trim().is_empty());
        blank(&self.label) && blank(&self.identifier)
    }

    fn name(&self) -> String {
        match (&self.label, &self.identifier) {
            (Some(l), _) if !l.is_empty() => format!("\"{l}\""),
            (_, Some(id)) if !id.is_empty() => format!("`{id}`"),
            _ => format!(
                "an unlabelled {}",
                self.kind
                    .as_deref()
                    .map(|k| k.strip_prefix("XCUIElementType").unwrap_or(k))
                    .unwrap_or("element")
            ),
        }
    }
}

/// One thing a test step does, as the gate sees it.
#[derive(Debug, Clone)]
pub enum Action {
    Launch,
    Activate,
    Background,
    /// Tree, screenshot, expectation: reading the screen.
    Read,
    Scroll,
    Tap(Target),
    Type {
        target: Target,
        /// Only the length is ever written down.
        chars: usize,
        /// The text has a newline: Return sends in many apps.
        presses_return: bool,
    },
    Clear(Target),
    /// A swipe, long press, double tap or drag on an element (`gesture`
    /// names which, as test files spell it).
    Gesture {
        gesture: &'static str,
        target: Target,
    },
    /// Something done to the phone rather than to an element: `home`,
    /// `button`, `open_url`, `orientation`, `reset_permission`, `lock`,
    /// `unlock`. `what` says what, for the question and the journal (a
    /// button's name, a URL's scheme and host).
    Device {
        name: &'static str,
        what: String,
    },
    /// A keyboard key: Return, Delete, or hiding the keyboard.
    Key {
        name: &'static str,
        /// Return sends in many apps.
        presses_return: bool,
    },
    Terminate,
}

impl Action {
    fn name(&self) -> &'static str {
        match self {
            Action::Launch => "launch",
            Action::Activate => "activate",
            Action::Background => "background",
            Action::Read => "read",
            Action::Scroll => "scroll",
            Action::Tap(_) => "tap",
            Action::Type { .. } => "type",
            Action::Clear(_) => "clear",
            Action::Gesture { gesture, .. } => gesture,
            Action::Key { .. } => "key",
            Action::Device { name, .. } => name,
            Action::Terminate => "terminate",
        }
    }

    fn target(&self) -> Option<&Target> {
        match self {
            Action::Tap(t)
            | Action::Clear(t)
            | Action::Type { target: t, .. }
            | Action::Gesture { target: t, .. } => Some(t),
            _ => None,
        }
    }

    /// The least level that allows it.
    fn needs(&self) -> Level {
        match self {
            Action::Launch
            | Action::Activate
            | Action::Background
            | Action::Read
            | Action::Scroll
            | Action::Key {
                name: "dismiss", ..
            }
            | Action::Device {
                name: "home" | "orientation",
                ..
            } => Level::Observe,
            _ => Level::Operate,
        }
    }
}

/// Asks the person. `None`: nobody to ask (no terminal), so the answer is no.
pub trait Approver {
    /// A yes/no question about one commit.
    fn approve(&mut self, question: &str) -> Option<bool>;
    /// The level for an app the policy says to ask about.
    fn choose(&mut self, app: &str) -> Option<Level>;
}

/// Never anyone to ask: every commit and every `ask` is refused.
pub struct NoApprover;

impl Approver for NoApprover {
    fn approve(&mut self, _: &str) -> Option<bool> {
        None
    }
    fn choose(&mut self, _: &str) -> Option<Level> {
        None
    }
}

/// Asks on the terminal, when stdin and stderr are both one (not when an
/// MCP server or a script runs forge).
pub struct TerminalApprover;

impl TerminalApprover {
    fn ask(prompt: &str) -> Option<String> {
        use std::io::IsTerminal;
        if !(std::io::stdin().is_terminal() && std::io::stderr().is_terminal()) {
            return None;
        }
        eprint!("{prompt} ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).ok()?;
        Some(line.trim().to_lowercase())
    }
}

impl Approver for TerminalApprover {
    fn approve(&mut self, question: &str) -> Option<bool> {
        let answer = TerminalApprover::ask(&format!("{question} [y/N]"))?;
        Some(matches!(answer.as_str(), "y" | "yes"))
    }

    fn choose(&mut self, app: &str) -> Option<Level> {
        let answer = TerminalApprover::ask(&format!(
            "{app} is not in your device-control policy. For this run: [d]eny, [o]bserve (read and scroll only) or o[p]erate (tap and type; commits still ask)? [d/o/p]"
        ))?;
        Some(match answer.as_str() {
            "o" | "observe" => Level::Observe,
            "p" | "operate" => Level::Operate,
            _ => Level::Deny,
        })
    }
}

/// Every decision in another app, one JSON line each. Typed text is never
/// written, only its length.
pub struct Journal {
    file: std::fs::File,
    pub path: PathBuf,
}

impl Journal {
    pub fn create(dir: &Path) -> std::io::Result<Journal> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("journal.jsonl");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        Ok(Journal { file, path })
    }

    fn write(&mut self, app: &str, action: &Action, decision: &str, reason: Option<&str>) {
        let target = action.target();
        let mut line = serde_json::json!({
            "time": unix_ms(),
            "app": app,
            "action": action.name(),
            "decision": decision,
        });
        if let Some(t) = target {
            line["target"] = serde_json::json!({
                "label": t.label,
                "identifier": t.identifier,
                "kind": t.kind,
            });
        }
        if let Action::Type {
            chars,
            presses_return,
            ..
        } = action
        {
            line["typed_chars"] = serde_json::json!(chars);
            line["presses_return"] = serde_json::json!(presses_return);
        }
        if let Some(reason) = reason {
            line["reason"] = serde_json::json!(reason);
        }
        let _ = writeln!(self.file, "{line}");
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Decides each step of a run bound to one app.
pub struct Gate {
    app: String,
    level: Level,
    /// Another developer's app: hard refusals and the journal apply.
    foreign: bool,
    approver: Box<dyn Approver>,
    journal: Option<Journal>,
}

impl Gate {
    /// The developer's own app: everything is allowed, as before.
    pub fn own(app: &str) -> Gate {
        Gate {
            app: app.to_owned(),
            level: Level::Full,
            foreign: false,
            approver: Box::new(NoApprover),
            journal: None,
        }
    }

    /// Another developer's app at `level`.
    pub fn foreign(
        app: &str,
        level: Level,
        approver: Box<dyn Approver>,
        journal: Option<Journal>,
    ) -> Gate {
        Gate {
            app: app.to_owned(),
            level,
            foreign: true,
            approver,
            journal,
        }
    }

    pub fn is_foreign(&self) -> bool {
        self.foreign
    }

    pub fn level(&self) -> Level {
        self.level
    }

    /// Whether the gate needs to know what a tap or typing will touch
    /// (its label, identifier and kind) before allowing it.
    pub fn wants_targets(&self) -> bool {
        self.foreign
    }

    /// Allows the action, asks the person, or refuses with the reason.
    pub fn check(&mut self, action: &Action) -> Result<(), String> {
        let decision = self.decide(action);
        if let Some(journal) = self.journal.as_mut() {
            match &decision {
                Ok(how) => journal.write(&self.app, action, how, None),
                Err(why) => journal.write(&self.app, action, "refused", Some(why)),
            }
        }
        decision.map(|_| ())
    }

    fn decide(&mut self, action: &Action) -> Result<&'static str, String> {
        if !self.foreign {
            return Ok("allowed");
        }
        if let Some(why) = never(&self.app) {
            return Err(why);
        }
        if self.level < action.needs() {
            return Err(format!(
                "{} {} needs the `{}` level in {}; the policy gives it `{}`",
                action.name(),
                action.target().map(Target::name).unwrap_or_default(),
                action.needs().name(),
                self.app,
                self.level.name()
            )
            .replace("  ", " "));
        }
        let target = action.target();
        if let Some(t) = target {
            if t.kind.as_deref().is_some_and(is_secure) {
                return Err(format!(
                    "{} is a secure field (password, passcode or code): Forge never types into or reads one in another app; the person does that step",
                    t.name()
                ));
            }
        }
        if self.level == Level::Full {
            return Ok("allowed");
        }
        let commit = match action {
            Action::Tap(t) if !t.kind.as_deref().is_some_and(is_text_input) => {
                is_commit(t.label.as_deref(), t.identifier.as_deref())
                    .map(|word| format!("tap {} (\"{word}\")", t.name()))
                    // Nothing says what it does (found by kind or position):
                    // it could be Send, so it is treated as a commit.
                    .or_else(|| {
                        t.is_unidentified().then(|| {
                            format!(
                                "tap {}, which has no label or identifier to say what it does",
                                t.name()
                            )
                        })
                    })
            }
            Action::Type {
                target,
                presses_return: true,
                ..
            } => Some(format!(
                "type text ending in Return into {}, which can send it",
                target.name()
            )),
            // A full swipe runs a row's first action (often Delete) and a
            // drag moves things: neither says what it will do.
            Action::Gesture {
                gesture: gesture @ ("swipe" | "drag"),
                target,
            } => Some(format!(
                "{gesture} {}, which can delete, archive or move it",
                target.name()
            )),
            // Like a tap: what the label says, or nothing to say.
            Action::Gesture { gesture, target } => {
                is_commit(target.label.as_deref(), target.identifier.as_deref())
                    .map(|word| format!("{gesture} {} (\"{word}\")", target.name()))
                    .or_else(|| {
                        target.is_unidentified().then(|| {
                            format!(
                        "{gesture} {}, which has no label or identifier to say what it does",
                        target.name()
                    )
                        })
                    })
            }
            Action::Key {
                presses_return: true,
                ..
            } => Some("press Return, which can send what was typed".to_string()),
            // A link can start a call, a payment or a purchase; the Action
            // and Camera buttons do whatever the person set them to.
            Action::Device {
                name: name @ ("open_url" | "button" | "reset_permission" | "lock" | "unlock"),
                what,
            } => Some(format!("{} {what}", name.replace('_', " "))),
            _ => None,
        };
        let Some(commit) = commit else {
            return Ok("allowed");
        };
        let question = format!(
            "Forge is about to {commit} in {}. This can change something outside the phone. Allow it?",
            self.app
        );
        match self.approver.approve(&question) {
            Some(true) => Ok("approved"),
            Some(false) => Err(format!("the person declined: {commit}")),
            None => Err(format!(
                "{commit} in {} is a commit, which needs the person's approval each time, and there is nobody at a terminal to ask. Ask the person to do this step, or to run forge themselves",
                self.app
            )),
        }
    }
}

/// How a run reaches an app it was pointed at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    /// One of the developer's own apps.
    Own,
    /// Another developer's app, allowed at this level.
    Foreign(Level),
}

/// Who an app belongs to, and so which rules apply to it.
pub struct Scope<'a> {
    /// `--any-app` was given.
    pub any_app: bool,
    /// Bundle ids of the developer's own apps (forge.toml next to the test
    /// file or in the current folder).
    pub own: &'a [String],
    pub policy: &'a Policy,
}

impl Scope<'_> {
    /// Whether `installed` (the id on the phone) is the developer's own:
    /// installed by xtool under a free team (`XTL-` prefix), named by a
    /// forge.toml here, or marked `full` in the person's policy.
    pub fn is_own(&self, installed: &str) -> bool {
        installed.starts_with("XTL-")
            || self
                .own
                .iter()
                .any(|id| id == installed || id == original_id(installed))
            || self.policy.rule_for(installed) == Rule::Full
    }

    /// Decides whether a run may bind to `installed`, asking the person for
    /// an `ask` app when there is a terminal.
    pub fn access(&self, installed: &str, approver: &mut dyn Approver) -> Result<Access, String> {
        if let Some(why) = never(installed) {
            return Err(why);
        }
        if self.is_own(installed) {
            return Ok(Access::Own);
        }
        if !self.any_app {
            return Err(format!(
                "{installed} is not one of your Forge apps. Forge only acts in apps it didn't build when the person passes --any-app, and then only as far as {} allows (docs/forge-ui-testing.md#apps-forge-didnt-build)",
                self.policy.where_()
            ));
        }
        let level = match self.policy.rule_for(installed).level() {
            Some(level) => level,
            None => approver.choose(installed).ok_or_else(|| {
                format!(
                    "{installed} is not in {} (default: ask), and there is nobody at a terminal to ask. Add a line for it, e.g. \"{installed}\" = \"observe\"",
                    self.policy.where_()
                )
            })?,
        };
        if level == Level::Deny {
            return Err(format!("{installed} is `deny` in {}", self.policy.where_()));
        }
        Ok(Access::Foreign(level))
    }

    /// Whether a read of the screen (tree, screenshot) may happen with
    /// `front` in front. The home screen and the developer's own apps are
    /// always fine; another app needs `--any-app` and at least `observe`.
    pub fn may_read(&self, front: Option<&str>, approver: &mut dyn Approver) -> Result<(), String> {
        let Some(front) = front else {
            return if self.any_app {
                Ok(())
            } else {
                Err("WebDriverAgent did not say which app is in front, so Forge won't read the screen; bring your app to the front, or pass --any-app".into())
            };
        };
        if front == SPRINGBOARD {
            return Ok(());
        }
        self.access(front, approver).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Says(Option<bool>, Option<Level>, Vec<String>);
    impl Approver for Says {
        fn approve(&mut self, q: &str) -> Option<bool> {
            self.2.push(q.to_owned());
            self.0
        }
        fn choose(&mut self, _: &str) -> Option<Level> {
            self.1
        }
    }

    fn button(label: &str) -> Target {
        Target {
            label: Some(label.into()),
            identifier: None,
            kind: Some("XCUIElementTypeButton".into()),
        }
    }

    #[test]
    fn the_policy_file_parses_and_strips_xtool_prefixes() {
        let policy = Policy::parse(
            r#"
default = "deny"
[apps]
"com.apple.weather" = "operate"
"org.forgemobile.agentlab" = "full"
"#,
        )
        .unwrap();
        assert_eq!(policy.rule_for("com.apple.weather"), Rule::Operate);
        assert_eq!(
            policy.rule_for("XTL-ND4P87PX52.org.forgemobile.agentlab"),
            Rule::Full
        );
        assert_eq!(policy.rule_for("com.example.bank"), Rule::Deny);
        assert_eq!(Policy::default().rule_for("x.y"), Rule::Ask);
        assert!(Policy::parse("default = \"sometimes\"").is_err());
        assert!(Policy::parse("defualt = \"ask\"").is_err());
    }

    #[test]
    fn commit_words_match_labels_and_identifiers() {
        assert_eq!(is_commit(Some("Send"), None).as_deref(), Some("send"));
        assert_eq!(
            is_commit(None, Some("composer.sendButton")).as_deref(),
            Some("send")
        );
        assert_eq!(is_commit(Some("Pay now"), None).as_deref(), Some("pay"));
        assert_eq!(
            is_commit(Some("Delete Conversation"), None).as_deref(),
            Some("delete")
        );
        assert_eq!(
            is_commit(Some("Deleting…"), None).as_deref(),
            Some("delete")
        );
        assert_eq!(is_commit(Some("Posts"), None).as_deref(), Some("post"));
        assert_eq!(is_commit(Some("Allow"), None).as_deref(), Some("allow"));
        // Not commits.
        assert_eq!(is_commit(Some("Calendar"), None), None);
        assert_eq!(is_commit(Some("Payload size"), None), None);
        assert_eq!(is_commit(Some("chat-input"), Some("chat-input")), None);
        assert_eq!(is_commit(Some("Back"), Some("BackButton")), None);
    }

    #[test]
    fn own_apps_are_never_gated() {
        let mut gate = Gate::own("XTL-ND4P87PX52.org.forgemobile.pocketlibrary");
        assert!(gate.check(&Action::Tap(button("Delete"))).is_ok());
        assert!(gate
            .check(&Action::Type {
                target: Target {
                    kind: Some("SecureTextField".into()),
                    ..Target::default()
                },
                chars: 4,
                presses_return: true,
            })
            .is_ok());
    }

    #[test]
    fn levels_limit_what_another_app_allows() {
        let mut observe = Gate::foreign(
            "com.example.bank",
            Level::Observe,
            Box::new(NoApprover),
            None,
        );
        assert!(observe.check(&Action::Read).is_ok());
        assert!(observe.check(&Action::Scroll).is_ok());
        assert!(observe.check(&Action::Launch).is_ok());
        let err = observe.check(&Action::Tap(button("Accounts"))).unwrap_err();
        assert!(err.contains("needs the `operate` level"), "{err}");
        assert!(observe.check(&Action::Terminate).is_err());

        let mut operate = Gate::foreign(
            "com.apple.weather",
            Level::Operate,
            Box::new(NoApprover),
            None,
        );
        assert!(operate.check(&Action::Tap(button("London"))).is_ok());
        assert!(operate
            .check(&Action::Type {
                target: Target::default(),
                chars: 6,
                presses_return: false
            })
            .is_ok());
    }

    #[test]
    fn commits_need_approval_and_are_refused_without_a_person() {
        let mut gate = Gate::foreign(
            "com.anthropic.claude",
            Level::Operate,
            Box::new(NoApprover),
            None,
        );
        let err = gate.check(&Action::Tap(button("Send"))).unwrap_err();
        assert!(err.contains("needs the person's approval"), "{err}");
        let err = gate
            .check(&Action::Type {
                target: Target::default(),
                chars: 5,
                presses_return: true,
            })
            .unwrap_err();
        assert!(err.contains("Return"), "{err}");
        // A text field labelled "Send a message" only takes focus.
        let field = Target {
            label: Some("Send a message".into()),
            identifier: None,
            kind: Some("XCUIElementTypeTextView".into()),
        };
        assert!(gate.check(&Action::Tap(field)).is_ok());

        let mut yes = Gate::foreign(
            "com.anthropic.claude",
            Level::Operate,
            Box::new(Says(Some(true), None, vec![])),
            None,
        );
        assert!(yes.check(&Action::Tap(button("Send"))).is_ok());
        let mut no = Gate::foreign(
            "com.anthropic.claude",
            Level::Operate,
            Box::new(Says(Some(false), None, vec![])),
            None,
        );
        assert!(no
            .check(&Action::Tap(button("Send")))
            .unwrap_err()
            .contains("declined"));
        // Full: no commit gate, but still no secure fields.
        let mut full = Gate::foreign("org.example.mine", Level::Full, Box::new(NoApprover), None);
        assert!(full.check(&Action::Tap(button("Send"))).is_ok());
        let secure = Target {
            kind: Some("XCUIElementTypeSecureTextField".into()),
            ..Target::default()
        };
        assert!(full
            .check(&Action::Type {
                target: secure,
                chars: 8,
                presses_return: false
            })
            .unwrap_err()
            .contains("secure field"));
    }

    #[test]
    fn a_tap_on_an_unlabelled_control_is_a_commit() {
        // DeepSeek's send arrow can be a Button with no label and no id: a
        // tap found by kind or position must not slip past the gate.
        let unlabelled = |kind: &str, label: Option<&str>| Target {
            label: label.map(str::to_owned),
            identifier: None,
            kind: Some(format!("XCUIElementType{kind}")),
        };
        let mut gate = Gate::foreign(
            "com.deepseek.chat",
            Level::Operate,
            Box::new(NoApprover),
            None,
        );
        let err = gate
            .check(&Action::Tap(unlabelled("Button", None)))
            .unwrap_err();
        assert!(err.contains("no label or identifier"), "{err}");
        assert!(err.contains("needs the person's approval"), "{err}");
        // A blank label is no label.
        assert!(gate
            .check(&Action::Tap(unlabelled("Button", Some("  "))))
            .is_err());
        // Focusing an unlabelled text view is fine; so is a labelled,
        // harmless button.
        assert!(gate
            .check(&Action::Tap(unlabelled("TextView", None)))
            .is_ok());
        assert!(gate
            .check(&Action::Tap(unlabelled("Button", Some("Think"))))
            .is_ok());
        // The person can allow it, and `full` doesn't ask.
        let mut yes = Gate::foreign(
            "com.deepseek.chat",
            Level::Operate,
            Box::new(Says(Some(true), None, vec![])),
            None,
        );
        assert!(yes.check(&Action::Tap(unlabelled("Button", None))).is_ok());
        let mut full = Gate::foreign("com.deepseek.chat", Level::Full, Box::new(NoApprover), None);
        assert!(full.check(&Action::Tap(unlabelled("Button", None))).is_ok());
    }

    #[test]
    fn some_apps_are_refused_at_every_level() {
        let policy = Policy::parse("[apps]\n\"com.apple.Preferences\" = \"full\"").unwrap();
        let scope = Scope {
            any_app: true,
            own: &[],
            policy: &policy,
        };
        let err = scope
            .access("com.apple.Preferences", &mut NoApprover)
            .unwrap_err();
        assert!(err.contains("Settings"), "{err}");
        let mut gate = Gate::foreign(
            "com.apple.Passbook",
            Level::Full,
            Box::new(NoApprover),
            None,
        );
        assert!(gate.check(&Action::Read).is_err());
    }

    #[test]
    fn other_apps_need_any_app_and_a_level() {
        let policy = Policy::parse(
            "[apps]\n\"com.apple.weather\" = \"observe\"\n\"com.example.bank\" = \"deny\"",
        )
        .unwrap();
        let own = vec!["org.forgemobile.pocketlibrary".to_owned()];
        let without = Scope {
            any_app: false,
            own: &own,
            policy: &policy,
        };
        assert_eq!(
            without.access("org.forgemobile.pocketlibrary", &mut NoApprover),
            Ok(Access::Own)
        );
        assert_eq!(
            without.access("XTL-ND4P87PX52.com.example.anything", &mut NoApprover),
            Ok(Access::Own)
        );
        let err = without
            .access("com.apple.weather", &mut NoApprover)
            .unwrap_err();
        assert!(err.contains("--any-app"), "{err}");
        assert!(without
            .may_read(Some("com.anthropic.claude"), &mut NoApprover)
            .is_err());
        assert!(without.may_read(Some(SPRINGBOARD), &mut NoApprover).is_ok());
        assert!(without.may_read(None, &mut NoApprover).is_err());

        let with = Scope {
            any_app: true,
            own: &own,
            policy: &policy,
        };
        assert_eq!(
            with.access("com.apple.weather", &mut NoApprover),
            Ok(Access::Foreign(Level::Observe))
        );
        assert!(with
            .access("com.example.bank", &mut NoApprover)
            .unwrap_err()
            .contains("deny"));
        // Not listed: ask, and with nobody to ask, refused.
        assert!(with
            .access("com.anthropic.claude", &mut NoApprover)
            .unwrap_err()
            .contains("nobody at a terminal"));
        let mut says = Says(None, Some(Level::Operate), vec![]);
        assert_eq!(
            with.access("com.anthropic.claude", &mut says),
            Ok(Access::Foreign(Level::Operate))
        );
    }

    #[test]
    fn the_journal_keeps_lengths_not_text() {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::create(dir.path()).unwrap();
        let path = journal.path.clone();
        let mut gate = Gate::foreign(
            "com.anthropic.claude",
            Level::Operate,
            Box::new(NoApprover),
            Some(journal),
        );
        gate.check(&Action::Type {
            target: Target {
                identifier: Some("chat-input".into()),
                ..Target::default()
            },
            chars: 11,
            presses_return: false,
        })
        .unwrap();
        let _ = gate.check(&Action::Tap(button("Send")));
        drop(gate);
        let text = std::fs::read_to_string(path).unwrap();
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["typed_chars"], 11);
        assert_eq!(lines[0]["decision"], "allowed");
        assert_eq!(lines[1]["decision"], "refused");
        assert_eq!(lines[1]["target"]["label"], "Send");
    }
}
