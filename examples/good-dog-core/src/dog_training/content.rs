//! Exercise content bundled into the app. The JSON files under `content/` are
//! exported from the Good Dog web app (see `scripts/export-from-web.sh`).

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::types::{ExerciseContent, TopicGroup};

/// A skill the exercises teach, such as "Name response".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningObjective {
    pub id: String,
    pub label: String,
    pub plain_english: String,
}

/// A Learn screen group of exercises.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Topic {
    pub id: TopicGroup,
    pub title: String,
    pub description: String,
}

/// A glossary category, such as "Reward-based training".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlossaryCategory {
    pub id: String,
    pub title: String,
    pub description: String,
}

/// One published glossary term.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlossaryTerm {
    pub id: String,
    pub preferred_term: String,
    #[serde(default)]
    pub alternative_terms: Vec<String>,
    /// Everyday phrases owners might search for.
    #[serde(default)]
    pub search_phrases: Vec<String>,
    pub category: String,
    pub short_definition: String,
    pub deeper_explanation: String,
    pub example: String,
    #[serde(default)]
    pub common_misunderstanding: Option<String>,
    #[serde(default)]
    pub related_term_ids: Vec<String>,
    #[serde(default)]
    pub relevant_exercise_ids: Vec<String>,
    pub content_version: u32,
}

/// The bundled glossary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Glossary {
    pub categories: Vec<GlossaryCategory>,
    pub terms: Vec<GlossaryTerm>,
}

impl Glossary {
    pub fn term(&self, id: &str) -> Option<&GlossaryTerm> {
        self.terms.iter().find(|t| t.id == id)
    }
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

const TOPICS_JSON: &str = include_str!("../../content/topics.json");
const GLOSSARY_JSON: &str = include_str!("../../content/glossary.json");
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

/// The Learn topics, in display order.
///
/// # Panics
///
/// As for [`bundled_exercises`].
pub fn bundled_topics() -> &'static [Topic] {
    static TOPICS: OnceLock<Vec<Topic>> = OnceLock::new();
    TOPICS.get_or_init(|| serde_json::from_str(TOPICS_JSON).expect("bundled topics.json"))
}

/// The published glossary.
///
/// # Panics
///
/// As for [`bundled_exercises`].
pub fn bundled_glossary() -> &'static Glossary {
    static GLOSSARY: OnceLock<Glossary> = OnceLock::new();
    GLOSSARY.get_or_init(|| serde_json::from_str(GLOSSARY_JSON).expect("bundled glossary.json"))
}
