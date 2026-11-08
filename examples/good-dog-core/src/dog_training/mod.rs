//! The dog-training domain: types, the [`DogTrainingPolicy`] and the bundled
//! exercise content.

mod content;
mod markup;
mod policy;
mod types;
mod why_today;

pub use content::{
    bundled_exercises, bundled_glossary, bundled_learning_objectives, bundled_topics,
    parse_exercises, parse_learning_objectives, ContentError, Glossary, GlossaryCategory,
    GlossaryTerm, LearningObjective, Topic,
};
pub use markup::{parse_term_links, plain_text, TextRun};
pub use policy::DogTrainingPolicy;
pub use types::{
    AvailableTime, DogSubject, ExerciseContent, LifeStage, TopicGroup, TrainingExperience,
    WhyThisWorks,
};
pub use why_today::explain_plan_item;
