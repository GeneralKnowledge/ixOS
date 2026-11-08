//! `forge_diagnose`'s correlation (docs/forge-m6.md, M6.1): puts the last
//! failure next to what explains it. The UI step or explore action that
//! failed and the screen it was on, the app's log lines just before, the
//! symbolicated crash if the app crashed, and the files changed in this
//! session, with whether the crash is in one of them.
//!
//! It infers nothing it can't point at, and says so when it can't:
//!
//! - A crash is "in a changed file" only when its source path resolves to
//!   the same repository-relative path as a changed file. Two files with
//!   the same name in different folders never match.
//! - When the changed files can't be listed (not a git checkout), whether
//!   the crash is in one is `null`, not `false`.
//! - What the run couldn't collect (no log, crash reports not listed, a
//!   report that couldn't be symbolicated) is listed under `collection`, so
//!   a missing crash never reads as "the app didn't crash".

use std::path::{Component, Path, PathBuf};

use serde_json::{json, Value};

/// Most changed files listed; the rest are counted.
const MAX_CHANGED: usize = 200;

/// The files changed in this session, as far as git can tell.
#[derive(Debug, Clone, Default)]
pub struct Changes {
    /// The repository's top folder, when the app is in a git checkout.
    pub repo: Option<PathBuf>,
    /// The app's folder relative to `repo` (`examples/good-dog`).
    pub app: Option<String>,
    /// Repository-relative paths changed since `since`, committed or not,
    /// plus untracked files. `None` when they couldn't be listed.
    pub files: Option<Vec<String>>,
    /// Why `files` is `None`.
    pub error: Option<String>,
    /// The commit the session started at.
    pub since: Option<String>,
    /// Files that already had uncommitted changes when the session started:
    /// their diff may include edits from before it.
    pub dirty_at_start: Vec<String>,
    /// More than [`MAX_CHANGED`] files changed; `files` holds the first.
    pub truncated: usize,
}

impl Changes {
    /// The repository-relative paths changed since `since` (a commit; the
    /// session's start), across the whole repository: a crash in a shared
    /// crate counts as much as one in the app.
    pub fn read(app_dir: &Path, since: Option<&str>, dirty_at_start: &[String]) -> Changes {
        let mut changes = Changes {
            since: since.map(str::to_owned),
            dirty_at_start: dirty_at_start.to_vec(),
            ..Changes::default()
        };
        let top = match git(app_dir, &["rev-parse", "--show-toplevel"]) {
            Ok(lines) => lines.into_iter().next().map(PathBuf::from),
            Err(e) => {
                changes.error = Some(format!("not a git checkout ({e})"));
                return changes;
            }
        };
        changes.repo = top;
        changes.app = git(app_dir, &["rev-parse", "--show-prefix"])
            .ok()
            .map(|l| l.into_iter().next().unwrap_or_default())
            .map(|p| p.trim_end_matches('/').to_string());
        let base = since.unwrap_or("HEAD");
        let diff = git(app_dir, &["diff", "--name-only", base, "--", ":/"]);
        let untracked = git(
            app_dir,
            &[
                "ls-files",
                "--others",
                "--exclude-standard",
                "--full-name",
                "--",
                ":/",
            ],
        );
        match (diff, untracked) {
            (Ok(mut files), Ok(more)) => {
                files.extend(more);
                files.sort();
                files.dedup();
                changes.truncated = files.len().saturating_sub(MAX_CHANGED);
                files.truncate(MAX_CHANGED);
                changes.files = Some(files);
            }
            (Err(e), _) | (_, Err(e)) => {
                changes.error = Some(format!("git couldn't list the changes since {base}: {e}"))
            }
        }
        changes
    }

    /// Where a source path from a crash report is in the repository, and
    /// whether it changed in this session.
    pub fn locate(&self, file: &str) -> Value {
        let (repo_path, resolved_by) = self.resolve(file);
        let location = match (&repo_path, resolved_by) {
            (Some(_), _) => "in_repository",
            (None, "outside") => "outside_repository",
            _ => "unresolved",
        };
        let changed = match (&repo_path, &self.files) {
            (Some(p), Some(files)) => {
                if files.contains(p) {
                    json!(true)
                } else if self.truncated > 0 {
                    // Not among the listed files, but the list is cut.
                    Value::Null
                } else {
                    json!(false)
                }
            }
            (None, Some(_)) if location == "outside_repository" => json!(false),
            _ => Value::Null,
        };
        let note = match (location, &changed) {
            ("outside_repository", _) => {
                "The crash's frame is in code outside this repository (the toolchain or a dependency); no file of this session can contain it.".to_string()
            }
            ("unresolved", _) => format!(
                "Couldn't place {file} in the repository, so whether it changed is unknown (a file with the same name proves nothing)."
            ),
            (_, Value::Null) if self.files.is_none() => format!(
                "Whether it changed is unknown: {}.",
                self.error.as_deref().unwrap_or("the changes couldn't be listed")
            ),
            (_, Value::Null) => format!(
                "Whether it changed is unknown: more than {MAX_CHANGED} files changed and it isn't among those listed."
            ),
            (_, Value::Bool(true)) if self.dirty_at_start.iter().any(|d| Some(d) == repo_path.as_ref()) => {
                "Changed, but it already had uncommitted edits when the session started: the change may predate it.".to_string()
            }
            (_, Value::Bool(true)) => "Changed in this session.".to_string(),
            _ => "Not changed in this session.".to_string(),
        };
        json!({
            "file": file,
            "repo_path": repo_path,
            "location": location,
            "resolved_by": resolved_by,
            "changed": changed,
            "note": note,
        })
    }

    /// The repository-relative path of `file`, and how it was found:
    /// `repository_path` (an absolute path inside this checkout),
    /// `app_relative` (forge-crash's paths under the app folder),
    /// `repository_relative`, `other_checkout` (an absolute path from a
    /// build in another clone, matched on the app folder and everything
    /// below it), `outside` or `unresolved`.
    fn resolve(&self, file: &str) -> (Option<String>, &'static str) {
        let path = normalise(file);
        let exists = |rel: &str| self.repo.as_ref().is_some_and(|r| r.join(rel).exists());
        if path.starts_with('/') {
            if let Some(repo) = &self.repo {
                for root in [repo.clone(), repo.canonicalize().unwrap_or(repo.clone())] {
                    let root = normalise(&root.display().to_string());
                    if let Some(rel) = path.strip_prefix(&format!("{root}/")) {
                        return (Some(rel.to_string()), "repository_path");
                    }
                }
            }
            // Built in another clone of the repository: a tail of the path
            // that starts at one of the repository's own top folders
            // (`examples/…`, `crates/…`), has at least three parts and
            // names a file here. The longest such tail wins.
            if let Some(repo) = &self.repo {
                for (at, _) in path.match_indices('/') {
                    let rel = &path[at + 1..];
                    let top = rel.split('/').next().unwrap_or("");
                    if rel.split('/').count() >= 3
                        && !top.is_empty()
                        && repo.join(top).is_dir()
                        && exists(rel)
                    {
                        return (Some(rel.to_string()), "other_checkout");
                    }
                }
            }
            if OUTSIDE.iter().any(|m| path.contains(m)) {
                return (None, "outside");
            }
            return (None, "unresolved");
        }
        if path.starts_with("../") || path.is_empty() {
            return (None, "unresolved");
        }
        if let Some(app) = self.app.as_deref() {
            let rel = if app.is_empty() {
                path.clone()
            } else {
                format!("{app}/{path}")
            };
            if exists(&rel) {
                return (Some(rel), "app_relative");
            }
        }
        if exists(&path) {
            return (Some(path), "repository_relative");
        }
        (None, "unresolved")
    }
}

/// Paths that are never this repository's own source.
const OUTSIDE: &[&str] = &[
    "/rustc/",
    "/.cargo/registry/",
    "/.cargo/git/",
    "/.rustup/",
    "/Library/Developer/",
    "/Applications/Xcode",
    "/usr/lib/swift/",
    "/swift/stdlib/",
];

/// `a\b/./c/../d.rs` → `a/b/d.rs`: forward slashes, no `.` or `..` parts.
pub fn normalise(path: &str) -> String {
    let unix = path.replace('\\', "/");
    let absolute = unix.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for c in Path::new(&unix).components() {
        match c {
            Component::Normal(p) => parts.push(p.to_str().unwrap_or("")),
            Component::ParentDir => {
                if parts.last().is_some_and(|p| *p != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push("..");
                }
            }
            _ => {}
        }
    }
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}

/// The failed result, read for what explains it.
pub fn correlate(failure: &Value, changes: &Changes) -> Value {
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
                "reproduced": i["reproduced"], "changed_files": i["changed_files"],
            })
        })
        .collect();
    let culprit = crash.map(|c| &c["culprit"]).filter(|c| !c.is_null());
    let culprit_file = culprit.and_then(|c| c["file"].as_str());
    let source = culprit_file.map(|f| changes.locate(f));
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
    let symbolicated = crash.map(symbolication);
    let collection = collection(watch, step, crash);
    let mut next = Vec::new();
    match symbolicated {
        Some("other_build") => next.push("The crash report is from another build of the app: its file and line point at today's code, not the code that crashed. Rebuild, install and reproduce before trusting its frames.".to_string()),
        Some("no_local_build") => next.push("The crash isn't symbolicated: no build of the app on this computer matches it. Build the app here (forge_build), then read the report again with `forge crash <file> --project <app>`.".to_string()),
        Some("unreadable") => next.push("The crash report couldn't be read; open the .ips file under the run's crashes/ folder.".to_string()),
        _ => {}
    }
    if crash.is_some() {
        match &source {
            Some(s) if s["changed"] == json!(true) => next.push(format!(
                "The crash is in {}, changed in this session: start there.",
                s["repo_path"].as_str().unwrap_or("")
            )),
            Some(s) if s["changed"] == json!(false) && s["location"] == "in_repository" => next.push(format!(
                "The crash is in {}, not changed in this session: the change may have exposed it; check what the changed files call there.",
                s["repo_path"].as_str().unwrap_or("")
            )),
            Some(s) => next.push(s["note"].as_str().unwrap_or("").to_string()),
            None => next.push("No frame in the app's own code: read the crashed thread and the log tail.".into()),
        }
    }
    if !issues.is_empty() {
        next.push("Replay an issue with forge_run_ui_test on its repro_file; once it reproduces, copy it into the app's uitests/ as a regression test.".into());
    }
    if step.is_object() && crash.is_none() {
        let checked = collection["crash_reports"] == "none_new";
        next.push(if checked {
            "The step failed without a crash (the phone listed no new report): compare on_screen and the tree with what the step expected; the log tail says what the app did.".into()
        } else {
            "The step failed and no crash report was collected, but the phone's reports weren't checked (see collection): don't rule out a crash; `forge device crashes` lists them.".into()
        });
    }
    let evidence = data["evidence"].as_str().map(|p| {
        let path = Path::new(p);
        let found = if path.is_absolute() {
            path.is_file()
        } else {
            failure["app"]["path"]
                .as_str()
                .map(|a| Path::new(a).join(path).is_file())
                .unwrap_or(false)
                || path.is_file()
        };
        json!({ "path": p, "exists": found })
    });
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
            "symbolicated": symbolicated, "file": c["file"], "error": c.get("error"),
        })),
        "culprit_source": source,
        "collection": collection,
        "log_tail": log_tail,
        "changes": {
            "since": changes.since,
            "files": changes.files,
            "unknown": changes.error,
            "more_not_listed": changes.truncated,
            "dirty_at_start": changes.dirty_at_start,
        },
        // Kept for older readers: set only on a path match.
        "crash_in_changed_file": source.as_ref()
            .filter(|s| s["changed"] == json!(true))
            .map(|s| s["repo_path"].clone()),
        "evidence": evidence,
        "next": next,
    })
}

/// How far a crash entry could be read (as `forge`'s watch reports it).
fn symbolication(crash: &Value) -> &'static str {
    if let Some(s) = crash["symbolicated"].as_str() {
        return match s {
            "matched" => "matched",
            "other_build" => "other_build",
            "no_local_build" => "no_local_build",
            "unreadable" => "unreadable",
            _ => "unknown",
        };
    }
    if crash.get("error").is_some() {
        return "unreadable";
    }
    match crash["build"]["status"].as_str() {
        Some("matched") => "matched",
        Some("other_build") => "other_build",
        Some("not_found") => "no_local_build",
        _ => "unknown",
    }
}

/// What the run collected and what it couldn't: `log` and `crash_reports`
/// as the watch reported them, or `unknown` for a result without a watch.
fn collection(watch: &Value, step: &Value, crash: Option<&Value>) -> Value {
    if watch.is_null() {
        return json!({
            "watched": false,
            "log": if step.get("log_tail").is_some() { "collected" } else { "unknown" },
            "crash_reports": if crash.is_some() { "found" } else { "unknown" },
            "notes": ["This result has no watch section (a build, or an older forge): nothing says whether the app logged or crashed."],
        });
    }
    json!({
        "watched": watch["status"] != "not_watched",
        "log": watch["log_status"].as_str().unwrap_or("unknown"),
        "log_lines": watch["log_lines"],
        "crash_reports": watch["crash_reports"].as_str().unwrap_or(
            if watch["crashes_checked"] == json!(true) { "checked" } else { "unknown" }
        ),
        "notes": watch["notes"],
    })
}

fn git(dir: &Path, args: &[&str]) -> Result<Vec<String>, String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().next().unwrap_or("git failed").to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_owned)
        .filter(|l| !l.is_empty())
        .collect())
}

/// The commit `dir` is at, if it is a git checkout.
pub fn head(dir: &Path) -> Option<String> {
    git(dir, &["rev-parse", "HEAD"]).ok()?.into_iter().next()
}

/// Files with uncommitted changes in `dir`'s checkout right now (at the
/// session's start), repository-relative.
pub fn dirty(dir: &Path) -> Vec<String> {
    Changes::read(dir, None, &[]).files.unwrap_or_default()
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

    /// A repository on disk with Good Dog's layout and a same-named file
    /// in another crate.
    fn repo() -> (tempfile::TempDir, Changes) {
        let tmp = tempfile::tempdir().unwrap();
        for f in [
            "examples/good-dog/rust/src/training.rs",
            "examples/good-dog/rust/src/lib.rs",
            "examples/good-dog/Sources/AppHost/TodayView.swift",
            "crates/forge-runtime/src/training.rs",
        ] {
            let p = tmp.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "").unwrap();
        }
        let changes = Changes {
            repo: Some(tmp.path().to_path_buf()),
            app: Some("examples/good-dog".into()),
            files: Some(vec!["crates/forge-runtime/src/training.rs".into()]),
            since: Some("abc".into()),
            ..Changes::default()
        };
        (tmp, changes)
    }

    #[test]
    fn the_same_file_name_in_another_folder_is_not_a_match() {
        let (_tmp, changes) = repo();
        // forge-crash gives paths under the app folder relative to it.
        let s = changes.locate("rust/src/training.rs");
        assert_eq!(s["repo_path"], "examples/good-dog/rust/src/training.rs");
        assert_eq!(s["resolved_by"], "app_relative");
        assert_eq!(s["changed"], false);
        let failure = json!({"ok": false, "stage": "test",
            "data": {"failed_step": {"step": 4, "crash": crash("rust/src/training.rs")}}});
        let c = correlate(&failure, &changes);
        assert!(c["crash_in_changed_file"].is_null());
        assert!(c["next"]
            .to_string()
            .contains("not changed in this session"));
    }

    #[test]
    fn absolute_paths_in_this_checkout_and_in_another_resolve_to_the_same_file() {
        let (tmp, mut changes) = repo();
        changes.files = Some(vec!["examples/good-dog/rust/src/training.rs".into()]);
        let here = format!(
            "{}/examples/good-dog/rust/src/training.rs",
            tmp.path().display()
        );
        let s = changes.locate(&here);
        assert_eq!(s["resolved_by"], "repository_path");
        assert_eq!(s["changed"], true);
        // Built on the laptop in its own clone.
        let s = changes.locate("/home/reuben/ixOS/examples/good-dog/rust/src/training.rs");
        assert_eq!(s["resolved_by"], "other_checkout");
        assert_eq!(s["repo_path"], "examples/good-dog/rust/src/training.rs");
        assert_eq!(s["changed"], true);
        // Windows separators and dot segments.
        let s = changes.locate("rust\\src\\.\\x\\..\\training.rs");
        assert_eq!(s["repo_path"], "examples/good-dog/rust/src/training.rs");
    }

    #[test]
    fn code_outside_the_repository_is_said_to_be_outside() {
        let (_tmp, changes) = repo();
        let s = changes.locate("/rustc/90b35a6/library/core/src/panicking.rs");
        assert_eq!(s["location"], "outside_repository");
        assert_eq!(s["changed"], false);
        let s = changes.locate("/home/reuben/.cargo/registry/src/x/uniffi-0.28/src/lib.rs");
        assert_eq!(s["location"], "outside_repository");
    }

    #[test]
    fn an_unplaceable_path_or_unknown_changes_are_unknown_not_unchanged() {
        let (_tmp, mut changes) = repo();
        // A relative path that is nowhere in the repository.
        let s = changes.locate("Sources/Elsewhere/training.rs");
        assert_eq!(s["location"], "unresolved");
        assert!(s["changed"].is_null());
        assert!(s["note"]
            .as_str()
            .unwrap()
            .contains("same name proves nothing"));
        // Not a git checkout: nothing can be said about changes.
        changes.files = None;
        changes.error = Some("not a git checkout".into());
        let s = changes.locate("rust/src/training.rs");
        assert!(s["changed"].is_null());
        assert!(s["note"].as_str().unwrap().contains("not a git checkout"));
        let c = correlate(
            &json!({"ok": false, "stage": "test", "data": {"failed_step": {"crash": crash("rust/src/training.rs")}}}),
            &changes,
        );
        assert!(c["crash_in_changed_file"].is_null());
        assert_eq!(c["changes"]["unknown"], "not a git checkout");
    }

    #[test]
    fn normalising_paths() {
        assert_eq!(normalise("a\\b/./c/../d.rs"), "a/b/d.rs");
        assert_eq!(normalise("/x/../y/z.swift"), "/y/z.swift");
        assert_eq!(normalise("./Sources/A.swift"), "Sources/A.swift");
        assert_eq!(normalise("../up.rs"), "../up.rs");
    }

    #[test]
    fn a_crash_in_a_changed_file_is_named_and_the_layer_comes_from_its_file() {
        let (_tmp, mut changes) = repo();
        changes.files = Some(vec!["examples/good-dog/rust/src/training.rs".into()]);
        let failure = json!({
            "ok": false, "stage": "test",
            "data": {"failed_step": {
                "step": 4, "action": "tap exercise.outcome.easy", "error": "element not found",
                "on_screen": "Home Screen", "tree": "failure-step4-tree.json",
                "log_tail": ["+900ms GoodDogXtool[9] <Error>: [exercise] saving"],
                "crash": crash("rust/src/training.rs"),
            }, "evidence": "Good/.forge/runs/x/evidence.json",
            "watch": {"status": "watched", "log_status": "collected", "log_lines": 40,
                      "crash_reports": "found", "notes": []}},
        });
        let c = correlate(&failure, &changes);
        assert_eq!(c["layer"], "rust");
        assert_eq!(
            c["crash_in_changed_file"],
            "examples/good-dog/rust/src/training.rs"
        );
        assert_eq!(c["failed_step"]["step"], 4);
        assert_eq!(c["crash"]["culprit"]["line"], 88);
        assert_eq!(c["crash"]["symbolicated"], "matched");
        assert!(c["crash"].get("thread").is_none());
        assert_eq!(c["collection"]["log"], "collected");
        assert_eq!(c["evidence"]["exists"], false);
        assert!(c["next"][0]
            .as_str()
            .unwrap()
            .contains("changed in this session"));
    }

    #[test]
    fn an_explore_walk_gives_its_errors_and_the_watched_crash() {
        let (_tmp, changes) = repo();
        let failure = json!({
            "ok": false, "stage": "test",
            "data": {
                "issues": [
                    {"issue": "app_closed", "severity": "error", "message": "closed after tap", "screen": "ab", "step": 7, "repro_file": "repro/issue-1-app_closed.toml"},
                    {"issue": "no_identifier", "severity": "warning", "message": "…"},
                ],
                "watch": {"status": "watched", "crashes": [crash("Sources/AppHost/TodayView.swift")], "log_tail": [], "crash_reports": "found", "log_status": "empty"},
            },
        });
        let c = correlate(&failure, &changes);
        assert_eq!(c["layer"], "swift");
        assert_eq!(c["explore_errors"].as_array().unwrap().len(), 1);
        assert_eq!(
            c["explore_errors"][0]["repro_file"],
            "repro/issue-1-app_closed.toml"
        );
        assert!(c["crash_in_changed_file"].is_null());
        assert_eq!(
            c["culprit_source"]["repo_path"],
            "examples/good-dog/Sources/AppHost/TodayView.swift"
        );
        assert_eq!(c["collection"]["log"], "empty");
    }

    #[test]
    fn crashes_from_another_build_or_without_symbols_say_so() {
        let (_tmp, changes) = repo();
        let mut other = crash("rust/src/training.rs");
        other["build"]["status"] = json!("other_build");
        let c = correlate(
            &json!({"ok": false, "stage": "test", "data": {"failed_step": {"crash": other}}}),
            &changes,
        );
        assert_eq!(c["crash"]["symbolicated"], "other_build");
        assert!(c["next"][0].as_str().unwrap().contains("another build"));
        let unreadable =
            json!({"file": "crashes/x.ips", "error": "bad json", "symbolicated": "unreadable"});
        let c = correlate(
            &json!({"ok": false, "stage": "test", "data": {"failed_step": {"crash": unreadable}}}),
            &changes,
        );
        assert_eq!(c["crash"]["symbolicated"], "unreadable");
        assert!(c["next"][0].as_str().unwrap().contains("couldn't be read"));
    }

    #[test]
    fn a_failed_step_without_a_crash_check_doesnt_rule_out_a_crash() {
        let (_tmp, changes) = repo();
        let failure = json!({"ok": false, "stage": "test", "data": {
            "failed_step": {"step": 2, "error": "not found"},
            "watch": {"status": "watched", "log_status": "unavailable", "crash_reports": "unavailable",
                      "notes": ["crash reports not checked: usbmuxd"]},
        }});
        let c = correlate(&failure, &changes);
        assert_eq!(c["collection"]["crash_reports"], "unavailable");
        assert!(c["next"][0]
            .as_str()
            .unwrap()
            .contains("don't rule out a crash"));
        let mut checked = failure.clone();
        checked["data"]["watch"]["crash_reports"] = json!("none_new");
        let c = correlate(&checked, &changes);
        assert!(c["next"][0].as_str().unwrap().contains("without a crash"));
    }

    #[test]
    fn a_build_failure_keeps_its_stage() {
        let c = correlate(
            &json!({"ok": false, "stage": "swift", "data": {}}),
            &Changes::default(),
        );
        assert_eq!(c["layer"], "swift");
        assert!(c["crash"].is_null() && c["failed_step"].is_null());
        assert_eq!(c["collection"]["watched"], false);
    }

    #[test]
    fn changes_cover_the_whole_repository_and_say_when_git_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let c = Changes::read(tmp.path(), None, &[]);
        assert!(c.files.is_none());
        assert!(c.error.unwrap().contains("not a git checkout"));
        // This checkout: an untracked file in another crate is listed with
        // its repository path when asked from the app's folder.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let app = root.join("examples/good-dog");
        let c = Changes::read(&app, None, &[]);
        if c.files.is_some() {
            assert_eq!(c.app.as_deref(), Some("examples/good-dog"));
        }
    }
}
