//! The accessibility tree of the screen, as WebDriverAgent reports it
//! (`GET /source?format=json`), in Forge's own types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// One element of the accessibility tree.
/// Serialises as JSON for agents (`forge --json ui tree`, failure files):
/// empty fields are left out. Reads back what it writes (baselines keep the
/// tree of each approved screenshot).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiNode {
    /// The element type without Apple's prefix: `Button`, `TextField`, `Cell`…
    pub kind: String,
    /// `.accessibilityIdentifier(...)`, for tests only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    /// What VoiceOver reads.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// A field's text, a toggle's state, a slider's position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub enabled: bool,
    pub visible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<Rect>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<UiNode>,
}

/// Element types that take typed text.
pub const TEXT_INPUTS: &[&str] = &["TextField", "SecureTextField", "TextView", "SearchField"];

/// Element types a person can act on; the accessibility audit expects each
/// of them to have a label.
const INTERACTIVE: &[&str] = &[
    "Button",
    "TextField",
    "SecureTextField",
    "TextView",
    "SearchField",
    "Switch",
    "Toggle",
    "Slider",
    "Stepper",
    "Link",
    "SegmentedControl",
    "Picker",
    "MenuItem",
    "Tab",
];

/// Whether an element type (with or without Apple's `XCUIElementType`
/// prefix) is one a person acts on.
pub fn is_interactive_kind(kind: &str) -> bool {
    INTERACTIVE.contains(&kind.strip_prefix("XCUIElementType").unwrap_or(kind))
}

impl UiNode {
    /// Parses WebDriverAgent's JSON source.
    pub fn from_wda(value: &Value) -> UiNode {
        let text = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        let flag = |key: &str, default: bool| match value.get(key) {
            Some(Value::Bool(b)) => *b,
            Some(Value::Number(n)) => n.as_i64() != Some(0),
            Some(Value::String(s)) => s == "1" || s == "true",
            _ => default,
        };
        let kind = text("type").unwrap_or_default();
        let kind = kind
            .strip_prefix("XCUIElementType")
            .unwrap_or(&kind)
            .to_owned();
        let frame = value.get("rect").and_then(|rect| {
            let num = |key: &str| rect.get(key).and_then(Value::as_f64);
            Some(Rect {
                x: num("x")?,
                y: num("y")?,
                width: num("width")?,
                height: num("height")?,
            })
        });
        let value_text = match value.get("value") {
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            Some(Value::Number(n)) => Some(n.to_string()),
            Some(Value::Bool(b)) => Some(b.to_string()),
            _ => None,
        };
        UiNode {
            kind,
            identifier: text("rawIdentifier"),
            label: text("label"),
            value: value_text,
            enabled: flag("isEnabled", true),
            visible: flag("isVisible", true),
            frame,
            children: value
                .get("children")
                .and_then(Value::as_array)
                .map(|children| children.iter().map(UiNode::from_wda).collect())
                .unwrap_or_default(),
        }
    }

    /// Every node, depth first, starting with this one.
    pub fn walk(&self) -> impl Iterator<Item = &UiNode> {
        let mut stack = vec![self];
        std::iter::from_fn(move || {
            let node = stack.pop()?;
            stack.extend(node.children.iter().rev());
            Some(node)
        })
    }

    /// The `index`th node in [`UiNode::walk`] order.
    pub fn nth_mut(&mut self, index: usize) -> Option<&mut UiNode> {
        fn go<'a>(node: &'a mut UiNode, index: &mut usize) -> Option<&'a mut UiNode> {
            if *index == 0 {
                return Some(node);
            }
            *index -= 1;
            for child in &mut node.children {
                if let Some(found) = go(child, index) {
                    return Some(found);
                }
            }
            None
        }
        let mut index = index;
        go(self, &mut index)
    }

    pub fn is_interactive(&self) -> bool {
        is_interactive_kind(&self.kind)
    }

    /// The tree as indented text, one element per line. Containers without
    /// any identifier, label or value are folded away so the meaningful
    /// elements stand out.
    pub fn render(&self) -> String {
        let mut out = String::new();
        self.render_into(&mut out, 0);
        out
    }

    fn render_into(&self, out: &mut String, depth: usize) {
        let meaningful = self.identifier.is_some()
            || self.label.is_some()
            || self.value.is_some()
            || self.is_interactive()
            || depth == 0;
        let child_depth = if meaningful {
            out.push_str(&"  ".repeat(depth));
            out.push_str(&self.summary());
            out.push('\n');
            depth + 1
        } else {
            depth
        };
        for child in &self.children {
            child.render_into(out, child_depth);
        }
    }

    /// `Button id=library.add "Add Book"` style one-liner.
    pub fn summary(&self) -> String {
        let mut line = self.kind.clone();
        if let Some(id) = &self.identifier {
            line.push_str(&format!(" id={id}"));
        }
        if let Some(label) = &self.label {
            line.push_str(&format!(" {label:?}"));
        }
        if let Some(value) = &self.value {
            line.push_str(&format!(" value={value:?}"));
        }
        if !self.enabled {
            line.push_str(" (disabled)");
        }
        // WDA reports the Application element itself as not visible even
        // while it is in front.
        if !self.visible && self.kind != "Application" {
            line.push_str(" (hidden)");
        }
        line
    }

    /// What a test could target on this screen: visible elements that have
    /// an identifier or can be acted on, one line each, at most `limit`.
    /// Shown when a step can't find its element.
    /// Elements whose identifier, label or value contains `find` (any
    /// case), only visible ones when `visible`, at most `limit`, in tree
    /// order: `forge ui tree --find`.
    pub fn matching(&self, find: Option<&str>, visible: bool, limit: usize) -> Vec<&UiNode> {
        let find = find.map(str::to_lowercase);
        self.walk()
            .filter(|n| !visible || n.visible)
            .filter(|n| match &find {
                None => n.identifier.is_some() || n.label.is_some() || n.is_interactive(),
                Some(f) => [&n.identifier, &n.label, &n.value]
                    .into_iter()
                    .flatten()
                    .any(|t| t.to_lowercase().contains(f.as_str())),
            })
            .take(limit)
            .collect()
    }

    pub fn on_screen(&self, limit: usize) -> Vec<String> {
        self.walk()
            .filter(|n| n.visible && (n.identifier.is_some() || n.is_interactive()))
            .take(limit)
            .map(UiNode::summary)
            .collect()
    }

    /// Visible elements with an identifier, leaving out those inside an
    /// element with the same identifier.
    fn outermost_with_identifier(&self) -> Vec<&UiNode> {
        fn go<'a>(node: &'a UiNode, inside: Option<&str>, out: &mut Vec<&'a UiNode>) {
            let id = node.identifier.as_deref();
            let nested = id.is_some() && id == inside;
            if node.visible && id.is_some() && !nested {
                out.push(node);
            }
            let inside = if node.visible && id.is_some() {
                id
            } else {
                inside
            };
            for child in &node.children {
                go(child, inside, out);
            }
        }
        let mut out = Vec::new();
        go(self, None, &mut out);
        out
    }

    /// Visible interactive elements that VoiceOver has no name for, and
    /// those a test cannot find by identifier. The full audit is M3; this is
    /// the part every screen needs.
    pub fn audit(&self) -> Vec<Finding> {
        let mut findings = Vec::new();
        // One identifier on two visible elements: a test that finds it may
        // get the wrong one. SwiftUI also puts a control's identifier on
        // wrappers around it (a toolbar item, a keyboard toolbar); the app
        // can't avoid that and `forge ui` taps the control, so an element
        // inside another with the same identifier is not a duplicate.
        let mut seen = std::collections::HashSet::new();
        for node in self.outermost_with_identifier() {
            if let Some(id) = &node.identifier {
                if !seen.insert(id.as_str()) {
                    findings.push(Finding::new("duplicate_identifier", "warning", node));
                }
            }
        }
        // A SwiftUI Toggle is a labelled Switch with UIKit's switch inside
        // it, unlabelled; VoiceOver reads the outer one only.
        let mut inner: Vec<*const UiNode> = Vec::new();
        for outer in self
            .walk()
            .filter(|n| n.is_interactive() && n.label.is_some())
        {
            for child in outer.walk().skip(1) {
                if child.kind == outer.kind && child.label.is_none() {
                    inner.push(child);
                }
            }
        }
        for node in self
            .walk()
            .filter(|n| n.visible && n.is_interactive())
            .filter(|n| !inner.contains(&(*n as *const UiNode)))
        {
            if node.label.is_none() {
                findings.push(Finding::new("no_label", "error", node));
            }
            if node.identifier.is_none() {
                findings.push(Finding::new("no_identifier", "warning", node));
            }
        }
        findings.extend(self.small_targets(&inner));
        findings.extend(self.clipped());
        findings
    }

    /// Controls a finger can hardly hit: smaller than 44 x 44 pt (Apple's
    /// minimum), measured on the tree. A control counts at the size of the
    /// row around it when that row holds nothing else to tap (a Form row
    /// is tapped anywhere). Left out: controls in navigation, tool and tab
    /// bars (the system widens their touch area) and links inside a
    /// sentence (text links are exempt in WCAG 2.5.8).
    fn small_targets(&self, inner: &[*const UiNode]) -> Vec<Finding> {
        let mut out = Vec::new();
        let mut path = Vec::new();
        self.small_targets_into(&mut path, inner, &mut out);
        out
    }

    fn small_targets_into<'a>(
        &'a self,
        path: &mut Vec<&'a UiNode>,
        inner: &[*const UiNode],
        out: &mut Vec<Finding>,
    ) {
        if self.visible
            && self.enabled
            && self.is_interactive()
            && !inner.contains(&(self as *const UiNode))
        {
            if let Some(frame) = self.frame.filter(|f| f.width > 0.0 && f.height > 0.0) {
                let in_bar = path
                    .iter()
                    .any(|a| matches!(a.kind.as_str(), "NavigationBar" | "Toolbar" | "TabBar"));
                let inline = self.kind == "Link"
                    && path
                        .iter()
                        .any(|a| matches!(a.kind.as_str(), "StaticText" | "TextView"));
                let target = row_around(self, frame, path);
                if !in_bar
                    && !inline
                    && (target.width < MIN_TARGET - SLACK || target.height < MIN_TARGET - SLACK)
                {
                    let mut finding = Finding::new("small_tap_target", "warning", self);
                    finding.detail = Some(format!(
                        "{} x {} pt, under 44 x 44",
                        target.width.round(),
                        target.height.round()
                    ));
                    out.push(finding);
                }
            }
        }
        path.push(self);
        for child in &self.children {
            child.small_targets_into(path, inner, out);
        }
        path.pop();
    }

    /// Text cut off, measured on the tree: a visible text or labelled
    /// control running past the left or right edge of the screen, or two
    /// texts drawn over each other. Both are what a larger text size breaks
    /// first. Text truncated with "…" inside its own frame isn't visible
    /// here: the tree keeps the full label.
    fn clipped(&self) -> Vec<Finding> {
        let mut out = Vec::new();
        let screen = self.frame.filter(|f| f.width > 0.0);
        let shown = |n: &&UiNode| {
            n.visible
                && n.label.is_some()
                && n.frame.is_some_and(|f| f.width > 0.0 && f.height > 0.0)
        };
        if let Some(screen) = screen {
            for node in self
                .walk()
                .filter(shown)
                .filter(|n| n.is_interactive() || n.kind == "StaticText")
            {
                let f = node.frame.expect("shown");
                let on_screen = f.x < screen.x + screen.width && f.x + f.width > screen.x;
                let side = if f.x < screen.x - 1.0 {
                    Some("left")
                } else if f.x + f.width > screen.x + screen.width + 1.0 {
                    Some("right")
                } else {
                    None
                };
                if let (true, Some(side)) = (on_screen, side) {
                    let mut finding = Finding::new("clipped", "warning", node);
                    finding.detail = Some(format!("runs off the {side} side of the screen"));
                    out.push(finding);
                }
            }
        }
        // Texts over texts. A text inside another (a row's label and its
        // parts) is not an overlap, and neither is a list's content
        // scrolling under a bar that stays put: only texts in the same
        // scrolling container (or both outside any) are compared.
        type Text<'a> = (&'a UiNode, Vec<&'a UiNode>);
        let mut texts: Vec<Text> = Vec::new();
        fn collect<'a>(node: &'a UiNode, path: &mut Vec<&'a UiNode>, out: &mut Vec<Text<'a>>) {
            if node.kind == "StaticText"
                && node.visible
                && node.label.is_some()
                && node.frame.is_some_and(|f| f.width > 0.0 && f.height > 0.0)
            {
                out.push((node, path.clone()));
            }
            path.push(node);
            for child in &node.children {
                collect(child, path, out);
            }
            path.pop();
        }
        collect(self, &mut Vec::new(), &mut texts);
        let mut reported: Vec<&UiNode> = Vec::new();
        let has = |path: &[&UiNode], node: &UiNode| path.iter().any(|n| std::ptr::eq(*n, node));
        for (i, (a, a_path)) in texts.iter().enumerate() {
            for (b, b_path) in texts.iter().skip(i + 1) {
                let same_scroller = match (scroller(a_path), scroller(b_path)) {
                    (None, None) => true,
                    (Some(x), Some(y)) => std::ptr::eq(x, y),
                    _ => false,
                };
                if has(a_path, b) || has(b_path, a) || has(&reported, b) || !same_scroller {
                    continue;
                }
                let (fa, fb) = (a.frame.expect("text"), b.frame.expect("text"));
                let w = (fa.x + fa.width).min(fb.x + fb.width) - fa.x.max(fb.x);
                let h = (fa.y + fa.height).min(fb.y + fb.height) - fa.y.max(fb.y);
                let smaller = (fa.width * fa.height).min(fb.width * fb.height);
                // A quarter of the smaller text, and more than a pixel row:
                // line spacing makes neighbours touch.
                if w > 1.0 && h > 2.0 && w * h > smaller * 0.25 {
                    let mut finding = Finding::new("clipped", "warning", b);
                    finding.detail = Some(format!("drawn over {}", a.summary()));
                    out.push(finding);
                    reported.push(b);
                }
            }
        }
        out
    }
}

/// The nearest scrolling container in a path from the root.
fn scroller<'a>(path: &[&'a UiNode]) -> Option<&'a UiNode> {
    path.iter()
        .rev()
        .copied()
        .find(|n| matches!(n.kind.as_str(), "ScrollView" | "Table" | "CollectionView"))
}

/// Apple's minimum tap target, in points.
const MIN_TARGET: f64 = 44.0;
/// How far under [`MIN_TARGET`] still counts as meeting it. A system list
/// row is 44 pt with its separator, and the tree reports its cell as 43 pt
/// (Good Dog's picker options on Cati, 2026-11-08): Apple's own rows are
/// not small targets.
const SLACK: f64 = 1.0;

/// The area a finger can hit for `node`: its own frame, or the row around
/// it when that row holds no other control and hugs it (at most 24 pt
/// around it on each side, so a screen-sized container doesn't count).
fn row_around(node: &UiNode, frame: Rect, path: &[&UiNode]) -> Rect {
    const HUG: f64 = 24.0;
    let mut best = frame;
    for ancestor in path.iter().rev() {
        let Some(a) = ancestor.frame else { break };
        let hugs = a.x <= frame.x + 0.5
            && a.y <= frame.y + 0.5
            && a.x + a.width >= frame.x + frame.width - 0.5
            && a.y + a.height >= frame.y + frame.height - 0.5
            && frame.x - a.x <= HUG
            && frame.y - a.y <= HUG
            && (a.x + a.width) - (frame.x + frame.width) <= HUG
            && (a.y + a.height) - (frame.y + frame.height) <= HUG;
        let alone = ancestor
            .walk()
            .filter(|n| n.visible && n.is_interactive())
            .all(|n| {
                std::ptr::eq(n, node)
                    || n.walk().any(|d| std::ptr::eq(d, node))
                    || node.walk().any(|d| std::ptr::eq(d, n))
            });
        if !hugs || !alone {
            break;
        }
        best = a;
    }
    best
}

/// One accessibility audit result. `no_label` is an accessibility error
/// (VoiceOver has nothing to read); `no_identifier` is a test-robustness
/// warning (tests fall back to the label).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    pub issue: &'static str,
    pub severity: &'static str,
    pub element: ElementSummary,
    /// What was measured, for `small_tap_target` and `clipped`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl Finding {
    fn new(issue: &'static str, severity: &'static str, node: &UiNode) -> Finding {
        Finding {
            issue,
            severity,
            element: ElementSummary {
                kind: node.kind.clone(),
                identifier: node.identifier.clone(),
                label: node.label.clone(),
                summary: node.summary(),
            },
            detail: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ElementSummary {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The element as `forge ui tree` prints it.
    pub summary: String,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = match self.issue {
            "no_label" => "no accessibility label",
            "no_identifier" => "no identifier (tests fall back to the label)",
            "duplicate_identifier" => "identifier used by more than one element",
            "small_tap_target" => "tap target too small",
            "clipped" => "text cut off",
            other => other,
        };
        match &self.detail {
            Some(detail) => write!(f, "{what} ({detail}): {}", self.element.summary),
            None => write!(f, "{what}: {}", self.element.summary),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "type": "XCUIElementTypeApplication", "label": "Pocket Library", "isEnabled": "1",
            "rect": {"x": 0, "y": 0, "width": 430, "height": 932},
            "children": [
                {"type": "XCUIElementTypeOther", "children": [
                    {"type": "XCUIElementTypeButton", "rawIdentifier": "library.add", "label": "Add Book",
                     "isEnabled": "1", "isVisible": "1"},
                    {"type": "XCUIElementTypeButton", "label": "", "isEnabled": "0", "isVisible": "1"}
                ]},
                {"type": "XCUIElementTypeTextField", "rawIdentifier": "bookForm.title", "value": "Dune",
                 "label": "Title", "isEnabled": true}
            ]
        })
    }

    #[test]
    fn parses_wda_source() {
        let root = UiNode::from_wda(&sample());
        assert_eq!(root.kind, "Application");
        assert_eq!(root.frame.unwrap().width, 430.0);
        let nodes: Vec<_> = root.walk().map(|n| n.kind.as_str()).collect();
        assert_eq!(
            nodes,
            ["Application", "Other", "Button", "Button", "TextField"]
        );
        let field = root.walk().last().unwrap();
        assert_eq!(field.identifier.as_deref(), Some("bookForm.title"));
        assert_eq!(field.value.as_deref(), Some("Dune"));
        assert!(!root.walk().nth(3).unwrap().enabled);
    }

    #[test]
    fn renders_without_empty_containers() {
        let text = UiNode::from_wda(&sample()).render();
        assert_eq!(
            text,
            "Application \"Pocket Library\"\n  Button id=library.add \"Add Book\"\n  Button (disabled)\n  TextField id=bookForm.title \"Title\" value=\"Dune\"\n"
        );
    }

    #[test]
    fn audit_flags_unlabelled_controls() {
        let findings: Vec<String> = UiNode::from_wda(&sample())
            .audit()
            .iter()
            .map(ToString::to_string)
            .collect();
        let json = serde_json::to_value(UiNode::from_wda(&sample()).audit()).unwrap();
        assert_eq!(json[0]["issue"], "no_label");
        assert_eq!(json[0]["element"]["kind"], "Button");
        assert_eq!(json[1]["severity"], "warning");
        assert_eq!(
            findings,
            [
                "no accessibility label: Button (disabled)",
                "no identifier (tests fall back to the label): Button (disabled)"
            ]
        );
    }

    #[test]
    fn lists_what_is_on_screen_and_serialises() {
        let root = UiNode::from_wda(&sample());
        assert_eq!(
            root.on_screen(2),
            ["Button id=library.add \"Add Book\"", "Button (disabled)"]
        );
        let json = serde_json::to_value(&root).unwrap();
        assert_eq!(json["kind"], "Application");
        assert_eq!(
            json["children"][0]["children"][0]["identifier"],
            "library.add"
        );
        assert!(json["children"][0].get("identifier").is_none());
    }

    #[test]
    fn audit_flags_duplicate_identifiers() {
        // Pocket Library's five rating stars, before they had their own ids.
        let star = || json!({"type": "XCUIElementTypeButton", "rawIdentifier": "star", "label": "Favorite"});
        let root = UiNode::from_wda(&json!({
            "type": "XCUIElementTypeApplication",
            "children": [star(), star(), star()]
        }));
        let findings = root.audit();
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].issue, "duplicate_identifier");
        assert_eq!(
            findings[0].to_string(),
            "identifier used by more than one element: Button id=star \"Favorite\""
        );
    }

    #[test]
    fn audit_skips_the_switch_inside_a_labelled_toggle() {
        // Pocket Library's "Online catalogue search" Toggle as WDA saw it.
        let root = UiNode::from_wda(&json!({
            "type": "XCUIElementTypeApplication",
            "children": [{"type": "XCUIElementTypeSwitch", "rawIdentifier": "settings.onlineCatalogue",
                "label": "Online catalogue search", "value": "1", "children": [
                {"type": "XCUIElementTypeStaticText", "label": "Online catalogue search"},
                {"type": "XCUIElementTypeSwitch", "value": "1"}
            ]}]
        }));
        assert!(root.audit().is_empty());
        // An unlabelled Switch on its own is still an error.
        let root = UiNode::from_wda(&json!({
            "type": "XCUIElementTypeApplication",
            "children": [{"type": "XCUIElementTypeSwitch", "rawIdentifier": "x.on", "value": "1"}]
        }));
        assert_eq!(root.audit()[0].issue, "no_label");
    }

    #[test]
    fn audit_allows_a_wrapper_sharing_its_controls_identifier() {
        // SwiftUI puts a toolbar Button's identifier on its wrapper too.
        let root = UiNode::from_wda(&json!({
            "type": "XCUIElementTypeApplication",
            "children": [{"type": "XCUIElementTypeOther", "rawIdentifier": "tip.done", "children": [
                {"type": "XCUIElementTypeButton", "rawIdentifier": "tip.done", "label": "Done"}
            ]}]
        }));
        assert!(root.audit().is_empty());
    }

    #[test]
    fn nth_mut_follows_walk_order() {
        let mut root = UiNode::from_wda(&sample());
        assert_eq!(root.nth_mut(4).unwrap().kind, "TextField");
        assert!(root.nth_mut(5).is_none());
    }

    fn el(kind: &str, id: Option<&str>, label: &str, frame: (f64, f64, f64, f64)) -> Value {
        let mut v = json!({
            "type": format!("XCUIElementType{kind}"), "label": label, "isEnabled": "1", "isVisible": "1",
            "rect": {"x": frame.0, "y": frame.1, "width": frame.2, "height": frame.3},
            "children": []
        });
        if let Some(id) = id {
            v["rawIdentifier"] = json!(id);
        }
        v
    }

    fn app(children: Vec<Value>) -> UiNode {
        UiNode::from_wda(&json!({
            "type": "XCUIElementTypeApplication", "label": "App", "isEnabled": "1",
            "rect": {"x": 0, "y": 0, "width": 430, "height": 932},
            "children": children
        }))
    }

    fn issues(root: &UiNode) -> Vec<String> {
        root.audit().iter().map(ToString::to_string).collect()
    }

    #[test]
    fn audit_flags_small_tap_targets() {
        // Murmur's like button, 44 x 18 pt.
        let root = app(vec![el(
            "Button",
            Some("post.like"),
            "Like",
            (20.0, 300.0, 44.0, 18.0),
        )]);
        assert_eq!(
            issues(&root),
            ["tap target too small (44 x 18 pt, under 44 x 44): Button id=post.like \"Like\""]
        );
        assert_eq!(root.audit()[0].issue, "small_tap_target");
        // A system list row, 43 pt in the tree, meets it; 42 pt doesn't.
        let row = |h| app(vec![el("Button", None, "Adult", (0.0, 300.0, 250.0, h))]);
        assert!(row(43.0)
            .audit()
            .iter()
            .all(|f| f.issue != "small_tap_target"));
        assert!(row(42.0)
            .audit()
            .iter()
            .any(|f| f.issue == "small_tap_target"));
        // A disabled one can't be tapped at all.
        let mut off = el(
            "Button",
            Some("post.like"),
            "Like",
            (20.0, 300.0, 44.0, 18.0),
        );
        off["isEnabled"] = json!("0");
        assert!(app(vec![off]).audit().is_empty());
    }

    #[test]
    fn a_form_row_counts_as_the_target_but_a_crowded_one_does_not() {
        // Good Dog's onboarding form: a 34 pt picker button in a 44 pt row.
        let mut row = el("Other", None, "", (20.0, 346.0, 390.0, 44.0));
        row["children"] = json!([el(
            "Button",
            Some("dogForm.lifeStage"),
            "Age, Adult",
            (40.0, 351.0, 350.0, 34.0)
        )]);
        assert!(app(vec![row.clone()]).audit().is_empty());
        // The same row with a second control: the button alone is the target.
        row["children"].as_array_mut().unwrap().push(el(
            "Button",
            Some("dogForm.help"),
            "Help",
            (390.0, 346.0, 20.0, 44.0),
        ));
        let found = issues(&app(vec![row]));
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].contains("350 x 34 pt"), "{found:?}");
        // A container far bigger than the control doesn't count either.
        let mut screen = el("Other", None, "", (0.0, 0.0, 430.0, 932.0));
        screen["children"] = json!([el("Button", Some("x.go"), "Go", (40.0, 351.0, 350.0, 34.0))]);
        assert_eq!(app(vec![screen]).audit().len(), 1);
    }

    #[test]
    fn bar_buttons_and_links_in_a_sentence_are_left_out() {
        // Good Dog's Edit button in the navigation bar, 38 x 29 pt.
        let mut bar = el("NavigationBar", None, "My Dog", (0.0, 53.0, 430.0, 97.0));
        bar["children"] = json!([el(
            "Button",
            Some("dog.edit"),
            "Edit",
            (372.0, 61.0, 38.0, 29.0)
        )]);
        // A glossary link inside a step's text, 28 x 20 pt.
        let mut text = el(
            "StaticText",
            Some("exercise.step.1"),
            "Say their name, the cue",
            (62.0, 773.0, 328.0, 41.0),
        );
        text["children"] = json!([el(
            "Link",
            Some("gooddog-term:cue"),
            "cue",
            (149.0, 794.0, 28.0, 20.0)
        )]);
        assert!(app(vec![bar, text]).audit().is_empty());
    }

    #[test]
    fn audit_flags_text_cut_off() {
        // At the largest text size a title runs off the right edge.
        let root = app(vec![el(
            "StaticText",
            None,
            "Practise responding to their name",
            (20.0, 100.0, 520.0, 60.0),
        )]);
        assert_eq!(
            issues(&root),
            ["text cut off (runs off the right side of the screen): StaticText \"Practise responding to their name\""]
        );
        // Text wholly beside the screen (a carousel's next page) is not cut.
        let root = app(vec![el(
            "StaticText",
            None,
            "Page 2",
            (450.0, 100.0, 200.0, 60.0),
        )]);
        assert!(root.audit().is_empty());
        // Two texts drawn over each other in a row of fixed height.
        let mut cell = el("Cell", None, "", (20.0, 200.0, 390.0, 60.0));
        cell["children"] = json!([
            el("StaticText", None, "Skill", (40.0, 205.0, 200.0, 50.0)),
            el(
                "StaticText",
                None,
                "Name response: Started",
                (120.0, 210.0, 270.0, 50.0)
            ),
        ]);
        let mut list = el("CollectionView", None, "", (0.0, 150.0, 430.0, 700.0));
        list["children"] = json!([cell]);
        let found = issues(&app(vec![list]));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].starts_with("text cut off (drawn over StaticText \"Skill\")"),
            "{found:?}"
        );
    }

    #[test]
    fn a_list_scrolling_under_a_bar_is_not_an_overlap() {
        // Good Dog's "Done today" bar over the steps list (4-recorded).
        let mut list = el("CollectionView", None, "", (0.0, 150.0, 430.0, 782.0));
        list["children"] = json!([el(
            "StaticText",
            Some("exercise.step.1"),
            "Step 2",
            (20.0, 762.0, 390.0, 63.0)
        )]);
        let bar = el(
            "StaticText",
            Some("exercise.done"),
            "Done today: Went well",
            (0.0, 797.0, 430.0, 135.0),
        );
        assert!(app(vec![list, bar]).audit().is_empty());
    }

    #[test]
    fn recorded_good_dog_screens_have_no_size_or_clipping_findings() {
        // The GD1 journey's trees from the iPhone: the new checks must not
        // cry wolf on a screen a person looked at and found fine.
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/good-dog/uitests/baselines/journey"
        );
        let mut screens = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if !path.to_string_lossy().ends_with(".tree.json") {
                continue;
            }
            let tree: UiNode =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let new: Vec<String> = tree
                .audit()
                .iter()
                .filter(|f| matches!(f.issue, "small_tap_target" | "clipped"))
                .map(ToString::to_string)
                .collect();
            assert!(new.is_empty(), "{}: {new:?}", path.display());
            screens += 1;
        }
        assert_eq!(screens, 6);
    }
}
