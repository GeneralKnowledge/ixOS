//! `forge_diagnose`'s correlation (docs/forge-m6.md, M6.1): puts the last
//! failure next to what explains it. The UI step or explore action that
//! failed and the screen it was on, the app's log lines just before, the
//! symbolicated crash if the app crashed, and the files changed in this
//! session, with whether the crash is in one of them. It infers nothing it
//! can't point at: `layer` comes from the crash's own file or the result's
//! stage.

use std::path::Path;

use serde_json::{json, Value};

/// The failed result, read for what explains it. `changed` lists the files
/// changed since the session started (relative to the repository).
pub fn correlate(failure: &Value, changed: &[String]) -> Value {
    let data = &failure["data"];
    let step = &data["failed_step"];
    let watch = &data["watch"];
    let crash = step
        .get("crash")
        .or_else(|| watch["crashes"].as_array().and_then(|c| c.first()))
        .filter(|c| !c.is_null());
    let log_tail = step
        .get("log_tail")
        .or_else(|| watch.get("log_tail"))
        .cloned()
        .unwrap_or(Value::Null);
    // An explore walk: its error-severity issues, each with its replay file.
    let issues: Vec<Value> = data["issues"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|i| i["severity"] == "error")
        .take(5)
        .map(|i| {
            json!({
                "issue": i["issue"], "message": i["message"], "screen": i["screen"],
                "step": i["step"], "repro_file": i["repro_file"],
            })
        })
        .collect();
    let culprit = crash.map(|c| &c["culprit"]).filter(|c| !c.is_null());
    let culprit_file = culprit.and_then(|c| c["file"].as_str());
    let layer = match culprit_file {
        Some(f) if f.ends_with(".rs") => "rust",
        Some(f) if f.ends_with(".swift") => "swift",
        _ => match failure["stage"].as_str() {
            Some(s @ ("rust" | "swift" | "signing" | "device" | "usage")) => s,
            Some("test") if crash.is_some() => "app",
            Some("test") => "ui",
            _ => "unknown",
        },
    };
    let culprit_changed = culprit_file.and_then(|f| {
        let name = Path::new(f).file_name()?.to_str()?;
        changed
            .iter()
            .find(|c| Path::new(c).file_name().and_then(|n| n.to_str()) == Some(name))
            .cloned()
    });
    let mut next = Vec::new();
    if let Some(c) = crash {
        if c["build"]["status"] == "other_build" {
            next.push("The crash report is from another build of the app: rebuild, install and reproduce before reading its frames.".to_string());
        }
        match (culprit_file, &culprit_changed) {
            (Some(_), Some(file)) => next.push(format!(
                "The crash is in {file}, changed in this session: start there."
            )),
            (Some(f), None) => next.push(format!(
                "The crash is in {f}, not changed in this session: the change may have exposed it; check what the changed files call there."
            )),
            (None, _) => next.push("No frame in the app's own code: read the crashed thread and the log tail.".into()),
        }
    }
    if !issues.is_empty() {
        next.push("Replay an issue with forge_run_ui_test on its repro_file; once it reproduces, copy it into the app's uitests/ as a regression test.".into());
    }
    if step.is_object() && crash.is_none() {
        next.push("The step failed without a crash: compare on_screen and the tree with what the step expected; the log tail says what the app did.".into());
    }
    json!({
        "layer": layer,
        "failed_step": step.as_object().map(|s| json!({
            "step": s.get("step"), "action": s.get("action"), "error": s.get("error"),
            "on_screen": s.get("on_screen"), "tree": s.get("tree"),
        })),
        "explore_errors": issues,
        "crash": crash.map(|c| json!({
            "kind": c["kind"], "title": c["title"], "message": c["message"],
            "culprit": c["culprit"], "hint": c["hint"], "build": c["build"]["status"],
            "file": c["file"],
        })),
        "log_tail": log_tail,
        "changed_files": changed,
        "crash_in_changed_file": culprit_changed,
        "evidence": data["evidence"],
        "next": next,
    })
}

/// Files changed in `dir` since `since` (a commit), committed or not,
/// plus untracked ones; relative to the repository root.
pub fn changed_files(dir: &Path, since: Option<&str>) -> Vec<String> {
    let git = |args: &[&str]| -> Vec<String> {
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .map(str::to_owned)
                    .filter(|l| !l.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut files = git(&["diff", "--name-only", since.unwrap_or("HEAD"), "--", "."]);
    files.extend(git(&[
        "ls-files",
        "--others",
        "--exclude-standard",
        "--full-name",
        "--",
        ".",
    ]));
    files.sort();
    files.dedup();
    files.truncate(200);
    files
}

/// The commit `dir` is at, if it is a git checkout.
pub fn head(dir: &Path) -> Option<String> {
    std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "HEAD"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crash(file: &str) -> Value {
        json!({
            "kind": "rust_panic", "title": "Rust panic", "message": "index out of bounds",
            "culprit": {"function": "good_dog::training::record", "file": file, "line": 88},
            "hint": "…", "build": {"status": "matched"}, "file": "crashes/x.ips",
            "thread": {"frames": [1, 2, 3]},
        })
    }

    #[test]
    fn a_crash_in_a_changed_file_is_named_and_the_layer_comes_from_its_file() {
        let failure = json!({
            "ok": false, "stage": "test",
            "data": {"failed_step": {
                "step": 4, "action": "tap exercise.outcome.easy", "error": "element not found",
                "on_screen": "Home Screen", "tree": "failure-step4-tree.json",
                "log_tail": ["+900ms GoodDogXtool[9] <Error>: [exercise] saving"],
                "crash": crash("/src/examples/good-dog/rust/src/training.rs"),
            }, "evidence": "Good/.forge/runs/x/evidence.json"},
        });
        let changed = vec!["examples/good-dog/rust/src/training.rs".to_string()];
        let c = correlate(&failure, &changed);
        assert_eq!(c["layer"], "rust");
        assert_eq!(
            c["crash_in_changed_file"],
            "examples/good-dog/rust/src/training.rs"
        );
        assert_eq!(c["failed_step"]["step"], 4);
        assert_eq!(c["crash"]["culprit"]["line"], 88);
        // The crashed thread stays in the bundle, not in the answer.
        assert!(c["crash"].get("thread").is_none());
        assert_eq!(
            c["log_tail"][0],
            "+900ms GoodDogXtool[9] <Error>: [exercise] saving"
        );
        assert!(c["next"][0]
            .as_str()
            .unwrap()
            .contains("changed in this session"));
    }

    #[test]
    fn an_explore_walk_gives_its_errors_and_the_watched_crash() {
        let failure = json!({
            "ok": false, "stage": "test",
            "data": {
                "issues": [
                    {"issue": "app_closed", "severity": "error", "message": "closed after tap", "screen": "ab", "step": 7, "repro_file": "repro/issue-1-app_closed.toml"},
                    {"issue": "no_identifier", "severity": "warning", "message": "…"},
                ],
                "watch": {"crashes": [crash("Sources/AppHost/TodayView.swift")], "log_tail": []},
            },
        });
        let c = correlate(&failure, &[]);
        assert_eq!(c["layer"], "swift");
        assert_eq!(c["explore_errors"].as_array().unwrap().len(), 1);
        assert_eq!(
            c["explore_errors"][0]["repro_file"],
            "repro/issue-1-app_closed.toml"
        );
        assert!(c["crash_in_changed_file"].is_null());
        assert!(c["next"][0]
            .as_str()
            .unwrap()
            .contains("not changed in this session"));
    }

    #[test]
    fn a_build_failure_keeps_its_stage() {
        let c = correlate(&json!({"ok": false, "stage": "swift", "data": {}}), &[]);
        assert_eq!(c["layer"], "swift");
        assert!(c["crash"].is_null() && c["failed_step"].is_null());
    }
}
