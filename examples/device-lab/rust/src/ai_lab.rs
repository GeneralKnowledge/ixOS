//! AI Lab: repeatable experiments with a language model, with the timings
//! and device readings that make one run comparable with the next
//! (docs/research/esim-ai-lab-plan.md, phase A0).
//!
//! The model is Forge AI's (`native_services!(ai)`; on iOS Apple's on-device
//! model through `ForgeAppleModel`). This module only runs it: one request at
//! a time on a worker thread, a Stop that really stops it, and a record of
//! each run (time to first text, total time, a speed estimate, the app's
//! memory and the thermal state before and after).
//!
//! Structured output is the one experiment Swift runs itself, because
//! Foundation Models' guided generation has no Rust shape: the host reports
//! it through [`AiLab::begin_external`], [`AiLab::update_external`] and
//! [`AiLab::finish_external`], so it gets the same record.
//!
//! SwiftUI polls [`AiLab::snapshot`] while the screen is open; a snapshot
//! only copies memory.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use forge_runtime::{lock, logging, now_ms};
use forge_services::ai::{text, AiError, ChatRequest, LanguageModel, TextOut};

use crate::native::ai::{AiModel, AiModelInfo, HostModel};
use crate::native::device_status::{DeviceStatusService, ThermalState};

/// Runs kept on screen, newest first.
pub const HISTORY_LIMIT: usize = 20;

/// What an experiment tests.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AiExperimentKind {
    /// Free text from a short prompt.
    Generation,
    /// Following exact instructions (a format, a length, a list).
    Instructions,
    /// Summarising a passage the prompt carries.
    Summary,
    /// A typed answer through guided generation (run by the host).
    Structured,
    /// A long answer, to watch streaming and try Stop.
    LongAnswer,
    /// The user's own prompt.
    Custom,
}

/// One ready-made experiment: the same request every time, so runs can be
/// compared.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AiExperiment {
    pub kind: AiExperimentKind,
    pub title: String,
    /// What it checks, for the screen.
    pub detail: String,
    pub instructions: String,
    pub prompt: String,
    pub max_tokens: u32,
    pub temperature: f64,
}

/// Where a run is.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiRunState {
    Running,
    Finished,
    Stopped,
    /// The model declined, with an error or in words.
    Refused,
    Failed,
}

/// One run and what it measured.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AiRun {
    pub id: u64,
    pub kind: AiExperimentKind,
    pub model_id: String,
    pub prompt: String,
    pub answer: String,
    pub state: AiRunState,
    /// Why it failed or was refused.
    pub error: Option<String>,
    pub started_at_ms: i64,
    /// From the request to the first text.
    pub first_text_ms: Option<u64>,
    /// From the request to the end (so far, while running).
    pub total_ms: u64,
    /// How many pieces the answer arrived in (1 means no streaming).
    pub pieces: u64,
    /// Estimated tokens in the answer (about 4 characters each).
    pub answer_tokens: u64,
    /// Estimated tokens per second after the first text.
    pub tokens_per_second: Option<f64>,
    /// The app's memory footprint before and after.
    pub memory_before_bytes: Option<u64>,
    pub memory_after_bytes: Option<u64>,
    pub thermal_before: Option<ThermalState>,
    pub thermal_after: Option<ThermalState>,
}

/// Everything on the AI Lab screen.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AiLabSnapshot {
    pub model: AiModelInfo,
    /// The run in progress, if any.
    pub running: Option<AiRun>,
    /// Finished runs, newest first.
    pub history: Vec<AiRun>,
}

#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum AiLabError {
    #[error("A run is still going. Stop it first.")]
    Busy,
    #[error("Write a prompt first.")]
    EmptyPrompt,
    #[error("That run has already ended.")]
    NotRunning,
}

/// The ready-made experiments, in screen order.
#[uniffi::export]
pub fn ai_lab_experiments() -> Vec<AiExperiment> {
    vec![
        AiExperiment {
            kind: AiExperimentKind::Generation,
            title: "Short Answer".into(),
            detail: "Time to first text and speed on a small request.".into(),
            instructions: "Answer in two or three sentences.".into(),
            prompt: "Why does the sky look blue during the day and red at sunset?".into(),
            max_tokens: 160,
            temperature: 0.7,
        },
        AiExperiment {
            kind: AiExperimentKind::Instructions,
            title: "Follow Instructions".into(),
            detail: "Does it keep an exact format: five lines, numbered, no extra text.".into(),
            instructions: "Follow the format exactly. Write nothing before or after the list."
                .into(),
            prompt: "List five fruits, one per line, numbered 1 to 5, each in lower case with \
                     no other words."
                .into(),
            max_tokens: 80,
            temperature: 0.2,
        },
        AiExperiment {
            kind: AiExperimentKind::Summary,
            title: "Summarise".into(),
            detail: "One sentence from a paragraph it is given.".into(),
            instructions: "Summarise the passage in one sentence of at most 25 words. Use only \
                           facts from the passage."
                .into(),
            prompt: SUMMARY_PASSAGE.into(),
            max_tokens: 80,
            temperature: 0.3,
        },
        AiExperiment {
            kind: AiExperimentKind::Structured,
            title: "Structured Output".into(),
            detail: "A typed answer (name, minutes, ingredients) instead of free text.".into(),
            instructions: "You suggest simple recipes.".into(),
            prompt: "Suggest a quick vegetarian lunch.".into(),
            max_tokens: 300,
            temperature: 0.5,
        },
        AiExperiment {
            kind: AiExperimentKind::LongAnswer,
            title: "Long Answer".into(),
            detail: "Watch it stream, then try Stop part-way.".into(),
            instructions: "Write clearly, in several paragraphs.".into(),
            prompt: "Explain how a bicycle stays upright, from the basics to the details.".into(),
            max_tokens: 900,
            temperature: 0.7,
        },
    ]
}

const SUMMARY_PASSAGE: &str = "The lighthouse on Gull Rock was built in 1874 after three ships \
    were lost on the reef in a single winter. Its keeper, Ada Lowe, lived there for thirty-one \
    years with her two brothers, trimming the wick every four hours and logging each passing \
    vessel. When the light was automated in 1962, the island was left to the seabirds, and \
    today the tower is open to visitors on summer weekends.";

/// The lab the host opens once. Thread-safe.
#[derive(uniffi::Object)]
pub struct AiLab {
    model: Arc<dyn LanguageModel>,
    device: Option<DeviceStatusService>,
    state: Arc<Mutex<LabState>>,
}

#[derive(Default)]
struct LabState {
    next_id: u64,
    running: Option<Running>,
    history: Vec<AiRun>,
    model: Option<AiModelInfo>,
}

struct Running {
    run: AiRun,
    started: Instant,
    out: Arc<RunOut>,
}

#[uniffi::export]
impl AiLab {
    /// Runs experiments on `model` (iOS: `ForgeAppleModel()`).
    #[uniffi::constructor]
    pub fn new(model: Arc<dyn AiModel>) -> Arc<Self> {
        Arc::new(Self::with_model(
            Arc::new(HostModel(model)),
            DeviceStatusService::installed().ok(),
        ))
    }

    /// Asks the model again whether it can answer (Apple Intelligence
    /// turned on, its model downloaded). Cheap.
    pub fn refresh_model(&self) -> AiModelInfo {
        let info: AiModelInfo = self.model.describe().into();
        lock(&self.state).model = Some(info.clone());
        info
    }

    pub fn snapshot(&self) -> AiLabSnapshot {
        let model = lock(&self.state).model.clone();
        let model = model.unwrap_or_else(|| self.refresh_model());
        let state = lock(&self.state);
        AiLabSnapshot {
            model,
            running: state.running.as_ref().map(|r| {
                let mut run = r.run.clone();
                run.answer = r.out.text();
                run.pieces = r.out.pieces();
                run.first_text_ms = r.out.first_text_ms();
                run.total_ms = r.started.elapsed().as_millis() as u64;
                run
            }),
            history: state.history.clone(),
        }
    }

    /// Starts `prompt` on the model and returns the run's id at once; the
    /// answer streams into [`AiLab::snapshot`].
    pub fn run(
        &self,
        kind: AiExperimentKind,
        instructions: String,
        prompt: String,
        max_tokens: u32,
        temperature: f64,
    ) -> Result<u64, AiLabError> {
        let request = ChatRequest::new(&instructions, prompt.trim(), max_tokens.max(1))
            .temperature(temperature.clamp(0.0, 1.5));
        let (id, out) = self.begin(kind, prompt)?;
        let model = Arc::clone(&self.model);
        let lab = self.handle();
        let spawned = std::thread::Builder::new()
            .name("ai-lab".into())
            .spawn(move || {
                let result = model.generate(request, Arc::clone(&out) as Arc<dyn TextOut>);
                lab.finish(id, result.err());
            });
        if let Err(err) = spawned {
            self.handle().finish(
                id,
                Some(AiError::Engine {
                    reason: err.to_string(),
                }),
            );
        }
        Ok(id)
    }

    /// Stops the run in progress. The model stops at its next piece.
    pub fn stop(&self) {
        if let Some(running) = &lock(&self.state).running {
            running.out.stop();
        }
    }

    /// Whether the run in progress was asked to stop (for a run the host
    /// does itself).
    pub fn stop_requested(&self) -> bool {
        lock(&self.state)
            .running
            .as_ref()
            .is_some_and(|r| r.out.is_stopped())
    }

    /// A run the host does itself (structured output): starts the record
    /// and its clock.
    pub fn begin_external(
        &self,
        kind: AiExperimentKind,
        prompt: String,
    ) -> Result<u64, AiLabError> {
        self.begin(kind, prompt).map(|(id, _)| id)
    }

    /// The host's answer so far, as text. The first call marks the time to
    /// first text.
    pub fn update_external(&self, so_far: String) {
        if let Some(running) = &lock(&self.state).running {
            running.out.replace(&so_far);
        }
    }

    /// Ends the host's run: `error` is `None` when it finished,
    /// `refused` when the model declined.
    pub fn finish_external(
        &self,
        answer: String,
        error: Option<String>,
        refused: bool,
    ) -> Result<(), AiLabError> {
        let id = {
            let state = lock(&self.state);
            let running = state.running.as_ref().ok_or(AiLabError::NotRunning)?;
            running.out.replace(&answer);
            running.run.id
        };
        let error = match (error, refused) {
            (Some(reason), true) => Some(AiError::Refused { reason }),
            (Some(reason), false) => Some(AiError::Engine { reason }),
            (None, _) if self.stop_requested() => Some(AiError::Stopped),
            (None, _) => None,
        };
        self.handle().finish(id, error);
        Ok(())
    }

    pub fn clear_history(&self) {
        lock(&self.state).history.clear();
    }
}

impl AiLab {
    pub fn with_model(model: Arc<dyn LanguageModel>, device: Option<DeviceStatusService>) -> Self {
        forge_runtime::logging::init();
        Self {
            model,
            device,
            state: Arc::new(Mutex::new(LabState::default())),
        }
    }

    fn handle(&self) -> Handle {
        Handle {
            device: self.device.clone(),
            state: Arc::clone(&self.state),
        }
    }

    fn begin(
        &self,
        kind: AiExperimentKind,
        prompt: String,
    ) -> Result<(u64, Arc<RunOut>), AiLabError> {
        if prompt.trim().is_empty() {
            return Err(AiLabError::EmptyPrompt);
        }
        let model_id = self.model.describe().id;
        let device = self.device.as_ref().map(|d| d.status());
        let mut state = lock(&self.state);
        if state.running.is_some() {
            return Err(AiLabError::Busy);
        }
        state.next_id += 1;
        let id = state.next_id;
        let started = Instant::now();
        let out = Arc::new(RunOut::new(started));
        state.running = Some(Running {
            run: AiRun {
                id,
                kind,
                model_id,
                prompt,
                answer: String::new(),
                state: AiRunState::Running,
                error: None,
                started_at_ms: now_ms(),
                first_text_ms: None,
                total_ms: 0,
                pieces: 0,
                answer_tokens: 0,
                tokens_per_second: None,
                memory_before_bytes: device.as_ref().and_then(|s| s.memory_footprint_bytes),
                memory_after_bytes: None,
                thermal_before: device.as_ref().map(|s| s.thermal_state),
                thermal_after: None,
            },
            started,
            out: Arc::clone(&out),
        });
        logging::info!(target: "ai_lab", "run {id} ({kind:?}) started");
        Ok((id, out))
    }
}

/// What the worker thread needs to end a run.
struct Handle {
    device: Option<DeviceStatusService>,
    state: Arc<Mutex<LabState>>,
}

impl Handle {
    fn finish(&self, id: u64, error: Option<AiError>) {
        let device = self.device.as_ref().map(|d| d.status());
        let mut state = lock(&self.state);
        if state.running.as_ref().map(|r| r.run.id) != Some(id) {
            return;
        }
        let Some(Running {
            mut run,
            started,
            out,
        }) = state.running.take()
        else {
            return;
        };
        run.total_ms = started.elapsed().as_millis() as u64;
        run.answer = out.text();
        run.pieces = out.pieces();
        run.first_text_ms = out.first_text_ms();
        run.answer_tokens = text::estimate_tokens(&run.answer) as u64;
        run.tokens_per_second =
            tokens_per_second(run.answer_tokens, run.first_text_ms, run.total_ms);
        run.memory_after_bytes = device.as_ref().and_then(|s| s.memory_footprint_bytes);
        run.thermal_after = device.as_ref().map(|s| s.thermal_state);
        let stopped = out.is_stopped();
        (run.state, run.error) = outcome(&run.answer, error, stopped);
        logging::info!(
            target: "ai_lab",
            "run {id} ended {:?} after {} ms (first text {:?} ms)",
            run.state,
            run.total_ms,
            run.first_text_ms
        );
        state.history.insert(0, run);
        state.history.truncate(HISTORY_LIMIT);
    }
}

/// How a run ended. Apple's model often declines in words rather than with
/// an error, so a whole answer that only declines counts as a refusal.
fn outcome(answer: &str, error: Option<AiError>, stopped: bool) -> (AiRunState, Option<String>) {
    match error {
        Some(AiError::Stopped) => (AiRunState::Stopped, None),
        Some(AiError::Refused { reason }) => (AiRunState::Refused, Some(reason)),
        // A model may end with an error of its own when it is stopped.
        Some(_) if stopped => (AiRunState::Stopped, None),
        Some(other) => (AiRunState::Failed, Some(other.to_string())),
        None if stopped => (AiRunState::Stopped, None),
        None if !answer.trim().is_empty() && text::without_declines(answer).is_empty() => (
            AiRunState::Refused,
            Some("The model declined in its answer.".into()),
        ),
        None => (AiRunState::Finished, None),
    }
}

/// Tokens per second after the first text, when there was enough to time.
fn tokens_per_second(tokens: u64, first_text_ms: Option<u64>, total_ms: u64) -> Option<f64> {
    let first = first_text_ms?;
    let streaming_ms = total_ms.checked_sub(first)?;
    (tokens > 1 && streaming_ms >= 50).then(|| tokens as f64 * 1000.0 / streaming_ms as f64)
}

/// The run's answer as it arrives, with its stop switch and the time of the
/// first text.
struct RunOut {
    started: Instant,
    inner: Mutex<OutState>,
    stopped: std::sync::atomic::AtomicBool,
}

#[derive(Default)]
struct OutState {
    text: String,
    pieces: u64,
    first_text_ms: Option<u64>,
}

impl RunOut {
    fn new(started: Instant) -> Self {
        Self {
            started,
            inner: Mutex::new(OutState::default()),
            stopped: Default::default(),
        }
    }

    fn stop(&self) {
        self.stopped
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn text(&self) -> String {
        lock(&self.inner).text.clone()
    }

    fn pieces(&self) -> u64 {
        lock(&self.inner).pieces
    }

    fn first_text_ms(&self) -> Option<u64> {
        lock(&self.inner).first_text_ms
    }

    /// The host's whole answer so far (structured output reports snapshots,
    /// not pieces).
    fn replace(&self, so_far: &str) {
        let mut inner = lock(&self.inner);
        if so_far == inner.text {
            return;
        }
        if inner.first_text_ms.is_none() && !so_far.is_empty() {
            inner.first_text_ms = Some(self.started.elapsed().as_millis() as u64);
        }
        inner.text = so_far.to_string();
        inner.pieces += 1;
    }
}

impl TextOut for RunOut {
    fn push(&self, delta: &str) -> bool {
        if self.is_stopped() {
            return false;
        }
        let mut inner = lock(&self.inner);
        if inner.first_text_ms.is_none() && !delta.is_empty() {
            inner.first_text_ms = Some(self.started.elapsed().as_millis() as u64);
        }
        inner.text.push_str(delta);
        inner.pieces += 1;
        true
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use forge_services::ai::ScriptedModel;

    use super::*;

    fn lab(model: ScriptedModel) -> AiLab {
        AiLab::with_model(Arc::new(model), None)
    }

    fn wait_for_end(lab: &AiLab) -> AiRun {
        for _ in 0..500 {
            let snapshot = lab.snapshot();
            if snapshot.running.is_none() {
                if let Some(run) = snapshot.history.first() {
                    return run.clone();
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the run never ended");
    }

    fn short(lab: &AiLab, prompt: &str) -> Result<u64, AiLabError> {
        lab.run(
            AiExperimentKind::Custom,
            String::new(),
            prompt.into(),
            50,
            0.5,
        )
    }

    #[test]
    fn a_run_streams_and_is_recorded() {
        let lab = lab(ScriptedModel::new([
            "The sky is blue because of scattering.",
        ]));
        let id = short(&lab, "Why is the sky blue?").unwrap();
        let run = wait_for_end(&lab);
        assert_eq!(run.id, id);
        assert_eq!(run.state, AiRunState::Finished);
        assert_eq!(run.answer, "The sky is blue because of scattering.");
        assert!(run.pieces >= 1);
        assert!(run.first_text_ms.is_some());
        assert!(run.answer_tokens > 0);
        assert_eq!(run.model_id, lab.snapshot().model.id);
    }

    #[test]
    fn one_run_at_a_time_and_stop_ends_it() {
        let model = ScriptedModel::new(["one two three four five six seven eight nine ten"])
            .pause(Duration::from_millis(20));
        let lab = lab(model);
        short(&lab, "Count").unwrap();
        assert_eq!(short(&lab, "Again"), Err(AiLabError::Busy));
        std::thread::sleep(Duration::from_millis(30));
        lab.stop();
        let run = wait_for_end(&lab);
        assert_eq!(run.state, AiRunState::Stopped);
        assert_ne!(
            run.answer,
            "one two three four five six seven eight nine ten"
        );
        // The lab is free again.
        assert!(short(&lab, "Again").is_ok());
    }

    #[test]
    fn an_empty_prompt_is_refused_before_the_model() {
        let lab = lab(ScriptedModel::new(["never"]));
        assert_eq!(short(&lab, "   "), Err(AiLabError::EmptyPrompt));
        assert!(lab.snapshot().history.is_empty());
    }

    #[test]
    fn errors_and_declines_in_words_are_shown_apart() {
        let lab = lab(ScriptedModel::with_results([
            Err(AiError::Refused {
                reason: "Apple Intelligence declined to answer this.".into(),
            }),
            Ok("I'm sorry, but I can't help with that.".into()),
            Err(AiError::Unavailable {
                reason: "Turn on Apple Intelligence.".into(),
            }),
        ]));
        short(&lab, "a").unwrap();
        let refused = wait_for_end(&lab);
        assert_eq!(refused.state, AiRunState::Refused);
        short(&lab, "b").unwrap();
        let in_words = wait_for_end(&lab);
        assert_eq!(in_words.state, AiRunState::Refused);
        short(&lab, "c").unwrap();
        let failed = wait_for_end(&lab);
        assert_eq!(failed.state, AiRunState::Failed);
        assert!(failed.error.unwrap().contains("Turn on Apple Intelligence"));
        assert_eq!(lab.snapshot().history.len(), 3);
    }

    #[test]
    fn the_host_reports_structured_runs() {
        let lab = lab(ScriptedModel::new(["unused"]));
        lab.begin_external(AiExperimentKind::Structured, "Lunch".into())
            .unwrap();
        assert_eq!(
            lab.begin_external(AiExperimentKind::Structured, "Again".into()),
            Err(AiLabError::Busy)
        );
        lab.update_external("{\"name\":".into());
        assert_eq!(lab.snapshot().running.unwrap().answer, "{\"name\":");
        lab.finish_external("{\"name\":\"Salad\"}".into(), None, false)
            .unwrap();
        let run = lab.snapshot().history[0].clone();
        assert_eq!(run.state, AiRunState::Finished);
        assert_eq!(run.kind, AiExperimentKind::Structured);
        assert!(run.first_text_ms.is_some());
        assert_eq!(
            lab.finish_external(String::new(), None, false),
            Err(AiLabError::NotRunning)
        );
    }

    #[test]
    fn history_keeps_the_newest_runs() {
        let replies: Vec<String> = (0..HISTORY_LIMIT + 3)
            .map(|i| format!("answer {i}"))
            .collect();
        let lab = lab(ScriptedModel::new(replies));
        for _ in 0..HISTORY_LIMIT + 3 {
            short(&lab, "go").unwrap();
            wait_for_end(&lab);
        }
        let history = lab.snapshot().history;
        assert_eq!(history.len(), HISTORY_LIMIT);
        assert!(history[0].id > history[1].id);
        lab.clear_history();
        assert!(lab.snapshot().history.is_empty());
    }

    #[test]
    fn speed_needs_enough_to_time() {
        assert_eq!(tokens_per_second(100, Some(500), 1500), Some(100.0));
        assert_eq!(tokens_per_second(100, None, 1500), None);
        assert_eq!(tokens_per_second(1, Some(10), 1500), None);
        assert_eq!(tokens_per_second(100, Some(1490), 1500), None);
    }

    #[test]
    fn every_experiment_has_a_prompt() {
        let experiments = ai_lab_experiments();
        assert!(experiments
            .iter()
            .any(|e| e.kind == AiExperimentKind::Structured));
        assert!(experiments
            .iter()
            .all(|e| !e.prompt.trim().is_empty() && e.max_tokens > 0));
    }
}
