//! Planner mode (H4): the request becomes a plan of small tasks, each with
//! the tasks it needs first and the checks that prove it done, chosen from
//! the harness's list. The plan is one JSON object, shaped by a grammar
//! like the executor's calls, and checked again here (ids, order, sizes).
//! The harness then runs ready tasks one at a time, each with a fresh
//! context, and the journal is the plan's state.

use std::collections::{BTreeMap, BTreeSet};

use forge_services::ai::ChatMessage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::calls::{check_path, common_rules};
use super::evidence::Criterion;
use super::journal::{Entry, TaskStatus};

/// The most tasks a plan may have.
pub const MAX_TASKS: usize = 6;

/// The longest task id.
pub const ID_MAX: usize = 16;

/// The longest task description, in characters.
pub const DO_MAX: usize = 400;

/// One task of a plan.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlanTask {
    pub id: String,
    /// What to do, naming the files and functions.
    pub description: String,
    /// Tasks that must be completed first (always earlier in the plan).
    pub after: Vec<String>,
    pub criteria: Vec<Criterion>,
}

/// A plan's state, owned by the harness.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanState {
    /// The model is writing the plan.
    Planning,
    /// Shown before anything changes; runs once the person approves it.
    AwaitingApproval,
    Running,
    /// Every task was verified, and so was the request's own check.
    Completed,
    Failed,
    Stopped,
    /// The person didn't approve it.
    Discarded,
    /// Recovery mode found the task can't go on without the person: the
    /// reason is the question, answered on screen.
    Blocked,
}

impl PlanState {
    pub fn is_final(self) -> bool {
        matches!(
            self,
            PlanState::Completed | PlanState::Failed | PlanState::Stopped | PlanState::Discarded
        )
    }

    /// A plan that can go on: approved, or resumed from its last verified
    /// task.
    pub fn needs_answer(self) -> bool {
        self == PlanState::Blocked
    }

    pub fn can_continue(self) -> bool {
        matches!(
            self,
            PlanState::AwaitingApproval | PlanState::Failed | PlanState::Stopped
        )
    }
}

/// A tests-first request (the 1060 lab's C0 benchmark, 2026-11-25): with
/// tests it worked out itself, Bonsai 9B wrote a wrong test in about two
/// tasks of three, then "fixed" correct code against it; with correct
/// tests it wrote passing code. So Bonsai writes the tests, the person
/// checks (and may edit) them, and only then does Bonsai write the code,
/// which must leave the tests as the person approved them.
pub const TESTS_TASK: &str = "tests";
pub const CODE_TASK: &str = "code";
/// Where a tests-first request's tests go.
pub const TESTS_FILE: &str = "test_app.py";
pub const TESTS_FIRST_THOUGHT: &str =
    "Tests first: Bonsai writes the tests, you check them, then Bonsai writes the code.";
/// Shown while the tests wait for the person.
pub const TESTS_REVIEW_NOTE: &str = "Check the tests before Bonsai writes the code. If a test \
     expects the wrong thing, fix it in Python Lab first: the code must pass the tests as you \
     approve them.";

/// The two tasks of a tests-first request, with the tests in `file`.
pub fn tests_first_tasks(file: &str) -> Vec<PlanTask> {
    vec![
        PlanTask {
            id: TESTS_TASK.into(),
            description: format!(
                "Write only the tests, in {file}: a unittest.TestCase that imports the module \
                 the request describes and checks what the request states, with small examples \
                 whose expected values you worked out by hand. Don't write the module itself: \
                 the next task writes it, after the person has checked your tests."
            ),
            after: Vec::new(),
            criteria: vec![Criterion::TestsWritten { path: file.into() }],
        },
        PlanTask {
            id: CODE_TASK.into(),
            description: format!(
                "Write the code the request asks for, so the tests in {file} pass. The person \
                 checked those tests: read them first, and don't change {file}."
            ),
            after: vec![TESTS_TASK.into()],
            criteria: vec![Criterion::TestsPass],
        },
    ]
}

/// The most recoveries one task gets before the plan fails.
pub const MAX_RECOVERIES: u32 = 2;

/// What Recovery mode decided for a failed task (H5).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    /// Run the task again, told what went wrong.
    Retry { diagnosis: String },
    /// Run it again as a changed task.
    Revise {
        description: String,
        criteria: Vec<Criterion>,
    },
    /// The plan itself was wrong: write a new one for what is left.
    Replan { diagnosis: String },
    /// Only the person can unblock it: ask them.
    Blocked { question: String },
}

/// Recovery mode's instructions.
pub fn recovery_instructions() -> String {
    "You are Bonsai's recovery step. A task of a plan failed, and the project's files are back as \
     they were before it started. From the evidence, decide one thing:\n\
     - retry: the task is right, the work went wrong. Give a diagnosis: what to do differently.\n\
     - revise: the task asked for something wrong or impossible to check. Give the task as it \
     should be, with its checks.\n\
     - replan: the plan missed something the task needs (a file or function that doesn't exist). \
     Say what is missing; the planner writes a new plan for what is left.\n\
     - blocked: only the person can help (it needs the network, a file they must give, a choice \
     only they can make). Ask them one clear question.\n\
     Never retry the same thing that failed. Answer with one JSON object, like these:\n\
     {\"thought\": \"The test expects a rounded value.\", \"decision\": \"retry\", \"diagnosis\": \"Round the mean to 2 decimals before returning it.\"}\n\
     {\"thought\": \"report.py imports stats, which no task makes.\", \"decision\": \"replan\", \"diagnosis\": \"stats.py with mean() is missing.\"}\n\
     {\"thought\": \"Python has no network here.\", \"decision\": \"blocked\", \"question\": \"Python can't reach the internet on this phone. Should I use sample data instead?\"}"
        .into()
}

/// The GBNF grammar a recovery decision must follow.
pub fn recovery_grammar() -> String {
    let mut text = String::from(
        "root ::= \"{\\\"thought\\\": \" thought \", \\\"decision\\\": \" decision \"}\"\n",
    );
    text += "decision ::= \"\\\"retry\\\", \\\"diagnosis\\\": \" do | \"\\\"revise\\\", \\\"do\\\": \" do \", \\\"checks\\\": [\" checks \"]\" | \"\\\"replan\\\", \\\"diagnosis\\\": \" do | \"\\\"blocked\\\", \\\"question\\\": \" do\n";
    text + &plan_rules()
}

/// The request for a recovery: the failed task, why it failed, its last
/// results, and the project as it is now (rolled back).
pub fn recovery_messages(
    view: &PlanView,
    task: &PlanTask,
    reason: &str,
    recent: &str,
    overview: &str,
    refused: &[(String, String)],
) -> Vec<ChatMessage> {
    let checks = task
        .criteria
        .iter()
        .map(Criterion::describe)
        .collect::<Vec<_>>()
        .join("; ");
    let mut user = format!(
        "Request: {}\n\nFailed task {}: {}\nDone when: {checks}.\nWhy it failed: {reason}\n",
        view.request, task.id, task.description
    );
    if !recent.is_empty() {
        user += &format!("\nIts last results:\n{recent}\n");
    }
    user += &format!("\nProject files now (rolled back):\n{overview}");
    let mut messages = vec![
        ChatMessage::system(recovery_instructions()),
        ChatMessage::user(user),
    ];
    for (text, error) in refused {
        // cut: the model's refused decision, quoted back to it
        let shown: String = text.chars().take(800).collect();
        messages.push(ChatMessage::assistant(shown));
        messages.push(ChatMessage::user(format!(
            "That can't be used: {error} Answer with one corrected decision."
        )));
    }
    messages
}

/// Parses and checks a recovery decision for task `id`.
pub fn parse_recovery(text: &str, id: &str) -> Result<(String, Decision), String> {
    let value: Value = serde_json::from_str(text.trim())
        .map_err(|err| format!("The answer was not one JSON object ({err})."))?;
    let thought = value
        .get("thought")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let field = |name: &str| -> Result<String, String> {
        let text = value
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        if text.is_empty() {
            return Err(format!("The decision needs \"{name}\"."));
        }
        if text.chars().count() > DO_MAX {
            return Err(format!(
                "\"{name}\" is too long (at most {DO_MAX} characters)."
            ));
        }
        Ok(text.to_string())
    };
    let decision = match value.get("decision").and_then(Value::as_str) {
        Some("retry") => Decision::Retry {
            diagnosis: field("diagnosis")?,
        },
        Some("revise") => Decision::Revise {
            description: description(id, &value)?,
            criteria: checks(id, &value)?,
        },
        Some("replan") => Decision::Replan {
            diagnosis: field("diagnosis")?,
        },
        Some("blocked") => Decision::Blocked {
            question: field("question")?,
        },
        Some(other) => return Err(format!("There is no decision called {other:?}.")),
        None => return Err("The answer has no \"decision\".".into()),
    };
    Ok((thought, decision))
}

/// The planner's instructions: the shape of a plan and the checks it may
/// choose.
pub fn instructions() -> String {
    format!(
        "You are Bonsai's planner. A programmer will do the request as a few small tasks, one \
         after another, in a Python project on an iPhone (Python 3 and its standard library only: \
         no network, no pip, no input()). The programmer sees one task at a time and a line on \
         each task done before it, so every task must say exactly what to make: file names, \
         function names, what they return or print.\n\n\
         Answer with one JSON object: a short thought, then the tasks. Each task has an id (short, \
         lower case), what to do, the ids of earlier tasks it needs (\"after\"), and the checks \
         that prove it done.\n\n\
         Checks:\n\
         - {{\"check\": \"tests_pass\"}}: the tests pass (unittest, in files named test_*.py)\n\
         - {{\"check\": \"run_prints\", \"path\": \"main.py\", \"text\": \"15\"}}: running main.py prints 15\n\
         - {{\"check\": \"work_runs\"}}: the task's last run or test run succeeded\n\
         - {{\"check\": \"makes_image\", \"path\": \"chart.svg\", \"format\": \"svg\"}}: running it makes chart.svg\n\n\
         Rules:\n\
         - One to {MAX_TASKS} tasks, each small enough to finish in about ten steps.\n\
         - A task can only come after tasks listed before it.\n\
         - A task that writes tests also runs them and checks tests_pass.\n\
         - If the request already says how to split it, follow it.\n\n\
         Example:\n{}",
        example()
    )
}

/// An example plan in the grammar's exact shape.
pub fn example() -> String {
    "{\"thought\": \"The module first, then the program that uses it.\", \"tasks\": [\
     {\"id\": \"area\", \"do\": \"Write shapes.py with area(w, h) returning w * h, and test_shapes.py \
     testing it with unittest. Run the tests.\", \"after\": [], \"checks\": [{\"check\": \"tests_pass\"}]}, \
     {\"id\": \"main\", \"do\": \"Write main.py that imports shapes and prints area(3, 5). Run main.py.\", \
     \"after\": [\"area\"], \"checks\": [{\"check\": \"run_prints\", \"path\": \"main.py\", \"text\": \"15\"}]}]}"
        .into()
}

/// The GBNF grammar a plan must follow.
pub fn grammar() -> String {
    let more = MAX_TASKS - 1;
    let mut text = format!(
        "root ::= \"{{\\\"thought\\\": \" thought \", \\\"tasks\\\": [\" task (\", \" task){{0,{more}}} \"]}}\"\n"
    );
    text += "task ::= \"{\\\"id\\\": \" id \", \\\"do\\\": \" do \", \\\"after\\\": [\" (id (\", \" id)*)? \"], \\\"checks\\\": [\" checks \"]}\"\n";
    text + &plan_rules()
}

/// The rules plans and recoveries share: checks, ids, what to do.
fn plan_rules() -> String {
    let mut text = String::from("checks ::= check (\", \" check){0,3}\n");
    text += "check ::= \"{\\\"check\\\": \\\"tests_pass\\\"}\" | \"{\\\"check\\\": \\\"work_runs\\\"}\" | \"{\\\"check\\\": \\\"run_prints\\\", \\\"path\\\": \" path \", \\\"text\\\": \" string \"}\" | \"{\\\"check\\\": \\\"makes_image\\\", \\\"path\\\": \" path \", \\\"format\\\": \" format \"}\"\n";
    text += "format ::= \"\\\"svg\\\"\" | \"\\\"png\\\"\"\n";
    text += &format!(
        "id ::= \"\\\"\" [a-z] [a-z0-9-]{{0,{}}} \"\\\"\"\n",
        ID_MAX - 1
    );
    text += &format!("do ::= \"\\\"\" char{{1,{DO_MAX}}} \"\\\"\"\n");
    text + &common_rules()
}

/// The request for a plan: the instructions, the request and the project
/// as it is, what happened so far when replanning, then each refused plan
/// and why.
pub fn messages(
    request: &str,
    overview: &str,
    earlier: &str,
    progress: Option<&str>,
    refused: &[(String, String)],
) -> Vec<ChatMessage> {
    let mut user = format!("Request: {request}\n\nProject files now:\n{overview}");
    if !earlier.is_empty() {
        user += &format!("\n\nEarlier work in this project:\n{earlier}");
    }
    if let Some(progress) = progress {
        user += &format!(
            "\n\n{progress}\nWrite a new plan for what is left. Don't repeat the tasks done (their \
             ids may appear in \"after\") and don't reuse their ids."
        );
    }
    let mut messages = vec![ChatMessage::system(instructions()), ChatMessage::user(user)];
    for (text, error) in refused {
        // cut: the model's refused plan, quoted back to it
        let shown: String = text.chars().take(1200).collect();
        messages.push(ChatMessage::assistant(shown));
        messages.push(ChatMessage::user(format!(
            "That plan can't be used: {error} Answer with one corrected plan."
        )));
    }
    messages
}

/// Parses and checks a plan: the thought and its tasks. `done` are the ids
/// of tasks already verified (a replan may come after them, never reuse
/// them).
pub fn parse(text: &str, done: &[String]) -> Result<(String, Vec<PlanTask>), String> {
    let value: Value = serde_json::from_str(text.trim())
        .map_err(|err| format!("The answer was not one JSON object ({err})."))?;
    let thought = value
        .get("thought")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let Some(Value::Array(items)) = value.get("tasks") else {
        return Err("The plan has no \"tasks\" list.".into());
    };
    if items.is_empty() {
        return Err("The plan has no tasks.".into());
    }
    // A replan's tasks join the ones already done, under the same limit.
    let room = MAX_TASKS.saturating_sub(done.len()).max(1);
    if items.len() > room {
        return Err(format!(
            "The plan has {} tasks; use at most {room}.",
            items.len()
        ));
    }
    let mut tasks: Vec<PlanTask> = Vec::new();
    for item in items {
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .ok_or("Every task needs an \"id\".")?
            .to_string();
        let id_ok = !id.is_empty()
            && id.len() <= ID_MAX
            && id.starts_with(|c: char| c.is_ascii_lowercase())
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !id_ok {
            return Err(format!(
                "{id:?} is not a task id (lower case letters, digits and -, at most {ID_MAX})."
            ));
        }
        if tasks.iter().any(|t| t.id == id) {
            return Err(format!("Two tasks are called {id:?}."));
        }
        if done.contains(&id) {
            return Err(format!(
                "{id:?} is a task already done; give the new task another id."
            ));
        }
        let description = description(&id, item)?;
        let mut after = Vec::new();
        for need in item
            .get("after")
            .and_then(Value::as_array)
            .ok_or(format!("Task {id:?} needs an \"after\" list."))?
        {
            let need = need
                .as_str()
                .ok_or(format!("Task {id:?}: \"after\" lists task ids."))?;
            if need == id {
                return Err(format!("Task {id:?} can't come after itself."));
            }
            if !tasks.iter().any(|t| t.id == need) && !done.iter().any(|d| d == need) {
                return Err(format!(
                    "Task {id:?} comes after {need:?}, which is not an earlier task."
                ));
            }
            if !after.iter().any(|a: &String| a == need) {
                after.push(need.to_string());
            }
        }
        let criteria = checks(&id, item)?;
        tasks.push(PlanTask {
            id,
            description,
            after,
            criteria,
        });
    }
    Ok((thought, tasks))
}

fn description(id: &str, item: &Value) -> Result<String, String> {
    let description = item
        .get("do")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if description.is_empty() {
        return Err(format!("Task {id:?} doesn't say what to do."));
    }
    if description.chars().count() > DO_MAX {
        return Err(format!(
            "Task {id:?} is too long; say it in at most {DO_MAX} characters."
        ));
    }
    Ok(description.to_string())
}

fn checks(id: &str, item: &Value) -> Result<Vec<Criterion>, String> {
    let checks = item
        .get("checks")
        .and_then(Value::as_array)
        .filter(|c| !c.is_empty())
        .ok_or(format!("Task {id:?} needs at least one check."))?;
    if checks.len() > 4 {
        return Err(format!("Task {id:?} has more than 4 checks."));
    }
    checks.iter().map(|check| criterion(id, check)).collect()
}

fn criterion(id: &str, check: &Value) -> Result<Criterion, String> {
    let field = |name: &str| check.get(name).and_then(Value::as_str).unwrap_or("");
    let path = || -> Result<String, String> {
        let path = field("path");
        check_path(path).map_err(|err| format!("Task {id:?}: {err}"))?;
        Ok(path.to_string())
    };
    match field("check") {
        "tests_pass" => Ok(Criterion::TestsPass),
        "work_runs" => Ok(Criterion::WorkRuns),
        "run_prints" => {
            let text = field("text");
            if text.trim().is_empty() {
                return Err(format!(
                    "Task {id:?}: run_prints needs the text the run prints."
                ));
            }
            Ok(Criterion::RunPrints {
                path: path()?,
                text: text.to_string(),
            })
        }
        "makes_image" => {
            let format = field("format");
            if !["svg", "png"].contains(&format) {
                return Err(format!("Task {id:?}: makes_image is svg or png."));
            }
            Ok(Criterion::ProducesImage {
                path: path()?,
                format: format.to_string(),
                min_width: 0,
            })
        }
        other => Err(format!("Task {id:?}: there is no check called {other:?}.")),
    }
}

/// A plan's entries, from its `PlanRequest` on: the current plan, when no
/// task outside a plan came after it.
pub fn current(entries: &[Entry]) -> Option<&[Entry]> {
    let start = entries
        .iter()
        .rposition(|e| matches!(e, Entry::PlanRequest { .. }))?;
    let later_alone = entries[start..]
        .iter()
        .any(|e| matches!(e, Entry::Task { plan: None, .. }));
    (!later_alone).then(|| &entries[start..])
}

/// What the journal says about a plan.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanView {
    pub id: String,
    pub request: String,
    /// The request's own checks, made once every task is verified.
    pub criteria: Vec<Criterion>,
    pub model: String,
    pub state: PlanState,
    pub reason: Option<String>,
    pub thought: String,
    /// The plan as it stands: revisions applied, and after a replan the
    /// tasks done before it followed by the new ones.
    pub tasks: Vec<PlanTask>,
    /// Each plan task's last executor task: its journal id, status and the
    /// reason or evidence.
    pub runs: BTreeMap<String, (String, TaskStatus, Option<String>)>,
    /// What the executor is told on a task's next try: a diagnosis, the
    /// person's answer.
    pub notes: BTreeMap<String, Vec<String>>,
    /// Recoveries each task has had.
    pub recoveries: BTreeMap<String, u32>,
    /// The task Recovery mode blocked on, and its question.
    pub blocked: Option<(String, String)>,
    /// A replan Recovery mode chose whose new tasks aren't written yet
    /// (the app closed between the two): the task and the diagnosis.
    pub replan: Option<(String, String)>,
    /// Whether it ever ran (its checkpoint was taken): a plan that didn't
    /// starts afresh, with no files to put back.
    pub started: bool,
    /// A tests-first request's test file.
    pub tests_file: Option<String>,
    /// The tests as the person approved them.
    pub tests_approved: Option<String>,
    /// Model answers the Scripted model has used up when the plan goes
    /// on: all of them, except a task's that ended unverified with no
    /// recovery after it (resuming runs that task again from scratch).
    pub resume_turns: usize,
}

impl PlanView {
    pub fn read(plan: &[Entry]) -> Option<Self> {
        let Some(Entry::PlanRequest {
            plan: id,
            request,
            criteria,
            model,
            ..
        }) = plan.first()
        else {
            return None;
        };
        let mut view = PlanView {
            id: id.clone(),
            request: request.clone(),
            criteria: criteria.clone(),
            model: model.clone(),
            state: PlanState::Planning,
            reason: None,
            thought: String::new(),
            tasks: Vec::new(),
            runs: BTreeMap::new(),
            notes: BTreeMap::new(),
            recoveries: BTreeMap::new(),
            blocked: None,
            replan: None,
            started: false,
            tests_file: None,
            tests_approved: None,
            resume_turns: 0,
        };
        // Executor task id -> plan task id.
        let mut owner: BTreeMap<String, String> = BTreeMap::new();
        // The last executor task and its answers since, until a recovery.
        let mut open_turns = 0usize;
        for entry in plan {
            match entry {
                Entry::Plan { thought, tasks, .. } => {
                    view.thought = thought.clone();
                    // A replan keeps the tasks already done, in order.
                    let done = view.completed_owned();
                    // A task that isn't done is replaced, even when the new
                    // plan reuses its id: its notes and last run go with it.
                    // Its recovery count stays, so replans can't go on forever.
                    for task in &view.tasks {
                        if !done.contains(&task.id) {
                            view.notes.remove(&task.id);
                            view.runs.remove(&task.id);
                        }
                    }
                    let mut kept: Vec<PlanTask> = view
                        .tasks
                        .iter()
                        .filter(|t| done.contains(&t.id))
                        .cloned()
                        .collect();
                    kept.extend(tasks.iter().cloned());
                    // The harness writes a tests-first request's plan
                    // itself: no model answer to count.
                    let fixed = view.tests_file.is_some() && view.tasks.is_empty();
                    view.tasks = kept;
                    view.replan = None;
                    if !fixed {
                        view.resume_turns += 1;
                    }
                }
                Entry::TestsFirst { file, .. } => view.tests_file = Some(file.clone()),
                Entry::TestsApproved { content, .. } => {
                    view.tests_approved = Some(content.clone());
                }
                Entry::PlanInvalid { .. } | Entry::RecoveryInvalid { .. } => view.resume_turns += 1,
                Entry::Recovery {
                    plan_task,
                    decision,
                    ..
                } => {
                    view.resume_turns += 1 + open_turns;
                    open_turns = 0;
                    *view.recoveries.entry(plan_task.clone()).or_default() += 1;
                    match decision {
                        Decision::Retry { diagnosis } => view
                            .notes
                            .entry(plan_task.clone())
                            .or_default()
                            .push(format!("An earlier try failed. Diagnosis: {diagnosis}")),
                        Decision::Revise {
                            description,
                            criteria,
                        } => {
                            if let Some(task) = view.tasks.iter_mut().find(|t| &t.id == plan_task) {
                                task.description = description.clone();
                                task.criteria = criteria.clone();
                            }
                        }
                        Decision::Replan { diagnosis } => {
                            view.replan = Some((plan_task.clone(), diagnosis.clone()));
                        }
                        Decision::Blocked { question } => {
                            view.blocked = Some((plan_task.clone(), question.clone()));
                            // Blocked from the decision itself, so a status
                            // write that failed after it still asks.
                            view.state = PlanState::Blocked;
                            view.reason = Some(question.clone());
                        }
                    }
                }
                Entry::PersonAnswer {
                    plan_task, text, ..
                } => {
                    let question = view.blocked.take().map(|(_, q)| q).unwrap_or_default();
                    view.notes
                        .entry(plan_task.clone())
                        .or_default()
                        .push(format!(
                            "You asked the person: {question} They answered: {text}"
                        ));
                }
                Entry::Answer { .. } | Entry::Invalid { .. } => open_turns += 1,
                Entry::PlanStatus { status, reason, .. } => {
                    view.started |= *status == PlanState::Running;
                    view.state = *status;
                    view.reason = reason.clone();
                }
                Entry::Task {
                    task,
                    plan_task: Some(part),
                    ..
                } => {
                    owner.insert(task.clone(), part.clone());
                    // A new try: an earlier one's answers, unfinished and
                    // never recovered from, are given again.
                    open_turns = 0;
                    view.runs
                        .insert(part.clone(), (task.clone(), TaskStatus::Ready, None));
                }
                Entry::Status {
                    task,
                    status,
                    reason,
                    ..
                } => {
                    if let Some(part) = owner.get(task) {
                        view.runs
                            .insert(part.clone(), (task.clone(), *status, reason.clone()));
                        if *status == TaskStatus::Completed {
                            view.resume_turns += open_turns;
                            open_turns = 0;
                        }
                    }
                }
                _ => {}
            }
        }
        // Until the person approves the tests, a tests-first plan keeps its
        // tests task first and every other task after it, whatever a
        // replan wrote, so nothing can run past the review.
        if let (Some(file), None) = (view.tests_file.clone(), &view.tests_approved) {
            // The tests task keeps its own checks and waits for nothing
            // (a revision or replan can't swap them, or make a cycle).
            let tests = tests_first_tasks(&file).remove(0);
            // It also moves first, so the task list stays in the order the
            // tasks run (restart_point() reads it that way).
            match view.tasks.iter().position(|t| t.id == TESTS_TASK) {
                Some(index) => {
                    let mut task = view.tasks.remove(index);
                    task.criteria = tests.criteria;
                    task.after.clear();
                    view.tasks.insert(0, task);
                }
                None => view.tasks.insert(0, tests),
            }
            for task in view.tasks.iter_mut().filter(|t| t.id != TESTS_TASK) {
                if !task.after.iter().any(|a| a == TESTS_TASK) {
                    task.after.push(TESTS_TASK.into());
                }
            }
        } else if view.tests_file.is_some() {
            // Approved: the tests ran first, so they stay first (put back
            // if a replan left them out), and restart_point() picks their
            // checkpoint only until a later task is verified: never one
            // that rolls back the tests or a later task's work.
            let task = match view.tasks.iter().position(|t| t.id == TESTS_TASK) {
                Some(index) => view.tasks.remove(index),
                None => {
                    tests_first_tasks(view.tests_file.as_deref().unwrap_or(TESTS_FILE)).remove(0)
                }
            };
            view.tasks.insert(0, task);
        }
        // Approved tests bind every task after the tests task, and the
        // request itself, whatever a revision or replan wrote since.
        if let (Some(file), Some(content)) = (view.tests_file.clone(), view.tests_approved.clone())
        {
            let unchanged = Criterion::FileUnchanged {
                path: file.clone(),
                content: content.clone(),
            };
            for check in [Criterion::TestsPass, unchanged] {
                if !view.criteria.contains(&check) {
                    view.criteria.push(check);
                }
            }
            for task in view.tasks.iter_mut().filter(|t| t.id != TESTS_TASK) {
                if !task
                    .criteria
                    .iter()
                    .any(|c| matches!(c, Criterion::FileUnchanged { path, .. } if *path == file))
                {
                    task.criteria.push(Criterion::FileUnchanged {
                        path: file.clone(),
                        content: content.clone(),
                    });
                }
                view.notes.entry(task.id.clone()).or_default().insert(
                    0,
                    format!(
                        "The person checked the tests in {file} and approved them. Read them, \
                         and make them pass without changing {file}."
                    ),
                );
            }
        }
        Some(view)
    }

    /// A tests-first request whose tests are written and verified, waiting
    /// for the person to check them.
    pub fn awaiting_test_review(&self) -> bool {
        self.tests_file.is_some()
            && self.tests_approved.is_none()
            && self.status_of(TESTS_TASK) == TaskStatus::Completed
            // A plan that failed or stopped before the approval was saved
            // still waits for it: Resume approves before anything else.
            && self.state != PlanState::Discarded
    }

    /// Whether it can be approved or resumed: waiting for approval, ended
    /// early, or blocked with its question answered.
    pub fn can_continue(&self) -> bool {
        // An unanswered question blocks the plan whatever its status says:
        // a failed status write can leave it Failed in memory only.
        // A plan that failed with every task done failed its final check:
        // Resume would only run the same checks on the same work, so it
        // ends there.
        let finished = !self.tasks.is_empty() && self.next().is_none();
        self.blocked.is_none()
            && (self.state.can_continue() || self.state == PlanState::Blocked)
            && !(self.state == PlanState::Failed && finished)
    }

    fn completed_owned(&self) -> Vec<String> {
        self.completed().into_iter().map(str::to_string).collect()
    }

    /// The ids of the tasks verified so far.
    pub fn done_ids(&self) -> Vec<String> {
        self.completed_owned()
    }

    /// What happened so far, for a replan.
    pub fn progress(&self, failed: &PlanTask, reason: &str, diagnosis: &str) -> String {
        let mut text = String::from("Done already:\n");
        let done = self.completed();
        for task in self.tasks.iter().filter(|t| done.contains(t.id.as_str())) {
            text += &format!("- {}: {}\n", task.id, task.description);
        }
        if done.is_empty() {
            text += "- nothing yet\n";
        }
        text += &format!(
            "Task {} ({}) failed: {reason}\nWhat is missing: {diagnosis}\n\
             Plan at most {} tasks for what is left.\n",
            failed.id,
            failed.description,
            MAX_TASKS.saturating_sub(done.len()).max(1)
        );
        text
    }

    pub fn status_of(&self, task: &str) -> TaskStatus {
        self.runs
            .get(task)
            .map(|(_, status, _)| *status)
            .unwrap_or(TaskStatus::Ready)
    }

    pub fn completed(&self) -> BTreeSet<&str> {
        self.tasks
            .iter()
            .filter(|t| self.status_of(&t.id) == TaskStatus::Completed)
            .map(|t| t.id.as_str())
            .collect()
    }

    /// The next task to run: the first one not completed whose tasks
    /// before it all are.
    pub fn next(&self) -> Option<&PlanTask> {
        let done = self.completed();
        self.tasks.iter().find(|t| {
            !done.contains(t.id.as_str()) && t.after.iter().all(|a| done.contains(a.as_str()))
        })
    }

    /// The checkpoint the next task starts from: the last verified task's,
    /// or the plan's own (taken before its first task).
    pub fn restart_point(&self) -> String {
        self.tasks
            .iter()
            .rev()
            .find_map(|t| match self.runs.get(&t.id) {
                Some((run, TaskStatus::Completed, _)) => Some(run.clone()),
                _ => None,
            })
            .unwrap_or_else(|| self.id.clone())
    }

    /// The request the executor sees for `task`: what to do, the whole
    /// request for context, and one line per task already done.
    pub fn executor_request(&self, task: &PlanTask) -> String {
        let mut text = format!(
            "{}\n\n(This is one task of a plan for: {})",
            task.description, self.request
        );
        let done: Vec<String> = self
            .tasks
            .iter()
            .filter_map(|t| match self.runs.get(&t.id) {
                Some((_, TaskStatus::Completed, reason)) if t.id != task.id => Some(format!(
                    "- {}: {} {}",
                    t.id,
                    t.description,
                    reason.as_deref().unwrap_or("")
                )),
                _ => None,
            })
            .collect();
        if !done.is_empty() {
            text += &format!("\nDone already:\n{}", done.join("\n"));
        }
        if let Some(notes) = self.notes.get(&task.id) {
            text += &format!("\n{}", notes.join("\n"));
        }
        text
    }
}
