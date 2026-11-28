//! The harness against a scripted model and the machine's own python3
//! (the same wrapper the phone runs, with real Python behaviour).

use std::sync::Arc;
use std::time::{Duration, Instant};

use forge_services::ai::{AiError, ChatRequest, LanguageModel, ModelInfo, ScriptedModel, TextOut};

use super::calls::{self, check_path, grammar, parse, with_finish};
use super::evidence::{ladder, Criterion, Evidence};
use super::journal::{Entry, Journal, TaskStatus};
use super::plan::{self, PlanState};
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
fn an_edit_forgives_quotes_and_indent_when_one_line_fits() {
    if !has_python() {
        return;
    }
    let (dir, lab) = seeded("t5-fix-from-traceback");
    // What Bonsai 9B sent six times on the T5 phone run: single quotes
    // and nine spaces where the file has double quotes and eight.
    let out = lab.execute(&call(
        r#"{"tool": "edit_file", "path": "inventory.py", "old": "         lines.append(name + ': ' + count)", "new": "         lines.append(name + ': ' + str(count))"}"#,
    ));
    assert!(out.ok, "{}", out.text);
    assert!(
        out.text
            .contains("matched line 7 once quotes and indentation were ignored"),
        "{}",
        out.text
    );
    let text = std::fs::read_to_string(dir.path().join("p/inventory.py")).unwrap();
    assert!(
        text.contains("\n        lines.append(name + ': ' + str(count))\n"),
        "{text}"
    );
    assert!(!text.contains("+ count)"), "{text}");
    // A line that isn't there still gets "did you mean", not a guess.
    let out = lab.execute(&call(
        r#"{"tool": "edit_file", "path": "inventory.py", "old": "lines.append(name + ': ' + amount)", "new": "x"}"#,
    ));
    assert!(!out.ok && out.text.contains("Did you mean"), "{}", out.text);
}

#[test]
fn a_forgiving_edit_keeps_the_files_tabs_and_never_cuts_inside_a_character() {
    let dir = tempfile::tempdir().unwrap();
    let lab = PythonLab::new(
        dir.path().into(),
        Arc::new(LocalPython),
        Duration::from_secs(5),
    );
    std::fs::write(
        dir.path().join("f.txt"),
        "def f():\n\tvalue = \"x\"\n\treturn value\n",
    )
    .unwrap();
    let edit = |path: &str, file: &str, old: &str, new: &str| {
        std::fs::write(dir.path().join(path), file).unwrap();
        // Read first: a file changed outside Bonsai must be read again.
        let read = serde_json::json!({"tool": "read_file", "path": path});
        assert!(lab.execute(&call(&read.to_string())).ok);
        let args = serde_json::json!({"tool": "edit_file", "path": path, "old": old, "new": new});
        let out = lab.execute(&call(&args.to_string()));
        (out, std::fs::read_to_string(dir.path().join(path)).unwrap())
    };
    let tabbed = "def f():\n\tvalue = \"x\"\n\treturn value\n";
    // Copied with spaces and single quotes: the file's tab is kept.
    let (out, text) = edit("f.txt", tabbed, "    value = 'x'", "    value = 'y'");
    assert!(out.ok, "{}", out.text);
    assert_eq!(text, "def f():\n\tvalue = 'y'\n\treturn value\n");
    // Several new lines indented with spaces into tab-indented code would
    // mix the two, which Python refuses: no guess.
    let (out, text) = edit(
        "f.txt",
        tabbed,
        "    value = 'x'",
        "    if True:\n        value = 'y'",
    );
    assert!(!out.ok, "{}", out.text);
    assert_eq!(text, tabbed);
    // With tabs, they go in at the right depths, deeper and shallower.
    let (out, text) = edit(
        "f.txt",
        tabbed,
        "\tvalue = 'x'",
        "\tif True:\n\t\tvalue = 'y'\nz = 1",
    );
    assert!(out.ok, "{}", out.text);
    assert_eq!(
        text,
        "def f():\n\tif True:\n\t\tvalue = 'y'\nz = 1\n\treturn value\n"
    );
    // Depths compare in columns: a tab in old is the file's eight spaces.
    let (out, text) = edit(
        "g.txt",
        "def f():\n        value = \"x\"\n        return value\n",
        "\tvalue = 'x'",
        "        value = 'y'",
    );
    assert!(out.ok, "{}", out.text);
    assert_eq!(
        text,
        "def f():\n        value = 'y'\n        return value\n"
    );
    // Only delimiters are forgiven: a quote inside a string must match.
    let (out, _) = edit(
        "h.txt",
        "label = \"Bob's\"\n",
        "label = 'Bob\"s'",
        "label = 'x'",
    );
    assert!(!out.ok, "{}", out.text);
    let (out, text) = edit(
        "h.txt",
        "print(\"a: \" + b)\n",
        "print('a: ' + b)",
        "print('a: ' + str(b))",
    );
    assert!(out.ok, "{}", out.text);
    assert_eq!(text, "print('a: ' + str(b))\n");
    // A wide space where an indent would be: no panic, an answer.
    let (out, text) = edit("g.txt", "x = \"a\"\n", " x = 'a'", "\u{2003}x = 'b'");
    assert!(out.ok, "{}", out.text);
    assert_eq!(text, "\u{2003}x = 'b'\n");
}

#[test]
fn double_quotes_inside_a_string_get_a_hint() {
    if !has_python() {
        return;
    }
    let (_dir, lab) = seeded("t1-hello");
    // What Bonsai 4B wrote five times running on the T-P1 phone run.
    let out = lab.execute(&call(
        r#"{"tool": "write_file", "path": "chart.py", "content": "x = 1\ns = f\"<rect x=\"{x}\"/>\n"}"#,
    ));
    assert!(!out.ok);
    assert!(
        out.text.contains("wrap the string in single quotes"),
        "{}",
        out.text
    );
    // Other syntax errors don't get it.
    let out = lab.execute(&call(
        r#"{"tool": "write_file", "path": "bad.py", "content": "def f(:\n    pass\n"}"#,
    ));
    assert!(!out.text.contains("single quotes"), "{}", out.text);
}

#[test]
fn an_svg_chart_must_draw_the_values() {
    let check = |svg: &str| {
        let svg = svg.to_string();
        Criterion::SvgBars {
            path: "chart.svg".into(),
            values: vec!["3".into(), "7".into(), "5".into()],
        }
        .check(&[], &|_| Some(svg.clone()))
    };
    let bar = |h: u32, label: &str| {
        format!("<rect x='0' y='0' width='40' height='{h}' fill='teal'/><text x='1' y='2'>{label}</text>")
    };
    let svg = |body: String| {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"400\" height=\"300\">{body}</svg>"
        )
    };
    // The phone run's chart: every bar 150 high.
    let flat = svg(bar(150, "3") + &bar(150, "7") + &bar(150, "5"));
    assert!(check(&flat)
        .unwrap_err()
        .contains("don't follow the values"));
    // Sized by value, after a full-size background.
    let good = svg(
        "<rect width=\"400\" height=\"300\" fill=\"white\"/>".to_string()
            + &bar(60, "3")
            + &bar(140, "7")
            + &bar(100, "5"),
    );
    assert!(check(&good).is_ok(), "{:?}", check(&good));
    // Bars across: the widths carry the values.
    let across = svg(
        "<rect y='0' height='20' width='30'/><rect y='30' height='20' width='70'/>\
         <rect y='60' height='20' width='50'/><text>3</text><text>7</text><text>Value: 5</text>"
            .into(),
    );
    assert!(check(&across).is_ok(), "{:?}", check(&across));
    // Bars of no width don't show.
    let thin = svg(
        "<rect width='0' height='60'/><rect width='0' height='140'/><rect width='0' height='100'/>\
         <text>3</text><text>7</text><text>5</text>"
            .into(),
    );
    assert!(check(&thin).is_err());
    // Bars only inside <defs> aren't drawn.
    let hidden = svg(format!(
        "<defs>{}</defs><rect width='400' height='300'/>",
        bar(60, "3") + &bar(140, "7") + &bar(100, "5")
    ));
    assert!(check(&hidden).is_err());
    // Bars after a closing tag with a space in it still count.
    let spaced = svg(format!(
        "<defs><linearGradient id='g'/></defs >{}",
        bar(60, "3") + &bar(140, "7") + &bar(100, "5")
    ));
    assert!(check(&spaced).is_ok(), "{:?}", check(&spaced));
    // Lengths in percent.
    let percent = svg(
        "<rect width='10%' height='60'/><rect width='10%' height='140'/><rect width='10%' height='100'/>\
         <text>3</text><text>7</text><text>5</text>"
            .into(),
    );
    assert!(check(&percent).is_ok(), "{:?}", check(&percent));
    // Lengths in em: the e isn't an exponent.
    let em = svg(
        "<rect width='2em' height='3em'/><rect width='2em' height='7em'/><rect width='2em' height='5em'/>\
         <text>3</text><text>7</text><text>5</text>"
            .into(),
    );
    assert!(check(&em).is_ok(), "{:?}", check(&em));
    // A label missing.
    let unlabelled = svg(bar(60, "3") + &bar(140, "7") + &bar(100, ""));
    assert!(check(&unlabelled).unwrap_err().contains("5"));
    // Missing file.
    let gone = Criterion::SvgBars {
        path: "chart.svg".into(),
        values: vec!["3".into()],
    }
    .check(&[], &|_| None);
    assert!(gone.is_err());
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
    let checkpoints = dir.path().join(".ladder/t3-one-fix/.harness/checkpoints");
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
                plan: None,
                plan_task: None,
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
    let entries = Journal::replay(&dir.path().join(".ladder/t1-hello"));
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
        "",
        &history[..14],
        100_000,
    );
    assert_eq!(&full[..earlier.len()], &earlier[..]);
    let tight = prompt::messages(
        system,
        "Task",
        &[Criterion::WorkRuns],
        "",
        "",
        &history,
        1_000,
    );
    let total: usize = tight
        .iter()
        .map(|m| m.content.chars().count() / 3 + 4)
        .sum();
    assert!(total <= 1_000, "{total}");
    // The last step is always there in full.
    assert!(tight.last().unwrap().content.contains("detail detail"));
    assert!(tight[1].content.contains("earlier steps are left out"));
    // In between, older calls and results are shortened before any step goes.
    let middle = prompt::messages(
        system,
        "Task",
        &[Criterion::WorkRuns],
        "",
        "",
        &history,
        1_400,
    );
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
        h.set_approve_plans(false);
        h.start_ladder_scripted(task.id.into()).unwrap();
        let snapshot = wait(&h);
        if task.ends_blocked {
            let plan = snapshot.plan.clone().unwrap();
            assert_eq!(plan.status, PlanState::Blocked, "{}: {plan:?}", task.id);
            assert!(plan.question.is_some());
            h.answer_plan_scripted("Yes, print a sample.".into())
                .unwrap();
        }
        let snapshot = if task.ends_blocked {
            wait(&h)
        } else {
            snapshot
        };
        if task.planned {
            let plan = snapshot.plan.clone().unwrap();
            assert_eq!(plan.status, PlanState::Completed, "{}: {plan:?}", task.id);
            assert_eq!(plan.passed as usize, plan.tasks.len());
            assert!(plan.reason.unwrap().starts_with("All "));
        } else {
            assert!(snapshot.plan.is_none());
        }
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
        if task.id == "tp1-svg-chart" {
            // The finished task can be stepped back through, whole files and all.
            let first = h.replay(snapshot.steps[0].index).unwrap();
            assert_eq!(first.tool.as_deref(), Some("write_file"));
            assert_eq!(first.path.as_deref(), Some("chart.py"));
            assert!(
                first.code.starts_with("values = [3, 7, 5]\n"),
                "{}",
                first.code
            );
            assert!(!first.writing && first.thought_done);
            assert!(h.replay(99).is_none());
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
    // Every scripted answer parses against the tools (or the planner) it
    // will meet.
    for task in ladder() {
        for text in super::evidence::script(task.id) {
            let fits = parse(&text, &lab_tools()).is_ok()
                || (task.planned
                    && (plan::parse(&text, &[]).is_ok()
                        || plan::parse_recovery(&text, "x").is_ok()));
            assert!(fits, "{}: {text}", task.id);
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

fn file_run(project: &std::path::Path, path: &str) -> super::python_lab::PythonLabOutput {
    use std::process::Command;
    let script =
        super::python_lab::python_lab_file_script(project.display().to_string(), path.into());
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
fn a_project_file_runs_as_main_and_imports_its_neighbours() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::write(root.join("helper.py"), "def two():\n    return 2\n").unwrap();
    std::fs::write(
        root.join("main.py"),
        "import helper\nprint(__name__, helper.two())\nhelper.two() + 1\n",
    )
    .unwrap();
    let out = file_run(root, "main.py");
    assert!(out.ok, "{}", out.output);
    assert_eq!(out.output.trim(), "__main__ 2\n3");

    // A file in a folder imports from its own folder and from the project.
    std::fs::write(root.join("lib/util.py"), "X = 5\n").unwrap();
    std::fs::write(
        root.join("lib/my tool.py"),
        "import util, helper\nprint(util.X + helper.two())\n",
    )
    .unwrap();
    let out = file_run(root, "lib/my tool.py");
    assert!(out.ok, "{}", out.output);
    assert_eq!(out.output.trim(), "7");

    // `import __main__` is the file being run, not the host.
    std::fs::write(
        root.join("peek.py"),
        "import __main__\nprint(__main__.NAME)\n",
    )
    .unwrap();
    std::fs::write(root.join("entry.py"), "NAME = 'entry'\nimport peek\n").unwrap();
    let out = file_run(root, "entry.py");
    assert!(out.ok, "{}", out.output);
    assert_eq!(out.output.trim(), "entry");
}

#[test]
fn a_project_files_traceback_names_the_file_and_line() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("helper.py"), "def boom():\n    return 1 / 0\n").unwrap();
    std::fs::write(root.join("main.py"), "import helper\n\nhelper.boom()\n").unwrap();
    let out = file_run(root, "main.py");
    assert!(!out.ok);
    assert!(
        out.output.contains("File \"main.py\", line 3"),
        "{}",
        out.output
    );
    assert!(
        out.output.contains("File \"helper.py\", line 2"),
        "{}",
        out.output
    );
    assert!(out.output.contains("ZeroDivisionError"), "{}", out.output);
    // An edit is picked up on the next run (the module is forgotten).
    std::fs::write(root.join("helper.py"), "def boom():\n    return 'fine'\n").unwrap();
    std::fs::write(
        root.join("main.py"),
        "import helper\nprint(helper.boom())\n",
    )
    .unwrap();
    let out = file_run(root, "main.py");
    assert!(out.ok, "{}", out.output);
    assert_eq!(out.output.trim(), "fine");
}

/// Two projects run one after the other in one interpreter, as on the
/// phone: the second's `import helper` gets its own helper, not the first's.
#[test]
fn a_project_never_imports_another_projects_module() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    for (name, word) in [("a", "from a"), ("b", "from b")] {
        let root = dir.path().join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("helper.py"), format!("WORD = {word:?}\n")).unwrap();
        std::fs::write(root.join("main.py"), "import helper\nprint(helper.WORD)\n").unwrap();
    }
    let script = |name: &str| {
        super::python_lab::python_lab_file_script(
            dir.path().join(name).display().to_string(),
            "main.py".into(),
        )
    };
    let both = format!("{}\n{}", script("a"), script("b"));
    let output = std::process::Command::new("python3")
        .arg("-I")
        .arg("-c")
        .arg(&both)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(text.trim(), "from a\nfrom b", "{text}");
}

/// Runs `path` as Python Lab's console does (PL1d), with `stdin` as what
/// the person types. The host stands in for PythonBridge's streaming
/// runner: it prints an escaping exception's traceback unless the wrapper
/// already showed it. Returns (ok, stdout, stderr).
fn console_run(project: &std::path::Path, path: &str, stdin: &str) -> (bool, String, String) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    const HOST: &str = "import sys, traceback\ntry:\n    exec(compile(sys.argv[1], '<console>', 'exec'), {'__name__': '__devicelab__'})\nexcept BaseException as e:\n    if not getattr(e, '_devicelab_shown', False):\n        traceback.print_exc()\n    sys.exit(1)\n";
    let script =
        super::python_lab::python_lab_console_script(project.display().to_string(), path.into());
    let mut child = Command::new("python3")
        .arg("-I")
        .arg("-c")
        .arg(HOST)
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn the_console_reads_input_and_keeps_stderr_apart() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("main.py"),
        "import sys\nname = input('Name? ')\nprint('Hello', name)\nprint('careful', file=sys.stderr)\n",
    )
    .unwrap();
    let (ok, out, err) = console_run(root, "main.py", "Ada\n");
    assert!(ok, "{out}{err}");
    assert_eq!(out, "Name? Hello Ada\n");
    assert_eq!(err, "careful\n");
}

#[test]
fn a_console_traceback_goes_to_stderr_once_with_short_paths() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("helper.py"), "def half(n):\n    return n / 0\n").unwrap();
    std::fs::write(
        root.join("main.py"),
        "import helper\nprint('before')\nhelper.half(4)\n",
    )
    .unwrap();
    let (ok, out, err) = console_run(root, "main.py", "");
    assert!(!ok);
    assert_eq!(out, "before\n");
    assert!(
        err.starts_with("Traceback (most recent call last):\n"),
        "{err}"
    );
    assert!(err.contains("File \"main.py\", line 3"), "{err}");
    assert!(err.contains("File \"helper.py\", line 2"), "{err}");
    assert!(
        err.trim_end()
            .ends_with("ZeroDivisionError: division by zero"),
        "{err}"
    );
    assert_eq!(err.matches("Traceback").count(), 1, "{err}");
    assert!(!err.contains(&root.display().to_string()), "{err}");
    assert!(
        !out.contains(super::python_lab::FAILED_MARK)
            && !err.contains(super::python_lab::FAILED_MARK)
    );
}

#[test]
fn the_live_view_reads_a_half_written_answer() {
    let full = r#"{"thought": "Write the \"hello\" file.", "tool": "write_file", "path": "hello.py", "content": "print(\"Hello from Bonsai\")\nprint('é \u00e9')\n"}"#;
    // Partway through the thought.
    let live = super::live::read(&full[..full.find("hello").unwrap() + 1]);
    assert_eq!(live.thought, "Write the \"h");
    assert!(!live.thought_done);
    assert_eq!(live.tool, None);
    // Partway through the tool name: not shown until complete.
    let cut = full.find("write_f").unwrap() + 5;
    let live = super::live::read(&full[..cut]);
    assert!(live.thought_done);
    assert_eq!(live.thought, "Write the \"hello\" file.");
    assert_eq!(live.tool, None);
    // Partway through the file: path known, code growing.
    let cut = full.find("Bonsai").unwrap();
    let live = super::live::read(&full[..cut]);
    assert_eq!(live.tool.as_deref(), Some("write_file"));
    assert_eq!(live.path.as_deref(), Some("hello.py"));
    assert_eq!(live.code, "print(\"Hello from ");
    assert!(live.writing);
    // A cut inside an escape or a \u sequence doesn't break anything.
    let cut = full.find("\\u00e9").unwrap() + 3;
    let live = super::live::read(&full[..cut]);
    assert!(live.code.ends_with("print('é "), "{:?}", live.code);
    // The whole answer.
    let live = super::live::read(full);
    assert_eq!(live.code, "print(\"Hello from Bonsai\")\nprint('é é')\n");
    assert!(!live.writing);
    // A key name inside the content is not taken for the key.
    let tricky = r#"{"thought": "t", "tool": "write_file", "path": "a.py", "content": "x = '\"path\": \"no'"#;
    let live = super::live::read(tricky);
    assert_eq!(live.path.as_deref(), Some("a.py"));
    assert_eq!(live.code, "x = '\"path\": \"no'");
    // edit_file shows the new lines, with the old ones apart.
    let edit = r#"{"thought": "Fix.", "tool": "edit_file", "path": "average.py", "old": "len(numbers) + 1", "new": "len(num"#;
    let live = super::live::read(edit);
    assert_eq!(live.replacing, "len(numbers) + 1");
    assert_eq!(live.code, "len(num");
    assert!(live.writing);
    // A surrogate pair becomes one character; half a pair waits.
    let emoji = r#"{"thought": "t", "tool": "write_file", "path": "a.py", "content": "print('\uD83D\uDE00')"#;
    assert_eq!(super::live::read(emoji).code, "print('😀')");
    let half = &emoji[..emoji.find("\\uDE00").unwrap() + 2];
    assert_eq!(super::live::read(half).code, "print('");
    // A lone high surrogate right before the closing quote is finished.
    let lone = r#"{"thought": "t", "tool": "write_file", "path": "a.py", "content": "x\uD83D""#;
    let live = super::live::read(lone);
    assert_eq!(live.code, "x\u{FFFD}");
    assert!(!live.writing);
}

// MARK: Plans (H4)

#[test]
fn the_plan_grammar_has_tasks_ids_and_the_checks_to_choose_from() {
    let g = plan::grammar();
    assert!(g.starts_with("root ::= \"{\\\"thought\\\": \" thought \", \\\"tasks\\\": [\" task (\", \" task){0,5} \"]}\""), "{g}");
    for check in ["tests_pass", "work_runs", "run_prints", "makes_image"] {
        assert!(
            g.contains(&format!("\\\"check\\\": \\\"{check}\\\"")),
            "{check}:\n{g}"
        );
    }
    assert!(g.contains("id ::= \"\\\"\" [a-z] [a-z0-9-]{0,15} \"\\\"\""));
    assert!(g.contains("char ::="));
    // The instructions' example is a plan the parser takes.
    let (thought, tasks) = plan::parse(&plan::example(), &[]).unwrap();
    assert!(!thought.is_empty());
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[1].after, vec!["area".to_string()]);
    assert_eq!(
        tasks[1].criteria,
        vec![Criterion::RunPrints {
            path: "main.py".into(),
            text: "15".into()
        }]
    );
    assert!(plan::instructions().contains(&plan::example()));
}

#[test]
fn a_plan_that_cant_run_in_order_is_refused_with_a_reason() {
    let task = |id: &str, after: &str, check: &str| {
        format!(r#"{{"id": "{id}", "do": "Do {id}.", "after": [{after}], "checks": [{check}]}}"#)
    };
    let tests = r#"{"check": "tests_pass"}"#;
    let plan_of =
        |tasks: Vec<String>| format!(r#"{{"thought": "t", "tasks": [{}]}}"#, tasks.join(", "));
    for (text, expected) in [
        (plan_of(vec![]), "no tasks"),
        (
            plan_of(vec![task("a", "", tests), task("a", "", tests)]),
            "Two tasks are called",
        ),
        (
            plan_of(vec![task("a", r#""b""#, tests), task("b", "", tests)]),
            "not an earlier task",
        ),
        (plan_of(vec![task("a", r#""a""#, tests)]), "after itself"),
        (plan_of(vec![task("a", "", "")]), "at least one check"),
        (
            plan_of(vec![task("a", "", r#"{"check": "looks_nice"}"#)]),
            "no check called",
        ),
        (
            plan_of(vec![task(
                "a",
                "",
                r#"{"check": "run_prints", "path": "../x.py", "text": "1"}"#,
            )]),
            "inside the project",
        ),
        (
            plan_of(vec![task(
                "a",
                "",
                r#"{"check": "run_prints", "path": "x.py", "text": " "}"#,
            )]),
            "needs the text",
        ),
        (plan_of(vec![task("Big", "", tests)]), "not a task id"),
        (
            plan_of((0..7).map(|i| task(&format!("t{i}"), "", tests)).collect()),
            "at most 6",
        ),
        ("{\"thought\": \"t\"}".to_string(), "no \"tasks\""),
    ] {
        let err = plan::parse(&text, &[]).unwrap_err();
        assert!(err.contains(expected), "{text}: {err}");
    }
    // A replan shares the six slots with the tasks already done.
    let six = plan_of((0..6).map(|i| task(&format!("t{i}"), "", tests)).collect());
    assert!(plan::parse(&six, &[]).is_ok());
    let err = plan::parse(&six, &["done".to_string()]).unwrap_err();
    assert!(err.contains("at most 5"), "{err}");
}

#[test]
fn a_plan_waits_for_approval_then_runs_or_is_discarded() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let h = harness(&dir);
    assert!(h.snapshot().approve_plans);
    h.start_ladder_scripted("t6-two-functions".into()).unwrap();
    let snapshot = wait(&h);
    let plan = snapshot.plan.unwrap();
    assert_eq!(plan.status, PlanState::AwaitingApproval);
    assert!(plan.can_continue);
    assert_eq!(plan.tasks.len(), 2);
    assert_eq!(plan.tasks[1].after, vec!["shout".to_string()]);
    assert!(plan.tasks.iter().all(|t| t.status == TaskStatus::Ready));
    // Nothing ran: no task, no files.
    assert!(snapshot.task.is_none());
    assert!(h.files().is_empty());

    h.continue_plan_scripted().unwrap();
    let snapshot = wait(&h);
    let plan = snapshot.plan.unwrap();
    assert_eq!(plan.status, PlanState::Completed, "{plan:?}");
    assert_eq!(plan.passed, 2);
    assert!(!plan.can_continue);
    assert!(matches!(
        h.continue_plan_scripted(),
        Err(HarnessError::NoPlan)
    ));
    let transcript = h.transcript();
    assert!(transcript.contains("## Plan"), "{transcript}");
    assert!(
        transcript.contains("- whisper (after shout)"),
        "{transcript}"
    );
    assert!(
        transcript.contains("return text.lower() + \\\"...\\\""),
        "{transcript}"
    );

    // A plan the person doesn't approve never runs.
    h.start_ladder_scripted("t6-two-functions".into()).unwrap();
    wait(&h);
    h.discard_plan().unwrap();
    let plan = h.snapshot().plan.unwrap();
    assert_eq!(plan.status, PlanState::Discarded);
    assert!(matches!(
        h.continue_plan_scripted(),
        Err(HarnessError::NoPlan)
    ));
    assert!(matches!(h.discard_plan(), Err(HarnessError::NoPlan)));
    assert!(h.files().is_empty());
}

/// Answers like the Scripted model, then fails: the phone losing the
/// model halfway through a plan.
struct FailAfter(ScriptedModel, std::sync::atomic::AtomicUsize);

impl Brain for FailAfter {
    fn describe(&self) -> ModelInfo {
        self.0.describe()
    }

    fn generate(
        &self,
        request: ChatRequest,
        _grammar: Option<&str>,
        out: Arc<dyn TextOut>,
    ) -> Result<(), AiError> {
        use std::sync::atomic::Ordering;
        if self.1.fetch_sub(1, Ordering::SeqCst) == 0 {
            return Err(AiError::Unavailable {
                reason: "the model was unloaded".into(),
            });
        }
        self.0.generate(request, out)
    }
}

#[test]
fn a_failed_plan_resumes_from_its_last_verified_task() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let h = harness(&dir);
    h.set_approve_plans(false);
    let script = evidence::script("t7-plan-with-dependency");
    // The plan, task letter's three answers, then task tests writes its
    // file and the model goes.
    let brain = Arc::new(FailAfter(
        ScriptedModel::new(script.clone()),
        std::sync::atomic::AtomicUsize::new(5),
    ));
    h.start(brain, task("t7-plan-with-dependency")).unwrap();
    let snapshot = wait(&h);
    let plan = snapshot.plan.unwrap();
    assert_eq!(plan.status, PlanState::Failed, "{plan:?}");
    assert!(plan
        .reason
        .as_deref()
        .unwrap()
        .starts_with("Task tests: The model failed"));
    assert_eq!(plan.passed, 1);
    assert_eq!(plan.tasks[0].status, TaskStatus::Completed);
    assert_eq!(plan.tasks[1].status, TaskStatus::Failed);
    assert_eq!(plan.tasks[2].status, TaskStatus::Ready);
    assert!(plan.can_continue);
    let root = dir
        .path()
        .join(LADDER_FOLDER)
        .join("t7-plan-with-dependency");
    assert!(root.join("test_grades.py").exists());

    h.continue_plan_scripted().unwrap();
    let snapshot = wait(&h);
    let plan = snapshot.plan.unwrap();
    assert_eq!(plan.status, PlanState::Completed, "{plan:?}");
    assert_eq!(plan.passed, 3);
    // letter ran once; tests ran twice (the failed try, then the resumed one).
    let entries = Journal::replay(&root);
    let runs_of = |id: &str| {
        entries
            .iter()
            .filter(|e| matches!(e, Entry::Task { plan_task: Some(p), .. } if p == id))
            .count()
    };
    assert_eq!(
        (runs_of("letter"), runs_of("tests"), runs_of("report")),
        (1, 2, 1)
    );
    // The report's task saw a line on each task done before it, with fresh
    // steps of its own.
    assert_eq!(snapshot.steps.len(), 3);
    assert_eq!(snapshot.steps[0].index, 1);
}

#[test]
fn the_executor_sees_its_task_and_one_line_per_task_done() {
    let entries = vec![
        Entry::PlanRequest {
            plan: "plan-1".into(),
            request: "Grades and a report.".into(),
            criteria: vec![],
            model: "m".into(),
            at_ms: 1,
        },
        Entry::Plan {
            plan: "plan-1".into(),
            thought: "t".into(),
            tasks: plan::parse(&evidence::script("t7-plan-with-dependency")[0], &[])
                .unwrap()
                .1,
            model_ms: 1,
        },
        Entry::Task {
            task: "task-1".into(),
            description: "letter".into(),
            criteria: vec![Criterion::WorkRuns],
            model: "m".into(),
            at_ms: 2,
            plan: Some("plan-1".into()),
            plan_task: Some("letter".into()),
        },
        Entry::Status {
            task: "task-1".into(),
            status: TaskStatus::Completed,
            reason: Some("Verified: grades.py ran without an error.".into()),
            at_ms: 3,
        },
    ];
    let view = plan::PlanView::read(&entries).unwrap();
    let next = view.next().unwrap();
    assert_eq!(next.id, "tests");
    assert_eq!(view.restart_point(), "task-1");
    let request = view.executor_request(next);
    assert!(request.starts_with("Write test_grades.py"), "{request}");
    assert!(
        request.contains("plan for: Grades and a report."),
        "{request}"
    );
    assert!(
        request.contains("Done already:\n- letter: Write grades.py"),
        "{request}"
    );
    assert!(request.contains("Verified: grades.py ran"), "{request}");
    // Nothing done yet: the plan's own checkpoint.
    let view = plan::PlanView::read(&entries[..2]).unwrap();
    assert_eq!(view.restart_point(), "plan-1");
    assert!(!view
        .executor_request(view.next().unwrap())
        .contains("Done already"));
}

#[test]
fn a_plan_the_app_closed_during_stops_and_can_resume() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("scratch");
    std::fs::create_dir_all(&root).unwrap();
    {
        let mut journal = Journal::open(&root).unwrap();
        for entry in [
            Entry::PlanRequest {
                plan: "plan-1".into(),
                request: "Grades.".into(),
                criteria: vec![],
                model: "m".into(),
                at_ms: 1,
            },
            Entry::Plan {
                plan: "plan-1".into(),
                thought: "t".into(),
                tasks: plan::parse(&evidence::script("t7-plan-with-dependency")[0], &[])
                    .unwrap()
                    .1,
                model_ms: 1,
            },
            Entry::PlanStatus {
                plan: "plan-1".into(),
                status: PlanState::Running,
                reason: None,
                at_ms: 2,
            },
            Entry::Task {
                task: "task-1".into(),
                description: "letter".into(),
                criteria: vec![Criterion::WorkRuns],
                model: "m".into(),
                at_ms: 3,
                plan: Some("plan-1".into()),
                plan_task: Some("letter".into()),
            },
            Entry::Status {
                task: "task-1".into(),
                status: TaskStatus::Running,
                reason: None,
                at_ms: 4,
            },
            Entry::StepStarted {
                task: "task-1".into(),
                step: 1,
                at_ms: 5,
            },
        ] {
            journal.append(&entry).unwrap();
        }
    }
    let h = harness(&dir);
    h.open_project("scratch".into()).unwrap();
    let snapshot = h.snapshot();
    assert_eq!(snapshot.task.unwrap().status, TaskStatus::Stopped);
    let plan = snapshot.plan.unwrap();
    assert_eq!(plan.status, PlanState::Stopped);
    assert_eq!(
        plan.reason.as_deref(),
        Some("The app closed during task letter.")
    );
    assert!(plan.can_continue);
    assert_eq!(plan.passed, 0);
    // Written back: opening it again changes nothing.
    h.open_project("scratch".into()).unwrap();
    assert_eq!(Journal::replay(&root).len(), 8);
}

#[test]
fn restore_puts_the_project_back_as_the_checkpoint_kept_it() {
    let dir = tempfile::tempdir().unwrap();
    let lab = PythonLab::new(
        dir.path().into(),
        Arc::new(LocalPython),
        Duration::from_secs(5),
    );
    std::fs::write(dir.path().join("a.py"), "a = 1\n").unwrap();
    std::fs::create_dir_all(dir.path().join("pkg")).unwrap();
    std::fs::write(dir.path().join("pkg/m.py"), "m = 1\n").unwrap();
    lab.checkpoint("one").unwrap();
    std::fs::write(dir.path().join("a.py"), "a = 2\n").unwrap();
    std::fs::write(dir.path().join("b.py"), "b = 1\n").unwrap();
    std::fs::remove_dir_all(dir.path().join("pkg")).unwrap();
    std::fs::create_dir_all(dir.path().join(".harness")).unwrap();
    std::fs::write(dir.path().join(".harness/journal.jsonl"), "{}\n").unwrap();
    lab.restore("one").unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.py")).unwrap(),
        "a = 1\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("pkg/m.py")).unwrap(),
        "m = 1\n"
    );
    assert!(!dir.path().join("b.py").exists());
    // The harness's own records stay.
    assert!(dir.path().join(".harness/journal.jsonl").exists());
    assert!(lab.restore("missing").is_err());

    // Hidden files stay at any depth; folders it emptied go.
    std::fs::create_dir_all(dir.path().join("keep")).unwrap();
    std::fs::write(dir.path().join("keep/.env"), "x\n").unwrap();
    std::fs::create_dir_all(dir.path().join("gone/deep")).unwrap();
    std::fs::write(dir.path().join("gone/deep/g.py"), "g = 1\n").unwrap();
    lab.restore("one").unwrap();
    assert!(dir.path().join("keep/.env").exists());
    assert!(!dir.path().join("gone").exists());
    // A checkpoint is published whole: no partial copy is left behind.
    lab.checkpoint("two").unwrap();
    let names: Vec<String> = std::fs::read_dir(dir.path().join(".harness/checkpoints"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert!(!names.iter().any(|n| n.contains("partial")), "{names:?}");
}

#[test]
fn an_answer_saved_before_the_app_closed_lets_the_plan_go_on() {
    let tasks = plan::parse(&evidence::script("t7-plan-with-dependency")[0], &[])
        .unwrap()
        .1;
    let first = tasks[0].id.clone();
    let mut entries = vec![
        Entry::PlanRequest {
            plan: "plan-1".into(),
            request: "Grades.".into(),
            criteria: vec![],
            model: "m".into(),
            at_ms: 1,
        },
        Entry::Plan {
            plan: "plan-1".into(),
            thought: "t".into(),
            tasks,
            model_ms: 1,
        },
        Entry::PlanStatus {
            plan: "plan-1".into(),
            status: PlanState::Running,
            reason: None,
            at_ms: 2,
        },
        Entry::Recovery {
            plan: "plan-1".into(),
            plan_task: first.clone(),
            thought: "t".into(),
            decision: plan::Decision::Blocked {
                question: "Which file?".into(),
            },
            model_ms: 1,
        },
        Entry::PlanStatus {
            plan: "plan-1".into(),
            status: PlanState::Blocked,
            reason: Some("Which file?".into()),
            at_ms: 3,
        },
    ];
    let view = plan::PlanView::read(&entries).unwrap();
    assert!(view.started);
    assert!(!view.can_continue());
    // Without the Blocked status (its write failed) it still asks.
    let decided = &entries[..entries.len() - 1];
    let view = plan::PlanView::read(decided).unwrap();
    assert_eq!(view.state, PlanState::Blocked);
    assert!(view.state.needs_answer() && view.blocked.is_some());
    assert!(!view.can_continue());
    // When that write failed, the plan is marked Failed in memory only:
    // the question still stands, and Resume must not run the task.
    let mut failed = decided.to_vec();
    failed.push(Entry::PlanStatus {
        plan: "plan-1".into(),
        status: PlanState::Failed,
        reason: Some("Could not save the plan status".into()),
        at_ms: 3,
    });
    let view = plan::PlanView::read(&failed).unwrap();
    assert_eq!(view.state, PlanState::Failed);
    assert_eq!(
        view.blocked.as_ref().map(|(_, q)| q.as_str()),
        Some("Which file?")
    );
    assert!(!view.can_continue());
    entries.push(Entry::PersonAnswer {
        plan: "plan-1".into(),
        plan_task: first,
        text: "data.csv".into(),
        at_ms: 4,
    });
    let view = plan::PlanView::read(&entries).unwrap();
    assert_eq!(view.state, PlanState::Blocked);
    assert!(view.can_continue());
}

#[test]
fn a_draft_that_never_ran_waits_for_approval_when_resumed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("scratch");
    std::fs::create_dir_all(&root).unwrap();
    {
        let mut journal = Journal::open(&root).unwrap();
        for entry in [
            Entry::PlanRequest {
                plan: "plan-1".into(),
                request: "Grades.".into(),
                criteria: vec![],
                model: "m".into(),
                at_ms: 1,
            },
            Entry::PlanStatus {
                plan: "plan-1".into(),
                status: PlanState::Planning,
                reason: None,
                at_ms: 2,
            },
            Entry::Plan {
                plan: "plan-1".into(),
                thought: "t".into(),
                tasks: plan::parse(&evidence::script("t7-plan-with-dependency")[0], &[])
                    .unwrap()
                    .1,
                model_ms: 1,
            },
        ] {
            journal.append(&entry).unwrap();
        }
    }
    let h = harness(&dir);
    h.open_project("scratch".into()).unwrap();
    assert_eq!(h.snapshot().plan.unwrap().status, PlanState::Stopped);
    // No checkpoint was ever taken, and approval is on: it asks first.
    h.continue_plan_scripted().unwrap();
    let plan = wait(&h).plan.unwrap();
    assert_eq!(plan.status, PlanState::AwaitingApproval, "{plan:?}");
}

#[test]
fn a_forgiving_edit_leaves_quotes_inside_a_docstring_alone() {
    let dir = tempfile::tempdir().unwrap();
    let lab = PythonLab::new(
        dir.path().into(),
        Arc::new(LocalPython),
        Duration::from_secs(5),
    );
    let text = "def f():\n    \"\"\"\n    say \"hi\"\n    \"\"\"\n    return 1\n";
    std::fs::write(dir.path().join("d.py"), text).unwrap();
    let outcome = edit(&lab, "d.py", "say 'hi'", "say 'bye'");
    assert!(!outcome.ok, "{}", outcome.text);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("d.py")).unwrap(),
        text
    );
    // Triple quotes in a comment or a short string don't fool it.
    let text = "# \"\"\"\nmark = '\"\"\"'\ntext = \"\"\"\nstatus = \"on\"\n\"\"\"\n";
    std::fs::write(dir.path().join("e.py"), text).unwrap();
    let outcome = edit(&lab, "e.py", "status = 'on'", "status = 'off'");
    assert!(!outcome.ok, "{}", outcome.text);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("e.py")).unwrap(),
        text
    );
    // An escaped triple quote doesn't close the string.
    let text = "text = \"\"\"\nescaped: \\\"\"\"\nstatus = \"on\"\n\"\"\"\n";
    std::fs::write(dir.path().join("g.py"), text).unwrap();
    let outcome = edit(&lab, "g.py", "status = 'on'", "status = 'off'");
    assert!(!outcome.ok, "{}", outcome.text);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("g.py")).unwrap(),
        text
    );
    // A short string continued with a backslash is still a string.
    let text = "text = 'prefix \\\nstatus = \"on\" \\\n'\n";
    std::fs::write(dir.path().join("h.py"), text).unwrap();
    let outcome = edit(&lab, "h.py", "status = 'on' \\", "status = 'off' \\");
    assert!(!outcome.ok, "{}", outcome.text);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("h.py")).unwrap(),
        text
    );
    // Non-ASCII text in a docstring doesn't trip it up.
    let text = "def f():\n    \"\"\"café ☕\"\"\"\n    return \"x\"\n";
    std::fs::write(dir.path().join("i.py"), text).unwrap();
    let outcome = edit(&lab, "i.py", "return 'x'", "return 'y'");
    assert!(outcome.ok, "{}", outcome.text);
    // Code after a docstring still gets the forgiving match.
    let text = "def f():\n    \"\"\"Doc.\"\"\"\n    return \"x\"\n";
    std::fs::write(dir.path().join("f.py"), text).unwrap();
    let outcome = edit(&lab, "f.py", "return 'x'", "return 'y'");
    assert!(outcome.ok, "{}", outcome.text);
}

// MARK: Recovery (H5)

#[test]
fn recovery_decisions_follow_their_grammar_and_are_checked() {
    let g = plan::recovery_grammar();
    for decision in ["retry", "revise", "replan", "blocked"] {
        assert!(
            g.contains(&format!("\\\"{decision}\\\"")),
            "{decision}:\n{g}"
        );
    }
    assert!(g.contains("checks ::="));
    let (thought, decision) = plan::parse_recovery(
        r#"{"thought": "t", "decision": "revise", "do": "Write a.py.", "checks": [{"check": "work_runs"}]}"#,
        "a",
    )
    .unwrap();
    assert_eq!(thought, "t");
    assert_eq!(
        decision,
        plan::Decision::Revise {
            description: "Write a.py.".into(),
            criteria: vec![Criterion::WorkRuns]
        }
    );
    for (text, expected) in [
        (
            r#"{"thought": "t", "decision": "retry"}"#,
            "needs \"diagnosis\"",
        ),
        (
            r#"{"thought": "t", "decision": "blocked", "question": " "}"#,
            "needs \"question\"",
        ),
        (
            r#"{"thought": "t", "decision": "shrug"}"#,
            "no decision called",
        ),
        (
            r#"{"thought": "t", "decision": "revise", "do": "x", "checks": []}"#,
            "at least one check",
        ),
        (r#"{"thought": "t"}"#, "no \"decision\""),
    ] {
        let err = plan::parse_recovery(text, "a").unwrap_err();
        assert!(err.contains(expected), "{text}: {err}");
    }
    // Every example in the instructions is a decision the parser takes.
    for line in plan::recovery_instructions()
        .lines()
        .filter(|l| l.starts_with('{'))
    {
        plan::parse_recovery(line, "a").unwrap_or_else(|e| panic!("{line}: {e}"));
    }
    // A replan may come after tasks done, never reuse their ids.
    let done = vec!["stats".to_string()];
    let replan = r#"{"thought": "t", "tasks": [{"id": "report", "do": "Run it.", "after": ["stats"], "checks": [{"check": "work_runs"}]}]}"#;
    assert!(plan::parse(replan, &done).is_ok());
    assert!(plan::parse(replan, &[])
        .unwrap_err()
        .contains("not an earlier task"));
    let reuse = replan.replace("\"id\": \"report\"", "\"id\": \"stats\"");
    assert!(plan::parse(&reuse, &done)
        .unwrap_err()
        .contains("already done"));
}

#[test]
fn a_missing_dependency_is_replanned_and_the_plan_completes() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let h = harness(&dir);
    h.set_approve_plans(false);
    h.start_ladder_scripted("t8-replan".into()).unwrap();
    let plan = wait(&h).plan.unwrap();
    assert_eq!(plan.status, PlanState::Completed, "{plan:?}");
    let ids: Vec<&str> = plan.tasks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["stats", "report"]);
    let entries = Journal::replay(&dir.path().join(LADDER_FOLDER).join("t8-replan"));
    assert!(entries.iter().any(|e| matches!(
        e,
        Entry::Recovery { plan_task, decision: plan::Decision::Replan { .. }, .. } if plan_task == "run-report"
    )));
    assert!(entries.iter().any(|e| matches!(
        e,
        Entry::Result { tool, text, .. } if tool == "give_up" && text.starts_with("Gave up: report.py imports stats")
    )));
    assert!(h.transcript().contains("give_up"));
}

#[test]
fn a_task_that_cant_succeed_ends_blocked_and_the_answer_reaches_it() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let h = harness(&dir);
    h.set_approve_plans(false);
    h.start_ladder_scripted("t9-blocked".into()).unwrap();
    let snapshot = wait(&h);
    let plan = snapshot.plan.unwrap();
    assert_eq!(plan.status, PlanState::Blocked, "{plan:?}");
    assert!(plan.question.as_deref().unwrap().contains("internet"));
    assert!(!plan.can_continue);
    assert_eq!(plan.tasks[0].recoveries, 1);
    // Rolled back to before the task: the download program is gone.
    let root = dir.path().join(LADDER_FOLDER).join("t9-blocked");
    assert!(!root.join("weather.py").exists());
    assert!(matches!(
        h.continue_plan_scripted(),
        Err(HarnessError::NoPlan)
    ));
    assert!(matches!(
        h.answer_plan_scripted("  ".into()),
        Err(HarnessError::EmptyTask)
    ));

    h.answer_plan_scripted("Yes, print a sample.".into())
        .unwrap();
    let plan = wait(&h).plan.unwrap();
    assert_eq!(plan.status, PlanState::Completed, "{plan:?}");
    assert!(plan.question.is_none());
    let entries = Journal::replay(&root);
    let told = entries.iter().any(|e| {
        matches!(
            e,
            Entry::PersonAnswer { text, .. } if text == "Yes, print a sample."
        )
    });
    assert!(told);
    // The rerun saw the question and the answer.
    let view = plan::PlanView::read(plan::current(&entries).unwrap()).unwrap();
    let request = view.executor_request(&view.tasks[0]);
    assert!(
        request.contains("They answered: Yes, print a sample."),
        "{request}"
    );
    assert!(std::fs::read_to_string(root.join("weather.py"))
        .unwrap()
        .contains("sample"));
}

#[test]
fn a_task_that_keeps_failing_ends_the_plan_after_two_recoveries() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let h = harness(&dir);
    h.set_approve_plans(false);
    let plan_text = r#"{"thought": "t", "tasks": [{"id": "a", "do": "Do it.", "after": [], "checks": [{"check": "work_runs"}]}]}"#;
    let give_up = r#"{"thought": "t", "tool": "give_up", "reason": "can't"}"#;
    let retry = r#"{"thought": "t", "decision": "retry", "diagnosis": "try harder"}"#;
    h.start_request(
        scripted(&[plan_text, give_up, retry, give_up, retry, give_up]),
        "Do it.".into(),
        SCRATCH_PROJECT,
        true,
    )
    .unwrap();
    let plan = wait(&h).plan.unwrap();
    assert_eq!(plan.status, PlanState::Failed, "{plan:?}");
    assert_eq!(
        plan.reason.as_deref(),
        Some("Task a: Bonsai gave up: can't (after 2 recoveries)")
    );
    assert_eq!(plan.tasks[0].recoveries, 2);
    assert!(plan.can_continue);
}

// MARK: Context (H6)

#[test]
fn read_symbol_shows_one_definition_with_its_line_numbers() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let lab = PythonLab::new(
        dir.path().into(),
        Arc::new(LocalPython),
        Duration::from_secs(5),
    );
    std::fs::write(
        dir.path().join("m.py"),
        "import math\n\n\ndef area(r):\n    return math.pi * r ** 2\n\n\nclass Box:\n    @property\n    def size(self):\n        return 1\n",
    )
    .unwrap();
    let read = |name: &str| {
        lab.execute(&call(&format!(
            r#"{{"tool": "read_symbol", "path": "m.py", "name": "{name}"}}"#
        )))
    };
    let out = read("area");
    assert!(out.ok, "{}", out.text);
    assert_eq!(
        out.text,
        "m.py, lines 4 to 5:\n  4| def area(r):\n  5|     return math.pi * r ** 2"
    );
    // A method, with its decorator, by its own name or as Class.method.
    let out = read("size");
    assert!(
        out.ok
            && out
                .text
                .starts_with("m.py, lines 9 to 11:\n  9|     @property"),
        "{}",
        out.text
    );
    assert_eq!(read("Box.size").text, out.text);
    let out = read("volume");
    assert!(
        !out.ok && out.text.contains("It has: Box, Box.size, area."),
        "{}",
        out.text
    );
    std::fs::write(dir.path().join("bad.py"), "def broken(:\n").unwrap();
    let out = lab.execute(&call(
        r#"{"tool": "read_symbol", "path": "bad.py", "name": "broken"}"#,
    ));
    assert!(
        !out.ok
            && out
                .text
                .starts_with("Python can't read the definitions in bad.py"),
        "{}",
        out.text
    );
}

#[test]
fn earlier_work_is_one_line_per_finished_task_or_plan() {
    let task = |id: &str, description: &str, plan: Option<&str>| Entry::Task {
        task: id.into(),
        description: description.into(),
        criteria: vec![],
        model: "m".into(),
        at_ms: 1,
        plan: plan.map(str::to_string),
        plan_task: plan.map(|_| "x".to_string()),
    };
    let status = |id: &str, status: TaskStatus, reason: &str| Entry::Status {
        task: id.into(),
        status,
        reason: Some(reason.into()),
        at_ms: 2,
    };
    let entries = vec![
        task("task-1", "Write hello.py", None),
        status(
            "task-1",
            TaskStatus::Completed,
            "Verified: hello.py printed Hello.",
        ),
        Entry::PlanRequest {
            plan: "plan-1".into(),
            request: "Grades and a report".into(),
            criteria: vec![],
            model: "m".into(),
            at_ms: 3,
        },
        task("task-2", "letter", Some("plan-1")),
        status("task-2", TaskStatus::Completed, "Verified."),
        Entry::PlanStatus {
            plan: "plan-1".into(),
            status: PlanState::Failed,
            reason: Some("Task tests: stuck.".into()),
            at_ms: 4,
        },
        task("task-3", "Now add a chart", None),
        status("task-3", TaskStatus::Running, ""),
    ];
    let earlier = prompt::earlier_work(&entries, &["task-3"]);
    assert_eq!(
        earlier,
        "- Write hello.py (completed: Verified: hello.py printed Hello.)\n- Grades and a report (failed: Task tests: stuck.)"
    );
    // The task running now is never its own earlier work.
    assert_eq!(prompt::earlier_work(&entries[..2], &["task-1"]), "");
}

#[test]
fn a_second_request_is_told_the_earlier_work() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let h = harness(&dir);
    let brain = scripted(
        &evidence::script("t11-second-request")
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    h.start(
        Arc::clone(&brain) as Arc<dyn Brain>,
        task("t11-second-request"),
    )
    .unwrap();
    let snapshot = wait(&h);
    assert_eq!(
        snapshot.task.unwrap().status,
        TaskStatus::Completed,
        "{:?}",
        snapshot.steps
    );
    let first = &brain.0.requests()[0];
    let task_message = &first.messages[1].content;
    assert!(
        task_message
            .contains("Earlier work in this project:\n- Write shapes.py with rectangle_area"),
        "{task_message}"
    );
    assert!(snapshot.steps[0]
        .result
        .contains("def circle_area(radius):"));
    assert!(!snapshot.steps[0].result.contains("rectangle_area"));
}

// MARK: PL1e, Bonsai in the person's projects

#[test]
fn a_write_over_a_file_changed_since_bonsai_read_it_is_refused() {
    if !has_python() {
        return;
    }
    let (_dir, lab) = seeded("t1-hello");
    let wrote = lab.execute(&call(
        r#"{"tool": "write_file", "path": "notes.txt", "content": "one\n"}"#,
    ));
    assert!(wrote.ok, "{}", wrote.text);
    // Someone edits it in Python Lab or the Files app.
    std::fs::write(lab.root().join("notes.txt"), "one\nmine\n").unwrap();
    let refused = lab.execute(&call(
        r#"{"tool": "write_file", "path": "notes.txt", "content": "two\n"}"#,
    ));
    assert!(!refused.ok);
    assert!(
        refused.text.contains("notes.txt changed outside Bonsai"),
        "{}",
        refused.text
    );
    let edit = lab.execute(&call(
        r#"{"tool": "edit_file", "path": "notes.txt", "old": "one", "new": "1"}"#,
    ));
    assert!(!edit.ok && edit.text.contains("changed outside Bonsai"));
    assert_eq!(
        std::fs::read_to_string(lab.root().join("notes.txt")).unwrap(),
        "one\nmine\n"
    );
    // Once it has read the new text, it may change it.
    assert!(
        lab.execute(&call(r#"{"tool": "read_file", "path": "notes.txt"}"#))
            .ok
    );
    let wrote = lab.execute(&call(
        r#"{"tool": "write_file", "path": "notes.txt", "content": "two\n"}"#,
    ));
    assert!(wrote.ok, "{}", wrote.text);
}

#[test]
fn a_run_does_not_forgive_an_earlier_outside_change() {
    if !has_python() {
        return;
    }
    let (_dir, lab) = seeded("t1-hello");
    assert!(
        lab.execute(&call(
            r#"{"tool": "write_file", "path": "notes.txt", "content": "one\n"}"#
        ))
        .ok
    );
    std::fs::write(lab.root().join("notes.txt"), "mine\n").unwrap();
    // A run that fails at once, and one that leaves notes.txt alone.
    let _ = lab.execute(&call(r#"{"tool": "run_python", "path": "missing.py"}"#));
    assert!(
        lab.execute(&call(
            r#"{"tool": "write_file", "path": "quiet.py", "content": "x = 1\n"}"#
        ))
        .ok
    );
    let _ = lab.execute(&call(r#"{"tool": "run_python", "path": "quiet.py"}"#));
    let refused = lab.execute(&call(
        r#"{"tool": "write_file", "path": "notes.txt", "content": "two\n"}"#,
    ));
    assert!(!refused.ok, "{}", refused.text);
    assert_eq!(
        std::fs::read_to_string(lab.root().join("notes.txt")).unwrap(),
        "mine\n"
    );
}

#[test]
fn a_run_that_touches_a_file_changed_outside_does_not_bless_it() {
    if !has_python() {
        return;
    }
    let (_dir, lab) = seeded("t1-hello");
    assert!(
        lab.execute(&call(
            r#"{"tool": "write_file", "path": "notes.txt", "content": "one\n"}"#
        ))
        .ok
    );
    std::fs::write(lab.root().join("notes.txt"), "mine\n").unwrap();
    assert!(
        lab.execute(&call(
            r#"{"tool": "write_file", "path": "append.py", "content": "open('notes.txt', 'a').write('more\\n')\n"}"#
        ))
        .ok
    );
    let _ = lab.execute(&call(r#"{"tool": "run_python", "path": "append.py"}"#));
    let refused = lab.execute(&call(
        r#"{"tool": "write_file", "path": "notes.txt", "content": "two\n"}"#,
    ));
    assert!(!refused.ok, "{}", refused.text);
    assert_eq!(
        std::fs::read_to_string(lab.root().join("notes.txt")).unwrap(),
        "mine\nmore\n"
    );
}

#[test]
fn what_bonsais_own_script_changes_is_not_an_outside_change() {
    if !has_python() {
        return;
    }
    let (_dir, lab) = seeded("t1-hello");
    assert!(
        lab.execute(&call(
            r#"{"tool": "write_file", "path": "data.txt", "content": "a\n"}"#
        ))
        .ok
    );
    assert!(lab
        .execute(&call(
            r#"{"tool": "write_file", "path": "grow.py", "content": "open('data.txt', 'a').write('b\\n')\n"}"#
        ))
        .ok);
    let ran = lab.execute(&call(r#"{"tool": "run_python", "path": "grow.py"}"#));
    assert!(ran.ok, "{}", ran.text);
    let wrote = lab.execute(&call(
        r#"{"tool": "write_file", "path": "data.txt", "content": "c\n"}"#,
    ));
    assert!(wrote.ok, "{}", wrote.text);
}

#[test]
fn a_request_runs_in_the_chosen_project() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let h = harness(&dir);
    // A project that doesn't exist is refused, not made.
    assert!(matches!(
        h.start_request(scripted(&[FINISH_CALL]), "Say hi".into(), "My Work", false),
        Err(HarnessError::Storage { .. })
    ));
    assert!(!dir.path().join("My Work").exists());
    // Hidden names and ladder names aren't a person's projects.
    for bad in [".ladder", "t1-hello", "../x"] {
        assert!(
            h.start_request(scripted(&[FINISH_CALL]), "Say hi".into(), bad, false)
                .is_err(),
            "{bad}"
        );
    }
    std::fs::create_dir(dir.path().join("My Work")).unwrap();
    h.start_request(scripted(&[FINISH_CALL]), "Say hi".into(), "My Work", false)
        .unwrap();
    let snapshot = wait(&h);
    assert_eq!(snapshot.project.as_deref(), Some("My Work"));
    // The task's journal is in that project, not in scratch.
    assert!(dir.path().join("My Work").join(Journal::FILE).is_file());
    assert!(!dir.path().join("scratch").exists());
    // A name with spaces around it is the same project, and the screen
    // is told its saved name.
    h.start_request(
        scripted(&[FINISH_CALL]),
        "Say hi".into(),
        "  My Work ",
        false,
    )
    .unwrap();
    assert_eq!(wait(&h).project.as_deref(), Some("My Work"));
}

#[test]
fn a_tracebacks_project_frames_are_found_for_opening() {
    let output = "before\nTraceback (most recent call last):\n  File \"old.py\", line 1, in <module>\nValueError\nTraceback (most recent call last):\n  File \"main.py\", line 3, in <module>\n    helper.half(4)\n  File \"lib/helper.py\", line 2, in half\n    return n / 0\n  File \"/usr/lib/python3.15/fractions.py\", line 9, in f\n  File \"<string>\", line 1, in <module>\nZeroDivisionError: division by zero\n";
    let frames = super::python_lab::python_traceback_frames(output.into());
    let found: Vec<_> = frames
        .iter()
        .map(|f| (f.file.as_str(), f.line, f.function.as_str()))
        .collect();
    assert_eq!(
        found,
        [("main.py", 3, "<module>"), ("lib/helper.py", 2, "half")]
    );
    assert!(super::python_lab::python_traceback_frames("all fine\n".into()).is_empty());
}

#[test]
fn outputs_list_every_kind_but_python() {
    use super::artifacts::{list_outputs, output_kind_of};
    assert_eq!(output_kind_of("out/results.CSV"), Some("table"));
    assert_eq!(output_kind_of("data.json"), Some("json"));
    assert_eq!(output_kind_of("report.md"), Some("text"));
    assert_eq!(output_kind_of("page.html"), Some("html"));
    assert_eq!(output_kind_of("plot.png"), Some("png"));
    assert_eq!(output_kind_of("model.bin"), Some("file"));
    assert_eq!(output_kind_of("main.py"), None);

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("__pycache__")).unwrap();
    std::fs::create_dir_all(root.join(".workspace")).unwrap();
    std::fs::write(root.join("main.py"), "print(1)").unwrap();
    std::fs::write(root.join("__pycache__/main.cpython-315.pyc"), "x").unwrap();
    std::fs::write(root.join(".workspace/runs.json"), "[]").unwrap();
    std::fs::write(root.join("results.csv"), "a,b\n1,2\n").unwrap();
    std::fs::write(root.join("notes.txt"), "hi").unwrap();
    let mut found: Vec<_> = list_outputs(root)
        .into_iter()
        .map(|a| (a.path, a.kind))
        .collect();
    found.sort();
    assert_eq!(
        found,
        [
            ("notes.txt".to_string(), "text".to_string()),
            ("results.csv".to_string(), "table".to_string())
        ]
    );
}

#[test]
fn tables_read_like_python_csv_writes_them() {
    use super::artifacts::{parse_table, CELL_MAX_CHARS, TABLE_MAX_COLUMNS, TABLE_MAX_ROWS};
    let table = parse_table(
        "\u{feff}name,note\r\nAda,\"likes, commas\"\r\nBo,\"said \"\"hi\"\"\nthen left\"\r\n",
        ',',
        false,
    );
    assert_eq!(
        table.rows,
        vec![
            vec!["name", "note"],
            vec!["Ada", "likes, commas"],
            vec!["Bo", "said \"hi\"\nthen left"],
        ]
    );
    assert_eq!(table.total_rows, 3);
    assert!(!table.cut);

    // No newline at the end, tabs, and an empty last cell.
    let tsv = parse_table("a\tb\n1\t", '\t', false);
    assert_eq!(tsv.rows, vec![vec!["a", "b"], vec!["1", ""]]);

    let long = "x".repeat(CELL_MAX_CHARS + 5);
    let many: String = (0..TABLE_MAX_ROWS + 10)
        .map(|n| format!("{n},{long}\n"))
        .collect();
    let big = parse_table(&many, ',', true);
    assert_eq!(big.rows.len(), TABLE_MAX_ROWS);
    assert_eq!(big.total_rows, (TABLE_MAX_ROWS + 10) as u64);
    assert!(big.cut && big.partial);
    let wide: String = (0..TABLE_MAX_COLUMNS + 100)
        .map(|n| format!("{n},"))
        .collect();
    let wide = parse_table(&wide, ',', false);
    assert_eq!(wide.rows[0].len(), TABLE_MAX_COLUMNS);
    assert!(wide.cut);
    assert_eq!(big.rows[0][1].chars().count(), CELL_MAX_CHARS + 1);
}

#[test]
fn json_is_indented_and_text_is_cut_at_a_character() {
    use super::artifacts::{artifact_text, TEXT_MAX_BYTES};
    let dir = tempfile::tempdir().unwrap();
    let json = dir.path().join("data.json");
    std::fs::write(&json, r#"{"a":[1,2]}"#).unwrap();
    let shown = artifact_text(json.to_string_lossy().into()).unwrap();
    assert_eq!(shown.text, "{\n  \"a\": [\n    1,\n    2\n  ]\n}");
    assert!(!shown.truncated);

    let broken = dir.path().join("broken.json");
    std::fs::write(&broken, "{not json").unwrap();
    assert_eq!(
        artifact_text(broken.to_string_lossy().into()).unwrap().text,
        "{not json"
    );

    let long = dir.path().join("long.txt");
    std::fs::write(&long, "é".repeat(TEXT_MAX_BYTES)).unwrap();
    let cut = artifact_text(long.to_string_lossy().into()).unwrap();
    assert!(cut.truncated);
    assert!(cut.text.len() <= TEXT_MAX_BYTES);

    assert!(artifact_text(dir.path().join("missing.txt").to_string_lossy().into()).is_err());
}

/// Runs `path` under Python Lab's debugger with `breakpoints`, answering
/// each pause with the next of `commands`. A stand-in `_devicelab_io`
/// prints each pause as `PAUSE <json>`. Returns (ok, stdout, stderr).
fn debug_run(
    project: &std::path::Path,
    path: &str,
    breakpoints: &[(&str, u32)],
    commands: &[&str],
) -> (bool, String, String) {
    let breakpoints = breakpoints
        .iter()
        .map(|(file, line)| crate::workspace::DebugBreakpoint {
            file: file.to_string(),
            line: *line,
        })
        .collect();
    let script = super::python_lab::python_lab_debug_script(
        project.display().to_string(),
        path.into(),
        breakpoints,
    );
    debug_host(&script, commands)
}

const DEBUG_HOST: &str = "import sys, types, traceback\nio = types.ModuleType('_devicelab_io')\ndef _write(kind, text):\n    if kind == 6:\n        sys.__stdout__.write('PAUSE ' + text + '\\n')\n    else:\n        (sys.__stdout__ if kind == 1 else sys.__stderr__).write(text)\nio.write = _write\nio.readline = lambda: sys.__stdin__.readline()\nsys.modules['_devicelab_io'] = io\ntry:\n    exec(compile(sys.argv[1], '<console>', 'exec'), {'__name__': '__devicelab__'})\nexcept BaseException as e:\n    if not getattr(e, '_devicelab_shown', False):\n        traceback.print_exc()\n    sys.exit(1)\n";

/// Runs `script` in `debug_run`'s stand-in host.
fn debug_host(script: &str, commands: &[&str]) -> (bool, String, String) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("python3")
        .arg("-I")
        .arg("-c")
        .arg(DEBUG_HOST)
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input: String = commands.iter().map(|c| format!("{c}\n")).collect();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn pauses(stdout: &str) -> Vec<super::python_lab::DebugPause> {
    stdout
        .lines()
        .filter_map(|l| l.strip_prefix("PAUSE "))
        .map(|j| super::python_lab::python_debug_pause(j.to_string()).expect("a pause"))
        .collect()
}

fn at(pause: &super::python_lab::DebugPause) -> (String, u32, String) {
    let top = pause.stack.last().unwrap();
    (top.file.clone(), top.line, top.function.clone())
}

#[test]
fn debug_pauses_on_breakpoints_and_steps() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::write(
        root.join("lib/helper.py"),
        "def half(n):\n    result = n / 2\n    return result\n",
    )
    .unwrap();
    std::fs::write(
        root.join("main.py"),
        "import json\nfrom lib.helper import half\ntotal = 10\nitems = [1, 2, 3]\nx = half(total)\ny = json.dumps(items)\nprint('done', x, y)\n",
    )
    .unwrap();
    // Pause on line 5, step into half, step over a line, out, then continue.
    // The json module's own lines never pause.
    let (ok, out, err) = debug_run(
        root,
        "main.py",
        &[("main.py", 5)],
        &["step", "next", "out", "continue"],
    );
    assert!(ok, "{out}{err}");
    assert!(out.contains("done 5.0 [1, 2, 3]"), "{out}");
    let p = pauses(&out);
    let places: Vec<_> = p.iter().map(at).collect();
    assert_eq!(
        places,
        [
            ("main.py".into(), 5, "<module>".into()),
            ("lib/helper.py".into(), 2, "half".into()),
            ("lib/helper.py".into(), 3, "half".into()),
            // Out of half: the caller's next line.
            ("main.py".into(), 6, "<module>".into()),
        ],
        "{out}"
    );
    // Module variables, without modules or functions.
    let names: Vec<_> = p[0].variables.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["total", "items"]);
    assert_eq!(p[0].variables[1].kind, "list");
    assert_eq!(p[0].variables[1].value, "[1, 2, 3]");
    // In half: its argument, then its local once set.
    assert_eq!(p[1].stack.len(), 2);
    assert_eq!(p[2].variables[1].name, "result");
    assert_eq!(p[2].variables[1].value, "5.0");
}

#[test]
fn debug_without_a_reachable_breakpoint_just_runs() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("main.py"),
        "print('one')\nif False:\n    print('never')\nprint('two')\n",
    )
    .unwrap();
    let (ok, out, err) = debug_run(root, "main.py", &[("main.py", 3)], &[]);
    assert!(ok, "{out}{err}");
    assert_eq!(out, "one\ntwo\n");
    // A breakpoint on the first line pauses there; an empty command continues.
    let (ok, out, _) = debug_run(root, "main.py", &[("main.py", 1)], &[""]);
    assert!(ok);
    assert_eq!(pauses(&out).len(), 1);
    assert!(out.ends_with("one\ntwo\n"), "{out}");
}

#[test]
fn debug_errors_still_show_the_projects_traceback() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("main.py"), "a = 1\nb = 0\nprint(a / b)\n").unwrap();
    let (ok, out, err) = debug_run(root, "main.py", &[("main.py", 2)], &["continue"]);
    assert!(!ok);
    assert_eq!(pauses(&out).len(), 1);
    assert!(err.contains("File \"main.py\", line 3"), "{err}");
    assert!(err.contains("ZeroDivisionError"), "{err}");
    let frames = super::python_lab::python_traceback_frames(err);
    assert_eq!(frames.len(), 1);
}

#[test]
fn debug_pause_json_is_read() {
    use super::python_lab::python_debug_pause;
    assert!(python_debug_pause("not json".into()).is_none());
    assert!(python_debug_pause(r#"{"stack": [], "variables": []}"#.into()).is_none());
    let pause = python_debug_pause(
        r#"{"stack": [{"file": "main.py", "line": 2, "function": "<module>"}], "variables": [{"name": "x", "kind": "int", "value": "1"}], "more": true}"#.into(),
    )
    .unwrap();
    assert_eq!(pause.variables[0].value, "1");
    assert!(pause.more_variables);
}

#[test]
fn debug_pauses_in_a_trailing_call() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("main.py"),
        "def main():\n    value = 3\n    return value * 2\n\nmain()\n",
    )
    .unwrap();
    let (ok, out, err) = debug_run(root, "main.py", &[("main.py", 3)], &["continue"]);
    assert!(ok, "{out}{err}");
    let places: Vec<_> = pauses(&out).iter().map(at).collect();
    assert_eq!(places, [("main.py".into(), 3, "main".into())], "{out}");
    // The last expression's value is still printed.
    assert!(out.ends_with("6\n"), "{out}");
}

#[test]
fn a_debug_session_forgets_the_last_ones_breakpoints() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("main.py"), "a = 1\nb = 2\nprint(a + b)\n").unwrap();
    let with = |lines: Vec<crate::workspace::DebugBreakpoint>| {
        super::python_lab::python_lab_debug_script(
            root.display().to_string(),
            "main.py".into(),
            lines,
        )
    };
    let first = with(vec![crate::workspace::DebugBreakpoint {
        file: "main.py".into(),
        line: 2,
    }]);
    let second = with(Vec::new());
    // Both sessions in one interpreter, as on the phone.
    let (ok, out, err) = debug_host(&format!("{first}\n{second}"), &["continue"]);
    assert!(ok, "{out}{err}");
    assert_eq!(pauses(&out).len(), 1, "{out}");
    assert_eq!(out.matches("3\n").count(), 2, "{out}");
}

// MARK: Review fixes after the Python Lab merge

fn plan_entries() -> (Vec<Entry>, Vec<String>) {
    let tasks = plan::parse(&evidence::script("t7-plan-with-dependency")[0], &[])
        .unwrap()
        .1;
    let ids = tasks.iter().map(|t| t.id.clone()).collect();
    let entries = vec![
        Entry::PlanRequest {
            plan: "plan-1".into(),
            request: "Grades.".into(),
            criteria: vec![],
            model: "m".into(),
            at_ms: 1,
        },
        Entry::Plan {
            plan: "plan-1".into(),
            thought: "t".into(),
            tasks,
            model_ms: 1,
        },
        Entry::PlanStatus {
            plan: "plan-1".into(),
            status: PlanState::Running,
            reason: None,
            at_ms: 2,
        },
    ];
    (entries, ids)
}

fn ran(entries: &mut Vec<Entry>, plan_task: &str, status: TaskStatus) {
    let task = format!("run-{plan_task}-{}", entries.len());
    entries.push(Entry::Task {
        task: task.clone(),
        description: "d".into(),
        criteria: vec![],
        model: "m".into(),
        at_ms: 3,
        plan: Some("plan-1".into()),
        plan_task: Some(plan_task.into()),
    });
    entries.push(Entry::Status {
        task,
        status,
        reason: Some("the check failed".into()),
        at_ms: 4,
    });
}

#[test]
fn a_plan_that_failed_its_final_check_is_not_offered_resume() {
    let (mut entries, ids) = plan_entries();
    for id in &ids {
        ran(&mut entries, id, TaskStatus::Completed);
    }
    entries.push(Entry::PlanStatus {
        plan: "plan-1".into(),
        status: PlanState::Failed,
        reason: Some("The request's checks failed.".into()),
        at_ms: 5,
    });
    let view = plan::PlanView::read(&entries).unwrap();
    assert!(view.next().is_none());
    assert!(!view.can_continue());
    // A plan that failed with a task still to do can go on.
    let (mut entries, ids) = plan_entries();
    ran(&mut entries, &ids[0], TaskStatus::Failed);
    entries.push(Entry::PlanStatus {
        plan: "plan-1".into(),
        status: PlanState::Failed,
        reason: Some("Task failed.".into()),
        at_ms: 5,
    });
    assert!(plan::PlanView::read(&entries).unwrap().can_continue());
}

#[test]
fn a_replan_whose_tasks_were_never_written_stays_pending() {
    let (mut entries, ids) = plan_entries();
    ran(&mut entries, &ids[0], TaskStatus::Failed);
    entries.push(Entry::Recovery {
        plan: "plan-1".into(),
        plan_task: ids[0].clone(),
        thought: "t".into(),
        decision: plan::Decision::Replan {
            diagnosis: "The data needs cleaning first.".into(),
        },
        model_ms: 1,
    });
    let view = plan::PlanView::read(&entries).unwrap();
    assert_eq!(
        view.replan,
        Some((ids[0].clone(), "The data needs cleaning first.".into()))
    );
    // The new tasks end it.
    let tasks = plan::parse(&evidence::script("t7-plan-with-dependency")[0], &[])
        .unwrap()
        .1;
    entries.push(Entry::Plan {
        plan: "plan-1".into(),
        thought: "again".into(),
        tasks,
        model_ms: 1,
    });
    assert_eq!(plan::PlanView::read(&entries).unwrap().replan, None);
}

#[test]
fn restoring_a_checkpoint_resets_what_bonsai_knows_of_each_file() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let lab = PythonLab::new(
        dir.path().into(),
        Arc::new(LocalPython),
        Duration::from_secs(5),
    );
    let write = |path: &str, content: &str| {
        let args = serde_json::json!({"tool": "write_file", "path": path, "content": content});
        lab.execute(&call(&args.to_string()))
    };
    assert!(write("notes.txt", "one\n").ok);
    lab.checkpoint("p0").unwrap();
    assert!(write("notes.txt", "two\n").ok);
    assert!(write("extra.txt", "x\n").ok);
    lab.restore("p0").unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
        "one\n"
    );
    assert!(!dir.path().join("extra.txt").exists());
    assert!(!dir.path().join(".harness/restore.partial").exists());
    // Bonsai put the file back itself, so it may edit it without a reread.
    let args =
        serde_json::json!({"tool": "edit_file", "path": "notes.txt", "old": "one", "new": "three"});
    let out = lab.execute(&call(&args.to_string()));
    assert!(out.ok, "{}", out.text);
}

#[test]
fn read_symbol_counts_as_reading_the_file() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let lab = PythonLab::new(
        dir.path().into(),
        Arc::new(LocalPython),
        Duration::from_secs(5),
    );
    std::fs::write(dir.path().join("main.py"), "def f():\n    return 1\n").unwrap();
    let read = serde_json::json!({"tool": "read_symbol", "path": "main.py", "name": "f"});
    assert!(lab.execute(&call(&read.to_string())).ok);
    // The person saves another version: Bonsai must read it again.
    std::fs::write(dir.path().join("main.py"), "def f():\n    return 2\n").unwrap();
    let write = serde_json::json!({"tool": "write_file", "path": "main.py", "content": "x = 1\n"});
    let out = lab.execute(&call(&write.to_string()));
    assert!(!out.ok, "{}", out.text);
}

#[test]
fn a_forgiving_edit_refuses_a_tab_after_an_indent_between_tab_stops() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let lab = PythonLab::new(
        dir.path().into(),
        Arc::new(LocalPython),
        Duration::from_secs(5),
    );
    let text = "if x:\n\t  value = 'a'\n";
    std::fs::write(dir.path().join("t.py"), text).unwrap();
    let outcome = edit(&lab, "t.py", "\tvalue = 'a'", "\tif y:\n\t\tvalue = 'b'");
    assert!(!outcome.ok, "{}", outcome.text);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("t.py")).unwrap(),
        text
    );
}

// MARK: Tests first (C0: the person checks the tests before the code)

fn write_call(path: &str, content: &str) -> String {
    serde_json::json!({"thought": "Write it.", "tool": "write_file", "path": path, "content": content})
        .to_string()
}

#[test]
fn tests_first_waits_for_the_person_then_the_code_must_pass_their_tests_unchanged() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let h = harness(&dir);
    // Bonsai's test expects the wrong thing (two marks, the request says one).
    let wrong = "import unittest\nfrom shout import shout\n\n\nclass T(unittest.TestCase):\n    def test_shout(self):\n        self.assertEqual(shout(\"hi\"), \"HI!!\")\n";
    let fixed = wrong.replace("HI!!", "HI!");
    let tests = write_call(plan::TESTS_FILE, wrong);
    h.start_tests_first(
        scripted(&[&tests, FINISH_CALL]),
        "Write shout.py with shout(s): s in capitals with one exclamation mark.".into(),
        SCRATCH_PROJECT,
    )
    .unwrap();
    let plan = wait(&h).plan.unwrap();
    assert_eq!(plan.status, PlanState::AwaitingApproval, "{plan:?}");
    assert_eq!(plan.thought, plan::TESTS_FIRST_THOUGHT);
    assert_eq!(plan.tasks[0].status, TaskStatus::Completed, "{plan:?}");
    assert_eq!(plan.tasks[1].status, TaskStatus::Ready);
    assert_eq!(plan.tests_file.as_deref(), Some(plan::TESTS_FILE));
    assert_eq!(plan.tests_to_check.as_deref(), Some(wrong));
    assert!(plan.can_continue);

    // The person fixes the test in Python Lab; the screen shows the edit.
    let root = std::path::PathBuf::from(h.project_folder().unwrap());
    std::fs::write(root.join(plan::TESTS_FILE), &fixed).unwrap();
    assert_eq!(
        h.snapshot().plan.unwrap().tests_to_check.as_deref(),
        Some(fixed.as_str())
    );

    // Approved: the code must pass the tests as the person left them.
    let code = write_call("shout.py", "def shout(s):\n    return s.upper() + \"!\"\n");
    let replies = [code.as_str(), RUN_TESTS, FINISH_CALL];
    h.continue_brain(|_, _| scripted(&replies)).unwrap();
    let plan = wait(&h).plan.unwrap();
    assert_eq!(plan.status, PlanState::Completed, "{plan:?}");
    assert_eq!(plan.tests_to_check, None);
    assert!(
        plan.tasks[1]
            .acceptance
            .contains(&format!("{} is left unchanged", plan::TESTS_FILE)),
        "{:?}",
        plan.tasks[1].acceptance
    );
    assert_eq!(
        std::fs::read_to_string(root.join(plan::TESTS_FILE)).unwrap(),
        fixed
    );
}

#[test]
fn tests_first_code_that_changes_the_approved_tests_is_not_verified() {
    if !has_python() {
        return;
    }
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let h = harness(&dir);
    let tests = "import unittest\nfrom shout import shout\n\n\nclass T(unittest.TestCase):\n    def test_shout(self):\n        self.assertEqual(shout(\"hi\"), \"HI!\")\n";
    let write_tests = write_call(plan::TESTS_FILE, tests);
    h.start_tests_first(
        scripted(&[&write_tests, FINISH_CALL]),
        "Write shout.py with shout(s): s in capitals with one exclamation mark.".into(),
        SCRATCH_PROJECT,
    )
    .unwrap();
    assert_eq!(wait(&h).plan.unwrap().status, PlanState::AwaitingApproval);

    // Bonsai bends the test to its wrong code instead of fixing the code.
    let bent = write_call(plan::TESTS_FILE, &tests.replace("HI!", "hi"));
    let code = write_call("shout.py", "def shout(s):\n    return s\n");
    let give_up = r#"{"thought": "t", "tool": "give_up", "reason": "can't"}"#;
    let blocked = r#"{"thought": "t", "decision": "blocked", "question": "Help?"}"#;
    let replies = [
        bent.as_str(),
        code.as_str(),
        RUN_TESTS,
        FINISH_CALL,
        give_up,
        blocked,
    ];
    h.continue_brain(|_, _| scripted(&replies)).unwrap();
    let snapshot = wait(&h);
    let task = snapshot.task.unwrap();
    assert!(
        task.result
            .as_deref()
            .is_some_and(|r| r.contains("give") || r.contains("gave")),
        "{task:?}"
    );
    let steps = snapshot.steps;
    assert!(
        steps.iter().any(|s| s
            .result
            .contains(&format!("{} was changed", plan::TESTS_FILE))),
        "{steps:?}"
    );
    // The rollback before Recovery keeps the person's approved tests.
    let root = std::path::PathBuf::from(h.project_folder().unwrap());
    assert_eq!(
        std::fs::read_to_string(root.join(plan::TESTS_FILE)).unwrap(),
        tests
    );
}

#[test]
fn tests_written_needs_a_unittest_test() {
    let check = Criterion::TestsWritten {
        path: "test_app.py".into(),
    };
    let none = |_: &str| None;
    assert!(check.check(&[], &none).unwrap_err().contains("missing"));
    let empty = |_: &str| Some("import unittest\n".to_string());
    assert!(check.check(&[], &empty).is_err());
    let two = |_: &str| {
        Some("import unittest\nclass T(unittest.TestCase):\n    def test_a(self): pass\n    def test_b(self): pass\n".to_string())
    };
    assert_eq!(check.check(&[], &two).unwrap(), "test_app.py has 2 tests");
}

#[test]
fn a_tests_first_plan_keeps_its_review_through_replans_and_failures() {
    let plan_id = "plan-1".to_string();
    let mut entries = vec![
        Entry::PlanRequest {
            plan: plan_id.clone(),
            request: "r".into(),
            criteria: Vec::new(),
            model: "m".into(),
            at_ms: 0,
        },
        Entry::TestsFirst {
            plan: plan_id.clone(),
            file: plan::TESTS_FILE.into(),
        },
        Entry::Plan {
            plan: plan_id.clone(),
            thought: "t".into(),
            tasks: plan::tests_first_tasks(plan::TESTS_FILE),
            model_ms: 0,
        },
        // A replan that leaves the tests out.
        Entry::Plan {
            plan: plan_id.clone(),
            thought: "t".into(),
            tasks: vec![plan::PlanTask {
                id: "code".into(),
                description: "Just code.".into(),
                after: Vec::new(),
                criteria: vec![Criterion::WorkRuns],
            }],
            model_ms: 1,
        },
    ];
    // A replan whose tests task waits for another task and checks less.
    entries.push(Entry::Plan {
        plan: plan_id.clone(),
        thought: "t".into(),
        tasks: vec![
            plan::PlanTask {
                id: "code".into(),
                description: "Just code.".into(),
                after: Vec::new(),
                criteria: vec![Criterion::WorkRuns],
            },
            plan::PlanTask {
                id: plan::TESTS_TASK.into(),
                description: "Tests later.".into(),
                after: vec!["code".into()],
                criteria: vec![Criterion::WorkRuns],
            },
        ],
        model_ms: 2,
    });
    let view = plan::PlanView::read(&entries).unwrap();
    // It also moves first, so restart_point() reads the tasks in run order.
    let tests = &view.tasks[0];
    assert_eq!(tests.id, plan::TESTS_TASK);
    assert!(tests.after.is_empty());
    assert_eq!(
        tests.criteria,
        vec![Criterion::TestsWritten {
            path: plan::TESTS_FILE.into()
        }]
    );
    assert_eq!(view.next().unwrap().id, plan::TESTS_TASK);
    entries.pop();
    let view = plan::PlanView::read(&entries).unwrap();
    assert_eq!(view.tasks[0].id, plan::TESTS_TASK);
    assert_eq!(view.next().unwrap().id, plan::TESTS_TASK);
    assert!(view.tasks[1].after.contains(&plan::TESTS_TASK.to_string()));

    // The tests task passes, then the approval fails and the plan fails:
    // it still waits for the review.
    entries.push(Entry::Task {
        task: "task-1".into(),
        description: "d".into(),
        criteria: Vec::new(),
        model: "m".into(),
        at_ms: 0,
        plan: Some(plan_id.clone()),
        plan_task: Some(plan::TESTS_TASK.into()),
    });
    entries.push(Entry::Status {
        task: "task-1".into(),
        status: TaskStatus::Completed,
        reason: None,
        at_ms: 0,
    });
    entries.push(Entry::PlanStatus {
        plan: plan_id.clone(),
        status: PlanState::Failed,
        reason: Some("Could not save a checkpoint".into()),
        at_ms: 0,
    });
    let view = plan::PlanView::read(&entries).unwrap();
    assert!(view.awaiting_test_review());
    assert!(view.can_continue());

    // Approved: the request itself needs the tests passing, unchanged.
    entries.push(Entry::TestsApproved {
        plan: plan_id.clone(),
        file: plan::TESTS_FILE.into(),
        content: "tests".into(),
        at_ms: 0,
    });
    let view = plan::PlanView::read(&entries).unwrap();
    assert!(!view.awaiting_test_review());
    assert!(view.criteria.contains(&Criterion::TestsPass));
    assert!(view.criteria.contains(&Criterion::FileUnchanged {
        path: plan::TESTS_FILE.into(),
        content: "tests".into(),
    }));

    // The tests stay first after approval, so once the code passes a
    // resume starts from the code's checkpoint, not the tests'.
    assert_eq!(view.tasks[0].id, plan::TESTS_TASK);
    entries.push(Entry::Task {
        task: "task-2".into(),
        description: "d".into(),
        criteria: Vec::new(),
        model: "m".into(),
        at_ms: 0,
        plan: Some(plan_id),
        plan_task: Some("code".into()),
    });
    entries.push(Entry::Status {
        task: "task-2".into(),
        status: TaskStatus::Completed,
        reason: None,
        at_ms: 0,
    });
    let view = plan::PlanView::read(&entries).unwrap();
    assert_eq!(view.restart_point(), "task-2");
}

#[test]
fn tests_written_ignores_a_test_inside_a_string() {
    let check = Criterion::TestsWritten {
        path: "test_app.py".into(),
    };
    let fake = |_: &str| {
        Some("import unittest\nexample = \"\"\"\ndef test_x(self): pass\n\"\"\"\n".to_string())
    };
    assert!(check.check(&[], &fake).is_err());
}
