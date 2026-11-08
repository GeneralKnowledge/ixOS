//! Parity with the TypeScript engine beyond the web app's own sample of 100.
//! `tests/fixtures/recorded.json` is written by `scripts/export-from-web.sh`
//! from the web app's code. Each entry keeps a readable summary of the chosen
//! exercise ids and a digest of the full text (items, roles, "why today",
//! budget, swap), so a wording change fails too.

mod common;

use std::collections::HashMap;

use common::{build_scenarios, fnv, plan_day, recorded, request, GOLDEN_NOW};
use good_dog_core::coaching::{
    apply_outcome_to_progress, ProgressSnapshot, ProgressionConfig, RecentSession, SessionOutcome,
    SkillState, DAY_MS,
};
use good_dog_core::dog_training::{
    bundled_exercises, bundled_learning_objectives, AvailableTime, DogSubject, LifeStage,
    TrainingExperience,
};

fn report(mismatches: &[String], total: usize) {
    assert!(
        mismatches.is_empty(),
        "{} of {total} differ from the TypeScript engine:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

#[test]
fn every_golden_scenario_and_swap_matches() {
    let recorded = recorded();
    let scenarios = build_scenarios();
    assert_eq!(scenarios.len(), 900);
    assert_eq!(recorded.scenarios.len(), 900);

    let mut mismatches = Vec::new();
    for scenario in &scenarios {
        let expected = &recorded.scenarios[&scenario.key];
        let day = plan_day(&request(
            &scenario.dog,
            &scenario.progress,
            &scenario.recent,
            scenario.short_plan,
            GOLDEN_NOW,
        ));
        if day.ids != expected.ids || fnv(&day.text) != expected.digest {
            mismatches.push(format!(
                "{}: expected {}, got {}\n{}",
                scenario.key, expected.ids, day.ids, day.text
            ));
        }
    }
    report(&mismatches, 900);
}

const STATES: [SkillState; 6] = SkillState::ALL;
const OUTCOMES: [SessionOutcome; 4] = [
    SessionOutcome::Easy,
    SessionOutcome::Easy,
    SessionOutcome::GettingThere,
    SessionOutcome::TooDifficult,
];

fn state_key(state: SkillState) -> String {
    serde_json::to_value(state)
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
}

/// A week of planning and practice per dog, outcomes fed back into progress,
/// exactly as `export-from-web.mjs` simulates it. Covers recent-repeat
/// avoidance, welfare cooldowns, the calm list and progression.
#[test]
fn a_simulated_week_matches_for_every_kind_of_dog() {
    let recorded = recorded();
    let exercises = bundled_exercises();
    let config = ProgressionConfig::default();
    let mut mismatches = Vec::new();
    let mut count = 0;

    for stage in LifeStage::ALL {
        for time in AvailableTime::ALL {
            for reason in ["", "Recall help", "Settle at home"] {
                for start in -1i32..STATES.len() as i32 {
                    count += 1;
                    let dog = DogSubject {
                        id: "d".into(),
                        name: "Moss".into(),
                        life_stage: stage,
                        available_time: time,
                        primary_reason: reason.into(),
                        training_experience: if start < 0 {
                            TrainingExperience::New
                        } else {
                            TrainingExperience::Some
                        },
                    };
                    let mut progress: HashMap<String, ProgressSnapshot> = HashMap::new();
                    if start >= 0 {
                        for (i, objective) in bundled_learning_objectives().iter().enumerate() {
                            let state = STATES[(i + start as usize) % STATES.len()];
                            let easy_streak = match state {
                                SkillState::NeedsEasier | SkillState::NotIntroduced => 0,
                                _ => (i % 4) as u32 + 1,
                            };
                            progress.insert(
                                objective.id.clone(),
                                ProgressSnapshot {
                                    state,
                                    easy_streak,
                                    recent_outcomes: Vec::new(),
                                },
                            );
                        }
                    }
                    let mut recent: Vec<RecentSession> = Vec::new();
                    let mut days = Vec::new();
                    let mut texts = Vec::new();
                    let mut k = 0usize;
                    for d in 0..7i64 {
                        let now = GOLDEN_NOW + d * DAY_MS;
                        let day = plan_day(&request(&dog, &progress, &recent, d == 3, now));
                        days.push(day.ids);
                        texts.push(day.text);
                        for (j, item) in day.plan.items.iter().enumerate() {
                            let exercise = exercises
                                .iter()
                                .find(|e| e.core.id == item.exercise_id)
                                .unwrap();
                            let objective = exercise.core.learning_objective_id.clone();
                            let outcome = OUTCOMES[k % 4];
                            let welfare = k % 7 == 5;
                            let completed_at = now + 600_000 + j as i64 * 60_000;
                            k += 1;
                            recent.insert(
                                0,
                                RecentSession {
                                    exercise_id: exercise.core.id.clone(),
                                    learning_objective_id: objective.clone(),
                                    completed_at,
                                    outcome,
                                    welfare_concern: welfare,
                                },
                            );
                            let current = progress
                                .get(&objective)
                                .cloned()
                                .unwrap_or_else(ProgressSnapshot::fresh);
                            let next = apply_outcome_to_progress(
                                &current,
                                outcome,
                                welfare,
                                &config,
                                completed_at,
                            );
                            progress.insert(objective, next);
                        }
                    }
                    let mut keys: Vec<_> = progress.keys().cloned().collect();
                    keys.sort();
                    for key in keys {
                        let p = &progress[&key];
                        texts.push(format!("{key}={}/{}", state_key(p.state), p.easy_streak));
                    }

                    let key = format!(
                        "{}|{}|{reason}|start={start}",
                        state_key_stage(stage),
                        state_key_time(time)
                    );
                    let expected = &recorded.weeks[&key];
                    if days != expected.days || fnv(&texts.join("\n")) != expected.digest {
                        let first_diff = days
                            .iter()
                            .zip(&expected.days)
                            .position(|(a, b)| a != b)
                            .map_or("final progress or wording".to_string(), |d| {
                                format!("day {d}: expected {}, got {}", expected.days[d], days[d])
                            });
                        mismatches.push(format!("{key}: {first_diff}"));
                    }
                }
            }
        }
    }
    assert_eq!(count, recorded.weeks.len());
    report(&mismatches, count);
}

fn state_key_stage(stage: LifeStage) -> String {
    serde_json::to_value(stage)
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
}

fn state_key_time(time: AvailableTime) -> String {
    serde_json::to_value(time)
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
}
