//! The accessibility tree of the screen, as WebDriverAgent reports it
//! (`GET /source?format=json`), in Forge's own types.

use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// One element of the accessibility tree.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UiNode {
    /// The element type without Apple's prefix: `Button`, `TextField`, `Cell`…
    pub kind: String,
    /// `.accessibilityIdentifier(...)`, for tests only.
    pub identifier: Option<String>,
    /// What VoiceOver reads.
    pub label: Option<String>,
    /// A field's text, a toggle's state, a slider's position.
    pub value: Option<String>,
    pub enabled: bool,
    pub visible: bool,
    pub frame: Option<Rect>,
    pub children: Vec<UiNode>,
}

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
        INTERACTIVE.contains(&self.kind.as_str())
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
        if !self.visible {
            line.push_str(" (hidden)");
        }
        line
    }

    /// Visible interactive elements that VoiceOver has no name for, and
    /// those a test cannot find by identifier. The full audit is M3; this is
    /// the part every screen needs.
    pub fn audit(&self) -> Vec<String> {
        let mut findings = Vec::new();
        for node in self.walk().filter(|n| n.visible && n.is_interactive()) {
            if node.label.is_none() {
                findings.push(format!("no accessibility label: {}", node.summary()));
            }
            if node.identifier.is_none() {
                findings.push(format!(
                    "no identifier (tests fall back to the label): {}",
                    node.summary()
                ));
            }
        }
        findings
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
        let findings = UiNode::from_wda(&sample()).audit();
        assert_eq!(
            findings,
            [
                "no accessibility label: Button (disabled)",
                "no identifier (tests fall back to the label): Button (disabled)"
            ]
        );
    }

    #[test]
    fn nth_mut_follows_walk_order() {
        let mut root = UiNode::from_wda(&sample());
        assert_eq!(root.nth_mut(4).unwrap().kind, "TextField");
        assert!(root.nth_mut(5).is_none());
    }
}
