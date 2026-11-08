//! The dog-training domain: types, the [`DogTrainingPolicy`] and the bundled
//! exercise content.

mod content;
mod markup;
mod pathways;
mod policy;
mod types;
mod why_today;

pub use content::{
    bundled_exercises, bundled_glossary, bundled_learning_objectives, bundled_topics,
    parse_exercises, parse_learning_objectives, ContentError, Glossary, GlossaryCategory,
    GlossaryTerm, LearningObjective, Topic,
};
pub use markup::{parse_term_links, plain_text, TextRun};
pub use pathways::{
    bundled_pathways, check_pathways, match_pathways, parse_pathways, stage_readiness, Pathway,
    PathwayStage, StageBlocker, StageGate, StageReadiness, GATE_PRACTICE_DAYS,
};
pub use policy::{DogTrainingPolicy, GoalFocusPolicy, GOAL_FOCUS_BOOST};
pub use types::{
    AvailableTime, DogSubject, ExerciseContent, LifeStage, TopicGroup, TrainingExperience,
    WhyThisWorks,
};
pub use why_today::explain_plan_item;
