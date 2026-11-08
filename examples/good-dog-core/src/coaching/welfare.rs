use super::plan::RecentSession;
use super::progression::OutcomeRecord;
use super::types::{ProgressionConfig, DAY_MS};

/// Low-pressure exercises offered while any welfare cooldown is running.
pub const WELFARE_CALM_EXERCISE_IDS: [&str; 5] = [
    "ex-engagement-easy",
    "ex-mat-settle",
    "ex-rest-spot",
    "ex-reward-marker",
    "ex-handling-touch",
];

pub fn welfare_cooldown_ms(config: &ProgressionConfig) -> i64 {
    config.welfare_cooldown_days * DAY_MS
}

/// Was a welfare concern reported for this objective within the cooldown?
pub fn objective_in_welfare_cooldown(
    learning_objective_id: &str,
    recent_sessions: &[RecentSession],
    now: i64,
    config: &ProgressionConfig,
) -> bool {
    let window = welfare_cooldown_ms(config);
    recent_sessions.iter().any(|s| {
        s.learning_objective_id == learning_objective_id
            && s.welfare_concern
            && now - s.completed_at < window
    })
}

/// Does a skill's own history hold a welfare concern within the cooldown?
pub fn has_recent_welfare_in_progress(
    recent_outcomes: &[OutcomeRecord],
    now: i64,
    config: &ProgressionConfig,
) -> bool {
    let window = welfare_cooldown_ms(config);
    recent_outcomes
        .iter()
        .any(|o| o.welfare_concern && now - o.at < window)
}
