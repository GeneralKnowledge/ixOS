//! Engine behaviour with a tiny made-up domain, ported from the web app's
//! `coaching/*.test.ts`. If the engine only worked for dogs, it would show here.

use std::collections::HashMap;

use good_dog_core::coaching::{
    apply_outcome_to_progress, generate_daily_plan, pick_plan_item_replacement,
    should_prefer_easier_variation, should_prefer_harder_variation, CoachingExercise,
    CoachingPolicy, CoachingSubject, ExerciseCore, GeneratedPlan, OutcomeRecord, PlanBudget,
    PlanItemRole, PlanReason, PlanRequest, ProgressSnapshot, ProgressionConfig, RecentSession,
    SessionOutcome, SkillState, WhyTodayInput,
};

#[derive(Clone)]
struct Step {
    core: ExerciseCore,
    advanced: bool,
}

impl CoachingExercise for Step {
    fn core(&self) -> &ExerciseCore {
        &self.core
    }
}

struct Learner {
    advanced: bool,
    minutes: i32,
}

impl CoachingSubject for Learner {
    fn name(&self) -> &str {
        "Sam"
    }
}

#[derive(Default)]
struct Policy {
    nothing_eligible: bool,
    no_starters: bool,
    budget: Option<PlanBudget>,
}

impl CoachingPolicy for Policy {
    type Exercise = Step;
    type Subject = Learner;

    fn is_eligible(&self, exercise: &Step, learner: &Learner) -> bool {
        !self.nothing_eligible && (learner.advanced || !exercise.advanced)
    }

    fn daily_budget(&self, learner: &Learner) -> PlanBudget {
        self.budget.unwrap_or(PlanBudget {
            max_minutes: learner.minutes,
            item_count: 3,
        })
    }

    fn starter_exercises<'a>(&self, eligible: &[&'a Step], _: &Learner) -> Vec<&'a Step> {
        if self.no_starters {
            return Vec::new();
        }
        eligible
            .iter()
            .copied()
            .filter(|e| e.core.id == "a")
            .collect()
    }

    fn affinity_score(&self, _: &Step, _: &Learner) -> i32 {
        0
    }

    fn explain(&self, input: &WhyTodayInput<'_>) -> String {
        let mut parts = vec![input.role.as_str()];
        if input.preferred_easier {
            parts.push("easier");
        }
        if input.preferred_harder {
            parts.push("harder");
        }
        if input.is_starter {
            parts.push("starter");
        }
        parts.join(",")
    }
}

fn step(id: &str) -> Step {
    Step {
        core: ExerciseCore {
            id: id.into(),
            learning_objective_id: format!("skill-{id}"),
            category: format!("cat-{id}"),
            difficulty: 1,
            estimated_minutes: 2,
            prerequisite_ids: Vec::new(),
            easier_variation_id: None,
            harder_variation_id: None,
        },
        advanced: false,
    }
}

fn library() -> Vec<Step> {
    let mut d = step("d");
    d.advanced = true;
    let mut needs_a = step("needs-a");
    needs_a.core.prerequisite_ids = vec!["a".into()];
    vec![step("a"), step("b"), step("c"), d, needs_a]
}

fn progress(state: SkillState, easy_streak: u32) -> ProgressSnapshot {
    ProgressSnapshot {
        state,
        easy_streak,
        recent_outcomes: Vec::new(),
    }
}

fn only(objective: &str, state: SkillState) -> HashMap<String, ProgressSnapshot> {
    HashMap::from([(objective.to_string(), progress(state, 0))])
}

const NOW: i64 = 1_768_467_600_000;

fn plan_with(
    policy: &Policy,
    learner: &Learner,
    exercises: &[Step],
    progress: &HashMap<String, ProgressSnapshot>,
    recent: &[RecentSession],
    short_plan: bool,
) -> GeneratedPlan {
    generate_daily_plan(&PlanRequest {
        policy,
        subject: learner,
        exercises,
        progress_by_objective: progress,
        recent_sessions: recent,
        short_plan,
        config: ProgressionConfig::default(),
        now: NOW,
    })
}

fn ids(plan: &GeneratedPlan) -> Vec<&str> {
    plan.items.iter().map(|i| i.exercise_id.as_str()).collect()
}

const LEARNER: Learner = Learner {
    advanced: false,
    minutes: 10,
};

#[test]
fn uses_the_starter_list_until_there_is_progress() {
    let plan = plan_with(
        &Policy::default(),
        &LEARNER,
        &library(),
        &HashMap::new(),
        &[],
        false,
    );
    assert_eq!(ids(&plan), ["a"]);
    assert_eq!(plan.reason, PlanReason::Starter);
    assert!(plan.items[0].why_today.contains("starter"));
}

#[test]
fn filters_by_the_policys_eligibility() {
    let policy = Policy {
        no_starters: true,
        ..Policy::default()
    };
    let plan = plan_with(
        &policy,
        &LEARNER,
        &library(),
        &only("skill-a", SkillState::Practising),
        &[],
        false,
    );
    assert!(!ids(&plan).contains(&"d"));
}

#[test]
fn offers_an_exercise_only_once_its_prerequisites_are_under_way() {
    let locked = plan_with(
        &Policy::default(),
        &LEARNER,
        &library(),
        &only("skill-a", SkillState::NotIntroduced),
        &[],
        false,
    );
    assert!(!ids(&locked).contains(&"needs-a"));

    let roomy = Policy {
        budget: Some(PlanBudget {
            max_minutes: 30,
            item_count: 10,
        }),
        ..Policy::default()
    };
    let unlocked = plan_with(
        &roomy,
        &LEARNER,
        &library(),
        &only("skill-a", SkillState::Practising),
        &[],
        false,
    );
    assert!(ids(&unlocked).contains(&"needs-a"));
}

#[test]
fn respects_time_and_item_limits() {
    let learner = Learner {
        advanced: false,
        minutes: 4,
    };
    let plan = plan_with(
        &Policy::default(),
        &learner,
        &library(),
        &only("skill-a", SkillState::Practising),
        &[],
        false,
    );
    assert!(plan.total_minutes <= 4);
}

#[test]
fn a_short_plan_has_one_skill_item() {
    let plan = plan_with(
        &Policy::default(),
        &LEARNER,
        &library(),
        &only("skill-a", SkillState::Practising),
        &[],
        true,
    );
    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.items[0].role, PlanItemRole::Skill);
    assert_eq!(plan.reason, PlanReason::ShortOnTime);
}

#[test]
fn swaps_in_the_easier_variation_when_a_skill_needs_easier() {
    let mut hard = step("hard");
    hard.core.easier_variation_id = Some("easy".into());
    hard.core.difficulty = 3;
    let mut easy = step("easy");
    easy.core.learning_objective_id = "skill-easy".into();
    let policy = Policy {
        no_starters: true,
        ..Policy::default()
    };
    let plan = plan_with(
        &policy,
        &LEARNER,
        &[hard, easy],
        &only("skill-hard", SkillState::NeedsEasier),
        &[],
        false,
    );
    assert_eq!(plan.items[0].exercise_id, "easy");
    assert!(plan.items[0].why_today.contains("easier"));
}

#[test]
fn swaps_in_the_harder_variation_when_a_skill_is_ready() {
    let mut base = step("base");
    base.core.harder_variation_id = Some("stretch".into());
    let mut stretch = step("stretch");
    stretch.core.learning_objective_id = "skill-stretch".into();
    stretch.core.difficulty = 2;
    let policy = Policy {
        no_starters: true,
        ..Policy::default()
    };
    let ready = HashMap::from([(
        "skill-base".to_string(),
        progress(SkillState::ReadyToIncrease, 3),
    )]);
    let plan = plan_with(&policy, &LEARNER, &[base, stretch], &ready, &[], false);
    assert!(ids(&plan).contains(&"stretch"));
}

#[test]
fn always_returns_one_item_when_anything_is_eligible() {
    let learner = Learner {
        advanced: false,
        minutes: 0,
    };
    let plan = plan_with(
        &Policy::default(),
        &learner,
        &library(),
        &only("skill-a", SkillState::Practising),
        &[],
        false,
    );
    assert!(!plan.items.is_empty());
}

#[test]
fn returns_an_empty_plan_when_nothing_is_eligible() {
    let policy = Policy {
        nothing_eligible: true,
        ..Policy::default()
    };
    let plan = plan_with(&policy, &LEARNER, &library(), &HashMap::new(), &[], false);
    assert!(plan.items.is_empty());
}

#[test]
fn leaves_out_an_objective_after_a_welfare_concern() {
    let policy = Policy {
        no_starters: true,
        budget: Some(PlanBudget {
            max_minutes: 30,
            item_count: 10,
        }),
        ..Policy::default()
    };
    let recent = [RecentSession {
        exercise_id: "b".into(),
        learning_objective_id: "skill-b".into(),
        completed_at: NOW - 60_000,
        outcome: SessionOutcome::Easy,
        welfare_concern: true,
    }];
    let plan = plan_with(
        &policy,
        &LEARNER,
        &library(),
        &only("skill-a", SkillState::Practising),
        &recent,
        false,
    );
    assert!(!plan.items.is_empty());
    assert!(!ids(&plan).contains(&"b"));
}

#[test]
fn a_swap_never_offers_an_excluded_exercise() {
    let policy = Policy::default();
    let exercises = library();
    let progress = only("skill-a", SkillState::Practising);
    let request = PlanRequest {
        policy: &policy,
        subject: &LEARNER,
        exercises: &exercises,
        progress_by_objective: &progress,
        recent_sessions: &[],
        short_plan: false,
        config: ProgressionConfig::default(),
        now: NOW,
    };
    let exclude = vec!["a".to_string(), "b".to_string()];
    let pick = pick_plan_item_replacement(&request, &exclude, "a", PlanItemRole::Skill).unwrap();
    assert!(!exclude.contains(&pick.exercise_id));
    assert!(pick
        .why_today
        .starts_with("You asked for something different today."));

    let everything: Vec<String> = exercises.iter().map(|e| e.core.id.clone()).collect();
    assert!(pick_plan_item_replacement(&request, &everything, "a", PlanItemRole::Skill).is_none());
    assert!(pick_plan_item_replacement(&request, &[], "missing", PlanItemRole::Skill).is_none());
}

// Progression, from progression.test.ts.

fn apply(p: &ProgressSnapshot, outcome: SessionOutcome, welfare: bool) -> ProgressSnapshot {
    apply_outcome_to_progress(p, outcome, welfare, &ProgressionConfig::default(), NOW)
}

#[test]
fn one_easy_result_is_never_ready_to_increase() {
    let next = apply(&ProgressSnapshot::fresh(), SessionOutcome::Easy, false);
    assert!(matches!(
        next.state,
        SkillState::Introduced | SkillState::Practising
    ));
}

#[test]
fn repeated_easy_results_progress_gradually() {
    let config = ProgressionConfig::default();
    let mut p = ProgressSnapshot::fresh();
    for _ in 0..config.easy_results_for_increase {
        p = apply(&p, SessionOutcome::Easy, false);
    }
    assert!(matches!(
        p.state,
        SkillState::BecomingConsistent | SkillState::ReadyToIncrease
    ));
    while p.state != SkillState::ReadyToIncrease
        && p.easy_streak < config.easy_results_for_increase + 3
    {
        p = apply(&p, SessionOutcome::Easy, false);
    }
    assert_eq!(p.state, SkillState::ReadyToIncrease);
    assert!(should_prefer_harder_variation(&p));
}

#[test]
fn mixed_results_consolidate() {
    let p = apply(&ProgressSnapshot::fresh(), SessionOutcome::Easy, false);
    let p = apply(&p, SessionOutcome::GettingThere, false);
    assert_eq!(p.state, SkillState::Practising);
    assert_eq!(p.easy_streak, 0);
    assert!(!should_prefer_harder_variation(&p));
}

#[test]
fn too_difficult_needs_easier() {
    let p = apply(
        &progress(SkillState::Practising, 2),
        SessionOutcome::TooDifficult,
        false,
    );
    assert_eq!(p.state, SkillState::NeedsEasier);
    assert!(should_prefer_easier_variation(&p));
}

#[test]
fn a_welfare_concern_overrides_progression() {
    let p = apply(
        &progress(SkillState::ReadyToIncrease, 4),
        SessionOutcome::Easy,
        true,
    );
    assert_eq!(p.state, SkillState::NeedsEasier);
    assert_eq!(p.easy_streak, 0);
}

#[test]
fn one_easy_does_not_clear_needs_easier_during_a_welfare_cooldown() {
    let current = ProgressSnapshot {
        state: SkillState::NeedsEasier,
        easy_streak: 0,
        recent_outcomes: vec![OutcomeRecord {
            outcome: SessionOutcome::Easy,
            welfare_concern: true,
            at: NOW - 60_000,
        }],
    };
    assert_eq!(
        apply(&current, SessionOutcome::Easy, false).state,
        SkillState::NeedsEasier
    );
}

#[test]
fn getting_there_resets_the_streak_and_never_skips_ahead() {
    for state in SkillState::ALL {
        let next = apply(&progress(state, 2), SessionOutcome::GettingThere, false);
        assert_eq!(next.easy_streak, 0);
        let expected = if state == SkillState::NotIntroduced {
            SkillState::Introduced
        } else {
            SkillState::Practising
        };
        assert_eq!(next.state, expected, "from {state:?}");
    }
}

#[test]
fn recent_outcomes_keep_the_newest_window() {
    let config = ProgressionConfig::default();
    let mut p = ProgressSnapshot::fresh();
    for i in 0..(config.recent_window_size + 2) as i64 {
        p = apply_outcome_to_progress(&p, SessionOutcome::Easy, false, &config, NOW + i);
    }
    assert_eq!(p.recent_outcomes.len(), config.recent_window_size);
    assert_eq!(
        p.recent_outcomes[0].at,
        NOW + config.recent_window_size as i64 + 1
    );
}
