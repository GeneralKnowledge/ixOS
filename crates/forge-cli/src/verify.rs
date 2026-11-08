//! `forge verify`: does every requirement have current evidence?
//!
//! Reads `<app>/requirements.toml` and the bundles in `<app>/.forge/runs/`
//! (docs/evidence.md), and prints one row per requirement: its newest twin
//! verdict (from `forge check swift`), its newest device verdict (from
//! `forge ui run`), and whether each was recorded for the app's files as
//! they are now. It is the agent's stop condition and the page a person
//! reviews. It runs nothing itself.

use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use crate::evidence::{self, Requirement};
use crate::runner::CommandRunner;

/// One proof (twin or device) of one requirement, from the newest bundle
/// that judged it.
#[derive(Debug, Clone, PartialEq)]
pub struct Proof {
    /// `passed`, `failed`, `not_run`, `missing`, or `none` (no bundle yet).
    pub verdict: String,
    /// The bundle's id (its UTC start time).
    pub run: Option<String>,
    pub commit: Option<String>,
    /// Recorded for the app's files as they are now (same `build.tree`).
    /// `None` when either side has no tree (not a git checkout, or a bundle
    /// written before provenance was added).
    pub current: Option<bool>,
}

impl Proof {
    fn none() -> Self {
        Proof {
            verdict: "none".into(),
            run: None,
            commit: None,
            current: None,
        }
    }

    /// Passed, for the code as it is now.
    fn holds(&self) -> bool {
        self.verdict == "passed" && self.current == Some(true)
    }

    fn json(&self) -> Value {
        json!({
            "verdict": self.verdict,
            "run": self.run,
            "commit": self.commit,
            "current": self.current,
        })
    }

    fn cell(&self) -> String {
        match (&self.run, self.current) {
            (None, _) => "none".into(),
            (Some(_), Some(true)) => self.verdict.clone(),
            (Some(_), Some(false)) => format!("{} (stale)", self.verdict),
            (Some(_), None) => format!("{} (?)", self.verdict),
        }
    }
}

#[derive(Debug)]
pub struct Row {
    pub id: String,
    pub statement: String,
    /// `None` when the requirement names no twin test.
    pub twin: Option<Proof>,
    /// `None` when the requirement names no device scenario.
    pub device: Option<Proof>,
}

/// Where a requirement stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Every proof it asks for passed for the current files.
    Proven,
    /// Something can still be done here: run a check, fix a failure.
    Open,
    /// `--twin-only`: nothing Linux can run proves it (only a device
    /// scenario, or its twin test needs Apple's frameworks: `not_run`).
    /// It needs the phone or macOS, and doesn't fail the command.
    NotHere,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Proven => "proven",
            Status::Open => "open",
            Status::NotHere => "not_here",
        }
    }
}

impl Row {
    /// With `twin_only`, a device proof is shown but not required (a session
    /// without the phone).
    pub fn status(&self, twin_only: bool) -> Status {
        let twin = self.twin.as_ref().is_none_or(Proof::holds);
        let device = twin_only || self.device.as_ref().is_none_or(Proof::holds);
        if twin && device && (self.twin.is_some() || !twin_only) {
            return Status::Proven;
        }
        let linux_cannot = match &self.twin {
            None => true,
            Some(p) => p.verdict == "not_run" && p.current != Some(false),
        };
        if twin_only && linux_cannot {
            Status::NotHere
        } else {
            Status::Open
        }
    }
}

/// Every `evidence.json` under `<app>/.forge/runs/`, oldest first (run ids
/// are UTC times, so names sort by time).
pub fn bundles(app: &Path) -> Vec<Value> {
    let Ok(entries) = fs::read_dir(app.join(".forge/runs")) else {
        return Vec::new();
    };
    let mut dirs: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join("evidence.json").is_file())
        .collect();
    dirs.sort();
    dirs.iter()
        .filter_map(|d| fs::read_to_string(d.join("evidence.json")).ok())
        .filter_map(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|v| {
            v["schema"]
                .as_str()
                .is_some_and(|s| s.starts_with("forge.evidence/"))
        })
        .collect()
}

/// The table: for each requirement, the newest bundle of each kind that
/// judged it, compared with `tree_now`.
pub fn rows(reqs: &[Requirement], bundles: &[Value], tree_now: Option<&str>) -> Vec<Row> {
    let newest = |kind: &str, id: &str, side: &str| -> Proof {
        bundles
            .iter()
            .rev()
            .filter(|b| b["kind"] == kind)
            .find_map(|b| {
                let r = b["requirements"]
                    .as_array()?
                    .iter()
                    .find(|r| r["id"] == id)?;
                let verdict = r[side]["verdict"].as_str()?;
                // A twin bundle carries `device: unverified` for every
                // requirement; that is not a device result.
                if verdict == "unverified" || verdict == "no_twin" {
                    return None;
                }
                let tree = b["build"]["tree"].as_str();
                Some(Proof {
                    verdict: verdict.to_string(),
                    run: b["id"].as_str().map(str::to_string),
                    commit: b["build"]["commit"].as_str().map(str::to_string),
                    current: match (tree, tree_now) {
                        (Some(a), Some(b)) => Some(a == b),
                        _ => None,
                    },
                })
            })
            .unwrap_or_else(Proof::none)
    };
    reqs.iter()
        .map(|r| Row {
            id: r.id.clone(),
            statement: r.statement.clone(),
            twin: (!r.twin.is_empty()).then(|| newest("linux-twin", &r.id, "twin")),
            device: (!r.device.is_empty()).then(|| newest("device", &r.id, "device")),
        })
        .collect()
}

pub fn render(rows: &[Row], twin_only: bool) -> String {
    let cell = |p: &Option<Proof>| p.as_ref().map_or("-".to_string(), Proof::cell);
    let mut out = format!(
        "  {:<8} {:<18} {:<18} {:<8} {}\n",
        "ID", "Linux twin", "iPhone", "Status", "Requirement"
    );
    for r in rows {
        out.push_str(&format!(
            "  {:<8} {:<18} {:<18} {:<8} {}\n",
            r.id,
            cell(&r.twin),
            cell(&r.device),
            match r.status(twin_only) {
                Status::NotHere => "not here",
                st => st.as_str(),
            },
            r.statement
        ));
    }
    out
}

pub fn verify(
    runner: &dyn CommandRunner,
    app: &Path,
    twin_only: bool,
    out: &mut dyn std::io::Write,
) -> Result<(), String> {
    let reqs = evidence::load_requirements(app)?;
    if reqs.is_empty() {
        return Err(format!(
            "{} has no {}: list the app's requirements and the tests that prove them (docs/evidence.md#requirements)",
            app.display(),
            evidence::REQUIREMENTS_FILE
        ));
    }
    let tree_now = evidence::app_tree(runner, app);
    let all = bundles(app);
    let rows = rows(&reqs, &all, tree_now.as_deref());
    let open: Vec<&Row> = rows
        .iter()
        .filter(|r| r.status(twin_only) == Status::Open)
        .collect();
    let not_here: Vec<&str> = rows
        .iter()
        .filter(|r| r.status(twin_only) == Status::NotHere)
        .map(|r| r.id.as_str())
        .collect();
    crate::report::set_data(json!({
        "tree": tree_now,
        "bundles": all.len(),
        "twin_only": twin_only,
        "proven": rows.iter().filter(|r| r.status(twin_only) == Status::Proven).count(),
        "not_here": not_here,
        "requirements": rows.iter().map(|r| json!({
            "id": r.id,
            "statement": r.statement,
            "twin": r.twin.as_ref().map(Proof::json),
            "device": r.device.as_ref().map(Proof::json),
            "status": r.status(twin_only).as_str(),
        })).collect::<Vec<_>>(),
    }));
    let w = |out: &mut dyn std::io::Write, s: &str| {
        out.write_all(s.as_bytes()).map_err(|e| e.to_string())
    };
    w(out, &render(&rows, twin_only))?;
    if tree_now.is_none() {
        w(out, "Note: the app is not in a git checkout, so whether evidence is current can't be told (shown as ?).\n")?;
    }
    w(
        out,
        "stale = recorded for different files than the app has now; run the check again.\n",
    )?;
    if open.is_empty() {
        let line = if not_here.is_empty() {
            format!(
                "[forge] All {} requirements have current evidence{}.\n",
                rows.len(),
                if twin_only {
                    " on Linux (iPhone not required)"
                } else {
                    ""
                }
            )
        } else {
            format!(
                "[forge] Everything Linux can prove has current evidence. Not provable here (needs the iPhone or macOS): {}.\n",
                not_here.join(", ")
            )
        };
        w(out, &line)?;
        return Ok(());
    }
    let msg = format!(
        "{} of {} requirements lack current evidence: {}",
        open.len(),
        rows.len(),
        open.iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    crate::report::record_failure(
        "verify",
        vec![crate::report::JsonHint {
            stage: "test",
            problem: msg.clone(),
            fix: "Run `forge check swift` for twin evidence and `forge ui run <scenario>` for iPhone evidence, then verify again.".into(),
        }],
        "",
    );
    Err(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(id: &str, twin: bool, device: bool) -> Requirement {
        Requirement {
            id: id.into(),
            statement: format!("{id} works"),
            twin: if twin { vec!["T.test".into()] } else { vec![] },
            device: if device {
                vec!["uitests/a.toml".into()]
            } else {
                vec![]
            },
        }
    }

    fn twin_bundle(id: &str, tree: &str, verdicts: &[(&str, &str)]) -> Value {
        json!({
            "schema": "forge.evidence/1", "id": id, "kind": "linux-twin",
            "build": {"commit": "c1", "tree": tree},
            "requirements": verdicts.iter().map(|(r, v)| json!({
                "id": r, "twin": {"verdict": v}, "device": {"verdict": "unverified"}
            })).collect::<Vec<_>>(),
        })
    }

    fn device_bundle(id: &str, tree: &str, reqs: &[&str], verdict: &str) -> Value {
        json!({
            "schema": "forge.evidence/1", "id": id, "kind": "device",
            "build": {"commit": "c1", "tree": tree},
            "requirements": reqs.iter().map(|r| json!({
                "id": r, "device": {"verdict": verdict}
            })).collect::<Vec<_>>(),
        })
    }

    #[test]
    fn newest_bundle_wins_and_stale_trees_do_not_prove() {
        let reqs = [
            req("A", true, false),
            req("B", true, true),
            req("C", false, true),
        ];
        let bundles = [
            twin_bundle("1", "old", &[("A", "passed"), ("B", "passed")]),
            twin_bundle("2", "now", &[("A", "failed"), ("B", "passed")]),
            device_bundle("3", "old", &["B", "C"], "passed"),
            device_bundle("4", "now", &["C"], "passed"),
        ];
        let rows = rows(&reqs, &bundles, Some("now"));
        assert_eq!(rows[0].twin.as_ref().unwrap().verdict, "failed");
        assert_eq!(rows[0].status(false), Status::Open);
        // B: twin current, device only from an older tree.
        assert_eq!(rows[1].device.as_ref().unwrap().current, Some(false));
        assert_eq!(rows[1].status(false), Status::Open);
        assert_eq!(
            rows[1].status(true),
            Status::Proven,
            "twin-only ignores the device column"
        );
        // C: device only, current.
        assert!(rows[2].twin.is_none());
        assert_eq!(rows[2].status(false), Status::Proven);
        assert_eq!(
            rows[2].status(true),
            Status::NotHere,
            "nothing on Linux proves a device-only requirement"
        );
        let text = render(&rows, false);
        assert!(text.contains("passed (stale)"), "{text}");
    }

    #[test]
    fn no_bundle_is_none_and_twin_unverified_is_not_a_device_result() {
        let reqs = [req("A", true, true)];
        let rows = rows(
            &reqs,
            &[twin_bundle("1", "t", &[("A", "passed")])],
            Some("t"),
        );
        assert_eq!(rows[0].device.as_ref().unwrap().verdict, "none");
        assert_eq!(rows[0].status(false), Status::Open);
        assert_eq!(rows[0].status(true), Status::Proven);
        let rows = super::rows(&reqs, &[], None);
        assert_eq!(rows[0].twin.as_ref().unwrap().cell(), "none");
        assert_eq!(rows[0].status(true), Status::Open, "no run yet: go run it");
    }

    #[test]
    fn a_twin_test_that_needs_apple_frameworks_is_not_provable_on_linux() {
        let reqs = [req("A", true, true)];
        let rows = rows(
            &reqs,
            &[twin_bundle("1", "t", &[("A", "not_run")])],
            Some("t"),
        );
        assert_eq!(rows[0].status(true), Status::NotHere);
        assert_eq!(rows[0].status(false), Status::Open);
    }

    #[test]
    fn bundles_are_read_oldest_first_from_runs() {
        let app = tempfile::tempdir().unwrap();
        for (dir, id) in [
            ("2026-11-08T10-00-00-check-swift", "b"),
            ("2026-11-07T10-00-00-check-swift", "a"),
        ] {
            let d = app.path().join(".forge/runs").join(dir);
            fs::create_dir_all(&d).unwrap();
            fs::write(
                d.join("evidence.json"),
                json!({"schema": "forge.evidence/1", "id": id}).to_string(),
            )
            .unwrap();
        }
        let ids: Vec<Value> = bundles(app.path())
            .iter()
            .map(|b| b["id"].clone())
            .collect();
        assert_eq!(ids, vec![json!("a"), json!("b")]);
    }
}
