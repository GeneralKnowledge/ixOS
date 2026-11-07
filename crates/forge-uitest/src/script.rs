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
/// failure, are written to `out_dir`.
pub fn run(
    script: &Script,
    app: &str,
    driver: &mut dyn Driver,
    out_dir: &Path,
    out: &mut dyn Write,
) -> Result<Report> {
    std::fs::create_dir_all(out_dir)?;
    let timeout = Duration::from_secs_f64(script.timeout.max(0.0));
    let mut report = Report::default();
    writeln!(out, "{}", script.name)?;
    for (index, step) in script.steps.iter().enumerate() {
        let number = index + 1;
        let started = Instant::now();
        let result = run_step(step, app, driver, timeout, out_dir, out, &mut report);
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
                record_failure(driver, number, out_dir, out, &mut report);
                report.failed = Some((number, error.to_string()));
                report.failed_step = Some(step.describe());
                return Ok(report);
            }
        }
    }
    writeln!(out, "{} steps passed", report.passed)?;
    Ok(report)
}

fn run_step(
    step: &Step,
    app: &str,
    driver: &mut dyn Driver,
    timeout: Duration,
    out_dir: &Path,
    out: &mut dyn Write,
    report: &mut Report,
) -> Result<()> {
    let locate = |t: &Target| t.locator().map_err(UiError::Device);
    if let Some(launch) = &step.launch {
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
    }
    if let Some(target) = &step.tap {
        return act(driver, &locate(target)?, timeout, &mut |d, e| d.tap(e));
    }
    if let Some(typing) = &step.type_text {
        return act(driver, &locate(&typing.target)?, timeout, &mut |d, e| {
            if typing.clear {
                d.clear(e)?;
            }
            d.type_text(e, &typing.text)
        });
    }
    if let Some(target) = &step.clear {
        return act(driver, &locate(target)?, timeout, &mut |d, e| d.clear(e));
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
        let mut mismatch = None;
        for (attribute, wanted) in [("value", &expect.value), ("label", &expect.has_label)] {
            if let Some(wanted) = wanted {
                let found = driver.attribute(&element, attribute)?;
                if found.as_deref() != Some(wanted.as_str()) {
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
            let found = driver.attribute(&element, "enabled")?;
            let is = matches!(found.as_deref(), Some("true") | Some("1"));
            if is != wanted {
                mismatch = Some((
                    if wanted { "enabled" } else { "disabled" }.into(),
                    if is { "enabled" } else { "disabled" }.into(),
                ));
            }
        }
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
}
