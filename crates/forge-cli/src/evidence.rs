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
    let doc = document(run, id);
    let text = serde_json::to_string_pretty(&doc).unwrap_or_default();
    fs::write(dir.join("evidence.json"), text + "\n")
        .map_err(|e| format!("write evidence.json: {e}"))?;
    Ok(dir)
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
}
