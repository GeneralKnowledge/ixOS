//! The run record (docs/forge-m6.md, M6.4): one bundle that says what an
//! agent run changed, what it ran, what it didn't, and what a person
//! accepted.
//!
//! `forge verify --run <START>` reads every evidence bundle written since
//! `START` (a run id, `2026-11-08T10-00-00`, UTC), the `forge mcp` run
//! journal of the same period and git, and writes
//! `<app>/.forge/runs/<now>-run/` with `evidence.json` (`kind = "run"`) and
//! `report.md`, which is what a person reads. It runs nothing itself.
//!
//! `forge verify --accept <ID> --note "…"` records a person's acceptance of
//! one requirement for the app's files as they are now, as its own bundle
//! (`kind = "human"`). It asks the person to type the id back, and it is
//! refused unless stdin is a terminal and outside `forge mcp`, so no agent
//! can record it.
//!
//! The rules the record keeps: a check that was expected and didn't run is
//! listed under `not_run` with its reason, never as passed; a walk on a
//! model of the app (`kind = "fixture"`) is listed apart and proves nothing
//! about the device; a twin pass never fills the device column and a
//! device pass never fills the person column.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::evidence::{self, Requirement};
use crate::runner::{Cmd, CommandRunner};
use crate::verify;

/// Everything the record is made from, gathered before it is judged, so the
/// judging is a pure function (and tested without git or a phone).
pub struct Inputs {
    pub app: PathBuf,
    /// The run's start, as a run id.
    pub since: String,
    pub requirements: Vec<Requirement>,
    /// Every bundle of the app, oldest first (the person column reads
    /// acceptances from before the run too).
    pub bundles: Vec<Value>,
    /// The `forge mcp` journal lines of the run (start line included) and
    /// the app's command history (`via: "cli"`), in time order.
    pub journal: Vec<Value>,
    pub journal_path: Option<String>,
    /// Commits made since the start, oldest first.
    pub commits: Vec<Commit>,
    /// `git status --porcelain` at the end, one entry per line.
    pub dirty_at_end: Option<Vec<String>>,
    pub repository: Option<String>,
    pub head: Option<String>,
    pub tree_now: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Commit {
    pub hash: String,
    /// When it was committed, as a run id (UTC).
    pub at: String,
    pub subject: String,
}

/// A run id (`2026-11-08T10-00-00`), or a refusal saying what is expected.
pub fn check_stamp(s: &str) -> Result<(), String> {
    let b = s.as_bytes();
    let digits = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18];
    let shaped = b.len() == 19
        && digits.iter().all(|&i| b[i].is_ascii_digit())
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b'-'
        && b[16] == b'-';
    if shaped {
        Ok(())
    } else {
        Err(format!(
            "--run {s:?}: give the run's start as a run id in UTC, like 2026-11-08T10-00-00 (the name of a .forge/runs folder starts with one)"
        ))
    }
}

/// `2026-11-08T10-00-00` as ISO 8601 for git (`2026-11-08T10:00:00Z`).
fn iso(stamp: &str) -> String {
    format!("{}T{}Z", &stamp[..10], stamp[11..].replace('-', ":"))
}

/// The record itself.
pub fn build(i: &Inputs, id: &str) -> Value {
    let since = |b: &&Value| b["id"].as_str().is_some_and(|x| x >= i.since.as_str());
    let in_run: Vec<&Value> = i.bundles.iter().filter(since).collect();
    let of_kind =
        |k: &str| -> Vec<&Value> { in_run.iter().copied().filter(|b| b["kind"] == k).collect() };
    let calls: Vec<&Value> = i.journal.iter().filter(|l| l["event"] == "call").collect();
    let start_line = i.journal.iter().find(|l| l["event"] == "start");
    let no_device = start_line.is_some_and(|s| s["no_device"] == json!(true));

    // Builds and installs, from the journal.
    let builds: Vec<Value> = calls
        .iter()
        .filter(|c| {
            matches!(
                c["tool"].as_str(),
                Some(
                    "forge_build_ios"
                        | "forge_install_device"
                        // The same steps at the terminal (history.rs).
                        | "build ios"
                        | "run ios"
                        | "install ios"
                )
            )
        })
        .map(|c| {
            json!({"seq": c["seq"], "at": c["at"], "tool": c["tool"], "via": c["via"].as_str().unwrap_or("mcp"), "ok": c["ok"], "error": c["error"]})
        })
        .collect();

    // The checks that ran, by kind. A model walk is never a device check.
    let summary = |b: &&Value| {
        json!({
            "id": b["id"],
            "command": b["command"],
            "ok": b["ok"],
            "scenario": b["scenario"],
            "requirements": b["requirements"].as_array().map(|r| r.iter().map(|r| json!({
                "id": r["id"],
                "twin": r["twin"]["verdict"],
                "device": r["device"]["verdict"],
            })).collect::<Vec<_>>()),
        })
    };
    let twin_runs: Vec<Value> = of_kind("linux-twin").iter().map(summary).collect();
    let device: Vec<&Value> = of_kind("device");
    let ui_runs: Vec<Value> = device
        .iter()
        .filter(|b| b["command"] == "ui run")
        .map(summary)
        .collect();
    let walk = |b: &&Value| {
        let issues = b["explore"]["issues"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let count = |s: &str| issues.iter().filter(|x| x["severity"] == s).count();
        json!({
            "id": b["id"],
            "kind": b["kind"],
            "ok": b["ok"],
            "seed": b["explore"]["seed"],
            "screens": b["explore"]["screens"],
            "errors": count("error"),
            "warnings": count("warning"),
            "looked_different": b["explore"]["visual"]["changed"],
        })
    };
    let walks: Vec<Value> = device
        .iter()
        .filter(|b| b["command"] == "ui explore")
        .map(walk)
        .collect();
    let fixture_walks: Vec<Value> = of_kind("fixture").iter().map(walk).collect();

    // The phone, from the device bundles.
    let phone = device
        .iter()
        .rev()
        .map(|b| &b["where"]["device"])
        .chain(device.iter().rev().map(|b| &b["device"]))
        .find(|d| d.is_object())
        .cloned();

    // Screenshots: their verdicts and new accessibility findings.
    let mut screenshots = Vec::new();
    let mut findings = Vec::new();
    for b in &device {
        for s in b["visual"]["screens"].as_array().into_iter().flatten() {
            screenshots.push(json!({"run": b["id"], "name": s["name"], "verdict": s["verdict"], "diff": s["diff"]}));
            for f in s["new_findings"].as_array().into_iter().flatten() {
                findings.push(json!({"run": b["id"], "screen": s["name"], "finding": f}));
            }
        }
    }

    // Crashes and what the watch collected.
    let mut crashes = Vec::new();
    let mut watch_gaps = Vec::new();
    for b in &device {
        let w = &b["watch"];
        for c in w["crashes"].as_array().into_iter().flatten() {
            crashes.push(json!({
                "run": b["id"],
                "title": c["title"].as_str().or(c["kind"].as_str()),
                "symbolicated": c["symbolicated"],
                "file": c["file"],
            }));
        }
        let unread = w["log_status"] == "unavailable"
            || w["log_status"] == "partial"
            || w["crash_reports"] == "unavailable";
        if unread || w["status"] == "not_watched" {
            watch_gaps.push(json!({
                "run": b["id"],
                "status": w["status"],
                "log_status": w["log_status"],
                "crash_reports": w["crash_reports"],
                "notes": w["notes"],
            }));
        }
    }

    // Failures, and the commits between each and the next success of the
    // same tool: the repairs.
    let failures: Vec<Value> = calls
        .iter()
        .enumerate()
        .filter(|(_, c)| c["ok"] == json!(false))
        .map(|(n, c)| {
            let at = c["at"].as_str().unwrap_or("");
            let next_ok = calls[n + 1..]
                .iter()
                .find(|d| d["tool"] == c["tool"] && d["ok"] == json!(true));
            let until = next_ok.and_then(|d| d["at"].as_str());
            let repairs: Vec<Value> = i
                .commits
                .iter()
                .filter(|m| m.at.as_str() >= at && until.is_none_or(|u| m.at.as_str() <= u))
                .map(|m| json!({"commit": m.hash, "subject": m.subject}))
                .collect();
            json!({
                "seq": c["seq"],
                "at": c["at"],
                "tool": c["tool"],
                "stage": c["stage"],
                "error": c["error"],
                "then_passed": next_ok.map(|d| d["seq"].clone()),
                "repairs": repairs,
            })
        })
        .collect();

    // Requirements: the newest twin and device verdicts from this run, and
    // a person's acceptance from any time, each for the files as they are.
    let run_bundles: Vec<Value> = i
        .bundles
        .iter()
        .filter(|b| since(b) || b["kind"] == "human")
        .cloned()
        .collect();
    let rows = verify::rows(&i.requirements, &run_bundles, i.tree_now.as_deref());
    let mut not_run = Vec::new();
    let mut not_accepted = Vec::new();
    let mut requirements = Vec::new();
    for r in &rows {
        if let Some(t) = &r.twin {
            match t.verdict.as_str() {
                "none" => not_run.push(json!({"requirement": r.id, "check": "twin", "reason": "no Linux twin run judged it since the run started"})),
                "not_run" => not_run.push(json!({"requirement": r.id, "check": "twin", "reason": "its twin test needs Apple's frameworks: not runnable on Linux"})),
                _ if t.current == Some(false) => not_run.push(json!({"requirement": r.id, "check": "twin", "reason": "its last twin run was for other files; not run on the code as it is now"})),
                _ => {}
            }
        }
        if let Some(d) = &r.device {
            let reason = match (d.verdict.as_str(), no_device) {
                ("none", true) => Some("the run had no phone (forge mcp --no-device)"),
                ("none", false) => Some("no run of its iPhone scenario since the run started"),
                _ if d.current == Some(false) => Some(
                    "its last iPhone run was for other files; not run on the code as it is now",
                ),
                _ => None,
            };
            if let Some(reason) = reason {
                not_run.push(json!({"requirement": r.id, "check": "device", "reason": reason}));
            }
        }
        if !r.person.holds_acceptance() {
            not_accepted.push(json!(r.id));
        }
        requirements.push(json!({
            "id": r.id,
            "statement": r.statement,
            "twin": r.twin.as_ref().map(verify::Proof::json),
            "device": r.device.as_ref().map(verify::Proof::json),
            "person": r.person.json(),
            "status": r.status(false).as_str(),
        }));
    }
    let proven = rows
        .iter()
        .filter(|r| r.status(false) == verify::Status::Proven)
        .count();
    let complete = proven == rows.len() && not_run.is_empty() && not_accepted.is_empty();

    json!({
        "schema": evidence::SCHEMA,
        "id": id,
        "kind": "run",
        "command": "verify --run",
        "since": i.since,
        "ok": complete,
        "verdict": if complete { "complete" } else { "incomplete" },
        "app": i.app.display().to_string(),
        "repository": {
            "root": i.repository,
            "head": i.head,
            "commits": i.commits.iter().map(|c| json!({"commit": c.hash, "at": c.at, "subject": c.subject})).collect::<Vec<_>>(),
            "dirty_at_end": i.dirty_at_end,
        },
        "journal": i.journal_path,
        "tool_calls": calls.len(),
        "builds": builds,
        "device": phone,
        "checks": {
            "twin": twin_runs,
            "ui_runs": ui_runs,
            "walks": walks,
            "fixture_walks": fixture_walks,
            "fixture_note": "Walks on a model of the app: how Forge explored, never how the app behaves on the phone. Never counted as device checks.",
        },
        "screenshots": screenshots,
        "new_accessibility_findings": findings,
        "crashes": crashes,
        "watch_gaps": watch_gaps,
        "failures": failures,
        "requirements": requirements,
        "proven": proven,
        "not_run": not_run,
        "not_accepted": not_accepted,
    })
}

impl verify::Proof {
    /// Accepted by a person, for the files as they are now.
    fn holds_acceptance(&self) -> bool {
        self.verdict == "accepted" && self.current == Some(true)
    }
}

/// `report.md`: the record as a person reads it.
pub fn report_md(r: &Value) -> String {
    let mut s = String::new();
    let list = |v: &Value| v.as_array().cloned().unwrap_or_default();
    let text = |v: &Value| v.as_str().map(str::to_owned).unwrap_or_else(|| "-".into());
    s.push_str(&format!(
        "# Run record {}\n\nApp: `{}`. Since {} (UTC). Verdict: **{}**: {} of {} requirements proven, {} check(s) not run, {} not accepted by a person.\n\n",
        text(&r["id"]),
        text(&r["app"]),
        text(&r["since"]),
        text(&r["verdict"]),
        r["proven"],
        list(&r["requirements"]).len(),
        list(&r["not_run"]).len(),
        list(&r["not_accepted"]).len(),
    ));
    s.push_str("## Requirements\n\n| ID | Linux twin | iPhone | Person | Status |\n|---|---|---|---|---|\n");
    let cell = |p: &Value| -> String {
        if p.is_null() {
            return "-".into();
        }
        let v = text(&p["verdict"]);
        match p["current"].as_bool() {
            Some(false) => format!("{v} (stale)"),
            None if p["run"].is_null() => v,
            None => format!("{v} (?)"),
            Some(true) => v,
        }
    };
    for q in list(&r["requirements"]) {
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            text(&q["id"]),
            cell(&q["twin"]),
            cell(&q["device"]),
            cell(&q["person"]),
            text(&q["status"])
        ));
    }
    s.push_str("\n## Not run\n\n");
    let not_run = list(&r["not_run"]);
    if not_run.is_empty() {
        s.push_str("Every expected check ran.\n");
    }
    for n in &not_run {
        s.push_str(&format!(
            "- {} {}: {}\n",
            text(&n["requirement"]),
            text(&n["check"]),
            text(&n["reason"])
        ));
    }
    let not_accepted = list(&r["not_accepted"]);
    if !not_accepted.is_empty() {
        s.push_str(&format!(
            "\nNot accepted by a person (only `forge verify --accept`, typed by a person, records that): {}\n",
            not_accepted.iter().map(text).collect::<Vec<_>>().join(", ")
        ));
    }
    s.push_str("\n## What changed\n\n");
    let commits = list(&r["repository"]["commits"]);
    if commits.is_empty() {
        s.push_str("No commits since the start.\n");
    }
    for c in &commits {
        let short: String = text(&c["commit"]).chars().take(8).collect();
        s.push_str(&format!("- `{short}` {}\n", text(&c["subject"])));
    }
    match r["repository"]["dirty_at_end"].as_array() {
        Some(d) if !d.is_empty() => {
            s.push_str(&format!("\nUncommitted at the end: {} file(s).\n", d.len()))
        }
        Some(_) => s.push_str("\nNothing uncommitted at the end.\n"),
        None => {
            s.push_str("\nWhether anything was left uncommitted is unknown (not a git checkout).\n")
        }
    }
    s.push_str("\n## What ran\n\n");
    s.push_str(&format!(
        "- Commands and tool calls recorded: {} ({}).\n",
        r["tool_calls"],
        r["journal"].as_str().unwrap_or("no journal found")
    ));
    for b in list(&r["builds"]) {
        s.push_str(&format!(
            "- {} ({}) at {}: {}\n",
            text(&b["tool"]),
            if b["via"] == "cli" {
                "terminal"
            } else {
                "forge mcp"
            },
            text(&b["at"]),
            if b["ok"] == json!(true) {
                "ok".to_string()
            } else {
                format!("failed: {}", text(&b["error"]))
            }
        ));
    }
    let checks = &r["checks"];
    s.push_str(&format!(
        "- Linux twin runs: {}. UI test runs on the phone: {}. Exploration walks on the phone: {}.\n",
        list(&checks["twin"]).len(),
        list(&checks["ui_runs"]).len(),
        list(&checks["walks"]).len()
    ));
    let fixtures = list(&checks["fixture_walks"]);
    if !fixtures.is_empty() {
        s.push_str(&format!(
            "- Walks on a model of the app: {}. They show how Forge explored, never how the app behaves on the phone.\n",
            fixtures.len()
        ));
    }
    if let Some(d) = r["device"].as_object() {
        s.push_str(&format!(
            "- Phone: {}.\n",
            serde_json::to_string(d).unwrap_or_default()
        ));
    }
    s.push_str("\n## Failures and repairs\n\n");
    let failures = list(&r["failures"]);
    if failures.is_empty() {
        s.push_str("No tool call failed.\n");
    }
    for f in &failures {
        let repairs = list(&f["repairs"]);
        s.push_str(&format!(
            "- #{} {} ({}): {}. {}\n",
            f["seq"],
            text(&f["tool"]),
            text(&f["stage"]),
            text(&f["error"]),
            match (f["then_passed"].is_null(), repairs.is_empty()) {
                (true, _) => "Never passed afterwards.".to_string(),
                (false, true) =>
                    format!("Passed at #{} with no commit in between.", f["then_passed"]),
                (false, false) => format!(
                    "Passed at #{} after {}.",
                    f["then_passed"],
                    repairs
                        .iter()
                        .map(|c| format!(
                            "`{}` {}",
                            text(&c["commit"]).chars().take(8).collect::<String>(),
                            text(&c["subject"])
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }
        ));
    }
    let crashes = list(&r["crashes"]);
    let gaps = list(&r["watch_gaps"]);
    if !crashes.is_empty() || !gaps.is_empty() {
        s.push_str("\n## Crashes and logs\n\n");
        for c in &crashes {
            s.push_str(&format!(
                "- {}: {} (symbolicated: {})\n",
                text(&c["run"]),
                text(&c["title"]),
                text(&c["symbolicated"])
            ));
        }
        for g in &gaps {
            s.push_str(&format!(
                "- {}: the app's log or crash reports weren't collected ({}, {}).\n",
                text(&g["run"]),
                text(&g["log_status"]),
                text(&g["crash_reports"])
            ));
        }
    }
    let shots = list(&r["screenshots"]);
    let findings = list(&r["new_accessibility_findings"]);
    if !shots.is_empty() || !findings.is_empty() {
        s.push_str("\n## Screenshots and accessibility\n\n");
        for v in ["match", "changed", "new", "recorded"] {
            let n = shots.iter().filter(|x| x["verdict"] == v).count();
            if n > 0 {
                s.push_str(&format!("- {n} screenshot(s) {v}\n"));
            }
        }
        for f in &findings {
            s.push_str(&format!(
                "- New finding on {}: {}\n",
                text(&f["screen"]),
                text(&f["finding"])
            ));
        }
    }
    s
}

fn git(runner: &dyn CommandRunner, app: &Path, args: &[&str]) -> Option<String> {
    evidence::git_line(runner, app, args, None)
}

/// The `forge mcp` journals that cover the run: `*-mcp/journal.jsonl`
/// started at or after `since`, in the app's runs and the repository's.
fn journals(app: &Path, repo: Option<&Path>, since: &str) -> Vec<PathBuf> {
    let mut dirs = vec![app.join(".forge/runs")];
    if let Some(r) = repo {
        dirs.push(r.join(".forge/runs"));
    }
    let mut found: Vec<PathBuf> = dirs
        .iter()
        .flat_map(|d| fs::read_dir(d).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().is_some_and(|n| {
                let n = n.to_string_lossy();
                n.ends_with("-mcp") && n.len() >= 19 && n[..19] >= *since
            })
        })
        .map(|p| p.join("journal.jsonl"))
        .filter(|p| p.is_file())
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Calls in the order they happened, MCP's and the terminal's together
/// (stable, so each journal keeps its own order within a second).
fn sort_calls(lines: &mut [Value]) {
    lines.sort_by(|a, b| {
        let at = |v: &Value| v["at"].as_str().unwrap_or("").to_owned();
        at(a).cmp(&at(b))
    });
}

fn read_journal(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// `forge verify --run`.
pub fn record(
    runner: &dyn CommandRunner,
    app: &Path,
    since: &str,
    journal: Option<&Path>,
    out: &mut dyn std::io::Write,
) -> Result<(), String> {
    check_stamp(since)?;
    let requirements = evidence::load_requirements(app)?;
    let repository = git(runner, app, &["rev-parse", "--show-toplevel"]);
    let commits = git(
        runner,
        app,
        &[
            "log",
            &format!("--since={}", iso(since)),
            "--reverse",
            "--format=%H%x09%ct%x09%s",
        ],
    )
    .map(|text| {
        text.lines()
            .filter_map(|l| {
                let mut p = l.splitn(3, '\t');
                let hash = p.next()?.to_owned();
                let secs: u64 = p.next()?.parse().ok()?;
                Some(Commit {
                    hash,
                    at: crate::report::stamp_at(secs),
                    subject: p.next().unwrap_or("").to_owned(),
                })
            })
            .collect()
    })
    .unwrap_or_default();
    let dirty_at_end = runner
        .capture(
            &Cmd::new("git")
                .arg("-C")
                .arg(app)
                .args(["status", "--porcelain"]),
        )
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(str::to_owned)
                .collect()
        });
    let journal_files = match journal {
        Some(p) if p.is_file() => vec![p.to_path_buf()],
        Some(p) => return Err(format!("--journal {}: no such file", p.display())),
        None => journals(app, repository.as_deref().map(Path::new), since),
    };
    let mut journal_lines: Vec<Value> =
        journal_files.iter().flat_map(|p| read_journal(p)).collect();
    // What a person (or a script) ran at the terminal: the app's history.
    let history = crate::history::since(app, since);
    let history_used = !history.is_empty();
    journal_lines.extend(history);
    if history_used {
        // One numbering for both: #n is the nth call or command of the run.
        sort_calls(&mut journal_lines);
        let mut n = 0;
        for line in journal_lines.iter_mut().filter(|l| l["event"] == "call") {
            n += 1;
            line["seq"] = json!(n);
        }
    }
    let mut journal_files: Vec<String> = journal_files
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    if history_used {
        journal_files.push(app.join(crate::history::FILE).display().to_string());
    }
    let inputs = Inputs {
        app: app.to_path_buf(),
        since: since.to_owned(),
        requirements,
        bundles: verify::bundles(app),
        journal: journal_lines,
        journal_path: journal_files.into_iter().reduce(|a, b| format!("{a}, {b}")),
        commits,
        dirty_at_end,
        repository,
        head: git(runner, app, &["rev-parse", "HEAD"]),
        tree_now: evidence::app_tree(runner, app),
    };
    let id = crate::report::timestamp();
    let record = build(&inputs, &id);
    let dir = app.join(".forge/runs").join(format!("{id}-run"));
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let md = report_md(&record);
    fs::write(dir.join("report.md"), &md).map_err(|e| format!("report.md: {e}"))?;
    let mut doc = record.clone();
    doc["artifacts"] = json!([{"kind": "report", "path": "report.md"}]);
    evidence::hash_artifacts(&mut doc, &dir);
    fs::write(
        dir.join("evidence.json"),
        serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n",
    )
    .map_err(|e| format!("evidence.json: {e}"))?;
    let mut data = record;
    data["out"] = json!(dir.display().to_string());
    data["report"] = json!(dir.join("report.md").display().to_string());
    crate::report::set_data(data);
    out.write_all(md.as_bytes()).map_err(|e| e.to_string())?;
    writeln!(out, "\nRecord: {}", dir.join("evidence.json").display())
        .map_err(|e| e.to_string())?;
    // Writing the record succeeded; whether the run is complete is in it.
    Ok(())
}

/// Why `--accept` can't run here, if it can't: only a person at a terminal
/// records an acceptance.
pub fn person_present(stdin_is_terminal: bool, under_mcp: bool) -> Result<(), String> {
    if under_mcp {
        return Err("forge verify --accept is refused inside forge mcp: only a person records an acceptance, from their own terminal".into());
    }
    if !stdin_is_terminal {
        return Err("forge verify --accept needs a person at a terminal (stdin is not one): an agent or a script can't record an acceptance".into());
    }
    Ok(())
}

/// `forge verify --accept`.
pub fn accept(
    runner: &dyn CommandRunner,
    app: &Path,
    id: &str,
    note: &str,
    confirm: &mut dyn std::io::BufRead,
    out: &mut dyn std::io::Write,
) -> Result<(), String> {
    use std::io::IsTerminal;
    person_present(
        std::io::stdin().is_terminal(),
        std::env::var_os("FORGE_MCP_CHILD").is_some(),
    )?;
    accept_confirmed(runner, app, id, note, confirm, out)
}

fn accept_confirmed(
    runner: &dyn CommandRunner,
    app: &Path,
    id: &str,
    note: &str,
    confirm: &mut dyn std::io::BufRead,
    out: &mut dyn std::io::Write,
) -> Result<(), String> {
    let reqs = evidence::load_requirements(app)?;
    let Some(req) = reqs.iter().find(|r| r.id == id) else {
        return Err(format!(
            "{id} is not in {} (it has: {})",
            app.join(evidence::REQUIREMENTS_FILE).display(),
            reqs.iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    };
    if note.trim().is_empty() {
        return Err("--note is required: say what you looked at on the phone".into());
    }
    let build = evidence::provenance(runner, app);
    let w = |out: &mut dyn std::io::Write, s: String| {
        out.write_all(s.as_bytes()).map_err(|e| e.to_string())
    };
    w(
        out,
        format!(
            "Accept {id} \"{}\" for the app's files as they are now (commit {}{})?\nType {id} to confirm: ",
            req.statement,
            build["commit"].as_str().unwrap_or("unknown"),
            if build["dirty"] == json!(true) { ", with uncommitted changes" } else { "" }
        ),
    )?;
    out.flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    confirm.read_line(&mut line).map_err(|e| e.to_string())?;
    if line.trim() != id {
        return Err("not confirmed: nothing recorded".into());
    }
    let by = git(runner, app, &["config", "user.name"])
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "unknown".into());
    let stamp = crate::report::timestamp();
    let doc = json!({
        "schema": evidence::SCHEMA,
        "id": stamp,
        "kind": "human",
        "command": "verify --accept",
        "ok": true,
        "build": build,
        "requirements": [{
            "id": req.id,
            "statement": req.statement,
            "person": {"verdict": "accepted", "by": by, "note": note},
        }],
    });
    let dir = app.join(".forge/runs").join(format!("{stamp}-accept"));
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    fs::write(
        dir.join("evidence.json"),
        serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n",
    )
    .map_err(|e| format!("evidence.json: {e}"))?;
    crate::report::set_data(json!({"accepted": id, "by": by, "bundle": dir.join("evidence.json")}));
    w(
        out,
        format!(
            "\nRecorded: {id} accepted by {by}. {}\n",
            dir.join("evidence.json").display()
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(id: &str) -> Requirement {
        Requirement {
            id: id.into(),
            statement: format!("{id} works"),
            twin: vec!["T.test".into()],
            device: vec!["uitests/a.toml".into()],
        }
    }

    fn inputs(bundles: Vec<Value>, journal: Vec<Value>, commits: Vec<Commit>) -> Inputs {
        Inputs {
            app: PathBuf::from("examples/good-dog"),
            since: "2026-11-08T10-00-00".into(),
            requirements: vec![req("GD-1"), req("GD-2")],
            bundles,
            journal,
            journal_path: Some("j.jsonl".into()),
            commits,
            dirty_at_end: Some(vec![]),
            repository: Some("/repo".into()),
            head: Some("abc".into()),
            tree_now: Some("t".into()),
        }
    }

    fn twin(id: &str, reqs: &[(&str, &str)]) -> Value {
        json!({"schema": "forge.evidence/1", "id": id, "kind": "linux-twin", "command": "check swift", "ok": true,
            "build": {"tree": "t"},
            "requirements": reqs.iter().map(|(r, v)| json!({"id": r, "twin": {"verdict": v}, "device": {"verdict": "unverified"}})).collect::<Vec<_>>()})
    }

    fn ui_run(id: &str, req: &str, verdict: &str) -> Value {
        json!({"schema": "forge.evidence/1", "id": id, "kind": "device", "command": "ui run", "ok": verdict == "passed",
            "build": {"tree": "t"}, "where": {"device": {"ios_major": 26}},
            "visual": {"screens": [{"name": "today", "verdict": "changed", "new_findings": ["tap target too small: Button"]}]},
            "watch": {"status": "watched", "log_status": "collected", "crash_reports": "found",
                "crashes": [{"title": "Fatal error", "symbolicated": "matched"}]},
            "requirements": [{"id": req, "device": {"verdict": verdict}}]})
    }

    fn call(seq: u64, at: &str, tool: &str, ok: bool) -> Value {
        json!({"event": "call", "seq": seq, "at": at, "tool": tool, "ok": ok, "stage": if ok { Value::Null } else { json!("swift") }, "error": if ok { Value::Null } else { json!("build failed") }})
    }

    #[test]
    fn a_skipped_check_is_not_run_never_passed() {
        // GD-1: twin and device passed in the run. GD-2: twin passed long
        // before the run, device never ran.
        let bundles = vec![
            twin("2026-11-07T09-00-00", &[("GD-2", "passed")]),
            twin("2026-11-08T10-05-00", &[("GD-1", "passed")]),
            ui_run("2026-11-08T10-20-00", "GD-1", "passed"),
        ];
        let r = build(&inputs(bundles, vec![], vec![]), "2026-11-08T11-00-00");
        assert_eq!(r["kind"], "run");
        assert_eq!(r["verdict"], "incomplete");
        assert_eq!(r["ok"], false);
        let not_run: Vec<(String, String)> = r["not_run"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| {
                (
                    n["requirement"].as_str().unwrap().into(),
                    n["check"].as_str().unwrap().into(),
                )
            })
            .collect();
        assert_eq!(
            not_run,
            vec![
                ("GD-2".into(), "twin".into()),
                ("GD-2".into(), "device".into())
            ],
            "{}",
            r["not_run"]
        );
        // GD-1 is proven but nobody accepted it: still incomplete.
        assert_eq!(r["proven"], 1);
        assert_eq!(r["not_accepted"], json!(["GD-1", "GD-2"]));
        assert_eq!(r["requirements"][0]["person"]["verdict"], "none");
        // The crash, the changed screenshot and its new finding are kept.
        assert_eq!(r["crashes"][0]["title"], "Fatal error");
        assert_eq!(r["screenshots"][0]["verdict"], "changed");
        assert_eq!(r["new_accessibility_findings"].as_array().unwrap().len(), 1);
        let md = report_md(&r);
        assert!(
            md.contains("GD-2 device: no run of its iPhone scenario since the run started"),
            "{md}"
        );
        assert!(md.contains("Not accepted by a person"), "{md}");
    }

    #[test]
    fn a_device_pass_never_fills_the_person_column_and_a_fixture_never_fills_the_device_one() {
        let fixture = json!({"schema": "forge.evidence/1", "id": "2026-11-08T10-10-00", "kind": "fixture",
            "command": "ui explore", "ok": true, "build": {"tree": "t"},
            "explore": {"seed": 7, "screens": 6, "issues": [{"severity": "error"}]}, "requirements": []});
        let accepted = json!({"schema": "forge.evidence/1", "id": "2026-11-08T10-30-00", "kind": "human",
            "build": {"tree": "t"}, "requirements": [{"id": "GD-1", "person": {"verdict": "accepted", "by": "Reuben"}}]});
        let bundles = vec![
            twin(
                "2026-11-08T10-05-00",
                &[("GD-1", "passed"), ("GD-2", "passed")],
            ),
            fixture,
            ui_run("2026-11-08T10-20-00", "GD-1", "passed"),
            ui_run("2026-11-08T10-21-00", "GD-2", "passed"),
            accepted,
        ];
        let r = build(&inputs(bundles, vec![], vec![]), "x");
        assert_eq!(r["requirements"][0]["person"]["verdict"], "accepted");
        assert_eq!(r["requirements"][1]["person"]["verdict"], "none");
        assert_eq!(r["not_accepted"], json!(["GD-2"]));
        assert_eq!(r["checks"]["walks"].as_array().unwrap().len(), 0);
        assert_eq!(r["checks"]["fixture_walks"][0]["errors"], 1);
        let md = report_md(&r);
        assert!(
            md.contains("never how the app behaves on the phone"),
            "{md}"
        );
    }

    #[test]
    fn a_failure_and_the_commits_that_fixed_it_are_the_repair() {
        let journal = vec![
            json!({"event": "start", "no_device": true}),
            call(1, "2026-11-08T10-01-00", "forge_build_ios", false),
            call(2, "2026-11-08T10-09-00", "forge_build_ios", true),
            call(3, "2026-11-08T10-10-00", "forge_check", false),
        ];
        let commits = vec![
            Commit {
                hash: "aaaaaaaaaa".into(),
                at: "2026-11-08T10-05-00".into(),
                subject: "Fix the picker's binding".into(),
            },
            Commit {
                hash: "bbbbbbbbbb".into(),
                at: "2026-11-08T10-20-00".into(),
                subject: "Later work".into(),
            },
        ];
        let r = build(&inputs(vec![], journal, commits), "x");
        let f = &r["failures"];
        assert_eq!(f[0]["then_passed"], 2);
        assert_eq!(
            f[0]["repairs"],
            json!([{"commit": "aaaaaaaaaa", "subject": "Fix the picker's binding"}])
        );
        // Never passed again: every later commit is listed, and it says so.
        assert!(f[1]["then_passed"].is_null());
        assert_eq!(r["builds"].as_array().unwrap().len(), 2);
        // No phone in this run: that is the reason given.
        assert!(
            r["not_run"].to_string().contains("--no-device"),
            "{}",
            r["not_run"]
        );
        let md = report_md(&r);
        assert!(
            md.contains("Passed at #2 after `aaaaaaaa` Fix the picker's binding."),
            "{md}"
        );
        assert!(md.contains("Never passed afterwards."), "{md}");
    }

    #[test]
    fn builds_a_person_ran_at_the_terminal_are_in_the_record() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path();
        fs::write(app.join("forge.toml"), "").unwrap();
        fs::create_dir_all(app.join(".forge")).unwrap();
        let line = |at: &str, tool: &str, ok: bool| {
            json!({"schema": crate::history::SCHEMA, "event": "call", "via": "cli", "at": at, "tool": tool, "ok": ok, "stage": if ok {"ok"} else {"swift"}, "error": if ok {Value::Null} else {json!("xtool dev build failed")}})
                .to_string()
        };
        fs::write(
            app.join(crate::history::FILE),
            [
                line("2026-11-08T09-00-00", "build ios", true),
                line("2026-11-08T10-01-00", "build ios", false),
                line("2026-11-08T10-09-00", "run ios", true),
                line("2026-11-08T10-10-00", "inspect", true),
            ]
            .join("\n"),
        )
        .unwrap();
        let history = crate::history::since(app, "2026-11-08T10-00-00");
        assert_eq!(history.len(), 3);
        let mut journal = history;
        for (n, l) in journal.iter_mut().enumerate() {
            l["seq"] = json!(n + 1);
        }
        let r = build(&inputs(vec![], journal, vec![]), "x");
        let builds = r["builds"].as_array().unwrap();
        assert_eq!(builds.len(), 2, "{builds:?}");
        assert_eq!(builds[0]["via"], "cli");
        assert_eq!(r["failures"][0]["tool"], "build ios");
        let md = report_md(&r);
        assert!(
            md.contains(
                "build ios (terminal) at 2026-11-08T10-01-00: failed: xtool dev build failed"
            ),
            "{md}"
        );
    }

    #[test]
    fn only_a_person_at_a_terminal_can_accept() {
        assert!(person_present(false, false)
            .unwrap_err()
            .contains("stdin is not one"));
        assert!(person_present(true, true)
            .unwrap_err()
            .contains("inside forge mcp"));
        assert!(person_present(true, false).is_ok());
    }

    #[test]
    fn an_acceptance_needs_the_id_typed_back_and_names_the_files() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path();
        fs::write(
            app.join(evidence::REQUIREMENTS_FILE),
            "[[requirement]]\nid = \"GD-1\"\nstatement = \"Record an outcome\"\ndevice = [\"uitests/journey.toml\"]\n",
        )
        .unwrap();
        let runner = crate::runner::MockRunner::default();
        runner.respond("git", 0, "Reuben\n");
        let mut out = Vec::new();
        let refused = accept_confirmed(
            &runner,
            app,
            "GD-1",
            "Looked on Cati",
            &mut "no\n".as_bytes(),
            &mut out,
        );
        assert_eq!(refused.unwrap_err(), "not confirmed: nothing recorded");
        assert!(!app.join(".forge/runs").exists());
        assert!(accept_confirmed(
            &runner,
            app,
            "GD-9",
            "x",
            &mut "GD-9\n".as_bytes(),
            &mut out
        )
        .is_err());
        assert!(accept_confirmed(
            &runner,
            app,
            "GD-1",
            " ",
            &mut "GD-1\n".as_bytes(),
            &mut out
        )
        .is_err());
        accept_confirmed(
            &runner,
            app,
            "GD-1",
            "Looked on Cati",
            &mut "GD-1\n".as_bytes(),
            &mut out,
        )
        .unwrap();
        let bundles = verify::bundles(app);
        assert_eq!(bundles.len(), 1);
        assert_eq!(bundles[0]["kind"], "human");
        assert_eq!(
            bundles[0]["requirements"][0]["person"]["verdict"],
            "accepted"
        );
        assert_eq!(
            bundles[0]["requirements"][0]["person"]["note"],
            "Looked on Cati"
        );
        assert_eq!(bundles[0]["requirements"][0]["person"]["by"], "Reuben");
        assert!(String::from_utf8(out)
            .unwrap()
            .contains("Type GD-1 to confirm"));
    }

    #[test]
    fn the_start_must_be_a_run_id() {
        assert!(check_stamp("2026-11-08T10-00-00").is_ok());
        assert!(check_stamp("2026-11-08 10:00").is_err());
        assert!(check_stamp("HEAD").is_err());
        assert_eq!(iso("2026-11-08T10-00-00"), "2026-11-08T10:00:00Z");
    }
}
