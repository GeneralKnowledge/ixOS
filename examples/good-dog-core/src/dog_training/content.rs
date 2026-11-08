//! Exercise content bundled into the app. The JSON files under `content/` are
//! exported from the Good Dog web app (see `scripts/export-from-web.sh`).

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::types::ExerciseContent;

/// A skill the exercises teach, such as "Name response".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningObjective {
    pub id: String,
    pub label: String,
    pub plain_english: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ContentError {
    #[error("content JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn parse_exercises(json: &str) -> Result<Vec<ExerciseContent>, ContentError> {
    Ok(serde_json::from_str(json)?)
}

pub fn parse_learning_objectives(json: &str) -> Result<Vec<LearningObjective>, ContentError> {
    Ok(serde_json::from_str(json)?)
}

const EXERCISES_JSON: &str = include_str!("../../content/exercises.json");
const LEARNING_OBJECTIVES_JSON: &str = include_str!("../../content/learning-objectives.json");

/// The exercise library shipped with the app, in the web app's order (the
/// planner breaks ties by this order).
///
/// # Panics
///
/// Never in practice: the bundled JSON is fixed at compile time and
/// `tests/content.rs` parses and checks it.
pub fn bundled_exercises() -> &'static [ExerciseContent] {
    static EXERCISES: OnceLock<Vec<ExerciseContent>> = OnceLock::new();
    EXERCISES.get_or_init(|| parse_exercises(EXERCISES_JSON).expect("bundled exercises.json"))
}

/// The learning objectives shipped with the app.
///
/// # Panics
///
/// As for [`bundled_exercises`].
pub fn bundled_learning_objectives() -> &'static [LearningObjective] {
    static OBJECTIVES: OnceLock<Vec<LearningObjective>> = OnceLock::new();
    OBJECTIVES.get_or_init(|| {
        parse_learning_objectives(LEARNING_OBJECTIVES_JSON)
            .expect("bundled learning-objectives.json")
    })
}
