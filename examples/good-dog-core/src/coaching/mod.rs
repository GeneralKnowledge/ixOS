//! Domain-neutral planning and progression engine.
//!
//! The engine decides *how* a plan is built; a [`CoachingPolicy`] decides what
//! is suitable for one subject. Prerequisites, easier and harder variations,
//! recent-repeat avoidance, variety, the time budget, welfare cooldowns and
//! the skill-progress state machine all live here.

mod plan;
mod policy;
mod progression;
mod types;
mod welfare;

pub use plan::{
    generate_daily_plan, pick_plan_item_replacement, GeneratedPlan, PlanReason, PlanRequest,
    PlannedItem, RecentSession, ReplacementPick, MAX_REPLACES_PER_ITEM,
};
pub use policy::{CoachingPolicy, PlanBudget, WhyTodayInput};
pub use progression::{
    apply_outcome_to_progress, describe_skill_state, should_prefer_easier_variation,
    should_prefer_harder_variation, summarise_recent_outcomes, OutcomeRecord, OutcomeSummary,
    ProgressSnapshot,
};
pub use types::{
    CoachingExercise, CoachingSubject, ExerciseCore, PlanItemRole, ProgressionConfig,
    SessionOutcome, SkillState, DAY_MS,
};
pub use welfare::{
    has_recent_welfare_in_progress, objective_in_welfare_cooldown, welfare_cooldown_ms,
    WELFARE_CALM_EXERCISE_IDS,
};
