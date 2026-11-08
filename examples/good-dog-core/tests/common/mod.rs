//! The golden scenarios, built exactly as the web app's `golden-scenarios.ts`
//! builds them, and a helper that formats a plan the way its fixtures record it.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};

use good_dog_core::coaching::{
    generate_daily_plan, pick_plan_item_replacement, GeneratedPlan, PlanRequest, ProgressSnapshot,
    ProgressionConfig, RecentSession, SessionOutcome, SkillState,
};
use good_dog_core::dog_training::{
    bundled_exercises, bundled_learning_objectives, AvailableTime, DogSubject, DogTrainingPolicy,
    LifeStage, TrainingExperience,
};
use serde::Deserialize;

/// 2026-01-15 09:00:00 UTC, the web app's `GOLDEN_NOW`.
pub const GOLDEN_NOW: i64 = 1_768_467_600_000;

pub struct Scenario {
    pub key: String,
    pub dog: DogSubject,
    pub progress: HashMap<String, ProgressSnapshot>,
    pub recent: Vec<RecentSession>,
    pub short_plan: bool,
}

const REASONS: [&str; 6] = [
    "",
    "Recall help",
    "Loose lead walking",
    "Settle at home",
    "Everyday manners",
    "New puppy basics",
];

fn life_stage_key(stage: LifeStage) -> &'static str {
    match stage {
        LifeStage::YoungPuppy => "young_puppy",
        LifeStage::OlderPuppy => "older_puppy",
        LifeStage::Adolescent => "adolescent",
        LifeStage::Adult => "adult",
        LifeStage::Senior => "senior",
    }
}

fn time_key(time: AvailableTime) -> &'static str {
    match time {
        AvailableTime::FewMinutes => "few_minutes",
        AvailableTime::About10 => "about_10",
        AvailableTime::More => "more",
    }
}

fn dog(
    life_stage: LifeStage,
    available_time: AvailableTime,
    reason: &str,
    exp: TrainingExperience,
) -> DogSubject {
    DogSubject {
        id: "d".into(),
        name: "Moss".into(),
        life_stage,
        available_time,
        primary_reason: reason.into(),
        training_experience: exp,
    }
}

pub fn build_scenarios() -> Vec<Scenario> {
    let mut scenarios = Vec::new();

    for stage in LifeStage::ALL {
        for time in AvailableTime::ALL {
            for reason in REASONS {
                for (exp, exp_key) in [
                    (TrainingExperience::New, "new"),
                    (TrainingExperience::Some, "some"),
                ] {
                    for short_plan in [false, true] {
                        scenarios.push(Scenario {
                            key: format!(
                                "empty|{}|{}|{}|{}|{}",
                                life_stage_key(stage),
                                time_key(time),
                                reason,
                                exp_key,
                                short_plan
                            ),
                            dog: dog(stage, time, reason, exp),
                            progress: HashMap::new(),
                            recent: Vec::new(),
                            short_plan,
                        });
                    }
                }
            }
        }
    }

    let objectives = bundled_learning_objectives();
    for rotation in 0..SkillState::ALL.len() {
        let progress: HashMap<String, ProgressSnapshot> = objectives
            .iter()
            .enumerate()
            .map(|(index, objective)| {
                let state = SkillState::ALL[(index + rotation) % SkillState::ALL.len()];
                let easy_streak = match state {
                    SkillState::NeedsEasier | SkillState::NotIntroduced => 0,
                    _ => (index % 4) as u32 + 1,
                };
                (
                    objective.id.clone(),
                    ProgressSnapshot {
                        state,
                        easy_streak,
                        recent_outcomes: Vec::new(),
                    },
                )
            })
            .collect();

        for stage in LifeStage::ALL {
            for time in AvailableTime::ALL {
                for reason in ["", "Recall help", "Loose lead walking"] {
                    for with_recent in [false, true] {
                        let recent = if with_recent {
                            let outcomes = [
                                SessionOutcome::Easy,
                                SessionOutcome::GettingThere,
                                SessionOutcome::TooDifficult,
                                SessionOutcome::Easy,
                            ];
                            objectives
                                .iter()
                                .take(4)
                                .enumerate()
                                .map(|(i, objective)| RecentSession {
                                    exercise_id: format!("ex-{}", objective.id),
                                    learning_objective_id: objective.id.clone(),
                                    completed_at: GOLDEN_NOW - (i as i64 + 1) * 3_600_000,
                                    outcome: outcomes[i],
                                    welfare_concern: false,
                                })
                                .collect()
                        } else {
                            Vec::new()
                        };
                        scenarios.push(Scenario {
                            key: format!(
                                "progress{rotation}|{}|{}|{reason}|recent={with_recent}",
                                life_stage_key(stage),
                                time_key(time)
                            ),
                            dog: dog(stage, time, reason, TrainingExperience::Some),
                            progress: progress.clone(),
                            recent,
                            short_plan: false,
                        });
                    }
                }
            }
        }
    }
    scenarios
}

/// A plan as the web app's golden fixture records it.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedPlan {
    pub items: Vec<String>,
    pub meta: RecordedMeta,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedMeta {
    pub short_plan: bool,
    pub total_minutes: i32,
    pub reason: String,
}

pub fn request<'a>(
    dog: &'a DogSubject,
    progress: &'a HashMap<String, ProgressSnapshot>,
    recent: &'a [RecentSession],
    short_plan: bool,
    now: i64,
) -> PlanRequest<'a, DogTrainingPolicy> {
    PlanRequest {
        policy: &DogTrainingPolicy,
        subject: dog,
        exercises: bundled_exercises(),
        progress_by_objective: progress,
        recent_sessions: recent,
        short_plan,
        config: ProgressionConfig::default(),
        now,
    }
}

pub fn plan_for(scenario: &Scenario) -> GeneratedPlan {
    generate_daily_plan(&request(
        &scenario.dog,
        &scenario.progress,
        &scenario.recent,
        scenario.short_plan,
        GOLDEN_NOW,
    ))
}

pub fn record(plan: &GeneratedPlan) -> RecordedPlan {
    RecordedPlan {
        items: item_lines(plan),
        meta: RecordedMeta {
            short_plan: plan.short_plan,
            total_minutes: plan.total_minutes,
            reason: plan.reason.as_str().to_string(),
        },
    }
}

fn item_lines(plan: &GeneratedPlan) -> Vec<String> {
    plan.items
        .iter()
        .map(|i| format!("{} | {} | {}", i.exercise_id, i.role.as_str(), i.why_today))
        .collect()
}

/// One day as `scripts/export-from-web.mjs` records it: the plan, the swap for
/// its first item, a short id summary and the full text that gets digested.
pub struct Day {
    pub plan: GeneratedPlan,
    pub ids: String,
    pub text: String,
}

pub fn plan_day(request: &PlanRequest<'_, DogTrainingPolicy>) -> Day {
    let plan = generate_daily_plan(request);
    let swap = plan.items.first().and_then(|first| {
        let exclude: Vec<String> = plan.items.iter().map(|i| i.exercise_id.clone()).collect();
        pick_plan_item_replacement(request, &exclude, &first.exercise_id, first.role)
    });
    let ids = format!(
        "{}>{}",
        plan.items
            .iter()
            .map(|i| i.exercise_id.as_str())
            .collect::<Vec<_>>()
            .join(","),
        swap.as_ref().map_or("-", |s| s.exercise_id.as_str())
    );
    let mut lines = item_lines(&plan);
    lines.push(format!(
        "{}/{}/{}",
        plan.short_plan,
        plan.total_minutes,
        plan.reason.as_str()
    ));
    lines.push(swap.map_or("-".to_string(), |s| {
        format!("{} | {}", s.exercise_id, s.why_today)
    }));
    Day {
        plan,
        ids,
        text: lines.join("\n"),
    }
}

/// FNV-1a 64 over UTF-8, as in `scripts/export-from-web.mjs`.
pub fn fnv(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// `tests/fixtures/recorded.json`, written by `scripts/export-from-web.sh`.
#[derive(Deserialize)]
pub struct Recorded {
    pub scenarios: BTreeMap<String, RecordedScenario>,
    pub weeks: BTreeMap<String, RecordedWeek>,
}

#[derive(Deserialize)]
pub struct RecordedScenario {
    pub ids: String,
    pub digest: String,
}

#[derive(Deserialize)]
pub struct RecordedWeek {
    pub days: Vec<String>,
    pub digest: String,
}

pub fn recorded() -> Recorded {
    serde_json::from_str(include_str!("../fixtures/recorded.json")).unwrap()
}
