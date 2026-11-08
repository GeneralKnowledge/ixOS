use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::policy::{CoachingPolicy, PlanBudget, WhyTodayInput};
use super::progression::{
    should_prefer_easier_variation, should_prefer_harder_variation, ProgressSnapshot,
};
use super::types::{
    CoachingExercise, CoachingSubject, PlanItemRole, ProgressionConfig, SessionOutcome, SkillState,
    DAY_MS,
};
use super::welfare::{objective_in_welfare_cooldown, WELFARE_CALM_EXERCISE_IDS};

/// How many times one plan slot may be swapped in a day.
pub const MAX_REPLACES_PER_ITEM: u32 = 2;

/// A finished session, as the planner sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentSession {
    pub exercise_id: String,
    pub learning_objective_id: String,
    /// Unix milliseconds.
    pub completed_at: i64,
    pub outcome: SessionOutcome,
    pub welfare_concern: bool,
}

/// Everything one planning call needs.
pub struct PlanRequest<'a, P: CoachingPolicy> {
    pub policy: &'a P,
    pub subject: &'a P::Subject,
    pub exercises: &'a [P::Exercise],
    /// Progress keyed by learning objective id.
    pub progress_by_objective: &'a HashMap<String, ProgressSnapshot>,
    pub recent_sessions: &'a [RecentSession],
    /// One item within the short-plan minutes, for a busy day.
    pub short_plan: bool,
    pub config: ProgressionConfig,
    /// Unix milliseconds.
    pub now: i64,
}

/// One planned activity. Storage gives it an id and an exercise version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedItem {
    pub exercise_id: String,
    pub role: PlanItemRole,
    pub why_today: String,
}

/// Why the plan looks the way it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanReason {
    ShortOnTime,
    Adaptive,
    Starter,
}

impl PlanReason {
    pub fn as_str(self) -> &'static str {
        match self {
            PlanReason::ShortOnTime => "short_on_time",
            PlanReason::Adaptive => "adaptive",
            PlanReason::Starter => "starter",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedPlan {
    pub items: Vec<PlannedItem>,
    pub short_plan: bool,
    pub total_minutes: i32,
    pub reason: PlanReason,
}

/// The alternative chosen for a swapped plan slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplacementPick {
    pub exercise_id: String,
    pub why_today: String,
}

struct Scored<'a, E> {
    exercise: &'a E,
    preferred_easier: bool,
    preferred_harder: bool,
    progress: Option<&'a ProgressSnapshot>,
    score: i32,
}

fn prerequisites_met<E: CoachingExercise>(
    exercise: &E,
    progress_by_objective: &HashMap<String, ProgressSnapshot>,
    by_id: &HashMap<&str, &E>,
) -> bool {
    exercise.core().prerequisite_ids.iter().all(|pre_id| {
        let Some(pre) = by_id.get(pre_id.as_str()) else {
            return false;
        };
        let Some(progress) = progress_by_objective.get(&pre.core().learning_objective_id) else {
            return false;
        };
        matches!(
            progress.state,
            SkillState::Practising
                | SkillState::BecomingConsistent
                | SkillState::ReadyToIncrease
                | SkillState::Introduced
        )
    })
}

fn recently_practised(
    exercise_id: &str,
    recent_sessions: &[RecentSession],
    now: i64,
    avoid_within_days: i64,
) -> bool {
    let window = avoid_within_days * DAY_MS;
    recent_sessions
        .iter()
        .any(|s| s.exercise_id == exercise_id && now - s.completed_at < window)
}

/// Swaps in the easier or harder variation when the skill's state asks for it.
fn resolve_variation<'a, E: CoachingExercise>(
    exercise: &'a E,
    progress: Option<&ProgressSnapshot>,
    by_id: &HashMap<&str, &'a E>,
) -> (&'a E, bool, bool) {
    let Some(progress) = progress else {
        return (exercise, false, false);
    };
    let core = exercise.core();
    if should_prefer_easier_variation(progress) {
        if let Some(easier) = core
            .easier_variation_id
            .as_deref()
            .and_then(|id| by_id.get(id))
        {
            return (easier, true, false);
        }
    }
    if should_prefer_harder_variation(progress) {
        if let Some(harder) = core
            .harder_variation_id
            .as_deref()
            .and_then(|id| by_id.get(id))
        {
            return (harder, false, true);
        }
    }
    (exercise, false, false)
}

fn score_exercise<P: CoachingPolicy>(
    request: &PlanRequest<'_, P>,
    exercise: &P::Exercise,
    progress: Option<&ProgressSnapshot>,
) -> i32 {
    let core = exercise.core();
    let mut score = 10 - core.difficulty;
    score += request.policy.affinity_score(exercise, request.subject);

    match progress.map(|p| p.state) {
        Some(SkillState::Practising) => score += 3,
        Some(SkillState::BecomingConsistent) | Some(SkillState::ReadyToIncrease) => score += 2,
        None | Some(SkillState::NotIntroduced) if core.prerequisite_ids.is_empty() => score += 2,
        _ => {}
    }

    if recently_practised(
        &core.id,
        request.recent_sessions,
        request.now,
        request.config.avoid_repeat_within_days,
    ) {
        score -= 8;
    }

    // Mild variety preference: less-used learning objectives.
    let objective_count = request
        .recent_sessions
        .iter()
        .filter(|s| s.learning_objective_id == core.learning_objective_id)
        .count() as i32;
    score - objective_count
}

fn pick_role(index: usize, short_plan: bool) -> PlanItemRole {
    if short_plan {
        return PlanItemRole::Skill;
    }
    match index {
        0 => PlanItemRole::Engage,
        1 => PlanItemRole::Skill,
        _ => PlanItemRole::Everyday,
    }
}

/// Exercises that suit the subject, have their prerequisites under way, and
/// are not in a welfare cooldown.
fn eligible_exercises<'a, P: CoachingPolicy>(
    request: &PlanRequest<'a, P>,
    by_id: &HashMap<&str, &'a P::Exercise>,
    exclude: &HashSet<&str>,
) -> Vec<&'a P::Exercise> {
    request
        .exercises
        .iter()
        .filter(|exercise| {
            let core = exercise.core();
            !exclude.contains(core.id.as_str())
                && request.policy.is_eligible(exercise, request.subject)
                && prerequisites_met(*exercise, request.progress_by_objective, by_id)
                && !objective_in_welfare_cooldown(
                    &core.learning_objective_id,
                    request.recent_sessions,
                    request.now,
                    &request.config,
                )
        })
        .collect()
}

fn any_welfare_cooldown<P: CoachingPolicy>(request: &PlanRequest<'_, P>) -> bool {
    let window = request.config.welfare_cooldown_days * DAY_MS;
    request
        .recent_sessions
        .iter()
        .any(|s| s.welfare_concern && request.now - s.completed_at < window)
}

fn calm_only<'a, E: CoachingExercise>(eligible: &[&'a E]) -> Vec<&'a E> {
    eligible
        .iter()
        .copied()
        .filter(|e| WELFARE_CALM_EXERCISE_IDS.contains(&e.core().id.as_str()))
        .collect()
}

/// Scores each candidate (after swapping in variations) and sorts best first.
/// The sort is stable, so equal scores keep the exercise library's order.
fn score_candidates<'a, P: CoachingPolicy>(
    request: &PlanRequest<'a, P>,
    candidates: &[&'a P::Exercise],
    by_id: &HashMap<&str, &'a P::Exercise>,
    adjust: impl Fn(&P::Exercise, i32) -> i32,
) -> Vec<Scored<'a, P::Exercise>> {
    let mut scored: Vec<_> = candidates
        .iter()
        .map(|&exercise| {
            let base_progress = request
                .progress_by_objective
                .get(&exercise.core().learning_objective_id);
            let (resolved, preferred_easier, preferred_harder) =
                resolve_variation(exercise, base_progress, by_id);
            let progress = request
                .progress_by_objective
                .get(&resolved.core().learning_objective_id);
            let mut score = adjust(resolved, score_exercise(request, resolved, progress));
            // A struggling skill with no easier variation should not come back unchanged.
            if base_progress.map(|p| p.state) == Some(SkillState::NeedsEasier)
                && exercise.core().easier_variation_id.is_none()
                && resolved.core().id == exercise.core().id
            {
                score -= 25;
            }
            Scored {
                exercise: resolved,
                preferred_easier,
                preferred_harder,
                progress,
                score,
            }
        })
        .collect();
    scored.sort_by_key(|s| std::cmp::Reverse(s.score));
    scored
}

/// Builds today's plan.
pub fn generate_daily_plan<P: CoachingPolicy>(request: &PlanRequest<'_, P>) -> GeneratedPlan {
    let short_plan = request.short_plan;
    let budget = if short_plan {
        PlanBudget {
            max_minutes: request.config.short_plan_minutes,
            item_count: 1,
        }
    } else {
        request.policy.daily_budget(request.subject)
    };
    let by_id: HashMap<&str, &P::Exercise> = request
        .exercises
        .iter()
        .map(|e| (e.core().id.as_str(), e))
        .collect();

    let eligible = eligible_exercises(request, &by_id, &HashSet::new());
    let has_any_progress = request
        .progress_by_objective
        .values()
        .any(|p| p.state != SkillState::NotIntroduced);

    let mut candidates = eligible.clone();
    if any_welfare_cooldown(request) && !eligible.is_empty() {
        let calm = calm_only(&eligible);
        if !calm.is_empty() {
            candidates = calm;
        }
    }
    if !has_any_progress {
        let starters = request.policy.starter_exercises(&eligible, request.subject);
        if !starters.is_empty() {
            candidates = starters;
        }
    }

    let scored = score_candidates(request, &candidates, &by_id, |_, score| score);

    let mut selected: Vec<usize> = Vec::new();
    let mut used_objectives: HashSet<&str> = HashSet::new();
    let mut used_categories: HashSet<&str> = HashSet::new();
    let mut minutes = 0;

    for (index, candidate) in scored.iter().enumerate() {
        if selected.len() >= budget.item_count {
            break;
        }
        let core = candidate.exercise.core();
        if minutes + core.estimated_minutes > budget.max_minutes && !selected.is_empty() {
            continue;
        }
        if used_objectives.contains(core.learning_objective_id.as_str()) && !short_plan {
            continue;
        }
        // Encourage variety across categories when possible.
        if !selected.is_empty()
            && used_categories.contains(core.category.as_str())
            && scored.iter().enumerate().any(|(other, s)| {
                let c = s.exercise.core();
                !used_categories.contains(c.category.as_str())
                    && !used_objectives.contains(c.learning_objective_id.as_str())
                    && !selected.contains(&other)
            })
        {
            continue;
        }
        selected.push(index);
        used_objectives.insert(&core.learning_objective_id);
        used_categories.insert(&core.category);
        minutes += core.estimated_minutes;
    }

    // Guarantee at least one item.
    if selected.is_empty() && !scored.is_empty() {
        selected.push(0);
        minutes = scored[0].exercise.core().estimated_minutes;
    }

    let items = selected
        .iter()
        .enumerate()
        .map(|(position, &index)| {
            let item = &scored[index];
            let role = pick_role(position, short_plan);
            PlannedItem {
                exercise_id: item.exercise.core().id.clone(),
                role,
                why_today: request.policy.explain(&WhyTodayInput {
                    subject_name: request.subject.name(),
                    role,
                    skill_state: item.progress.map(|p| p.state),
                    preferred_easier: item.preferred_easier,
                    preferred_harder: item.preferred_harder,
                    is_starter: !has_any_progress,
                }),
            }
        })
        .collect();

    GeneratedPlan {
        items,
        short_plan,
        total_minutes: minutes,
        reason: if short_plan {
            PlanReason::ShortOnTime
        } else if has_any_progress {
            PlanReason::Adaptive
        } else {
            PlanReason::Starter
        },
    }
}

/// Picks one alternative for an open plan slot ("something different today").
/// It prefers the same category and an easier exercise, and returns `None`
/// when nothing else is suitable. `request.short_plan` is ignored.
pub fn pick_plan_item_replacement<P: CoachingPolicy>(
    request: &PlanRequest<'_, P>,
    exclude_exercise_ids: &[String],
    replace_exercise_id: &str,
    item_role: PlanItemRole,
) -> Option<ReplacementPick> {
    let by_id: HashMap<&str, &P::Exercise> = request
        .exercises
        .iter()
        .map(|e| (e.core().id.as_str(), e))
        .collect();
    let replaced = by_id.get(replace_exercise_id)?.core();
    let exclude: HashSet<&str> = exclude_exercise_ids.iter().map(String::as_str).collect();

    let eligible = eligible_exercises(request, &by_id, &exclude);
    if eligible.is_empty() {
        return None;
    }

    let mut candidates = eligible.clone();
    if any_welfare_cooldown(request) {
        let calm = calm_only(&eligible);
        if !calm.is_empty() {
            candidates = calm;
        }
    }

    let scored = score_candidates(request, &candidates, &by_id, |exercise, mut score| {
        let core = exercise.core();
        if core.category == replaced.category {
            score += 5;
        }
        if core.difficulty < replaced.difficulty {
            score += 4;
        }
        if core.difficulty > replaced.difficulty + 1 {
            score -= 6;
        }
        score
    });

    let best = scored.first()?;
    let explanation = request.policy.explain(&WhyTodayInput {
        subject_name: request.subject.name(),
        role: item_role,
        skill_state: best.progress.map(|p| p.state),
        preferred_easier: best.preferred_easier
            || best.exercise.core().difficulty < replaced.difficulty,
        preferred_harder: best.preferred_harder,
        is_starter: false,
    });
    Some(ReplacementPick {
        exercise_id: best.exercise.core().id.clone(),
        why_today: format!("You asked for something different today. {explanation}"),
    })
}
