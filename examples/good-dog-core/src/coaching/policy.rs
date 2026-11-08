use super::types::{CoachingExercise, CoachingSubject, PlanItemRole, SkillState};

/// Time and item limits for one plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanBudget {
    pub max_minutes: i32,
    pub item_count: usize,
}

/// What the policy needs to word the reason for choosing an item today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WhyTodayInput<'a> {
    pub subject_name: &'a str,
    pub role: PlanItemRole,
    pub skill_state: Option<SkillState>,
    pub preferred_easier: bool,
    pub preferred_harder: bool,
    pub is_starter: bool,
}

/// Everything the engine needs to know about one domain. A domain implements
/// this once; the engine stays free of domain knowledge.
pub trait CoachingPolicy {
    type Exercise: CoachingExercise;
    type Subject: CoachingSubject;

    /// Could this exercise ever suit this subject?
    fn is_eligible(&self, exercise: &Self::Exercise, subject: &Self::Subject) -> bool;

    /// Time and item limits for a normal (not short) plan.
    fn daily_budget(&self, subject: &Self::Subject) -> PlanBudget;

    /// Gentle first exercises, used until the subject has any progress. May be empty.
    fn starter_exercises<'a>(
        &self,
        eligible: &[&'a Self::Exercise],
        subject: &Self::Subject,
    ) -> Vec<&'a Self::Exercise>;

    /// Extra score for exercises that suit this subject's goals and experience.
    fn affinity_score(&self, exercise: &Self::Exercise, subject: &Self::Subject) -> i32;

    /// Plain-English reason shown to the user for choosing an exercise today.
    fn explain(&self, input: &WhyTodayInput<'_>) -> String;
}
