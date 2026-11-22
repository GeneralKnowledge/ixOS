//! The task journal: one append-only JSON-lines file per project
//! (`.harness/journal.jsonl`). Every event is written and flushed before
//! the screen shows it, and the harness's state is the journal replayed
//! (docs/bonsai-harness-plan.md, "The task journal").

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::evidence::{Criterion, Evidence};
use super::plan::{PlanState, PlanTask};

/// A task's status, owned by the harness.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Ready,
    Running,
    /// The executor said it was done; the harness is checking the evidence.
    Verifying,
    /// The evidence met every acceptance criterion.
    Completed,
    Failed,
    Stopped,
}

impl TaskStatus {
    pub fn is_final(self) -> bool {
        matches!(
            self,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Stopped
        )
    }
}

/// One line of the journal.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Entry {
    /// A new task.
    Task {
        task: String,
        description: String,
        criteria: Vec<Criterion>,
        model: String,
        at_ms: i64,
        /// The plan this task belongs to, and its id there (H4).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plan: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plan_task: Option<String>,
    },
    Status {
        task: String,
        status: TaskStatus,
        reason: Option<String>,
        at_ms: i64,
    },
    /// Written before the model is asked, so a step the app died in is
    /// known on replay.
    StepStarted { task: String, step: u32, at_ms: i64 },
    /// The model's call, as it wrote it.
    Answer {
        task: String,
        step: u32,
        text: String,
        model_ms: u64,
    },
    /// An answer that was not a valid call, and why.
    Invalid {
        task: String,
        step: u32,
        text: String,
        error: String,
        model_ms: u64,
    },
    /// What the call did.
    Result {
        task: String,
        step: u32,
        tool: String,
        ok: bool,
        text: String,
        evidence: Vec<Evidence>,
        tool_ms: u64,
    },
    /// A hint added to the last result (the stuck rule).
    Nudge {
        task: String,
        step: u32,
        text: String,
    },
    /// A request the planner splits into tasks; `criteria` are the
    /// request's own checks, made once every task is verified.
    PlanRequest {
        plan: String,
        request: String,
        criteria: Vec<Criterion>,
        model: String,
        at_ms: i64,
    },
    /// A plan answer that couldn't be used, and why.
    PlanInvalid {
        plan: String,
        text: String,
        error: String,
        model_ms: u64,
    },
    /// The plan the planner wrote.
    Plan {
        plan: String,
        thought: String,
        tasks: Vec<PlanTask>,
        model_ms: u64,
    },
    PlanStatus {
        plan: String,
        status: PlanState,
        reason: Option<String>,
        at_ms: i64,
    },
}

impl Entry {
    pub fn task(&self) -> &str {
        match self {
            Entry::Task { task, .. }
            | Entry::Status { task, .. }
            | Entry::StepStarted { task, .. }
            | Entry::Answer { task, .. }
            | Entry::Invalid { task, .. }
            | Entry::Result { task, .. }
            | Entry::Nudge { task, .. } => task,
            Entry::PlanRequest { plan, .. }
            | Entry::PlanInvalid { plan, .. }
            | Entry::Plan { plan, .. }
            | Entry::PlanStatus { plan, .. } => plan,
        }
    }
}

pub struct Journal {
    path: PathBuf,
    file: File,
}

impl Journal {
    pub const FILE: &'static str = ".harness/journal.jsonl";

    /// Opens (or creates) the journal of the project at `root`.
    pub fn open(root: &Path) -> std::io::Result<Self> {
        let path = root.join(Self::FILE);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;
        // A line cut off when the app died: end it, so the next entry
        // starts on a line of its own.
        let length = file.metadata()?.len();
        if length > 0 {
            let mut last = [0u8; 1];
            file.seek(SeekFrom::Start(length - 1))?;
            file.read_exact(&mut last)?;
            if last[0] != b'\n' {
                file.write_all(b"\n")?;
            }
        }
        Ok(Self { path, file })
    }

    /// Writes one entry and flushes it to storage.
    pub fn append(&mut self, entry: &Entry) -> std::io::Result<()> {
        let mut line = serde_json::to_string(entry).map_err(std::io::Error::other)?;
        line.push('\n');
        self.file.write_all(line.as_bytes())?;
        self.file.sync_data()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every entry of the project's journal, oldest first. A line that
    /// doesn't parse (the app died while writing it) is skipped.
    pub fn replay(root: &Path) -> Vec<Entry> {
        let Ok(file) = File::open(root.join(Self::FILE)) else {
            return Vec::new();
        };
        // Split on raw bytes: a line cut inside a multibyte character is
        // skipped like any other unreadable line, and replay carries on.
        BufReader::new(file)
            .split(b'\n')
            .map_while(Result::ok)
            .filter_map(|line| serde_json::from_slice(&line).ok())
            .collect()
    }
}

/// The entries of the last task in `entries`, from its `Task` line on.
pub fn last_task(entries: &[Entry]) -> &[Entry] {
    let start = entries
        .iter()
        .rposition(|e| matches!(e, Entry::Task { .. }))
        .unwrap_or(entries.len());
    &entries[start..]
}

/// The last status of the task whose entries these are.
pub fn status_of(task: &[Entry]) -> Option<(TaskStatus, Option<String>)> {
    task.iter().rev().find_map(|e| match e {
        Entry::Status { status, reason, .. } => Some((*status, reason.clone())),
        _ => None,
    })
}

/// Steps the app died in: started, but with no answer or result after.
pub fn interrupted_steps(task: &[Entry]) -> Vec<u32> {
    let mut open: Vec<u32> = Vec::new();
    for entry in task {
        match entry {
            Entry::StepStarted { step, .. } => open.push(*step),
            Entry::Result { step, .. } | Entry::Invalid { step, .. } => open.retain(|s| s != step),
            _ => {}
        }
    }
    open
}
