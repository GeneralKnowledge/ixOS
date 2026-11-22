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
    pub fn can_continue(self) -> bool {
        matches!(
            self,
            PlanState::AwaitingApproval | PlanState::Failed | PlanState::Stopped
        )
    }
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
    text += "task ::= \"{\\\"id\\\": \" id \", \\\"do\\\": \" do \", \\\"after\\\": [\" (id (\", \" id)*)? \"], \\\"checks\\\": [\" check (\", \" check){0,3} \"]}\"\n";
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
/// as it is, then each refused plan and why.
pub fn messages(request: &str, overview: &str, refused: &[(String, String)]) -> Vec<ChatMessage> {
    let mut messages = vec![
        ChatMessage::system(instructions()),
        ChatMessage::user(format!(
            "Request: {request}\n\nProject files now:\n{overview}"
        )),
    ];
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

/// Parses and checks a plan: the thought and its tasks.
pub fn parse(text: &str) -> Result<(String, Vec<PlanTask>), String> {
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
    if items.len() > MAX_TASKS {
        return Err(format!(
            "The plan has {} tasks; use at most {MAX_TASKS}.",
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
            if !tasks.iter().any(|t| t.id == need) {
                return Err(format!(
                    "Task {id:?} comes after {need:?}, which is not an earlier task."
                ));
            }
            if !after.iter().any(|a: &String| a == need) {
                after.push(need.to_string());
            }
        }
        let checks = item
            .get("checks")
            .and_then(Value::as_array)
            .filter(|c| !c.is_empty())
            .ok_or(format!("Task {id:?} needs at least one check."))?;
        let mut criteria = Vec::new();
        for check in checks {
            criteria.push(criterion(&id, check)?);
        }
        tasks.push(PlanTask {
            id,
            description: description.to_string(),
            after,
            criteria,
        });
    }
    Ok((thought, tasks))
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
    pub tasks: Vec<PlanTask>,
    /// Each plan task's last executor task: its journal id, status and the
    /// reason or evidence.
    pub runs: BTreeMap<String, (String, TaskStatus, Option<String>)>,
    /// Model answers that led somewhere: the plan's, and those of tasks
    /// that ended verified (where the Scripted model picks up on resume).
    pub verified_turns: usize,
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
            verified_turns: 0,
        };
        let mut turns: BTreeMap<String, usize> = BTreeMap::new();
        // Executor task id -> plan task id.
        let mut owner: BTreeMap<String, String> = BTreeMap::new();
        for entry in plan {
            match entry {
                Entry::Plan { thought, tasks, .. } => {
                    view.thought = thought.clone();
                    view.tasks = tasks.clone();
                    view.verified_turns += 1;
                }
                Entry::PlanInvalid { .. } => view.verified_turns += 1,
                Entry::Answer { task, .. } | Entry::Invalid { task, .. } => {
                    *turns.entry(task.clone()).or_default() += 1
                }
                Entry::PlanStatus { status, reason, .. } => {
                    view.state = *status;
                    view.reason = reason.clone();
                }
                Entry::Task {
                    task,
                    plan_task: Some(part),
                    ..
                } => {
                    owner.insert(task.clone(), part.clone());
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
                    }
                }
                _ => {}
            }
        }
        view.verified_turns += view
            .runs
            .values()
            .filter(|(_, status, _)| *status == TaskStatus::Completed)
            .map(|(task, _, _)| turns.get(task).copied().unwrap_or(0))
            .sum::<usize>();
        Some(view)
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
        text
    }
}
