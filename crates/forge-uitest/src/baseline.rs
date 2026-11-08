//! Approved screenshots for a UI test file, and comparing a run with them.
//!
//! A scenario's baselines live next to it, in git:
//!
//! ```text
//! uitests/m0.toml
//! uitests/baselines/m0/1-library@1290x2796.png        the approved screenshot
//! uitests/baselines/m0/1-library@1290x2796.tree.json  its accessibility tree
//! ```
//!
//! The size is part of the name, so one scenario can hold baselines for
//! several phones. A run's screenshots are compared after the run, on Linux;
//! nothing here needs the phone. See docs/visual-regression.md.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::script::{Target, Visual};
use crate::tree::UiNode;
use crate::visual::{self, Comparison, Options, PixelRect, Region, TreeChange};
use crate::Locator;

/// What to do with a run's screenshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Compare with the baselines; a change fails the run.
    Check,
    /// Make every screenshot the baseline (no comparison).
    Record,
}

/// `uitests/baselines/<stem>` for `uitests/<stem>.toml`.
pub fn dir_for(scenario: &Path) -> PathBuf {
    let stem = scenario
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "scenario".into());
    scenario
        .parent()
        .unwrap_or(Path::new("."))
        .join("baselines")
        .join(stem)
}

/// The tree saved next to a screenshot: `form.png` → `form.tree.json`.
pub fn tree_path(png: &Path) -> PathBuf {
    png.with_extension("tree.json")
}

/// The screenshots of a run that are compared: the scenario's named ones,
/// not the screenshot taken at a failure.
pub fn compared(screenshots: &[PathBuf]) -> Vec<PathBuf> {
    screenshots
        .iter()
        .filter(|p| {
            p.file_stem()
                .is_some_and(|s| !s.to_string_lossy().starts_with("failure-step"))
        })
        .cloned()
        .collect()
}

/// One screenshot's result.
#[derive(Debug, Clone, Serialize)]
pub struct Screen {
    pub name: String,
    /// `match`, `changed`, `new` (no baseline for this size) or `recorded`.
    pub verdict: &'static str,
    /// The screenshot, relative to the run folder.
    pub screenshot: String,
    /// The baseline it was compared with or written to.
    pub baseline: String,
    pub width: u32,
    pub height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<Region>,
    /// Changed areas too small to count (a blinking cursor).
    #[serde(skip_serializing_if = "is_zero")]
    pub ignored_regions: usize,
    /// The diff image, relative to the run folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tree_changes: Vec<TreeChange>,
    /// Accessibility findings the baseline didn't have.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub new_findings: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

impl Screen {
    /// Whether this screenshot fails a check run.
    pub fn failed(&self) -> bool {
        self.verdict == "changed" || !self.new_findings.is_empty()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub mode: Mode,
    /// Where the baselines are (as given, usually relative to the app).
    pub baselines: String,
    pub screens: Vec<Screen>,
}

impl Report {
    pub fn failed(&self) -> bool {
        self.screens.iter().any(Screen::failed)
    }

    /// A line per screenshot, then what changed, for the terminal.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for s in &self.screens {
            let detail = match s.verdict {
                "changed" if s.regions.is_empty() => " (size changed)".to_string(),
                "changed" => format!(
                    " ({} area{}, {:.2}% of pixels), diff {}",
                    s.regions.len(),
                    if s.regions.len() == 1 { "" } else { "s" },
                    s.changed_ratio.unwrap_or(0.0) * 100.0,
                    s.diff.as_deref().unwrap_or("-")
                ),
                _ => String::new(),
            };
            out.push_str(&format!("  {:<8} {}{detail}\n", s.verdict, s.name));
            for r in s.regions.iter().take(5) {
                let at = format!(
                    "{}x{} pt at ({}, {})",
                    r.rect.width, r.rect.height, r.rect.x, r.rect.y
                );
                if r.elements.is_empty() {
                    out.push_str(&format!("           area {at}\n"));
                } else {
                    out.push_str(&format!(
                        "           area {at}: {}\n",
                        r.elements.join("; ")
                    ));
                }
            }
            for c in s.tree_changes.iter().take(10) {
                out.push_str(&format!("           {c}\n"));
            }
            for f in &s.new_findings {
                out.push_str(&format!("           accessibility, new: {f}\n"));
            }
            for n in &s.notes {
                out.push_str(&format!("           {n}\n"));
            }
        }
        out
    }
}

/// Compares (or records) each screenshot of a run. `run_dir` is the run's
/// output folder; diff images go to `run_dir/visual/`.
pub fn compare_run(
    scenario: &Path,
    settings: &Visual,
    screenshots: &[PathBuf],
    run_dir: &Path,
    mode: Mode,
) -> Result<Report, String> {
    let base_dir = dir_for(scenario);
    let mut screens = Vec::new();
    for png in compared(screenshots) {
        screens.push(one(&png, &base_dir, settings, run_dir, mode)?);
    }
    Ok(Report {
        mode,
        baselines: base_dir.display().to_string(),
        screens,
    })
}

fn rel(path: &Path, to: &Path) -> String {
    path.strip_prefix(to).unwrap_or(path).display().to_string()
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn load_tree(path: &Path) -> Option<UiNode> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn one(
    png: &Path,
    base_dir: &Path,
    settings: &Visual,
    run_dir: &Path,
    mode: Mode,
) -> Result<Screen, String> {
    let name = png
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let bytes = read(png)?;
    let (width, height) =
        visual::png_size(&bytes).map_err(|e| format!("{}: {e}", png.display()))?;
    let stem = format!("{name}@{width}x{height}");
    let base_png = base_dir.join(format!("{stem}.png"));
    let base_tree = base_dir.join(format!("{stem}.tree.json"));
    let mut screen = Screen {
        name: name.clone(),
        verdict: "match",
        screenshot: rel(png, run_dir),
        baseline: base_png.display().to_string(),
        width,
        height,
        changed_ratio: None,
        regions: Vec::new(),
        ignored_regions: 0,
        diff: None,
        tree_changes: Vec::new(),
        new_findings: Vec::new(),
        notes: Vec::new(),
    };
    if mode == Mode::Record {
        store(png, &base_png)?;
        screen.verdict = "recorded";
        return Ok(screen);
    }
    if !base_png.is_file() {
        screen.verdict = "new";
        let others = other_sizes(base_dir, &name);
        screen.notes.push(if others.is_empty() {
            "no baseline yet: look at the screenshot, then `forge ui accept <run>`".into()
        } else {
            format!(
                "no baseline at {width}x{height} (there is one at {}): another phone?",
                others.join(", ")
            )
        });
        return Ok(screen);
    }

    let actual = visual::decode_png(&bytes)?;
    let baseline = visual::decode_png(&read(&base_png)?)
        .map_err(|e| format!("{}: {e}", base_png.display()))?;
    let tree = load_tree(&tree_path(png));
    let old_tree = load_tree(&base_tree);
    let scale = visual::scale_of(width, tree.as_ref().or(old_tree.as_ref()));
    let mut opts = Options {
        scale,
        ..Options::default()
    };
    if let Some(t) = settings.tolerance {
        opts.tolerance = t;
    }
    if let Some(m) = settings.min_region_pt {
        opts.min_region_pt = m;
    }
    if settings.status_bar_masked() {
        opts.masks.push(visual::status_bar(width, height, scale));
    }
    for target in &settings.mask {
        let found = [tree.as_ref(), old_tree.as_ref()]
            .into_iter()
            .flatten()
            .flat_map(|t| frames(t, target, scale))
            .collect::<Vec<_>>();
        if found.is_empty() {
            screen
                .notes
                .push(format!("mask {}: not on this screen", describe(target)));
        }
        opts.masks.extend(found);
    }
    if tree.is_none() {
        screen
            .notes
            .push("no accessibility tree was saved with this screenshot".into());
    }

    let comparison: Comparison = visual::compare(&baseline, &actual, &opts);
    screen.changed_ratio = Some((comparison.changed_ratio() * 10_000.0).round() / 10_000.0);
    screen.ignored_regions = comparison.ignored_regions;
    screen.regions = comparison.regions.clone();
    if let Some(t) = &tree {
        for r in &mut screen.regions {
            r.locate(t);
        }
    }
    if let (Some(old), Some(new)) = (&old_tree, &tree) {
        screen.tree_changes = visual::tree_changes(old, new);
        screen.new_findings = visual::new_findings(old, new);
    }
    if comparison.changed() {
        screen.verdict = "changed";
        if let Some(diff) = &comparison.diff {
            let dir = run_dir.join("visual");
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let path = dir.join(format!("{name}.diff.png"));
            std::fs::write(&path, visual::encode_png(diff)?)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            screen.diff = Some(rel(&path, run_dir));
        }
    }
    Ok(screen)
}

/// Copies a screenshot and its tree to a baseline path.
fn store(png: &Path, base_png: &Path) -> Result<(), String> {
    if let Some(dir) = base_png.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::copy(png, base_png).map_err(|e| format!("{}: {e}", base_png.display()))?;
    let tree = tree_path(png);
    let base_tree = tree_path(base_png);
    if tree.is_file() {
        std::fs::copy(&tree, &base_tree).map_err(|e| format!("{}: {e}", base_tree.display()))?;
    } else {
        // An old tree would describe a different screen.
        let _ = std::fs::remove_file(&base_tree);
    }
    Ok(())
}

/// Makes a run's screenshots the scenario's baselines: all of them, or only
/// those named in `only`. Returns the baselines written.
pub fn accept(scenario: &Path, run_dir: &Path, only: &[String]) -> Result<Vec<PathBuf>, String> {
    let base_dir = dir_for(scenario);
    let mut pngs: Vec<PathBuf> = std::fs::read_dir(run_dir)
        .map_err(|e| format!("{}: {e}", run_dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    pngs.sort();
    let pngs = compared(&pngs);
    let name = |p: &PathBuf| {
        p.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    for wanted in only {
        if !pngs.iter().any(|p| &name(p) == wanted) {
            return Err(format!(
                "{} has no screenshot named `{wanted}` (it has: {})",
                run_dir.display(),
                pngs.iter().map(name).collect::<Vec<_>>().join(", ")
            ));
        }
    }
    let mut written = Vec::new();
    for png in pngs
        .iter()
        .filter(|p| only.is_empty() || only.contains(&name(p)))
    {
        let (w, h) = visual::png_size(&read(png)?)?;
        let base = base_dir.join(format!("{}@{w}x{h}.png", name(png)));
        store(png, &base)?;
        written.push(base);
    }
    if written.is_empty() {
        return Err(format!(
            "{} has no screenshots to accept",
            run_dir.display()
        ));
    }
    Ok(written)
}

fn other_sizes(base_dir: &Path, name: &str) -> Vec<String> {
    let prefix = format!("{name}@");
    let mut sizes: Vec<String> = std::fs::read_dir(base_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            let size = file.strip_prefix(&prefix)?.strip_suffix(".png")?.to_owned();
            Some(size)
        })
        .collect();
    sizes.sort();
    sizes
}

fn describe(target: &Target) -> String {
    match target.locator() {
        Ok(l) => l.to_string(),
        Err(e) => e,
    }
}

/// The frames, in pixels, of the visible elements a mask names.
fn frames(tree: &UiNode, target: &Target, scale: f64) -> Vec<PixelRect> {
    let Ok(locator) = target.locator() else {
        return Vec::new();
    };
    tree.walk()
        .filter(|n| n.visible || n.kind == "Application")
        .filter(|n| match &locator {
            Locator::Id(id) => n.identifier.as_deref() == Some(id),
            Locator::Label(label) => n.label.as_deref() == Some(label),
            Locator::LabelContains(text) => n
                .label
                .as_deref()
                .is_some_and(|l| l.contains(text.as_str())),
        })
        .filter_map(|n| n.frame)
        .map(|f| PixelRect::from_points(f, scale))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Rect;
    use crate::visual::{encode_png, Image};

    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const BLACK: [u8; 4] = [0, 0, 0, 255];

    /// A 100 x 200 pt screen at 3x with a Save button and a clock label.
    fn tree(save_label: &str) -> UiNode {
        let el = |kind: &str, id: Option<&str>, label: &str, y: f64| UiNode {
            kind: kind.into(),
            identifier: id.map(Into::into),
            label: Some(label.into()),
            value: None,
            enabled: true,
            visible: true,
            frame: Some(Rect {
                x: 10.0,
                y,
                width: 40.0,
                height: 20.0,
            }),
            children: Vec::new(),
        };
        UiNode {
            kind: "Application".into(),
            label: Some("App".into()),
            visible: false,
            enabled: true,
            frame: Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 200.0,
            }),
            children: vec![
                el("Button", Some("form.save"), save_label, 100.0),
                el("StaticText", Some("form.clock"), "10:41", 150.0),
            ],
            ..UiNode::default()
        }
    }

    fn shoot(dir: &Path, name: &str, image: &Image, tree: &UiNode) -> PathBuf {
        let png = dir.join(format!("{name}.png"));
        std::fs::write(&png, encode_png(image).unwrap()).unwrap();
        std::fs::write(tree_path(&png), serde_json::to_vec(tree).unwrap()).unwrap();
        png
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        scenario: PathBuf,
        root: PathBuf,
    }

    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join("uitests")).unwrap();
        let scenario = root.join("uitests/form.toml");
        Fixture {
            _tmp: tmp,
            scenario,
            root,
        }
    }

    fn run_dir(f: &Fixture, n: u32) -> PathBuf {
        let dir = f.root.join(format!(".forge/runs/{n}-ui-run"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn first_run_is_new_then_accepted_then_matches() {
        let f = fixture();
        let run1 = run_dir(&f, 1);
        let screen = Image::new(300, 600, WHITE);
        let shots = vec![
            shoot(&run1, "form", &screen, &tree("Save")),
            run1.join("failure-step3.png"),
        ];
        let report =
            compare_run(&f.scenario, &Visual::default(), &shots, &run1, Mode::Check).unwrap();
        assert_eq!(
            report.screens.len(),
            1,
            "failure screenshots are not compared"
        );
        assert_eq!(report.screens[0].verdict, "new");
        assert!(!report.failed());

        let written = accept(&f.scenario, &run1, &[]).unwrap();
        assert_eq!(
            written,
            vec![f.root.join("uitests/baselines/form/form@300x600.png")]
        );
        assert!(f
            .root
            .join("uitests/baselines/form/form@300x600.tree.json")
            .is_file());
        assert!(accept(&f.scenario, &run1, &["nope".into()]).is_err());

        let run2 = run_dir(&f, 2);
        let shots = vec![shoot(&run2, "form", &screen, &tree("Save"))];
        let report =
            compare_run(&f.scenario, &Visual::default(), &shots, &run2, Mode::Check).unwrap();
        assert_eq!(report.screens[0].verdict, "match");
        assert_eq!(report.screens[0].changed_ratio, Some(0.0));
        assert!(!report.failed());
    }

    #[test]
    fn a_change_fails_with_a_diff_located_on_elements() {
        let f = fixture();
        let run1 = run_dir(&f, 1);
        let before = Image::new(300, 600, WHITE);
        compare_run(
            &f.scenario,
            &Visual::default(),
            &[shoot(&run1, "form", &before, &tree("Save"))],
            &run1,
            Mode::Record,
        )
        .unwrap();

        // The Save button (y 100-120 pt) is redrawn with another label.
        let mut after = before.clone();
        after.fill(
            PixelRect {
                x: 45,
                y: 310,
                width: 60,
                height: 40,
            },
            BLACK,
        );
        // The status bar changed too (masked) and the clock (masked by the
        // scenario).
        after.fill(
            PixelRect {
                x: 0,
                y: 0,
                width: 300,
                height: 30,
            },
            BLACK,
        );
        after.fill(
            PixelRect {
                x: 40,
                y: 455,
                width: 50,
                height: 40,
            },
            BLACK,
        );
        let run2 = run_dir(&f, 2);
        let settings: Visual = toml::from_str("mask = [{ id = \"form.clock\" }]").unwrap();
        let report = compare_run(
            &f.scenario,
            &settings,
            &[shoot(&run2, "form", &after, &tree("Done"))],
            &run2,
            Mode::Check,
        )
        .unwrap();
        let s = &report.screens[0];
        assert_eq!(s.verdict, "changed", "{s:?}");
        assert!(report.failed());
        assert_eq!(s.regions.len(), 1, "{:?}", s.regions);
        assert_eq!(s.regions[0].elements, vec!["Button id=form.save \"Done\""]);
        assert_eq!(s.diff.as_deref(), Some("visual/form.diff.png"));
        assert!(run2.join("visual/form.diff.png").is_file());
        assert_eq!(s.tree_changes.len(), 1);
        assert_eq!(s.tree_changes[0].change, "label");
        let text = report.render();
        assert!(text.contains("changed  form (1 area"), "{text}");
        assert!(text.contains("label Button id=form.save"), "{text}");
    }

    #[test]
    fn another_phone_size_is_new_and_says_so() {
        let f = fixture();
        let run1 = run_dir(&f, 1);
        compare_run(
            &f.scenario,
            &Visual::default(),
            &[shoot(
                &run1,
                "form",
                &Image::new(300, 600, WHITE),
                &tree("Save"),
            )],
            &run1,
            Mode::Record,
        )
        .unwrap();
        let run2 = run_dir(&f, 2);
        let report = compare_run(
            &f.scenario,
            &Visual::default(),
            &[shoot(
                &run2,
                "form",
                &Image::new(200, 400, WHITE),
                &tree("Save"),
            )],
            &run2,
            Mode::Check,
        )
        .unwrap();
        assert_eq!(report.screens[0].verdict, "new");
        assert!(report.screens[0].notes[0].contains("300x600"));
    }

    #[test]
    fn baselines_live_next_to_the_scenario() {
        assert_eq!(
            dir_for(Path::new("app/uitests/m0.toml")),
            PathBuf::from("app/uitests/baselines/m0")
        );
    }
}
