//! The owner's goals: starting one from a pathway, moving through its
//! stages on evidence, and pausing, resuming or setting it aside without
//! losing its history. Plain Rust over [`Stored`], like `training.rs`.
//!
//! Welfare comes first: a stage held by a welfare rest or by "making it
//! easier" never moves on, whatever the owner answers.

use good_dog_core::coaching::ProgressionConfig;
use good_dog_core::dog_training::{
    bundled_exercises, match_pathways, stage_readiness, Pathway, PathwayStage, StageBlocker,
    StageReadiness,
};
use serde::{Deserialize, Serialize};

use crate::model::*;
use crate::training::{exercise, Stored};
use crate::AppError;

/// At most this many goals are active at once, so Today stays calm.
pub(crate) const MAX_ACTIVE_GOALS: usize = 3;
/// The longest "in your own words" kept with a goal.
const MAX_OWN_WORDS_CHARS: usize = 200;

/// The pathways the app offers. Bundled ones in the app; tests swap in a
/// sample. Not saved: goals refer to pathways by id.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pathways(pub &'static [Pathway]);

impl Default for Pathways {
    fn default() -> Self {
        Self(good_dog_core::dog_training::bundled_pathways())
    }
}

impl PartialEq for Pathways {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct StoredGoal {
    pub id: u64,
    pub pathway_id: String,
    /// The pathway's `contentVersion` when the goal started.
    pub pathway_version: u32,
    pub status: GoalStatus,
    /// The stage being worked on (the last one once finished).
    pub stage_id: String,
    pub started_at_ms: i64,
    pub own_words: Option<String>,
    /// Oldest first.
    pub events: Vec<StoredGoalEvent>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct StoredGoalEvent {
    pub at_ms: i64,
    pub kind: GoalEventKind,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum GoalEventKind {
    Started,
    /// The owner said the gate was met. `ahead_of_evidence` when the app's
    /// own evidence wasn't there yet (the owner knows their dog).
    Passed {
        stage_id: String,
        ahead_of_evidence: bool,
    },
    NotYet {
        stage_id: String,
    },
    SteppedBack {
        from_stage_id: String,
        to_stage_id: String,
    },
    Paused,
    Resumed,
    SetAside,
    Finished,
}

impl Stored {
    fn pathway(&self, id: &str) -> Option<&'static Pathway> {
        self.pathways.0.iter().find(|p| p.id == id)
    }

    fn goal_index(&self, goal_id: u64) -> Result<usize, AppError> {
        self.goals
            .iter()
            .position(|g| g.id == goal_id)
            .ok_or(AppError::NotFound {
                what: "that goal".into(),
            })
    }

    fn active_count(&self) -> usize {
        self.goals
            .iter()
            .filter(|g| g.status == GoalStatus::Active)
            .count()
    }

    fn suits_dog(&self, pathway: &Pathway) -> bool {
        self.dog
            .as_ref()
            .is_none_or(|d| pathway.life_stages.contains(&d.life_stage.into()))
    }

    /// Pathways the owner can start: those that suit the dog's age.
    pub fn pathway_summaries(&self) -> Vec<PathwaySummary> {
        self.pathways
            .0
            .iter()
            .filter(|p| self.suits_dog(p))
            .map(pathway_summary)
            .collect()
    }

    /// Pathways matching what the owner typed, best first. Empty when
    /// nothing matches: the app then shows the full list, never a guess.
    pub fn suggest_pathways(&self, own_words: &str) -> Vec<PathwaySummary> {
        match_pathways(own_words, self.pathways.0)
            .into_iter()
            .filter(|p| self.suits_dog(p))
            .map(pathway_summary)
            .collect()
    }

    /// Active goals first, then paused, finished and set aside; newest first
    /// within each.
    pub fn goal_summaries(&self) -> Vec<GoalSummary> {
        let rank = |s: GoalStatus| match s {
            GoalStatus::Active => 0,
            GoalStatus::Paused => 1,
            GoalStatus::Finished => 2,
            GoalStatus::SetAside => 3,
        };
        let mut goals: Vec<&StoredGoal> = self.goals.iter().collect();
        goals.sort_by(|a, b| rank(a.status).cmp(&rank(b.status)).then(b.id.cmp(&a.id)));
        goals
            .into_iter()
            .filter_map(|g| self.goal_summary(g))
            .collect()
    }

    fn goal_summary(&self, goal: &StoredGoal) -> Option<GoalSummary> {
        let pathway = self.pathway(&goal.pathway_id)?;
        let index = pathway.stage_index(&goal.stage_id).unwrap_or(0);
        Some(GoalSummary {
            id: goal.id,
            pathway_id: goal.pathway_id.clone(),
            title: pathway.title.clone(),
            status: goal.status,
            stage_title: pathway.stages[index].title.clone(),
            stage_number: index as u32 + 1,
            stage_count: pathway.stages.len() as u32,
            own_words: goal.own_words.clone(),
        })
    }

    /// Starts a goal at the pathway's first stage. Starting one the owner
    /// already has resumes it instead, so history is never split.
    pub fn start_goal(
        &mut self,
        pathway_id: &str,
        own_words: Option<String>,
        now: LocalTime,
    ) -> Result<u64, AppError> {
        self.dog.as_ref().ok_or(AppError::NoDog)?;
        let pathway = self.pathway(pathway_id).ok_or(AppError::NotFound {
            what: "that goal".into(),
        })?;
        if !self.suits_dog(pathway) {
            return Err(AppError::InvalidInput {
                reason: "this goal doesn't suit your dog's age yet".into(),
            });
        }
        if let Some(existing) = self
            .goals
            .iter()
            .find(|g| g.pathway_id == pathway_id && g.status != GoalStatus::Finished)
        {
            let id = existing.id;
            if existing.status != GoalStatus::Active {
                self.resume_goal(id, now)?;
            }
            return Ok(id);
        }
        self.check_room_for_one_more()?;
        let own_words = tidy_own_words(own_words)?;
        let id = self.take_goal_id();
        self.goals.push(StoredGoal {
            id,
            pathway_id: pathway.id.clone(),
            pathway_version: pathway.content_version,
            status: GoalStatus::Active,
            stage_id: pathway.stages[0].id.clone(),
            started_at_ms: now.ms,
            own_words,
            events: vec![event(now, GoalEventKind::Started)],
        });
        self.drop_unstarted_plan(now);
        Ok(id)
    }

    fn take_goal_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn check_room_for_one_more(&self) -> Result<(), AppError> {
        if self.active_count() >= MAX_ACTIVE_GOALS {
            return Err(AppError::InvalidInput {
                reason: format!(
                    "you can work on {MAX_ACTIVE_GOALS} goals at once; pause one to start another"
                ),
            });
        }
        Ok(())
    }

    pub fn pause_goal(&mut self, goal_id: u64, now: LocalTime) -> Result<(), AppError> {
        self.set_status(goal_id, GoalStatus::Paused, GoalEventKind::Paused, now)
    }

    pub fn set_goal_aside(&mut self, goal_id: u64, now: LocalTime) -> Result<(), AppError> {
        self.set_status(goal_id, GoalStatus::SetAside, GoalEventKind::SetAside, now)
    }

    /// Picks a paused or set-aside goal up again at the stage it was on.
    pub fn resume_goal(&mut self, goal_id: u64, now: LocalTime) -> Result<(), AppError> {
        let i = self.goal_index(goal_id)?;
        match self.goals[i].status {
            GoalStatus::Active => return Ok(()),
            GoalStatus::Finished => {
                return Err(AppError::InvalidInput {
                    reason: "this goal is finished".into(),
                })
            }
            GoalStatus::Paused | GoalStatus::SetAside => {}
        }
        self.check_room_for_one_more()?;
        self.set_status(goal_id, GoalStatus::Active, GoalEventKind::Resumed, now)
    }

    fn set_status(
        &mut self,
        goal_id: u64,
        status: GoalStatus,
        kind: GoalEventKind,
        now: LocalTime,
    ) -> Result<(), AppError> {
        let i = self.goal_index(goal_id)?;
        let goal = &mut self.goals[i];
        if goal.status == status {
            return Ok(());
        }
        if goal.status == GoalStatus::Finished {
            return Err(AppError::InvalidInput {
                reason: "this goal is finished".into(),
            });
        }
        goal.status = status;
        goal.events.push(event(now, kind));
        self.drop_unstarted_plan(now);
        Ok(())
    }

    /// The app's own evidence for a goal's current stage.
    fn readiness(&self, pathway: &Pathway, stage: &PathwayStage, now: LocalTime) -> StageReadiness {
        let _ = pathway;
        stage_readiness(
            stage,
            bundled_exercises(),
            &self.progress_map_for_goals(),
            &self.all_sessions(),
            now.ms,
            &ProgressionConfig::default(),
        )
    }

    /// The owner's answer to the current stage's gate. "Yes" moves on (or
    /// finishes the goal after the last stage) unless welfare holds the
    /// stage; it is accepted ahead of the app's evidence, because the owner
    /// sees the dog every day, and the history says so. "Not yet" is
    /// recorded and changes nothing else.
    pub fn answer_gate(&mut self, goal_id: u64, met: bool, now: LocalTime) -> Result<(), AppError> {
        let i = self.goal_index(goal_id)?;
        let goal = &self.goals[i];
        if goal.status != GoalStatus::Active {
            return Err(AppError::InvalidInput {
                reason: "resume this goal first".into(),
            });
        }
        let pathway = self.pathway(&goal.pathway_id).ok_or(AppError::NotFound {
            what: "that goal's pathway".into(),
        })?;
        let index = pathway.stage_index(&goal.stage_id).unwrap_or(0);
        let stage = &pathway.stages[index];
        if !met {
            let kind = GoalEventKind::NotYet {
                stage_id: stage.id.clone(),
            };
            self.goals[i].events.push(event(now, kind));
            return Ok(());
        }
        let readiness = self.readiness(pathway, stage, now);
        if readiness.held() {
            return Err(AppError::InvalidInput {
                reason: held_reason(&readiness),
            });
        }
        let goal = &mut self.goals[i];
        goal.events.push(event(
            now,
            GoalEventKind::Passed {
                stage_id: stage.id.clone(),
                ahead_of_evidence: !readiness.ready_for_check(),
            },
        ));
        match pathway.stages.get(index + 1) {
            Some(next) => goal.stage_id = next.id.clone(),
            None => {
                goal.status = GoalStatus::Finished;
                goal.events.push(event(now, GoalEventKind::Finished));
            }
        }
        self.drop_unstarted_plan(now);
        Ok(())
    }

    /// Goes back one stage: harder than it looked, or after a break. Normal,
    /// never a failure.
    pub fn step_back(&mut self, goal_id: u64, now: LocalTime) -> Result<(), AppError> {
        let i = self.goal_index(goal_id)?;
        let goal = &self.goals[i];
        let pathway = self.pathway(&goal.pathway_id).ok_or(AppError::NotFound {
            what: "that goal's pathway".into(),
        })?;
        let index = pathway.stage_index(&goal.stage_id).unwrap_or(0);
        if index == 0 {
            return Err(AppError::InvalidInput {
                reason: "this is already the first stage".into(),
            });
        }
        let to = pathway.stages[index - 1].id.clone();
        let goal = &mut self.goals[i];
        let from = std::mem::replace(&mut goal.stage_id, to.clone());
        if goal.status == GoalStatus::Finished {
            goal.status = GoalStatus::Active;
        }
        goal.events.push(event(
            now,
            GoalEventKind::SteppedBack {
                from_stage_id: from,
                to_stage_id: to,
            },
        ));
        self.drop_unstarted_plan(now);
        Ok(())
    }

    /// A goal in full: every stage explained, the gate, and its history.
    pub fn goal_detail(&self, goal_id: u64, now: LocalTime) -> Result<GoalDetail, AppError> {
        let goal = &self.goals[self.goal_index(goal_id)?];
        let pathway = self.pathway(&goal.pathway_id).ok_or(AppError::NotFound {
            what: "that goal's pathway".into(),
        })?;
        let summary = self.goal_summary(goal).expect("pathway exists");
        let name = self.dog.as_ref().map_or("your dog", |d| d.name.as_str());
        let current = pathway.stage_index(&goal.stage_id).unwrap_or(0);
        let finished = goal.status == GoalStatus::Finished;
        let stages = pathway
            .stages
            .iter()
            .enumerate()
            .map(|(i, s)| GoalStage {
                id: s.id.clone(),
                title: s.title.clone(),
                state: if i < current || finished {
                    StageState::Passed
                } else if i == current {
                    StageState::Current
                } else {
                    StageState::Upcoming
                },
                what_it_means: s.what_it_means.clone(),
                why_it_matters: s.why_it_matters.clone(),
                exercises: s
                    .exercise_ids
                    .iter()
                    .filter_map(|id| exercise(id))
                    .map(crate::exercise_summary)
                    .collect(),
                everyday_practice: s.everyday_practice.clone(),
                ready_when: s.gate.summary.clone(),
                if_it_is_hard: s.if_gate_fails.clone(),
                maintenance: s.maintenance.clone(),
            })
            .collect();
        let gate = (!finished).then(|| {
            let stage = &pathway.stages[current];
            let readiness = self.readiness(pathway, stage, now);
            GateStatus {
                ready_for_check: readiness.ready_for_check(),
                held: readiness.held(),
                question: stage.gate.owner_check.replace("{dog}", name),
                note: gate_note(&readiness, name, goal.status),
            }
        });
        let history = goal
            .events
            .iter()
            .rev()
            .map(|e| GoalEvent {
                at_ms: e.at_ms,
                text: event_text(&e.kind, pathway),
            })
            .collect();
        Ok(GoalDetail {
            summary,
            pathway_summary: pathway.summary.clone(),
            stages,
            gate,
            history,
        })
    }

    /// The current-stage exercises of every active goal: the planner leans
    /// towards these.
    pub fn focus_exercise_ids(&self) -> Vec<String> {
        self.goals
            .iter()
            .filter(|g| g.status == GoalStatus::Active)
            .filter_map(|g| self.pathway(&g.pathway_id)?.stage(&g.stage_id))
            .flat_map(|s| s.exercise_ids.iter().cloned())
            .collect()
    }

    /// The active goal today's plan serves most: the one with most of its
    /// current-stage exercises in the plan, then the newest.
    pub fn goal_focus(&self, planned_exercise_ids: &[&str]) -> Option<GoalFocus> {
        self.goals
            .iter()
            .filter(|g| g.status == GoalStatus::Active)
            .filter_map(|g| {
                let pathway = self.pathway(&g.pathway_id)?;
                let stage = pathway.stage(&g.stage_id)?;
                let hits = stage
                    .exercise_ids
                    .iter()
                    .filter(|id| planned_exercise_ids.contains(&id.as_str()))
                    .count();
                Some((hits, g.id, pathway, stage))
            })
            .max_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, goal_id, pathway, stage)| GoalFocus {
                goal_id,
                goal_title: pathway.title.clone(),
                stage_title: stage.title.clone(),
                why: stage.why_it_matters.clone(),
            })
    }
}

fn event(now: LocalTime, kind: GoalEventKind) -> StoredGoalEvent {
    StoredGoalEvent {
        at_ms: now.ms,
        kind,
    }
}

fn pathway_summary(p: &Pathway) -> PathwaySummary {
    PathwaySummary {
        id: p.id.clone(),
        title: p.title.clone(),
        summary: p.summary.clone(),
        stage_count: p.stages.len() as u32,
    }
}

fn tidy_own_words(words: Option<String>) -> Result<Option<String>, AppError> {
    let Some(words) = words else { return Ok(None) };
    let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.is_empty() {
        return Ok(None);
    }
    if words.chars().count() > MAX_OWN_WORDS_CHARS {
        return Err(AppError::InvalidInput {
            reason: format!("keep it to {MAX_OWN_WORDS_CHARS} characters"),
        });
    }
    if words.chars().any(char::is_control) {
        return Err(AppError::InvalidInput {
            reason: "that has characters that can't be shown".into(),
        });
    }
    Ok(Some(words))
}

fn exercise_title(id: &str) -> String {
    exercise(id).map_or_else(|| id.to_string(), |e| e.title.clone())
}

fn held_reason(readiness: &StageReadiness) -> String {
    for blocker in &readiness.blockers {
        match blocker {
            StageBlocker::WelfareRest { exercise_id, .. } => {
                return format!(
                    "\u{201c}{}\u{201d} is resting after a welfare concern. Moving on waits until the rest is over.",
                    exercise_title(exercise_id)
                )
            }
            StageBlocker::MakingItEasier { exercise_id } => {
                return format!(
                    "\u{201c}{}\u{201d} is being made easier after a hard session. Moving on waits until it goes well again.",
                    exercise_title(exercise_id)
                )
            }
            _ => {}
        }
    }
    String::new()
}

fn gate_note(readiness: &StageReadiness, dog: &str, status: GoalStatus) -> String {
    if status != GoalStatus::Active {
        return "Resume this goal to carry on where you left off.".into();
    }
    if readiness.held() {
        return held_reason(readiness);
    }
    match readiness.blockers.first() {
        None => format!("{dog} has had good practice on different days. Is this true yet?"),
        Some(StageBlocker::NotStarted { exercise_id }) => format!(
            "Start with \u{201c}{}\u{201d}. Today's plan will include it.",
            exercise_title(exercise_id)
        ),
        Some(StageBlocker::NeedsMorePractice { exercise_id }) => format!(
            "Keep practising \u{201c}{}\u{201d} on a few different days. If it's already true, you can say so.",
            exercise_title(exercise_id)
        ),
        Some(_) => String::new(),
    }
}

fn stage_title(pathway: &Pathway, id: &str) -> String {
    pathway
        .stage(id)
        .map_or_else(|| id.to_string(), |s| s.title.clone())
}

fn event_text(kind: &GoalEventKind, pathway: &Pathway) -> String {
    match kind {
        GoalEventKind::Started => "Started this goal".into(),
        GoalEventKind::Passed { stage_id, .. } => {
            format!(
                "Moved on from \u{201c}{}\u{201d}",
                stage_title(pathway, stage_id)
            )
        }
        GoalEventKind::NotYet { stage_id } => format!(
            "Not quite there with \u{201c}{}\u{201d} yet",
            stage_title(pathway, stage_id)
        ),
        GoalEventKind::SteppedBack { to_stage_id, .. } => format!(
            "Went back to \u{201c}{}\u{201d}",
            stage_title(pathway, to_stage_id)
        ),
        GoalEventKind::Paused => "Paused".into(),
        GoalEventKind::Resumed => "Picked up again".into(),
        GoalEventKind::SetAside => "Set aside".into(),
        GoalEventKind::Finished => "Every stage done: now it's occasional practice".into(),
    }
}
