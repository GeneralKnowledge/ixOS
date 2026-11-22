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
mod prompt;
pub mod python_lab;
pub mod samples;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use forge_runtime::{lock, logging, now_ms};
use forge_services::ai::{
    AiError, ChatRequest, Collect, LanguageModel, ModelInfo, ScriptedModel, TextOut,
};

use crate::native::ai::{AiModelError, AiModelInfo, AiRequest, TextSink};
use calls::{parse, with_finish, ToolCall, FINISH};
use evidence::{Criterion, Evidence, LadderTask};
use journal::{Entry, Journal, TaskStatus};
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
                .pause(Duration::from_millis(400)),
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
                acceptance: t.criteria.iter().map(Criterion::describe).collect(),
            })
            .collect()
    }

    pub fn snapshot(&self) -> HarnessSnapshot {
        let state = lock(&self.shared.state);
        let task = journal::last_task(&state.entries);
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
        }
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
        self.start_request(Arc::new(HostBrain(model)), request, &project)
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
        prompt::transcript(
            state.project.as_deref().unwrap_or("?"),
            journal::last_task(&state.entries),
        )
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
        self.launch(
            brain,
            Arc::new(lab),
            task.id,
            task.request.into(),
            task.criteria,
        )
    }

    pub fn start_request(
        &self,
        brain: Arc<dyn Brain>,
        request: String,
        project: &str,
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
        self.launch(
            brain,
            Arc::new(lab),
            project,
            request,
            vec![Criterion::WorkRuns],
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

    fn release(&self) {
        lock(&self.shared.state).running = false;
    }

    fn launch(
        &self,
        brain: Arc<dyn Brain>,
        lab: Arc<PythonLab>,
        project: &str,
        request: String,
        criteria: Vec<Criterion>,
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
        {
            let mut state = lock(&self.shared.state);
            state.entries = Journal::replay(lab.root());
            state.journal = Some(journal);
            state.project = Some(project.to_string());
            state.root = Some(lab.root().to_path_buf());
            state.started = Some(Instant::now());
            state.phase = HarnessPhase::Thinking;
            state.environment = Some(Arc::clone(&lab) as Arc<dyn Environment>);
        }
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
                run.task(request, criteria);
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

    fn task(&self, request: String, criteria: Vec<Criterion>) {
        let task = format!("task-{}", now_ms());
        let started = Instant::now();
        let opened = self
            .record(Entry::Task {
                task: task.clone(),
                description: request.clone(),
                criteria: criteria.clone(),
                model: self.brain.describe().id,
                at_ms: now_ms(),
            })
            .and_then(|_| self.status(&task, TaskStatus::Running, None));
        if opened.is_err() {
            return;
        }
        logging::info!(target: "harness", "{task} started");
        let End(status, reason) = match self.work(&task, &request, &criteria, started) {
            Ok(end) | Err(end) => end,
        };
        logging::info!(target: "harness", "{task} ended {status:?}");
        let _ = self.status(&task, status, Some(reason));
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
    ) -> Result<End, End> {
        let tools = with_finish(&self.environment.tools());
        let grammar = calls::grammar(&tools);
        let system = prompt::instructions(&tools);
        let overview = self.environment.overview();
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
            let messages =
                prompt::messages(&system, request, criteria, &overview, &history, budget);
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
                Err(err) => {
                    return Err(End(TaskStatus::Failed, format!("The model failed: {err}")))
                }
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
