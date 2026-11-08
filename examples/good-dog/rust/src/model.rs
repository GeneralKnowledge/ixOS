//! The records and enums the hosts see. Names are prefixed with the domain
//! where a plain name would shadow a Swift or Foundation type.

use good_dog_core::coaching::{PlanItemRole, SessionOutcome, SkillState};
use good_dog_core::dog_training::{AvailableTime, LifeStage, TextRun, TrainingExperience};
use serde::{Deserialize, Serialize};

/// How old the dog is, in training terms.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DogLifeStage {
    /// Under about 4 months.
    YoungPuppy,
    /// About 4 to 6 months.
    OlderPuppy,
    /// About 6 months to 2 years.
    Adolescent,
    Adult,
    Senior,
}

/// How long the owner can usually train in a day.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DailyTime {
    FewMinutes,
    AboutTenMinutes,
    MoreThanTen,
}

/// What the owner most wants help with. A closed set: the web app asks for
/// free text and matches keywords, which its own docs call brittle.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TrainingGoal {
    General,
    Recall,
    LooseLead,
    Settle,
    Manners,
    PuppyBasics,
}

/// The owner's training experience.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum OwnerExperience {
    New,
    Some,
}

/// The dog, as the owner describes it.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DogProfile {
    pub name: String,
    pub life_stage: DogLifeStage,
    pub daily_time: DailyTime,
    pub goal: TrainingGoal,
    pub experience: OwnerExperience,
}

/// The time now, with the phone's offset from UTC, so Rust can tell which
/// local day it is ("today's plan").
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    /// Unix milliseconds.
    pub ms: i64,
    pub utc_offset_minutes: i32,
}

/// The slot an activity fills in the day's plan.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PlanRole {
    /// A warm-up.
    Engage,
    /// The main skill.
    Skill,
    /// Something that fits into everyday life.
    Everyday,
}

/// How a practice session went, as the owner reports it.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SessionResult {
    Easy,
    GettingThere,
    TooDifficult,
}

/// One activity in today's plan.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct PlanActivity {
    pub id: u64,
    pub exercise_id: String,
    pub title: String,
    pub summary: String,
    pub minutes: u32,
    pub role: PlanRole,
    pub why_today: String,
    /// The result recorded for it today, once practised.
    pub result: Option<SessionResult>,
    /// Whether "Try something else" is still offered (at most twice, and not once practised).
    pub can_swap: bool,
}

/// Today's plan.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct TodayPlan {
    pub activities: Vec<PlanActivity>,
    pub total_minutes: u32,
    pub done_count: u32,
    /// The owner chose "Only a few minutes today".
    pub short_plan: bool,
    /// The short plan is still offered (nothing practised yet, not already short).
    pub can_shorten: bool,
    /// A first plan, built from gentle starter exercises.
    pub starter: bool,
    /// The active goal today's plan works towards most, when there is one.
    pub goal_focus: Option<GoalFocus>,
}

/// Which goal today's plan serves, and why, for the top of Today.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct GoalFocus {
    pub goal_id: u64,
    pub goal_title: String,
    pub stage_title: String,
    /// Rust's wording: why this stage matters now.
    pub why: String,
}

/// Where a goal stands.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Paused,
    /// Every stage passed; the skill is kept up with occasional practice.
    Finished,
    /// The owner stopped working on it. Its history stays.
    SetAside,
}

/// A goal the owner can start.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct PathwaySummary {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub stage_count: u32,
}

/// One of the owner's goals, for a list.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct GoalSummary {
    pub id: u64,
    pub pathway_id: String,
    pub title: String,
    pub status: GoalStatus,
    /// The stage being worked on (the last one once finished).
    pub stage_title: String,
    /// 1-based.
    pub stage_number: u32,
    pub stage_count: u32,
    /// What the owner wrote when they started it, if anything.
    pub own_words: Option<String>,
}

/// Where a stage is on the owner's route.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageState {
    Passed,
    Current,
    Upcoming,
}

/// One stage of a goal, explained.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct GoalStage {
    pub id: String,
    pub title: String,
    pub state: StageState,
    pub what_it_means: String,
    pub why_it_matters: String,
    pub exercises: Vec<ExerciseSummary>,
    pub everyday_practice: String,
    /// What "ready" looks like, in words.
    pub ready_when: String,
    pub if_it_is_hard: String,
    /// For passed stages: how to keep the skill.
    pub maintenance: String,
}

/// Whether the current stage's gate can be asked.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct GateStatus {
    /// The app's evidence is there: ask the owner `question`.
    pub ready_for_check: bool,
    /// A welfare rest or "making it easier" holds the stage. Moving on is
    /// refused until it ends.
    pub held: bool,
    /// The gate as a question to the owner, with the dog's name.
    pub question: String,
    /// Rust's plain-English note on where things stand.
    pub note: String,
}

/// Something that happened to a goal, worded by Rust.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct GoalEvent {
    pub at_ms: i64,
    pub text: String,
}

/// A goal in full.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct GoalDetail {
    pub summary: GoalSummary,
    pub pathway_summary: String,
    pub stages: Vec<GoalStage>,
    /// `None` once the goal is finished.
    pub gate: Option<GateStatus>,
    /// Newest first.
    pub history: Vec<GoalEvent>,
}

/// A short row for an exercise.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ExerciseSummary {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub minutes: u32,
}

/// A piece of text, linked to a glossary term or not.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct TextPiece {
    pub text: String,
    pub term_id: Option<String>,
}

/// Text whose glossary terms the host can show as links.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct LinkedText {
    pub pieces: Vec<TextPiece>,
}

impl From<Vec<TextRun>> for LinkedText {
    fn from(runs: Vec<TextRun>) -> Self {
        Self {
            pieces: runs
                .into_iter()
                .map(|r| TextPiece {
                    text: r.text,
                    term_id: r.term_id,
                })
                .collect(),
        }
    }
}

/// A glossary term in a list.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct TermSummary {
    pub id: String,
    pub name: String,
    pub short_definition: String,
}

/// A quiet "why this works" note on an exercise.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct WhyItWorks {
    pub text: String,
    pub term: Option<TermSummary>,
}

/// Everything the exercise screen shows.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ExerciseDetail {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub minutes: u32,
    pub purpose: LinkedText,
    pub preparation: LinkedText,
    pub steps: Vec<LinkedText>,
    pub look_for: LinkedText,
    pub if_difficult: LinkedText,
    pub safety_note: Option<LinkedText>,
    pub hint: String,
    pub why_it_works: Option<WhyItWorks>,
    /// Where the dog is with this exercise's skill.
    pub skill: SkillProgress,
}

/// A glossary term in full.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct TermDetail {
    pub id: String,
    pub name: String,
    pub also_called: Vec<String>,
    pub short_definition: String,
    pub explanation: LinkedText,
    pub example: LinkedText,
    pub common_misunderstanding: Option<LinkedText>,
    pub related: Vec<TermSummary>,
    pub exercises: Vec<ExerciseSummary>,
}

/// A Learn topic and its exercises.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct LearnTopic {
    pub id: String,
    pub title: String,
    pub description: String,
    pub exercises: Vec<ExerciseSummary>,
}

/// A glossary category and its terms.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct GlossarySection {
    pub title: String,
    pub description: String,
    pub terms: Vec<TermSummary>,
}

/// What a Learn search found.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct LearnSearchResults {
    pub exercises: Vec<ExerciseSummary>,
    pub terms: Vec<TermSummary>,
}

/// Where a skill is, in the owner's words.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SkillLevel {
    NotStarted,
    Started,
    Practising,
    BecomingConsistent,
    ReadyForMore,
    MakingEasier,
}

impl From<SkillState> for SkillLevel {
    fn from(state: SkillState) -> Self {
        match state {
            SkillState::NotIntroduced => Self::NotStarted,
            SkillState::Introduced => Self::Started,
            SkillState::Practising => Self::Practising,
            SkillState::BecomingConsistent => Self::BecomingConsistent,
            SkillState::ReadyToIncrease => Self::ReadyForMore,
            SkillState::NeedsEasier => Self::MakingEasier,
        }
    }
}

/// One skill's progress.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct SkillProgress {
    pub objective_id: String,
    pub name: String,
    pub plain_english: String,
    pub level: SkillLevel,
    /// A sentence about the level, empty before the skill starts.
    pub description: String,
    pub sessions: u32,
}

/// The My Dog progress summary.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ProgressSummary {
    pub sessions_total: u32,
    /// Sessions in the last 7 days, today included.
    pub sessions_this_week: u32,
    /// Days in the last 7 with at least one session.
    pub days_practised_this_week: u32,
    /// Skills that have started, the furthest along first.
    pub skills: Vec<SkillProgress>,
}

/// One recorded session.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct SessionRecord {
    pub id: u64,
    pub exercise_id: String,
    pub exercise_title: String,
    pub completed_at_ms: i64,
    pub result: SessionResult,
    pub welfare_concern: bool,
}

/// What the app says after a session is recorded.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct SessionFeedback {
    pub message: String,
    pub skill: SkillProgress,
}

// Conversions to the engine's types.

impl From<DogLifeStage> for LifeStage {
    fn from(stage: DogLifeStage) -> Self {
        match stage {
            DogLifeStage::YoungPuppy => Self::YoungPuppy,
            DogLifeStage::OlderPuppy => Self::OlderPuppy,
            DogLifeStage::Adolescent => Self::Adolescent,
            DogLifeStage::Adult => Self::Adult,
            DogLifeStage::Senior => Self::Senior,
        }
    }
}

impl From<DailyTime> for AvailableTime {
    fn from(time: DailyTime) -> Self {
        match time {
            DailyTime::FewMinutes => Self::FewMinutes,
            DailyTime::AboutTenMinutes => Self::About10,
            DailyTime::MoreThanTen => Self::More,
        }
    }
}

impl From<OwnerExperience> for TrainingExperience {
    fn from(exp: OwnerExperience) -> Self {
        match exp {
            OwnerExperience::New => Self::New,
            OwnerExperience::Some => Self::Some,
        }
    }
}

impl TrainingGoal {
    /// The phrase the dog-training policy matches goal keywords in. These are
    /// the answers the web app's golden scenarios use.
    pub fn as_reason(self) -> &'static str {
        match self {
            Self::General => "",
            Self::Recall => "Recall help",
            Self::LooseLead => "Loose lead walking",
            Self::Settle => "Settle at home",
            Self::Manners => "Everyday manners",
            Self::PuppyBasics => "New puppy basics",
        }
    }
}

impl From<PlanItemRole> for PlanRole {
    fn from(role: PlanItemRole) -> Self {
        match role {
            PlanItemRole::Engage => Self::Engage,
            PlanItemRole::Skill => Self::Skill,
            PlanItemRole::Everyday => Self::Everyday,
        }
    }
}

impl From<PlanRole> for PlanItemRole {
    fn from(role: PlanRole) -> Self {
        match role {
            PlanRole::Engage => Self::Engage,
            PlanRole::Skill => Self::Skill,
            PlanRole::Everyday => Self::Everyday,
        }
    }
}

impl From<SessionResult> for SessionOutcome {
    fn from(result: SessionResult) -> Self {
        match result {
            SessionResult::Easy => Self::Easy,
            SessionResult::GettingThere => Self::GettingThere,
            SessionResult::TooDifficult => Self::TooDifficult,
        }
    }
}
