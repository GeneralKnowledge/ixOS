use serde::{Deserialize, Serialize};

use super::types::{ProgressionConfig, SessionOutcome, SkillState};
use super::welfare::has_recent_welfare_in_progress;

/// One remembered outcome for a skill.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeRecord {
    pub outcome: SessionOutcome,
    pub welfare_concern: bool,
    /// Unix milliseconds.
    pub at: i64,
}

/// A skill's progress for one subject.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressSnapshot {
    pub state: SkillState,
    pub easy_streak: u32,
    #[serde(default)]
    pub recent_outcomes: Vec<OutcomeRecord>,
}

impl ProgressSnapshot {
    /// A skill nobody has practised yet.
    pub fn fresh() -> Self {
        Self {
            state: SkillState::NotIntroduced,
            easy_streak: 0,
            recent_outcomes: Vec::new(),
        }
    }
}

/// Moves a skill on after one session. One easy result never makes a skill
/// ready for a harder step, and a welfare concern or a too-difficult result
/// always makes it easier.
pub fn apply_outcome_to_progress(
    current: &ProgressSnapshot,
    outcome: SessionOutcome,
    welfare_concern: bool,
    config: &ProgressionConfig,
    now: i64,
) -> ProgressSnapshot {
    let mut recent_outcomes = Vec::with_capacity(current.recent_outcomes.len() + 1);
    recent_outcomes.push(OutcomeRecord {
        outcome,
        welfare_concern,
        at: now,
    });
    recent_outcomes.extend(current.recent_outcomes.iter().copied());
    recent_outcomes.truncate(config.recent_window_size);

    if welfare_concern || outcome == SessionOutcome::TooDifficult {
        return ProgressSnapshot {
            state: SkillState::NeedsEasier,
            easy_streak: 0,
            recent_outcomes,
        };
    }

    if outcome == SessionOutcome::GettingThere {
        let state = if current.state == SkillState::NotIntroduced {
            SkillState::Introduced
        } else {
            SkillState::Practising
        };
        return ProgressSnapshot {
            state,
            easy_streak: 0,
            recent_outcomes,
        };
    }

    // Easy.
    let easy_streak = current.easy_streak + 1;
    let mut state = match current.state {
        SkillState::NotIntroduced => SkillState::Introduced,
        SkillState::Introduced => SkillState::Practising,
        SkillState::NeedsEasier => {
            if has_recent_welfare_in_progress(&recent_outcomes, now, config) {
                SkillState::NeedsEasier
            } else {
                SkillState::Practising
            }
        }
        SkillState::Practising => {
            if easy_streak >= config.easy_results_for_increase.saturating_sub(1).max(2) {
                SkillState::BecomingConsistent
            } else {
                SkillState::Practising
            }
        }
        SkillState::BecomingConsistent => {
            if easy_streak >= config.easy_results_for_increase {
                SkillState::ReadyToIncrease
            } else {
                SkillState::BecomingConsistent
            }
        }
        SkillState::ReadyToIncrease => SkillState::ReadyToIncrease,
    };

    // One easy result alone must never jump to ready_to_increase.
    if (current.state == SkillState::NotIntroduced
        || current.state == SkillState::Introduced
        || current.easy_streak == 0)
        && easy_streak < config.easy_results_for_increase
        && state == SkillState::ReadyToIncrease
    {
        state = SkillState::BecomingConsistent;
    }

    ProgressSnapshot {
        state,
        easy_streak,
        recent_outcomes,
    }
}

pub fn should_prefer_easier_variation(progress: &ProgressSnapshot) -> bool {
    progress.state == SkillState::NeedsEasier
}

pub fn should_prefer_harder_variation(progress: &ProgressSnapshot) -> bool {
    progress.state == SkillState::ReadyToIncrease
}

/// A short, generic sentence about a skill's state, or `None` before it starts.
pub fn describe_skill_state(state: SkillState) -> Option<&'static str> {
    match state {
        SkillState::NotIntroduced => None,
        SkillState::Introduced => Some("We’ve started practising this."),
        SkillState::Practising => Some("This skill is still in progress."),
        SkillState::BecomingConsistent => Some("This is becoming more familiar."),
        SkillState::ReadyToIncrease => {
            Some("We’ve had enough good practice to try a tiny step forward.")
        }
        SkillState::NeedsEasier => Some("We’re keeping this easier while confidence rebuilds."),
    }
}

/// A one-word summary of a skill's recent outcomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutcomeSummary {
    Empty,
    Welfare,
    MostlyHard,
    MostlyEasy,
    Mixed,
}

pub fn summarise_recent_outcomes(outcomes: &[OutcomeRecord]) -> OutcomeSummary {
    if outcomes.is_empty() {
        return OutcomeSummary::Empty;
    }
    if outcomes.iter().any(|o| o.welfare_concern) {
        return OutcomeSummary::Welfare;
    }
    let easy = outcomes
        .iter()
        .filter(|o| o.outcome == SessionOutcome::Easy)
        .count();
    let hard = outcomes
        .iter()
        .filter(|o| o.outcome == SessionOutcome::TooDifficult)
        .count();
    if hard > easy {
        return OutcomeSummary::MostlyHard;
    }
    if easy + 1 >= outcomes.len() && easy > 0 {
        return OutcomeSummary::MostlyEasy;
    }
    OutcomeSummary::Mixed
}
