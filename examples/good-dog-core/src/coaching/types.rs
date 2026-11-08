use serde::{Deserialize, Serialize};

/// One day in milliseconds. Times in the engine are Unix milliseconds.
pub const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// How a practice session went, as the user reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionOutcome {
    Easy,
    GettingThere,
    TooDifficult,
}

/// Where a skill (a learning objective) is for one subject.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillState {
    NotIntroduced,
    Introduced,
    Practising,
    BecomingConsistent,
    ReadyToIncrease,
    NeedsEasier,
}

impl SkillState {
    pub const ALL: [SkillState; 6] = [
        SkillState::NotIntroduced,
        SkillState::Introduced,
        SkillState::Practising,
        SkillState::BecomingConsistent,
        SkillState::ReadyToIncrease,
        SkillState::NeedsEasier,
    ];
}

/// The slot an item fills in the day's plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanItemRole {
    Engage,
    Skill,
    Everyday,
}

impl PlanItemRole {
    pub fn as_str(self) -> &'static str {
        match self {
            PlanItemRole::Engage => "engage",
            PlanItemRole::Skill => "skill",
            PlanItemRole::Everyday => "everyday",
        }
    }
}

/// Tuning for progression and planning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgressionConfig {
    /// Easy results in a row before a skill is ready for a harder step.
    pub easy_results_for_increase: u32,
    /// How many recent outcomes a skill remembers.
    pub recent_window_size: usize,
    /// Avoid repeating an exercise practised within this many days.
    pub avoid_repeat_within_days: i64,
    /// The time budget of a short ("only a few minutes") plan.
    pub short_plan_minutes: i32,
    /// Days to avoid an objective after a welfare concern is reported.
    pub welfare_cooldown_days: i64,
}

impl Default for ProgressionConfig {
    fn default() -> Self {
        Self {
            easy_results_for_increase: 3,
            recent_window_size: 5,
            avoid_repeat_within_days: 1,
            short_plan_minutes: 4,
            welfare_cooldown_days: 3,
        }
    }
}

/// The part of an exercise the engine needs. Domains embed it in their own
/// exercise type and expose it through [`CoachingExercise`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExerciseCore {
    pub id: String,
    pub learning_objective_id: String,
    pub category: String,
    pub difficulty: i32,
    pub estimated_minutes: i32,
    #[serde(default)]
    pub prerequisite_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub easier_variation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harder_variation_id: Option<String>,
}

/// An exercise the engine can plan with.
pub trait CoachingExercise {
    fn core(&self) -> &ExerciseCore;
}

impl CoachingExercise for ExerciseCore {
    fn core(&self) -> &ExerciseCore {
        self
    }
}

/// The subject being coached. The engine only needs a name for its wording.
pub trait CoachingSubject {
    fn name(&self) -> &str;
}
