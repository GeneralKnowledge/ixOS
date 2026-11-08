//! The app's saved data and the rules that change it. Everything here is
//! plain Rust over [`Stored`], with the time passed in, so it is tested
//! without the FFI layer or a clock. `lib.rs` wraps it for the hosts.

use std::collections::{BTreeMap, HashMap};

use good_dog_core::coaching::{
    apply_outcome_to_progress, describe_skill_state, generate_daily_plan,
    pick_plan_item_replacement, PlanReason, PlanRequest, ProgressSnapshot, ProgressionConfig,
    RecentSession, SkillState, DAY_MS, MAX_REPLACES_PER_ITEM,
};
use good_dog_core::dog_training::{
    bundled_exercises, bundled_learning_objectives, DogSubject, DogTrainingPolicy, ExerciseContent,
};
use serde::{Deserialize, Serialize};

use crate::model::*;
use crate::AppError;

/// Sessions further back than this don't influence today's plan.
const RECENT_DAYS: i64 = 14;
/// Old plans are dropped after this many days; sessions are kept.
const KEEP_PLANS_DAYS: i64 = 60;
/// The longest dog name accepted.
pub(crate) const MAX_NAME_CHARS: usize = 40;

/// Everything saved, as one JSON document (`gooddog.json`).
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub(crate) struct Stored {
    pub next_id: u64,
    pub dog: Option<DogProfile>,
    pub plans: Vec<StoredPlan>,
    pub sessions: Vec<StoredSession>,
    /// Skill progress by learning objective id.
    pub progress: BTreeMap<String, ProgressSnapshot>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct StoredPlan {
    /// Local day number (days since 1970-01-01 in the phone's time zone).
    pub day: i64,
    pub short_plan: bool,
    pub starter: bool,
    pub activities: Vec<StoredActivity>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct StoredActivity {
    pub id: u64,
    pub exercise_id: String,
    /// The exercise text version shown, so history keeps what was practised.
    pub content_version: u32,
    pub role: PlanRole,
    pub why_today: String,
    pub swaps: u32,
    pub session_id: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct StoredSession {
    pub id: u64,
    pub activity_id: Option<u64>,
    pub exercise_id: String,
    pub learning_objective_id: String,
    pub content_version: u32,
    pub completed_at_ms: i64,
    pub result: SessionResult,
    pub welfare_concern: bool,
}

pub(crate) fn local_day(now: LocalTime) -> i64 {
    (now.ms + i64::from(now.utc_offset_minutes) * 60_000).div_euclid(DAY_MS)
}

pub(crate) fn exercise(id: &str) -> Option<&'static ExerciseContent> {
    bundled_exercises().iter().find(|e| e.core.id == id)
}

/// A skill's name. Three of the web app's exercises use objectives it never
/// named; their exercise's title stands in.
fn objective_names(objective_id: &str) -> (String, String) {
    if let Some(o) = bundled_learning_objectives()
        .iter()
        .find(|o| o.id == objective_id)
    {
        return (o.label.clone(), o.plain_english.clone());
    }
    let title = bundled_exercises()
        .iter()
        .find(|e| e.core.learning_objective_id == objective_id)
        .map_or_else(|| objective_id.replace('-', " "), |e| e.title.clone());
    (title, String::new())
}

/// The validation rule for a dog's profile. Returns it tidied.
pub(crate) fn validated_profile(mut profile: DogProfile) -> Result<DogProfile, AppError> {
    let name: String = profile
        .name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if name.is_empty() {
        return Err(AppError::InvalidInput {
            reason: "the dog needs a name".into(),
        });
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(AppError::InvalidInput {
            reason: format!("a name can be at most {MAX_NAME_CHARS} characters"),
        });
    }
    if name.chars().any(char::is_control) {
        return Err(AppError::InvalidInput {
            reason: "the name has characters that can't be shown".into(),
        });
    }
    profile.name = name;
    Ok(profile)
}

impl Stored {
    fn take_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn subject(&self) -> Result<DogSubject, AppError> {
        let dog = self.dog.as_ref().ok_or(AppError::NoDog)?;
        Ok(DogSubject {
            id: "dog".into(),
            name: dog.name.clone(),
            life_stage: dog.life_stage.into(),
            available_time: dog.daily_time.into(),
            primary_reason: dog.goal.as_reason().into(),
            training_experience: dog.experience.into(),
        })
    }

    fn recent_sessions(&self, now: LocalTime) -> Vec<RecentSession> {
        self.sessions
            .iter()
            .filter(|s| now.ms - s.completed_at_ms < RECENT_DAYS * DAY_MS)
            .map(|s| RecentSession {
                exercise_id: s.exercise_id.clone(),
                learning_objective_id: s.learning_objective_id.clone(),
                completed_at: s.completed_at_ms,
                outcome: s.result.into(),
                welfare_concern: s.welfare_concern,
            })
            .collect()
    }

    fn progress_map(&self) -> HashMap<String, ProgressSnapshot> {
        self.progress
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    fn today_index(&self, now: LocalTime) -> Option<usize> {
        let day = local_day(now);
        self.plans.iter().position(|p| p.day == day)
    }

    /// Makes sure today has a plan, building one if needed. Returns whether
    /// anything changed (and so needs saving). A plan stays the same all day.
    pub fn ensure_today(&mut self, now: LocalTime) -> Result<bool, AppError> {
        if self.today_index(now).is_some() {
            return Ok(false);
        }
        let plan = self.build_plan(now, false)?;
        let oldest = local_day(now) - KEEP_PLANS_DAYS;
        self.plans.retain(|p| p.day >= oldest);
        self.plans.push(plan);
        Ok(true)
    }

    fn build_plan(&mut self, now: LocalTime, short_plan: bool) -> Result<StoredPlan, AppError> {
        let subject = self.subject()?;
        let progress = self.progress_map();
        let recent = self.recent_sessions(now);
        let plan = generate_daily_plan(&PlanRequest {
            policy: &DogTrainingPolicy,
            subject: &subject,
            exercises: bundled_exercises(),
            progress_by_objective: &progress,
            recent_sessions: &recent,
            short_plan,
            config: ProgressionConfig::default(),
            now: now.ms,
        });
        let activities = plan
            .items
            .into_iter()
            .map(|item| StoredActivity {
                id: self.take_id(),
                content_version: exercise(&item.exercise_id).map_or(1, |e| e.content_version),
                exercise_id: item.exercise_id,
                role: item.role.into(),
                why_today: item.why_today,
                swaps: 0,
                session_id: None,
            })
            .collect();
        Ok(StoredPlan {
            day: local_day(now),
            short_plan,
            starter: plan.reason == PlanReason::Starter,
            activities,
        })
    }

    pub fn today(&self, now: LocalTime) -> Option<TodayPlan> {
        let plan = &self.plans[self.today_index(now)?];
        let activities: Vec<PlanActivity> = plan
            .activities
            .iter()
            .filter_map(|a| {
                let e = exercise(&a.exercise_id)?;
                let result = a
                    .session_id
                    .and_then(|id| self.sessions.iter().find(|s| s.id == id))
                    .map(|s| s.result);
                Some(PlanActivity {
                    id: a.id,
                    exercise_id: a.exercise_id.clone(),
                    title: e.title.clone(),
                    summary: e.summary.clone(),
                    minutes: e.core.estimated_minutes.max(0) as u32,
                    role: a.role,
                    why_today: a.why_today.clone(),
                    result,
                    can_swap: result.is_none() && a.swaps < MAX_REPLACES_PER_ITEM,
                })
            })
            .collect();
        let done_count = activities.iter().filter(|a| a.result.is_some()).count() as u32;
        Some(TodayPlan {
            total_minutes: activities.iter().map(|a| a.minutes).sum(),
            done_count,
            short_plan: plan.short_plan,
            can_shorten: !plan.short_plan && done_count == 0,
            starter: plan.starter,
            activities,
        })
    }

    /// Replaces today's plan with a one-activity short plan, if nothing has
    /// been practised yet.
    pub fn shorten_today(&mut self, now: LocalTime) -> Result<(), AppError> {
        self.ensure_today(now)?;
        let index = self.today_index(now).expect("ensured");
        let plan = &self.plans[index];
        if plan.short_plan {
            return Ok(());
        }
        if plan.activities.iter().any(|a| a.session_id.is_some()) {
            return Err(AppError::InvalidInput {
                reason: "today's practice has already started".into(),
            });
        }
        let short = self.build_plan(now, true)?;
        self.plans[index] = short;
        Ok(())
    }

    /// Swaps one of today's activities for something different.
    pub fn swap(&mut self, activity_id: u64, now: LocalTime) -> Result<(), AppError> {
        let subject = self.subject()?;
        let index = self.today_index(now).ok_or(AppError::NotFound {
            what: "today's plan".into(),
        })?;
        let plan = &self.plans[index];
        let activity =
            plan.activities
                .iter()
                .find(|a| a.id == activity_id)
                .ok_or(AppError::NotFound {
                    what: "that activity".into(),
                })?;
        if activity.session_id.is_some() {
            return Err(AppError::InvalidInput {
                reason: "that activity has already been practised".into(),
            });
        }
        if activity.swaps >= MAX_REPLACES_PER_ITEM {
            return Err(AppError::NoAlternative);
        }
        let exclude: Vec<String> = plan
            .activities
            .iter()
            .map(|a| a.exercise_id.clone())
            .collect();
        let progress = self.progress_map();
        let recent = self.recent_sessions(now);
        let pick = pick_plan_item_replacement(
            &PlanRequest {
                policy: &DogTrainingPolicy,
                subject: &subject,
                exercises: bundled_exercises(),
                progress_by_objective: &progress,
                recent_sessions: &recent,
                short_plan: plan.short_plan,
                config: ProgressionConfig::default(),
                now: now.ms,
            },
            &exclude,
            &activity.exercise_id,
            activity.role.into(),
        )
        .ok_or(AppError::NoAlternative)?;

        let activity = self.plans[index]
            .activities
            .iter_mut()
            .find(|a| a.id == activity_id)
            .expect("found above");
        activity.content_version = exercise(&pick.exercise_id).map_or(1, |e| e.content_version);
        activity.exercise_id = pick.exercise_id;
        activity.why_today = pick.why_today;
        activity.swaps += 1;
        Ok(())
    }

    /// Records a practice session and moves the skill on. `activity_id` ties
    /// it to one of today's activities; without one it is extra practice.
    pub fn record(
        &mut self,
        exercise_id: &str,
        activity_id: Option<u64>,
        result: SessionResult,
        welfare_concern: bool,
        now: LocalTime,
    ) -> Result<String, AppError> {
        self.subject()?;
        let e = exercise(exercise_id).ok_or(AppError::NotFound {
            what: "that exercise".into(),
        })?;
        let session_id = self.take_id();
        if let Some(activity_id) = activity_id {
            let activity = self
                .today_index(now)
                .and_then(|i| {
                    self.plans[i]
                        .activities
                        .iter_mut()
                        .find(|a| a.id == activity_id)
                })
                .ok_or(AppError::NotFound {
                    what: "that activity".into(),
                })?;
            if activity.exercise_id != exercise_id {
                return Err(AppError::InvalidInput {
                    reason: "the activity is a different exercise".into(),
                });
            }
            if activity.session_id.is_some() {
                return Err(AppError::InvalidInput {
                    reason: "that activity has already been recorded".into(),
                });
            }
            activity.session_id = Some(session_id);
        }
        let objective = e.core.learning_objective_id.clone();
        self.sessions.push(StoredSession {
            id: session_id,
            activity_id,
            exercise_id: exercise_id.to_string(),
            learning_objective_id: objective.clone(),
            content_version: e.content_version,
            completed_at_ms: now.ms,
            result,
            welfare_concern,
        });
        let current = self
            .progress
            .get(&objective)
            .cloned()
            .unwrap_or_else(ProgressSnapshot::fresh);
        let next = apply_outcome_to_progress(
            &current,
            result.into(),
            welfare_concern,
            &ProgressionConfig::default(),
            now.ms,
        );
        self.progress.insert(objective.clone(), next);
        Ok(objective)
    }

    /// Saves a new or changed profile. A plan nobody has started yet is
    /// dropped, so the next look at Today fits the new details.
    pub fn set_profile(&mut self, profile: DogProfile, now: LocalTime) {
        self.dog = Some(profile);
        if let Some(index) = self.today_index(now) {
            if self.plans[index]
                .activities
                .iter()
                .all(|a| a.session_id.is_none())
            {
                self.plans.remove(index);
            }
        }
    }

    pub fn skill(&self, objective_id: &str) -> SkillProgress {
        let (name, plain_english) = objective_names(objective_id);
        let state = self
            .progress
            .get(objective_id)
            .map_or(SkillState::NotIntroduced, |p| p.state);
        SkillProgress {
            objective_id: objective_id.to_string(),
            name,
            plain_english,
            level: state.into(),
            description: describe_skill_state(state).unwrap_or_default().to_string(),
            sessions: self
                .sessions
                .iter()
                .filter(|s| s.learning_objective_id == objective_id)
                .count() as u32,
        }
    }

    pub fn progress_summary(&self, now: LocalTime) -> ProgressSummary {
        let today = local_day(now);
        let offset = i64::from(now.utc_offset_minutes) * 60_000;
        let week: Vec<&StoredSession> = self
            .sessions
            .iter()
            .filter(|s| today - (s.completed_at_ms + offset).div_euclid(DAY_MS) < 7)
            .collect();
        let mut days: Vec<i64> = week
            .iter()
            .map(|s| (s.completed_at_ms + offset).div_euclid(DAY_MS))
            .collect();
        days.sort_unstable();
        days.dedup();
        let mut skills: Vec<SkillProgress> = self
            .progress
            .iter()
            .filter(|(_, p)| p.state != SkillState::NotIntroduced)
            .map(|(id, _)| self.skill(id))
            .collect();
        skills.sort_by_key(|s| (level_rank(s.level), std::cmp::Reverse(s.sessions)));
        ProgressSummary {
            sessions_total: self.sessions.len() as u32,
            sessions_this_week: week.len() as u32,
            days_practised_this_week: days.len() as u32,
            skills,
        }
    }

    pub fn history(&self, limit: usize) -> Vec<SessionRecord> {
        self.sessions
            .iter()
            .rev()
            .take(limit)
            .map(|s| SessionRecord {
                id: s.id,
                exercise_id: s.exercise_id.clone(),
                exercise_title: exercise(&s.exercise_id)
                    .map_or_else(|| s.exercise_id.clone(), |e| e.title.clone()),
                completed_at_ms: s.completed_at_ms,
                result: s.result,
                welfare_concern: s.welfare_concern,
            })
            .collect()
    }
}

/// Furthest along first; a skill being made easier comes after the active ones.
fn level_rank(level: SkillLevel) -> u8 {
    match level {
        SkillLevel::ReadyForMore => 0,
        SkillLevel::BecomingConsistent => 1,
        SkillLevel::Practising => 2,
        SkillLevel::Started => 3,
        SkillLevel::MakingEasier => 4,
        SkillLevel::NotStarted => 5,
    }
}

/// What to say after a session. Every promise here is something the planner
/// really does (see good-dog-core's tests).
pub(crate) fn feedback_message(
    dog_name: &str,
    result: SessionResult,
    welfare_concern: bool,
    level: SkillLevel,
) -> String {
    if welfare_concern {
        return format!(
            "Thanks for telling us. {dog_name} gets a rest from this skill for the next few days, and the plans stay gentle meanwhile. If you're worried about {dog_name}'s health or behaviour, talk to your vet."
        );
    }
    match (result, level) {
        (SessionResult::TooDifficult, _) => format!(
            "That's useful to know. Next time we'll make this easier for {dog_name}, or pick something gentler."
        ),
        (SessionResult::GettingThere, _) => {
            format!("Nice work. We'll keep practising this at the same level with {dog_name}.")
        }
        (SessionResult::Easy, SkillLevel::ReadyForMore) => format!(
            "{dog_name} has had enough good practice to try a small step forward next time."
        ),
        (SessionResult::Easy, SkillLevel::MakingEasier) => format!(
            "Good session. We'll keep things easy for {dog_name} a little longer while confidence rebuilds."
        ),
        (SessionResult::Easy, _) => format!("Great session with {dog_name}. Keep it short and fun."),
    }
}
