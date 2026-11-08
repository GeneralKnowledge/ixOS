//! UI tests as TOML files: a list of steps a person can read and review.
//!
//! ```toml
//! name = "Add a book"
//! app = "org.forgemobile.pocketlibrary"
//!
//! [[step]]
//! launch = {}
//!
//! [[step]]
//! tap = { id = "library.add" }
//!
//! [[step]]
//! type = { id = "bookForm.title", text = "Hello from Forge" }
//!
//! [[step]]
//! expect = { id = "bookForm.title", value = "Hello from Forge" }
//!
//! [[step]]
//! screenshot = "form"
//! ```

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::policy::{self, Action, Gate};
use crate::{ensure_front, wait_for, wait_gone, Driver, Locator, Result, UiError};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    pub name: String,
    /// Bundle id of the app under test, as written in its forge.toml.
    /// Free-account installs are renamed by xtool; see [`Script::resolve_app`].
    pub app: String,
    /// Seconds to wait for an element before failing (default 10).
    #[serde(default = "default_timeout")]
    pub timeout: f64,
    #[serde(rename = "step", default)]
    pub steps: Vec<Step>,
}

fn default_timeout() -> f64 {
    10.0
}

/// Exactly one action per step.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub launch: Option<Launch>,
    pub activate: Option<toml::Table>,
    pub terminate: Option<toml::Table>,
    pub background: Option<f64>,
    pub tap: Option<Target>,
    #[serde(rename = "type")]
    pub type_text: Option<TypeText>,
    pub clear: Option<Target>,
    pub scroll: Option<Scroll>,
    pub expect: Option<Expect>,
    pub screenshot: Option<String>,
    pub tree: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    #[serde(default)]
    pub args: Vec<String>,
}

/// Which element: an identifier, an exact label, or part of a label.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub id: Option<String>,
    pub label: Option<String>,
    pub label_contains: Option<String>,
}

// No `deny_unknown_fields` here: serde does not support it together with
// `flatten`. `Target` still rejects unknown keys.
#[derive(Debug, Deserialize)]
pub struct TypeText {
    #[serde(flatten)]
    pub target: Target,
    pub text: String,
    /// Empty the field first.
    #[serde(default)]
    pub clear: bool,
}

/// `scroll = { id = "list", direction = "down" }` scrolls a scroll view;
/// `direction = "visible"` (the default) scrolls until the target itself is
/// on screen.
#[derive(Debug, Deserialize)]
pub struct Scroll {
    #[serde(flatten)]
    pub target: Target,
    #[serde(default = "default_direction")]
    pub direction: String,
}

fn default_direction() -> String {
    "visible".into()
}

const DIRECTIONS: &[&str] = &["visible", "up", "down", "left", "right"];

#[derive(Debug, Deserialize)]
pub struct Expect {
    #[serde(flatten)]
    pub target: Target,
    /// The element's value (a field's text) equals this.
    pub value: Option<String>,
    /// The element's label equals this.
    pub has_label: Option<String>,
    pub enabled: Option<bool>,
    /// Nothing matches the target any more.
    #[serde(default)]
    pub gone: bool,
}

impl Target {
    pub fn locator(&self) -> std::result::Result<Locator, String> {
        match (&self.id, &self.label, &self.label_contains) {
            (Some(id), None, None) => Ok(Locator::Id(id.clone())),
            (None, Some(label), None) => Ok(Locator::Label(label.clone())),
            (None, None, Some(text)) => Ok(Locator::LabelContains(text.clone())),
            _ => Err("give exactly one of `id`, `label`, `label_contains`".into()),
        }
    }
}

impl Script {
    pub fn load(path: &Path) -> std::result::Result<Script, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Script::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn parse(text: &str) -> std::result::Result<Script, String> {
        let script: Script = toml::from_str(text).map_err(|e| e.to_string())?;
        for (i, step) in script.steps.iter().enumerate() {
            step.check().map_err(|e| format!("step {}: {e}", i + 1))?;
        }
        Ok(script)
    }
}

impl Step {
    fn actions(&self) -> usize {
        [
            self.launch.is_some(),
            self.activate.is_some(),
            self.terminate.is_some(),
            self.background.is_some(),
            self.tap.is_some(),
            self.type_text.is_some(),
            self.clear.is_some(),
            self.scroll.is_some(),
            self.expect.is_some(),
            self.screenshot.is_some(),
            self.tree.is_some(),
        ]
        .iter()
        .filter(|set| **set)
        .count()
    }

    fn check(&self) -> std::result::Result<(), String> {
        if self.actions() != 1 {
            return Err("each step needs exactly one action".into());
        }
        for target in [&self.tap, &self.clear].into_iter().flatten() {
            target.locator()?;
        }
        if let Some(t) = &self.type_text {
            t.target.locator()?;
        }
        if let Some(e) = &self.expect {
            e.target.locator()?;
        }
        if let Some(s) = &self.scroll {
            s.target.locator()?;
            if !DIRECTIONS.contains(&s.direction.as_str()) {
                return Err(format!(
                    "scroll direction `{}`: use one of {}",
                    s.direction,
                    DIRECTIONS.join(", ")
                ));
            }
        }
        Ok(())
    }

    /// One line for the progress output.
    pub fn describe(&self) -> String {
        let loc = |t: &Target| t.locator().map(|l| l.to_string()).unwrap_or_default();
        if let Some(l) = &self.launch {
            return if l.args.is_empty() {
                "launch the app".into()
            } else {
                format!("launch the app with {:?}", l.args)
            };
        }
        if self.activate.is_some() {
            return "bring the app to the front".into();
        }
        if self.terminate.is_some() {
            return "stop the app".into();
        }
        if let Some(s) = self.background {
            return format!("background the app for {s} s");
        }
        if let Some(t) = &self.tap {
            return format!("tap {}", loc(t));
        }
        if let Some(t) = &self.type_text {
            return format!("type {:?} into {}", t.text, loc(&t.target));
        }
        if let Some(t) = &self.clear {
            return format!("clear {}", loc(t));
        }
        if let Some(s) = &self.scroll {
            return if s.direction == "visible" {
                format!("scroll until {} is on screen", loc(&s.target))
            } else {
                format!("scroll {} {}", loc(&s.target), s.direction)
            };
        }
        if let Some(e) = &self.expect {
            let mut what = vec![];
            if e.gone {
                what.push("is gone".to_string());
            }
            if let Some(v) = &e.value {
                what.push(format!("has value {v:?}"));
            }
            if let Some(v) = &e.has_label {
                what.push(format!("has label {v:?}"));
            }
            if let Some(v) = e.enabled {
                what.push(if v { "is enabled" } else { "is disabled" }.to_string());
            }
            if what.is_empty() {
                what.push("is on screen".into());
            }
            return format!("check {} {}", loc(&e.target), what.join(", "));
        }
        if let Some(name) = &self.screenshot {
            return format!("screenshot {name}");
        }
        "print the accessibility tree".into()
    }
}

/// The outcome of one run.
#[derive(Debug, Default)]
pub struct Report {
    pub passed: usize,
    /// The failing step's number and the error.
    pub failed: Option<(usize, String)>,
    /// What the failing step tried to do.
    pub failed_step: Option<String>,
    /// At a failure: what a test could target on screen instead.
    pub on_screen: Vec<String>,
    /// At a failure: the whole accessibility tree, as JSON.
    pub tree_file: Option<PathBuf>,
    pub screenshots: Vec<PathBuf>,
}

/// Runs `script` against `driver` with `app` as the installed bundle id.
/// Progress goes to `out`; screenshots, and the tree and a screenshot at a
/// failure, are written to `out_dir`. For the developer's own app: nothing
/// is gated.
pub fn run(
    script: &Script,
    app: &str,
    driver: &mut dyn Driver,
    out_dir: &Path,
    out: &mut dyn Write,
) -> Result<Report> {
    run_gated(script, app, driver, out_dir, out, &mut Gate::own(app))
}

/// [`run`], with every step checked by `gate` first (another developer's
/// app: docs/research/general-device-control.md#safety).
pub fn run_gated(
    script: &Script,
    app: &str,
    driver: &mut dyn Driver,
    out_dir: &Path,
    out: &mut dyn Write,
    gate: &mut Gate,
) -> Result<Report> {
    std::fs::create_dir_all(out_dir)?;
    let timeout = Duration::from_secs_f64(script.timeout.max(0.0));
    let mut report = Report::default();
    writeln!(out, "{}", script.name)?;
    for (index, step) in script.steps.iter().enumerate() {
        let number = index + 1;
        let started = Instant::now();
        let result = run_step(step, app, driver, timeout, out_dir, out, &mut report, gate);
        let took = started.elapsed().as_secs_f64();
        match result {
            Ok(()) => {
                writeln!(
                    out,
                    "  ok    {number:>2}. {} ({took:.1} s)",
                    step.describe()
                )?;
                report.passed += 1;
            }
            Err(error) => {
                writeln!(
                    out,
                    "  FAIL  {number:>2}. {}\n        {error}",
                    step.describe()
                )?;
                // In another app, keep only what the bound app showed:
                // never capture whatever else came to the front.
                let capture = !gate.is_foreign()
                    || driver.foreground().ok().flatten().as_deref() == Some(app);
                if capture {
                    record_failure(driver, number, out_dir, out, &mut report);
                }
                report.failed = Some((number, error.to_string()));
                report.failed_step = Some(step.describe());
                return Ok(report);
            }
        }
    }
    writeln!(out, "{} steps passed", report.passed)?;
    Ok(report)
}

#[allow(clippy::too_many_arguments)]
fn run_step(
    step: &Step,
    app: &str,
    driver: &mut dyn Driver,
    timeout: Duration,
    out_dir: &Path,
    out: &mut dyn Write,
    report: &mut Report,
    gate: &mut Gate,
) -> Result<()> {
    let locate = |t: &Target| t.locator().map_err(UiError::Device);
    let allow = |gate: &mut Gate, action: Action| gate.check(&action).map_err(UiError::Refused);
    // The coarse check before anything happens; taps and typing are checked
    // again below once the element (its label and kind) is known.
    let coarse = if step.launch.is_some() {
        Action::Launch
    } else if step.activate.is_some() {
        Action::Activate
    } else if step.terminate.is_some() {
        Action::Terminate
    } else if step.background.is_some() {
        Action::Background
    } else if step.scroll.is_some() {
        Action::Scroll
    } else {
        // Reading; or a tap, typing or clear, checked with its element below.
        Action::Read
    };
    allow(gate, coarse)?;
    if let Some(launch) = &step.launch {
        // Launching an app that is already running only brings it to the
        // front, and its launch arguments are silently dropped. Stop it first
        // when there are arguments, so they always apply (an app that isn't
        // running is fine).
        if !launch.args.is_empty() {
            let _ = driver.terminate(app);
        }
        return driver.launch(app, &launch.args);
    }
    if step.activate.is_some() {
        return driver.activate(app);
    }
    if step.terminate.is_some() {
        return driver.terminate(app);
    }
    if let Some(seconds) = step.background {
        return driver.background(seconds);
    }
    // Everything below reads or touches the screen: only ever the app under
    // test. Expecting an element gone is the exception, since it also holds
    // after the app was terminated; a screenshot is evidence either way.
    let gone = step.expect.as_ref().is_some_and(|e| e.gone);
    if step.screenshot.is_none() && !gone {
        ensure_front(driver, app, timeout)?;
    } else if gate.is_foreign() {
        // In another app a screenshot is only of that app (or the home
        // screen after it was stopped), never of whatever else is open.
        let front = driver.foreground()?;
        if !matches!(front.as_deref(), Some(f) if f == app || f == policy::SPRINGBOARD) {
            return Err(UiError::Refused(format!(
                "{} is in front, not {app}; Forge won't read another app's screen",
                front.as_deref().unwrap_or("an unknown app")
            )));
        }
    }
    if let Some(target) = &step.tap {
        return tap(driver, &locate(target)?, timeout, gate);
    }
    if let Some(typing) = &step.type_text {
        let mut checked = false;
        return act(driver, &locate(&typing.target)?, timeout, &mut |d, e| {
            if !checked && gate.wants_targets() {
                let action = Action::Type {
                    target: target_of(d, e)?,
                    chars: typing.text.chars().count(),
                    presses_return: typing.text.contains(['\n', '\r']),
                };
                allow(gate, action)?;
                checked = true;
            }
            if typing.clear {
                d.clear(e)?;
            }
            d.type_text(e, &typing.text)
        });
    }
    if let Some(target) = &step.clear {
        let mut checked = false;
        return act(driver, &locate(target)?, timeout, &mut |d, e| {
            if !checked && gate.wants_targets() {
                allow(gate, Action::Clear(target_of(d, e)?))?;
                checked = true;
            }
            d.clear(e)
        });
    }
    if let Some(scroll) = &step.scroll {
        return act(driver, &locate(&scroll.target)?, timeout, &mut |d, e| {
            d.scroll(e, &scroll.direction)
        });
    }
    if let Some(expect) = &step.expect {
        return check(driver, expect, &locate(&expect.target)?, timeout);
    }
    if let Some(name) = &step.screenshot {
        let path = out_dir.join(format!("{}.png", file_name(name)));
        std::fs::write(&path, driver.screenshot()?)?;
        report.screenshots.push(path);
        return Ok(());
    }
    let tree = driver.tree()?;
    write!(out, "{}", indent(&tree.render()))?;
    Ok(())
}

/// Finds the element and acts on it. SwiftUI re-renders freely, so an
/// element can go stale between the find and the action: then it is found
/// again and the action retried, once.
fn act(
    driver: &mut dyn Driver,
    locator: &Locator,
    timeout: Duration,
    action: &mut dyn FnMut(&mut dyn Driver, &crate::ElementRef) -> Result<()>,
) -> Result<()> {
    let element = wait_for(driver, locator, timeout)?;
    match action(driver, &element) {
        Err(UiError::Stale(_)) => {
            let element = wait_for(driver, locator, timeout)?;
            action(driver, &element)
        }
        other => other,
    }
}

/// How long a tap whose reply says the element went stale waits for the
/// element to finish leaving (a sheet or keyboard animating away).
const TAP_GONE_SETTLE: Duration = Duration::from_secs(2);

/// Taps the element. A tap often makes its own element go away: Done
/// closes the keyboard and its toolbar, Save dismisses the sheet. WDA can
/// then answer the tap with "stale element reference", because the element
/// no longer exists once the tap has landed. If the element is gone after
/// such a reply, the tap did its job and the step passes; whether it had
/// the right effect is for the next step's expectation to check. If the
/// element is still there, the screen re-rendered under the tap, so it is
/// found and tapped again, once, like any other action.
fn tap(
    driver: &mut dyn Driver,
    locator: &Locator,
    timeout: Duration,
    gate: &mut Gate,
) -> Result<()> {
    let mut retried = false;
    // Asked once per step, not again when a re-render makes the tap retry.
    let mut checked = !gate.wants_targets();
    loop {
        let element = wait_for(driver, locator, timeout)?;
        if !checked {
            let target = match target_of(driver, &element) {
                // Re-rendered before it could be read: find it again, once.
                Err(UiError::Stale(_)) if !retried => {
                    retried = true;
                    continue;
                }
                other => other?,
            };
            gate.check(&Action::Tap(target)).map_err(UiError::Refused)?;
            checked = true;
        }
        // A stale reply from wait_hittable means the element went away
        // before the tap: found again below, like any action.
        let result = wait_hittable(driver, &element, timeout).and_then(|()| {
            let why = match driver.tap(&element) {
                Err(UiError::Stale(why)) => why,
                other => return other,
            };
            match wait_gone(driver, locator, TAP_GONE_SETTLE) {
                Ok(()) => {
                    eprintln!("          the tap took {locator} off the screen");
                    Ok(())
                }
                Err(UiError::Mismatch { .. }) => Err(UiError::Stale(why)),
                Err(e) => Err(e),
            }
        });
        match result {
            Err(UiError::Stale(_)) if !retried => retried = true,
            other => return other,
        }
    }
}

/// What the policy needs to know about an element: its label, identifier
/// and kind, as WebDriverAgent reports them.
fn target_of(driver: &mut dyn Driver, element: &crate::ElementRef) -> Result<policy::Target> {
    Ok(policy::Target {
        label: driver.attribute(element, "label")?,
        identifier: driver.attribute(element, "name")?,
        kind: driver.attribute(element, "type")?,
    })
}

/// Waits (up to `timeout`) until WDA says a tap would reach the element.
/// An element can be in the tree before it can take a tap: a keyboard
/// toolbar's button while the keyboard is still sliding in ignored a tap
/// on Cati. If WDA doesn't report `hittable`, or it stays false, the tap
/// goes ahead and the next step's expectation decides.
fn wait_hittable(
    driver: &mut dyn Driver,
    element: &crate::ElementRef,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match driver.attribute(element, "hittable")?.as_deref() {
            Some("true") | None => return Ok(()),
            _ if Instant::now() >= deadline => return Ok(()),
            _ => std::thread::sleep(crate::POLL),
        }
    }
}

/// Polls until the expectation holds or the timeout passes, so a test can
/// expect what an async Rust call will eventually show.
fn check(
    driver: &mut dyn Driver,
    expect: &Expect,
    locator: &Locator,
    timeout: Duration,
) -> Result<()> {
    if expect.gone {
        return wait_gone(driver, locator, timeout);
    }
    let deadline = Instant::now() + timeout;
    loop {
        let element = wait_for(
            driver,
            locator,
            deadline.saturating_duration_since(Instant::now()),
        )?;
        let mismatch = match mismatch(driver, expect, &element) {
            Ok(mismatch) => mismatch,
            // Re-rendered between the find and the read: read again.
            Err(UiError::Stale(_)) if Instant::now() < deadline => {
                std::thread::sleep(crate::POLL);
                continue;
            }
            Err(e) => return Err(e),
        };
        match mismatch {
            None => return Ok(()),
            Some((expected, found)) if Instant::now() >= deadline => {
                return Err(UiError::Mismatch {
                    locator: locator.clone(),
                    expected,
                    found,
                })
            }
            Some(_) => std::thread::sleep(crate::POLL),
        }
    }
}

/// What differs from the expectation, as (expected, found), if anything.
fn mismatch(
    driver: &mut dyn Driver,
    expect: &Expect,
    element: &crate::ElementRef,
) -> Result<Option<(String, String)>> {
    let mut mismatch = None;
    for (attribute, wanted) in [("value", &expect.value), ("label", &expect.has_label)] {
        if let Some(wanted) = wanted {
            let found = driver.attribute(element, attribute)?;
            // An empty field reports no value at all: that is "".
            if found.as_deref().unwrap_or("") != wanted.as_str() {
                mismatch = Some((
                    format!("{attribute} {wanted:?}"),
                    found
                        .map(|v| format!("{v:?}"))
                        .unwrap_or_else(|| format!("no {attribute}")),
                ));
            }
        }
    }
    if let Some(wanted) = expect.enabled {
        let found = driver.attribute(element, "enabled")?;
        let is = matches!(found.as_deref(), Some("true") | Some("1"));
        if is != wanted {
            mismatch = Some((
                if wanted { "enabled" } else { "disabled" }.into(),
                if is { "enabled" } else { "disabled" }.into(),
            ));
        }
    }
    Ok(mismatch)
}

/// Saves what was on screen when a step failed. Best effort: the failure
/// itself is the report.
fn record_failure(
    driver: &mut dyn Driver,
    number: usize,
    out_dir: &Path,
    out: &mut dyn Write,
    report: &mut Report,
) {
    match driver.tree() {
        Ok(tree) => {
            report.on_screen = tree.on_screen(15);
            if !report.on_screen.is_empty() {
                let _ = writeln!(out, "        on screen:");
                for line in &report.on_screen {
                    let _ = writeln!(out, "          {line}");
                }
            }
            let text = out_dir.join(format!("failure-step{number}-tree.txt"));
            let json = out_dir.join(format!("failure-step{number}-tree.json"));
            let saved = std::fs::write(&text, tree.render()).is_ok()
                && serde_json::to_vec_pretty(&tree)
                    .map(|bytes| std::fs::write(&json, bytes).is_ok())
                    .unwrap_or(false);
            if saved {
                let _ = writeln!(out, "        screen at failure: {}", text.display());
                report.tree_file = Some(json);
            }
        }
        Err(e) => {
            let _ = writeln!(out, "        could not read the screen: {e}");
        }
    }
    if let Ok(png) = driver.screenshot() {
        let path = out_dir.join(format!("failure-step{number}.png"));
        if std::fs::write(&path, png).is_ok() {
            report.screenshots.push(path);
        }
    }
}

fn file_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn indent(text: &str) -> String {
    text.lines().map(|l| format!("        {l}\n")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeDriver;
    use crate::UiNode;

    fn node(kind: &str, id: &str, label: &str) -> UiNode {
        UiNode {
            kind: kind.into(),
            identifier: Some(id.into()),
            label: Some(label.into()),
            enabled: true,
            visible: true,
            ..UiNode::default()
        }
    }

    fn library() -> UiNode {
        UiNode {
            kind: "Application".into(),
            children: vec![node("Button", "library.add", "Add Book")],
            ..UiNode::default()
        }
    }

    fn form() -> UiNode {
        UiNode {
            kind: "Application".into(),
            children: vec![
                node("TextField", "bookForm.title", "Title"),
                node("Button", "bookForm.save", "Add"),
            ],
            ..UiNode::default()
        }
    }

    /// A fake phone with app `a` already in front.
    fn in_front(screen: UiNode) -> FakeDriver {
        let mut driver = FakeDriver::new(screen);
        driver.front = Some("a".into());
        driver
    }

    const M0: &str = r#"
        name = "M0"
        app = "org.forgemobile.pocketlibrary"
        timeout = 0

        [[step]]
        launch = {}
        [[step]]
        tap = { id = "library.add" }
        [[step]]
        type = { id = "bookForm.title", text = "Hello from Forge" }
        [[step]]
        expect = { id = "bookForm.title", value = "Hello from Forge" }
        [[step]]
        screenshot = "form"
    "#;

    /// The Claude app's chat screen, as G0 read it on Cati.
    fn chat() -> UiNode {
        UiNode {
            kind: "Application".into(),
            children: vec![
                node("TextView", "chat-input", "Write your prompt to Claude"),
                node("Button", "Send", "Send"),
                UiNode {
                    kind: "SecureTextField".into(),
                    identifier: Some("pin".into()),
                    enabled: true,
                    visible: true,
                    ..UiNode::default()
                },
            ],
            ..UiNode::default()
        }
    }

    const CHAT: &str = r#"
        name = "Say hello"
        app = "com.anthropic.claude"
        timeout = 0

        [[step]]
        activate = {}
        [[step]]
        type = { id = "chat-input", text = "Hello" }
        [[step]]
        tap = { id = "Send" }
    "#;

    fn foreign_run(level: policy::Level, script: &str) -> (Report, FakeDriver, String) {
        let script = Script::parse(script).unwrap();
        let mut driver = FakeDriver::new(chat());
        driver.front = Some("com.anthropic.claude".into());
        let dir = tempfile::tempdir().unwrap();
        let journal = policy::Journal::create(dir.path()).unwrap();
        let path = journal.path.clone();
        let mut gate = Gate::foreign(
            "com.anthropic.claude",
            level,
            Box::new(policy::NoApprover),
            Some(journal),
        );
        let mut out = Vec::new();
        let report = run_gated(
            &script,
            "com.anthropic.claude",
            &mut driver,
            dir.path(),
            &mut out,
            &mut gate,
        )
        .unwrap();
        drop(gate);
        let journal = std::fs::read_to_string(path).unwrap();
        (report, driver, journal)
    }

    #[test]
    fn another_app_types_but_never_sends_without_approval() {
        let (report, driver, journal) = foreign_run(policy::Level::Operate, CHAT);
        assert_eq!(report.passed, 2);
        let (step, why) = report.failed.unwrap();
        assert_eq!(step, 3);
        assert!(why.contains("needs the person's approval"), "{why}");
        assert!(!driver.calls.iter().any(|c| c.starts_with("tap")));
        assert!(driver
            .calls
            .iter()
            .any(|c| c.starts_with("type chat-input")));
        // The journal has the length of what was typed, never the text.
        assert!(journal.contains("\"typed_chars\":5"), "{journal}");
        assert!(!journal.contains("Hello"), "{journal}");
        assert!(journal.contains("\"refused\""), "{journal}");
    }

    #[test]
    fn observe_reads_but_does_not_type() {
        let (report, driver, _) = foreign_run(policy::Level::Observe, CHAT);
        let (step, why) = report.failed.unwrap();
        assert_eq!(step, 2);
        assert!(why.contains("`operate`"), "{why}");
        assert!(!driver.calls.iter().any(|c| c.starts_with("type")));
        // The bound app is in front, so the failure's tree and screenshot
        // are kept as usual.
        assert!(report.tree_file.is_some());
    }

    #[test]
    fn return_and_secure_fields_are_gated_in_another_app() {
        let script = r#"
            name = "x"
            app = "com.anthropic.claude"
            timeout = 0
            [[step]]
            type = { id = "chat-input", text = "Hi\n" }
        "#;
        let (report, driver, _) = foreign_run(policy::Level::Operate, script);
        assert!(report.failed.unwrap().1.contains("Return"));
        assert!(!driver.calls.iter().any(|c| c.starts_with("type")));
        let script = r#"
            name = "x"
            app = "com.anthropic.claude"
            timeout = 0
            [[step]]
            type = { id = "pin", text = "1234" }
        "#;
        let (report, _, _) = foreign_run(policy::Level::Full, script);
        assert!(report.failed.unwrap().1.contains("secure field"));
        // Full: sending is allowed.
        let (report, driver, _) = foreign_run(policy::Level::Full, CHAT);
        assert!(report.failed.is_none());
        assert!(driver.calls.iter().any(|c| c == "tap Send"));
    }

    #[test]
    fn another_app_never_screenshots_what_else_is_in_front() {
        let script = Script::parse(
            r#"
            name = "x"
            app = "com.anthropic.claude"
            timeout = 0
            [[step]]
            screenshot = "chat"
        "#,
        )
        .unwrap();
        let mut driver = FakeDriver::new(chat());
        driver.front = Some("com.example.bank".into());
        let dir = tempfile::tempdir().unwrap();
        let mut gate = Gate::foreign(
            "com.anthropic.claude",
            policy::Level::Operate,
            Box::new(policy::NoApprover),
            None,
        );
        let report = run_gated(
            &script,
            "com.anthropic.claude",
            &mut driver,
            dir.path(),
            &mut Vec::new(),
            &mut gate,
        )
        .unwrap();
        assert!(report
            .failed
            .unwrap()
            .1
            .contains("com.example.bank is in front"));
        assert!(report.screenshots.is_empty());
        assert!(report.tree_file.is_none());
        assert!(!driver.calls.iter().any(|c| c == "screenshot"));
    }

    #[test]
    fn an_empty_field_matches_an_empty_value() {
        let script = Script::parse(
            r#"
            name = "x"
            app = "a"
            timeout = 0
            [[step]]
            expect = { id = "bookForm.title", value = "" }
        "#,
        )
        .unwrap();
        let mut driver = in_front(form());
        let dir = tempfile::tempdir().unwrap();
        let report = run(&script, "a", &mut driver, dir.path(), &mut Vec::new()).unwrap();
        assert!(report.failed.is_none(), "{:?}", report.failed);
    }

    #[test]
    fn runs_the_m0_steps() {
        let script = Script::parse(M0).unwrap();
        let mut driver = FakeDriver::new(library());
        driver.on_tap.push(("library.add".into(), form()));
        let dir = tempfile::tempdir().unwrap();
        let mut out = Vec::new();
        let report = run(
            &script,
            "XTL-1.org.forgemobile.pocketlibrary",
            &mut driver,
            dir.path(),
            &mut out,
        )
        .unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(report.failed.is_none(), "{out}");
        assert_eq!(report.passed, 5);
        assert_eq!(
            driver.calls,
            [
                "launch XTL-1.org.forgemobile.pocketlibrary []",
                "tap library.add",
                "type bookForm.title \"Hello from Forge\"",
                "screenshot"
            ]
        );
        assert!(dir.path().join("form.png").exists());
        assert!(out.contains("ok     2. tap id `library.add`"), "{out}");
    }

    #[test]
    fn a_failure_names_the_locator_and_saves_the_screen() {
        let script = Script::parse(
            r#"
            name = "missing"
            app = "a"
            timeout = 0
            [[step]]
            tap = { id = "library.nope" }
        "#,
        )
        .unwrap();
        let mut driver = in_front(library());
        let dir = tempfile::tempdir().unwrap();
        let mut out = Vec::new();
        let report = run(&script, "a", &mut driver, dir.path(), &mut out).unwrap();
        assert_eq!(
            report.failed.unwrap().1,
            "no element matches id `library.nope`"
        );
        let tree = std::fs::read_to_string(dir.path().join("failure-step1-tree.txt")).unwrap();
        assert!(tree.contains("Button id=library.add \"Add Book\""));
        assert!(dir.path().join("failure-step1.png").exists());
        assert_eq!(report.on_screen, ["Button id=library.add \"Add Book\""]);
        assert_eq!(report.failed_step.as_deref(), Some("tap id `library.nope`"));
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(report.tree_file.unwrap()).unwrap()).unwrap();
        assert_eq!(json["children"][0]["identifier"], "library.add");
        let out = String::from_utf8(out).unwrap();
        assert!(
            out.contains("on screen:\n          Button id=library.add"),
            "{out}"
        );
    }

    #[test]
    fn a_stale_element_is_found_again_once() {
        let script =
            Script::parse("name='s'\napp='a'\ntimeout=0\n[[step]]\ntap={id='library.add'}\n")
                .unwrap();
        let mut driver = in_front(library());
        driver.stale_taps = 1;
        let dir = tempfile::tempdir().unwrap();
        let report = run(&script, "a", &mut driver, dir.path(), &mut Vec::new()).unwrap();
        assert!(report.failed.is_none(), "{:?}", report.failed);
        assert_eq!(driver.calls, ["tap library.add"]);

        // Stale twice in a row is a real failure, not retried forever.
        let mut driver = in_front(library());
        driver.stale_taps = 2;
        let report = run(&script, "a", &mut driver, dir.path(), &mut Vec::new()).unwrap();
        assert!(report.failed.unwrap().1.starts_with("element went stale"));
    }

    #[test]
    fn a_tap_that_takes_its_element_away_passes() {
        // Save closes the form: WDA answers the tap with "stale element
        // reference" because the button no longer exists.
        let script = Script::parse(
            "name='g'\napp='a'\ntimeout=0\n[[step]]\ntap={id='bookForm.save'}\n\
             [[step]]\nexpect={id='library.add'}\n",
        )
        .unwrap();
        let mut driver = in_front(form());
        driver.on_tap.push(("bookForm.save".into(), library()));
        driver.stale_after_tap = 1;
        let dir = tempfile::tempdir().unwrap();
        let report = run(&script, "a", &mut driver, dir.path(), &mut Vec::new()).unwrap();
        assert!(report.failed.is_none(), "{:?}", report.failed);
        assert_eq!(report.passed, 2);
        // Tapped once, not again.
        assert_eq!(driver.calls, ["tap bookForm.save"]);
    }

    #[test]
    fn a_stale_tap_on_an_element_still_there_taps_again_once() {
        // The reply was stale but the button is still on screen (the view
        // re-rendered): tap it again; twice is a failure, as before.
        let script =
            Script::parse("name='s'\napp='a'\ntimeout=0\n[[step]]\ntap={id='library.add'}\n")
                .unwrap();
        let mut driver = in_front(library());
        driver.stale_after_tap = 1;
        let dir = tempfile::tempdir().unwrap();
        let report = run(&script, "a", &mut driver, dir.path(), &mut Vec::new()).unwrap();
        assert!(report.failed.is_none(), "{:?}", report.failed);
        assert_eq!(driver.calls, ["tap library.add", "tap library.add"]);

        let mut driver = in_front(library());
        driver.stale_after_tap = 2;
        let report = run(&script, "a", &mut driver, dir.path(), &mut Vec::new()).unwrap();
        assert!(report.failed.unwrap().1.starts_with("element went stale"));
    }

    #[test]
    fn an_expectation_reads_again_after_a_stale_read() {
        let script = Script::parse(
            "name='v'\napp='a'\ntimeout=1\n[[step]]\nexpect={id='library.add', has_label='Add Book'}\n",
        )
        .unwrap();
        let mut driver = in_front(library());
        driver.stale_reads = 1;
        let dir = tempfile::tempdir().unwrap();
        let report = run(&script, "a", &mut driver, dir.path(), &mut Vec::new()).unwrap();
        assert!(report.failed.is_none(), "{:?}", report.failed);
    }

    #[test]
    fn a_wrong_value_fails_with_what_was_found() {
        let script = Script::parse(
            "name='v'\napp='a'\ntimeout=0\n[[step]]\nexpect={id='bookForm.title', value='Hi'}\n",
        )
        .unwrap();
        let mut driver = in_front(form());
        let dir = tempfile::tempdir().unwrap();
        let report = run(&script, "a", &mut driver, dir.path(), &mut Vec::new()).unwrap();
        assert_eq!(
            report.failed.unwrap().1,
            "id `bookForm.title`: expected value \"Hi\", found no value"
        );
    }

    #[test]
    fn refuses_to_act_when_another_app_is_in_front() {
        let script =
            Script::parse("name='w'\napp='a'\ntimeout=0\n[[step]]\ntap={id='library.add'}\n")
                .unwrap();
        let mut driver = FakeDriver::new(library());
        driver.front = Some("com.anthropic.claude".into());
        let dir = tempfile::tempdir().unwrap();
        let report = run(&script, "a", &mut driver, dir.path(), &mut Vec::new()).unwrap();
        assert_eq!(
            report.failed.unwrap().1,
            "a is not in front (com.anthropic.claude is); refusing to act on another app"
        );
        assert!(driver.calls.iter().all(|c| !c.starts_with("tap")));
    }

    #[test]
    fn expecting_gone_after_terminate_needs_no_app_in_front() {
        let script = Script::parse(
            "name='l'\napp='a'\ntimeout=0\n[[step]]\nlaunch={}\n[[step]]\nbackground=3\n\
             [[step]]\nterminate={}\n[[step]]\nexpect={id='nothing.here', gone=true}\n\
             [[step]]\nlaunch={}\n[[step]]\ntap={id='library.add'}\n",
        )
        .unwrap();
        let mut driver = FakeDriver::new(library());
        let dir = tempfile::tempdir().unwrap();
        let report = run(&script, "a", &mut driver, dir.path(), &mut Vec::new()).unwrap();
        assert!(report.failed.is_none(), "{:?}", report.failed);
        assert_eq!(report.passed, 6);
    }

    #[test]
    fn launch_with_arguments_stops_the_app_first() {
        let script = Script::parse(
            "name='l'\napp='a'\n[[step]]\nlaunch={}\n[[step]]\nlaunch={args=['-Reset','YES']}\n",
        )
        .unwrap();
        let mut driver = FakeDriver::new(library());
        let dir = tempfile::tempdir().unwrap();
        let report = run(&script, "a", &mut driver, dir.path(), &mut Vec::new()).unwrap();
        assert!(report.failed.is_none(), "{:?}", report.failed);
        assert_eq!(
            driver.calls,
            [
                "launch a []",
                "terminate a",
                "launch a [\"-Reset\", \"YES\"]"
            ]
        );
    }

    #[test]
    fn taps_the_control_when_a_wrapper_shares_its_identifier() {
        // SwiftUI put tip.keyboardDone on a toolbar wrapper and its Button.
        let mut wrapper = node("Other", "tip.keyboardDone", "");
        wrapper.children = vec![node("Button", "tip.keyboardDone", "Done")];
        let screen = UiNode {
            kind: "Application".into(),
            children: vec![wrapper],
            ..UiNode::default()
        };
        let script = Script::parse(
            "name='d'\napp='a'\ntimeout=0\n[[step]]\ntap={id='tip.keyboardDone'}\n\
             [[step]]\nscroll={id='tip.keyboardDone'}\n[[step]]\nscroll={id='tip.keyboardDone', direction='down'}\n",
        )
        .unwrap();
        let mut driver = in_front(screen);
        let dir = tempfile::tempdir().unwrap();
        let report = run(&script, "a", &mut driver, dir.path(), &mut Vec::new()).unwrap();
        assert!(report.failed.is_none(), "{:?}", report.failed);
        assert_eq!(
            driver.calls,
            [
                "tap tip.keyboardDone",
                "scroll tip.keyboardDone visible",
                "scroll tip.keyboardDone down"
            ]
        );
        // The Button (walk index 2) is picked, not the wrapper (1).
        let picked = crate::wait_for(
            &mut driver,
            &Locator::Id("tip.keyboardDone".into()),
            Duration::ZERO,
        )
        .unwrap();
        assert_eq!(picked, crate::ElementRef("2".into()));

        // A control a tap can't reach loses to a wrapper it can.
        let mut wrapper = node("Other", "tip.keyboardDone", "Done");
        let mut button = node("Button", "tip.keyboardDone", "Done");
        button.visible = false;
        wrapper.children = vec![button];
        let mut driver = in_front(UiNode {
            kind: "Application".into(),
            children: vec![wrapper],
            ..UiNode::default()
        });
        let picked = crate::wait_for(
            &mut driver,
            &Locator::Id("tip.keyboardDone".into()),
            Duration::ZERO,
        )
        .unwrap();
        assert_eq!(picked, crate::ElementRef("1".into()));
        assert!(Script::parse(
            "name='x'\napp='a'\n[[step]]\nscroll={id='a', direction='sideways'}\n"
        )
        .is_err());
    }

    #[test]
    fn rejects_steps_with_zero_or_two_actions() {
        let err = Script::parse("name='x'\napp='a'\n[[step]]\n").unwrap_err();
        assert_eq!(err, "step 1: each step needs exactly one action");
        let err =
            Script::parse("name='x'\napp='a'\n[[step]]\ntap={id='a', label='b'}\n").unwrap_err();
        assert_eq!(
            err,
            "step 1: give exactly one of `id`, `label`, `label_contains`"
        );
        assert!(Script::parse("name='x'\napp='a'\n[[step]]\ntap={x='a'}\n").is_err());
    }
}

#[cfg(test)]
mod example_files {
    /// The test files in the repository must parse.
    #[test]
    fn pocket_library_m0_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/pocket-library/uitests/m0.toml");
        let script = super::Script::load(&path).unwrap();
        assert_eq!(script.app, "org.forgemobile.pocketlibrary");
        assert_eq!(script.steps.len(), 11);
    }

    /// The starter's test, which `forge init` copies with the bundle id filled in.
    #[test]
    fn starter_smoke_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../templates/ios-app/uitests/smoke.toml");
        let script = super::Script::load(&path).unwrap();
        assert_eq!(script.app, "FORGE_BUNDLE_ID");
        assert_eq!(script.steps.len(), 8);
    }
}
