//! Good Dog's Rust core: the app the SwiftUI host presents. It owns the
//! dog's profile, today's plan, practice sessions and skill progress, and
//! serves the exercise library and glossary. The planning rules come from
//! `good-dog-core` (the engine ported from the Good Dog web app); this crate
//! adds saving, the day boundary and the API the host calls.
//!
//! Every command that changes data saves before it returns and then
//! publishes [`AppEvent::DataChanged`]. Reads are quick in-memory calls the
//! host may make from the main thread.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use forge_runtime::{background, lock, EventHub, Lifecycle, Phase};
use forge_storage::{JsonFile, StorageError};
use good_dog_core::dog_training::{
    bundled_exercises, bundled_glossary, bundled_topics, parse_term_links, plain_text,
    ExerciseContent, GlossaryTerm,
};

mod model;
#[cfg(test)]
mod tests;
mod training;

pub use model::*;
use training::{exercise, feedback_message, validated_profile, Stored};

uniffi::setup_scaffolding!("forge_ffi");

// Native services (Keychain, app directories, local notifications,
// permissions, deep links) as `crate::native`. Daily reminders (GD2) will use
// the notifications. See docs/native-services.md.
forge_services::native_services!();

forge_runtime::app_shell! {
    phase: AppPhase,
    cancellation: Cancellation,
    events: AppEvents(AppEvent::EventsDropped),
}

forge_runtime::log_export!();

/// Schema version of `gooddog.json`. Bump it, and add a migration, when
/// [`Stored`] changes shape.
const STORE_VERSION: u32 = 1;

/// Something the host may want to react to.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum AppEvent {
    /// Saved data changed: re-read whatever is on screen.
    DataChanged,
    /// This subscriber fell behind and missed `count` events; re-read everything.
    EventsDropped { count: u64 },
    /// The app changed phase. `resumed` is true on the way back from the
    /// background, which may be a new day: re-read today's plan.
    PhaseChanged { phase: AppPhase, resumed: bool },
}

/// Every error the API returns. `reason` fields are diagnostics, not UI copy.
#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum AppError {
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    #[error("not found: {what}")]
    NotFound { what: String },
    /// There is no dog profile yet; onboarding comes first.
    #[error("no dog profile yet")]
    NoDog,
    /// Nothing else suits the dog right now, or the activity was swapped twice.
    #[error("no alternative activity")]
    NoAlternative,
    /// The data file could not be read or written.
    #[error("storage error: {reason}")]
    Storage { reason: String },
    /// `shutdown()` was called; the instance accepts no more work.
    #[error("the app has been shut down")]
    ShutDown,
}

impl From<StorageError> for AppError {
    fn from(err: StorageError) -> Self {
        Self::Storage {
            reason: err.to_string(),
        }
    }
}

/// The app instance the host creates once and shares. Thread-safe.
#[derive(uniffi::Object)]
pub struct AppCore {
    state: Arc<Mutex<Stored>>,
    file: Arc<JsonFile<Stored>>,
    events: EventHub<AppEvent>,
    shut_down: Arc<AtomicBool>,
    lifecycle: Lifecycle,
    #[cfg(feature = "devtools")]
    _devtools: Option<forge_devtools::DevTools>,
}

#[uniffi::export]
impl AppCore {
    /// Opens the app's data in `storage_directory` (created if missing).
    /// Fails with `Storage` if the saved data can't be used; it is left untouched.
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Result<Arc<Self>, AppError> {
        forge_runtime::logging::init();
        if storage_directory.is_empty() {
            return Err(AppError::InvalidInput {
                reason: "storage directory must not be empty".into(),
            });
        }
        let storage_directory = PathBuf::from(storage_directory);
        std::fs::create_dir_all(&storage_directory).map_err(|e| AppError::Storage {
            reason: e.to_string(),
        })?;
        let file = JsonFile::new(storage_directory.join("gooddog.json"), STORE_VERSION);
        let stored: Stored = file.load()?.unwrap_or_default();
        // Never log the dog's name or anything the owner typed.
        forge_runtime::logging::info!(
            "opened: profile {}, {} sessions",
            stored.dog.is_some(),
            stored.sessions.len()
        );
        let state = Arc::new(Mutex::new(stored));
        let lifecycle = Lifecycle::new();
        Ok(Arc::new(Self {
            #[cfg(feature = "devtools")]
            _devtools: devtools::start(&storage_directory, &state, &lifecycle),
            state,
            file: Arc::new(file),
            events: EventHub::new(),
            shut_down: Arc::new(AtomicBool::new(false)),
            lifecycle,
        }))
    }

    // MARK: The dog

    /// The dog's profile, or `None` before onboarding.
    pub fn dog(&self) -> Option<DogProfile> {
        lock(&self.state).dog.clone()
    }

    /// The longest name `save_dog` accepts.
    pub fn max_name_length(&self) -> u32 {
        training::MAX_NAME_CHARS as u32
    }

    /// Creates or changes the profile. The name is trimmed; an empty or
    /// over-long name is `InvalidInput`. A plan not yet started is rebuilt.
    pub async fn save_dog(
        &self,
        profile: DogProfile,
        now: LocalTime,
    ) -> Result<DogProfile, AppError> {
        let profile = validated_profile(profile)?;
        self.change("save-dog", move |stored| {
            stored.set_profile(profile.clone(), now);
            Ok(profile)
        })
        .await
    }

    // MARK: Today

    /// Today's plan, built the first time it is asked for each day and kept
    /// for the rest of it. `NoDog` before onboarding.
    pub async fn today(&self, now: LocalTime) -> Result<TodayPlan, AppError> {
        {
            let state = lock(&self.state);
            if let Some(plan) = state.today(now) {
                return Ok(plan);
            }
            state.dog.as_ref().ok_or(AppError::NoDog)?;
        }
        self.change("today", move |stored| {
            stored.ensure_today(now)?;
            Ok(stored.today(now).expect("just built"))
        })
        .await
    }

    /// Swaps today's plan for one short activity ("only a few minutes
    /// today"). Refused once anything has been practised.
    pub async fn shorten_today(&self, now: LocalTime) -> Result<TodayPlan, AppError> {
        self.change("shorten-today", move |stored| {
            stored.shorten_today(now)?;
            Ok(stored.today(now).expect("ensured"))
        })
        .await
    }

    /// Replaces one of today's activities with something different. At
    /// most twice per activity; `NoAlternative` when nothing else suits.
    pub async fn swap_activity(
        &self,
        activity_id: u64,
        now: LocalTime,
    ) -> Result<TodayPlan, AppError> {
        self.change("swap-activity", move |stored| {
            stored.swap(activity_id, now)?;
            Ok(stored.today(now).expect("swapped today"))
        })
        .await
    }

    /// Records how a practice session went and moves the skill on.
    /// `activity_id` ties it to one of today's activities; leave it out for
    /// extra practice started from Learn. A welfare concern rests the skill
    /// for a few days and keeps plans gentle meanwhile.
    pub async fn record_session(
        &self,
        exercise_id: String,
        activity_id: Option<u64>,
        result: SessionResult,
        welfare_concern: bool,
        now: LocalTime,
    ) -> Result<SessionFeedback, AppError> {
        self.change("record-session", move |stored| {
            let objective =
                stored.record(&exercise_id, activity_id, result, welfare_concern, now)?;
            let skill = stored.skill(&objective);
            let name = stored
                .dog
                .as_ref()
                .map(|d| d.name.clone())
                .unwrap_or_default();
            Ok(SessionFeedback {
                message: feedback_message(&name, result, welfare_concern, skill.level),
                skill,
            })
        })
        .await
    }

    // MARK: Progress

    pub fn progress(&self, now: LocalTime) -> ProgressSummary {
        lock(&self.state).progress_summary(now)
    }

    /// Recorded sessions, newest first.
    pub fn history(&self, limit: u32) -> Vec<SessionRecord> {
        lock(&self.state).history(limit as usize)
    }

    /// Deletes the profile, plans, sessions and progress.
    pub async fn erase_all(&self) -> Result<(), AppError> {
        self.change("erase-all", |stored| {
            *stored = Stored::default();
            Ok(())
        })
        .await
    }

    // MARK: Learn

    /// One exercise in full, with the dog's progress on its skill.
    pub fn exercise(&self, exercise_id: String) -> Result<ExerciseDetail, AppError> {
        let e = exercise(&exercise_id).ok_or(AppError::NotFound {
            what: "that exercise".into(),
        })?;
        let glossary = bundled_glossary();
        let linked = |text: &str| LinkedText::from(parse_term_links(text, glossary));
        let skill = lock(&self.state).skill(&e.core.learning_objective_id);
        Ok(ExerciseDetail {
            id: e.core.id.clone(),
            title: e.title.clone(),
            summary: e.summary.clone(),
            minutes: minutes(e),
            purpose: linked(&e.purpose),
            preparation: linked(&e.preparation),
            steps: e.steps.iter().map(|s| linked(s)).collect(),
            look_for: linked(&e.look_for),
            if_difficult: linked(&e.if_difficult),
            safety_note: e.safety_note.as_deref().map(linked),
            hint: plain_text(&e.hint, glossary),
            why_it_works: e.why_this_works.as_ref().map(|w| WhyItWorks {
                text: plain_text(&w.plain_why, glossary),
                term: glossary.term(&w.term_id).map(term_summary),
            }),
            skill,
        })
    }

    /// The Learn topics, each with the exercises that suit the dog's age
    /// (every exercise before onboarding). Empty topics are left out.
    pub fn learn_topics(&self) -> Vec<LearnTopic> {
        let stage = lock(&self.state).dog.as_ref().map(|d| d.life_stage);
        bundled_topics()
            .iter()
            .map(|topic| LearnTopic {
                id: serde_json::to_value(topic.id)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default(),
                title: topic.title.clone(),
                description: topic.description.clone(),
                exercises: bundled_exercises()
                    .iter()
                    .filter(|e| e.topic_group == topic.id)
                    .filter(|e| stage.is_none_or(|s| e.life_stages.contains(&s.into())))
                    .map(exercise_summary)
                    .collect(),
            })
            .filter(|t| !t.exercises.is_empty())
            .collect()
    }

    /// The glossary by category, terms in alphabetical order.
    pub fn glossary(&self) -> Vec<GlossarySection> {
        let glossary = bundled_glossary();
        glossary
            .categories
            .iter()
            .map(|c| {
                let mut terms: Vec<TermSummary> = glossary
                    .terms
                    .iter()
                    .filter(|t| t.category == c.id)
                    .map(term_summary)
                    .collect();
                terms.sort_by_key(|t| t.name.to_lowercase());
                GlossarySection {
                    title: c.title.clone(),
                    description: c.description.clone(),
                    terms,
                }
            })
            .filter(|s| !s.terms.is_empty())
            .collect()
    }

    pub fn term(&self, term_id: String) -> Result<TermDetail, AppError> {
        let glossary = bundled_glossary();
        let t = glossary.term(&term_id).ok_or(AppError::NotFound {
            what: "that term".into(),
        })?;
        let linked = |text: &str| LinkedText::from(parse_term_links(text, glossary));
        Ok(TermDetail {
            id: t.id.clone(),
            name: t.preferred_term.clone(),
            also_called: t.alternative_terms.clone(),
            short_definition: t.short_definition.clone(),
            explanation: linked(&t.deeper_explanation),
            example: linked(&t.example),
            common_misunderstanding: t.common_misunderstanding.as_deref().map(linked),
            related: t
                .related_term_ids
                .iter()
                .filter_map(|id| glossary.term(id))
                .map(term_summary)
                .collect(),
            exercises: t
                .relevant_exercise_ids
                .iter()
                .filter_map(|id| exercise(id))
                .map(exercise_summary)
                .collect(),
        })
    }

    /// Exercises and terms matching `query` (ignoring case), for the Learn
    /// search field. Quick enough to call on every keystroke.
    pub fn search_learn(&self, query: String) -> LearnSearchResults {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return LearnSearchResults {
                exercises: Vec::new(),
                terms: Vec::new(),
            };
        }
        let matches = |text: &str| text.to_lowercase().contains(&query);
        LearnSearchResults {
            exercises: bundled_exercises()
                .iter()
                .filter(|e| matches(&e.title) || matches(&e.summary))
                .map(exercise_summary)
                .collect(),
            terms: bundled_glossary()
                .terms
                .iter()
                .filter(|t| term_matches(t, &matches))
                .map(term_summary)
                .collect(),
        }
    }

    // MARK: Events and lifecycle

    /// A new event stream. Each consumer takes its own.
    pub fn events(&self) -> Result<Arc<AppEvents>, AppError> {
        AppEvents::subscribe(&self.events).ok_or(AppError::ShutDown)
    }

    pub fn phase(&self) -> AppPhase {
        self.lifecycle.phase().into()
    }

    /// The host reports a lifecycle change. Coming back from the background
    /// emits `PhaseChanged { resumed: true }`; `Terminated` is `shutdown()`.
    pub fn set_phase(&self, phase: AppPhase) {
        if phase == AppPhase::Terminated {
            return self.shutdown();
        }
        if let Some(change) = self.lifecycle.enter(phase.into()) {
            self.events.publish(AppEvent::PhaseChanged {
                phase,
                resumed: change.resumed,
            });
        }
    }

    /// Refuses new commands with `ShutDown` and ends every event stream.
    /// Saves already running finish. Idempotent.
    pub fn shutdown(&self) {
        self.shut_down.store(true, Ordering::SeqCst);
        if self.lifecycle.enter(Phase::Terminated).is_some() {
            self.events.publish(AppEvent::PhaseChanged {
                phase: AppPhase::Terminated,
                resumed: false,
            });
        }
        self.events.close();
    }
}

impl AppCore {
    /// Runs `apply` on a copy of the data off the calling thread, saves the
    /// copy, and only then makes it current and publishes `DataChanged`. A
    /// failed rule or save changes nothing.
    async fn change<T: Send + 'static>(
        &self,
        name: &'static str,
        apply: impl FnOnce(&mut Stored) -> Result<T, AppError> + Send + 'static,
    ) -> Result<T, AppError> {
        if self.shut_down.load(Ordering::SeqCst) {
            return Err(AppError::ShutDown);
        }
        let (state, file, events) = (
            Arc::clone(&self.state),
            Arc::clone(&self.file),
            self.events.clone(),
        );
        background(name, move || {
            let mut state = lock(&state);
            let mut next = state.clone();
            let value = apply(&mut next)?;
            if next != *state {
                file.save(&next)?;
                *state = next;
                events.publish(AppEvent::DataChanged);
            }
            Ok(value)
        })
        .await
    }
}

fn minutes(e: &ExerciseContent) -> u32 {
    e.core.estimated_minutes.max(0) as u32
}

fn exercise_summary(e: &ExerciseContent) -> ExerciseSummary {
    ExerciseSummary {
        id: e.core.id.clone(),
        title: e.title.clone(),
        summary: e.summary.clone(),
        minutes: minutes(e),
    }
}

fn term_summary(t: &GlossaryTerm) -> TermSummary {
    TermSummary {
        id: t.id.clone(),
        name: t.preferred_term.clone(),
        short_definition: t.short_definition.clone(),
    }
}

fn term_matches(t: &GlossaryTerm, matches: &impl Fn(&str) -> bool) -> bool {
    matches(&t.preferred_term)
        || t.alternative_terms.iter().any(|a| matches(a))
        || t.search_phrases.iter().any(|p| matches(p))
}

/// What Good Dog shows `forge device` in development builds. Never expose
/// the dog's name or anything the owner typed.
#[cfg(feature = "devtools")]
mod devtools {
    use super::*;
    use forge_devtools::{Config, DevTools};

    pub(crate) fn start(
        storage_directory: &std::path::Path,
        state: &Arc<Mutex<Stored>>,
        lifecycle: &Lifecycle,
    ) -> Option<DevTools> {
        let app = option_env!("FORGE_APP_ID").unwrap_or(env!("CARGO_PKG_NAME"));
        let config = Config::new(
            app,
            env!("CARGO_PKG_VERSION"),
            storage_directory.join(".forge-devtools-pairing"),
        );
        let tools = match DevTools::start(config, lifecycle.clone()) {
            Ok(tools) => tools,
            Err(e) => {
                forge_runtime::logging::warn!("devtools did not start: {e}");
                return None;
            }
        };
        let s = state.clone();
        tools.probe("sessions", move || lock(&s).sessions.len().to_string());
        let s = state.clone();
        tools.probe("profile", move || lock(&s).dog.is_some().to_string());
        Some(tools)
    }
}
