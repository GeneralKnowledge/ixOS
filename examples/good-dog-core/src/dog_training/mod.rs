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
    bundled_pathways, bundled_safety_rules, check_pathways, check_safety_rules, match_pathways,
    parse_pathways, parse_safety_rules, stage_readiness, AppEnforcement, LessonKind, Pathway,
    PathwayStage, SafetyRule, StageBlocker, StageGate, StageLesson, StageReadiness,
    GATE_PRACTICE_DAYS,
};
pub use policy::{DogTrainingPolicy, GoalFocusPolicy, GOAL_FOCUS_BOOST};
pub use types::{
    AvailableTime, DogSubject, ExerciseContent, LifeStage, TopicGroup, TrainingExperience,
    WhyThisWorks,
};
pub use why_today::explain_plan_item;
