//! Exploratory UI testing: walk an app's screens without a script, check
//! each one, and write down what went wrong as `forge ui run` files that
//! replay it. See `docs/forge-explore.md`.
//!
//! The walk works on [`UiNode`] trees through any [`Driver`], so it runs the
//! same against WebDriverAgent today and against a long-lived session or the
//! accessibility service later. It is deliberately simple and deterministic:
//!
//! - A **screen** is identified by its structure (identifiers, and the kinds
//!   and labels of controls without one), never by pixels or values, so the
//!   same screen showing different data is one screen.
//! - On each screen it tries every safe control once, in an order fixed by
//!   the seed. Rows of a list count once.
//! - When a screen has nothing left to try it goes back (a back or dismiss
//!   button), or relaunches the app and replays the shortest known path to
//!   a screen that still has something to try.
//! - It never types into secure fields, never taps controls that act
//!   outside the phone (send, pay, share…), and skips destructive ones
//!   (delete, sign out…) unless allowed.
//! - It stops at the action or time budget, whichever comes first.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::{wait_for, Driver, Locator, Result, UiError, UiNode};

/// What an exploration may do and how long it may take.
#[derive(Debug, Clone)]
pub struct Config {
    /// The installed bundle id (after xtool's free-account renaming).
    pub app: String,
    /// The bundle id as written in forge.toml, for the replay files.
    pub script_app: String,
    /// Stop after this many actions (taps, typing, launches).
    pub max_actions: usize,
    /// Stop after this long, whatever is left.
    pub budget: Duration,
    /// Fixes the order controls are tried in; the same seed on the same
    /// app state walks the same way.
    pub seed: u64,
    /// Passed to every launch, e.g. a larger Dynamic Type size.
    pub launch_args: Vec<String>,
    /// Text to type into a field, by identifier. Other fields get
    /// [`Config::default_text`].
    pub fixtures: HashMap<String, String>,
    pub default_text: String,
    /// Also tap destructive controls (delete, remove, sign out…). Controls
    /// that act outside the phone (send, pay, share…) are never tapped.
    pub allow_destructive: bool,
    /// The app was launched on throwaway test data (its `[ui_test]
    /// test_data_args`), so the explorer may change data: save, add, edit,
    /// rate, type into forms. Without it the walk only looks around: it
    /// never touches anything that could change the person's own data, and
    /// destructive controls stay off whatever `allow_destructive` says.
    pub test_data: bool,
    /// Identifiers or labels never to touch.
    pub skip: Vec<String>,
    /// At most this many controls tried per screen.
    pub per_screen: usize,
    /// How long to wait for a control before giving up on it.
    pub find_timeout: Duration,
    /// Save a screenshot and the tree of every new screen.
    pub screenshots: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            app: String::new(),
            script_app: String::new(),
            max_actions: 60,
            budget: Duration::from_secs(10 * 60),
            seed: 1,
            launch_args: Vec::new(),
            fixtures: HashMap::new(),
            default_text: "Forge".into(),
            allow_destructive: false,
            test_data: false,
            skip: Vec::new(),
            per_screen: 15,
            find_timeout: Duration::from_secs(5),
            screenshots: true,
        }
    }
}

/// UIKit's own launch arguments for the largest accessibility text size,
/// honoured by any app launched through XCTest.
pub const LARGEST_TEXT_ARGS: [&str; 2] = [
    "-UIPreferredContentSizeCategoryName",
    "UICTContentSizeCategoryAccessibilityXXXL",
];

/// One thing the explorer does to the app.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Action {
    Launch,
    Tap(Locator),
    Type(Locator, String),
}

impl Action {
    pub fn describe(&self) -> String {
        match self {
            Action::Launch => "launch".into(),
            Action::Tap(locator) => format!("tap {locator}"),
            Action::Type(locator, text) => format!("type {text:?} into {locator}"),
        }
    }

    /// The action as a `forge ui run` step.
    fn toml_step(&self, launch_args: &[String]) -> String {
        let target = |locator: &Locator| locator.toml_fields();
        match self {
            Action::Launch if launch_args.is_empty() => "launch = {}".into(),
            Action::Launch => format!(
                "launch = {{ args = [{}] }}",
                launch_args
                    .iter()
                    .map(|a| quote(a))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Action::Tap(locator) => format!("tap = {{ {} }}", target(locator)),
            Action::Type(locator, text) => {
                format!("type = {{ {}, text = {} }}", target(locator), quote(text))
            }
        }
    }
}

fn quote(text: &str) -> String {
    toml::Value::String(text.to_owned()).to_string()
}

/// A control the explorer could act on.
#[derive(Debug, Clone)]
struct Candidate {
    action: Action,
    /// Identifies the control across visits: its kind and normalised
    /// identifier, or its label. Rows of a list share one key.
    key: String,
    /// A back or dismiss control: tried last, and used to leave the screen.
    exit: bool,
}

/// A control the explorer chose not to touch, and why.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Skipped {
    pub element: String,
    pub reason: String,
}

/// Words that mark a control acting outside the phone or on someone else:
/// never tapped by the explorer. Matched as whole words of the label or
/// identifier. The any-app policy (docs/research/general-device-control.md)
/// will own the full list; this is the explorer's floor.
const EXTERNAL: &[&str] = &[
    "send",
    "post",
    "reply",
    "pay",
    "buy",
    "purchase",
    "checkout",
    "subscribe",
    "unsubscribe",
    "transfer",
    "share",
    "call",
    "facetime",
    "publish",
    "submit",
    "invite",
    "follow",
    "unfollow",
    "report",
    "block",
    "donate",
];

/// Words that mark a destructive control: skipped unless
/// [`Config::allow_destructive`].
const DESTRUCTIVE: &[&str] = &[
    "delete", "remove", "erase", "reset", "clear", "wipe", "discard", "trash", "logout", "signout",
];

/// Two-word forms of the above.
const DESTRUCTIVE_PAIRS: &[(&str, &str)] = &[("sign", "out"), ("log", "out")];

/// Words that mark a control changing the app's data. Without test data
/// the explorer never touches them. Seen on Cati (2026-11-08): a walk on a
/// real Pocket Library rated a book (`star`), changed its status
/// (`checkmark.circle`), opened it through Continue Reading and saved
/// "Explored Book" entries.
const DATA_CHANGING: &[&str] = &[
    "save",
    "add",
    "create",
    "new",
    "edit",
    "update",
    "rename",
    "set",
    "apply",
    "confirm",
    "done",
    "rate",
    "rating",
    "star",
    "favorite",
    "favourite",
    "like",
    "heart",
    "bookmark",
    "flag",
    "pin",
    "mark",
    "checkmark",
    "status",
    "progress",
    "finish",
    "finished",
    "complete",
    "read",
    "reading",
    "continue",
    "resume",
    "start",
    "import",
    "export",
    "move",
    "archive",
    "restore",
    "upload",
    "download",
    "record",
    "sync",
    "refresh",
];

/// Element kinds that change a setting or a value as soon as they are
/// touched.
const DATA_KINDS: &[&str] = &[
    "Switch",
    "Toggle",
    "Slider",
    "Stepper",
    "Picker",
    "PickerWheel",
    "SegmentedControl",
];

/// Labels of controls that leave a screen.
const EXIT_LABELS: &[&str] = &["back", "cancel", "close", "done", "dismiss", "ok"];

fn words(text: &str) -> Vec<String> {
    // Split on anything but letters and digits, and on camelCase humps.
    let mut out = Vec::new();
    let mut word = String::new();
    let mut prev_lower = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if ch.is_uppercase() && prev_lower && !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            prev_lower = ch.is_lowercase();
            word.extend(ch.to_lowercase());
        } else {
            prev_lower = false;
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// Why a control must not be touched, if it mustn't.
fn gate(node: &UiNode, config: &Config) -> Option<String> {
    for skip in &config.skip {
        if node.identifier.as_deref() == Some(skip) || node.label.as_deref() == Some(skip) {
            return Some("on the skip list".into());
        }
    }
    let mut all = Vec::new();
    for text in [&node.identifier, &node.label].into_iter().flatten() {
        all.extend(words(text));
    }
    if let Some(word) = all.iter().find(|w| EXTERNAL.contains(&w.as_str())) {
        return Some(format!("acts outside the phone (\"{word}\")"));
    }
    if !config.test_data {
        const WHY: &str = "the app is on the person's own data";
        let destructive = all.iter().any(|w| DESTRUCTIVE.contains(&w.as_str()))
            || all
                .windows(2)
                .any(|w| DESTRUCTIVE_PAIRS.contains(&(w[0].as_str(), w[1].as_str())));
        if destructive {
            return Some(format!("destructive; never on {WHY}"));
        }
        if let Some(word) = all.iter().find(|w| DATA_CHANGING.contains(&w.as_str())) {
            return Some(format!("may change data (\"{word}\"); {WHY}"));
        }
        if DATA_KINDS.contains(&node.kind.as_str()) {
            return Some(format!("{} changes a setting; {WHY}", node.kind));
        }
        if TYPEABLE.contains(&node.kind.as_str()) && node.kind != "SearchField" {
            return Some(format!("typing may be saved; {WHY}"));
        }
    }
    if config.allow_destructive {
        return None;
    }
    if let Some(word) = all.iter().find(|w| DESTRUCTIVE.contains(&w.as_str())) {
        return Some(format!("destructive (\"{word}\"); --allow-destructive"));
    }
    if all
        .windows(2)
        .any(|w| DESTRUCTIVE_PAIRS.contains(&(w[0].as_str(), w[1].as_str())))
    {
        return Some("destructive (sign out); --allow-destructive".into());
    }
    None
}

/// Element kinds the explorer taps.
const TAPPABLE: &[&str] = &[
    "Button", "Cell", "Link", "Tab", "Switch", "Toggle", "MenuItem", "Image",
];
/// Element kinds the explorer types into.
const TYPEABLE: &[&str] = &["TextField", "TextView", "SearchField"];

/// Replaces the parts of an identifier that name data (row ids, UUIDs,
/// numbers) with `*`, so `library.book.42` and `library.book.43` are one
/// control and one screen.
pub fn normalise_identifier(id: &str) -> String {
    id.split('.')
        .map(|part| {
            let digits = !part.is_empty() && part.chars().all(|c| c.is_ascii_digit());
            let hexish = part.len() >= 8
                && part.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
                && part.chars().any(|c| c.is_ascii_digit());
            if digits || hexish {
                "*"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// The structure of a screen, independent of the data it shows: visible
/// identifiers (normalised), controls without one by kind and label, and
/// alerts, sheets and navigation bars by kind. Values, frames and the
/// keyboard are left out.
pub fn fingerprint(tree: &UiNode) -> String {
    let mut parts = BTreeSet::new();
    collect_fingerprint(tree, &mut parts);
    // FNV-1a: stable across runs and Rust versions, unlike DefaultHasher.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in &parts {
        for byte in part.bytes().chain([0]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{:08x}", hash >> 32)
}

fn collect_fingerprint(node: &UiNode, parts: &mut BTreeSet<String>) {
    if node.kind == "Keyboard" {
        return;
    }
    if node.visible || node.kind == "Application" {
        if let Some(id) = &node.identifier {
            parts.insert(format!("{}#{}", node.kind, normalise_identifier(id)));
        } else if node.is_interactive() && node.kind != "Cell" {
            if let Some(label) = &node.label {
                parts.insert(format!("{}:{label}", node.kind));
            }
        }
        if matches!(
            node.kind.as_str(),
            "Alert" | "Sheet" | "NavigationBar" | "TabBar"
        ) {
            parts.insert(node.kind.clone());
        }
    }
    for child in &node.children {
        collect_fingerprint(child, parts);
    }
}

/// A screen's name for people: its navigation bar or alert title.
fn title(tree: &UiNode) -> Option<String> {
    tree.walk()
        .find(|n| matches!(n.kind.as_str(), "Alert" | "NavigationBar") && n.visible)
        .and_then(|n| n.label.clone().or_else(|| n.identifier.clone()))
}

/// The controls on a screen, and those the explorer won't touch.
fn candidates(tree: &UiNode, config: &Config) -> (Vec<Candidate>, Vec<Skipped>) {
    let mut found = Vec::new();
    let mut skipped = Vec::new();
    let mut keys = HashSet::new();
    visit(tree, config, false, &mut found, &mut skipped, &mut keys);
    (found, skipped)
}

fn visit(
    node: &UiNode,
    config: &Config,
    back_button: bool,
    found: &mut Vec<Candidate>,
    skipped: &mut Vec<Skipped>,
    keys: &mut HashSet<String>,
) {
    // Keyboard keys are typing, not navigation.
    if node.kind == "Keyboard" {
        return;
    }
    if node.visible && node.enabled {
        consider(node, config, back_button, found, skipped, keys);
    }
    // A navigation bar's first button is its back button: its label is the
    // previous screen's title, not "Back".
    if node.kind == "NavigationBar" {
        for (i, child) in node.children.iter().enumerate() {
            let first_button = i == 0 && child.kind == "Button";
            visit(child, config, first_button, found, skipped, keys);
        }
        return;
    }
    for child in &node.children {
        visit(child, config, false, found, skipped, keys);
    }
}

fn consider(
    node: &UiNode,
    config: &Config,
    back_button: bool,
    found: &mut Vec<Candidate>,
    skipped: &mut Vec<Skipped>,
    keys: &mut HashSet<String>,
) {
    let kind = node.kind.as_str();
    if kind.contains("Secure") {
        skipped.push(Skipped {
            element: node.summary(),
            reason: "secure field: never typed into".into(),
        });
        return;
    }
    let tappable = TAPPABLE.contains(&kind);
    let typeable = TYPEABLE.contains(&kind);
    if !tappable && !typeable {
        return;
    }
    // An image is only a control if someone made it one.
    if kind == "Image" && node.identifier.is_none() {
        return;
    }
    // Without an identifier or a label there is nothing to find it by
    // except coordinates, which Forge never uses; the audit reports it.
    let (locator, key) = match (&node.identifier, &node.label) {
        (Some(id), _) => (
            Locator::Id(id.clone()),
            format!("{kind}#{}", normalise_identifier(id)),
        ),
        // Rows of a list without identifiers: try one of them.
        (None, Some(_)) if kind == "Cell" => {
            let Some(label) = &node.label else { return };
            (Locator::Label(label.clone()), "Cell".into())
        }
        (None, Some(label)) => (Locator::Label(label.clone()), format!("{kind}:{label}")),
        (None, None) => return,
    };
    if !keys.insert(key.clone()) {
        return;
    }
    if let Some(reason) = gate(node, config) {
        skipped.push(Skipped {
            element: node.summary(),
            reason,
        });
        return;
    }
    let action = if typeable {
        let text = node
            .identifier
            .as_ref()
            .and_then(|id| config.fixtures.get(id))
            .cloned()
            .unwrap_or_else(|| config.default_text.clone());
        Action::Type(locator, text)
    } else {
        Action::Tap(locator)
    };
    let label = node.label.as_deref().unwrap_or_default().to_lowercase();
    let exit = kind == "Button"
        && (node.identifier.as_deref() == Some("BackButton")
            || EXIT_LABELS.contains(&label.as_str())
            || back_button);
    found.push(Candidate { action, key, exit });
}

/// A small, dependency-free PRNG (SplitMix64): the walk only needs a
/// reproducible shuffle.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            items.swap(i, j);
        }
    }
}

/// A problem the explorer found, with the way to see it again.
#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    /// 1-based, in the order found.
    pub number: usize,
    /// `app_closed`, `left_app`, `error_alert`, `action_failed`,
    /// `dead_end`, `clipped`, `no_effect`, or an audit issue (`no_label`,
    /// `no_identifier`, `duplicate_identifier`).
    pub issue: String,
    /// `error`, `warning` or `info`.
    pub severity: String,
    pub message: String,
    /// The screen it was seen on.
    pub screen: String,
    /// The journal step that showed it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<usize>,
    /// The actions from launch that lead to it.
    pub repro: Vec<String>,
    /// The `forge ui run` file that replays it, relative to the output folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repro_file: Option<String>,
    #[serde(skip)]
    actions: Vec<Action>,
}

/// One action of the walk.
#[derive(Debug, Clone, Serialize)]
pub struct StepRecord {
    pub step: usize,
    pub from: String,
    pub action: String,
    /// The screen it led to, `app_closed`, `left_app:<bundle id>`,
    /// `not_found` or `failed`.
    pub outcome: String,
    pub ms: u64,
}

/// What the explorer learnt about one screen.
#[derive(Debug, Clone, Serialize)]
pub struct ScreenSummary {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// How to reach it from launch.
    pub path: Vec<String>,
    pub tried: usize,
    /// Controls never tried because the budget ran out or the per-screen
    /// limit was reached.
    pub untried: usize,
    /// Screens reached from it.
    pub exits: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<Skipped>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub app: String,
    pub seed: u64,
    pub actions: usize,
    pub duration_ms: u64,
    /// `explored` (nothing left to try), `action_budget` or `time_budget`.
    pub stopped: String,
    pub screens: Vec<ScreenSummary>,
    pub issues: Vec<Issue>,
    pub journal: Vec<StepRecord>,
}

impl Report {
    pub fn errors(&self) -> usize {
        self.issues.iter().filter(|i| i.severity == "error").count()
    }
}

struct Screen {
    id: String,
    title: Option<String>,
    path: Vec<Action>,
    untried: Vec<Candidate>,
    tried_keys: HashSet<String>,
    tried: usize,
    over_limit: usize,
    exits: BTreeSet<String>,
    skipped: BTreeSet<Skipped>,
    has_exit: bool,
    screenshot: Option<String>,
}

/// Where an action left the app.
enum Landed {
    Screen(String),
    Closed,
    Other(String),
}

struct Explorer<'a> {
    config: &'a Config,
    driver: &'a mut dyn Driver,
    out_dir: Option<&'a Path>,
    out: &'a mut dyn Write,
    rng: Rng,
    screens: Vec<Screen>,
    index: HashMap<String, usize>,
    issues: Vec<Issue>,
    seen_issues: HashSet<String>,
    journal: Vec<StepRecord>,
    actions: usize,
    root: Option<String>,
    /// The screen in front and its tree, after the last action.
    current: String,
    tree: UiNode,
    /// The actions since the last launch.
    trail: Vec<Action>,
    /// Exit controls used in a row to leave exhausted screens.
    exits_in_row: usize,
}

/// Explores `config.app`. Progress goes to `out`; screenshots, trees,
/// `explore.json` and one replay file per issue go to `out_dir`.
pub fn run(
    config: &Config,
    driver: &mut dyn Driver,
    out_dir: Option<&Path>,
    out: &mut dyn Write,
) -> Result<Report> {
    let started = Instant::now();
    let deadline = started + config.budget;
    if let Some(dir) = out_dir {
        std::fs::create_dir_all(dir.join("screens"))?;
        std::fs::create_dir_all(dir.join("repro"))?;
    }
    let mut explorer = Explorer {
        config,
        driver,
        out_dir,
        out,
        rng: Rng(config.seed),
        screens: Vec::new(),
        index: HashMap::new(),
        issues: Vec::new(),
        seen_issues: HashSet::new(),
        journal: Vec::new(),
        actions: 0,
        root: None,
        current: String::new(),
        tree: UiNode::default(),
        trail: Vec::new(),
        exits_in_row: 0,
    };
    writeln!(
        explorer.out,
        "Exploring {} (seed {}, at most {} actions, {} s)",
        config.app,
        config.seed,
        config.max_actions,
        config.budget.as_secs()
    )?;
    explorer.relaunch()?;
    explorer.root = Some(explorer.current.clone());
    let stopped = loop {
        if explorer.actions >= config.max_actions {
            break "action_budget";
        }
        if Instant::now() >= deadline {
            break "time_budget";
        }
        if !explorer.step()? {
            break "explored";
        }
    };
    explorer.finish(stopped, started)
}

impl Explorer<'_> {
    /// Does one thing; false when nothing reachable is left to try.
    fn step(&mut self) -> Result<bool> {
        let here = self.index[&self.current];
        if let Some(candidate) = self.next_untried(here) {
            self.screens[here].tried += 1;
            self.screens[here].tried_keys.insert(candidate.key.clone());
            self.exits_in_row = 0;
            self.perform(candidate.action, true)?;
            return Ok(true);
        }
        // Nothing left here. Is anything left anywhere?
        let Some(target) = self.nearest_frontier() else {
            return Ok(false);
        };
        // Leave through a back or dismiss control if the screen has one,
        // twice in a row at most (two exhausted screens can point at each
        // other); otherwise relaunch and replay the way to the target.
        match self.exit_action() {
            Some(action) if Some(&self.current) != self.root.as_ref() && self.exits_in_row < 2 => {
                self.exits_in_row += 1;
                self.perform(action, false)?;
            }
            _ => {
                self.exits_in_row = 0;
                self.replay(target)?;
            }
        }
        Ok(true)
    }

    /// The next control to try on screen `here`. Controls that appeared
    /// since the first visit (a button enabled by typing) are added.
    fn next_untried(&mut self, here: usize) -> Option<Candidate> {
        let (fresh, skipped) = candidates(&self.tree, self.config);
        let screen = &mut self.screens[here];
        screen.skipped.extend(skipped);
        for candidate in fresh {
            let known = screen.tried_keys.contains(&candidate.key)
                || screen.untried.iter().any(|c| c.key == candidate.key);
            if !known && screen.tried + screen.untried.len() < self.config.per_screen {
                screen.has_exit |= candidate.exit;
                // New controls go in front of the exits.
                let at = screen.untried.iter().position(|c| c.exit);
                match at {
                    Some(at) if !candidate.exit => screen.untried.insert(at, candidate),
                    _ => screen.untried.push(candidate),
                }
            }
        }
        if screen.untried.is_empty() {
            return None;
        }
        Some(screen.untried.remove(0))
    }

    /// The back or dismiss control of the screen in front.
    fn exit_action(&self) -> Option<Action> {
        let (found, _) = candidates(&self.tree, self.config);
        found.into_iter().find(|c| c.exit).map(|c| c.action)
    }

    /// The screen with something left to try that is quickest to reach.
    fn nearest_frontier(&self) -> Option<usize> {
        self.screens
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.untried.is_empty())
            .min_by_key(|(_, s)| s.path.len())
            .map(|(i, _)| i)
    }

    /// Relaunches and replays the path to screen `target`. If the path no
    /// longer leads there, the screen's remaining controls are dropped so
    /// the walk can't loop on it.
    fn replay(&mut self, target: usize) -> Result<()> {
        // Every path starts at the launch screen: no need to restart when
        // already there.
        if Some(&self.current) != self.root.as_ref() {
            self.relaunch()?;
        } else {
            self.trail.clear();
        }
        let path = self.screens[target].path.clone();
        for action in path {
            if self.actions >= self.config.max_actions {
                return Ok(());
            }
            self.perform(action, false)?;
        }
        if self.current != self.screens[target].id {
            let dropped = self.screens[target].untried.len();
            self.screens[target].over_limit += dropped;
            self.screens[target].untried.clear();
            writeln!(
                self.out,
                "          could not get back to {}; leaving its {dropped} remaining controls",
                self.screens[target].id
            )?;
        }
        Ok(())
    }

    fn relaunch(&mut self) -> Result<()> {
        self.trail.clear();
        // Stop it first so launch arguments always apply.
        let _ = self.driver.terminate(&self.config.app);
        self.perform(Action::Launch, false)
    }

    /// Performs `action`, records it, and looks at where it led.
    fn perform(&mut self, action: Action, exploring: bool) -> Result<()> {
        self.actions += 1;
        let step = self.actions;
        let from = self.current.clone();
        let before = self.tree.clone();
        let started = Instant::now();
        let result = match &action {
            Action::Launch => self
                .driver
                .launch(&self.config.app, &self.config.launch_args),
            Action::Tap(locator) => self.on_element(locator, &mut |d, e| d.tap(e)),
            Action::Type(locator, text) => {
                self.on_element(locator, &mut |d, e| d.type_text(e, text))
            }
        };
        let mut outcome = match result {
            Ok(()) => None,
            Err(UiError::NotFound(_)) => Some("not_found".to_owned()),
            Err(UiError::Wda(why)) => {
                let path = self.path_with(&action);
                self.report(
                    "action_failed",
                    "warning",
                    format!("{} failed: {why}", action.describe()),
                    &from,
                    Some(step),
                    path,
                );
                Some("failed".to_owned())
            }
            Err(other) => return Err(other),
        };
        if !matches!(action, Action::Launch) && outcome.as_deref() != Some("not_found") {
            self.trail.push(action.clone());
        }
        let landed = self.observe(step)?;
        let ms = started.elapsed().as_millis() as u64;
        let mut recover = None;
        match &landed {
            Landed::Screen(id) => {
                if exploring && *id != from {
                    if let Some(&i) = self.index.get(&from) {
                        self.screens[i].exits.insert(id.clone());
                    }
                }
                if exploring
                    && *id == from
                    && outcome.is_none()
                    && self.tree == before
                    && is_plain_tap(&action, &before)
                {
                    let path = self.trail.clone();
                    self.report(
                        "no_effect",
                        "info",
                        format!("{} changed nothing on screen", action.describe()),
                        &from,
                        Some(step),
                        path,
                    );
                }
                outcome.get_or_insert_with(|| id.clone());
            }
            Landed::Closed => {
                let path = self.trail.clone();
                self.report(
                    "app_closed",
                    "error",
                    format!(
                        "the app closed after {} (a crash or an exit; see `forge device crashes`)",
                        action.describe()
                    ),
                    &from,
                    Some(step),
                    path,
                );
                outcome = Some("app_closed".into());
                recover = Some(None);
            }
            Landed::Other(bundle) => {
                let path = self.trail.clone();
                self.report(
                    "left_app",
                    "warning",
                    format!("{} opened {bundle}", action.describe()),
                    &from,
                    Some(step),
                    path,
                );
                outcome = Some(format!("left_app:{bundle}"));
                recover = Some(Some(bundle.clone()));
                // Coming back lands where the action started, so later
                // replays leave it out (it would stop at the other app).
                self.trail.pop();
            }
        }
        let outcome = outcome.unwrap_or_default();
        writeln!(
            self.out,
            "  {step:>3}. {:<10} {} -> {outcome} ({:.1} s)",
            if from.is_empty() { "-" } else { &from },
            action.describe(),
            ms as f64 / 1000.0
        )?;
        self.journal.push(StepRecord {
            step,
            from,
            action: action.describe(),
            outcome,
            ms,
        });
        match recover {
            None => Ok(()),
            // The app closed: start it again.
            Some(None) if matches!(action, Action::Launch) => Err(UiError::Device(format!(
                "{} closed right after launch; nothing to explore (see `forge device crashes`)",
                self.config.app
            ))),
            Some(None) => self.relaunch(),
            // Another app came to the front: come back without restarting,
            // or restart if that fails.
            Some(Some(_)) => {
                let back = self.driver.activate(&self.config.app).is_ok()
                    && matches!(self.observe(step)?, Landed::Screen(_));
                if back {
                    Ok(())
                } else {
                    self.relaunch()
                }
            }
        }
    }

    /// Finds the element and acts on it, finding it again once if the
    /// screen re-rendered in between.
    fn on_element(
        &mut self,
        locator: &Locator,
        act: &mut dyn FnMut(&mut dyn Driver, &crate::ElementRef) -> Result<()>,
    ) -> Result<()> {
        let element = wait_for(self.driver, locator, self.config.find_timeout)?;
        match act(self.driver, &element) {
            Err(UiError::Stale(_)) => {
                let element = wait_for(self.driver, locator, self.config.find_timeout)?;
                match act(self.driver, &element) {
                    // A tap that takes its own element away is fine.
                    Err(UiError::Stale(_)) => Ok(()),
                    other => other,
                }
            }
            other => other,
        }
    }

    /// Reads the screen after an action: which app is in front, which
    /// screen this is, and what is wrong with it if it is new.
    fn observe(&mut self, step: usize) -> Result<Landed> {
        // The tree first: WebDriverAgent waits for the app to be idle, by
        // which time a crash has taken the app off the screen.
        let tree = self.driver.tree()?;
        let front = self.driver.foreground()?;
        match front.as_deref() {
            Some(app) if app == self.config.app => {}
            Some(other) if other != crate::fake::SPRINGBOARD => {
                return Ok(Landed::Other(other.to_owned()))
            }
            _ => return Ok(Landed::Closed),
        }
        let id = fingerprint(&tree);
        self.tree = tree;
        self.current = id.clone();
        if !self.index.contains_key(&id) {
            self.discover(&id, step)?;
        } else {
            // Keep the shortest known way here.
            let i = self.index[&id];
            if self.trail.len() < self.screens[i].path.len() {
                self.screens[i].path = self.trail.clone();
            }
        }
        self.check_alerts(&id, step);
        Ok(Landed::Screen(id))
    }

    fn discover(&mut self, id: &str, step: usize) -> Result<()> {
        let (mut found, skipped) = candidates(&self.tree, self.config);
        // Seeded order, exits last.
        let mut exits: Vec<Candidate> = Vec::new();
        found.retain(|c| {
            if c.exit {
                exits.push(c.clone());
                false
            } else {
                true
            }
        });
        self.rng.shuffle(&mut found);
        let has_exit = !exits.is_empty();
        found.extend(exits);
        let over_limit = found.len().saturating_sub(self.config.per_screen);
        found.truncate(self.config.per_screen);
        let mut screenshot = None;
        if let (Some(dir), true) = (self.out_dir, self.config.screenshots) {
            let tree_path = dir.join("screens").join(format!("{id}.json"));
            std::fs::write(
                &tree_path,
                serde_json::to_vec_pretty(&self.tree).map_err(std::io::Error::other)?,
            )?;
            if let Ok(png) = self.driver.screenshot() {
                let name = format!("screens/{id}.png");
                std::fs::write(dir.join(&name), png)?;
                screenshot = Some(name);
            }
        }
        writeln!(
            self.out,
            "          new screen {id}{}: {} controls to try",
            title(&self.tree)
                .map(|t| format!(" \"{t}\""))
                .unwrap_or_default(),
            found.len()
        )?;
        self.index.insert(id.to_owned(), self.screens.len());
        self.screens.push(Screen {
            id: id.to_owned(),
            title: title(&self.tree),
            path: self.trail.clone(),
            untried: found,
            tried_keys: HashSet::new(),
            tried: 0,
            over_limit,
            exits: BTreeSet::new(),
            skipped: skipped.into_iter().collect(),
            has_exit,
            screenshot,
        });
        self.check_screen(id, step);
        Ok(())
    }

    /// The checks every new screen gets: Forge's accessibility audit, and
    /// controls pushed off the side of the screen.
    fn check_screen(&mut self, id: &str, step: usize) {
        let tree = self.tree.clone();
        for finding in tree.audit() {
            let path = self.trail.clone();
            self.report(
                finding.issue,
                finding.severity,
                finding.to_string(),
                id,
                Some(step),
                path,
            );
        }
        let width = tree.frame.map(|f| f.width).filter(|w| *w > 0.0);
        if let Some(width) = width {
            for node in tree.walk().filter(|n| {
                n.visible && (n.is_interactive() || n.kind == "StaticText") && n.label.is_some()
            }) {
                let Some(frame) = node.frame else { continue };
                if frame.width > 0.0 && (frame.x < -1.0 || frame.x + frame.width > width + 1.0) {
                    let path = self.trail.clone();
                    self.report(
                        "clipped",
                        "warning",
                        format!("runs off the side of the screen: {}", node.summary()),
                        id,
                        Some(step),
                        path,
                    );
                }
            }
        }
    }

    /// Alerts that say something went wrong. Seen on every visit, reported
    /// once per text.
    fn check_alerts(&mut self, id: &str, step: usize) {
        const ERROR_WORDS: &[&str] = &[
            "error", "failed", "failure", "couldn", "unable", "invalid", "wrong", "problem",
        ];
        let alerts: Vec<String> = self
            .tree
            .walk()
            .filter(|n| n.kind == "Alert" && n.visible)
            .map(|alert| {
                alert
                    .walk()
                    .filter_map(|n| n.label.as_deref())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        for text in alerts {
            let lower = text.to_lowercase();
            if ERROR_WORDS.iter().any(|w| lower.contains(w)) {
                let path = self.trail.clone();
                self.report(
                    "error_alert",
                    "warning",
                    format!("an alert reports a problem: \"{text}\""),
                    id,
                    Some(step),
                    path,
                );
            }
        }
    }

    fn path_with(&self, action: &Action) -> Vec<Action> {
        let mut path = self.trail.clone();
        path.push(action.clone());
        path
    }

    /// Records an issue once: the same issue on the same element (the
    /// message) is reported where it was first seen.
    fn report(
        &mut self,
        issue: &str,
        severity: &str,
        message: String,
        screen: &str,
        step: Option<usize>,
        actions: Vec<Action>,
    ) {
        let key = format!("{issue}\u{0}{message}");
        if !self.seen_issues.insert(key) {
            return;
        }
        let number = self.issues.len() + 1;
        let _ = writeln!(self.out, "          {severity}: {message}");
        self.issues.push(Issue {
            number,
            issue: issue.to_owned(),
            severity: severity.to_owned(),
            message,
            screen: screen.to_owned(),
            step,
            repro: actions.iter().map(Action::describe).collect(),
            repro_file: None,
            actions,
        });
    }

    fn finish(mut self, stopped: &str, started: Instant) -> Result<Report> {
        // A screen with no way out but relaunching: nothing tried on it led
        // anywhere and it has no back or dismiss control.
        let dead_ends: Vec<(String, Vec<Action>)> = self
            .screens
            .iter()
            .filter(|s| {
                Some(&s.id) != self.root.as_ref()
                    && s.untried.is_empty()
                    && s.exits.is_empty()
                    && !s.has_exit
            })
            .map(|s| (s.id.clone(), s.path.clone()))
            .collect();
        for (id, path) in dead_ends {
            self.report(
                "dead_end",
                "warning",
                format!("no way out of screen {id} except relaunching the app"),
                &id,
                None,
                path,
            );
        }
        if let Some(dir) = self.out_dir {
            for issue in &mut self.issues {
                let name = format!("repro/issue-{}-{}.toml", issue.number, issue.issue);
                std::fs::write(
                    dir.join(&name),
                    repro_file(self.config, issue, &issue.actions),
                )?;
                issue.repro_file = Some(name);
            }
        }
        let screens = self
            .screens
            .iter()
            .map(|s| ScreenSummary {
                id: s.id.clone(),
                title: s.title.clone(),
                path: s.path.iter().map(Action::describe).collect(),
                tried: s.tried,
                untried: s.untried.len() + s.over_limit,
                exits: s.exits.iter().cloned().collect(),
                skipped: s.skipped.iter().cloned().collect(),
                screenshot: s.screenshot.clone(),
            })
            .collect();
        let report = Report {
            app: self.config.app.clone(),
            seed: self.config.seed,
            actions: self.actions,
            duration_ms: started.elapsed().as_millis() as u64,
            stopped: stopped.to_owned(),
            screens,
            issues: self.issues,
            journal: self.journal,
        };
        if let Some(dir) = self.out_dir {
            std::fs::write(
                dir.join("explore.json"),
                serde_json::to_vec_pretty(&report).map_err(std::io::Error::other)?,
            )?;
        }
        let count = |s: &str| report.issues.iter().filter(|i| i.severity == s).count();
        writeln!(
            self.out,
            "{} screens, {} actions, stopped: {stopped}. Issues: {} errors, {} warnings, {} notes",
            report.screens.len(),
            report.actions,
            count("error"),
            count("warning"),
            count("info"),
        )?;
        Ok(report)
    }
}

/// Taps that should change something: buttons and links. Switches, tabs
/// and text fields legitimately leave the tree as it was.
fn is_plain_tap(action: &Action, tree: &UiNode) -> bool {
    let Action::Tap(locator) = action else {
        return false;
    };
    tree.walk()
        .find(|n| locator.matches(n))
        .is_some_and(|n| matches!(n.kind.as_str(), "Button" | "Link" | "MenuItem"))
}

/// A `forge ui run` file that replays an issue: launch, the actions that
/// led to it, and a screenshot of the result.
fn repro_file(config: &Config, issue: &Issue, actions: &[Action]) -> String {
    let mut text = format!(
        "# Found by `forge ui explore` (seed {}), issue {}: {}\n# {}\n\n",
        config.seed, issue.number, issue.issue, issue.message
    );
    text.push_str(&format!(
        "name = {}\napp = {}\n",
        quote(&format!("Explore issue {}: {}", issue.number, issue.issue)),
        quote(&config.script_app)
    ));
    let mut steps = vec![Action::Launch];
    steps.extend(actions.iter().cloned());
    for action in steps {
        text.push_str(&format!(
            "\n[[step]]\n{}\n",
            action.toml_step(&config.launch_args)
        ));
    }
    text.push_str(&format!(
        "\n[[step]]\nscreenshot = \"issue-{}\"\n",
        issue.number
    ));
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Rect;
    use crate::ElementRef;

    const APP: &str = "org.forgemobile.pocketlibrary";

    fn node(kind: &str, id: Option<&str>, label: Option<&str>) -> UiNode {
        UiNode {
            kind: kind.into(),
            identifier: id.map(Into::into),
            label: label.map(Into::into),
            enabled: true,
            visible: true,
            ..UiNode::default()
        }
    }

    fn app(children: Vec<UiNode>) -> UiNode {
        UiNode {
            kind: "Application".into(),
            label: Some("Pocket Library".into()),
            visible: false,
            enabled: true,
            frame: Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 430.0,
                height: 932.0,
            }),
            children,
            ..UiNode::default()
        }
    }

    fn nav(title: &str, buttons: Vec<UiNode>) -> UiNode {
        UiNode {
            children: buttons,
            ..node("NavigationBar", None, Some(title))
        }
    }

    /// Where a tap leads in the model app.
    #[derive(Clone)]
    enum To {
        Screen(&'static str),
        Crash,
        OtherApp(&'static str),
    }

    /// A small app as a graph of screens, driven like the phone.
    struct ModelApp {
        screens: HashMap<&'static str, UiNode>,
        taps: HashMap<(&'static str, &'static str), To>,
        current: &'static str,
        front: Option<String>,
        log: Vec<String>,
    }

    impl ModelApp {
        fn pocket_library() -> Self {
            let mut wide = node("StaticText", None, Some("A very long title"));
            wide.frame = Some(Rect {
                x: 20.0,
                y: 100.0,
                width: 600.0,
                height: 20.0,
            });
            let screens = HashMap::from([
                (
                    "library",
                    app(vec![
                        nav(
                            "Library",
                            vec![node("Button", Some("library.add"), Some("Add Book"))],
                        ),
                        node("Cell", Some("library.book.1"), Some("Dune")),
                        node("Cell", Some("library.book.2"), Some("Emma")),
                        node("Button", Some("library.settings"), Some("Settings")),
                        node("Button", Some("library.deleteAll"), Some("Delete All")),
                        node("Button", Some("library.share"), Some("Share List")),
                        node("Button", None, None),
                    ]),
                ),
                (
                    "form",
                    app(vec![
                        node("TextField", Some("bookForm.title"), Some("Title")),
                        node("SecureTextField", Some("bookForm.pin"), Some("PIN")),
                        node("Button", Some("bookForm.cancel"), Some("Cancel")),
                        node("Button", Some("bookForm.save"), Some("Save")),
                    ]),
                ),
                (
                    "detail",
                    app(vec![
                        nav("Dune", vec![node("Button", None, Some("Library"))]),
                        node("Button", Some("detail.read"), Some("Read")),
                        node("Button", Some("detail.sync"), Some("Sync")),
                        node("Link", Some("detail.web"), Some("Website")),
                        wide,
                    ]),
                ),
                (
                    "settings",
                    app(vec![
                        nav("Settings", vec![]),
                        node("Button", Some("settings.refresh"), Some("Refresh")),
                    ]),
                ),
                (
                    "alert",
                    app(vec![UiNode {
                        children: vec![
                            node("StaticText", None, Some("Sync failed")),
                            node("Button", None, Some("OK")),
                        ],
                        ..node("Alert", None, Some("Error"))
                    }]),
                ),
            ]);
            let taps = HashMap::from([
                (("library", "library.add"), To::Screen("form")),
                (("library", "library.book.1"), To::Screen("detail")),
                (("library", "library.book.2"), To::Screen("detail")),
                (("library", "library.settings"), To::Screen("settings")),
                (("form", "bookForm.cancel"), To::Screen("library")),
                (("form", "bookForm.save"), To::Screen("library")),
                (("detail", "Library"), To::Screen("library")),
                (("detail", "detail.read"), To::Crash),
                (("detail", "detail.sync"), To::Screen("alert")),
                (
                    ("detail", "detail.web"),
                    To::OtherApp("com.apple.mobilesafari"),
                ),
                (("alert", "OK"), To::Screen("detail")),
            ]);
            Self {
                screens,
                taps,
                current: "library",
                front: None,
                log: Vec::new(),
            }
        }

        fn screen(&mut self) -> &mut UiNode {
            self.screens.get_mut(self.current).expect("screen")
        }
    }

    impl Driver for ModelApp {
        fn launch(&mut self, bundle_id: &str, args: &[String]) -> Result<()> {
            self.log.push(format!("launch {args:?}"));
            self.front = Some(bundle_id.to_owned());
            self.current = "library";
            Ok(())
        }
        fn activate(&mut self, bundle_id: &str) -> Result<()> {
            self.log.push("activate".into());
            self.front = Some(bundle_id.to_owned());
            Ok(())
        }
        fn terminate(&mut self, _: &str) -> Result<()> {
            self.front = Some(crate::fake::SPRINGBOARD.into());
            Ok(())
        }
        fn background(&mut self, _: f64) -> Result<()> {
            Ok(())
        }
        fn scroll(&mut self, _: &ElementRef, _: &str) -> Result<()> {
            Ok(())
        }
        fn find_all(&mut self, locator: &Locator) -> Result<Vec<ElementRef>> {
            Ok(self
                .screen()
                .walk()
                .enumerate()
                .filter(|(_, n)| locator.matches(n))
                .map(|(i, _)| ElementRef(i.to_string()))
                .collect())
        }
        fn tap(&mut self, element: &ElementRef) -> Result<()> {
            let index: usize = element.0.parse().unwrap();
            let current = self.current;
            let target = self.screen().nth_mut(index).expect("node").clone();
            let key = target.identifier.or(target.label).unwrap_or_default();
            self.log.push(format!("tap {key}"));
            let to = self
                .taps
                .iter()
                .find(|((screen, k), _)| *screen == current && *k == key)
                .map(|(_, to)| to.clone());
            match to {
                Some(To::Screen(next)) => self.current = next,
                Some(To::Crash) => self.front = Some(crate::fake::SPRINGBOARD.into()),
                Some(To::OtherApp(app)) => self.front = Some(app.into()),
                None => {}
            }
            Ok(())
        }
        fn type_text(&mut self, element: &ElementRef, text: &str) -> Result<()> {
            let index: usize = element.0.parse().unwrap();
            self.log.push(format!("type {text}"));
            let node = self.screen().nth_mut(index).expect("node");
            node.value = Some(text.into());
            Ok(())
        }
        fn clear(&mut self, _: &ElementRef) -> Result<()> {
            Ok(())
        }
        fn attribute(&mut self, _: &ElementRef, _: &str) -> Result<Option<String>> {
            Ok(None)
        }
        fn tree(&mut self) -> Result<UiNode> {
            Ok(self.screen().clone())
        }
        fn screenshot(&mut self) -> Result<Vec<u8>> {
            Ok(b"\x89PNG".to_vec())
        }
        fn foreground(&mut self) -> Result<Option<String>> {
            Ok(self.front.clone())
        }
    }

    fn config() -> Config {
        Config {
            app: APP.into(),
            script_app: APP.into(),
            max_actions: 200,
            find_timeout: Duration::ZERO,
            fixtures: HashMap::from([("bookForm.title".into(), "Dune".into())]),
            test_data: true,
            ..Config::default()
        }
    }

    #[test]
    fn on_the_persons_own_data_it_only_looks_around() {
        let config = Config {
            test_data: false,
            allow_destructive: true,
            ..config()
        };
        let (report, model) = explore(&config, None);
        // No saving, adding, typing or deleting; reading the library,
        // opening a book and going back are fine.
        for touched in &model.log {
            assert!(
                ![
                    "tap library.add",
                    "type",
                    "tap library.deleteAll",
                    "tap bookForm"
                ]
                .iter()
                .any(|f| touched.starts_with(f)),
                "{touched} in {:?}",
                model.log
            );
        }
        assert!(model.log.iter().any(|l| l.starts_with("tap library.book.")));
        let reasons: Vec<_> = report
            .screens
            .iter()
            .flat_map(|s| &s.skipped)
            .map(|s| s.reason.as_str())
            .collect();
        assert!(reasons
            .iter()
            .any(|r| r.starts_with("may change data (\"add\")")));
        assert!(reasons.iter().any(|r| r.starts_with("destructive; never")));
    }

    #[test]
    fn ratings_status_and_reading_count_as_data_changes() {
        let config = Config::default();
        for (id, label) in [
            ("star.fill", "Rate 5 stars"),
            ("checkmark.circle", "Mark as finished"),
            ("library.continue", "Continue Reading"),
            ("bookForm.save", "Save"),
        ] {
            assert!(
                gate(&node("Button", Some(id), Some(label)), &config).is_some(),
                "{id}"
            );
        }
        let switch = node(
            "Switch",
            Some("settings.catalogue"),
            Some("Online catalogue search"),
        );
        assert!(gate(&switch, &config).is_some());
        let search = node(
            "SearchField",
            Some("library.search"),
            Some("Title or author"),
        );
        assert!(gate(&search, &config).is_none());
        assert!(gate(&node("Cell", Some("library.book.1"), Some("Dune")), &config).is_none());
    }

    fn explore(config: &Config, dir: Option<&Path>) -> (Report, ModelApp) {
        let mut model = ModelApp::pocket_library();
        let report = run(config, &mut model, dir, &mut Vec::new()).unwrap();
        (report, model)
    }

    fn issue<'a>(report: &'a Report, kind: &str) -> &'a Issue {
        report
            .issues
            .iter()
            .find(|i| i.issue == kind)
            .unwrap_or_else(|| panic!("no {kind} in {:#?}", report.issues))
    }

    #[test]
    fn visits_every_screen_and_stops_when_done() {
        let (report, _) = explore(&config(), None);
        assert_eq!(report.stopped, "explored");
        let titles: BTreeSet<_> = report
            .screens
            .iter()
            .map(|s| s.title.clone().unwrap_or_default())
            .collect();
        assert_eq!(
            titles,
            BTreeSet::from([
                "".to_owned(),
                "Dune".into(),
                "Error".into(),
                "Library".into(),
                "Settings".into()
            ])
        );
        assert!(report.screens.iter().all(|s| s.untried == 0));
    }

    #[test]
    fn finds_crashes_alerts_dead_ends_and_audit_issues() {
        let (report, _) = explore(&config(), None);
        let crash = issue(&report, "app_closed");
        assert_eq!(crash.severity, "error");
        assert_eq!(
            crash.repro.last().map(String::as_str),
            Some("tap id `detail.read`")
        );
        assert!(crash.repro[0].starts_with("tap id `library.book."));
        // Opening Safari came back to the same screen: not part of the way there.
        assert!(!crash.repro.iter().any(|a| a.contains("detail.web")));
        assert!(issue(&report, "error_alert")
            .message
            .contains("Sync failed"));
        assert!(issue(&report, "left_app")
            .message
            .contains("com.apple.mobilesafari"));
        assert!(issue(&report, "dead_end").message.contains("screen"));
        assert_eq!(issue(&report, "no_label").severity, "error");
        assert!(issue(&report, "no_effect")
            .message
            .contains("settings.refresh"));
        assert!(issue(&report, "clipped")
            .message
            .contains("A very long title"));
        assert_eq!(report.errors(), 2);
    }

    #[test]
    fn never_touches_external_destructive_or_secure_controls() {
        let (report, model) = explore(&config(), None);
        for forbidden in ["tap library.deleteAll", "tap library.share", "type"] {
            assert!(
                !model
                    .log
                    .iter()
                    .any(|l| l.starts_with(forbidden) && !l.ends_with("Dune")),
                "{forbidden} in {:?}",
                model.log
            );
        }
        // Typing used the fixture.
        assert!(model.log.contains(&"type Dune".to_owned()));
        let skipped: Vec<_> = report
            .screens
            .iter()
            .flat_map(|s| &s.skipped)
            .map(|s| s.reason.as_str())
            .collect();
        assert!(skipped.iter().any(|r| r.starts_with("destructive")));
        assert!(skipped.iter().any(|r| r.starts_with("acts outside")));
        assert!(skipped.iter().any(|r| r.starts_with("secure field")));
        // Rows of a list count once.
        let books = model
            .log
            .iter()
            .filter(|l| l.starts_with("tap library.book."))
            .collect::<BTreeSet<_>>();
        assert_eq!(books.len(), 1, "{books:?}");
    }

    #[test]
    fn destructive_controls_can_be_allowed_but_external_ones_never() {
        let config = Config {
            allow_destructive: true,
            ..config()
        };
        let (_, model) = explore(&config, None);
        assert!(model.log.contains(&"tap library.deleteAll".to_owned()));
        assert!(!model.log.contains(&"tap library.share".to_owned()));
    }

    #[test]
    fn the_same_seed_walks_the_same_way() {
        let walk = |seed| {
            let config = Config { seed, ..config() };
            explore(&config, None)
                .0
                .journal
                .iter()
                .map(|s| format!("{} {}", s.action, s.outcome))
                .collect::<Vec<_>>()
        };
        assert_eq!(walk(7), walk(7));
    }

    #[test]
    fn stops_at_the_action_budget() {
        let config = Config {
            max_actions: 3,
            ..config()
        };
        let (report, _) = explore(&config, None);
        assert_eq!(report.stopped, "action_budget");
        assert_eq!(report.actions, 3);
        assert_eq!(report.journal.len(), 3);
    }

    #[test]
    fn stops_at_the_time_budget() {
        let config = Config {
            budget: Duration::ZERO,
            ..config()
        };
        let (report, _) = explore(&config, None);
        assert_eq!(report.stopped, "time_budget");
    }

    #[test]
    fn writes_a_report_and_replay_files_that_forge_ui_run_accepts() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            launch_args: LARGEST_TEXT_ARGS.iter().map(|a| a.to_string()).collect(),
            ..config()
        };
        let (report, _) = explore(&config, Some(dir.path()));
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("explore.json")).unwrap())
                .unwrap();
        assert_eq!(json["stopped"], "explored");
        let crash = issue(&report, "app_closed");
        let file = dir.path().join(crash.repro_file.as_ref().unwrap());
        let script = crate::script::Script::load(&file).unwrap();
        assert_eq!(script.app, APP);
        let steps: Vec<String> = script.steps.iter().map(|s| s.describe()).collect();
        assert!(steps[0].starts_with("launch"), "{steps:?}");
        assert!(steps.iter().any(|s| s.contains("detail.read")), "{steps:?}");
        assert_eq!(
            script.steps[0].launch.as_ref().unwrap().args,
            LARGEST_TEXT_ARGS
        );
        // Every new screen has its tree and a screenshot.
        for screen in &report.screens {
            assert!(dir
                .path()
                .join(screen.screenshot.as_ref().unwrap())
                .is_file());
            assert!(dir
                .path()
                .join(format!("screens/{}.json", screen.id))
                .is_file());
        }
    }

    #[test]
    fn identifiers_that_name_data_are_one_control() {
        assert_eq!(normalise_identifier("library.book.42"), "library.book.*");
        assert_eq!(
            normalise_identifier("room.3f2a9c1e-77aa-4c1b.open"),
            "room.*.open"
        );
        assert_eq!(normalise_identifier("library.add"), "library.add");
        assert_eq!(normalise_identifier("deadbeef"), "deadbeef");
    }

    #[test]
    fn a_screen_is_its_structure_not_its_data() {
        let mut a = ModelApp::pocket_library().screens["detail"].clone();
        let b = a.clone();
        a.children[0].label = Some("Emma".into());
        a.children[1].value = Some("42%".into());
        assert_eq!(fingerprint(&a), fingerprint(&b));
        a.children
            .push(node("Button", Some("detail.more"), Some("More")));
        assert_ne!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn words_split_identifiers_and_labels() {
        assert_eq!(words("library.deleteAll"), ["library", "delete", "all"]);
        assert_eq!(words("Sign Out"), ["sign", "out"]);
        let config = Config {
            test_data: true,
            ..Config::default()
        };
        let gated = |id: &str, label: &str| gate(&node("Button", Some(id), Some(label)), &config);
        assert!(gated("account.signOut", "Sign Out").is_some());
        assert!(gated("chat.send", "Send").is_some());
        // Tipster's ids are not payments.
        assert!(gated("tip.amount", "Bill amount").is_none());
        assert!(gated("library.add", "Add Book").is_none());
        // Whole words only: "Settings" is not "set", "Postcode" is not "post".
        assert!(gated("form.postcode", "Postcode").is_none());
    }
}
