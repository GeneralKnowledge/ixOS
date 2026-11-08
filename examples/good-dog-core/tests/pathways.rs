//! Goal pathways: the format, the checks every shipped pathway must pass,
//! matching the owner's own words, the evidence a gate needs, and the
//! goal-steered planner.

mod common;

use std::collections::HashMap;

use common::{build_scenarios, GOLDEN_NOW};
use good_dog_core::coaching::{
    generate_daily_plan, PlanRequest, ProgressSnapshot, ProgressionConfig, RecentSession,
    SessionOutcome, SkillState, DAY_MS,
};
use good_dog_core::dog_training::{
    bundled_exercises, bundled_pathways, check_pathways, match_pathways, parse_pathways,
    stage_readiness, GoalFocusPolicy, Pathway, StageBlocker,
};

fn sample() -> Vec<Pathway> {
    parse_pathways(include_str!("fixtures/pathways-sample.json")).expect("sample parses")
}

#[test]
fn shipped_pathways_parse_and_pass_every_check() {
    let problems = check_pathways(bundled_pathways(), bundled_exercises());
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn the_sample_is_in_the_shipping_format_and_passes_every_check() {
    let pathways = sample();
    assert_eq!(pathways.len(), 2);
    assert_eq!(pathways[0].stages[1].opens_after, ["recall-name"]);
    let problems = check_pathways(&pathways, bundled_exercises());
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn checks_catch_broken_pathways() {
    let mut pathways = sample();
    pathways[0].stages[0].exercise_ids.push("ex-nope".into());
    pathways[0].stages[1].opens_after = vec!["recall-later".into()];
    pathways[0].stages[1].gate.owner_check = " ".into();
    pathways[1].id = "path-recall".into();
    // A puppy-only exercise in an adult-only pathway.
    pathways[1].life_stages = vec![good_dog_core::dog_training::LifeStage::Senior];
    pathways[1].stages[0].exercise_ids = vec!["ex-puppy-sounds".into()];
    let problems = check_pathways(&pathways, bundled_exercises());
    let has = |text: &str| problems.iter().any(|p| p.contains(text));
    assert!(has("unknown exercise ex-nope"), "{problems:#?}");
    assert!(has("opens after recall-later"), "{problems:#?}");
    assert!(has("gate.ownerCheck is empty"), "{problems:#?}");
    assert!(has("pathway id used twice"), "{problems:#?}");
    assert!(has("ex-puppy-sounds suits none"), "{problems:#?}");
}

#[test]
fn own_words_match_pathways_and_nothing_is_guessed() {
    let pathways = sample();
    let ids = |text: &str| -> Vec<String> {
        match_pathways(text, &pathways)
            .into_iter()
            .map(|p| p.id.clone())
            .collect()
    };
    assert_eq!(ids("He PULLS on the lead every walk"), ["path-loose-lead"]);
    assert_eq!(ids("she won't come back when I call"), ["path-recall"]);
    assert!(ids("barks at the postman").is_empty());
    assert!(ids("").is_empty());
}

fn session(exercise: &str, objective: &str, day: i64, outcome: SessionOutcome) -> RecentSession {
    RecentSession {
        exercise_id: exercise.into(),
        learning_objective_id: objective.into(),
        completed_at: GOLDEN_NOW - day * DAY_MS,
        outcome,
        welfare_concern: false,
    }
}

fn progress(state: SkillState) -> HashMap<String, ProgressSnapshot> {
    HashMap::from([(
        "name-response".to_string(),
        ProgressSnapshot {
            state,
            ..ProgressSnapshot::fresh()
        },
    )])
}

#[test]
fn a_gate_needs_a_consistent_skill_and_good_results_on_two_days() {
    let pathways = sample();
    let stage = &pathways[0].stages[0];
    let config = ProgressionConfig::default();
    let readiness = |progress: &HashMap<String, ProgressSnapshot>, sessions: &[RecentSession]| {
        stage_readiness(
            stage,
            bundled_exercises(),
            progress,
            sessions,
            GOLDEN_NOW,
            &config,
        )
    };
    let id = || "ex-name-response".to_string();

    // Never practised.
    let r = readiness(&HashMap::new(), &[]);
    assert_eq!(r.blockers, [StageBlocker::NotStarted { exercise_id: id() }]);
    assert!(!r.ready_for_check() && !r.held());

    // Still practising.
    let r = readiness(&progress(SkillState::Practising), &[]);
    assert_eq!(
        r.blockers,
        [StageBlocker::NeedsMorePractice { exercise_id: id() }]
    );

    // Consistent, but every good session was on one day: completions alone
    // never open a gate.
    let one_day = [
        session("ex-name-response", "name-response", 1, SessionOutcome::Easy),
        session("ex-name-response", "name-response", 1, SessionOutcome::Easy),
        session("ex-name-response", "name-response", 1, SessionOutcome::Easy),
    ];
    let r = readiness(&progress(SkillState::BecomingConsistent), &one_day);
    assert_eq!(
        r.blockers,
        [StageBlocker::NeedsMorePractice { exercise_id: id() }]
    );

    // Two different days with good results: ready to ask the owner.
    let two_days = [
        session("ex-name-response", "name-response", 1, SessionOutcome::Easy),
        session(
            "ex-name-response",
            "name-response",
            3,
            SessionOutcome::GettingThere,
        ),
    ];
    let r = readiness(&progress(SkillState::BecomingConsistent), &two_days);
    assert!(r.ready_for_check(), "{r:?}");
}

#[test]
fn welfare_and_making_it_easier_hold_a_stage() {
    let pathways = sample();
    let stage = &pathways[0].stages[0];
    let config = ProgressionConfig::default();
    let mut sessions = vec![
        session("ex-name-response", "name-response", 1, SessionOutcome::Easy),
        session("ex-name-response", "name-response", 3, SessionOutcome::Easy),
    ];

    let r = stage_readiness(
        stage,
        bundled_exercises(),
        &progress(SkillState::NeedsEasier),
        &sessions,
        GOLDEN_NOW,
        &config,
    );
    assert!(r.held());
    assert_eq!(
        r.blockers,
        [StageBlocker::MakingItEasier {
            exercise_id: "ex-name-response".into()
        }]
    );

    let mut worried = session("ex-name-response", "name-response", 0, SessionOutcome::Easy);
    worried.completed_at = GOLDEN_NOW - 1000;
    worried.welfare_concern = true;
    sessions.push(worried.clone());
    let r = stage_readiness(
        stage,
        bundled_exercises(),
        &progress(SkillState::ReadyToIncrease),
        &sessions,
        GOLDEN_NOW,
        &config,
    );
    assert!(r.held());
    assert_eq!(
        r.blockers,
        [StageBlocker::WelfareRest {
            exercise_id: "ex-name-response".into(),
            until_ms: worried.completed_at + config.welfare_cooldown_days * DAY_MS,
        }]
    );
}

#[test]
fn with_no_focus_the_goal_policy_plans_exactly_like_the_golden_policy() {
    let policy = GoalFocusPolicy {
        focus_exercise_ids: &[],
    };
    for scenario in build_scenarios() {
        let plan = generate_daily_plan(&PlanRequest {
            policy: &policy,
            subject: &scenario.dog,
            exercises: bundled_exercises(),
            progress_by_objective: &scenario.progress,
            recent_sessions: &scenario.recent,
            short_plan: scenario.short_plan,
            config: ProgressionConfig::default(),
            now: GOLDEN_NOW,
        });
        assert_eq!(plan, common::plan_for(&scenario), "{}", scenario.key);
    }
}

#[test]
fn a_focus_exercise_moves_into_the_plan() {
    // A dog past the starter plan, with no stated reason.
    let scenarios = build_scenarios();
    let scenario = scenarios
        .iter()
        .find(|s| !s.progress.is_empty() && !s.short_plan && s.dog.primary_reason.is_empty())
        .expect("a scenario with progress");
    let plan = |focus: &[String]| {
        generate_daily_plan(&PlanRequest {
            policy: &GoalFocusPolicy {
                focus_exercise_ids: focus,
            },
            subject: &scenario.dog,
            exercises: bundled_exercises(),
            progress_by_objective: &scenario.progress,
            recent_sessions: &[],
            short_plan: false,
            config: ProgressionConfig::default(),
            now: GOLDEN_NOW,
        })
    };
    let unfocused: Vec<String> = plan(&[]).items.into_iter().map(|i| i.exercise_id).collect();
    // Pick an eligible exercise the plain plan left out.
    let candidate = bundled_exercises()
        .iter()
        .filter(|e| e.life_stages.contains(&scenario.dog.life_stage))
        .filter(|e| e.core.prerequisite_ids.is_empty())
        .map(|e| e.core.id.clone())
        .find(|id| !unfocused.contains(id))
        .expect("an exercise outside the plan");
    let focused: Vec<String> = plan(std::slice::from_ref(&candidate))
        .items
        .into_iter()
        .map(|i| i.exercise_id)
        .collect();
    assert!(
        focused.contains(&candidate),
        "{candidate} not in {focused:?} (was {unfocused:?})"
    );
}
