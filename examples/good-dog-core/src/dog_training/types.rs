use serde::{Deserialize, Serialize};

use crate::coaching::{CoachingExercise, CoachingSubject, ExerciseCore};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifeStage {
    YoungPuppy,
    OlderPuppy,
    Adolescent,
    Adult,
    Senior,
}

impl LifeStage {
    pub const ALL: [LifeStage; 5] = [
        LifeStage::YoungPuppy,
        LifeStage::OlderPuppy,
        LifeStage::Adolescent,
        LifeStage::Adult,
        LifeStage::Senior,
    ];

    pub fn is_puppy(self) -> bool {
        matches!(self, LifeStage::YoungPuppy | LifeStage::OlderPuppy)
    }
}

/// How long the owner can usually spend in a day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AvailableTime {
    FewMinutes,
    #[serde(rename = "about_10")]
    About10,
    More,
}

impl AvailableTime {
    pub const ALL: [AvailableTime; 3] = [
        AvailableTime::FewMinutes,
        AvailableTime::About10,
        AvailableTime::More,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingExperience {
    New,
    Some,
}

/// How the Learn screen groups exercises.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopicGroup {
    EverydayFoundations,
    PuppyLife,
    WalkingTogether,
    ComingBack,
    CalmConfidence,
    LifeAtHome,
}

/// A quiet "why this works" note: plain English plus one glossary term.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhyThisWorks {
    pub plain_why: String,
    pub term_id: String,
}

/// One exercise as the app shows it. Text may hold glossary links written
/// `[[term-id]]` or `[[term-id|shown text]]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExerciseContent {
    #[serde(flatten)]
    pub core: ExerciseCore,
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub life_stages: Vec<LifeStage>,
    pub purpose: String,
    pub preparation: String,
    pub steps: Vec<String>,
    pub look_for: String,
    pub if_difficult: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_note: Option<String>,
    pub hint: String,
    pub topic_group: TopicGroup,
    #[serde(default)]
    pub glossary_term_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why_this_works: Option<WhyThisWorks>,
    /// Bumped whenever the exercise's text changes, so history keeps the version used.
    pub content_version: u32,
}

impl CoachingExercise for ExerciseContent {
    fn core(&self) -> &ExerciseCore {
        &self.core
    }
}

/// The dog being trained, as the planner needs it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DogSubject {
    pub id: String,
    pub name: String,
    pub life_stage: LifeStage,
    pub available_time: AvailableTime,
    /// The owner's free-text answer to "what would you like help with?".
    pub primary_reason: String,
    pub training_experience: TrainingExperience,
}

impl CoachingSubject for DogSubject {
    fn name(&self) -> &str {
        &self.name
    }
}
