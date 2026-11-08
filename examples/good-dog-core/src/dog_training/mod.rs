//! The dog-training domain: types, the [`DogTrainingPolicy`] and the bundled
//! exercise content.

mod content;
mod policy;
mod types;
mod why_today;

pub use content::{
    bundled_exercises, bundled_learning_objectives, parse_exercises, parse_learning_objectives,
    ContentError, LearningObjective,
};
pub use policy::DogTrainingPolicy;
pub use types::{
    AvailableTime, DogSubject, ExerciseContent, LifeStage, TopicGroup, TrainingExperience,
    WhyThisWorks,
};
pub use why_today::explain_plan_item;
