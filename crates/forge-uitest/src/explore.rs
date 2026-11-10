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

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::focus::{Focus, Route, Strength};
use crate::policy::{self, Gate};
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
    /// On the person's own data (`test_data` false), identifiers the app's
    /// developer declared safe to tap besides navigation (`[ui_test]
    /// real_data_safe` in forge.toml): a button that opens a sheet, say.
    /// The never-lists (system services, outside the phone) still apply.
    pub real_data_safe: Vec<String>,
    /// Identifiers or labels never to touch.
    pub skip: Vec<String>,
    /// At most this many controls tried per screen.
    pub per_screen: usize,
    /// How long to wait for a control before giving up on it.
    pub find_timeout: Duration,
    /// Save a screenshot and the tree of every new screen.
    pub screenshots: bool,
    /// What the walk is aimed at (M6.2): tried first, reached by known
    /// routes, and reported on. Empty: the plain seeded walk.
    pub focus: Focus,
    /// After the walk, replay up to this many issues once more to see
    /// whether they reproduce (errors first, then warnings on the focus).
    /// These replays are not counted in `max_actions`.
    pub verify: usize,
    /// When a screen has nothing left to try, scroll its list down at most
    /// this many times to reach controls below the fold (0: never scroll).
    pub scrolls_per_screen: usize,
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
            real_data_safe: Vec::new(),
            skip: Vec::new(),
            per_screen: 15,
            find_timeout: Duration::from_secs(5),
            screenshots: true,
            focus: Focus::default(),
            verify: 0,
            scrolls_per_screen: 3,
        }
    }
}

/// UIKit's own launch arguments for the largest accessibility text size,
/// honoured by any app launched through XCTest.
pub const LARGEST_TEXT_ARGS: [&str; 2] = [
    "-UIPreferredContentSizeCategoryName",
    "UICTContentSizeCategoryAccessibilityXXXL",
];

/// Forge's own launch argument for dark or light mode (`-ForgeAppearance
/// dark`), honoured by Forge apps' root view (`forgeLaunchAppearance()`).
/// UIKit has no launch argument for it.
pub const APPEARANCE_ARG: &str = "-ForgeAppearance";

/// One thing the explorer does to the app.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Action {
    Launch,
    Tap(Locator),
    Type(Locator, String),
    /// Scroll a list or scroll view (`down`, `up`…) to reach controls below
    /// the fold.
    Scroll(Locator, String),
}

impl Action {
    pub fn describe(&self) -> String {
        match self {
            Action::Launch => "launch".into(),
            Action::Tap(locator) => format!("tap {locator}"),
            Action::Type(locator, text) => format!("type {text:?} into {locator}"),
            Action::Scroll(locator, direction) => format!("scroll {locator} {direction}"),
        }
    }

    /// The action as a `forge ui run` step.
    pub fn toml_step(&self, launch_args: &[String]) -> String {
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
            Action::Scroll(locator, direction) => format!(
                "scroll = {{ {}, direction = {} }}",
                target(locator),
                quote(direction)
            ),
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
    /// How strongly it is tied to the change being explored.
    strength: Strength,
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

/// Words that mark a control starting a system service the walk must never
/// start: the microphone (dictation turned on at Cati, and WebDriverAgent
/// then dropped the connection), the camera, the keyboard switcher, and
/// the system file picker, which is another app's UI. Matched in every mode.
const SYSTEM: &[&str] = &[
    "dictation",
    "dictate",
    "microphone",
    "mic",
    "camera",
    "globe",
    "globekey",
    "emoji",
    "import",
];

/// Two-word forms of the above.
const SYSTEM_PAIRS: &[(&str, &str)] = &[("next", "keyboard")];

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

/// Where a control sits, for the real-data policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Place {
    /// The first button of a navigation bar: the back button.
    back_button: bool,
    /// Inside a tab bar.
    tab_bar: bool,
}

/// Labels that leave a screen without saving: the only buttons outside
/// navigation that the explorer taps on the person's own data. "Done" and
/// "OK" are left out: they can confirm (an alert's OK after "Remove all?").
const LEAVE_LABELS: &[&str] = &["back", "cancel", "close", "dismiss"];

/// On the person's own data, only navigation is tapped. This is an allow
/// list, so a control the word lists don't know is skipped, not tapped:
/// list rows, tab bar items, the back button, Back/Cancel/Close/Dismiss,
/// typing into a search field, and what the developer declared in
/// [`Config::real_data_safe`]. Returns why anything else is skipped.
fn real_data_gate(node: &UiNode, config: &Config, place: Place) -> Option<String> {
    let id = node.identifier.as_deref();
    if id.is_some_and(|id| {
        config
            .real_data_safe
            .iter()
            .any(|s| s == id || normalise_identifier(s) == normalise_identifier(id))
    }) {
        return None;
    }
    let label = node.label.as_deref().unwrap_or("").to_lowercase();
    let navigation = match node.kind.as_str() {
        "Cell" | "Tab" => true,
        "SearchField" => true,
        "Button" => {
            place.back_button
                || place.tab_bar
                || id == Some("BackButton")
                || LEAVE_LABELS.contains(&label.as_str())
        }
        _ => false,
    };
    if navigation {
        None
    } else {
        Some(format!(
            "real data: only navigation is tapped (rows, tabs, back, cancel); a {} could change something. \
             Declare it in forge.toml `[ui_test] real_data_safe` if it is safe, or explore on test data",
            node.kind
        ))
    }
}

/// Why a control must not be touched, if it mustn't.
fn gate(node: &UiNode, config: &Config, place: Place) -> Option<String> {
    for skip in &config.skip {
        if node.identifier.as_deref() == Some(skip) || node.label.as_deref() == Some(skip) {
            return Some("on the skip list".into());
        }
    }
    let mut all = Vec::new();
    for text in [&node.identifier, &node.label].into_iter().flatten() {
        all.extend(words(text));
    }
    if let Some(word) = all.iter().find(|w| SYSTEM.contains(&w.as_str())) {
        return Some(format!("starts a system service (\"{word}\")"));
    }
    if all
        .windows(2)
        .any(|w| SYSTEM_PAIRS.contains(&(w[0].as_str(), w[1].as_str())))
    {
        return Some("starts a system service (keyboard switcher)".into());
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
        // The word lists above only add reasons; what decides is the
        // allow list.
        return real_data_gate(node, config, place);
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

/// Labels and identifiers of the keyboard's own controls (system and
/// third-party keyboards), lower-cased.
const KEYBOARD_MARKERS: &[&str] = &[
    "next keyboard",
    "dictate",
    "dictation",
    "globekey",
    "emoji",
    "keyboard",
];

/// How far above the top of the keys keyboard extension panels reach
/// (Grammarly's top panel, Tenor's search, the system suggestion bar).
const KEYBOARD_PANEL: f64 = 100.0;

/// Where the keyboard begins, if one is on screen: the top of the keys, the
/// `Keyboard` element or a keyboard control such as Next keyboard.
fn keyboard_top(tree: &UiNode) -> Option<f64> {
    tree.walk()
        .filter(|n| n.visible || n.kind == "Keyboard")
        .filter(|n| {
            n.kind == "Keyboard"
                || n.kind == "Key"
                || [&n.identifier, &n.label]
                    .into_iter()
                    .flatten()
                    .any(|t| KEYBOARD_MARKERS.contains(&t.to_lowercase().as_str()))
        })
        .filter_map(|n| n.frame.map(|f| f.y))
        .reduce(f64::min)
}

/// The app's own part of the screen: the tree without the keyboard, its
/// keys and the extension panels above them. Seen on Cati (2026-11-08):
/// left in, the explorer tapped Next keyboard into third-party keyboards,
/// typed on their keys and turned dictation on.
pub fn app_part(tree: &UiNode) -> UiNode {
    let cutoff = keyboard_top(tree).map(|top| top - KEYBOARD_PANEL);
    fn prune(node: &UiNode, cutoff: Option<f64>) -> UiNode {
        UiNode {
            children: node
                .children
                .iter()
                .filter(|c| c.kind != "Keyboard" && c.kind != "Key")
                .filter(|c| match (cutoff, c.frame) {
                    (Some(cutoff), Some(frame)) => frame.y < cutoff,
                    _ => true,
                })
                .map(|c| prune(c, cutoff))
                .collect(),
            ..node.clone()
        }
    }
    prune(tree, cutoff)
}

/// A screen's name for people: its navigation bar or alert title.
fn title(tree: &UiNode) -> Option<String> {
    tree.walk()
        .find(|n| matches!(n.kind.as_str(), "Alert" | "NavigationBar") && n.visible)
        .and_then(|n| n.label.clone().or_else(|| n.identifier.clone()))
}

/// The system alert iOS shows after a shake while typing: "Undo Typing",
/// "Undo Paste" and so on, with Cancel and Undo.
fn is_undo_alert(alert: &UiNode) -> bool {
    alert
        .walk()
        .filter_map(|n| n.label.as_deref())
        .any(|l| l.starts_with("Undo ") && l.split_whitespace().count() <= 3)
}

/// The controls on a screen, and those the explorer won't touch.
fn candidates(tree: &UiNode, config: &Config) -> (Vec<Candidate>, Vec<Skipped>) {
    let mut found = Vec::new();
    let mut skipped = Vec::new();
    let mut keys = HashSet::new();
    visit(
        tree,
        config,
        Place::default(),
        &mut found,
        &mut skipped,
        &mut keys,
    );
    (found, skipped)
}

fn visit(
    node: &UiNode,
    config: &Config,
    place: Place,
    found: &mut Vec<Candidate>,
    skipped: &mut Vec<Skipped>,
    keys: &mut HashSet<String>,
) {
    // Keyboard keys are typing, not navigation.
    if node.kind == "Keyboard" {
        return;
    }
    // iOS's Shake to Undo alert ("Undo Typing"): the system's, not the
    // app's. On Cati the walk tapped its Undo, which emptied the field it
    // had typed and kept Good Dog's Start button disabled. Only its Cancel.
    if node.kind == "Alert" && node.visible && is_undo_alert(node) {
        for button in node.walk().filter(|n| n.kind == "Button") {
            let label = button.label.as_deref().unwrap_or("");
            if label.eq_ignore_ascii_case("cancel") {
                consider(button, config, Place::default(), found, skipped, keys);
            } else {
                skipped.push(Skipped {
                    element: button.summary(),
                    reason: "the system's Shake to Undo alert: only Cancel is tapped".into(),
                });
            }
        }
        return;
    }
    if node.visible && node.enabled {
        consider(node, config, place, found, skipped, keys);
    }
    // A navigation bar's first button is its back button: its label is the
    // previous screen's title, not "Back".
    if node.kind == "NavigationBar" {
        for (i, child) in node.children.iter().enumerate() {
            let first_button = i == 0 && child.kind == "Button";
            let place = Place {
                back_button: first_button,
                ..place
            };
            visit(child, config, place, found, skipped, keys);
        }
        return;
    }
    let inner = Place {
        back_button: false,
        tab_bar: place.tab_bar || node.kind == "TabBar",
    };
    for child in &node.children {
        visit(child, config, inner, found, skipped, keys);
    }
}

fn consider(
    node: &UiNode,
    config: &Config,
    place: Place,
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
    if let Some(reason) = gate(node, config, place) {
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
            .unwrap_or_else(|| filler(node, &config.default_text));
        Action::Type(locator, text)
    } else {
        Action::Tap(locator)
    };
    let label = node.label.as_deref().unwrap_or_default().to_lowercase();
    let exit = kind == "Button"
        && (node.identifier.as_deref() == Some("BackButton")
            || EXIT_LABELS.contains(&label.as_str())
            || place.back_button);
    let strength = node
        .identifier
        .as_deref()
        .map(|id| config.focus.strength(id))
        .unwrap_or(Strength::None);
    found.push(Candidate {
        action,
        key,
        exit,
        strength,
    });
}

/// Element kinds that scroll.
const SCROLLABLE: &[&str] = &["Table", "CollectionView", "ScrollView"];

/// The list or scroll view to scroll for controls below the fold: the
/// first visible one, by identifier when it has one, else by kind and its
/// place among elements of that kind (never by coordinates).
fn scroll_target(tree: &UiNode) -> Option<Locator> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for node in tree.walk() {
        if !SCROLLABLE.contains(&node.kind.as_str()) {
            continue;
        }
        let index = seen.entry(node.kind.as_str()).or_default();
        let this = *index;
        *index += 1;
        if !node.visible {
            continue;
        }
        return Some(match &node.identifier {
            Some(id) => Locator::Id(id.clone()),
            None => Locator::Kind {
                kind: node.kind.clone(),
                label: None,
                index: Some(this),
            },
        });
    }
    None
}

/// What the any-app policy needs to know about the element `locator`
/// names on `tree`: its label, identifier and kind.
fn target_of(tree: &UiNode, locator: &Locator) -> policy::Target {
    let found = match locator {
        Locator::Id(id) => tree
            .walk()
            .find(|n| n.visible && n.identifier.as_deref() == Some(id)),
        Locator::Label(label) => tree
            .walk()
            .find(|n| n.visible && n.label.as_deref() == Some(label)),
        _ => None,
    };
    match found {
        Some(n) => policy::Target {
            label: n.label.clone(),
            identifier: n.identifier.clone(),
            kind: Some(n.kind.clone()),
        },
        None => match locator {
            Locator::Id(id) => policy::Target {
                identifier: Some(id.clone()),
                ..Default::default()
            },
            Locator::Label(label) => policy::Target {
                label: Some(label.clone()),
                ..Default::default()
            },
            _ => policy::Target::default(),
        },
    }
}

/// Words in a field's identifier or label that ask for a number.
const NUMBER_WORDS: &[&str] = &[
    "number", "count", "amount", "price", "cost", "total", "quantity", "qty", "age", "year",
    "years", "page", "pages", "minutes", "hours", "days", "percent", "rating", "goal", "bill",
    "tip", "people", "pin", "zip", "postcode",
];

/// Text that suits the field. WebDriverAgent's tree doesn't say which
/// keyboard a field uses, so this goes by its identifier, label and current
/// value: a field showing a number, or named like one, gets digits (seen on
/// Cati: "Forge" typed into a number field), an email field an address, a
/// web field a URL, a phone field a phone number. Anything else gets
/// `default`.
fn filler(node: &UiNode, default: &str) -> String {
    let mut all = Vec::new();
    for text in [&node.identifier, &node.label].into_iter().flatten() {
        all.extend(words(text));
    }
    let has = |list: &[&str]| all.iter().any(|w| list.contains(&w.as_str()));
    let numeric_value = node.value.as_deref().is_some_and(|v| {
        let v = v.trim();
        !v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || ".,-".contains(c))
    });
    if has(&["email", "mail"]) {
        "forge@example.com".into()
    } else if has(&["url", "website", "web", "link", "homepage"]) {
        "https://example.com".into()
    } else if has(&["phone", "tel", "mobile"]) {
        "5550100".into()
    } else if numeric_value || has(NUMBER_WORDS) {
        "3".into()
    } else {
        default.to_owned()
    }
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
    /// `repro` as `forge ui run` steps, so a later walk can replay it.
    pub route: Vec<String>,
    /// `observed` or `heuristic` ([`crate::focus::confidence`]).
    pub confidence: String,
    /// How it is tied to the change explored: `direct`, `entry`, `screen`
    /// or `none` (also when the walk had no focus).
    pub focus: String,
    /// What that tie rests on: `declared`, `inferred` or `none`
    /// ([`Strength::basis`]). Read from the source as text: never proof.
    pub focus_basis: String,
    /// The changed files it is tied to and how; empty when there is no
    /// link (not "unrelated": no link could be established).
    pub changed_files: Vec<serde_json::Value>,
    /// The screen's tree and screenshot, relative to the output folder.
    pub evidence: serde_json::Value,
    /// Replayed once after the walk: did it happen again? `None` when it
    /// wasn't replayed (not asked, out of time, or a kind a replay can't
    /// check, such as an audit finding).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reproduced: Option<bool>,
    #[serde(skip)]
    actions: Vec<Action>,
}

/// One action of the walk.
#[derive(Debug, Clone, Serialize)]
pub struct StepRecord {
    pub step: usize,
    /// `explore`, `route` (following a known way to the focus), `recheck`
    /// (an earlier walk's issue) or `verify` (replaying this walk's issue).
    pub phase: String,
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
    /// The identifiers on it: what a later walk's focus is matched with.
    pub identifiers: Vec<String>,
    /// `path` as `forge ui run` steps, for a later walk to follow.
    pub route: Vec<String>,
    /// It holds identifiers declared by (or leading to) the change.
    pub focus: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub app: String,
    pub seed: u64,
    /// What every launch was given (test data, appearance, text size): a
    /// later walk compares screenshots only with a walk launched the same.
    pub launch_args: Vec<String>,
    pub actions: usize,
    pub duration_ms: u64,
    /// `explored` (nothing left to try), `action_budget` or `time_budget`.
    pub stopped: String,
    pub screens: Vec<ScreenSummary>,
    pub issues: Vec<Issue>,
    pub journal: Vec<StepRecord>,
    /// With a focus: what of it was reached and tried, what wasn't, and
    /// the routes followed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus: Option<serde_json::Value>,
}

impl Report {
    pub fn errors(&self) -> usize {
        self.issues.iter().filter(|i| i.severity == "error").count()
    }

    /// The walk as a transition graph (`graph.json`): its screens, and for
    /// each action tried from a screen, where it led each time. An action
    /// is `repeatable` when it was done more than once from the same screen
    /// and always led to the same place. Built from the journal, so it
    /// holds only what was seen, not what the app could do.
    pub fn graph(&self) -> serde_json::Value {
        let mut edges: BTreeMap<(String, String), Vec<(usize, String)>> = BTreeMap::new();
        for s in &self.journal {
            if s.action == "launch" || s.from.is_empty() {
                continue;
            }
            edges
                .entry((s.from.clone(), s.action.clone()))
                .or_default()
                .push((s.step, s.outcome.clone()));
        }
        let edges: Vec<serde_json::Value> = edges
            .into_iter()
            .map(|((from, action), seen)| {
                let to: BTreeSet<&str> = seen.iter().map(|(_, o)| o.as_str()).collect();
                serde_json::json!({
                    "from": from,
                    "action": action,
                    "to": to,
                    "times": seen.len(),
                    "repeatable": seen.len() > 1 && to.len() == 1,
                    "steps": seen.iter().map(|(n, _)| n).collect::<Vec<_>>(),
                })
            })
            .collect();
        let screens: Vec<serde_json::Value> = self
            .screens
            .iter()
            .map(|s| {
                serde_json::json!({
                    "id": s.id,
                    "title": s.title,
                    "screenshot": s.screenshot,
                    "identifiers": s.identifiers,
                    "path": s.path,
                })
            })
            .collect();
        serde_json::json!({
            "format": "forge.explore-graph/1",
            "app": self.app,
            "seed": self.seed,
            "launch_args": self.launch_args,
            "root": self.screens.first().map(|s| s.id.clone()),
            "screens": screens,
            "edges": edges,
        })
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
    identifiers: BTreeSet<String>,
    /// The shortest known way here without no-op taps.
    route: Vec<Action>,
    /// Scrolls done here to reach controls below the fold.
    scrolls: usize,
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
    /// The any-app policy's gate: allows everything in the developer's own
    /// app; in another developer's app it checks each action against the
    /// person's level and the commit gate, and journals it.
    gate: &'a mut Gate,
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
    /// The trail without taps that left the screen as it was: the route a
    /// later walk follows (`ScreenSummary::route`).
    route_trail: Vec<Action>,
    /// Exit controls used in a row to leave exhausted screens.
    exits_in_row: usize,
    /// What the actions now are for (the journal's `phase`).
    phase: &'static str,
    /// Routes still to follow.
    routes: VecDeque<Route>,
    /// Routes followed: kind, target, where they ended, focus seen there.
    routes_done: Vec<serde_json::Value>,
    /// Direct focus identifiers seen, and the first screen each was on.
    focus_seen: BTreeMap<String, String>,
    /// Focus identifiers acted on, and the step that first did.
    focus_tried: BTreeMap<String, usize>,
    /// While replaying an issue: record nothing new.
    quiet: bool,
    /// The last action's outcome, and whether it left the tree as it was.
    last_outcome: String,
    last_unchanged: bool,
}

/// Explores `config.app`. Progress goes to `out`; screenshots, trees,
/// `explore.json` and one replay file per issue go to `out_dir`.
pub fn run(
    config: &Config,
    driver: &mut dyn Driver,
    out_dir: Option<&Path>,
    out: &mut dyn Write,
) -> Result<Report> {
    run_gated(config, driver, out_dir, out, &mut Gate::own(&config.app))
}

/// [`run`] in an app the any-app policy decides about: every launch, tap,
/// typing and scroll is put to `gate` first. A refused tap or typing is
/// skipped (and listed with the policy's reason); a refused launch ends
/// the walk.
pub fn run_gated(
    config: &Config,
    driver: &mut dyn Driver,
    out_dir: Option<&Path>,
    out: &mut dyn Write,
    gate: &mut Gate,
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
        gate,
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
        route_trail: Vec::new(),
        exits_in_row: 0,
        phase: "explore",
        routes: config.focus.routes.iter().cloned().collect(),
        routes_done: Vec::new(),
        focus_seen: BTreeMap::new(),
        focus_tried: BTreeMap::new(),
        quiet: false,
        last_outcome: String::new(),
        last_unchanged: false,
    };
    writeln!(
        explorer.out,
        "Exploring {} (seed {}, at most {} actions, {} s)",
        config.app,
        config.seed,
        config.max_actions,
        config.budget.as_secs()
    )?;
    if !config.focus.is_empty() {
        writeln!(
            explorer.out,
            "Aimed at the change: {} identifier(s) or prefixes, {} route(s) from earlier walks",
            config.focus.links.len(),
            config.focus.routes.len()
        )?;
    }
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
    explorer.verify_issues(deadline)?;
    explorer.finish(stopped, started)
}

impl Explorer<'_> {
    /// Does one thing; false when nothing reachable is left to try.
    fn step(&mut self) -> Result<bool> {
        let here = self.index[&self.current];
        if !self.config.focus.is_empty() {
            // The change first: its own controls here, then the known ways
            // to it (earlier issues to recheck first), then the ways in
            // from here, then the nearest screen with focus controls left.
            self.phase = "explore";
            if let Some(candidate) = self.next_untried(here, Strength::Direct) {
                self.try_candidate(here, candidate)?;
                return Ok(true);
            }
            while let Some(route) = self.routes.pop_front() {
                // A way to a screen this walk already took adds nothing;
                // an earlier issue is always checked again.
                let done = route.kind != "recheck"
                    && route.actions.iter().all(|a| {
                        let a = a.describe();
                        self.journal.iter().any(|s| s.action == a)
                    });
                if done {
                    self.routes_done.push(serde_json::json!({
                        "kind": route.kind, "target": route.target, "ended": "skipped: already walked",
                    }));
                    continue;
                }
                self.follow(route)?;
                return Ok(true);
            }
            if let Some(candidate) = self.next_untried(here, Strength::Entry) {
                self.try_candidate(here, candidate)?;
                return Ok(true);
            }
            if let Some(target) = self.frontier(Strength::Entry) {
                self.go_to(target)?;
                return Ok(true);
            }
        }
        if let Some(candidate) = self.next_untried(here, Strength::None) {
            self.try_candidate(here, candidate)?;
            return Ok(true);
        }
        // Nothing left in view: scroll the list for controls below the fold.
        if self.screens[here].scrolls < self.config.scrolls_per_screen {
            if let Some(list) = scroll_target(&self.tree) {
                self.screens[here].scrolls += 1;
                self.perform(Action::Scroll(list, "down".into()), true)?;
                // The end of the list: nothing moved, so stop scrolling here.
                if self.last_unchanged {
                    if let Some(&i) = self.index.get(&self.current) {
                        self.screens[i].scrolls = self.config.scrolls_per_screen;
                    }
                }
                return Ok(true);
            }
        }
        // Nothing left here. Is anything left anywhere?
        let Some(target) = self.frontier(Strength::None) else {
            return Ok(false);
        };
        self.go_to(target)?;
        Ok(true)
    }

    fn try_candidate(&mut self, here: usize, candidate: Candidate) -> Result<()> {
        self.screens[here].tried += 1;
        self.screens[here].tried_keys.insert(candidate.key.clone());
        self.exits_in_row = 0;
        self.perform(candidate.action, true)
    }

    /// Heads for screen `target`: leaves through a back or dismiss control
    /// if the screen has one, twice in a row at most (two exhausted screens
    /// can point at each other); otherwise relaunches and replays the way.
    fn go_to(&mut self, target: usize) -> Result<()> {
        match self.exit_action() {
            Some(action) if Some(&self.current) != self.root.as_ref() && self.exits_in_row < 2 => {
                self.exits_in_row += 1;
                self.perform(action, false)
            }
            _ => {
                self.exits_in_row = 0;
                self.replay(target)
            }
        }
    }

    /// Follows a route from an earlier walk: relaunch, then its actions,
    /// stopping at the first that can't be done.
    fn follow(&mut self, route: Route) -> Result<()> {
        self.phase = if route.kind == "recheck" {
            "recheck"
        } else {
            "route"
        };
        writeln!(
            self.out,
            "          following {}: {}",
            route.kind, route.target
        )?;
        if Some(&self.current) != self.root.as_ref() {
            self.relaunch()?;
        } else {
            self.trail.clear();
            self.route_trail.clear();
        }
        let mut ended = "followed";
        for action in &route.actions {
            if self.actions >= self.config.max_actions {
                ended = "budget";
                break;
            }
            // A control the route uses counts as tried where it is.
            if let Some(&here) = self.index.get(&self.current) {
                let screen = &mut self.screens[here];
                if let Some(at) = screen.untried.iter().position(|c| c.action == *action) {
                    let c = screen.untried.remove(at);
                    screen.tried_keys.insert(c.key);
                    screen.tried += 1;
                }
            }
            self.perform(action.clone(), true)?;
            if self.last_outcome == "not_found" || self.last_outcome == "failed" {
                ended = "broken";
                break;
            }
            if self.last_outcome == "app_closed" || self.last_outcome.starts_with("left_app") {
                ended = "left";
                break;
            }
        }
        let focus_here = self
            .index
            .get(&self.current)
            .map(|&i| {
                self.screens[i]
                    .identifiers
                    .iter()
                    .any(|id| self.config.focus.strength(id) >= Strength::Entry)
            })
            .unwrap_or(false);
        self.routes_done.push(serde_json::json!({
            "kind": route.kind,
            "target": route.target,
            "ended": ended,
            "last_outcome": self.last_outcome,
            "on_focus_screen": focus_here,
        }));
        self.phase = "explore";
        Ok(())
    }

    /// The next control to try on screen `here`, if it is tied to the
    /// change at least `min`. Controls that appeared since the first visit
    /// (a button enabled by typing) are added, focus ones in front.
    fn next_untried(&mut self, here: usize, min: Strength) -> Option<Candidate> {
        let (fresh, skipped) = candidates(&self.tree, self.config);
        let screen = &mut self.screens[here];
        screen.skipped.extend(skipped);
        for candidate in fresh {
            let known = screen.tried_keys.contains(&candidate.key)
                || screen.untried.iter().any(|c| c.key == candidate.key);
            if !known && screen.tried + screen.untried.len() < self.config.per_screen {
                screen.has_exit |= candidate.exit;
                // New controls go in front of the exits, and in front of
                // weaker ties to the change.
                let at = screen
                    .untried
                    .iter()
                    .position(|c| c.exit || (!candidate.exit && c.strength < candidate.strength));
                match at {
                    Some(at) if !candidate.exit => screen.untried.insert(at, candidate),
                    _ => screen.untried.push(candidate),
                }
            }
        }
        match screen.untried.first() {
            Some(c) if c.strength >= min => Some(screen.untried.remove(0)),
            _ => None,
        }
    }

    /// The back or dismiss control of the screen in front.
    fn exit_action(&self) -> Option<Action> {
        let (found, _) = candidates(&self.tree, self.config);
        found.into_iter().find(|c| c.exit).map(|c| c.action)
    }

    /// The screen quickest to reach with something left to try, tied to
    /// the change at least `min`.
    fn frontier(&self, min: Strength) -> Option<usize> {
        self.screens
            .iter()
            .enumerate()
            .filter(|(_, s)| s.untried.iter().any(|c| c.strength >= min))
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
            self.route_trail.clear();
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
        self.route_trail.clear();
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
        if let Err(why) = self.gate.check(&self.policy_action(&action)) {
            if matches!(action, Action::Launch) {
                return Err(UiError::Refused(why));
            }
            // Refused by the person's policy: not done, and listed as
            // skipped on the screen, so the gap in coverage is visible.
            self.actions -= 1;
            if let Some(&i) = self.index.get(&from) {
                self.screens[i].skipped.insert(Skipped {
                    element: action.describe(),
                    reason: format!("the any-app policy: {why}"),
                });
            }
            writeln!(self.out, "          refused {}: {why}", action.describe())?;
            self.last_outcome = "refused".into();
            self.last_unchanged = true;
            return Ok(());
        }
        let result = match &action {
            Action::Launch => self
                .driver
                .launch(&self.config.app, &self.config.launch_args),
            Action::Tap(locator) => self.on_element(locator, &mut |d, e| d.tap(e)),
            Action::Scroll(locator, direction) => {
                self.on_element(locator, &mut |d, e| d.scroll(e, direction))
            }
            Action::Type(locator, text) => self
                .on_element(locator, &mut |d, e| d.type_text(e, text))
                // Put the keyboard away so the walk carries on in the app.
                // Not every keyboard can be dismissed this way; what stays
                // up is cut from the tree by `app_part`.
                .map(|()| {
                    let _ = self.driver.dismiss_keyboard();
                }),
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
            self.route_trail.push(action.clone());
        }
        let landed = self.observe(step)?;
        // A tap that left the screen as it was isn't part of a route.
        if matches!(&landed, Landed::Screen(id) if *id == from)
            && matches!(action, Action::Tap(_))
            && self.route_trail.last() == Some(&action)
        {
            self.route_trail.pop();
        }
        let ms = started.elapsed().as_millis() as u64;
        self.last_unchanged = self.tree == before;
        if let Some(id) = crate::focus::action_id(&action) {
            if self.config.focus.strength(id) >= Strength::Entry {
                self.focus_tried.entry(id.to_owned()).or_insert(step);
            }
        }
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
                self.route_trail.pop();
            }
        }
        let outcome = outcome.unwrap_or_default();
        self.last_outcome = outcome.clone();
        writeln!(
            self.out,
            "  {step:>3}. {:<10} {} -> {outcome} ({:.1} s)",
            if from.is_empty() { "-" } else { &from },
            action.describe(),
            ms as f64 / 1000.0
        )?;
        self.journal.push(StepRecord {
            step,
            phase: self.phase.into(),
            from,
            action: action.describe(),
            outcome,
            ms,
        });
        let (outcome_was, unchanged_was) = (self.last_outcome.clone(), self.last_unchanged);
        let recovered = match recover {
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
        };
        // What this action did, not the relaunch that followed it.
        self.last_outcome = outcome_was;
        self.last_unchanged = unchanged_was;
        recovered
    }

    /// The action as the any-app policy sees it: what a tap or typing will
    /// touch, read from the screen in front.
    fn policy_action(&self, action: &Action) -> policy::Action {
        match action {
            Action::Launch => policy::Action::Launch,
            Action::Scroll(..) => policy::Action::Scroll,
            Action::Tap(locator) => policy::Action::Tap(target_of(&self.tree, locator)),
            Action::Type(locator, text) => policy::Action::Type {
                target: target_of(&self.tree, locator),
                chars: text.chars().count(),
                presses_return: text.contains('\n'),
            },
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
        // Only the app's own part: never the keyboard or its extensions.
        let tree = app_part(&self.driver.tree()?);
        let front = self.driver.foreground()?;
        match front.as_deref() {
            Some(app) if app == self.config.app => {}
            Some(other) if other != crate::fake::SPRINGBOARD => {
                return Ok(Landed::Other(other.to_owned()))
            }
            _ => return Ok(Landed::Closed),
        }
        let id = fingerprint(&tree);
        for seen in tree.walk().filter_map(|n| n.identifier.as_deref()) {
            if self.config.focus.strength(seen) == Strength::Direct {
                self.focus_seen
                    .entry(seen.to_owned())
                    .or_insert_with(|| id.clone());
            }
        }
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
            if self.route_trail.len() < self.screens[i].route.len() {
                self.screens[i].route = self.route_trail.clone();
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
        // The change first; the seeded order holds within each tie.
        found.sort_by_key(|c| std::cmp::Reverse(c.strength));
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
            route: self.route_trail.clone(),
            scrolls: 0,
            untried: found,
            tried_keys: HashSet::new(),
            tried: 0,
            over_limit,
            exits: BTreeSet::new(),
            skipped: skipped.into_iter().collect(),
            has_exit,
            screenshot,
            identifiers: self
                .tree
                .walk()
                .filter_map(|n| n.identifier.clone())
                .collect(),
        });
        self.check_screen(id, step);
        Ok(())
    }

    /// The checks every new screen gets: Forge's accessibility audit
    /// (labels, identifiers, small tap targets, text cut off).
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
        if self.quiet {
            return;
        }
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
            route: actions
                .iter()
                .map(|a| a.toml_step(&self.config.launch_args))
                .collect(),
            confidence: String::new(),
            focus: "none".into(),
            focus_basis: "none".into(),
            changed_files: Vec::new(),
            evidence: serde_json::Value::Null,
            reproduced: None,
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
        self.annotate_issues();
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
                identifiers: s.identifiers.iter().cloned().collect(),
                route: s
                    .route
                    .iter()
                    .map(|a| a.toml_step(&self.config.launch_args))
                    .collect(),
                focus: s
                    .identifiers
                    .iter()
                    .any(|id| self.config.focus.strength(id) >= Strength::Entry),
            })
            .collect();
        let focus = (!self.config.focus.is_empty()).then(|| self.focus_report());
        let report = Report {
            app: self.config.app.clone(),
            seed: self.config.seed,
            launch_args: self.config.launch_args.clone(),
            actions: self.actions,
            duration_ms: started.elapsed().as_millis() as u64,
            stopped: stopped.to_owned(),
            screens,
            issues: self.issues,
            journal: self.journal,
            focus,
        };
        if let Some(dir) = self.out_dir {
            std::fs::write(
                dir.join("explore.json"),
                serde_json::to_vec_pretty(&report).map_err(std::io::Error::other)?,
            )?;
            std::fs::write(
                dir.join("graph.json"),
                serde_json::to_vec_pretty(&report.graph()).map_err(std::io::Error::other)?,
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

impl Explorer<'_> {
    /// Fills in each issue's confidence, its tie to the change, the files
    /// behind that tie and the screen evidence.
    fn annotate_issues(&mut self) {
        let focus = &self.config.focus;
        let screens: HashMap<&str, &Screen> =
            self.screens.iter().map(|s| (s.id.as_str(), s)).collect();
        for issue in &mut self.issues {
            issue.confidence = crate::focus::confidence(issue).into();
            let screen = screens.get(issue.screen.as_str());
            let action_id = issue.actions.last().and_then(crate::focus::action_id);
            let action_strength = action_id
                .map(|id| focus.strength(id))
                .unwrap_or(Strength::None);
            // The screen's own tie: direct identifiers on it.
            let on_screen: Vec<&String> = screen
                .map(|s| {
                    s.identifiers
                        .iter()
                        .filter(|id| focus.strength(id) == Strength::Direct)
                        .collect()
                })
                .unwrap_or_default();
            let strength = action_strength.max(if on_screen.is_empty() {
                Strength::None
            } else {
                Strength::Screen
            });
            issue.focus = match strength {
                Strength::Direct => "direct",
                Strength::Entry => "entry",
                Strength::Screen => "screen",
                Strength::None => "none",
            }
            .into();
            issue.focus_basis = strength.basis().into();
            let mut files = action_id.map(|id| focus.files_for(id)).unwrap_or_default();
            if files.is_empty() {
                for id in on_screen.iter().take(3) {
                    files.extend(focus.files_for(id));
                }
            }
            let mut seen = HashSet::new();
            files.retain(|f| seen.insert(f["file"].to_string()));
            issue.changed_files = files;
            issue.evidence = serde_json::json!({
                "screen_tree": self.config.screenshots.then(|| format!("screens/{}.json", issue.screen)),
                "screenshot": screen.and_then(|s| s.screenshot.clone()),
                "step": issue.step,
            });
        }
    }

    fn focus_report(&self) -> serde_json::Value {
        let focus = &self.config.focus;
        let targets = focus.direct_targets();
        let reached = |t: &String| match t.strip_suffix('*') {
            Some(prefix) => self.focus_seen.keys().any(|k| k.starts_with(prefix)),
            None => self.focus_seen.contains_key(t),
        };
        let not_reached: Vec<&String> = targets.iter().filter(|t| !reached(t)).collect();
        let first_direct = self
            .focus_tried
            .iter()
            .filter(|(id, _)| focus.strength(id) == Strength::Direct)
            .map(|(_, step)| *step)
            .min();
        serde_json::json!({
            "targets": targets,
            "seen": self.focus_seen,
            "tried": self.focus_tried,
            "first_direct_action": first_direct,
            "not_reached": not_reached,
            "not_reached_note": (!not_reached.is_empty()).then_some(
                "Never on screen during the walk: behind data the test-data mode doesn't create, below the fold (scrolling isn't built), behind a skipped control, or out of budget. Not checked, so not passed."
            ),
            "routes": self.routes_done,
            "routes_not_followed": self.routes.len(),
            "basis_note": crate::focus::BASIS_NOTE,
        })
    }

    /// Replays issues once more to see whether they come back: errors
    /// first, then warnings on the focus, up to `config.verify`, while
    /// there is time. Kinds a replay can't check keep `reproduced: None`.
    fn verify_issues(&mut self, deadline: Instant) -> Result<()> {
        const CHECKABLE: &[&str] = &[
            "app_closed",
            "left_app",
            "action_failed",
            "error_alert",
            "no_effect",
        ];
        let mut order: Vec<usize> = (0..self.issues.len())
            .filter(|&i| CHECKABLE.contains(&self.issues[i].issue.as_str()))
            .filter(|&i| {
                self.issues[i].severity == "error"
                    || (self.issues[i].severity == "warning"
                        && self.issues[i]
                            .actions
                            .last()
                            .and_then(crate::focus::action_id)
                            .is_some_and(|id| self.config.focus.strength(id) >= Strength::Entry))
            })
            .collect();
        order.sort_by_key(|&i| (self.issues[i].severity != "error", i));
        order.truncate(self.config.verify);
        if order.is_empty() {
            return Ok(());
        }
        writeln!(
            self.out,
            "Replaying {} issue(s) to see whether they come back",
            order.len()
        )?;
        self.phase = "verify";
        self.quiet = true;
        for i in order {
            if Instant::now() >= deadline {
                break;
            }
            let actions = self.issues[i].actions.clone();
            if actions.is_empty() {
                continue;
            }
            self.relaunch()?;
            let mut done = true;
            for action in &actions {
                self.perform(action.clone(), false)?;
                if self.last_outcome == "not_found" {
                    done = false;
                    break;
                }
            }
            let issue = &self.issues[i];
            let again = done
                && match issue.issue.as_str() {
                    "app_closed" => self.last_outcome == "app_closed",
                    "left_app" => self.last_outcome.starts_with("left_app"),
                    "action_failed" => self.last_outcome == "failed",
                    "no_effect" => self.last_unchanged && self.last_outcome == issue.screen,
                    "error_alert" => {
                        let text = issue
                            .message
                            .split_once('"')
                            .map(|(_, t)| t.trim_end_matches('"'))
                            .unwrap_or("");
                        self.tree.walk().any(|n| {
                            n.kind == "Alert"
                                && n.walk()
                                    .filter_map(|c| c.label.as_deref())
                                    .collect::<Vec<_>>()
                                    .join(" ")
                                    == text
                        })
                    }
                    _ => false,
                };
            writeln!(
                self.out,
                "          issue {}: {}",
                issue.number,
                if again {
                    "came back"
                } else {
                    "did not come back"
                }
            )?;
            self.issues[i].reproduced = Some(again);
        }
        self.quiet = false;
        self.phase = "explore";
        Ok(())
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
        /// Typing brings up a keyboard, as on the phone.
        keyboard_up: bool,
        /// Whether `dismiss_keyboard` works (WDA can't always).
        keyboard_dismissable: bool,
        dismissals: usize,
    }

    fn at(mut n: UiNode, y: f64) -> UiNode {
        n.frame = Some(Rect {
            x: 0.0,
            y,
            width: 40.0,
            height: 40.0,
        });
        n
    }

    /// A system keyboard with a third-party extension's panels, as WDA
    /// showed it on Cati.
    fn keyboard() -> Vec<UiNode> {
        vec![
            at(
                UiNode {
                    children: vec![
                        at(node("Key", None, Some("c")), 700.0),
                        at(node("Key", None, Some("shift")), 760.0),
                    ],
                    ..node("Keyboard", None, None)
                },
                640.0,
            ),
            at(
                node("Button", Some("gr-button-toppanel-1"), Some("Tone")),
                560.0,
            ),
            at(node("Button", None, Some("Search Tenor")), 580.0),
            at(node("Button", None, Some("Next keyboard")), 860.0),
            at(node("Button", Some("dictation"), Some("Dictate")), 860.0),
        ]
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
                keyboard_up: false,
                keyboard_dismissable: true,
                dismissals: 0,
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
        fn scroll(&mut self, _: &ElementRef, direction: &str) -> Result<()> {
            self.log.push(format!("scroll {direction}"));
            // Everything in the screen's lists comes into view.
            for list in self
                .screen()
                .children
                .iter_mut()
                .filter(|n| n.kind == "Table")
            {
                for row in &mut list.children {
                    row.visible = true;
                }
            }
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
                Some(To::Screen(next)) => {
                    self.current = next;
                    self.keyboard_up = false;
                }
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
            self.keyboard_up = true;
            Ok(())
        }
        fn dismiss_keyboard(&mut self) -> Result<()> {
            self.dismissals += 1;
            if self.keyboard_dismissable {
                self.keyboard_up = false;
            }
            Ok(())
        }
        fn clear(&mut self, _: &ElementRef) -> Result<()> {
            Ok(())
        }
        fn attribute(&mut self, _: &ElementRef, _: &str) -> Result<Option<String>> {
            Ok(None)
        }
        fn tree(&mut self) -> Result<UiNode> {
            let mut tree = self.screen().clone();
            if self.keyboard_up {
                tree.children.extend(keyboard());
            }
            Ok(tree)
        }
        fn screenshot(&mut self) -> Result<Vec<u8>> {
            Ok(b"\x89PNG".to_vec())
        }
        fn foreground(&mut self) -> Result<Option<String>> {
            Ok(self.front.clone())
        }
    }

    #[test]
    fn the_shake_to_undo_alert_is_only_ever_cancelled() {
        let mut alert = node("Alert", None, Some("Undo Typing"));
        alert.children = vec![
            node("StaticText", None, Some("Undo Typing")),
            node("Button", None, Some("Cancel")),
            node("Button", None, Some("Undo")),
        ];
        let tree = app(vec![
            node("TextField", Some("dogForm.name"), Some("Name")),
            alert,
        ]);
        let (found, skipped) = candidates(&tree, &config());
        let labels: Vec<String> = found.iter().map(|c| c.key.clone()).collect();
        assert!(labels.iter().any(|l| l.contains("Cancel")), "{labels:?}");
        assert!(!labels.iter().any(|l| l.ends_with(":Undo")), "{labels:?}");
        assert!(skipped.iter().any(|s| s.reason.contains("Shake to Undo")));
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
                gate(
                    &node("Button", Some(id), Some(label)),
                    &config,
                    Place::default()
                )
                .is_some(),
                "{id}"
            );
        }
        let switch = node(
            "Switch",
            Some("settings.catalogue"),
            Some("Online catalogue search"),
        );
        assert!(gate(&switch, &config, Place::default()).is_some());
        let search = node(
            "SearchField",
            Some("library.search"),
            Some("Title or author"),
        );
        assert!(gate(&search, &config, Place::default()).is_none());
        assert!(gate(
            &node("Cell", Some("library.book.1"), Some("Dune")),
            &config,
            Place::default()
        )
        .is_none());
    }

    #[test]
    fn on_real_data_an_unknown_control_is_skipped_not_tapped() {
        let config = Config::default();
        let real = |kind: &str, id: Option<&str>, label: &str, place: Place| {
            gate(&node(kind, id, Some(label)), &config, place)
        };
        // A confirmation after "Remove all?": no word list knows "OK" or
        // "Yes", and the allow list doesn't either.
        assert!(real("Button", None, "Done", Place::default()).is_some());
        for label in ["OK", "Yes", "Go"] {
            let why = real("Button", None, label, Place::default()).unwrap();
            assert!(why.starts_with("real data: only navigation"), "{why}");
        }
        assert!(real(
            "Button",
            Some("library.settings"),
            "Settings",
            Place::default()
        )
        .is_some());
        assert!(real("Link", Some("detail.web"), "Website", Place::default()).is_some());
        // Navigation is tapped.
        assert!(real("Cell", Some("library.book.1"), "Dune", Place::default()).is_none());
        assert!(real("Button", None, "Cancel", Place::default()).is_none());
        let back = Place {
            back_button: true,
            ..Place::default()
        };
        assert!(real("Button", None, "Library", back).is_none());
        let tab = Place {
            tab_bar: true,
            ..Place::default()
        };
        assert!(real("Button", Some("tab.activity"), "Activity", tab).is_none());
        // A row whose words say it changes data is still skipped.
        assert!(real(
            "Cell",
            Some("library.continue"),
            "Continue Reading",
            Place::default()
        )
        .is_some());
    }

    #[test]
    fn the_developer_can_declare_controls_safe_on_real_data_but_not_external_ones() {
        let config = Config {
            real_data_safe: vec!["library.settings".into(), "library.share".into()],
            ..Config::default()
        };
        let settings = node("Button", Some("library.settings"), Some("Settings"));
        assert!(gate(&settings, &config, Place::default()).is_none());
        let share = node("Button", Some("library.share"), Some("Share List"));
        assert!(gate(&share, &config, Place::default())
            .unwrap()
            .starts_with("acts outside the phone"));
    }

    #[test]
    fn on_test_data_the_allow_list_does_not_apply() {
        let ok = node("Button", None, Some("OK"));
        assert!(gate(&ok, &config(), Place::default()).is_none());
        let settings = node("Button", Some("library.settings"), Some("Settings"));
        assert!(gate(&settings, &config(), Place::default()).is_none());
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

    /// A feed list whose last control is below the fold until scrolled.
    fn long_list() -> ModelApp {
        let mut model = ModelApp::pocket_library();
        let mut hidden = node("Button", Some("feeds.manage"), Some("Manage Feeds"));
        hidden.visible = false;
        let list = UiNode {
            children: vec![
                node("Cell", Some("feeds.row.1"), Some("Daring Feed")),
                hidden,
            ],
            ..node("Table", None, None)
        };
        model
            .screens
            .insert("library", app(vec![nav("Feeds", vec![]), list]));
        model.screens.insert(
            "manage",
            app(vec![nav(
                "Manage",
                vec![node("Button", None, Some("Feeds"))],
            )]),
        );
        model.taps = HashMap::from([
            (("library", "feeds.manage"), To::Screen("manage")),
            (("manage", "Feeds"), To::Screen("library")),
        ]);
        model
    }

    #[test]
    fn scrolls_a_list_to_reach_controls_below_the_fold() {
        let mut model = long_list();
        let report = run(&config(), &mut model, None, &mut Vec::new()).unwrap();
        assert!(
            model.log.iter().any(|l| l == "scroll down"),
            "{:?}",
            model.log
        );
        assert!(
            model.log.iter().any(|l| l == "tap feeds.manage"),
            "{:?}",
            model.log
        );
        assert!(report
            .screens
            .iter()
            .any(|s| s.title.as_deref() == Some("Manage")));
        // The scroll is part of the way there, as a step `forge ui run` takes.
        let manage = report
            .screens
            .iter()
            .find(|s| s.title.as_deref() == Some("Manage"))
            .unwrap();
        assert!(
            manage
                .route
                .iter()
                .any(|r| r.starts_with("scroll = { kind = \"Table\", index = 0")),
            "{:?}",
            manage.route
        );
        let parsed = crate::focus::parse_route(
            &manage
                .route
                .iter()
                .map(|r| serde_json::json!(r))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(matches!(parsed.first(), Some(Action::Scroll(_, d)) if d == "down"));
    }

    #[test]
    fn scrolling_can_be_turned_off_and_stops_at_the_end_of_a_list() {
        let off = Config {
            scrolls_per_screen: 0,
            ..config()
        };
        let mut model = long_list();
        run(&off, &mut model, None, &mut Vec::new()).unwrap();
        assert!(!model.log.iter().any(|l| l.starts_with("scroll")));
        // With scrolling on, a list that no longer moves is scrolled once
        // more at most, not up to the limit on every visit.
        let many = Config {
            scrolls_per_screen: 5,
            ..config()
        };
        let mut model = long_list();
        run(&many, &mut model, None, &mut Vec::new()).unwrap();
        let scrolls = model.log.iter().filter(|l| l.starts_with("scroll")).count();
        assert!(scrolls <= 3, "{scrolls} scrolls: {:?}", model.log);
    }

    #[test]
    fn in_another_developers_app_the_policy_decides_each_action() {
        // `observe`: launching and reading are allowed, tapping is not.
        let mut gate = Gate::foreign(
            APP,
            policy::Level::Observe,
            Box::new(policy::NoApprover),
            None,
        );
        let mut model = ModelApp::pocket_library();
        let report = run_gated(&config(), &mut model, None, &mut Vec::new(), &mut gate).unwrap();
        assert!(
            !model.log.iter().any(|l| l.starts_with("tap")),
            "{:?}",
            model.log
        );
        let skipped: Vec<&Skipped> = report.screens.iter().flat_map(|s| &s.skipped).collect();
        assert!(
            skipped
                .iter()
                .any(|s| s.reason.starts_with("the any-app policy") && s.reason.contains("operate")),
            "{skipped:#?}"
        );
        // `deny`: not even launched.
        let mut gate = Gate::foreign(APP, policy::Level::Deny, Box::new(policy::NoApprover), None);
        let mut model = ModelApp::pocket_library();
        let refused = run_gated(&config(), &mut model, None, &mut Vec::new(), &mut gate);
        assert!(matches!(refused, Err(UiError::Refused(_))));
        assert!(model.log.is_empty(), "{:?}", model.log);
    }

    #[test]
    fn writes_the_walk_as_a_transition_graph() {
        let dir = tempfile::tempdir().unwrap();
        let (report, _) = explore(&config(), Some(dir.path()));
        let graph: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("graph.json")).unwrap()).unwrap();
        assert_eq!(graph["format"], "forge.explore-graph/1");
        assert_eq!(
            graph["screens"].as_array().unwrap().len(),
            report.screens.len()
        );
        let root = report.screens[0].id.clone();
        let settings = report
            .screens
            .iter()
            .find(|s| s.title.as_deref() == Some("Settings"))
            .unwrap()
            .id
            .clone();
        let edge = graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["from"] == root && e["action"] == "tap id `library.settings`")
            .expect("the settings edge");
        assert_eq!(edge["to"], serde_json::json!([settings]));
        // The crash is an edge too, so a later comparison sees it.
        assert!(graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["to"] == serde_json::json!(["app_closed"])));
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
    fn never_touches_the_keyboard_or_dictation() {
        for dismissable in [true, false] {
            let mut model = ModelApp::pocket_library();
            model.keyboard_dismissable = dismissable;
            let report = run(&config(), &mut model, None, &mut Vec::new()).unwrap();
            assert!(model.dismissals > 0, "typing puts the keyboard away");
            for step in &report.journal {
                for forbidden in [
                    "Next keyboard",
                    "dictation",
                    "Tenor",
                    "gr-button",
                    "label \"c\"",
                ] {
                    assert!(!step.action.contains(forbidden), "{}", step.action);
                }
            }
            for issue in &report.issues {
                for forbidden in ["Key ", "Tone", "Tenor", "Dictate", "Next keyboard"] {
                    assert!(!issue.message.contains(forbidden), "{}", issue.message);
                }
            }
            // The form with and without a keyboard is one screen.
            assert_eq!(report.screens.len(), 5, "dismissable: {dismissable}");
        }
    }

    #[test]
    fn the_keyboard_and_its_panels_are_not_part_of_the_app() {
        let mut tree = ModelApp::pocket_library().screens["form"].clone();
        let without = tree.clone();
        tree.children.push(at(
            node("Button", Some("bookForm.cancel2"), Some("Close")),
            500.0,
        ));
        tree.children.extend(keyboard());
        let part = app_part(&tree);
        let kept: Vec<_> = part.walk().filter_map(|n| n.label.clone()).collect();
        assert!(kept.contains(&"Close".to_owned()), "{kept:?}");
        for gone in ["c", "Tone", "Search Tenor", "Next keyboard", "Dictate"] {
            assert!(!kept.contains(&gone.to_owned()), "{gone} in {kept:?}");
        }
        assert_eq!(app_part(&without), without);
    }

    #[test]
    fn system_services_are_never_started() {
        let config = config();
        for (id, label) in [
            ("dictation", "Dictate"),
            ("GlobeKey", "Next keyboard"),
            ("library.import", "Import EPUB"),
            ("scan.camera", "Scan with Camera"),
        ] {
            let why = gate(
                &node("Button", Some(id), Some(label)),
                &config,
                Place::default(),
            )
            .unwrap_or_default();
            assert!(why.starts_with("starts a system service"), "{id}: {why}");
        }
    }

    #[test]
    fn fields_get_text_that_fits() {
        let field = |id: &str, label: &str, value: Option<&str>| UiNode {
            value: value.map(Into::into),
            ..node("TextField", Some(id), Some(label))
        };
        let fill = |n: &UiNode| filler(n, "Forge");
        assert_eq!(fill(&field("goal.books", "Yearly goal", Some("10"))), "3");
        assert_eq!(fill(&field("x.y", "", Some("12.50"))), "3");
        assert_eq!(fill(&field("bookForm.pages", "Pages", None)), "3");
        assert_eq!(
            fill(&field("account.email", "Email", None)),
            "forge@example.com"
        );
        assert_eq!(
            fill(&field("profile.website", "Website", None)),
            "https://example.com"
        );
        assert_eq!(
            fill(&field("bookForm.title", "Title", Some("Dune"))),
            "Forge"
        );
        // A fixture still wins.
        let config = Config {
            fixtures: HashMap::from([("goal.books".into(), "12".into())]),
            ..config()
        };
        let tree = app(vec![field("goal.books", "Yearly goal", Some("10"))]);
        let (found, _) = candidates(&tree, &config);
        assert_eq!(
            found[0].action,
            Action::Type(Locator::Id("goal.books".into()), "12".into())
        );
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
        let gated = |id: &str, label: &str| {
            gate(
                &node("Button", Some(id), Some(label)),
                &config,
                Place::default(),
            )
        };
        assert!(gated("account.signOut", "Sign Out").is_some());
        assert!(gated("chat.send", "Send").is_some());
        // Tipster's ids are not payments.
        assert!(gated("tip.amount", "Bill amount").is_none());
        assert!(gated("library.add", "Add Book").is_none());
        // Whole words only: "Settings" is not "set", "Postcode" is not "post".
        assert!(gated("form.postcode", "Postcode").is_none());
    }

    fn detail_focus() -> Focus {
        use crate::focus::Link;
        let link = |id: &str, strength, how: &str| Link {
            id: id.trim_end_matches('*').into(),
            prefix: id.ends_with('*'),
            strength,
            file: "examples/pocket-library/Sources/AppHost/BookDetailView.swift".into(),
            how: how.into(),
        };
        Focus {
            links: vec![
                link(
                    "detail.read",
                    Strength::Direct,
                    "declared in BookDetailView.swift",
                ),
                link(
                    "detail.sync",
                    Strength::Direct,
                    "declared in BookDetailView.swift",
                ),
                link(
                    "detail.web",
                    Strength::Direct,
                    "declared in BookDetailView.swift",
                ),
                link(
                    "library.book.*",
                    Strength::Entry,
                    "LibraryView.swift presents BookDetailView",
                ),
            ],
            routes: vec![],
        }
    }

    #[test]
    fn a_focused_walk_reaches_the_changed_screen_first_and_ties_findings_to_the_change() {
        for seed in 1..=8 {
            let plain = Config { seed, ..config() };
            let (plain, _) = explore(&plain, None);
            let focused = Config {
                seed,
                focus: detail_focus(),
                ..config()
            };
            let (report, _) = explore(&focused, None);
            let focus_first = report.focus.as_ref().unwrap()["first_direct_action"]
                .as_u64()
                .unwrap() as usize;
            // Launch, open a book (an entry control), then a detail control.
            assert!(focus_first <= 3, "seed {seed}: {focus_first}");
            let plain_first = plain
                .journal
                .iter()
                .find(|s| s.action.contains("detail."))
                .map(|s| s.step)
                .unwrap();
            assert!(focus_first <= plain_first, "seed {seed}");
        }
        let (report, _) = explore(
            &Config {
                seed: 3,
                focus: detail_focus(),
                ..config()
            },
            None,
        );
        let crash = issue(&report, "app_closed");
        assert_eq!(crash.focus, "direct");
        assert_eq!(crash.confidence, "observed");
        assert_eq!(
            crash.changed_files[0]["file"],
            "examples/pocket-library/Sources/AppHost/BookDetailView.swift"
        );
        assert_eq!(
            crash.route.last().unwrap(),
            r#"tap = { id = "detail.read" }"#
        );
        // An issue away from the change has no tie, and says so.
        let off = report
            .issues
            .iter()
            .find(|i| i.focus == "none")
            .expect("an issue off the focus");
        assert!(off.changed_files.is_empty());
        let focus = report.focus.unwrap();
        assert_eq!(focus["not_reached"], serde_json::json!([]));
        assert!(focus["tried"]["detail.read"].is_number());
    }

    #[test]
    fn the_same_seed_and_focus_walk_the_same_way() {
        let run = || {
            let config = Config {
                seed: 11,
                focus: detail_focus(),
                verify: 2,
                ..config()
            };
            let (report, model) = explore(&config, None);
            (
                report
                    .journal
                    .iter()
                    .map(|s| format!("{} {} {} {}", s.phase, s.from, s.action, s.outcome))
                    .collect::<Vec<_>>(),
                model.log,
            )
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn issues_are_replayed_and_said_to_come_back() {
        let focused = Config {
            seed: 2,
            focus: detail_focus(),
            verify: 3,
            ..config()
        };
        let (report, _) = explore(&focused, None);
        let crash = issue(&report, "app_closed");
        assert_eq!(crash.reproduced, Some(true));
        let opened = issue(&report, "left_app");
        assert_eq!(opened.reproduced, Some(true));
        assert!(report.journal.iter().any(|s| s.phase == "verify"));
        // Audit findings aren't replayed.
        assert!(report
            .issues
            .iter()
            .filter(|i| i.issue == "no_label" || i.issue == "clipped")
            .all(|i| i.reproduced.is_none()));
        // Without a focus or `verify`, nothing is replayed.
        let (plain, _) = explore(&config(), None);
        assert!(plain.issues.iter().all(|i| i.reproduced.is_none()));
        assert!(plain.focus.is_none());
    }

    #[test]
    fn a_later_walk_follows_known_routes_and_rechecks_earlier_issues() {
        let dir = tempfile::tempdir().unwrap();
        let (first, _) = explore(&config(), Some(dir.path()));
        let previous: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("explore.json")).unwrap())
                .unwrap();
        let mut focus = detail_focus();
        focus.routes = crate::focus::routes_from(&previous, &focus, 5);
        assert!(focus.routes.iter().any(|r| r.kind == "recheck"));
        assert!(focus.routes.iter().any(|r| r.kind == "focus_screen"));
        let later = Config {
            seed: 99,
            max_actions: 12,
            focus,
            ..config()
        };
        let (second, model) = explore(&later, None);
        // Rechecks come first: the crash's own replay is the walk's start.
        assert_eq!(second.journal[1].phase, "recheck");
        assert!(model.log.iter().any(|l| l == "tap detail.read"));
        let compared = crate::focus::compare(&previous, "previous/explore.json", &second);
        let still: Vec<&str> = compared["still_present"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["issue"].as_str().unwrap())
            .collect();
        assert!(still.contains(&"app_closed"), "{compared:#}");
        // Twelve actions can't recheck everything the first walk found.
        assert!(
            !compared["not_rechecked"].as_array().unwrap().is_empty()
                || first.issues.len() == still.len()
        );
    }

    #[test]
    fn a_fixed_issue_is_gone_only_when_its_action_was_done_again() {
        let dir = tempfile::tempdir().unwrap();
        explore(&config(), Some(dir.path()));
        let previous: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("explore.json")).unwrap())
                .unwrap();
        // "Fix" the crash: Read now stays on the screen.
        let mut model = ModelApp::pocket_library();
        model.taps.remove(&("detail", "detail.read"));
        let mut focus = detail_focus();
        focus.routes = crate::focus::routes_from(&previous, &focus, 5);
        let later = Config { focus, ..config() };
        let report = run(&later, &mut model, None, &mut Vec::new()).unwrap();
        let compared = crate::focus::compare(&previous, "p", &report);
        let gone: Vec<&str> = compared["gone"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["issue"].as_str().unwrap())
            .collect();
        assert!(gone.contains(&"app_closed"), "{compared:#}");
    }
}
