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

pub mod http;
pub mod phone;
pub mod script;
pub mod tree;
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Locator {
    /// `.accessibilityIdentifier(...)`, matched exactly.
    Id(String),
    /// The accessibility label, matched exactly.
    Label(String),
    /// The accessibility label contains this text.
    LabelContains(String),
}

impl std::fmt::Display for Locator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Locator::Id(id) => write!(f, "id `{id}`"),
            Locator::Label(label) => write!(f, "label \"{label}\""),
            Locator::LabelContains(text) => write!(f, "label containing \"{text}\""),
        }
    }
}

impl Locator {
    /// The NSPredicate WebDriverAgent evaluates for this locator.
    pub fn predicate(&self) -> String {
        match self {
            Locator::Id(id) => format!("identifier == {}", quote(id)),
            Locator::Label(label) => format!("label == {}", quote(label)),
            Locator::LabelContains(text) => format!("label CONTAINS {}", quote(text)),
        }
    }

    /// Whether a node of a UI tree matches, using the same rules as
    /// [`Locator::predicate`]. Used by fakes and failure messages.
    pub fn matches(&self, node: &UiNode) -> bool {
        match self {
            Locator::Id(id) => node.identifier.as_deref() == Some(id.as_str()),
            Locator::Label(label) => node.label.as_deref() == Some(label.as_str()),
            Locator::LabelContains(text) => node
                .label
                .as_deref()
                .is_some_and(|label| label.contains(text.as_str())),
        }
    }
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
        if let Some(first) = driver.find_all(locator)?.into_iter().next() {
            return Ok(first);
        }
        if Instant::now() >= deadline {
            return Err(UiError::NotFound(locator.clone()));
        }
        std::thread::sleep(POLL);
    }
}

/// Waits up to `timeout` until nothing matches `locator`.
pub fn wait_gone(driver: &mut dyn Driver, locator: &Locator, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if driver.find_all(locator)?.is_empty() {
            return Ok(());
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

    #[derive(Default)]
    pub struct FakeDriver {
        pub screen: UiNode,
        /// What the driver was asked to do, one line per call.
        pub calls: Vec<String>,
        /// Replaces `screen` after the next tap on an element with this id.
        pub on_tap: Vec<(String, UiNode)>,
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
            Ok(())
        }
        fn activate(&mut self, bundle_id: &str) -> Result<()> {
            self.calls.push(format!("activate {bundle_id}"));
            Ok(())
        }
        fn terminate(&mut self, bundle_id: &str) -> Result<()> {
            self.calls.push(format!("terminate {bundle_id}"));
            Ok(())
        }
        fn background(&mut self, seconds: f64) -> Result<()> {
            self.calls.push(format!("background {seconds}"));
            Ok(())
        }
        fn find_all(&mut self, locator: &Locator) -> Result<Vec<ElementRef>> {
            Ok(self
                .screen
                .walk()
                .enumerate()
                .filter(|(_, node)| locator.matches(node))
                .map(|(i, _)| ElementRef(i.to_string()))
                .collect())
        }
        fn tap(&mut self, element: &ElementRef) -> Result<()> {
            let id = self.node(element)?.identifier.clone();
            self.calls
                .push(format!("tap {}", id.as_deref().unwrap_or("?")));
            if let Some(id) = id {
                if let Some(pos) = self.on_tap.iter().position(|(on, _)| *on == id) {
                    self.screen = self.on_tap.remove(pos).1;
                }
            }
            Ok(())
        }
        fn type_text(&mut self, element: &ElementRef, text: &str) -> Result<()> {
            let node = self.node(element)?;
            let id = node.identifier.clone();
            node.value = Some(format!("{}{text}", node.value.clone().unwrap_or_default()));
            self.calls
                .push(format!("type {} {text:?}", id.as_deref().unwrap_or("?")));
            Ok(())
        }
        fn clear(&mut self, element: &ElementRef) -> Result<()> {
            self.node(element)?.value = None;
            Ok(())
        }
        fn attribute(&mut self, element: &ElementRef, name: &str) -> Result<Option<String>> {
            let node = self.node(element)?;
            Ok(match name {
                "label" => node.label.clone(),
                "value" => node.value.clone(),
                "identifier" | "name" => node.identifier.clone(),
                "enabled" => Some(node.enabled.to_string()),
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicates_quote_and_escape() {
        assert_eq!(
            Locator::Id("library.add".into()).predicate(),
            r#"identifier == "library.add""#
        );
        assert_eq!(
            Locator::LabelContains(r#"say "hi" \o/"#.into()).predicate(),
            r#"label CONTAINS "say \"hi\" \\o/""#
        );
    }

    #[test]
    fn wait_for_reports_the_locator() {
        let mut driver = fake::FakeDriver::new(UiNode::default());
        let err = wait_for(&mut driver, &Locator::Id("nope".into()), Duration::ZERO).unwrap_err();
        assert_eq!(err.to_string(), "no element matches id `nope`");
    }
}
