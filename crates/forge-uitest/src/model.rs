//! A screen-graph model of an app, for running `forge ui explore` without a
//! phone (`--fixture`, docs/forge-m6.md M6.2 demonstration).
//!
//! A model is a JSON file: the app's screens as accessibility trees (each
//! recorded from the phone, such as a visual baseline's `.tree.json`, or
//! written by hand and marked `synthesized`), and where each tap leads. It
//! answers like WebDriverAgent would, so the explorer, its focus, replay
//! files and evidence all run unchanged.
//!
//! A walk on a model proves how Forge explores and reports, never how the
//! app behaves: its bundle says `kind = "fixture"`.
//!
//! ```json
//! {
//!   "app": "org.forgemobile.gooddog",
//!   "start": "today",
//!   "screens": {
//!     "today": {"tree": "../baselines/journey/2-today@1290x2796.tree.json"},
//!     "record": {"tree": "record.tree.json", "synthesized": "from ExerciseView.swift"}
//!   },
//!   "taps": [
//!     {"screen": "today", "target": "today.activity.0", "to": "exercise"},
//!     {"screen": "record-chosen", "target": "record.save", "to": "crash"}
//!   ]
//! }
//! ```
//!
//! `to` is a screen, `crash` (the app closes), `app:<bundle id>` (another
//! app comes to the front) or `stay`. A tap that no entry names leaves the
//! screen as it is.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::fake::SPRINGBOARD;
use crate::{Driver, ElementRef, Locator, Result, UiError, UiNode};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelFile {
    pub app: String,
    pub start: String,
    pub screens: BTreeMap<String, ScreenFile>,
    #[serde(default)]
    pub taps: Vec<Tap>,
    /// What the model stands for, shown in reports.
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenFile {
    /// The tree file, relative to the model file.
    pub tree: String,
    /// Set when the tree wasn't recorded from the phone: what it was
    /// written from.
    #[serde(default)]
    pub synthesized: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tap {
    pub screen: String,
    /// An identifier, or a label for controls without one.
    pub target: String,
    pub to: String,
}

/// A model app, driven like a phone.
pub struct Model {
    pub app: String,
    pub note: Option<String>,
    screens: BTreeMap<String, UiNode>,
    taps: Vec<Tap>,
    start: String,
    current: String,
    front: Option<String>,
    /// Screens whose trees were written by hand, and from what.
    pub synthesized: BTreeMap<String, String>,
    /// What was done to it, one line per call.
    pub log: Vec<String>,
}

impl Model {
    pub fn load(file: &Path) -> std::result::Result<Model, String> {
        let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let spec: ModelFile =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
        let dir = file.parent().unwrap_or(Path::new("."));
        let mut screens = BTreeMap::new();
        let mut synthesized = BTreeMap::new();
        for (name, screen) in &spec.screens {
            let path = dir.join(&screen.tree);
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("screen {name}: {}: {e}", path.display()))?;
            let tree: UiNode = serde_json::from_str(&text)
                .map_err(|e| format!("screen {name}: {}: {e}", path.display()))?;
            screens.insert(name.clone(), tree);
            if let Some(from) = &screen.synthesized {
                synthesized.insert(name.clone(), from.clone());
            }
        }
        if !screens.contains_key(&spec.start) {
            return Err(format!("start screen {:?} isn't in screens", spec.start));
        }
        for tap in &spec.taps {
            if !screens.contains_key(&tap.screen) {
                return Err(format!(
                    "a tap names screen {:?}, which isn't in screens",
                    tap.screen
                ));
            }
            let known = tap.to == "crash"
                || tap.to == "stay"
                || tap.to.starts_with("app:")
                || screens.contains_key(&tap.to);
            if !known {
                return Err(format!(
                    "tap {} on {}: `to` {:?} is neither a screen, crash, stay nor app:<bundle id>",
                    tap.target, tap.screen, tap.to
                ));
            }
        }
        Ok(Model {
            app: spec.app,
            note: spec.note,
            current: spec.start.clone(),
            start: spec.start,
            screens,
            taps: spec.taps,
            front: None,
            synthesized,
            log: Vec::new(),
        })
    }

    fn screen(&mut self) -> &mut UiNode {
        self.screens.get_mut(&self.current).expect("current screen")
    }

    fn node(&mut self, element: &ElementRef) -> Result<UiNode> {
        let index: usize = element
            .0
            .parse()
            .map_err(|_| UiError::Wda("bad element".into()))?;
        self.screen()
            .nth_mut(index)
            .cloned()
            .ok_or_else(|| UiError::Stale("the element is gone".into()))
    }
}

impl Driver for Model {
    fn launch(&mut self, bundle_id: &str, args: &[String]) -> Result<()> {
        self.log.push(format!("launch {args:?}"));
        if bundle_id != self.app {
            return Err(UiError::Device(format!("{bundle_id} is not installed")));
        }
        self.front = Some(bundle_id.to_owned());
        self.current = self.start.clone();
        Ok(())
    }
    fn activate(&mut self, bundle_id: &str) -> Result<()> {
        self.front = Some(bundle_id.to_owned());
        Ok(())
    }
    fn terminate(&mut self, _: &str) -> Result<()> {
        self.front = Some(SPRINGBOARD.into());
        Ok(())
    }
    fn background(&mut self, _: f64) -> Result<()> {
        Ok(())
    }
    fn scroll(&mut self, _: &ElementRef, _: &str) -> Result<()> {
        Ok(())
    }
    fn find_all(&mut self, locator: &Locator) -> Result<Vec<ElementRef>> {
        Ok(locator
            .find_in(self.screen())
            .into_iter()
            .map(|(i, _)| ElementRef(i.to_string()))
            .collect())
    }
    fn tap(&mut self, element: &ElementRef) -> Result<()> {
        let node = self.node(element)?;
        let key = node.identifier.or(node.label).unwrap_or_default();
        self.log.push(format!("tap {key}"));
        let to = self
            .taps
            .iter()
            .find(|t| t.screen == self.current && t.target == key)
            .map(|t| t.to.clone());
        match to.as_deref() {
            Some("crash") => self.front = Some(SPRINGBOARD.into()),
            Some("stay") | None => {}
            Some(other) => match other.strip_prefix("app:") {
                Some(bundle) => self.front = Some(bundle.to_owned()),
                None => self.current = other.to_owned(),
            },
        }
        Ok(())
    }
    fn type_text(&mut self, element: &ElementRef, text: &str) -> Result<()> {
        let index: usize = element
            .0
            .parse()
            .map_err(|_| UiError::Wda("bad element".into()))?;
        self.log.push(format!("type {text}"));
        if let Some(node) = self.screen().nth_mut(index) {
            node.value = Some(text.into());
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
        Ok(self.screen().clone())
    }
    fn screenshot(&mut self) -> Result<Vec<u8>> {
        // No pixels: a model has trees only.
        Err(UiError::Wda("a model app has no screenshots".into()))
    }
    fn foreground(&mut self) -> Result<Option<String>> {
        Ok(self.front.clone())
    }
}
