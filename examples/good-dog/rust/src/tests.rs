use std::sync::Arc;

use forge_runtime::block_on;
use good_dog_core::coaching::DAY_MS;

use super::*;

/// 2026-01-15 09:00 in London (UTC+0 in winter).
const NINE_AM: i64 = 1_768_467_600_000;

fn at(ms: i64) -> LocalTime {
    LocalTime {
        ms,
        utc_offset_minutes: 0,
    }
}

fn open(dir: &std::path::Path) -> Arc<AppCore> {
    AppCore::new(dir.to_string_lossy().into_owned()).unwrap()
}

fn moss() -> DogProfile {
    DogProfile {
        name: "  Moss  ".into(),
        life_stage: DogLifeStage::Adult,
        daily_time: DailyTime::AboutTenMinutes,
        goal: TrainingGoal::Recall,
        experience: OwnerExperience::New,
    }
}

fn onboarded(dir: &std::path::Path) -> Arc<AppCore> {
    let app = open(dir);
    block_on(app.save_dog(moss(), at(NINE_AM))).unwrap();
    app
}

#[test]
fn onboarding_validates_and_tidies_the_profile() {
    let dir = tempfile::tempdir().unwrap();
    let app = open(dir.path());
    assert_eq!(app.dog(), None);
    assert_eq!(block_on(app.today(at(NINE_AM))), Err(AppError::NoDog));

    let mut blank = moss();
    blank.name = "   ".into();
    assert!(matches!(
        block_on(app.save_dog(blank, at(NINE_AM))),
        Err(AppError::InvalidInput { .. })
    ));
    let mut long = moss();
    long.name = "x".repeat(41);
    assert!(matches!(
        block_on(app.save_dog(long, at(NINE_AM))),
        Err(AppError::InvalidInput { .. })
    ));
    assert_eq!(app.dog(), None, "a refused profile changes nothing");

    let mut spaced = moss();
    spaced.name = " Moss   the  Collie ".into();
    let saved = block_on(app.save_dog(spaced, at(NINE_AM))).unwrap();
    assert_eq!(saved.name, "Moss the Collie");
    assert_eq!(open(dir.path()).dog().unwrap().name, "Moss the Collie");
}

#[test]
fn a_new_dog_gets_a_starter_plan_that_stays_the_same_all_day() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded(dir.path());
    let plan = block_on(app.today(at(NINE_AM))).unwrap();
    assert!(plan.starter);
    assert!(!plan.activities.is_empty() && plan.activities.len() <= 3);
    assert!(plan.total_minutes <= 10);
    assert_eq!(plan.activities[0].role, PlanRole::Engage);
    assert!(plan
        .activities
        .iter()
        .all(|a| a.can_swap && a.result.is_none()));
    assert!(plan.can_shorten);

    // Later the same day, and after a relaunch: the same plan.
    let later = block_on(app.today(at(NINE_AM + 6 * 3_600_000))).unwrap();
    assert_eq!(later, plan);
    assert_eq!(block_on(open(dir.path()).today(at(NINE_AM))).unwrap(), plan);
}

#[test]
fn the_day_boundary_follows_the_phones_time_zone() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded(dir.path());
    // 23:30 UTC on the 15th is 00:30 on the 16th at UTC+1.
    let late = NINE_AM + 14 * 3_600_000 + 30 * 60_000;
    let plan_utc = block_on(app.today(at(late))).unwrap();
    let plan_plus_one = block_on(app.today(LocalTime {
        ms: late,
        utc_offset_minutes: 60,
    }))
    .unwrap();
    assert_ne!(plan_utc.activities[0].id, plan_plus_one.activities[0].id);
}

#[test]
fn practising_moves_the_skill_on_and_tomorrow_adapts() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded(dir.path());
    let plan = block_on(app.today(at(NINE_AM))).unwrap();
    let first = &plan.activities[0];
    let feedback = block_on(app.record_session(
        first.exercise_id.clone(),
        Some(first.id),
        SessionResult::Easy,
        false,
        at(NINE_AM + 60_000),
    ))
    .unwrap();
    assert_eq!(feedback.skill.level, SkillLevel::Started);
    assert!(feedback.message.contains("Moss"));

    let plan = block_on(app.today(at(NINE_AM + 120_000))).unwrap();
    assert_eq!(plan.done_count, 1);
    assert_eq!(plan.activities[0].result, Some(SessionResult::Easy));
    assert!(!plan.activities[0].can_swap);
    assert!(!plan.can_shorten, "no short plan once practice started");

    // Recording the same activity twice is refused.
    assert!(matches!(
        block_on(app.record_session(
            first.exercise_id.clone(),
            Some(first.id),
            SessionResult::Easy,
            false,
            at(NINE_AM + 180_000)
        )),
        Err(AppError::InvalidInput { .. })
    ));

    // The next day has a new, adaptive plan.
    let tomorrow = block_on(app.today(at(NINE_AM + DAY_MS))).unwrap();
    assert!(!tomorrow.starter);
    assert_eq!(tomorrow.done_count, 0);

    let progress = app.progress(at(NINE_AM + DAY_MS));
    assert_eq!(progress.sessions_total, 1);
    assert_eq!(progress.sessions_this_week, 1);
    assert_eq!(progress.days_practised_this_week, 1);
    assert_eq!(progress.skills.len(), 1);
    assert_eq!(app.history(10)[0].exercise_title, first.title);
}

#[test]
fn a_welfare_concern_rests_the_skill() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded(dir.path());
    let plan = block_on(app.today(at(NINE_AM))).unwrap();
    let activity = &plan.activities[1];
    let feedback = block_on(app.record_session(
        activity.exercise_id.clone(),
        Some(activity.id),
        SessionResult::Easy,
        true,
        at(NINE_AM + 60_000),
    ))
    .unwrap();
    assert_eq!(feedback.skill.level, SkillLevel::MakingEasier);
    assert!(feedback.message.contains("rest"));

    let objective = app
        .exercise(activity.exercise_id.clone())
        .unwrap()
        .skill
        .objective_id;
    let tomorrow = block_on(app.today(at(NINE_AM + DAY_MS))).unwrap();
    for a in &tomorrow.activities {
        assert_ne!(
            app.exercise(a.exercise_id.clone())
                .unwrap()
                .skill
                .objective_id,
            objective,
            "the concerned skill rests the next day"
        );
    }
}

#[test]
fn swapping_is_limited_and_never_repeats_todays_activities() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded(dir.path());
    let plan = block_on(app.today(at(NINE_AM))).unwrap();
    let id = plan.activities[0].id;
    let before: Vec<String> = plan
        .activities
        .iter()
        .map(|a| a.exercise_id.clone())
        .collect();

    let swapped = block_on(app.swap_activity(id, at(NINE_AM))).unwrap();
    let new = &swapped.activities[0];
    assert_eq!(new.id, id);
    assert!(!before.contains(&new.exercise_id));
    assert!(new
        .why_today
        .starts_with("You asked for something different today."));

    block_on(app.swap_activity(id, at(NINE_AM))).unwrap();
    let twice = block_on(app.today(at(NINE_AM))).unwrap();
    assert!(!twice.activities[0].can_swap);
    assert_eq!(
        block_on(app.swap_activity(id, at(NINE_AM))),
        Err(AppError::NoAlternative)
    );
    assert!(matches!(
        block_on(app.swap_activity(999, at(NINE_AM))),
        Err(AppError::NotFound { .. })
    ));
}

#[test]
fn a_short_plan_has_one_activity_until_practice_starts() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded(dir.path());
    let short = block_on(app.shorten_today(at(NINE_AM))).unwrap();
    assert!(short.short_plan && !short.can_shorten);
    assert_eq!(short.activities.len(), 1);
    assert_eq!(short.activities[0].role, PlanRole::Skill);
    assert!(short.total_minutes <= 4);
    // Asking again changes nothing.
    assert_eq!(block_on(app.shorten_today(at(NINE_AM))).unwrap(), short);
}

#[test]
fn changing_the_profile_rebuilds_a_plan_not_yet_started() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded(dir.path());
    let plan = block_on(app.today(at(NINE_AM))).unwrap();
    let mut puppy = moss();
    puppy.life_stage = DogLifeStage::YoungPuppy;
    puppy.daily_time = DailyTime::FewMinutes;
    block_on(app.save_dog(puppy, at(NINE_AM))).unwrap();
    let rebuilt = block_on(app.today(at(NINE_AM))).unwrap();
    assert_ne!(rebuilt.activities[0].id, plan.activities[0].id);
    assert!(rebuilt.total_minutes <= 5);

    // Once practice starts, the plan stays.
    let a = &rebuilt.activities[0];
    block_on(app.record_session(
        a.exercise_id.clone(),
        Some(a.id),
        SessionResult::GettingThere,
        false,
        at(NINE_AM),
    ))
    .unwrap();
    block_on(app.save_dog(moss(), at(NINE_AM))).unwrap();
    assert_eq!(
        block_on(app.today(at(NINE_AM))).unwrap().activities[0].id,
        a.id
    );
}

#[test]
fn extra_practice_from_learn_counts_without_a_plan_activity() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded(dir.path());
    let feedback = block_on(app.record_session(
        "ex-recall-foundation".into(),
        None,
        SessionResult::TooDifficult,
        false,
        at(NINE_AM),
    ))
    .unwrap();
    assert_eq!(feedback.skill.level, SkillLevel::MakingEasier);
    assert!(matches!(
        block_on(app.record_session("nope".into(), None, SessionResult::Easy, false, at(NINE_AM))),
        Err(AppError::NotFound { .. })
    ));
}

#[test]
fn learn_content_links_terms_and_suits_the_dog() {
    let dir = tempfile::tempdir().unwrap();
    let app = open(dir.path());
    let all: usize = app.learn_topics().iter().map(|t| t.exercises.len()).sum();
    assert_eq!(all, 28);
    block_on(app.save_dog(moss(), at(NINE_AM))).unwrap();
    let adult: usize = app.learn_topics().iter().map(|t| t.exercises.len()).sum();
    assert!(
        adult < all,
        "puppy-only exercises are left out for an adult"
    );

    let detail = app.exercise("ex-name-response".into()).unwrap();
    assert!(!detail.steps.is_empty());
    let linked: Vec<&TextPiece> = detail
        .purpose
        .pieces
        .iter()
        .filter(|p| p.term_id.is_some())
        .collect();
    assert_eq!(linked[0].term_id.as_deref(), Some("positive-reinforcement"));
    assert!(detail.purpose.pieces.iter().all(|p| !p.text.contains("[[")));

    let term = app.term("positive-reinforcement".into()).unwrap();
    assert!(!term.short_definition.is_empty());
    assert!(app.glossary().iter().map(|s| s.terms.len()).sum::<usize>() >= 40);

    let found = app.search_learn("RECALL".into());
    assert!(!found.exercises.is_empty());
    assert!(app.search_learn("  ".into()).exercises.is_empty());
    assert!(matches!(
        app.term("nope".into()),
        Err(AppError::NotFound { .. })
    ));
}

#[test]
fn every_skill_has_a_name_even_where_the_web_content_lacks_one() {
    let dir = tempfile::tempdir().unwrap();
    let app = open(dir.path());
    for id in ["ex-down-comfort", "ex-toilet-routine", "ex-puppy-mouthing"] {
        let skill = app.exercise(id.into()).unwrap().skill;
        assert!(!skill.name.is_empty(), "{id}");
        assert_eq!(skill.level, SkillLevel::NotStarted);
    }
}

#[test]
fn erase_all_starts_again_and_survives_a_relaunch() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded(dir.path());
    let events = app.events().unwrap();
    block_on(app.today(at(NINE_AM))).unwrap();
    assert_eq!(block_on(events.next()), Some(AppEvent::DataChanged));
    block_on(app.erase_all()).unwrap();
    assert_eq!(app.dog(), None);
    assert_eq!(open(dir.path()).dog(), None);
    assert!(app.history(10).is_empty());
}

#[test]
fn unreadable_data_is_reported_and_kept() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("gooddog.json"),
        r#"{"schema_version": 99, "data": {}}"#,
    )
    .unwrap();
    assert!(matches!(
        AppCore::new(dir.path().to_string_lossy().into_owned()),
        Err(AppError::Storage { .. })
    ));
    assert!(dir.path().join("gooddog.json").exists());
}

#[test]
fn shutdown_refuses_new_work_and_ends_streams() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded(dir.path());
    let events = app.events().unwrap();
    app.set_phase(AppPhase::Terminated);
    assert_eq!(
        block_on(events.next()),
        Some(AppEvent::PhaseChanged {
            phase: AppPhase::Terminated,
            resumed: false
        })
    );
    assert_eq!(block_on(events.next()), None);
    assert_eq!(block_on(app.today(at(NINE_AM))), Err(AppError::ShutDown));
}

// MARK: Goals (GD2a)

use good_dog_core::dog_training::{parse_pathways, Pathway};

/// The sample pathways from good-dog-core's tests, plus two copies so the
/// three-goal limit can be reached.
fn sample_pathways() -> &'static [Pathway] {
    static SAMPLE: std::sync::OnceLock<Vec<Pathway>> = std::sync::OnceLock::new();
    SAMPLE.get_or_init(|| {
        let mut pathways = parse_pathways(include_str!(
            "../../../good-dog-core/tests/fixtures/pathways-sample.json"
        ))
        .unwrap();
        for n in 1..=2 {
            let mut copy = pathways[1].clone();
            copy.id = format!("path-copy-{n}");
            copy.title = format!("Copy {n}");
            copy.match_phrases.clear();
            pathways.push(copy);
        }
        pathways
    })
}

fn open_with_goals(dir: &std::path::Path) -> Arc<AppCore> {
    AppCore::with_pathways(dir.to_string_lossy().into_owned(), sample_pathways()).unwrap()
}

fn onboarded_with_goals(dir: &std::path::Path) -> Arc<AppCore> {
    let app = open_with_goals(dir);
    block_on(app.save_dog(moss(), at(NINE_AM))).unwrap();
    app
}

#[test]
fn gd1_data_is_migrated_and_keeps_everything() {
    let dir = tempfile::tempdir().unwrap();
    {
        let app = onboarded(dir.path());
        block_on(app.today(at(NINE_AM))).unwrap();
        app.shutdown();
    }
    // Rewrite the file as GD1 saved it: schema 1, no `goals`.
    let path = dir.path().join("gooddog.json");
    let mut file: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    file["schema_version"] = 1.into();
    file["data"].as_object_mut().unwrap().remove("goals");
    std::fs::write(&path, file.to_string()).unwrap();

    let app = open_with_goals(dir.path());
    assert_eq!(app.dog().unwrap().name, "Moss");
    assert!(app.goals().is_empty());
    let plan = block_on(app.today(at(NINE_AM))).unwrap();
    assert!(!plan.activities.is_empty());
    block_on(app.start_goal("path-recall".into(), None, at(NINE_AM))).unwrap();
    app.shutdown();
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["schema_version"], 2);
}

#[test]
fn the_bundled_app_offers_no_goals_until_dogresearch_publishes_them() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded(dir.path());
    assert!(app.pathways().is_empty());
    assert!(matches!(
        block_on(app.start_goal("path-recall".into(), None, at(NINE_AM))),
        Err(AppError::NotFound { .. })
    ));
    assert_eq!(block_on(app.today(at(NINE_AM))).unwrap().goal_focus, None);
}

#[test]
fn a_new_goal_starts_at_its_first_stage_and_steers_today() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded_with_goals(dir.path());
    assert_eq!(app.pathways().len(), 4);
    assert_eq!(
        app.suggest_goals("she pulls on the lead".into())
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["path-loose-lead"]
    );
    assert!(app.suggest_goals("barks at the postman".into()).is_empty());

    let goal = block_on(app.start_goal(
        "path-recall".into(),
        Some("  runs off   at the park ".into()),
        at(NINE_AM),
    ))
    .unwrap();
    assert_eq!(goal.summary.status, GoalStatus::Active);
    assert_eq!(goal.summary.stage_number, 1);
    assert_eq!(goal.summary.stage_count, 2);
    assert_eq!(
        goal.summary.own_words.as_deref(),
        Some("runs off at the park")
    );
    assert_eq!(
        goal.stages.iter().map(|s| s.state).collect::<Vec<_>>(),
        [StageState::Current, StageState::Upcoming]
    );
    let gate = goal.gate.unwrap();
    assert!(!gate.ready_for_check && !gate.held);
    assert!(gate.question.contains("Moss"), "{}", gate.question);
    assert!(!gate.question.contains("{dog}"));

    // Today's plan (not started yet) is rebuilt around the goal.
    let today = block_on(app.today(at(NINE_AM))).unwrap();
    let focus = today.goal_focus.expect("goal focus");
    assert_eq!(focus.goal_id, goal.summary.id);
    assert_eq!(focus.stage_title, "Turning to their name");
    assert!(
        today
            .activities
            .iter()
            .any(|a| a.exercise_id == "ex-name-response"),
        "{:?}",
        today.activities
    );

    // Starting it again returns the same goal.
    let again = block_on(app.start_goal("path-recall".into(), None, at(NINE_AM))).unwrap();
    assert_eq!(again.summary.id, goal.summary.id);
    assert_eq!(app.goals().len(), 1);
}

#[test]
fn pausing_resuming_and_setting_aside_keep_the_stage_and_history() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded_with_goals(dir.path());
    let id = block_on(app.start_goal("path-recall".into(), None, at(NINE_AM)))
        .unwrap()
        .summary
        .id;
    block_on(app.answer_gate(id, true, at(NINE_AM))).unwrap();

    let paused = block_on(app.pause_goal(id, at(NINE_AM + 1000))).unwrap();
    assert_eq!(paused.summary.status, GoalStatus::Paused);
    assert_eq!(paused.summary.stage_number, 2);
    assert!(!paused.gate.as_ref().unwrap().note.is_empty());
    // A paused goal no longer steers the plan.
    let tomorrow = at(NINE_AM + DAY_MS);
    assert_eq!(block_on(app.today(tomorrow)).unwrap().goal_focus, None);

    let aside = block_on(app.set_goal_aside(id, tomorrow)).unwrap();
    assert_eq!(aside.summary.status, GoalStatus::SetAside);
    let resumed = block_on(app.resume_goal(id, tomorrow)).unwrap();
    assert_eq!(resumed.summary.status, GoalStatus::Active);
    assert_eq!(resumed.summary.stage_number, 2, "kept its stage");
    let history: Vec<&str> = resumed.history.iter().map(|e| e.text.as_str()).collect();
    assert_eq!(
        history,
        [
            "Picked up again",
            "Set aside",
            "Paused",
            "Moved on from \u{201c}Turning to their name\u{201d}",
            "Started this goal"
        ]
    );

    // All of it survives a relaunch.
    app.shutdown();
    let app = open_with_goals(dir.path());
    let again = app.goal(id, tomorrow).unwrap();
    assert_eq!(again.summary.stage_number, 2);
    assert_eq!(again.history.len(), 5);
}

#[test]
fn at_most_three_goals_are_active() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded_with_goals(dir.path());
    let start = |id: &str| block_on(app.start_goal(id.into(), None, at(NINE_AM)));
    let first = start("path-recall").unwrap().summary.id;
    start("path-loose-lead").unwrap();
    start("path-copy-1").unwrap();
    assert!(matches!(
        start("path-copy-2"),
        Err(AppError::InvalidInput { .. })
    ));
    block_on(app.pause_goal(first, at(NINE_AM))).unwrap();
    start("path-copy-2").unwrap();
    assert!(matches!(
        block_on(app.resume_goal(first, at(NINE_AM))),
        Err(AppError::InvalidInput { .. })
    ));
    let statuses: Vec<GoalStatus> = app.goals().iter().map(|g| g.status).collect();
    assert_eq!(
        statuses,
        [
            GoalStatus::Active,
            GoalStatus::Active,
            GoalStatus::Active,
            GoalStatus::Paused
        ]
    );
}

#[test]
fn the_gate_moves_on_finishes_and_steps_back() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded_with_goals(dir.path());
    let id = block_on(app.start_goal("path-recall".into(), None, at(NINE_AM)))
        .unwrap()
        .summary
        .id;

    let not_yet = block_on(app.answer_gate(id, false, at(NINE_AM))).unwrap();
    assert_eq!(not_yet.summary.stage_number, 1);
    assert!(not_yet.history[0].text.starts_with("Not quite there"));

    let second = block_on(app.answer_gate(id, true, at(NINE_AM))).unwrap();
    assert_eq!(second.summary.stage_number, 2);
    assert_eq!(second.stages[0].state, StageState::Passed);

    let done = block_on(app.answer_gate(id, true, at(NINE_AM))).unwrap();
    assert_eq!(done.summary.status, GoalStatus::Finished);
    assert!(done.gate.is_none());
    assert!(done.stages.iter().all(|s| s.state == StageState::Passed));
    assert!(matches!(
        block_on(app.pause_goal(id, at(NINE_AM))),
        Err(AppError::InvalidInput { .. })
    ));

    // Going back is always allowed, and reopens a finished goal.
    let back = block_on(app.step_back(id, at(NINE_AM))).unwrap();
    assert_eq!(back.summary.status, GoalStatus::Active);
    assert_eq!(back.summary.stage_number, 1);
    assert!(matches!(
        block_on(app.step_back(id, at(NINE_AM))),
        Err(AppError::InvalidInput { .. })
    ));
}

#[test]
fn evidence_opens_the_gate_and_welfare_holds_it() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded_with_goals(dir.path());
    let id = block_on(app.start_goal("path-recall".into(), None, at(NINE_AM)))
        .unwrap()
        .summary
        .id;
    let record = |day: i64, result: SessionResult, welfare: bool| {
        block_on(app.record_session(
            "ex-name-response".into(),
            None,
            result,
            welfare,
            at(NINE_AM + day * DAY_MS),
        ))
        .unwrap()
    };
    // Good results on several days make the skill consistent.
    for day in 0..4 {
        record(day, SessionResult::Easy, false);
    }
    let gate = app
        .goal(id, at(NINE_AM + 4 * DAY_MS))
        .unwrap()
        .gate
        .unwrap();
    assert!(gate.ready_for_check, "{gate:?}");

    // A welfare concern holds the stage: "yes" is refused, nothing moves.
    record(4, SessionResult::GettingThere, true);
    let later = at(NINE_AM + 4 * DAY_MS + 1000);
    let gate = app.goal(id, later).unwrap().gate.unwrap();
    assert!(gate.held && !gate.ready_for_check, "{gate:?}");
    assert!(gate.note.contains("welfare"), "{}", gate.note);
    match block_on(app.answer_gate(id, true, later)) {
        Err(AppError::InvalidInput { reason }) => assert!(reason.contains("welfare"), "{reason}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(app.goal(id, later).unwrap().summary.stage_number, 1);

    // After the rest, the stage stays held while the skill is made easier...
    let gate = app
        .goal(id, at(NINE_AM + 8 * DAY_MS))
        .unwrap()
        .gate
        .unwrap();
    assert!(gate.held && !gate.note.contains("welfare"), "{gate:?}");
    assert!(gate.note.contains("made easier"), "{}", gate.note);

    // ...and opens again once it goes well.
    for day in 8..12 {
        record(day, SessionResult::Easy, false);
    }
    let gate = app
        .goal(id, at(NINE_AM + 12 * DAY_MS))
        .unwrap()
        .gate
        .unwrap();
    assert!(!gate.held && gate.ready_for_check, "{gate:?}");
}

#[test]
fn erasing_clears_goals() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded_with_goals(dir.path());
    block_on(app.start_goal("path-recall".into(), None, at(NINE_AM))).unwrap();
    block_on(app.erase_all()).unwrap();
    assert!(app.goals().is_empty());
    assert_eq!(app.pathways().len(), 4, "the offered pathways stay");
}

// MARK: Guided session (GD-7)

/// Steps through a guided session to its outcome card.
fn to_outcome(session: &GuidedSession) {
    while session.card().can_go_next {
        session.next().unwrap();
    }
    assert_eq!(session.card().kind, GuidedCardKind::Outcome);
}

#[test]
fn a_guided_session_shows_get_ready_then_each_step_then_the_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded(dir.path());
    let activity = block_on(app.today(at(NINE_AM))).unwrap().activities[0].clone();
    let detail = app.exercise(activity.exercise_id.clone()).unwrap();
    let session = app
        .start_guided_session(activity.exercise_id.clone(), Some(activity.id), at(NINE_AM))
        .unwrap();
    assert_eq!(session.title(), detail.title);
    assert_eq!(session.if_difficult(), detail.if_difficult);

    let card = session.card();
    assert_eq!(card.kind, GuidedCardKind::GetReady);
    assert_eq!(card.text, detail.preparation);
    assert_eq!(card.step_count, detail.steps.len() as u32);
    assert_eq!(card.card_count, detail.steps.len() as u32 + 2);
    assert!(!card.can_go_back && card.can_go_next && !card.can_record);
    assert!(matches!(session.back(), Err(AppError::InvalidInput { .. })));

    for (i, step) in detail.steps.iter().enumerate() {
        let card = session.next().unwrap();
        assert_eq!(card.kind, GuidedCardKind::Step);
        assert_eq!(card.step_number, Some(i as u32 + 1));
        assert_eq!(&card.text, step);
        assert!(card.can_go_back && !card.can_record);
    }
    let outcome = session.next().unwrap();
    assert_eq!(outcome.kind, GuidedCardKind::Outcome);
    assert_eq!(outcome.text, detail.look_for);
    assert!(outcome.can_record && !outcome.can_go_next);
    let last = session.back().unwrap();
    assert_eq!(last.step_number, Some(detail.steps.len() as u32));
}

#[test]
fn recording_a_guided_session_matches_recording_from_the_exercise_screen() {
    let guided_dir = tempfile::tempdir().unwrap();
    let plain_dir = tempfile::tempdir().unwrap();
    let guided = onboarded(guided_dir.path());
    let plain = onboarded(plain_dir.path());
    let activity = block_on(guided.today(at(NINE_AM))).unwrap().activities[0].clone();
    assert_eq!(
        block_on(plain.today(at(NINE_AM))).unwrap().activities[0],
        activity
    );

    let session = guided
        .start_guided_session(activity.exercise_id.clone(), Some(activity.id), at(NINE_AM))
        .unwrap();
    // Not before the last step.
    assert!(matches!(
        block_on(guided.record_guided_session(
            session.clone(),
            SessionResult::GettingThere,
            false,
            at(NINE_AM)
        )),
        Err(AppError::InvalidInput { .. })
    ));
    assert!(guided.history(10).is_empty());
    to_outcome(&session);

    let later = at(NINE_AM + 60_000);
    let from_guided = block_on(guided.record_guided_session(
        session.clone(),
        SessionResult::GettingThere,
        false,
        later,
    ))
    .unwrap();
    let from_screen = block_on(plain.record_session(
        activity.exercise_id.clone(),
        Some(activity.id),
        SessionResult::GettingThere,
        false,
        later,
    ))
    .unwrap();
    assert_eq!(from_guided, from_screen);
    assert_eq!(guided.history(10), plain.history(10));
    assert_eq!(guided.progress(later), plain.progress(later));
    assert_eq!(
        block_on(guided.today(later)).unwrap(),
        block_on(plain.today(later)).unwrap()
    );
    // The saved files are the same too: one rule, one record.
    assert_eq!(
        std::fs::read_to_string(guided_dir.path().join("gooddog.json")).unwrap(),
        std::fs::read_to_string(plain_dir.path().join("gooddog.json")).unwrap()
    );

    // Recorded once, then it stays put.
    let card = session.card();
    assert!(card.recorded && !card.can_record && !card.can_go_back);
    assert!(matches!(
        block_on(guided.record_guided_session(session.clone(), SessionResult::Easy, false, later)),
        Err(AppError::InvalidInput { .. })
    ));
    assert_eq!(guided.history(10).len(), 1);
    // A done activity can't start another guided session.
    assert!(matches!(
        guided.start_guided_session(activity.exercise_id, Some(activity.id), later),
        Err(AppError::InvalidInput { .. })
    ));
}

#[test]
fn a_guided_welfare_concern_rests_the_skill_like_the_exercise_screen() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded(dir.path());
    let activity = block_on(app.today(at(NINE_AM))).unwrap().activities[0].clone();
    let session = app
        .start_guided_session(activity.exercise_id.clone(), Some(activity.id), at(NINE_AM))
        .unwrap();
    to_outcome(&session);
    let feedback =
        block_on(app.record_guided_session(session, SessionResult::Easy, true, at(NINE_AM)))
            .unwrap();
    assert_eq!(feedback.skill.level, SkillLevel::MakingEasier);
    assert!(app.history(1)[0].welfare_concern);
}

#[test]
fn leaving_a_guided_session_part_way_records_nothing_and_the_next_starts_again() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded(dir.path());
    let activity = block_on(app.today(at(NINE_AM))).unwrap().activities[0].clone();
    let saved = std::fs::read_to_string(dir.path().join("gooddog.json")).unwrap();
    let session = app
        .start_guided_session(activity.exercise_id.clone(), Some(activity.id), at(NINE_AM))
        .unwrap();
    session.next().unwrap();
    session.next().unwrap();
    drop(session);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("gooddog.json")).unwrap(),
        saved,
        "nothing saved"
    );
    assert!(app.history(10).is_empty());
    assert_eq!(app.progress(at(NINE_AM)).sessions_total, 0);

    // The app terminated part way: the reopened app knows nothing of it.
    app.shutdown();
    let reopened = open(dir.path());
    assert!(reopened.history(10).is_empty());
    let again = reopened
        .start_guided_session(activity.exercise_id, Some(activity.id), at(NINE_AM))
        .unwrap();
    assert_eq!(again.card().kind, GuidedCardKind::GetReady);
    assert_eq!(again.card().position, 0);
}

#[test]
fn a_guided_session_needs_what_a_record_needs() {
    let dir = tempfile::tempdir().unwrap();
    let app = open(dir.path());
    assert!(matches!(
        app.start_guided_session("ex-name-game".into(), None, at(NINE_AM)),
        Err(AppError::NoDog)
    ));
    block_on(app.save_dog(moss(), at(NINE_AM))).unwrap();
    assert!(matches!(
        app.start_guided_session("no-such-exercise".into(), None, at(NINE_AM)),
        Err(AppError::NotFound { .. })
    ));
    let plan = block_on(app.today(at(NINE_AM))).unwrap();
    let (a, b) = (&plan.activities[0], &plan.activities[1]);
    assert!(matches!(
        app.start_guided_session(b.exercise_id.clone(), Some(a.id), at(NINE_AM)),
        Err(AppError::InvalidInput { .. })
    ));
    // Extra practice from Learn, with no activity, is fine.
    let extra = app
        .start_guided_session(a.exercise_id.clone(), None, at(NINE_AM))
        .unwrap();
    to_outcome(&extra);
    block_on(app.record_guided_session(extra, SessionResult::Easy, false, at(NINE_AM))).unwrap();
    assert_eq!(app.history(10).len(), 1);
    assert_eq!(block_on(app.today(at(NINE_AM))).unwrap().done_count, 0);
    app.shutdown();
    assert!(matches!(
        app.start_guided_session(a.exercise_id.clone(), None, at(NINE_AM)),
        Err(AppError::ShutDown)
    ));
}

/// `uitests/guided.toml` taps Next a fixed number of times. It launches
/// with `-ForgeUITestData -ForgeUITestSampleDog`: the sample dog
/// (`GoodDogModel.launchSampleDog`) on the fixed clock (`LocalTime.uiTest`).
/// If this fails, the plan or the content changed: update the test file.
#[test]
fn the_ui_tests_sample_dog_starts_today_on_a_four_step_exercise() {
    let dir = tempfile::tempdir().unwrap();
    let app = open(dir.path());
    let ui_test = LocalTime {
        ms: 1_794_216_600_000,
        utc_offset_minutes: 0,
    };
    let sample = DogProfile {
        name: "Moss".into(),
        life_stage: DogLifeStage::Adult,
        daily_time: DailyTime::AboutTenMinutes,
        goal: TrainingGoal::Recall,
        experience: OwnerExperience::New,
    };
    block_on(app.save_dog(sample, ui_test)).unwrap();
    let first = block_on(app.today(ui_test)).unwrap().activities[0].clone();
    let steps = app.exercise(first.exercise_id.clone()).unwrap().steps.len();
    assert_eq!((first.exercise_id.as_str(), steps), ("ex-name-response", 4));
}

// MARK: Reminder (GD5)

fn reminder_days(app: &AppCore, now: LocalTime) -> Vec<i64> {
    lock(&app.state)
        .reminders(now)
        .iter()
        .map(|n| reminders::day_of(&n.id).unwrap())
        .collect()
}

#[test]
fn the_reminder_skips_a_day_already_practised_or_set_aside() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded(dir.path());
    let today = training::local_day(at(NINE_AM));
    assert_eq!(app.reminder_minute(), None);
    assert!(
        reminder_days(&app, at(NINE_AM)).is_empty(),
        "off until turned on"
    );

    // 18:00. No native services on Linux, so no permission prompt.
    assert!(block_on(app.set_reminder(Some(18 * 60), at(NINE_AM))).unwrap());
    assert_eq!(app.reminder_minute(), Some(18 * 60));
    let week: Vec<i64> = (today..today + 7).collect();
    assert_eq!(reminder_days(&app, at(NINE_AM)), week);
    let first = &lock(&app.state).reminders(at(NINE_AM))[0];
    assert_eq!(
        first.trigger,
        native::NotificationTrigger::At {
            unix_ms: today * DAY_MS + 18 * 3_600_000
        }
    );
    assert_eq!(first.category.as_deref(), Some(reminders::CATEGORY));
    assert!(first.title.contains("Moss"));

    // After 18:00 today's has gone by.
    assert_eq!(
        reminder_days(&app, at(NINE_AM + 10 * 3_600_000))[0],
        today + 1
    );

    // Practising today drops today's.
    let plan = block_on(app.today(at(NINE_AM))).unwrap();
    let activity = &plan.activities[0];
    block_on(app.record_session(
        activity.exercise_id.clone(),
        Some(activity.id),
        SessionResult::Easy,
        false,
        at(NINE_AM + 60_000),
    ))
    .unwrap();
    assert_eq!(reminder_days(&app, at(NINE_AM))[0], today + 1);

    // "Not today" from tomorrow's notification drops tomorrow's, tomorrow.
    let tomorrow = at(NINE_AM + DAY_MS);
    assert_eq!(reminder_days(&app, tomorrow)[0], today + 1);
    block_on(app.skip_reminder_day(today + 1)).unwrap();
    assert_eq!(reminder_days(&app, tomorrow)[0], today + 2);

    // Kept across a relaunch; off again after erasing.
    drop(app);
    let app = open(dir.path());
    assert_eq!(app.reminder_minute(), Some(18 * 60));
    block_on(app.erase_all()).unwrap();
    assert_eq!(app.reminder_minute(), None);
    assert!(block_on(app.set_reminder(Some(24 * 60), at(NINE_AM))).is_err());
}

#[test]
fn rescheduling_replaces_the_week_and_cancels_what_is_no_longer_wanted() {
    let dir = tempfile::tempdir().unwrap();
    let app = onboarded(dir.path());
    let (services, fakes) = native::NativeServices::for_tests(dir.path().join("native"));
    // The fake refuses times already past, like iOS.
    let now = at(forge_runtime::now_ms());
    block_on(app.set_reminder(Some(18 * 60), now)).unwrap();
    block_on(reminders::schedule(
        &services,
        lock(&app.state).reminders(now),
        now,
    ));
    let week = fakes.notifier.scheduled().len();
    assert!(week == 6 || week == 7, "{week}");

    block_on(app.set_reminder(None, now)).unwrap();
    block_on(reminders::schedule(
        &services,
        lock(&app.state).reminders(now),
        now,
    ));
    assert!(fakes.notifier.scheduled().is_empty());

    let buttons = reminders::category().actions;
    assert_eq!(buttons[0].id, reminders::START);
    assert_eq!(buttons[1].id, reminders::NOT_TODAY);
}
