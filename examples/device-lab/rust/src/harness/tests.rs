//! The harness against a scripted model and the machine's own python3
//! (the same wrapper the phone runs, with real Python behaviour).

use std::sync::Arc;
use std::time::{Duration, Instant};

use forge_services::ai::{AiError, ChatRequest, LanguageModel, ModelInfo, ScriptedModel, TextOut};

use super::calls::{self, check_path, grammar, parse, with_finish};
use super::evidence::{ladder, Criterion, Evidence};
use super::journal::{Entry, Journal, TaskStatus};
use super::python_lab::testing::{has_python, LocalPython};
use super::python_lab::PythonLab;
use super::*;

struct Scripted(ScriptedModel);

impl Brain for Scripted {
    fn describe(&self) -> ModelInfo {
        self.0.describe()
    }

    fn generate(
        &self,
        request: ChatRequest,
        grammar: Option<&str>,
        out: Arc<dyn TextOut>,
    ) -> Result<(), AiError> {
        assert!(grammar.is_some_and(|g| g.starts_with("root ::=")));
        self.0.generate(request, out)
    }
}

fn scripted(replies: &[&str]) -> Arc<Scripted> {
    Arc::new(Scripted(ScriptedModel::new(
        replies.iter().map(|s| s.to_string()),
    )))
}

fn lab_tools() -> Vec<calls::ToolSpec> {
    let dir = tempfile::tempdir().unwrap();
    let lab = PythonLab::new(
        dir.path().into(),
        Arc::new(LocalPython),
        Duration::from_secs(5),
    );
    with_finish(&lab.tools())
}

fn harness(dir: &tempfile::TempDir) -> BonsaiHarness {
    BonsaiHarness::with_limits(
        Arc::new(LocalPython),
        dir.path().to_path_buf(),
        HarnessLimits {
            run_seconds: 5,
            ..HarnessLimits::default()
        },
    )
}

fn wait(harness: &BonsaiHarness) -> HarnessSnapshot {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let snapshot = harness.snapshot();
        if !snapshot.running {
            return snapshot;
        }
        assert!(Instant::now() < deadline, "the task did not end");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn task(id: &str) -> evidence::LadderTask {
    ladder().into_iter().find(|t| t.id == id).unwrap()
}

// MARK: Calls and the grammar

#[test]
fn the_grammar_names_every_tool_in_order_with_a_thought_first() {
    let g = grammar(&lab_tools());
    assert!(g.starts_with("root ::= \"{\\\"thought\\\": \" thought \", \\\"tool\\\": \" ("));
    for name in [
        "list_files",
        "read_file",
        "write_file",
        "run_python",
        "run_tests",
        "finish",
    ] {
        assert!(
            g.contains(&format!("\"\\\"{name}\\\"\"")),
            "{name} missing:\n{g}"
        );
    }
    assert!(g.contains(
        "\"\\\"write_file\\\"\" \", \\\"path\\\": \" path \", \\\"content\\\": \" string"
    ));
    assert!(g.contains("thought ::= \"\\\"\" char{0,200} \"\\\"\""));
    assert!(g.contains("path ::= \"\\\"\" [A-Za-z0-9_] [A-Za-z0-9_./-]{0,63} \"\\\"\""));
}

#[test]
fn a_call_in_the_grammars_shape_parses() {
    let tools = lab_tools();
    let call = parse(
        r#"{"thought": "Make it.", "tool": "write_file", "path": "a.py", "content": "print(\"hi\")\n"}"#,
        &tools,
    )
    .unwrap();
    assert_eq!(call.thought, "Make it.");
    assert_eq!(call.tool, "write_file");
    assert_eq!(call.arg("path"), "a.py");
    assert_eq!(call.arg("content"), "print(\"hi\")\n");
    // Without the grammar a thought may be missing.
    assert!(parse(r#"{"tool": "run_tests"}"#, &tools).is_ok());
}

#[test]
fn bad_calls_are_refused_with_a_reason_the_model_can_use() {
    let tools = lab_tools();
    for (text, expected) in [
        ("write the file", "not one JSON object"),
        (r#"{"tool": "delete_everything"}"#, "no tool called"),
        (r#"{"tool": "read_file"}"#, "needs \"path\""),
        (r#"{"tool": "run_tests", "verbose": "yes"}"#, "no argument"),
        (
            r#"{"tool": "read_file", "path": "../secret.py"}"#,
            "inside the project",
        ),
        (r#"{"tool": "read_file", "path": "/etc/hosts"}"#, "relative"),
    ] {
        let err = parse(text, &tools).unwrap_err();
        assert!(err.contains(expected), "{text}: {err}");
    }
}

#[test]
fn project_paths_stay_inside_the_project() {
    for ok in ["a.py", "pkg/mod.py", "test_x.py", "data-1.csv"] {
        assert!(check_path(ok).is_ok(), "{ok}");
    }
    for bad in [
        "",
        "/a.py",
        "../a.py",
        "a/../../b",
        ".harness/journal.jsonl",
        "a//b",
        "a b.py",
        "a\\b",
    ] {
        assert!(check_path(bad).is_err(), "{bad}");
    }
}

// MARK: Evidence

#[test]
fn tests_pass_only_counts_a_run_after_the_last_change() {
    let read = |_: &str| None;
    let passed = Evidence::Tests {
        ran: 2,
        failures: 0,
        errors: 0,
    };
    let wrote = Evidence::Wrote {
        path: "a.py".into(),
        syntax_ok: true,
    };
    assert!(Criterion::TestsPass
        .check(std::slice::from_ref(&passed), &read)
        .is_ok());
    let stale = Criterion::TestsPass
        .check(&[passed.clone(), wrote.clone()], &read)
        .unwrap_err();
    assert!(stale.contains("not been run since the last change"));
    assert!(Criterion::TestsPass.check(&[wrote, passed], &read).is_ok());
    let none = Evidence::Tests {
        ran: 0,
        failures: 0,
        errors: 0,
    };
    assert!(Criterion::TestsPass
        .check(&[none], &read)
        .unwrap_err()
        .contains("no tests"));
}

#[test]
fn a_changed_test_file_fails_file_unchanged() {
    let criterion = Criterion::FileUnchanged {
        path: "t.py".into(),
        content: "same".into(),
    };
    assert!(criterion.check(&[], &|_| Some("same".into())).is_ok());
    assert!(criterion.check(&[], &|_| Some("other".into())).is_err());
}

// MARK: Python Lab

fn seeded(id: &str) -> (tempfile::TempDir, PythonLab) {
    let dir = tempfile::tempdir().unwrap();
    let lab = PythonLab::new(
        dir.path().join("p"),
        Arc::new(LocalPython),
        Duration::from_secs(5),
    );
    lab.reset(&task(id).seed).unwrap();
    (dir, lab)
}

fn call(text: &str) -> ToolCall {
    parse(text, &lab_tools()).unwrap()
}

#[test]
fn python_lab_runs_the_tests_and_reports_the_counts() {
    if !has_python() {
        return;
    }
    let (_dir, lab) = seeded("t3-one-fix");
    let failing = lab.execute(&call(r#"{"tool": "run_tests"}"#));
    assert!(!failing.ok, "{}", failing.text);
    assert!(
        failing
            .text
            .starts_with("1 tests ran: 1 failures, 0 errors."),
        "{}",
        failing.text
    );
    assert!(failing.text.contains("AssertionError"), "{}", failing.text);
    // Paths are relative to the project.
    assert!(
        !failing.text.contains(&lab.root().display().to_string()),
        "{}",
        failing.text
    );
    let fixed = lab.execute(&call(
        r#"{"tool": "write_file", "path": "average.py", "content": "def average(numbers):\n    return sum(numbers) / len(numbers)\n"}"#,
    ));
    assert!(
        fixed.ok && fixed.text.contains("Syntax OK"),
        "{}",
        fixed.text
    );
    let passing = lab.execute(&call(r#"{"tool": "run_tests"}"#));
    assert!(passing.ok, "{}", passing.text);
    assert_eq!(
        passing.evidence,
        vec![Evidence::Tests {
            ran: 1,
            failures: 0,
            errors: 0
        }]
    );
}

#[test]
fn a_syntax_error_is_the_writes_result() {
    if !has_python() {
        return;
    }
    let (_dir, lab) = seeded("t1-hello");
    let out = lab.execute(&call(
        r#"{"tool": "write_file", "path": "bad.py", "content": "def f(:\n    pass\n"}"#,
    ));
    assert!(!out.ok);
    assert!(
        out.text.contains("can't parse it") && out.text.contains("line 1"),
        "{}",
        out.text
    );
}

#[test]
fn a_program_that_raises_shows_only_the_projects_frames() {
    if !has_python() {
        return;
    }
    let (_dir, lab) = seeded("t1-hello");
    lab.execute(&call(
        r#"{"tool": "write_file", "path": "boom.py", "content": "print('before')\nraise ValueError('nope')\n"}"#,
    ));
    let out = lab.execute(&call(r#"{"tool": "run_python", "path": "boom.py"}"#));
    assert!(!out.ok);
    assert!(out.text.contains("before"), "{}", out.text);
    assert!(
        out.text.contains("File \"boom.py\", line 2"),
        "{}",
        out.text
    );
    assert!(out.text.contains("ValueError: nope"), "{}", out.text);
    assert!(!out.text.contains("runpy"), "{}", out.text);
}

#[test]
fn the_guard_refuses_writes_outside_the_project_and_the_network() {
    if !has_python() {
        return;
    }
    // Not under the system's temporary folder, which the guard allows.
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let lab = PythonLab::new(
        dir.path().join("p"),
        Arc::new(LocalPython),
        Duration::from_secs(5),
    );
    lab.reset(&[]).unwrap();
    lab.execute(&call(
        r#"{"tool": "write_file", "path": "escape.py", "content": "open('../escaped.txt', 'w').write('x')\n"}"#,
    ));
    let out = lab.execute(&call(r#"{"tool": "run_python", "path": "escape.py"}"#));
    assert!(
        !out.ok && out.text.contains("writing outside the project"),
        "{}",
        out.text
    );
    assert!(!dir.path().join("escaped.txt").exists());

    lab.execute(&call(
        r#"{"tool": "write_file", "path": "inside.py", "content": "open('note.txt', 'w').write('x')\nprint(open('note.txt').read())\n"}"#,
    ));
    let inside = lab.execute(&call(r#"{"tool": "run_python", "path": "inside.py"}"#));
    assert!(inside.ok, "{}", inside.text);

    lab.execute(&call(
        r#"{"tool": "write_file", "path": "net.py", "content": "import socket\nsocket.create_connection(('example.com', 80), timeout=2)\n"}"#,
    ));
    let net = lab.execute(&call(r#"{"tool": "run_python", "path": "net.py"}"#));
    assert!(
        !net.ok && net.text.contains("network is off"),
        "{}",
        net.text
    );
}

#[test]
fn input_gets_an_empty_stdin_instead_of_waiting() {
    if !has_python() {
        return;
    }
    let (_dir, lab) = seeded("t1-hello");
    lab.execute(&call(
        r#"{"tool": "write_file", "path": "ask.py", "content": "name = input('Name? ')\n"}"#,
    ));
    let out = lab.execute(&call(r#"{"tool": "run_python", "path": "ask.py"}"#));
    assert!(!out.ok && out.text.contains("EOFError"), "{}", out.text);
}

// MARK: The executor

const T3_FIX: &str = r#"{"thought": "Divide by the count.", "tool": "write_file", "path": "average.py", "content": "def average(numbers):\n    return sum(numbers) / len(numbers)\n"}"#;
const RUN_TESTS: &str = r#"{"thought": "Check.", "tool": "run_tests"}"#;
const FINISH_CALL: &str = r#"{"thought": "Done.", "tool": "finish", "summary": "Fixed the bug."}"#;

#[test]
fn t3_completes_only_once_the_evidence_shows_it() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let h = harness(&dir);
    // Claims done before doing anything, is sent back, then works.
    let brain = scripted(&[
        FINISH_CALL,
        RUN_TESTS,
        T3_FIX,
        FINISH_CALL,
        RUN_TESTS,
        FINISH_CALL,
    ]);
    h.start(brain, task("t3-one-fix")).unwrap();
    let snapshot = wait(&h);
    let record = snapshot.task.unwrap();
    assert_eq!(record.status, TaskStatus::Completed, "{:?}", snapshot.steps);
    assert_eq!(record.attempts, 3);
    assert!(record
        .result
        .unwrap()
        .contains("1 tests ran and all passed"));
    let results: Vec<_> = snapshot
        .steps
        .iter()
        .map(|s| (s.tool.as_str(), s.ok))
        .collect();
    assert_eq!(
        results,
        vec![
            ("finish", Some(false)),
            ("run_tests", Some(false)),
            ("write_file", Some(true)),
            ("finish", Some(false)),
            ("run_tests", Some(true)),
            ("finish", Some(true)),
        ]
    );
    assert!(snapshot.steps[0].result.contains("Not done yet"));
    // The fourth step: the fix came after the last test run.
    assert!(snapshot.steps[3]
        .result
        .contains("not been run since the last change"));
    // A checkpoint of the verified files.
    let checkpoints = dir.path().join("t3-one-fix/.harness/checkpoints");
    let task_dir = std::fs::read_dir(&checkpoints)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(task_dir.join("average.py").exists());
    // The transcript has the evidence.
    let transcript = h.transcript();
    assert!(transcript.contains("Status: Completed"), "{transcript}");
    assert!(
        transcript.contains("Thought: Divide by the count."),
        "{transcript}"
    );
}

#[test]
fn editing_the_test_instead_of_the_bug_is_not_accepted() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let h = harness(&dir);
    let cheat = r#"{"thought": "Change the test.", "tool": "write_file", "path": "test_average.py", "content": "import unittest\n\nclass T(unittest.TestCase):\n    def test_x(self):\n        pass\n"}"#;
    let brain = scripted(&[cheat, RUN_TESTS, FINISH_CALL]);
    let mut limited = task("t3-one-fix");
    limited.criteria = task("t3-one-fix").criteria;
    h.start(brain, limited).unwrap();
    // The scripted model repeats its last answer (finish) until the step limit.
    let snapshot = wait(&h);
    let record = snapshot.task.unwrap();
    assert_eq!(record.status, TaskStatus::Failed);
    assert!(
        snapshot.steps[2]
            .result
            .contains("test_average.py was changed"),
        "{:?}",
        snapshot.steps[2]
    );
}

#[test]
fn three_invalid_answers_in_a_row_end_the_task() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness(&dir);
    h.start(scripted(&["I will now write the file."]), task("t1-hello"))
        .unwrap();
    let snapshot = wait(&h);
    let record = snapshot.task.unwrap();
    assert_eq!(record.status, TaskStatus::Failed);
    assert!(record.result.unwrap().contains("3 answers in a row"));
    assert_eq!(snapshot.steps.len(), 3);
    assert!(snapshot.steps.iter().all(|s| s.tool == "invalid"));
}

#[test]
fn the_same_failing_call_gets_a_nudge_then_ends_as_stuck() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let h = harness(&dir);
    let missing = r#"{"thought": "Run it.", "tool": "run_python", "path": "hello.py"}"#;
    h.start(scripted(&[missing]), task("t1-hello")).unwrap();
    let snapshot = wait(&h);
    let record = snapshot.task.unwrap();
    assert_eq!(record.status, TaskStatus::Failed);
    assert!(record.result.unwrap().starts_with("Stuck"));
    assert_eq!(snapshot.steps.len(), 4);
    assert!(snapshot.steps[2]
        .result
        .contains("Repeating it will not work"));
}

#[test]
fn stop_ends_the_task_as_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness(&dir);
    let slow = ScriptedModel::new([r#"{"thought": "Look.", "tool": "list_files"}"#])
        .pause(Duration::from_millis(50));
    h.start(Arc::new(Scripted(slow)), task("t1-hello")).unwrap();
    std::thread::sleep(Duration::from_millis(30));
    h.stop();
    let snapshot = wait(&h);
    assert_eq!(snapshot.task.unwrap().status, TaskStatus::Stopped);
}

#[test]
fn a_busy_harness_refuses_a_second_task() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness(&dir);
    let slow = ScriptedModel::new([r#"{"thought": "Look.", "tool": "list_files"}"#])
        .pause(Duration::from_millis(50));
    h.start(Arc::new(Scripted(slow)), task("t1-hello")).unwrap();
    assert_eq!(
        h.start(scripted(&[FINISH_CALL]), task("t2-one-test")),
        Err(HarnessError::Busy)
    );
    h.stop();
    wait(&h);
}

#[test]
fn the_journal_replays_and_a_task_the_app_closed_during_is_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("scratch");
    std::fs::create_dir_all(&root).unwrap();
    {
        let mut journal = Journal::open(&root).unwrap();
        for entry in [
            Entry::Task {
                task: "task-1".into(),
                description: "Say hi".into(),
                criteria: vec![Criterion::WorkRuns],
                model: "m".into(),
                at_ms: 1,
            },
            Entry::Status {
                task: "task-1".into(),
                status: TaskStatus::Running,
                reason: None,
                at_ms: 2,
            },
            Entry::StepStarted {
                task: "task-1".into(),
                step: 1,
                at_ms: 3,
            },
        ] {
            journal.append(&entry).unwrap();
        }
    }
    // A line cut off as the app died.
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(root.join(Journal::FILE))
        .unwrap()
        .write_all(b"{\"event\": \"answ")
        .unwrap();

    let h = harness(&dir);
    h.open_project("scratch".into()).unwrap();
    let record = h.snapshot().task.unwrap();
    assert_eq!(record.status, TaskStatus::Stopped);
    assert_eq!(
        record.result.as_deref(),
        Some("The app closed during step 1.")
    );
    // Written back, so it stays stopped.
    let replayed = Journal::replay(&root);
    assert!(matches!(
        replayed.last(),
        Some(Entry::Status {
            status: TaskStatus::Stopped,
            ..
        })
    ));
}

#[test]
fn a_ladder_run_keeps_the_previous_runs_journal() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness(&dir);
    for _ in 0..2 {
        h.start(scripted(&["not a call"]), task("t1-hello"))
            .unwrap();
        wait(&h);
    }
    let runs = std::fs::read_dir(dir.path().join(".runs")).unwrap().count();
    assert_eq!(runs, 1);
    // The fresh project has only its own task.
    let entries = Journal::replay(&dir.path().join("t1-hello"));
    assert_eq!(
        entries
            .iter()
            .filter(|e| matches!(e, Entry::Task { .. }))
            .count(),
        1
    );
}

// MARK: The prompt

#[test]
fn the_history_shrinks_to_fit_the_budget_oldest_first() {
    let system = "system";
    let mut history = Vec::new();
    for step in 1..=8u32 {
        history.push(Entry::Answer {
            task: "t".into(),
            step,
            text: format!(
                r#"{{"thought": "step {step}", "tool": "write_file", "path": "a.py", "content": "{}"}}"#,
                "x = 1\\n".repeat(80)
            ),
            model_ms: 1,
        });
        history.push(Entry::Result {
            task: "t".into(),
            step,
            tool: "write_file".into(),
            ok: true,
            text: format!(
                "Wrote a.py (80 lines). Syntax OK.\n{}",
                "detail ".repeat(100)
            ),
            evidence: vec![],
            tool_ms: 1,
        });
    }
    let full = prompt::messages(
        system,
        "Task",
        &[Criterion::WorkRuns],
        "",
        &history,
        100_000,
    );
    assert_eq!(full.len(), 2 + 16);
    // While everything fits, each turn's prompt continues the previous
    // one, so llama.cpp can reuse its saved state and read only the new
    // step (AiLabLlama.swift).
    let earlier = prompt::messages(
        system,
        "Task",
        &[Criterion::WorkRuns],
        "",
        &history[..14],
        100_000,
    );
    assert_eq!(&full[..earlier.len()], &earlier[..]);
    let tight = prompt::messages(system, "Task", &[Criterion::WorkRuns], "", &history, 1_000);
    let total: usize = tight
        .iter()
        .map(|m| m.content.chars().count() / 3 + 4)
        .sum();
    assert!(total <= 1_000, "{total}");
    // The last step is always there in full.
    assert!(tight.last().unwrap().content.contains("detail detail"));
    assert!(tight[1].content.contains("earlier steps are left out"));
    // In between, older calls and results are shortened before any step goes.
    let middle = prompt::messages(system, "Task", &[Criterion::WorkRuns], "", &history, 1_400);
    assert_eq!(middle.len(), 2 + 16);
    assert!(middle[2].content.contains("…(") && middle[2].content.contains("characters)"));
    assert!(middle[3].content.contains("(rest left out)"));
}

#[test]
fn long_results_keep_their_start_and_end() {
    let text = format!("START{}END", "-".repeat(5000));
    let cut = prompt::cut(&text, 1500);
    assert!(cut.starts_with("START") && cut.ends_with("END"));
    assert!(cut.contains("characters left out"));
    assert!(cut.chars().count() < 1700);
}

#[test]
fn the_instructions_show_every_tool_in_the_grammars_shape() {
    let tools = lab_tools();
    let text = prompt::instructions(&tools);
    for tool in &tools {
        let example = calls::example(tool);
        assert!(text.contains(&example), "{example}");
        // Each example is itself a valid call.
        assert!(parse(&example, &tools).is_ok(), "{example}");
    }
}

#[test]
fn the_scripted_model_completes_every_ladder_task() {
    if !has_python() {
        return;
    }
    for task in ladder() {
        // Outside the temporary folder, which the guard allows writes to.
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let h = harness(&dir);
        h.start_ladder_scripted(task.id.into()).unwrap();
        let snapshot = wait(&h);
        let record = snapshot.task.unwrap();
        assert_eq!(
            record.status,
            TaskStatus::Completed,
            "{}: {:?}",
            task.id,
            snapshot.steps
        );
        assert_eq!(record.model_id, SCRIPTED_MODEL_ID);
        if task.id == "t1-hello" {
            let probe = &snapshot.steps[1].result;
            assert!(!probe.contains("ALLOWED"), "{probe}");
            assert!(
                probe.contains("write outside the project blocked"),
                "{probe}"
            );
            assert!(probe.contains("network blocked"), "{probe}");
            assert!(!dir.path().join("escape.txt").exists());
        }
        if task.id == "t3-one-fix" {
            assert!(snapshot.steps[0].result.contains("Not done yet"));
        }
        if task.id == "t5-fix-from-traceback" {
            assert!(snapshot.steps[0].result.contains("TypeError"));
            let missed = &snapshot.steps[1].result;
            assert!(missed.contains("Did you mean"), "{missed}");
            assert!(
                missed.contains("  7|         lines.append(name + \": \" + count)"),
                "{missed}"
            );
            assert!(snapshot.steps[2].result.starts_with("Edited inventory.py"));
        }
        if task.id == "t2-one-test" {
            // The screen shortens the test file; the transcript keeps it.
            assert!(snapshot.steps[1].call.contains("characters)"));
            assert!(h.transcript().contains("self.assertEqual(add(2, 3), 5)"));
        }
    }
    // Every scripted answer parses against the tools it will meet.
    for task in ladder() {
        for text in super::evidence::script(task.id) {
            parse(&text, &lab_tools()).unwrap_or_else(|e| panic!("{text}: {e:?}"));
        }
    }
}

fn edit(lab: &PythonLab, path: &str, old: &str, new: &str) -> ToolOutcome {
    let text = format!(
        "{{\"thought\": \"Edit.\", \"tool\": \"edit_file\", \"path\": {}, \"old\": {}, \"new\": {}}}",
        serde_json::to_string(path).unwrap(),
        serde_json::to_string(old).unwrap(),
        serde_json::to_string(new).unwrap()
    );
    lab.execute(&call(&text))
}

#[test]
fn edit_file_replaces_one_exact_match_and_explains_a_miss() {
    if !has_python() {
        return;
    }
    let (_dir, lab) = seeded("t3-one-fix");
    // Exact, once.
    let done = edit(&lab, "average.py", "(len(numbers) + 1)", "len(numbers)");
    assert!(done.ok, "{}", done.text);
    assert!(done
        .text
        .starts_with("Edited average.py (3 lines). Syntax OK."));
    assert!(lab
        .read_text("average.py")
        .unwrap()
        .contains("return sum(numbers) / len(numbers)\n"));
    // Trailing spaces in old are forgiven.
    let loose = edit(
        &lab,
        "average.py",
        "    return sum(numbers) / len(numbers)   \n",
        "    return sum(numbers) / max(len(numbers), 1)\n",
    );
    assert!(loose.ok, "{}", loose.text);
    assert!(lab
        .read_text("average.py")
        .unwrap()
        .ends_with("max(len(numbers), 1)\n"));
    // A near miss shows the closest lines.
    let miss = edit(&lab, "average.py", "    return sum(nums) / len(nums)", "x");
    assert!(!miss.ok);
    assert!(miss.text.contains("Did you mean"), "{}", miss.text);
    assert!(
        miss.text.contains("  3|     return sum(numbers)"),
        "{}",
        miss.text
    );
    // More than one match.
    let twice = edit(&lab, "average.py", "numbers", "values");
    assert!(!twice.ok);
    assert!(twice.text.contains("matches"), "{}", twice.text);
    // A missing file, and an edit that breaks the syntax is reported.
    assert!(!edit(&lab, "nope.py", "a", "b").ok);
    let broken = edit(
        &lab,
        "average.py",
        "def average(numbers):",
        "def average(numbers)",
    );
    assert!(!broken.ok);
    assert!(broken.text.contains("can't parse"), "{}", broken.text);
}

#[test]
fn the_project_map_lists_definitions_from_ast() {
    if !has_python() {
        return;
    }
    let (_dir, lab) = seeded("t3-one-fix");
    let map = lab.overview();
    assert!(
        map.contains("average.py (108 bytes): def average(numbers)"),
        "{map}"
    );
    assert!(
        map.contains("test_average.py (216 bytes): class AverageTest (test_three_numbers)"),
        "{map}"
    );
    std::fs::write(lab.root().join("broken.py"), "def f(:\n").unwrap();
    assert!(lab
        .overview()
        .contains("broken.py (8 bytes): Python cannot parse it (line 1)"));
}

fn manual_run(project: &std::path::Path, code: &str) -> super::python_lab::PythonLabOutput {
    use std::process::Command;
    let script = super::python_lab::python_lab_script(project.display().to_string(), code.into());
    // PythonBridge's runner: the script's output, ok when nothing escaped.
    let output = Command::new("python3")
        .arg("-I")
        .arg("-c")
        .arg(&script)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    super::python_lab::python_lab_output(
        project.display().to_string(),
        output.status.success(),
        text,
    )
}

#[test]
fn png_headers_give_the_size_and_artifacts_list_newest_first() {
    use super::artifacts::{kind_of, list, png_size};
    let mut head = vec![
        0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13,
    ];
    head.extend_from_slice(b"IHDR");
    head.extend_from_slice(&640u32.to_be_bytes());
    head.extend_from_slice(&480u32.to_be_bytes());
    assert_eq!(png_size(&head), Some((640, 480)));
    assert_eq!(png_size(b"GIF89a not a png at all......"), None);
    assert_eq!(kind_of("a/B.JPG"), Some("jpeg"));
    assert_eq!(kind_of("notes.txt"), None);

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("plots")).unwrap();
    std::fs::create_dir_all(dir.path().join(".harness")).unwrap();
    std::fs::write(dir.path().join("plots/a.png"), &head).unwrap();
    std::fs::write(dir.path().join(".harness/hidden.png"), &head).unwrap();
    std::fs::write(dir.path().join("notes.txt"), "x").unwrap();
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(dir.path().join("b.svg"), "<svg/>").unwrap();
    let found = list(dir.path());
    let paths: Vec<_> = found.iter().map(|a| a.path.as_str()).collect();
    assert_eq!(paths, vec!["b.svg", "plots/a.png"]);
    assert_eq!(found[1].width, Some(640));
    assert_eq!(found[0].width, None);
}

#[test]
fn python_lab_runs_in_its_project_and_the_samples_make_images() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let project = dir.path().join("scratch");
    std::fs::create_dir_all(&project).unwrap();
    // The last expression is shown; the working folder is the project.
    let run = manual_run(
        &project,
        "import os\nopen('note.txt', 'w').write('hi')\n6 * 7",
    );
    assert!(run.ok, "{}", run.output);
    assert_eq!(run.output.trim(), "42");
    assert!(project.join("note.txt").exists());
    // A failure: not ok, and only the script's own frames, with its line.
    let failed = manual_run(&project, "import json\nx = 1\njson.loads('{')\n");
    assert!(!failed.ok);
    assert!(
        failed.output.contains("File \"<script>\", line 3"),
        "{}",
        failed.output
    );
    assert!(
        failed.output.contains("json.loads('{')"),
        "{}",
        failed.output
    );
    assert!(
        !failed.output.contains("json/decoder.py"),
        "{}",
        failed.output
    );
    assert!(
        failed.output.contains("JSONDecodeError"),
        "{}",
        failed.output
    );
    // Writes stay inside the project.
    let outside = manual_run(&project, "open('../outside.txt', 'w')\n");
    assert!(!outside.ok);
    assert!(!dir.path().join("outside.txt").exists());

    for sample in super::samples::python_lab_graphics_samples() {
        let run = manual_run(&project, &sample.code);
        assert!(run.ok, "{}: {}", sample.id, run.output);
    }
    let found = super::artifacts::list(&project);
    let gradient = found.iter().find(|a| a.path == "gradient.png").unwrap();
    assert_eq!((gradient.width, gradient.height), (Some(256), Some(128)));
    for svg in ["chart.svg", "safety.svg"] {
        let text = std::fs::read_to_string(project.join(svg)).unwrap();
        // Well-formed XML, by Python's own parser.
        let check = manual_run(
            &project,
            &format!("import xml.etree.ElementTree as ET\nET.parse({svg:?}).getroot().tag"),
        );
        assert!(
            check.ok && check.output.contains("svg"),
            "{svg}: {}",
            check.output
        );
        assert!(text.starts_with("<svg"));
    }
}

#[cfg(unix)]
#[test]
fn a_path_through_a_link_is_refused() {
    let (dir, lab) = seeded("t1-hello");
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, lab.root().join("link")).unwrap();
    std::os::unix::fs::symlink(outside.join("gone.txt"), lab.root().join("dangling")).unwrap();
    for path in ["link/escape.txt", "dangling"] {
        let text = format!(
            "{{\"thought\": \"Write.\", \"tool\": \"write_file\", \"path\": {}, \"content\": \"x\"}}",
            serde_json::to_string(path).unwrap()
        );
        let out = lab.execute(&call(&text));
        assert!(!out.ok, "{path}: {}", out.text);
        assert!(out.text.contains("symbolic link"), "{}", out.text);
    }
    assert!(!outside.join("escape.txt").exists());
    assert!(!outside.join("gone.txt").exists());
}

#[test]
fn a_run_that_changes_source_makes_earlier_tests_stale() {
    if !has_python() {
        return;
    }
    let (_dir, lab) = seeded("t3-one-fix");
    edit(&lab, "average.py", "(len(numbers) + 1)", "len(numbers)");
    let tests = lab.execute(&call(r#"{"tool": "run_tests"}"#));
    assert!(tests.ok, "{}", tests.text);
    let script =
        "with open('average.py', 'a') as f:\n    f.write('\\nBROKEN = 1\\n')\nprint('done')\n";
    let text = format!(
        "{{\"thought\": \"Write.\", \"tool\": \"write_file\", \"path\": \"sneak.py\", \"content\": {}}}",
        serde_json::to_string(script).unwrap()
    );
    assert!(lab.execute(&call(&text)).ok);
    let ran = lab.execute(&call(r#"{"tool": "run_python", "path": "sneak.py"}"#));
    assert!(ran.ok, "{}", ran.text);
    assert!(
        ran.text.contains("The run changed average.py"),
        "{}",
        ran.text
    );
    let evidence: Vec<Evidence> = [tests.evidence, ran.evidence].concat();
    let read = |path: &str| lab.read_text(path);
    assert!(Criterion::TestsPass.check(&evidence, &read).is_err());
}

#[test]
fn replay_skips_a_line_cut_inside_a_character() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("scratch");
    std::fs::create_dir_all(&root).unwrap();
    let good = serde_json::to_string(&Entry::Status {
        task: "task-1".into(),
        status: TaskStatus::Stopped,
        reason: None,
        at_ms: 3,
    })
    .unwrap();
    let mut bytes = b"{\"kind\": \"answer\", \"text\": \"caf\xc3".to_vec();
    bytes.push(b'\n');
    bytes.extend_from_slice(good.as_bytes());
    bytes.push(b'\n');
    let file = root.join(Journal::FILE);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, bytes).unwrap();
    let entries = Journal::replay(&root);
    assert_eq!(entries.len(), 1);
    assert!(matches!(
        entries[0],
        Entry::Status {
            status: TaskStatus::Stopped,
            ..
        }
    ));
}
