//! Evidence bundles (docs/evidence.md): what a run proved, written to
//! `<app>/.forge/runs/<id>/evidence.json` next to the raw output it came from.
//!
//! An app lists its requirements in `requirements.toml`, each naming the
//! tests that prove it. `forge check swift` runs the headless twin (the real
//! Rust core and the portable Swift view model on Linux), reads the XCTest
//! results, and records per requirement whether its twin tests passed. A twin
//! pass is behaviour evidence, never device evidence: the `device` verdict
//! stays `unverified` until a run on an iPhone (`forge ui run`, #61) records
//! one, and the capability ledger's "iPhone: Yes" still needs a person.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::runner::{Cmd, CommandRunner};

pub const SCHEMA: &str = "forge.evidence/1";
pub const REQUIREMENTS_FILE: &str = "requirements.toml";

/// One entry of `requirements.toml`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub id: String,
    pub statement: String,
    /// Tests that prove it headless, as `Class.method` (XCTest).
    #[serde(default)]
    pub twin: Vec<String>,
    /// UI scenarios (paths relative to the app) that prove it on a device.
    #[serde(default)]
    pub device: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequirementsFile {
    #[serde(default)]
    requirement: Vec<Requirement>,
}

/// Reads `<app>/requirements.toml`; `Ok(empty)` when the app has none.
pub fn load_requirements(app: &Path) -> Result<Vec<Requirement>, String> {
    let path = app.join(REQUIREMENTS_FILE);
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    parse_requirements(&text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn parse_requirements(text: &str) -> Result<Vec<Requirement>, String> {
    let file: RequirementsFile = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut seen = std::collections::BTreeSet::new();
    for r in &file.requirement {
        if !seen.insert(r.id.as_str()) {
            return Err(format!("requirement {} is listed twice", r.id));
        }
        if r.twin.is_empty() && r.device.is_empty() {
            return Err(format!(
                "requirement {} names no test (`twin`) and no scenario (`device`)",
                r.id
            ));
        }
    }
    Ok(file.requirement)
}

/// One XCTest case as the run reported it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TestResult {
    /// `Class.method`, the same on Linux and macOS.
    pub name: String,
    pub passed: bool,
    pub seconds: f64,
}

/// XCTest's result lines: `Test Case 'Class.method' passed (0.009 seconds).`
/// on Linux, `Test Case '-[Module.Class method]' failed (0.010 seconds).` on
/// macOS.
pub fn parse_xctest(output: &str) -> Vec<TestResult> {
    let mut results = Vec::new();
    for line in output.lines() {
        let Some(rest) = line.trim().strip_prefix("Test Case '") else {
            continue;
        };
        let Some((raw, tail)) = rest.split_once("' ") else {
            continue;
        };
        let passed = if tail.starts_with("passed") {
            true
        } else if tail.starts_with("failed") {
            false
        } else {
            continue; // `started at …`
        };
        let seconds = tail
            .split_once('(')
            .and_then(|(_, s)| s.split_whitespace().next())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        results.push(TestResult {
            name: normalise_test_name(raw),
            passed,
            seconds,
        });
    }
    results
}

fn normalise_test_name(raw: &str) -> String {
    if let Some(inner) = raw.strip_prefix("-[").and_then(|s| s.strip_suffix(']')) {
        if let Some((class, method)) = inner.split_once(' ') {
            let class = class.rsplit('.').next().unwrap_or(class);
            return format!("{class}.{method}");
        }
    }
    // Linux may prefix the module: `AppHostTests.Class.method`.
    let parts: Vec<&str> = raw.split('.').collect();
    if parts.len() > 2 {
        return parts[parts.len() - 2..].join(".");
    }
    raw.to_string()
}

/// What one requirement's twin tests showed in this run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Every named test ran and passed.
    Passed,
    /// A named test ran and failed.
    Failed,
    /// A named test is in a file this run could not build (it needs Apple's
    /// frameworks), so this run says nothing about it.
    NotRun,
    /// A named test is in no file this run knows: the requirement points at
    /// a test that was renamed or deleted.
    Missing,
    /// No twin test is named; only a device run can prove it.
    NoTwin,
}

/// Judges each requirement from the test results. `skipped_classes` are the
/// test classes in files left out of the run (file stem = class name, the
/// convention every app here follows).
pub fn judge(
    reqs: &[Requirement],
    tests: &[TestResult],
    skipped_classes: &[String],
) -> Vec<(Verdict, Vec<String>)> {
    let by_name: BTreeMap<&str, &TestResult> = tests.iter().map(|t| (t.name.as_str(), t)).collect();
    reqs.iter()
        .map(|r| {
            if r.twin.is_empty() {
                return (Verdict::NoTwin, Vec::new());
            }
            let mut verdict = Verdict::Passed;
            let mut notes = Vec::new();
            for name in &r.twin {
                let v = match by_name.get(name.as_str()) {
                    Some(t) if t.passed => Verdict::Passed,
                    Some(_) => Verdict::Failed,
                    None => {
                        let class = name.split('.').next().unwrap_or("");
                        if skipped_classes.iter().any(|c| c == class) {
                            notes.push(format!("{name} needs Apple's frameworks"));
                            Verdict::NotRun
                        } else {
                            notes.push(format!("{name} did not run: no such test"));
                            Verdict::Missing
                        }
                    }
                };
                verdict = worse(verdict, v);
            }
            (verdict, notes)
        })
        .collect()
}

fn worse(a: Verdict, b: Verdict) -> Verdict {
    let rank = |v| match v {
        Verdict::Passed | Verdict::NoTwin => 0,
        Verdict::NotRun => 1,
        Verdict::Failed => 2,
        Verdict::Missing => 3,
    };
    if rank(b) > rank(a) {
        b
    } else {
        a
    }
}

/// Everything a bundle records about one run.
pub struct Run<'a> {
    pub command: &'a str,
    /// `linux-twin` or `device`.
    pub kind: &'a str,
    pub app: &'a Path,
    pub build: Value,
    pub tests: &'a [TestResult],
    pub requirements: &'a [Requirement],
    pub verdicts: &'a [(Verdict, Vec<String>)],
    /// The raw output, saved as `<file name>` in the bundle.
    pub raw_output: (&'a str, &'a str),
    pub duration_ms: u64,
}

/// Writes `<app>/.forge/runs/<time>-<command>/` with `evidence.json` and the
/// raw output. Returns the bundle directory.
pub fn write_bundle(run: &Run, id: &str) -> Result<PathBuf, String> {
    let slug: String = run
        .command
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let dir = run
        .app
        .join(".forge")
        .join("runs")
        .join(format!("{id}-{slug}"));
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let (log_name, log_text) = run.raw_output;
    fs::write(dir.join(log_name), log_text).map_err(|e| format!("write log: {e}"))?;
    let mut doc = document(run, id);
    hash_artifacts(&mut doc, &dir);
    write_json(&dir, &doc)?;
    Ok(dir)
}

fn write_json(dir: &Path, doc: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(doc).unwrap_or_default();
    fs::write(dir.join("evidence.json"), text + "\n")
        .map_err(|e| format!("write evidence.json: {e}"))
}

/// Adds `sha256` and `bytes` to every listed artifact that exists in `dir`,
/// so a bundle copied elsewhere can be checked against its files.
pub fn hash_artifacts(doc: &mut Value, dir: &Path) {
    let Some(list) = doc["artifacts"].as_array_mut() else {
        return;
    };
    for a in list {
        let Some(rel) = a["path"].as_str() else {
            continue;
        };
        if let Ok(bytes) = fs::read(dir.join(rel)) {
            a["sha256"] = json!(sha256_hex(&bytes));
            a["bytes"] = json!(bytes.len());
        }
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// What a run was built from (`build` in a bundle): the commit, whether the
/// app folder had changes, `tree` (a git tree hash of the app folder as it
/// was, committed or not, so `forge verify` can tell whether the evidence
/// is for the code as it is now) and the forge version.
pub fn provenance(runner: &dyn CommandRunner, app: &Path) -> Value {
    let git = |args: &[&str]| git_line(runner, app, args, None);
    json!({
        "commit": git(&["rev-parse", "HEAD"]),
        "dirty": git(&["status", "--porcelain", "--", "."]).map(|s| !s.is_empty()),
        "tree": app_tree(runner, app),
        "forge": env!("CARGO_PKG_VERSION"),
    })
}

fn git_line(
    runner: &dyn CommandRunner,
    app: &Path,
    args: &[&str],
    index: Option<&Path>,
) -> Option<String> {
    let mut cmd = Cmd::new("git").arg("-C").arg(app).args(args);
    if let Some(index) = index {
        cmd = cmd.env("GIT_INDEX_FILE", index);
    }
    runner
        .capture(&cmd)
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// The git tree hash of the app folder's files as they are on disk,
/// committed or not (ignored files such as `.forge/` are left out). Same
/// files, same hash. Uses a scratch index, so the real index is untouched.
pub fn app_tree(runner: &dyn CommandRunner, app: &Path) -> Option<String> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let index = std::env::temp_dir().join(format!(
        "forge-tree-{}-{}.index",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    // Approved screenshots (uitests/baselines/) say nothing about the code:
    // accepting one must not make the app's evidence stale.
    let tree = git_line(
        runner,
        app,
        &["add", "-A", "--", ".", ":(exclude,glob)**/baselines/**"],
        Some(&index),
    )
    .and_then(|_| git_line(runner, app, &["write-tree"], Some(&index)));
    let _ = fs::remove_file(&index);
    tree.filter(|t| !t.is_empty())
}

/// The app folder a file belongs to: the nearest folder above it with a
/// `forge.toml`.
pub fn app_of(file: &Path) -> Option<PathBuf> {
    let abs = if file.is_absolute() {
        file.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(file)
    };
    abs.ancestors()
        .skip(1)
        .find(|d| d.join("forge.toml").is_file())
        .map(Path::to_path_buf)
}

/// What a `forge ui run` on the phone showed, for its device bundle.
pub struct DeviceRun<'a> {
    /// The scenario file, relative to the app (as `requirements.toml` names it).
    pub scenario: &'a str,
    pub build: Value,
    pub device: Value,
    pub passed_steps: usize,
    /// The failing step: `{ "n", "action", "error", "on_screen", "tree" }`.
    pub failed_step: Option<Value>,
    /// Files in the bundle folder: (kind, path relative to the folder).
    pub artifacts: Vec<(&'static str, String)>,
    pub duration_ms: u64,
    /// The screenshots compared with their baselines
    /// (`forge_uitest::baseline::Report`), unless `--visual off`.
    pub visual: Option<Value>,
    /// Screenshots that differ from their baselines, or gained accessibility
    /// findings: they fail the run like a failed step.
    pub visual_failures: Vec<String>,
    /// The app's log and crash reports during the run (`watch.rs`).
    pub watch: Option<Value>,
}

/// Writes `evidence.json` (kind `device`) into `dir`, the run's output
/// folder, and sets the `device` verdict of every requirement that lists the
/// scenario. Requirements the scenario doesn't prove are left out.
pub fn write_device_bundle(
    run: &DeviceRun,
    reqs: &[Requirement],
    dir: &Path,
    id: &str,
) -> Result<PathBuf, String> {
    let ok = run.failed_step.is_none() && run.visual_failures.is_empty();
    let verdict = if ok { "passed" } else { "failed" };
    let notes: Vec<String> = run
        .visual_failures
        .iter()
        .map(|name| format!("screenshot {name} differs from its baseline"))
        .collect();
    let requirements: Vec<Value> = reqs
        .iter()
        .filter(|r| r.device.iter().any(|s| same_path(s, run.scenario)))
        .map(|r| {
            json!({
                "id": r.id,
                "statement": r.statement,
                "device": { "verdict": verdict, "scenarios": [run.scenario], "notes": notes },
            })
        })
        .collect();
    let mut doc = json!({
        "schema": SCHEMA,
        "id": id,
        "command": "ui run",
        "kind": "device",
        "ok": ok,
        "duration_ms": run.duration_ms,
        "where": {
            "host": format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
            "device": run.device,
        },
        "build": run.build,
        "scenario": run.scenario,
        "steps": { "passed": run.passed_steps, "failed": run.failed_step },
        "visual": run.visual,
        "watch": run.watch,
        "requirements": requirements,
        "artifacts": run.artifacts.iter().map(|(k, p)| json!({"kind": k, "path": p})).collect::<Vec<_>>(),
    });
    fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    hash_artifacts(&mut doc, dir);
    write_json(dir, &doc)?;
    Ok(dir.join("evidence.json"))
}

/// A `forge ui explore` walk, as a device bundle (docs/forge-explore.md).
pub struct ExploreRun {
    /// `build` from [`provenance`], read before the walk, plus `installed_as`.
    pub build: Value,
    /// `{ "udid", "ios_major" }`.
    pub device: Value,
    /// The walk's summary: seed, stopped, actions, screens, issues.
    pub explore: Value,
    /// The app's log and crash reports during the walk (`watch.rs`).
    pub watch: Option<Value>,
    /// Files the watch wrote (`device.log`, `crashes/…`).
    pub artifacts: Vec<(&'static str, String)>,
    /// No error-severity issue (a crash, an unlabelled control).
    pub ok: bool,
    pub duration_ms: u64,
}

/// Writes `evidence.json` (kind `device`, command `ui explore`) into `dir`,
/// the walk's output folder. A walk proves no requirement (nobody wrote down
/// what it checks), so `requirements` is empty; its issues and their replay
/// files are what it contributes. Lists `explore.json`, every screen's
/// screenshot and tree and every replay file as artifacts.
pub fn write_explore_bundle(run: &ExploreRun, dir: &Path, id: &str) -> Result<PathBuf, String> {
    let mut artifacts = Vec::new();
    if dir.join("explore.json").is_file() {
        artifacts.push(json!({"kind": "explore_report", "path": "explore.json"}));
    }
    for (sub, ext, kind) in [
        ("screens", "png", "screenshot"),
        ("screens", "json", "ui_tree"),
        ("repro", "toml", "repro"),
    ] {
        let mut files: Vec<String> = fs::read_dir(dir.join(sub))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == ext))
            .filter_map(|p| {
                p.file_name()
                    .map(|n| format!("{sub}/{}", n.to_string_lossy()))
            })
            .collect();
        files.sort();
        artifacts.extend(files.into_iter().map(|p| json!({"kind": kind, "path": p})));
    }
    artifacts.extend(
        run.artifacts
            .iter()
            .map(|(kind, path)| json!({"kind": kind, "path": path})),
    );
    let mut doc = json!({
        "schema": SCHEMA,
        "id": id,
        "command": "ui explore",
        "kind": "device",
        "ok": run.ok,
        "duration_ms": run.duration_ms,
        "where": {
            "host": format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
            "device": run.device,
        },
        "build": run.build,
        "explore": run.explore,
        "watch": run.watch,
        "requirements": [],
        "artifacts": artifacts,
    });
    fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    hash_artifacts(&mut doc, dir);
    write_json(dir, &doc)?;
    Ok(dir.join("evidence.json"))
}

/// `uitests/tip.toml` and `./uitests/tip.toml` are the same scenario.
fn same_path(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim_start_matches("./").replace('\\', "/");
    norm(a) == norm(b)
}

/// The `evidence.json` document.
pub fn document(run: &Run, id: &str) -> Value {
    let (log_name, _) = run.raw_output;
    let failed = run.tests.iter().filter(|t| !t.passed).count();
    let requirements: Vec<Value> = run
        .requirements
        .iter()
        .zip(run.verdicts)
        .map(|(r, (verdict, notes))| {
            json!({
                "id": r.id,
                "statement": r.statement,
                "twin": {
                    "verdict": verdict,
                    "tests": r.twin,
                    "notes": notes,
                },
                // Only a device run sets this; a Linux run never does.
                "device": {
                    "verdict": "unverified",
                    "scenarios": r.device,
                },
            })
        })
        .collect();
    let ok = failed == 0
        && run
            .verdicts
            .iter()
            .all(|(v, _)| !matches!(v, Verdict::Failed | Verdict::Missing));
    json!({
        "schema": SCHEMA,
        "id": id,
        "command": run.command,
        "kind": run.kind,
        "ok": ok,
        "duration_ms": run.duration_ms,
        "where": {
            "host": format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
            "device": null,
        },
        "build": run.build,
        "tests": {
            "passed": run.tests.len() - failed,
            "failed": failed,
            "cases": run.tests,
        },
        "requirements": requirements,
        "artifacts": [
            { "kind": "log", "path": log_name },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINUX: &str = "\
Test Suite 'LibraryModelTests' started at 2026-11-07 18:45:55.873
Test Case 'LibraryModelTests.testA' started at 2026-11-07 18:45:55.873
Test Case 'LibraryModelTests.testA' passed (0.009 seconds)
Test Case 'LibraryModelTests.testB' started at 2026-11-07 18:45:55.882
/x/Tests/AppHostTests/LibraryModelTests.swift:12: error: LibraryModelTests.testB : XCTAssertEqual failed
Test Case 'LibraryModelTests.testB' failed (0.014 seconds)
";

    #[test]
    fn reads_linux_and_macos_xctest_lines() {
        let r = parse_xctest(LINUX);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].name, "LibraryModelTests.testA");
        assert!(r[0].passed);
        assert_eq!(r[0].seconds, 0.009);
        assert!(!r[1].passed);

        let mac = "Test Case '-[AppHostTests.LibraryModelTests testA]' passed (0.010 seconds).";
        assert_eq!(parse_xctest(mac)[0].name, "LibraryModelTests.testA");
        assert_eq!(
            parse_xctest("Test Case 'AppHostTests.LibraryModelTests.testA' passed (1.0 seconds)")
                [0]
            .name,
            "LibraryModelTests.testA"
        );
    }

    #[test]
    fn requirements_need_a_proof_and_unique_ids() {
        let ok = parse_requirements(
            "[[requirement]]\nid = \"PL-1\"\nstatement = \"x\"\ntwin = [\"A.b\"]\n",
        )
        .unwrap();
        assert_eq!(ok[0].twin, ["A.b"]);
        assert!(
            parse_requirements("[[requirement]]\nid = \"PL-1\"\nstatement = \"x\"\n")
                .unwrap_err()
                .contains("names no test")
        );
        let twice = "[[requirement]]\nid = \"A\"\nstatement = \"x\"\ntwin = [\"A.b\"]\n".repeat(2);
        assert!(parse_requirements(&twice).unwrap_err().contains("twice"));
        assert!(
            parse_requirements("[[requirement]]\nid = \"A\"\nstatement = \"x\"\ntwn = []\n")
                .is_err()
        );
    }

    #[test]
    fn verdicts_keep_not_run_apart_from_missing() {
        let req = |id: &str, twin: &[&str]| Requirement {
            id: id.into(),
            statement: String::new(),
            twin: twin.iter().map(|s| s.to_string()).collect(),
            device: vec!["uitests/x.toml".into()],
        };
        let reqs = [
            req("pass", &["LibraryModelTests.testA"]),
            req(
                "fail",
                &["LibraryModelTests.testA", "LibraryModelTests.testB"],
            ),
            req("apple", &["ReadAloudTests.testSpeech"]),
            req("gone", &["LibraryModelTests.testRenamed"]),
            req("device", &[]),
        ];
        let v: Vec<Verdict> = judge(&reqs, &parse_xctest(LINUX), &["ReadAloudTests".into()])
            .into_iter()
            .map(|(v, _)| v)
            .collect();
        assert_eq!(
            v,
            [
                Verdict::Passed,
                Verdict::Failed,
                Verdict::NotRun,
                Verdict::Missing,
                Verdict::NoTwin
            ]
        );
    }

    #[test]
    fn a_bundle_is_written_and_never_claims_the_device() {
        let app = tempfile::tempdir().unwrap();
        let reqs = [Requirement {
            id: "PL-1".into(),
            statement: "Books survive a relaunch".into(),
            twin: vec!["LibraryModelTests.testA".into()],
            device: vec![],
        }];
        let tests = parse_xctest(LINUX);
        let verdicts = judge(&reqs, &tests, &[]);
        let run = Run {
            command: "check swift",
            kind: "linux-twin",
            app: app.path(),
            build: json!({"commit": "abc"}),
            tests: &tests,
            requirements: &reqs,
            verdicts: &verdicts,
            raw_output: ("swift-test.log", LINUX),
            duration_ms: 5,
        };
        let dir = write_bundle(&run, "2026-11-07T18-00-00").unwrap();
        assert!(dir.ends_with("2026-11-07T18-00-00-check-swift"));
        assert!(dir.join("swift-test.log").is_file());
        let doc: Value =
            serde_json::from_str(&fs::read_to_string(dir.join("evidence.json")).unwrap()).unwrap();
        assert_eq!(doc["schema"], SCHEMA);
        assert_eq!(doc["ok"], false, "testB failed");
        assert_eq!(doc["tests"]["failed"], 1);
        assert_eq!(doc["requirements"][0]["twin"]["verdict"], "passed");
        assert_eq!(doc["requirements"][0]["device"]["verdict"], "unverified");
        assert!(doc["where"]["device"].is_null());
    }

    #[test]
    fn a_device_bundle_judges_only_the_requirements_naming_its_scenario() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("failure-step3.png"), b"png").unwrap();
        let reqs = [
            Requirement {
                id: "T-1".into(),
                statement: "Tip is added".into(),
                twin: vec![],
                device: vec!["./uitests/tip.toml".into()],
            },
            Requirement {
                id: "T-2".into(),
                statement: "Other".into(),
                twin: vec!["A.b".into()],
                device: vec!["uitests/other.toml".into()],
            },
        ];
        let run = DeviceRun {
            scenario: "uitests/tip.toml",
            build: json!({"commit": "abc", "tree": "t1", "installed_as": "XTL-1.org.x"}),
            device: json!({"udid": "0000", "ios_major": 26}),
            passed_steps: 2,
            failed_step: Some(json!({"step": 3, "error": "no element"})),
            artifacts: vec![("screenshot", "failure-step3.png".into())],
            duration_ms: 9,
            visual: None,
            visual_failures: vec![],
            watch: None,
        };
        let path = write_device_bundle(&run, &reqs, dir.path(), "2026-11-08T09-00-00").unwrap();
        let doc: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(doc["kind"], "device");
        assert_eq!(doc["ok"], false);
        assert_eq!(doc["requirements"].as_array().unwrap().len(), 1);
        assert_eq!(doc["requirements"][0]["id"], "T-1");
        assert_eq!(doc["requirements"][0]["device"]["verdict"], "failed");
        assert_eq!(doc["where"]["device"]["udid"], "0000");
        assert_eq!(doc["artifacts"][0]["sha256"], sha256_hex(b"png"));
        assert_eq!(doc["artifacts"][0]["bytes"], 3);
    }

    #[test]
    fn baselines_are_left_out_of_the_app_tree_hash() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path();
        let runner = crate::runner::RealRunner;
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(app)
                .args(args)
                .output()
                .unwrap()
        };
        if !git(&["init", "-q"]).status.success() {
            return; // no git here
        }
        fs::create_dir_all(app.join("uitests/baselines/m0")).unwrap();
        fs::write(app.join("forge.toml"), "").unwrap();
        let before = app_tree(&runner, app).expect("a tree hash");
        fs::write(app.join("uitests/baselines/m0/form@2x3.png"), "png").unwrap();
        assert_eq!(app_tree(&runner, app).as_deref(), Some(before.as_str()));
        fs::write(app.join("uitests/m0.toml"), "x").unwrap();
        assert_ne!(app_tree(&runner, app).as_deref(), Some(before.as_str()));
    }

    #[test]
    fn a_visual_change_fails_the_device_verdict_with_a_note() {
        let dir = tempfile::tempdir().unwrap();
        let reqs = vec![Requirement {
            id: "T-1".into(),
            statement: "Tip is added".into(),
            twin: vec![],
            device: vec!["uitests/tip.toml".into()],
        }];
        let run = DeviceRun {
            scenario: "uitests/tip.toml",
            build: json!({}),
            device: json!({}),
            passed_steps: 5,
            failed_step: None,
            artifacts: vec![("visual_diff", "visual/form.diff.png".into())],
            duration_ms: 9,
            visual: Some(
                json!({"mode": "check", "screens": [{"name": "form", "verdict": "changed"}]}),
            ),
            visual_failures: vec!["form".into()],
            watch: None,
        };
        let path = write_device_bundle(&run, &reqs, dir.path(), "id").unwrap();
        let doc: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(doc["ok"], false);
        assert_eq!(doc["steps"]["failed"], Value::Null);
        assert_eq!(doc["visual"]["screens"][0]["verdict"], "changed");
        let device = &doc["requirements"][0]["device"];
        assert_eq!(device["verdict"], "failed");
        assert_eq!(
            device["notes"][0],
            "screenshot form differs from its baseline"
        );
    }

    #[test]
    fn an_explore_bundle_lists_its_files_and_proves_no_requirement() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        fs::create_dir_all(dir.join("screens")).unwrap();
        fs::create_dir_all(dir.join("repro")).unwrap();
        fs::write(dir.join("explore.json"), "{}").unwrap();
        fs::write(dir.join("screens/ab12.png"), [1u8, 2]).unwrap();
        fs::write(dir.join("screens/ab12.json"), "{}").unwrap();
        fs::write(dir.join("repro/issue-1-app_closed.toml"), "app = \"x\"").unwrap();
        let run = ExploreRun {
            build: json!({"commit": "abc", "dirty": false}),
            device: json!({"udid": "0000", "ios_major": 26}),
            explore: json!({"seed": 1, "issues": [{"issue": "app_closed", "severity": "error"}]}),
            watch: Some(json!({"log_lines": 0, "crashes": []})),
            artifacts: vec![("device_log", "device.log".into())],
            ok: false,
            duration_ms: 5,
        };
        let path = write_explore_bundle(&run, dir, "2026-11-08T13-00-00").unwrap();
        let doc: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(doc["schema"], SCHEMA);
        assert_eq!(doc["kind"], "device");
        assert_eq!(doc["command"], "ui explore");
        assert_eq!(doc["ok"], false);
        assert_eq!(doc["requirements"], json!([]));
        let kinds: Vec<(&str, &str)> = doc["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| (a["kind"].as_str().unwrap(), a["path"].as_str().unwrap()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("explore_report", "explore.json"),
                ("screenshot", "screens/ab12.png"),
                ("ui_tree", "screens/ab12.json"),
                ("repro", "repro/issue-1-app_closed.toml"),
                ("device_log", "device.log"),
            ]
        );
        assert_eq!(doc["artifacts"][1]["bytes"], 2);
    }

    #[test]
    fn the_app_of_a_scenario_is_the_nearest_folder_with_forge_toml() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Tipster");
        fs::create_dir_all(app.join("uitests")).unwrap();
        fs::write(app.join("forge.toml"), "").unwrap();
        assert_eq!(app_of(&app.join("uitests/tip.toml")), Some(app));
        assert_eq!(app_of(&dir.path().join("x.toml")), None);
    }
}
