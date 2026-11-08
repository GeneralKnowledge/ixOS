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
