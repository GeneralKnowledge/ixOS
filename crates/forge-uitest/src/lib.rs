//! Forge UI Testing: drive an app on a connected iPhone from Linux.
//!
//! The phone side is Apple's XCTest UI automation, hosted by a
//! WebDriverAgent runner that `xtool install` signed with the developer's
//! Apple ID. The Linux side starts that runner through `testmanagerd`, talks
//! WebDriver to it over usbmuxd, and exposes a small, semantic API:
//! elements are found by accessibility identifier or label, never by screen
//! coordinates. See `docs/forge-ui-testing.md`.
//!
//! - [`Driver`] is what tests use. [`wda::WdaDriver`] implements it over any
//!   [`http::HttpTransport`]; [`phone::Phone`] supplies the real transport,
//!   mounts the Developer Disk Image and starts the runner.
//! - [`script`] runs steps from a TOML file against any [`Driver`].
//! - [`policy`] decides what a run may do in an app Forge didn't build.
//! - [`visual`] and [`baseline`] compare a run's screenshots with approved
//!   ones (visual regression), on Linux.

pub mod baseline;
pub mod explore;
pub mod http;
pub mod phone;
pub mod policy;
pub mod script;
pub mod session;
pub mod tree;
pub mod visual;
pub mod wda;

use std::time::{Duration, Instant};

pub use tree::UiNode;

#[derive(Debug, thiserror::Error)]
pub enum UiError {
    #[error("no element matches {0}")]
    NotFound(Locator),
    #[error("{locator}: expected {expected}, found {found}")]
    Mismatch {
        locator: Locator,
        expected: String,
        found: String,
    },
    /// The element went away between finding it and acting on it (the
    /// screen re-rendered). Worth finding again once.
    #[error("element went stale: {0}")]
    Stale(String),
    /// Another app is in front, so acting would touch the wrong app.
    #[error("{expected} is not in front ({found} is); refusing to act on another app")]
    WrongApp { expected: String, found: String },
    /// The device-control policy said no (another developer's app; see
    /// [`policy`]). Never retried.
    #[error("refused by the device-control policy: {0}")]
    Refused(String),
    #[error("WebDriverAgent: {0}")]
    Wda(String),
    #[error("{0}")]
    Device(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, UiError>;

/// How a test names an element. Identifiers first: they are stable across
/// copy and layout changes. Labels are what VoiceOver reads, so finding by
/// label also checks that the element has one. There is deliberately no
/// coordinate locator.
///
/// [`Locator::Kind`] and [`Locator::Near`] are for apps Forge didn't build,
/// whose controls often have neither an identifier nor a label (a chat app's
/// message box). Forge's own apps give every control an identifier
/// (AGENTS.md rule 4) and their tests use `Id`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Locator {
    /// `.accessibilityIdentifier(...)`, matched exactly.
    Id(String),
    /// The accessibility label, matched exactly.
    Label(String),
    /// The accessibility label contains this text.
    LabelContains(String),
    /// Elements of one type (`Button`, `TextView`…), optionally with this
    /// exact label. With `index`, only the `index`th of them (from 0, in
    /// tree order, visible or not); without, the best of them, as for any
    /// locator that matches several.
    Kind {
        kind: String,
        label: Option<String>,
        index: Option<usize>,
    },
    /// The control nearest a piece of text on screen: a field whose
    /// placeholder is a separate text laid over it, or a field under or
    /// after its caption. `kind` is the control's type; without it, any
    /// text input. Found from the tree, so it needs element frames.
    Near { text: String, kind: Option<String> },
}

impl std::fmt::Display for Locator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Locator::Id(id) => write!(f, "id `{id}`"),
            Locator::Label(label) => write!(f, "label \"{label}\""),
            Locator::LabelContains(text) => write!(f, "label containing \"{text}\""),
            Locator::Kind { kind, label, index } => {
                match index {
                    Some(i) => write!(f, "{kind} #{i}")?,
                    None => write!(f, "{kind}")?,
                }
                if let Some(label) = label {
                    write!(f, " labelled \"{label}\"")?;
                }
                Ok(())
            }
            Locator::Near { text, kind } => write!(
                f,
                "{} near \"{text}\"",
                kind.as_deref().unwrap_or("text field")
            ),
        }
    }
}

/// `TextView` from `TextView` or `XCUIElementTypeTextView`.
fn short_kind(kind: &str) -> &str {
    kind.strip_prefix("XCUIElementType").unwrap_or(kind)
}

/// How far (in points) a control may be from its caption for
/// [`Locator::Near`]: further than this and it belongs to something else.
const NEAR_MAX: f64 = 120.0;

impl Locator {
    /// The NSPredicate WebDriverAgent evaluates for this locator.
    /// WebDriverAgent 16 rejects `identifier` ("The attribute 'identifier'
    /// is unknown", seen on iOS 26.3): it exposes the accessibility
    /// identifier as `name`. `name` falls back to the label for an element
    /// without an identifier, so a label that looks like an id could match
    /// too; ids use `screen.element` dots, which labels don't.
    ///
    /// A [`Locator::Near`] has none (it needs the tree's frames): its
    /// predicate is that of its kind, and [`Locator::find_in`] picks.
    pub fn predicate(&self) -> String {
        match self {
            Locator::Id(id) => format!("name == {}", quote(id)),
            Locator::Label(label) => format!("label == {}", quote(label)),
            Locator::LabelContains(text) => format!("label CONTAINS {}", quote(text)),
            Locator::Kind { kind, label, .. } => {
                let kind = format!(
                    "type == {}",
                    quote(&format!("XCUIElementType{}", short_kind(kind)))
                );
                match label {
                    Some(label) => format!("{kind} AND label == {}", quote(label)),
                    None => kind,
                }
            }
            Locator::Near { kind, .. } => match kind {
                Some(kind) => Locator::Kind {
                    kind: kind.clone(),
                    label: None,
                    index: None,
                }
                .predicate(),
                None => tree::TEXT_INPUTS
                    .iter()
                    .map(|k| format!("type == \"XCUIElementType{k}\""))
                    .collect::<Vec<_>>()
                    .join(" OR "),
            },
        }
    }

    /// Whether a node of a UI tree matches by itself, using the same rules
    /// as [`Locator::predicate`] (ignoring a `Kind`'s index). A `Near`
    /// matches its kind; which one is meant takes the whole tree
    /// ([`Locator::find_in`]).
    pub fn matches(&self, node: &UiNode) -> bool {
        match self {
            Locator::Id(id) => node.identifier.as_deref() == Some(id.as_str()),
            Locator::Label(label) => node.label.as_deref() == Some(label.as_str()),
            Locator::LabelContains(text) => node
                .label
                .as_deref()
                .is_some_and(|label| label.contains(text.as_str())),
            Locator::Kind { kind, label, .. } => {
                node.kind == short_kind(kind)
                    && label
                        .as_deref()
                        .is_none_or(|label| node.label.as_deref() == Some(label))
            }
            Locator::Near { kind, .. } => match kind {
                Some(kind) => node.kind == short_kind(kind),
                None => tree::TEXT_INPUTS.contains(&node.kind.as_str()),
            },
        }
    }

    /// The matching nodes of a whole tree, with their positions in
    /// [`UiNode::walk`] order: every match, except that a `Kind` with an
    /// index gives only that one and a `Near` gives only the nearest.
    pub fn find_in<'a>(&self, tree: &'a UiNode) -> Vec<(usize, &'a UiNode)> {
        let all = tree
            .walk()
            .enumerate()
            .filter(|(_, node)| self.matches(node));
        match self {
            Locator::Kind {
                index: Some(index), ..
            } => all.skip(*index).take(1).collect(),
            Locator::Near { text, .. } => {
                let candidates: Vec<_> = all.collect();
                nearest(tree, text, &candidates).into_iter().collect()
            }
            _ => all.collect(),
        }
    }

    /// Whether finding this needs the tree (and its frames), not just a
    /// query.
    pub fn needs_tree(&self) -> bool {
        matches!(self, Locator::Near { .. })
    }

    /// The fields of a `forge ui run` target for this locator, as TOML
    /// (`kind = "TextView", index = 0`).
    pub fn toml_fields(&self) -> String {
        match self {
            Locator::Id(id) => format!("id = {}", quote(id)),
            Locator::Label(label) => format!("label = {}", quote(label)),
            Locator::LabelContains(text) => format!("label_contains = {}", quote(text)),
            Locator::Kind { kind, label, index } => {
                let mut out = format!("kind = {}", quote(short_kind(kind)));
                if let Some(label) = label {
                    out.push_str(&format!(", label = {}", quote(label)));
                }
                if let Some(index) = index {
                    out.push_str(&format!(", index = {index}"));
                }
                out
            }
            Locator::Near { text, kind } => {
                let mut out = format!("near = {}", quote(text));
                if let Some(kind) = kind {
                    out.push_str(&format!(", kind = {}", quote(short_kind(kind))));
                }
                out
            }
        }
    }
}

/// Of `candidates`, the one nearest the first element showing `text`
/// (its label, or failing that its value; exactly, or failing that as part
/// of it). A candidate whose frame holds the text's centre (a placeholder
/// laid over its field) is nearest; then one below the text (a caption
/// above its field) or after it on the same line, by the gap between them,
/// up to [`NEAR_MAX`]. Elements without frames are never near anything.
fn nearest<'a>(
    tree: &'a UiNode,
    text: &str,
    candidates: &[(usize, &'a UiNode)],
) -> Option<(usize, &'a UiNode)> {
    let shows = |node: &UiNode, exact: bool| {
        [node.label.as_deref(), node.value.as_deref()]
            .into_iter()
            .flatten()
            .any(|t| if exact { t == text } else { t.contains(text) })
    };
    let anchor = [true, false].into_iter().find_map(|exact| {
        tree.walk()
            .enumerate()
            .filter(|(_, n)| n.kind != "Application" && n.visible)
            .find(|(_, n)| n.frame.is_some() && shows(n, exact))
    })?;
    let (anchor_at, anchor) = anchor;
    let a = anchor.frame?;
    let (cx, cy) = (a.x + a.width / 2.0, a.y + a.height / 2.0);
    let overlaps = |lo1: f64, len1: f64, lo2: f64, len2: f64| lo1 < lo2 + len2 && lo2 < lo1 + len1;
    let mut best: Option<(f64, usize, &UiNode)> = None;
    for &(at, node) in candidates {
        if at == anchor_at {
            continue;
        }
        let Some(f) = node.frame else { continue };
        let gap = if f.x <= cx && cx <= f.x + f.width && f.y <= cy && cy <= f.y + f.height {
            0.0
        } else if f.y >= a.y + a.height - 1.0 && overlaps(f.x, f.width, a.x, a.width) {
            f.y - (a.y + a.height)
        } else if f.x >= a.x + a.width - 1.0 && overlaps(f.y, f.height, a.y, a.height) {
            f.x - (a.x + a.width)
        } else {
            continue;
        };
        let gap = gap.max(0.0);
        if gap <= NEAR_MAX && best.is_none_or(|(top, _, _)| gap < top) {
            best = Some((gap, at, node));
        }
    }
    best.map(|(_, at, node)| (at, node))
}

/// A double-quoted NSPredicate string literal.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        if ch == '"' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

/// An element the driver found; valid until the screen changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementRef(pub String);

/// Everything a UI test does to the phone. Blocking; every call is one
/// round trip, and [`wait_for`] adds polling on top.
pub trait Driver {
    /// Launch an installed app (restarting it if it is running), with
    /// optional launch arguments.
    fn launch(&mut self, bundle_id: &str, args: &[String]) -> Result<()>;
    /// Bring a running app to the front without restarting it.
    fn activate(&mut self, bundle_id: &str) -> Result<()>;
    /// Stop an app.
    fn terminate(&mut self, bundle_id: &str) -> Result<()>;
    /// Send the app on screen to the background for `seconds`, then bring it back.
    fn background(&mut self, seconds: f64) -> Result<()>;
    /// Scroll: `direction` is `up`, `down`, `left` or `right` to scroll the
    /// scroll view `element`, or `visible` to scroll until `element` itself
    /// is on screen.
    fn scroll(&mut self, element: &ElementRef, direction: &str) -> Result<()>;
    /// Every element matching the locator, visible or not, in tree order.
    fn find_all(&mut self, locator: &Locator) -> Result<Vec<ElementRef>>;
    /// A real tap on the element's centre, as a finger would.
    fn tap(&mut self, element: &ElementRef) -> Result<()>;
    /// Focus the element and type into it with the keyboard.
    fn type_text(&mut self, element: &ElementRef, text: &str) -> Result<()>;
    /// Empty a text field.
    fn clear(&mut self, element: &ElementRef) -> Result<()>;
    /// An accessibility attribute: `label`, `value`, `name`, `enabled`, …
    fn attribute(&mut self, element: &ElementRef, name: &str) -> Result<Option<String>>;
    /// The accessibility tree of the app on screen.
    fn tree(&mut self) -> Result<UiNode>;
    /// A PNG of the whole screen.
    fn screenshot(&mut self) -> Result<Vec<u8>>;
    /// The bundle id of the app in front, if the phone says.
    fn foreground(&mut self) -> Result<Option<String>>;
}

/// Waits up to `timeout` for `bundle_id` to be the app in front, and fails
/// with [`UiError::WrongApp`] otherwise. Every tap, typing, expectation and
/// tree read checks this first, so a test never acts on whatever other app
/// happens to be open.
pub fn ensure_front(driver: &mut dyn Driver, bundle_id: &str, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let front = driver.foreground()?;
        if front.as_deref() == Some(bundle_id) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(UiError::WrongApp {
                expected: bundle_id.to_owned(),
                found: front.unwrap_or_else(|| "no app".into()),
            });
        }
        std::thread::sleep(POLL);
    }
}

/// How often [`wait_for`] asks again.
pub const POLL: Duration = Duration::from_millis(250);

/// The first element matching `locator`, waiting up to `timeout` for it to
/// appear. Tests never sleep: they wait for what they expect.
pub fn wait_for(
    driver: &mut dyn Driver,
    locator: &Locator,
    timeout: Duration,
) -> Result<ElementRef> {
    let deadline = Instant::now() + timeout;
    loop {
        // A search can go stale when the screen changes under it (and a
        // `Near` reads the tree, then queries): ask again until the timeout.
        let found = match driver.find_all(locator) {
            Err(UiError::Stale(_)) if Instant::now() < deadline => {
                std::thread::sleep(POLL);
                continue;
            }
            other => other?,
        };
        if !found.is_empty() {
            return pick(driver, found);
        }
        if Instant::now() >= deadline {
            return Err(UiError::NotFound(locator.clone()));
        }
        std::thread::sleep(POLL);
    }
}

/// Of several matches, the one a tap would reach: SwiftUI can put a
/// modifier's identifier on a wrapper as well as on the control (Tipster's
/// keyboard toolbar: an `Other` and a `Button`, both `tip.keyboardDone`).
/// Ranked by: hittable and a control (Button, field…), then hittable, then
/// a control, then the first match.
fn pick(driver: &mut dyn Driver, found: Vec<ElementRef>) -> Result<ElementRef> {
    if found.len() == 1 {
        return Ok(found.into_iter().next().expect("one"));
    }
    // (score, index) of the best match that answered.
    let mut best: Option<(usize, usize)> = None;
    let mut seen = Vec::new();
    let mut stale = None;
    for (i, element) in found.iter().enumerate() {
        // A copy that goes away while the others are read (SwiftUI
        // re-rendered) doesn't make the rest unusable: skip it.
        let read = driver.attribute(element, "type").and_then(|kind| {
            let hittable = driver.attribute(element, "hittable")?;
            Ok((
                kind.unwrap_or_default(),
                hittable.as_deref() == Some("true"),
            ))
        });
        let (kind, hittable) = match read {
            Ok(read) => read,
            Err(UiError::Stale(why)) => {
                seen.push("(gone)".to_owned());
                stale = Some(why);
                continue;
            }
            Err(e) => return Err(e),
        };
        let score = 2 * usize::from(hittable) + usize::from(tree::is_interactive_kind(&kind));
        let kind = kind
            .strip_prefix("XCUIElementType")
            .unwrap_or(&kind)
            .to_owned();
        seen.push(if hittable {
            kind
        } else {
            format!("{kind} (not hittable)")
        });
        if best.is_none_or(|(top, _)| score > top) {
            best = Some((score, i));
        }
    }
    let Some((_, index)) = best else {
        // Every copy went away: the caller finds again.
        return Err(UiError::Stale(stale.unwrap_or_default()));
    };
    // On stderr, so a test log shows which element a duplicate id reached.
    eprintln!(
        "          {} elements match: {}; using #{}",
        found.len(),
        seen.join(", "),
        index + 1
    );
    Ok(found[index].clone())
}

/// Waits up to `timeout` until nothing matches `locator`.
pub fn wait_gone(driver: &mut dyn Driver, locator: &Locator, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let mut stale_before = false;
    loop {
        match driver.find_all(locator) {
            Ok(found) if found.is_empty() => return Ok(()),
            Ok(_) => stale_before = false,
            // WDA can answer a search for an element that just went away
            // with "stale element reference … No matches found" (seen on
            // iOS 26.3 right after a keyboard closed). Twice in a row means
            // it is gone; once, ask again.
            Err(UiError::Stale(_)) if stale_before => return Ok(()),
            Err(UiError::Stale(_)) => {
                stale_before = true;
                std::thread::sleep(POLL);
                continue;
            }
            Err(e) => return Err(e),
        }
        if Instant::now() >= deadline {
            return Err(UiError::Mismatch {
                locator: locator.clone(),
                expected: "no element".into(),
                found: "still on screen".into(),
            });
        }
        std::thread::sleep(POLL);
    }
}

/// A [`Driver`] over a scripted UI tree, for testing code that drives the
/// phone without one. Taps and typing are recorded; `type_text` also sets the
/// node's value, so expectations about typed text can be checked.
pub mod fake {
    use super::*;

    /// What is in front once an app is terminated.
    pub const SPRINGBOARD: &str = "com.apple.springboard";

    #[derive(Default)]
    pub struct FakeDriver {
        pub screen: UiNode,
        /// What the driver was asked to do, one line per call.
        pub calls: Vec<String>,
        /// Replaces `screen` after the next tap on an element with this id.
        pub on_tap: Vec<(String, UiNode)>,
        /// The next this-many taps fail as if the element went stale.
        pub stale_taps: usize,
        /// The next this-many searches fail as if the element went stale.
        pub stale_finds: usize,
        /// The next this-many taps land, then fail as if the element went
        /// stale: WDA's answer when the tap made the element go away.
        pub stale_after_tap: usize,
        /// The next this-many attribute reads fail as if the element went
        /// stale.
        pub stale_reads: usize,
        /// The app in front: set by launch and activate; terminate goes
        /// back to SpringBoard.
        pub front: Option<String>,
        /// Replaces `screen` after the next typing (a Send button that
        /// only shows once there is text).
        pub after_type: Option<UiNode>,
    }

    impl FakeDriver {
        pub fn new(screen: UiNode) -> Self {
            Self {
                screen,
                ..Self::default()
            }
        }

        fn node(&mut self, element: &ElementRef) -> Result<&mut UiNode> {
            let index: usize = element
                .0
                .parse()
                .map_err(|_| UiError::Wda("bad id".into()))?;
            self.screen
                .nth_mut(index)
                .ok_or_else(|| UiError::Wda("stale element".into()))
        }
    }

    impl Driver for FakeDriver {
        fn launch(&mut self, bundle_id: &str, args: &[String]) -> Result<()> {
            self.calls.push(format!("launch {bundle_id} {args:?}"));
            self.front = Some(bundle_id.to_owned());
            Ok(())
        }
        fn activate(&mut self, bundle_id: &str) -> Result<()> {
            self.calls.push(format!("activate {bundle_id}"));
            self.front = Some(bundle_id.to_owned());
            Ok(())
        }
        fn terminate(&mut self, bundle_id: &str) -> Result<()> {
            self.calls.push(format!("terminate {bundle_id}"));
            self.front = Some(SPRINGBOARD.to_owned());
            Ok(())
        }
        fn background(&mut self, seconds: f64) -> Result<()> {
            self.calls.push(format!("background {seconds}"));
            Ok(())
        }
        fn scroll(&mut self, element: &ElementRef, direction: &str) -> Result<()> {
            let id = self.node(element)?.identifier.clone();
            self.calls.push(format!(
                "scroll {} {direction}",
                id.as_deref().unwrap_or("?")
            ));
            Ok(())
        }
        fn find_all(&mut self, locator: &Locator) -> Result<Vec<ElementRef>> {
            if self.stale_finds > 0 {
                self.stale_finds -= 1;
                return Err(UiError::Stale("No matches found".into()));
            }
            Ok(locator
                .find_in(&self.screen)
                .into_iter()
                .map(|(i, _)| ElementRef(i.to_string()))
                .collect())
        }
        fn tap(&mut self, element: &ElementRef) -> Result<()> {
            if self.stale_taps > 0 {
                self.stale_taps -= 1;
                return Err(UiError::Stale("tap".into()));
            }
            let node = self.node(element)?;
            let id = node.identifier.clone();
            // An id as it is; otherwise the label in quotes, or the kind.
            let name = id
                .clone()
                .or_else(|| node.label.as_ref().map(|l| format!("{l:?}")))
                .unwrap_or_else(|| node.kind.clone());
            self.calls.push(format!("tap {name}"));
            if let Some(id) = id {
                if let Some(pos) = self.on_tap.iter().position(|(on, _)| *on == id) {
                    self.screen = self.on_tap.remove(pos).1;
                }
            }
            if self.stale_after_tap > 0 {
                self.stale_after_tap -= 1;
                return Err(UiError::Stale("tap".into()));
            }
            Ok(())
        }
        fn type_text(&mut self, element: &ElementRef, text: &str) -> Result<()> {
            let node = self.node(element)?;
            let id = node.identifier.clone();
            node.value = Some(format!("{}{text}", node.value.clone().unwrap_or_default()));
            let kind = node.kind.clone();
            self.calls
                .push(format!("type {} {text:?}", id.as_deref().unwrap_or(&kind)));
            if let Some(screen) = self.after_type.take() {
                self.screen = screen;
            }
            Ok(())
        }
        fn clear(&mut self, element: &ElementRef) -> Result<()> {
            self.node(element)?.value = None;
            Ok(())
        }
        fn attribute(&mut self, element: &ElementRef, name: &str) -> Result<Option<String>> {
            if self.stale_reads > 0 {
                self.stale_reads -= 1;
                return Err(UiError::Stale("read".into()));
            }
            let node = self.node(element)?;
            Ok(match name {
                "label" => node.label.clone(),
                "value" => node.value.clone(),
                "identifier" | "name" => node.identifier.clone(),
                "enabled" => Some(node.enabled.to_string()),
                "type" => Some(format!("XCUIElementType{}", node.kind)),
                "hittable" => Some((node.visible && node.enabled).to_string()),
                _ => None,
            })
        }
        fn tree(&mut self) -> Result<UiNode> {
            Ok(self.screen.clone())
        }
        fn screenshot(&mut self) -> Result<Vec<u8>> {
            self.calls.push("screenshot".into());
            Ok(b"\x89PNG fake".to_vec())
        }
        fn foreground(&mut self) -> Result<Option<String>> {
            Ok(self.front.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicates_quote_and_escape() {
        assert_eq!(
            Locator::Id("library.add".into()).predicate(),
            r#"name == "library.add""#
        );
        assert_eq!(
            Locator::LabelContains(r#"say "hi" \o/"#.into()).predicate(),
            r#"label CONTAINS "say \"hi\" \\o/""#
        );
    }

    #[test]
    fn a_stale_search_twice_counts_as_gone() {
        let screen = UiNode {
            kind: "Application".into(),
            children: vec![UiNode {
                kind: "Button".into(),
                identifier: Some("tip.keyboardDone".into()),
                ..UiNode::default()
            }],
            ..UiNode::default()
        };
        let done = Locator::Id("tip.keyboardDone".into());
        let mut driver = fake::FakeDriver::new(screen);
        driver.stale_finds = 2;
        assert!(wait_gone(&mut driver, &done, Duration::from_secs(5)).is_ok());
        // Stale once, then found: still on screen.
        driver.stale_finds = 1;
        assert!(wait_gone(&mut driver, &done, Duration::ZERO).is_err());
    }

    #[test]
    fn a_duplicate_that_goes_stale_is_skipped() {
        let button = |kind: &str| UiNode {
            kind: kind.into(),
            identifier: Some("tip.keyboardDone".into()),
            visible: true,
            enabled: true,
            ..UiNode::default()
        };
        let screen = UiNode {
            kind: "Application".into(),
            children: vec![button("Other"), button("Button")],
            ..UiNode::default()
        };
        let done = Locator::Id("tip.keyboardDone".into());
        let mut driver = fake::FakeDriver::new(screen);
        // The wrapper's first read goes stale; the button still answers.
        driver.stale_reads = 1;
        let element = wait_for(&mut driver, &done, Duration::ZERO).unwrap();
        assert_eq!(
            driver.attribute(&element, "type").unwrap().unwrap(),
            "XCUIElementTypeButton"
        );
        // Every copy stale: the caller finds again.
        driver.stale_reads = 4;
        assert!(matches!(
            wait_for(&mut driver, &done, Duration::ZERO),
            Err(UiError::Stale(_))
        ));
    }

    fn at(kind: &str, label: Option<&str>, x: f64, y: f64, w: f64, h: f64) -> UiNode {
        UiNode {
            kind: kind.into(),
            label: label.map(str::to_owned),
            visible: true,
            enabled: true,
            frame: Some(tree::Rect {
                x,
                y,
                width: w,
                height: h,
            }),
            ..UiNode::default()
        }
    }

    /// DeepSeek's composer on Cati (2026-11-08): a TextView with no id and
    /// no label, its placeholder a StaticText laid over it, and a search
    /// field with a caption above it further up.
    pub(crate) fn deepseek() -> UiNode {
        UiNode {
            kind: "Application".into(),
            children: vec![
                at("StaticText", Some("Search chats"), 16.0, 100.0, 200.0, 20.0),
                at("SearchField", None, 16.0, 124.0, 398.0, 36.0),
                at("TextView", None, 16.0, 790.0, 398.0, 56.0),
                at(
                    "StaticText",
                    Some("Message or hold to speak"),
                    22.0,
                    795.0,
                    386.0,
                    45.0,
                ),
                at("Button", Some("Think"), 16.0, 850.0, 80.0, 30.0),
                at("Button", None, 110.0, 850.0, 40.0, 30.0),
            ],
            ..UiNode::default()
        }
    }

    #[test]
    fn finds_by_kind_label_and_index() {
        let screen = deepseek();
        let first = |l: Locator| l.find_in(&screen).first().map(|(i, _)| *i);
        let kind = |kind: &str, label: Option<&str>, index: Option<usize>| Locator::Kind {
            kind: kind.into(),
            label: label.map(str::to_owned),
            index,
        };
        assert_eq!(kind("Button", None, None).find_in(&screen).len(), 2);
        assert_eq!(first(kind("Button", None, Some(1))), Some(6));
        assert_eq!(
            first(kind("XCUIElementTypeButton", Some("Think"), None)),
            Some(5)
        );
        assert_eq!(first(kind("Button", None, Some(2))), None);
        assert_eq!(
            kind("Button", Some("Send"), Some(0)).predicate(),
            r#"type == "XCUIElementTypeButton" AND label == "Send""#
        );
        assert_eq!(kind("Button", None, Some(1)).to_string(), "Button #1");
        assert_eq!(
            kind("Button", Some("Send"), None).toml_fields(),
            r#"kind = "Button", label = "Send""#
        );
    }

    #[test]
    fn finds_the_field_under_a_placeholder_or_caption() {
        let screen = deepseek();
        let near = |text: &str, kind: Option<&str>| {
            Locator::Near {
                text: text.into(),
                kind: kind.map(str::to_owned),
            }
            .find_in(&screen)
            .into_iter()
            .map(|(i, n)| (i, n.kind.clone()))
            .collect::<Vec<_>>()
        };
        // The placeholder lies over the text view.
        assert_eq!(
            near("Message or hold to speak", None),
            [(3, "TextView".into())]
        );
        // Part of the text is enough when nothing shows it exactly.
        assert_eq!(near("hold to speak", None), [(3, "TextView".into())]);
        // A caption above its field.
        assert_eq!(near("Search chats", None), [(2, "SearchField".into())]);
        // The button after "Think" on the same line, by kind.
        assert_eq!(near("Think", Some("Button")), [(6, "Button".into())]);
        // Nothing near, or no such text: nothing found.
        assert!(near("Think", None).is_empty());
        assert!(near("Nowhere", None).is_empty());
        let mut driver = fake::FakeDriver::new(screen);
        let locator = Locator::Near {
            text: "Message or hold to speak".into(),
            kind: None,
        };
        assert_eq!(
            wait_for(&mut driver, &locator, Duration::ZERO).unwrap(),
            ElementRef("3".into())
        );
        assert_eq!(
            locator.to_string(),
            r#"text field near "Message or hold to speak""#
        );
    }

    #[test]
    fn wait_for_reports_the_locator() {
        let mut driver = fake::FakeDriver::new(UiNode::default());
        let err = wait_for(&mut driver, &Locator::Id("nope".into()), Duration::ZERO).unwrap_err();
        assert_eq!(err.to_string(), "no element matches id `nope`");
    }
}
