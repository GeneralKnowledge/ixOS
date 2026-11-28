//! Bonsai Harness: the system around a small local model that turns a
//! request into verified work (examples/device-lab/docs/bonsai-harness-plan.md).
//!
//! The harness owns the workflow and the model supplies one decision at a
//! time. This is the plan's first slice (H1 to H3, thin): one task, end to
//! end. The executor is mini-swe-agent sized: one constrained call per
//! answer, a linear history, limits, Stop. The harness, not the model,
//! decides when a task is complete, from the evidence the environment
//! recorded. Every step goes to the project's journal before the screen
//! shows it.
//!
//! The harness doesn't know about Python: environments plug in through
//! [`Environment`] (Python Lab, [`python_lab`], is the first) and models
//! through [`HarnessModel`].

pub mod artifacts;
pub mod calls;
pub mod evidence;
pub mod journal;
pub mod live;
pub mod plan;
mod prompt;
pub mod python_lab;
pub mod samples;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use forge_runtime::{lock, logging, now_ms};
use forge_services::ai::{
    AiError, ChatMessage, ChatRequest, Collect, LanguageModel, ModelInfo, ScriptedModel, TextOut,
};

use crate::native::ai::{AiModelError, AiModelInfo, AiRequest, TextSink};
use calls::{parse, with_finish, ToolCall, FINISH, GIVE_UP};
use evidence::{Criterion, Evidence, LadderTask};
use journal::{Entry, Journal, TaskStatus};
use plan::{PlanState, PlanView};
use python_lab::{PythonHost, PythonLab};

/// What a tool call did: whether it worked, the text the model sees, and
/// the evidence the harness keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolOutcome {
    pub ok: bool,
    pub text: String,
    pub evidence: Vec<Evidence>,
}

impl ToolOutcome {
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            ok: true,
            text: text.into(),
            evidence: Vec::new(),
        }
    }

    pub fn failed(text: impl Into<String>) -> Self {
        Self {
            ok: false,
            text: text.into(),
            evidence: Vec::new(),
        }
    }
}

/// Where the harness works: its tools, how to run them, the evidence they
/// leave.
pub trait Environment: Send + Sync {
    /// The tools the model may call (the harness adds `finish`).
    fn tools(&self) -> Vec<calls::ToolSpec>;
    /// A short view of the workspace for the start of a task.
    fn overview(&self) -> String;
    fn execute(&self, call: &ToolCall) -> ToolOutcome;
    /// A file's text, for acceptance checks.
    fn read_text(&self, path: &str) -> Option<String>;
    /// Stops a running tool (Stop).
    fn interrupt(&self);
    /// Keeps a copy of the workspace as it is now, under `name`.
    fn checkpoint(&self, name: &str) -> std::io::Result<()>;
    /// Puts the workspace back as the checkpoint `name` kept it.
    fn restore(&self, name: &str) -> std::io::Result<()>;
}

/// A model the harness can drive, implemented by the host (iOS: llama.cpp's
/// `LlamaCppModel`). `grammar` is a GBNF grammar the answer must follow.
/// Rust calls `generate` from a thread of its own, one request at a time.
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait HarnessModel: Send + Sync {
    fn describe(&self) -> AiModelInfo;
    async fn generate(
        &self,
        request: AiRequest,
        grammar: Option<String>,
        sink: Arc<TextSink>,
    ) -> Result<(), AiModelError>;
}

/// [`HarnessModel`] as Rust calls it, so tests can use a scripted model.
pub trait Brain: Send + Sync {
    fn describe(&self) -> ModelInfo;
    fn generate(
        &self,
        request: ChatRequest,
        grammar: Option<&str>,
        out: Arc<dyn TextOut>,
    ) -> Result<(), AiError>;
}

/// The Scripted model: replays a ladder task's known-good answers
/// (`evidence::script`), a little slowly so the screen can follow.
struct ScriptBrain(ScriptedModel);

/// The id the Scripted model records with each task.
pub const SCRIPTED_MODEL_ID: &str = "scripted";

impl ScriptBrain {
    fn new(answers: Vec<String>) -> Self {
        Self(
            ScriptedModel::new(answers)
                .info(ModelInfo {
                    id: SCRIPTED_MODEL_ID.into(),
                    name: "Scripted".into(),
                    available: true,
                    unavailable_reason: None,
                    context_tokens: 8_192,
                })
                // Slow enough on the phone for the screen to follow; tests
                // don't wait.
                .pause(Duration::from_millis(if cfg!(test) { 0 } else { 400 })),
        )
    }
}

impl Brain for ScriptBrain {
    fn describe(&self) -> ModelInfo {
        self.0.describe()
    }

    fn generate(
        &self,
        request: ChatRequest,
        _grammar: Option<&str>,
        out: Arc<dyn TextOut>,
    ) -> Result<(), AiError> {
        self.0.generate(request, out)
    }
}

struct HostBrain(Arc<dyn HarnessModel>);

impl Brain for HostBrain {
    fn describe(&self) -> ModelInfo {
        self.0.describe().into()
    }

    fn generate(
        &self,
        request: ChatRequest,
        grammar: Option<&str>,
        out: Arc<dyn TextOut>,
    ) -> Result<(), AiError> {
        forge_runtime::block_on(self.0.generate(
            request.into(),
            grammar.map(str::to_string),
            TextSink::new(out),
        ))
        .map_err(AiError::from)
    }
}

/// The limits a task runs under (shown on screen).
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct HarnessLimits {
    /// Model answers per task.
    pub max_steps: u32,
    /// One Python run.
    pub run_seconds: u32,
    /// The whole task.
    pub task_seconds: u32,
    /// The longest model answer.
    pub turn_tokens: u32,
    /// The longest result sent back to the model.
    pub result_chars: u32,
    /// Invalid answers in a row before the task fails.
    pub invalid_in_a_row: u32,
    /// The same failing call this many times gets a nudge; once more fails.
    pub same_failure_nudge: u32,
}

impl Default for HarnessLimits {
    fn default() -> Self {
        Self {
            max_steps: 12,
            run_seconds: 30,
            task_seconds: 600,
            turn_tokens: 1200,
            result_chars: 1500,
            invalid_in_a_row: 3,
            same_failure_nudge: 3,
        }
    }
}

/// A ready-made task (the ladder).
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct HarnessTask {
    pub id: String,
    pub title: String,
    pub request: String,
    /// What proves it done, in words.
    pub acceptance: Vec<String>,
    /// Bonsai plans it as several tasks first (H4).
    pub planned: bool,
    /// It can't succeed: it passes by ending Blocked with a clear question.
    pub ends_blocked: bool,
}

/// The task record, as the screen shows it.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct HarnessTaskRecord {
    pub id: String,
    pub description: String,
    pub status: TaskStatus,
    pub dependencies: Vec<String>,
    pub acceptance: Vec<String>,
    /// How many times the executor said it was done.
    pub attempts: u32,
    /// The evidence for Completed, or why it failed or stopped.
    pub result: Option<String>,
    pub model_id: String,
}

/// One executor step.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct HarnessStep {
    pub index: u32,
    pub thought: String,
    /// The tool, or "invalid" for an answer that wasn't a call.
    pub tool: String,
    /// The call as the model wrote it (long contents shortened).
    pub call: String,
    pub ok: Option<bool>,
    pub result: String,
    pub model_ms: u64,
    pub tool_ms: u64,
}

/// What the harness is doing now.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum HarnessPhase {
    Idle,
    Thinking,
    Acting,
    Verifying,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct HarnessSnapshot {
    pub running: bool,
    pub phase: HarnessPhase,
    pub project: Option<String>,
    pub task: Option<HarnessTaskRecord>,
    pub steps: Vec<HarnessStep>,
    /// The model's answer as it streams in.
    pub draft: String,
    /// The same answer read field by field (thought, tool, file, code),
    /// while Bonsai is writing it.
    pub live: Option<live::HarnessLive>,
    pub elapsed_ms: u64,
    pub limits: HarnessLimits,
    /// The plan the task belongs to, or the plan being written.
    pub plan: Option<HarnessPlan>,
    /// Plans are shown for approval before they run.
    pub approve_plans: bool,
}

/// A plan, as the screen shows it.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct HarnessPlan {
    pub id: String,
    pub request: String,
    pub status: PlanState,
    /// Why it stopped or failed, or what proved it complete.
    pub reason: Option<String>,
    /// The planner's one line on the plan.
    pub thought: String,
    pub tasks: Vec<HarnessPlanTask>,
    /// Tasks verified so far (as in "3 / 6 tasks passed").
    pub passed: u32,
    /// The request's own checks, made once every task is verified.
    pub acceptance: Vec<String>,
    /// Approve it, or resume it from its last verified task.
    pub can_continue: bool,
    /// Blocked: the question to answer before it can go on.
    pub question: Option<String>,
    /// A tests-first request's test file.
    pub tests_file: Option<String>,
    /// The tests waiting for the person to check them, as they are now.
    pub tests_to_check: Option<String>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct HarnessPlanTask {
    pub id: String,
    pub description: String,
    pub after: Vec<String>,
    pub acceptance: Vec<String>,
    pub status: TaskStatus,
    pub result: Option<String>,
    /// Times Recovery mode took it up after a failure.
    pub recoveries: u32,
}

/// A file of the project, for the screen.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ProjectFile {
    pub path: String,
    pub bytes: u64,
    pub text: String,
}

#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum HarnessError {
    #[error("A task is still running. Stop it first.")]
    Busy,
    #[error("Describe the task first.")]
    EmptyTask,
    #[error("There is no task called {id}.")]
    UnknownTask { id: String },
    #[error("There is no plan to approve or resume.")]
    NoPlan,
    #[error("{reason}")]
    Storage { reason: String },
}

/// The project a task in the person's own words works in.
pub const SCRATCH_PROJECT: &str = "scratch";

/// The folder under the projects folder that holds the ladder tasks'
/// projects. The harness wipes and reseeds them on every ladder run, so
/// they are hidden from the person's project list (python-lab-workspace-plan.md,
/// PL1a).
pub const LADDER_FOLDER: &str = ".ladder";

/// Whether `name` is a ladder task's id (ignoring case), which a person's
/// project may not be called.
pub fn is_ladder_task(name: &str) -> bool {
    let lower = name.to_lowercase();
    evidence::ladder().iter().any(|task| task.id == lower)
}

/// Moves ladder projects left at the top of `projects` by builds before
/// PL1a into [`LADDER_FOLDER`], once. Best effort: a folder that can't be
/// moved stays where it is.
pub fn move_old_ladder_projects(projects: &Path) {
    for task in evidence::ladder() {
        let old = projects.join(task.id);
        let new = projects.join(LADDER_FOLDER).join(task.id);
        if old.is_dir() && !new.exists() {
            let moved = std::fs::create_dir_all(projects.join(LADDER_FOLDER))
                .and_then(|_| std::fs::rename(&old, &new));
            if let Err(err) = moved {
                logging::warn!(target: "harness", "could not move ladder project {}: {err}", task.id);
            }
        }
    }
}

/// The harness the host opens once, over Python Lab. Thread-safe.
#[derive(uniffi::Object)]
pub struct BonsaiHarness {
    python: Arc<dyn PythonHost>,
    projects: PathBuf,
    limits: HarnessLimits,
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    stop: AtomicBool,
}

struct State {
    running: bool,
    phase: HarnessPhase,
    project: Option<String>,
    /// The open project's folder (a ladder task's is under `.ladder`).
    root: Option<PathBuf>,
    entries: Vec<Entry>,
    journal: Option<Journal>,
    out: Option<Arc<Collect>>,
    started: Option<Instant>,
    finished_ms: u64,
    environment: Option<Arc<dyn Environment>>,
    approve_plans: bool,
}

#[uniffi::export]
impl BonsaiHarness {
    /// `projects_folder` holds one folder per project (iOS: Documents/Python
    /// Lab, shown in the Files app); ladder tasks' projects are under its
    /// `.ladder` folder.
    #[uniffi::constructor]
    pub fn new(python: Arc<dyn PythonHost>, projects_folder: String) -> Arc<Self> {
        Arc::new(Self::with_limits(
            python,
            PathBuf::from(projects_folder),
            HarnessLimits::default(),
        ))
    }

    pub fn ladder(&self) -> Vec<HarnessTask> {
        evidence::ladder()
            .into_iter()
            .map(|t| HarnessTask {
                id: t.id.into(),
                title: t.title.into(),
                request: t.request.into(),
                acceptance: if t.ends_blocked {
                    vec!["it ends Blocked with one clear question, not a loop".into()]
                } else {
                    t.criteria.iter().map(Criterion::describe).collect()
                },
                planned: t.planned,
                ends_blocked: t.ends_blocked,
            })
            .collect()
    }

    pub fn snapshot(&self) -> HarnessSnapshot {
        let state = lock(&self.shared.state);
        let current_plan = plan::current(&state.entries);
        // While a plan is written, no task of its own has started yet.
        let task = match current_plan {
            Some(entries) if !entries.iter().any(|e| matches!(e, Entry::Task { .. })) => &[],
            _ => journal::last_task(&state.entries),
        };
        HarnessSnapshot {
            running: state.running,
            phase: state.phase,
            project: state.project.clone(),
            task: prompt::record(task),
            steps: prompt::steps(task),
            draft: match (&state.out, state.phase) {
                (Some(out), HarnessPhase::Thinking) => out.text(),
                _ => String::new(),
            },
            // Kept while the step runs and is checked, so the screen shows
            // the finished answer even if no poll saw its last tokens.
            live: match (&state.out, state.phase) {
                (
                    Some(out),
                    HarnessPhase::Thinking | HarnessPhase::Acting | HarnessPhase::Verifying,
                ) => {
                    let text = out.text();
                    (!text.is_empty()).then(|| live::read(&text))
                }
                _ => None,
            },
            elapsed_ms: match state.started {
                Some(started) if state.running => started.elapsed().as_millis() as u64,
                _ => state.finished_ms,
            },
            limits: self.limits.clone(),
            plan: current_plan.and_then(PlanView::read).map(|view| {
                let mut record = plan_record(&view);
                if view.awaiting_test_review() {
                    // Read now, so the person's edits in Python Lab show.
                    record.tests_to_check = match (&state.root, &view.tests_file) {
                        (Some(root), Some(file)) => std::fs::read_to_string(root.join(file)).ok(),
                        _ => None,
                    };
                }
                record
            }),
            approve_plans: state.approve_plans,
        }
    }

    /// Whether a plan waits for the person's approval before it runs.
    pub fn set_approve_plans(&self, approve: bool) {
        lock(&self.shared.state).approve_plans = approve;
    }

    /// Starts a ladder task in a fresh project of its own (the last run's
    /// journal is kept in the projects folder's `.runs`).
    pub fn start_ladder(
        &self,
        model: Arc<dyn HarnessModel>,
        task_id: String,
    ) -> Result<(), HarnessError> {
        let task = evidence::ladder()
            .into_iter()
            .find(|t| t.id == task_id)
            .ok_or(HarnessError::UnknownTask { id: task_id })?;
        self.start(Arc::new(HostBrain(model)), task)
    }

    /// Runs a ladder task with the Scripted model: known-good answers
    /// instead of a real model, to prove the tools, guard, evidence checks
    /// and journal on this device apart from any model.
    pub fn start_ladder_scripted(&self, task_id: String) -> Result<(), HarnessError> {
        let task = evidence::ladder()
            .into_iter()
            .find(|t| t.id == task_id)
            .ok_or(HarnessError::UnknownTask { id: task_id })?;
        self.start(Arc::new(ScriptBrain::new(evidence::script(task.id))), task)
    }

    /// Starts a task in the person's own words, in their Python Lab
    /// project `project` (`scratch` by default; kept between tasks), so
    /// Bonsai works on the person's files and they open its work in Python
    /// Lab (python-lab-workspace-plan.md, PL1e).
    pub fn start_custom(
        &self,
        model: Arc<dyn HarnessModel>,
        request: String,
        project: String,
    ) -> Result<(), HarnessError> {
        self.start_request(Arc::new(HostBrain(model)), request, &project, false)
    }

    /// Plans a task in the person's own words as several tasks, then runs
    /// them (after approval, when that's on), in the person's Python Lab
    /// project `project`, as [`Self::start_custom`] does.
    pub fn start_custom_plan(
        &self,
        model: Arc<dyn HarnessModel>,
        request: String,
        project: String,
    ) -> Result<(), HarnessError> {
        self.start_request(Arc::new(HostBrain(model)), request, &project, true)
    }

    /// A request in the person's own words, tests first: Bonsai writes the
    /// tests, the plan waits while the person checks (and may edit) them,
    /// and approving it has Bonsai write the code that passes them
    /// unchanged. In the person's Python Lab project `project`.
    pub fn start_custom_tests_first(
        &self,
        model: Arc<dyn HarnessModel>,
        request: String,
        project: String,
    ) -> Result<(), HarnessError> {
        self.start_tests_first(Arc::new(HostBrain(model)), request, &project)
    }

    /// Runs the plan on screen: approves it, or resumes it from its last
    /// verified task (the project's files go back to that task's
    /// checkpoint).
    pub fn continue_plan(&self, model: Arc<dyn HarnessModel>) -> Result<(), HarnessError> {
        self.continue_with(Some(model))
    }

    /// [`Self::continue_plan`] with the Scripted model, which picks up its
    /// known-good answers where the plan left off.
    pub fn continue_plan_scripted(&self) -> Result<(), HarnessError> {
        self.continue_with(None)
    }

    /// Answers a blocked plan's question, then runs on from the blocked
    /// task, which is told the answer (H5).
    pub fn answer_plan(
        &self,
        model: Arc<dyn HarnessModel>,
        answer: String,
    ) -> Result<(), HarnessError> {
        self.answer_with(Some(model), answer)
    }

    /// [`Self::answer_plan`] with the Scripted model.
    pub fn answer_plan_scripted(&self, answer: String) -> Result<(), HarnessError> {
        self.answer_with(None, answer)
    }

    /// Discards a plan waiting for approval; nothing of it runs.
    pub fn discard_plan(&self) -> Result<(), HarnessError> {
        let mut state = lock(&self.shared.state);
        if state.running {
            return Err(HarnessError::Busy);
        }
        let Some(view) = plan::current(&state.entries).and_then(PlanView::read) else {
            return Err(HarnessError::NoPlan);
        };
        if view.state != PlanState::AwaitingApproval {
            return Err(HarnessError::NoPlan);
        }
        let entry = Entry::PlanStatus {
            plan: view.id,
            status: PlanState::Discarded,
            reason: Some("Discarded.".into()),
            at_ms: now_ms(),
        };
        let Some(root) = state.root.clone() else {
            return Err(HarnessError::NoPlan);
        };
        Journal::open(&root)
            .and_then(|mut journal| journal.append(&entry))
            .map_err(|err| HarnessError::Storage {
                reason: format!("Could not write the journal: {err}"),
            })?;
        state.entries.push(entry);
        Ok(())
    }

    /// Opens a project's journal for the screen (the last task and its
    /// steps). A task the app closed during is marked stopped.
    pub fn open_project(&self, name: String) -> Result<(), HarnessError> {
        let mut state = lock(&self.shared.state);
        if state.running {
            return Err(HarnessError::Busy);
        }
        let (name, root) = self.project_root(&name)?;
        let entries = Journal::replay(&root);
        state.entries = entries;
        state.project = Some(name);
        state.root = Some(root.clone());
        state.journal = None;
        state.finished_ms = 0;
        let task = journal::last_task(&state.entries);
        if let (Some(Entry::Task { task: id, .. }), Some((status, _))) =
            (task.first(), journal::status_of(task))
        {
            if !status.is_final() {
                let id = id.clone();
                let steps = journal::interrupted_steps(task);
                let reason = match steps.last() {
                    Some(step) => format!("The app closed during step {step}."),
                    None => "The app closed during this task.".to_string(),
                };
                let entry = Entry::Status {
                    task: id,
                    status: TaskStatus::Stopped,
                    reason: Some(reason),
                    at_ms: now_ms(),
                };
                if let Ok(mut journal) = Journal::open(&root) {
                    let _ = journal.append(&entry);
                }
                state.entries.push(entry);
            }
        }
        // A plan the app closed during stops too, and can be resumed from
        // its last verified task.
        if let Some(view) = plan::current(&state.entries).and_then(PlanView::read) {
            if matches!(view.state, PlanState::Planning | PlanState::Running) {
                let at = view
                    .tasks
                    .iter()
                    .find(|t| {
                        !matches!(
                            view.status_of(&t.id),
                            TaskStatus::Completed | TaskStatus::Ready
                        )
                    })
                    .map(|t| format!("The app closed during task {}.", t.id))
                    .unwrap_or_else(|| "The app closed during this plan.".into());
                let entry = Entry::PlanStatus {
                    plan: view.id,
                    status: PlanState::Stopped,
                    reason: Some(at),
                    at_ms: now_ms(),
                };
                if let Ok(mut journal) = Journal::open(&root) {
                    let _ = journal.append(&entry);
                }
                state.entries.push(entry);
            }
        }
        Ok(())
    }

    /// Stops the model at its next token, or the Python run at its next
    /// instruction; the task ends Stopped.
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let state = lock(&self.shared.state);
        if let Some(out) = &state.out {
            out.stop();
        }
        if let Some(environment) = &state.environment {
            environment.interrupt();
        }
    }

    /// The last task as text, to copy as evidence.
    pub fn transcript(&self) -> String {
        let state = lock(&self.shared.state);
        let project = state.project.as_deref().unwrap_or("?");
        match plan::current(&state.entries) {
            Some(entries) => prompt::plan_transcript(project, entries),
            None => prompt::transcript(project, journal::last_task(&state.entries)),
        }
    }

    /// Step `step` of the last task as the live view drew it once written:
    /// its thought, tool, path and the whole file or edit, so the screen
    /// can go back through the steps. None for an answer that wasn't a call.
    pub fn replay(&self, step: u32) -> Option<live::HarnessLive> {
        let state = lock(&self.shared.state);
        journal::last_task(&state.entries)
            .iter()
            .find_map(|entry| match entry {
                Entry::Answer { step: s, text, .. } if *s == step => Some(live::read(text)),
                _ => None,
            })
    }

    /// The folder of the project on screen, for its images
    /// (`project_artifacts`).
    pub fn project_folder(&self) -> Option<String> {
        lock(&self.shared.state)
            .root
            .as_ref()
            .map(|root| root.display().to_string())
    }

    /// The open project's files.
    pub fn files(&self) -> Vec<ProjectFile> {
        let Some(root) = lock(&self.shared.state).root.clone() else {
            return Vec::new();
        };
        let lab = PythonLab::new(root.clone(), Arc::clone(&self.python), Duration::ZERO);
        lab.files()
            .into_iter()
            .map(|(path, bytes)| ProjectFile {
                text: std::fs::read_to_string(root.join(&path))
                    .unwrap_or_else(|_| "(not text)".into()),
                path,
                bytes,
            })
            .collect()
    }
}

impl BonsaiHarness {
    /// The last task, its step count and the paths of its project's
    /// files, all from one read of the state: the root is the one the task
    /// ran in (a ladder task's lives under the ladder folder). Paths only;
    /// nothing is read. `None` while a run is claimed: a new run resets its
    /// project (claim first, then reset), so its files aren't the last
    /// task's until it finishes.
    pub(crate) fn last_task_with_files(
        &self,
    ) -> Option<(Option<HarnessTaskRecord>, u32, Vec<String>)> {
        // The files are listed while the lock is held, so a new run can't
        // reset the root between reading the task and listing its files.
        // Listing names only, in a small project folder, keeps this short.
        let state = lock(&self.shared.state);
        if state.running {
            return None;
        }
        let task = journal::last_task(&state.entries);
        let files = state
            .root
            .clone()
            .map(|root| {
                PythonLab::new(root, Arc::clone(&self.python), Duration::ZERO)
                    .files()
                    .into_iter()
                    .map(|(path, _)| path)
                    .collect()
            })
            .unwrap_or_default();
        Some((
            prompt::record(task),
            prompt::steps(task).len() as u32,
            files,
        ))
    }

    pub fn with_limits(
        python: Arc<dyn PythonHost>,
        projects: PathBuf,
        limits: HarnessLimits,
    ) -> Self {
        logging::init();
        move_old_ladder_projects(&projects);
        Self {
            python,
            projects,
            limits,
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    running: false,
                    phase: HarnessPhase::Idle,
                    project: None,
                    root: None,
                    entries: Vec::new(),
                    journal: None,
                    out: None,
                    started: None,
                    finished_ms: 0,
                    environment: None,
                    approve_plans: true,
                }),
                stop: AtomicBool::new(false),
            }),
        }
    }

    /// A person's project: named as Python Lab names them (any letters,
    /// spaces; never hidden, never a ladder task's name).
    /// A project's checked name (trimmed, as Python Lab saves it) and its
    /// folder. Everything after uses that name, never the one passed in.
    fn project_root(&self, name: &str) -> Result<(String, PathBuf), HarnessError> {
        let name = crate::workspace::project_name(name).map_err(|err| HarnessError::Storage {
            reason: err.to_string(),
        })?;
        let root = self.projects.join(&name);
        Ok((name, root))
    }

    /// A ladder task's project, under [`LADDER_FOLDER`].
    fn ladder_root(&self, id: &str) -> Result<PathBuf, HarnessError> {
        calls::check_path(id).map_err(|reason| HarnessError::Storage { reason })?;
        Ok(self.projects.join(LADDER_FOLDER).join(id))
    }

    pub fn start(&self, brain: Arc<dyn Brain>, task: LadderTask) -> Result<(), HarnessError> {
        let root = self.ladder_root(task.id)?;
        let lab = PythonLab::new(
            root.clone(),
            Arc::clone(&self.python),
            Duration::from_secs(self.limits.run_seconds.into()),
        );
        self.claim()?;
        let prepared =
            archive_journal(&self.projects, task.id, &root).and_then(|_| lab.reset(&task.seed));
        if let Err(err) = prepared {
            self.release();
            return Err(HarnessError::Storage {
                reason: format!("Could not prepare the project: {err}"),
            });
        }
        let job = if task.planned {
            Job::Plan {
                request: task.request.into(),
                criteria: task.criteria,
            }
        } else {
            Job::Task {
                request: task.request.into(),
                criteria: task.criteria,
            }
        };
        self.launch(brain, Arc::new(lab), task.id, job)
    }

    pub fn start_request(
        &self,
        brain: Arc<dyn Brain>,
        request: String,
        project: &str,
        planned: bool,
    ) -> Result<(), HarnessError> {
        let kind = if planned {
            RequestKind::Plan
        } else {
            RequestKind::Task
        };
        self.start_kind(brain, request, project, kind)
    }

    /// [`Self::start_custom_tests_first`] with any brain.
    pub fn start_tests_first(
        &self,
        brain: Arc<dyn Brain>,
        request: String,
        project: &str,
    ) -> Result<(), HarnessError> {
        self.start_kind(brain, request, project, RequestKind::TestsFirst)
    }

    fn start_kind(
        &self,
        brain: Arc<dyn Brain>,
        request: String,
        project: &str,
        kind: RequestKind,
    ) -> Result<(), HarnessError> {
        let request = request.trim().to_string();
        if request.is_empty() {
            return Err(HarnessError::EmptyTask);
        }
        let (project, root) = self.project_root(project)?;
        let project = project.as_str();
        // Only scratch is made when missing: any other project is one the
        // person made in Python Lab, and a missing one was renamed or
        // deleted there.
        if project != SCRATCH_PROJECT && !root.is_dir() {
            return Err(HarnessError::Storage {
                reason: format!("There is no project called {project} any more. Choose another."),
            });
        }
        let lab = PythonLab::new(
            root.clone(),
            Arc::clone(&self.python),
            Duration::from_secs(self.limits.run_seconds.into()),
        );
        self.claim()?;
        if let Err(err) = std::fs::create_dir_all(&root) {
            self.release();
            return Err(HarnessError::Storage {
                reason: err.to_string(),
            });
        }
        let job = match kind {
            // Each planned task carries its own checks.
            RequestKind::Plan => Job::Plan {
                request,
                criteria: Vec::new(),
            },
            RequestKind::Task => Job::Task {
                request,
                criteria: vec![Criterion::WorkRuns],
            },
            RequestKind::TestsFirst => Job::TestsFirst { request },
        };
        self.launch(brain, Arc::new(lab), project, job)
    }

    /// `model`, or the Scripted model picking up its answers where the
    /// plan `view` of `project` left off.
    fn resume_brain(
        model: Option<Arc<dyn HarnessModel>>,
        project: &str,
        view: &PlanView,
    ) -> Arc<dyn Brain> {
        match model {
            Some(model) => Arc::new(HostBrain(model)),
            None => Arc::new(ScriptBrain::new(
                evidence::script(project)
                    .into_iter()
                    .skip(view.resume_turns)
                    .collect(),
            )),
        }
    }

    /// Marks the harness busy for the plan on screen, when `fits` it, in
    /// one hold of the lock, so another project can't be opened between
    /// the check and the claim. Returns the project, its folder and the
    /// plan.
    fn claim_plan(
        &self,
        fits: impl Fn(&PlanView) -> bool,
    ) -> Result<(String, PathBuf, PlanView), HarnessError> {
        let mut state = lock(&self.shared.state);
        if state.running {
            return Err(HarnessError::Busy);
        }
        let view = plan::current(&state.entries)
            .and_then(PlanView::read)
            .filter(|view| fits(view))
            .ok_or(HarnessError::NoPlan)?;
        let (Some(project), Some(root)) = (state.project.clone(), state.root.clone()) else {
            return Err(HarnessError::NoPlan);
        };
        state.running = true;
        self.shared.stop.store(false, Ordering::SeqCst);
        Ok((project, root, view))
    }

    /// Records the person's answer to the blocked plan on screen, then runs
    /// on with `model` (the Scripted model when none).
    fn answer_with(
        &self,
        model: Option<Arc<dyn HarnessModel>>,
        answer: String,
    ) -> Result<(), HarnessError> {
        let answer = answer.trim().to_string();
        if answer.is_empty() {
            return Err(HarnessError::EmptyTask);
        }
        let (project, root, view) = self.claim_plan(|view| view.blocked.is_some())?;
        let Some((task, _)) = view.blocked.clone() else {
            self.release();
            return Err(HarnessError::NoPlan);
        };
        let entry = Entry::PersonAnswer {
            plan: view.id.clone(),
            plan_task: task,
            text: answer,
            at_ms: now_ms(),
        };
        if let Err(err) = Journal::open(&root).and_then(|mut journal| journal.append(&entry)) {
            self.release();
            return Err(HarnessError::Storage {
                reason: format!("Could not write the journal: {err}"),
            });
        }
        // The screen shows the answer at once, so a launch that fails
        // leaves a plan to resume, not a question to answer again.
        lock(&self.shared.state).entries.push(entry);
        let brain = Self::resume_brain(model, &project, &view);
        let lab = PythonLab::new(
            root,
            Arc::clone(&self.python),
            Duration::from_secs(self.limits.run_seconds.into()),
        );
        self.launch(
            brain,
            Arc::new(lab),
            &project,
            Job::Continue { plan: view.id },
        )
    }

    /// Approves or resumes the plan on screen with `model` (the Scripted
    /// model when none).
    fn continue_with(&self, model: Option<Arc<dyn HarnessModel>>) -> Result<(), HarnessError> {
        self.continue_brain(|project, view| Self::resume_brain(model, project, view))
    }

    /// Approves or resumes the plan on screen with the brain `brain` picks
    /// for its project and plan.
    pub(crate) fn continue_brain(
        &self,
        brain: impl FnOnce(&str, &PlanView) -> Arc<dyn Brain>,
    ) -> Result<(), HarnessError> {
        let (project, root, view) = self.claim_plan(PlanView::can_continue)?;
        let brain = brain(&project, &view);
        let lab = PythonLab::new(
            root,
            Arc::clone(&self.python),
            Duration::from_secs(self.limits.run_seconds.into()),
        );
        self.launch(
            brain,
            Arc::new(lab),
            &project,
            Job::Continue { plan: view.id },
        )
    }

    /// Marks the harness busy, or says it already is.
    fn claim(&self) -> Result<(), HarnessError> {
        let mut state = lock(&self.shared.state);
        if state.running {
            return Err(HarnessError::Busy);
        }
        state.running = true;
        // Reset under the state lock, before any preparation, so a Stop
        // for this task (which follows the claim) is never cleared.
        self.shared.stop.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Gives up a claim after a run failed to start. Preparing may already
    /// have archived the last task's journal and reset its project, so the
    /// last task is forgotten too: nothing may pair its record with files
    /// it no longer has (the verified-task card reads both).
    fn release(&self) {
        let mut state = lock(&self.shared.state);
        state.running = false;
        state.entries.clear();
        state.root = None;
    }

    fn launch(
        &self,
        brain: Arc<dyn Brain>,
        lab: Arc<PythonLab>,
        project: &str,
        job: Job,
    ) -> Result<(), HarnessError> {
        let journal = match Journal::open(lab.root()) {
            Ok(journal) => journal,
            Err(err) => {
                self.release();
                return Err(HarnessError::Storage {
                    reason: format!("Could not open the journal: {err}"),
                });
            }
        };
        let approve = {
            let mut state = lock(&self.shared.state);
            state.entries = Journal::replay(lab.root());
            state.journal = Some(journal);
            state.project = Some(project.to_string());
            state.root = Some(lab.root().to_path_buf());
            state.started = Some(Instant::now());
            state.phase = HarnessPhase::Thinking;
            state.environment = Some(Arc::clone(&lab) as Arc<dyn Environment>);
            state.approve_plans
        };
        let shared = Arc::clone(&self.shared);
        let limits = self.limits.clone();
        let spawned = std::thread::Builder::new()
            .name("bonsai-harness".into())
            .spawn(move || {
                let run = Run {
                    shared: Arc::clone(&shared),
                    brain,
                    environment: lab,
                    limits,
                };
                match job {
                    Job::Task { request, criteria } => {
                        run.task(request.clone(), request, criteria, None);
                    }
                    Job::Plan { request, criteria } => run.plan(request, criteria, approve),
                    Job::Continue { plan } => run.continue_plan(&plan, approve),
                    Job::TestsFirst { request } => run.tests_first(request),
                }
                let mut state = lock(&shared.state);
                state.running = false;
                state.phase = HarnessPhase::Idle;
                state.finished_ms = state
                    .started
                    .map(|s| s.elapsed().as_millis() as u64)
                    .unwrap_or(0);
                state.environment = None;
                state.out = None;
            });
        if let Err(err) = spawned {
            self.release();
            return Err(HarnessError::Storage {
                reason: err.to_string(),
            });
        }
        Ok(())
    }
}

/// How a model error starts a task's end reason.
const MODEL_FAILED: &str = "The model failed";

/// How a failed checkpoint starts a task's or plan's end reason.
const CHECKPOINT_FAILED: &str = "Could not save a checkpoint";

/// Whether Recovery mode can help with a task that ended for `reason`: not
/// when the model or the journal failed.
fn recoverable(reason: &str) -> bool {
    !reason.starts_with(MODEL_FAILED)
        && !reason.starts_with(CHECKPOINT_FAILED)
        && !reason.starts_with("Could not write the journal")
}

/// What a launch does.
#[derive(Clone, Copy)]
enum RequestKind {
    Task,
    Plan,
    TestsFirst,
}

enum Job {
    /// One task.
    Task {
        request: String,
        criteria: Vec<Criterion>,
    },
    /// A request planned as several tasks, then run.
    Plan {
        request: String,
        criteria: Vec<Criterion>,
    },
    /// Approve or resume the plan with this id.
    Continue { plan: String },
    /// A request run tests first (plan::tests_first_tasks).
    TestsFirst { request: String },
}

/// A journal id: `prefix-<ms>`, never the same twice in this process.
fn fresh_id(prefix: &str) -> String {
    static LAST: AtomicI64 = AtomicI64::new(0);
    let now = now_ms();
    let mut last = LAST.load(Ordering::SeqCst);
    loop {
        let next = now.max(last + 1);
        match LAST.compare_exchange(last, next, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return format!("{prefix}-{next}"),
            Err(seen) => last = seen,
        }
    }
}

/// The plan as the screen shows it.
fn plan_record(view: &PlanView) -> HarnessPlan {
    let tasks: Vec<HarnessPlanTask> = view
        .tasks
        .iter()
        .map(|t| {
            let (status, result) = match view.runs.get(&t.id) {
                Some((_, status, reason)) => (*status, reason.clone()),
                None => (TaskStatus::Ready, None),
            };
            HarnessPlanTask {
                id: t.id.clone(),
                description: t.description.clone(),
                after: t.after.clone(),
                acceptance: t.criteria.iter().map(Criterion::describe).collect(),
                status,
                result,
                recoveries: view.recoveries.get(&t.id).copied().unwrap_or(0),
            }
        })
        .collect();
    HarnessPlan {
        id: view.id.clone(),
        request: view.request.clone(),
        status: view.state,
        reason: view.reason.clone(),
        thought: view.thought.clone(),
        passed: tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .count() as u32,
        tasks,
        acceptance: view.criteria.iter().map(Criterion::describe).collect(),
        can_continue: view.can_continue(),
        question: view.blocked.as_ref().map(|(_, question)| question.clone()),
        tests_file: view.tests_file.clone(),
        tests_to_check: None,
    }
}

/// Moves a ladder project's last journal to `<projects>/.runs/`, so a fresh
/// run doesn't lose the previous run's record.
fn archive_journal(projects: &Path, name: &str, root: &Path) -> std::io::Result<()> {
    let journal = root.join(Journal::FILE);
    if !journal.exists() {
        return Ok(());
    }
    let runs = projects.join(".runs");
    std::fs::create_dir_all(&runs)?;
    std::fs::rename(journal, runs.join(format!("{name}-{}.jsonl", now_ms())))
}

/// One task being worked on, on the harness's thread.
struct Run {
    shared: Arc<Shared>,
    brain: Arc<dyn Brain>,
    environment: Arc<dyn Environment>,
    limits: HarnessLimits,
}

/// How a task's loop ended.
struct End(TaskStatus, String);

impl Run {
    fn stopped(&self) -> bool {
        self.shared.stop.load(Ordering::SeqCst)
    }

    /// Writes `entry` to the journal, then shows it.
    fn record(&self, entry: Entry) -> Result<(), End> {
        let mut state = lock(&self.shared.state);
        if let Some(journal) = state.journal.as_mut() {
            if let Err(err) = journal.append(&entry) {
                logging::warn!(target: "harness", "journal write failed: {err}");
                return Err(End(
                    TaskStatus::Failed,
                    format!("Could not write the journal: {err}"),
                ));
            }
        }
        state.entries.push(entry);
        Ok(())
    }

    fn phase(&self, phase: HarnessPhase) {
        lock(&self.shared.state).phase = phase;
    }

    fn task_entries(&self, task: &str) -> Vec<Entry> {
        let state = lock(&self.shared.state);
        journal::last_task(&state.entries)
            .iter()
            .filter(|e| e.task() == task)
            .cloned()
            .collect()
    }

    /// Runs one task: `request` is what the model sees, `description` what
    /// the journal and the screen show. `part` is its plan and its id there.
    fn task(
        &self,
        request: String,
        description: String,
        criteria: Vec<Criterion>,
        part: Option<(&str, &str)>,
    ) -> End {
        let task = fresh_id("task");
        let started = Instant::now();
        let opened = self
            .record(Entry::Task {
                task: task.clone(),
                description,
                criteria: criteria.clone(),
                model: self.brain.describe().id,
                at_ms: now_ms(),
                plan: part.map(|(plan, _)| plan.to_string()),
                plan_task: part.map(|(_, id)| id.to_string()),
            })
            .and_then(|_| self.status(&task, TaskStatus::Running, None));
        if let Err(end) = opened {
            return end;
        }
        logging::info!(target: "harness", "{task} started");
        let End(status, reason) =
            match self.work(&task, &request, &criteria, started, part.is_some()) {
                Ok(end) | Err(end) => end,
            };
        logging::info!(target: "harness", "{task} ended {status:?}");
        if let Err(end) = self.status(&task, status, Some(reason.clone())) {
            return end;
        }
        End(status, reason)
    }

    fn plan_status(
        &self,
        plan: &str,
        status: PlanState,
        reason: Option<String>,
    ) -> Result<(), End> {
        let recorded = self.record(Entry::PlanStatus {
            plan: plan.into(),
            status,
            reason,
            at_ms: now_ms(),
        });
        if let Err(End(_, failure)) = &recorded {
            // The screen still learns the plan ended, and why; it can be
            // resumed once the journal can be written again.
            lock(&self.shared.state).entries.push(Entry::PlanStatus {
                plan: plan.into(),
                status: PlanState::Failed,
                reason: Some(failure.clone()),
                at_ms: now_ms(),
            });
        }
        recorded
    }

    /// Ends the plan the way `end` ended its task or its drafting.
    fn end_plan(&self, plan: &str, End(status, reason): End) {
        let state = if status == TaskStatus::Stopped {
            PlanState::Stopped
        } else {
            PlanState::Failed
        };
        logging::info!(target: "harness", "{plan} ended {state:?}");
        let _ = self.plan_status(plan, state, Some(reason));
    }

    fn plan_view(&self, plan: &str) -> Option<PlanView> {
        let state = lock(&self.shared.state);
        plan::current(&state.entries)
            .and_then(PlanView::read)
            .filter(|view| view.id == plan)
    }

    /// Planner mode: writes the plan, then waits for approval or runs it.
    fn plan(&self, request: String, criteria: Vec<Criterion>, approve: bool) {
        let plan = fresh_id("plan");
        let opened = self
            .record(Entry::PlanRequest {
                plan: plan.clone(),
                request: request.clone(),
                criteria,
                model: self.brain.describe().id,
                at_ms: now_ms(),
            })
            .and_then(|_| self.plan_status(&plan, PlanState::Planning, None));
        if opened.is_err() {
            return;
        }
        logging::info!(target: "harness", "{plan} started");
        if let Err(end) = self.draft(&plan, &request, None) {
            return self.end_plan(&plan, end);
        }
        if approve {
            let _ = self.plan_status(
                &plan,
                PlanState::AwaitingApproval,
                Some("Approve the plan to run it.".into()),
            );
            return;
        }
        self.run_plan(&plan);
    }

    /// A tests-first request: the harness writes its two-task plan itself
    /// and runs it; the plan stops for the person after the tests task.
    fn tests_first(&self, request: String) {
        let plan = fresh_id("plan");
        let file = plan::TESTS_FILE.to_string();
        let opened = self
            .record(Entry::PlanRequest {
                plan: plan.clone(),
                request,
                criteria: Vec::new(),
                model: self.brain.describe().id,
                at_ms: now_ms(),
            })
            .and_then(|_| {
                self.record(Entry::TestsFirst {
                    plan: plan.clone(),
                    file: file.clone(),
                })
            })
            .and_then(|_| {
                self.record(Entry::Plan {
                    plan: plan.clone(),
                    thought: plan::TESTS_FIRST_THOUGHT.into(),
                    tasks: plan::tests_first_tasks(&file),
                    model_ms: 0,
                })
            });
        if opened.is_err() {
            return;
        }
        logging::info!(target: "harness", "{plan} started, tests first");
        self.run_plan(&plan);
    }

    /// Records the tests as they are now (the person's edits included) as
    /// approved, and makes them part of the checkpoint the code task
    /// starts from, so a recovery's rollback keeps them.
    fn approve_tests(&self, view: &PlanView) -> Result<(), End> {
        let Some(file) = view.tests_file.clone() else {
            return Ok(());
        };
        let Some(content) = self.environment.read_text(&file) else {
            return Err(End(
                TaskStatus::Failed,
                format!("{file} is gone, so there are no tests to approve."),
            ));
        };
        self.environment
            .checkpoint(&view.restart_point())
            .map_err(|err| End(TaskStatus::Failed, format!("{CHECKPOINT_FAILED}: {err}")))?;
        self.record(Entry::TestsApproved {
            plan: view.id.clone(),
            file,
            content,
            at_ms: now_ms(),
        })
    }

    /// Approves the plan, or resumes it from its last verified task. A
    /// plan that never ran starts afresh, and waits for approval first
    /// (when `approve`) unless that is what it is getting now.
    fn continue_plan(&self, plan: &str, approve: bool) {
        let Some(view) = self.plan_view(plan) else {
            return;
        };
        if view.blocked.is_some() {
            // Nothing runs until the person answers the question.
            return;
        }
        if view.awaiting_test_review() {
            // Approving the tests: they stay as the person left them.
            if let Err(end) = self.approve_tests(&view) {
                return self.end_plan(plan, end);
            }
            return self.run_plan(plan);
        }
        if view.tasks.is_empty() {
            // It never got a plan: write one now.
            if let Err(end) = self
                .plan_status(plan, PlanState::Planning, None)
                .and_then(|_| self.draft(plan, &view.request, None))
            {
                return self.end_plan(plan, end);
            }
        }
        if !view.started {
            if approve && view.state != PlanState::AwaitingApproval {
                let _ = self.plan_status(
                    plan,
                    PlanState::AwaitingApproval,
                    Some("Approve the plan to run it.".into()),
                );
                return;
            }
        } else if view.state != PlanState::Blocked {
            // A blocked plan's files went back when it blocked, and keep
            // anything the person added to answer it.
            let point = view.restart_point();
            if let Err(err) = self.environment.restore(&point) {
                return self.end_plan(
                    plan,
                    End(
                        TaskStatus::Failed,
                        format!("Could not go back to the last verified task: {err}"),
                    ),
                );
            }
        }
        if let Some((task, diagnosis)) = view.replan.clone() {
            // The app closed after Recovery mode chose a replan and before
            // the new tasks were written: write them before any task runs.
            if let Some(failed) = view.tasks.iter().find(|t| t.id == task) {
                let reason = view
                    .runs
                    .get(&task)
                    .and_then(|(_, _, reason)| reason.clone())
                    .unwrap_or_else(|| "it wasn't verified".into());
                let progress = view.progress(failed, &reason, &diagnosis);
                if let Err(end) = self.draft(plan, &view.request, Some(&progress)) {
                    return self.end_plan(plan, end);
                }
            }
        }
        self.run_plan(plan);
    }

    /// One model answer under `grammar`: its text and how long it took.
    fn ask(&self, messages: Vec<ChatMessage>, grammar: &str) -> Result<(String, u64), End> {
        if self.stopped() {
            return Err(End(TaskStatus::Stopped, "Stopped.".into()));
        }
        self.phase(HarnessPhase::Thinking);
        let request = ChatRequest {
            model: None,
            messages,
            max_tokens: self.limits.turn_tokens,
            temperature: 0.0,
        };
        let shared = Arc::clone(&self.shared);
        let out = Arc::new(Collect::new().stop_when(move || shared.stop.load(Ordering::SeqCst)));
        lock(&self.shared.state).out = Some(Arc::clone(&out));
        let asked = Instant::now();
        let answer =
            self.brain
                .generate(request, Some(grammar), Arc::clone(&out) as Arc<dyn TextOut>);
        let model_ms = asked.elapsed().as_millis() as u64;
        if self.stopped() {
            return Err(End(TaskStatus::Stopped, "Stopped.".into()));
        }
        match answer {
            Ok(()) => Ok((out.text(), model_ms)),
            Err(AiError::Stopped) => Err(End(TaskStatus::Stopped, "Stopped.".into())),
            Err(err) => Err(End(TaskStatus::Failed, format!("{MODEL_FAILED}: {err}"))),
        }
    }

    /// Asks for a plan until one can be used (or the invalid limit).
    /// `progress` is what happened so far, when replanning.
    fn draft(&self, plan: &str, request: &str, progress: Option<&str>) -> Result<(), End> {
        let grammar = plan::grammar();
        let overview = self.environment.overview();
        let done = self
            .plan_view(plan)
            .map(|v| v.done_ids())
            .unwrap_or_default();
        let earlier = prompt::earlier_work(&lock(&self.shared.state).entries, &[plan]);
        let mut refused: Vec<(String, String)> = Vec::new();
        loop {
            if refused.len() as u32 >= self.limits.invalid_in_a_row {
                return Err(End(
                    TaskStatus::Failed,
                    format!("{} plans in a row couldn't be used.", refused.len()),
                ));
            }
            let messages = plan::messages(request, &overview, &earlier, progress, &refused);
            let (text, model_ms) = self.ask(messages, &grammar)?;
            match plan::parse(&text, &done) {
                Ok((thought, tasks)) => {
                    return self.record(Entry::Plan {
                        plan: plan.into(),
                        thought,
                        tasks,
                        model_ms,
                    });
                }
                Err(error) => {
                    self.record(Entry::PlanInvalid {
                        plan: plan.into(),
                        text: text.clone(),
                        error: error.clone(),
                        model_ms,
                    })?;
                    refused.push((text, error));
                }
            }
        }
    }

    /// Recovery mode: the failed task's evidence goes to the model, which
    /// decides to retry, revise the task, replan or ask the person.
    fn recover(
        &self,
        plan: &str,
        task: &plan::PlanTask,
        run: &str,
        reason: &str,
    ) -> Result<plan::Decision, End> {
        let Some(view) = self.plan_view(plan) else {
            return Err(End(TaskStatus::Failed, "The plan is gone.".into()));
        };
        let recent: Vec<String> = {
            let state = lock(&self.shared.state);
            let results: Vec<String> = state
                .entries
                .iter()
                .filter_map(|e| match e {
                    Entry::Result {
                        task, tool, text, ..
                    } if task == run => Some(format!("{tool}: {}", prompt::cut(text, 500))),
                    _ => None,
                })
                .collect();
            results[results.len().saturating_sub(3)..].to_vec()
        };
        let recent = recent.join("\n");
        let grammar = plan::recovery_grammar();
        let overview = self.environment.overview();
        let mut refused: Vec<(String, String)> = Vec::new();
        loop {
            if refused.len() as u32 >= self.limits.invalid_in_a_row {
                return Err(End(
                    TaskStatus::Failed,
                    format!(
                        "{reason} (and {} recovery answers in a row couldn't be used)",
                        refused.len()
                    ),
                ));
            }
            let messages =
                plan::recovery_messages(&view, task, reason, &recent, &overview, &refused);
            let (text, model_ms) = self.ask(messages, &grammar)?;
            match plan::parse_recovery(&text, &task.id) {
                Ok((thought, decision)) => {
                    self.record(Entry::Recovery {
                        plan: plan.into(),
                        plan_task: task.id.clone(),
                        thought,
                        decision: decision.clone(),
                        model_ms,
                    })?;
                    return Ok(decision);
                }
                Err(error) => {
                    self.record(Entry::RecoveryInvalid {
                        plan: plan.into(),
                        text: text.clone(),
                        error: error.clone(),
                        model_ms,
                    })?;
                    refused.push((text, error));
                }
            }
        }
    }

    /// Runs the plan's ready tasks in order, one at a time, each with a
    /// fresh context, until all are verified, one can't be recovered, or
    /// one needs the person.
    fn run_plan(&self, plan: &str) {
        let Some(view) = self.plan_view(plan) else {
            return;
        };
        if view.runs.is_empty() {
            // Where a resume before the first verified task starts from.
            if let Err(err) = self.environment.checkpoint(plan) {
                return self.end_plan(
                    plan,
                    End(
                        TaskStatus::Failed,
                        format!("{CHECKPOINT_FAILED} before the first task: {err}"),
                    ),
                );
            }
        }
        if self.plan_status(plan, PlanState::Running, None).is_err() {
            return;
        }
        loop {
            if self.stopped() {
                return self.end_plan(plan, End(TaskStatus::Stopped, "Stopped.".into()));
            }
            let Some(view) = self.plan_view(plan) else {
                return;
            };
            let Some(next) = view.next().cloned() else {
                return self.finish_plan(&view);
            };
            let End(status, reason) = self.task(
                view.executor_request(&next),
                next.description.clone(),
                next.criteria.clone(),
                Some((plan, &next.id)),
            );
            if status == TaskStatus::Completed {
                if self
                    .plan_view(plan)
                    .is_some_and(|view| view.awaiting_test_review())
                {
                    let _ = self.plan_status(
                        plan,
                        PlanState::AwaitingApproval,
                        Some(plan::TESTS_REVIEW_NOTE.into()),
                    );
                    return;
                }
                continue;
            }
            let failed = End(status, format!("Task {}: {reason}", next.id));
            if status == TaskStatus::Stopped || !recoverable(&reason) {
                return self.end_plan(plan, failed);
            }
            let Some(view) = self.plan_view(plan) else {
                return;
            };
            let tries = view.recoveries.get(&next.id).copied().unwrap_or(0);
            if tries >= plan::MAX_RECOVERIES {
                return self.end_plan(
                    plan,
                    End(
                        TaskStatus::Failed,
                        format!("Task {}: {reason} (after {tries} recoveries)", next.id),
                    ),
                );
            }
            // Back to the last verified task's files before deciding.
            let point = view.restart_point();
            if let Err(err) = self.environment.restore(&point) {
                return self.end_plan(
                    plan,
                    End(
                        TaskStatus::Failed,
                        format!(
                            "Task {}: {reason}. Then the files could not go back to \
                             checkpoint {point} ({err}); Resume tries that again.",
                            next.id
                        ),
                    ),
                );
            }
            let run = view
                .runs
                .get(&next.id)
                .map(|(run, _, _)| run.clone())
                .unwrap_or_default();
            logging::info!(target: "harness", "{plan}: recovering {}", next.id);
            match self.recover(plan, &next, &run, &reason) {
                Err(end) => return self.end_plan(plan, end),
                Ok(plan::Decision::Blocked { question }) => {
                    let _ = self.plan_status(plan, PlanState::Blocked, Some(question));
                    return;
                }
                Ok(plan::Decision::Replan { diagnosis }) => {
                    let progress = view.progress(&next, &reason, &diagnosis);
                    if let Err(end) = self.draft(plan, &view.request, Some(&progress)) {
                        return self.end_plan(plan, end);
                    }
                }
                Ok(_) => {}
            }
        }
    }

    /// Every task is verified: checks the request's own criteria against
    /// the evidence of the whole plan.
    fn finish_plan(&self, view: &PlanView) {
        let done = view.completed().len();
        if done < view.tasks.len() {
            return self.end_plan(
                &view.id,
                End(
                    TaskStatus::Failed,
                    "No task can start: the tasks before them aren't verified.".into(),
                ),
            );
        }
        self.phase(HarnessPhase::Verifying);
        // Only the verified runs' evidence: a rolled-back try's files are
        // gone. Kept in the order it came, so a later write still counts
        // against an earlier result.
        let kept: BTreeSet<&str> = view
            .runs
            .values()
            .filter(|(_, status, _)| *status == TaskStatus::Completed)
            .map(|(run, _, _)| run.as_str())
            .collect();
        let seen: Vec<Evidence> = {
            let state = lock(&self.shared.state);
            plan::current(&state.entries)
                .unwrap_or(&[])
                .iter()
                .filter_map(|e| match e {
                    Entry::Result { task, evidence, .. } if kept.contains(task.as_str()) => {
                        Some(evidence.clone())
                    }
                    _ => None,
                })
                .flatten()
                .collect()
        };
        let read = |path: &str| self.environment.read_text(path);
        let checks: Vec<Result<String, String>> = view
            .criteria
            .iter()
            .map(|c| c.check(&seen, &read))
            .collect();
        let missing: Vec<&str> = checks
            .iter()
            .filter_map(|c| c.as_ref().err().map(String::as_str))
            .collect();
        let tasks = format!("All {done} tasks verified");
        if missing.is_empty() {
            let proof: Vec<&str> = checks
                .iter()
                .filter_map(|c| c.as_ref().ok().map(String::as_str))
                .collect();
            let reason = if proof.is_empty() {
                format!("{tasks}.")
            } else {
                format!("{tasks}; {}.", proof.join("; "))
            };
            logging::info!(target: "harness", "{} completed", view.id);
            let _ = self.plan_status(&view.id, PlanState::Completed, Some(reason));
        } else {
            let _ = self.plan_status(
                &view.id,
                PlanState::Failed,
                Some(format!("{tasks}, but {}.", missing.join("; "))),
            );
        }
    }

    fn status(&self, task: &str, status: TaskStatus, reason: Option<String>) -> Result<(), End> {
        self.record(Entry::Status {
            task: task.into(),
            status,
            reason,
            at_ms: now_ms(),
        })
    }

    /// The executor loop. Ok and Err both carry how it ended; Err is an
    /// early end (Stop, a limit, a journal failure).
    fn work(
        &self,
        task: &str,
        request: &str,
        criteria: &[Criterion],
        started: Instant,
        planned: bool,
    ) -> Result<End, End> {
        let tools = with_finish(&self.environment.tools());
        let grammar = calls::grammar(&tools);
        let system = prompt::instructions(&tools);
        let overview = self.environment.overview();
        let earlier = {
            let state = lock(&self.shared.state);
            let plan = state.entries.iter().find_map(|e| match e {
                Entry::Task {
                    task: id,
                    plan: Some(plan),
                    ..
                } if id == task => Some(plan.clone()),
                _ => None,
            });
            let mut current = vec![task];
            if let Some(plan) = plan.as_deref() {
                current.push(plan);
            }
            prompt::earlier_work(&state.entries, &current)
        };
        let context = self.brain.describe().context_tokens;
        let limits = &self.limits;
        let mut invalid_streak = 0;
        let mut same_failure: Option<(String, String, u32)> = None;
        let mut shrink = 0u32;
        let mut step = 0u32;
        loop {
            if self.stopped() {
                return Err(End(TaskStatus::Stopped, "Stopped.".into()));
            }
            if step >= limits.max_steps {
                return Err(End(
                    TaskStatus::Failed,
                    format!(
                        "Used all {} steps without verified evidence.",
                        limits.max_steps
                    ),
                ));
            }
            if started.elapsed() >= Duration::from_secs(limits.task_seconds.into()) {
                return Err(End(
                    TaskStatus::Failed,
                    format!("Ran out of time ({} s).", limits.task_seconds),
                ));
            }
            step += 1;
            self.record(Entry::StepStarted {
                task: task.into(),
                step,
                at_ms: now_ms(),
            })?;
            self.phase(HarnessPhase::Thinking);
            let history = self.task_entries(task);
            let budget = prompt::budget(context, limits.turn_tokens, shrink);
            let messages = prompt::messages(
                &system, request, criteria, &overview, &earlier, &history, budget,
            );
            let request_ = ChatRequest {
                model: None,
                messages,
                max_tokens: limits.turn_tokens,
                temperature: 0.0,
            };
            let shared = Arc::clone(&self.shared);
            let out =
                Arc::new(Collect::new().stop_when(move || shared.stop.load(Ordering::SeqCst)));
            lock(&self.shared.state).out = Some(Arc::clone(&out));
            let asked = Instant::now();
            let answer = self.brain.generate(
                request_,
                Some(&grammar),
                Arc::clone(&out) as Arc<dyn TextOut>,
            );
            let model_ms = asked.elapsed().as_millis() as u64;
            if self.stopped() {
                return Err(End(TaskStatus::Stopped, "Stopped.".into()));
            }
            match answer {
                Ok(()) => {}
                Err(AiError::Stopped) => return Err(End(TaskStatus::Stopped, "Stopped.".into())),
                Err(AiError::ContextTooLong { reason }) if shrink < 3 => {
                    // Ask again with a smaller history; the step doesn't count.
                    logging::info!(target: "harness", "context too long, shrinking: {reason}");
                    shrink += 1;
                    step -= 1;
                    continue;
                }
                Err(err) => return Err(End(TaskStatus::Failed, format!("{MODEL_FAILED}: {err}"))),
            }
            let text = out.text();
            let call = match parse(&text, &tools) {
                Ok(call) => call,
                Err(error) => {
                    invalid_streak += 1;
                    self.record(Entry::Invalid {
                        task: task.into(),
                        step,
                        text,
                        error: error.clone(),
                        model_ms,
                    })?;
                    if invalid_streak >= limits.invalid_in_a_row {
                        return Err(End(
                            TaskStatus::Failed,
                            format!("{invalid_streak} answers in a row were not valid calls."),
                        ));
                    }
                    continue;
                }
            };
            invalid_streak = 0;
            self.record(Entry::Answer {
                task: task.into(),
                step,
                text: call.raw.clone(),
                model_ms,
            })?;

            if call.tool == FINISH {
                self.phase(HarnessPhase::Verifying);
                self.status(task, TaskStatus::Verifying, None)?;
                let history = self.task_entries(task);
                let seen: Vec<Evidence> = history
                    .iter()
                    .filter_map(|e| match e {
                        Entry::Result { evidence, .. } => Some(evidence.clone()),
                        _ => None,
                    })
                    .flatten()
                    .collect();
                let read = |path: &str| self.environment.read_text(path);
                let checks: Vec<Result<String, String>> =
                    criteria.iter().map(|c| c.check(&seen, &read)).collect();
                let missing: Vec<&String> =
                    checks.iter().filter_map(|c| c.as_ref().err()).collect();
                if missing.is_empty() {
                    let proof = checks
                        .iter()
                        .filter_map(|c| c.as_ref().ok().cloned())
                        .collect::<Vec<_>>()
                        .join("; ");
                    if let Err(err) = self.environment.checkpoint(task) {
                        logging::warn!(target: "harness", "checkpoint failed: {err}");
                        // A plan resumes from this checkpoint, so its task
                        // can't pass without one; a lone task can.
                        if planned {
                            return Err(End(
                                TaskStatus::Failed,
                                format!("{CHECKPOINT_FAILED}: {err}"),
                            ));
                        }
                    }
                    // A Stop or the time limit that landed during the last
                    // model turn or the checkpoint still wins.
                    if self.stopped() {
                        return Err(End(TaskStatus::Stopped, "Stopped.".into()));
                    }
                    if started.elapsed() >= Duration::from_secs(limits.task_seconds.into()) {
                        return Err(End(
                            TaskStatus::Failed,
                            format!("Ran out of time ({} s).", limits.task_seconds),
                        ));
                    }
                    self.record(Entry::Result {
                        task: task.into(),
                        step,
                        tool: FINISH.into(),
                        ok: true,
                        text: format!("Verified: {proof}."),
                        evidence: Vec::new(),
                        tool_ms: 0,
                    })?;
                    return Ok(End(TaskStatus::Completed, format!("Verified: {proof}.")));
                }
                let missing = missing
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                self.record(Entry::Result {
                    task: task.into(),
                    step,
                    tool: FINISH.into(),
                    ok: false,
                    text: format!(
                        "Not done yet: {missing}. The harness checks the evidence, so keep working until a result shows it."
                    ),
                    evidence: Vec::new(),
                    tool_ms: 0,
                })?;
                self.status(task, TaskStatus::Running, Some(missing))?;
                continue;
            }

            if call.tool == GIVE_UP {
                let reason = call.arg("reason").trim().to_string();
                self.record(Entry::Result {
                    task: task.into(),
                    step,
                    tool: GIVE_UP.into(),
                    ok: false,
                    text: format!("Gave up: {reason}"),
                    evidence: Vec::new(),
                    tool_ms: 0,
                })?;
                return Err(End(TaskStatus::Failed, format!("Bonsai gave up: {reason}")));
            }

            self.phase(HarnessPhase::Acting);
            let acted = Instant::now();
            let outcome = self.environment.execute(&call);
            let tool_ms = acted.elapsed().as_millis() as u64;
            if self.stopped() {
                return Err(End(TaskStatus::Stopped, "Stopped.".into()));
            }
            let text = prompt::cut(&outcome.text, limits.result_chars as usize);
            self.record(Entry::Result {
                task: task.into(),
                step,
                tool: call.tool.clone(),
                ok: outcome.ok,
                text: text.clone(),
                evidence: outcome.evidence,
                tool_ms,
            })?;

            // The stuck rule: the same failing call with the same result.
            let key = call.display(false).replacen(
                &format!(
                    "\"thought\": {}, ",
                    serde_json::Value::String(call.thought.clone())
                ),
                "",
                1,
            );
            same_failure = match (outcome.ok, same_failure.take()) {
                (true, _) => None,
                (false, Some((k, t, n))) if k == key && t == text => Some((k, t, n + 1)),
                (false, _) => Some((key, text, 1)),
            };
            if let Some((_, _, count)) = &same_failure {
                if *count == limits.same_failure_nudge {
                    self.record(Entry::Nudge {
                        task: task.into(),
                        step,
                        text: format!(
                            "You made the same call {count} times and got the same error. Repeating it will not work: read the error, then change the arguments or try a different approach."
                        ),
                    })?;
                } else if *count > limits.same_failure_nudge {
                    return Err(End(
                        TaskStatus::Failed,
                        format!("Stuck: the same failing call {count} times."),
                    ));
                }
            }
        }
    }
}
