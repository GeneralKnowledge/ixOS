//! Goal pathways: an owner's goal ("coming back when called") as ordered
//! stages, each with what it means, why it matters, the exercises it uses, an
//! everyday idea, and a gate the dog has to pass before the next stage.
//!
//! Pathways are written in GeneralKnowledge/DogResearch, in the shape of its
//! Foundation course (`kb/meta/uk-lite-course.json`), and exported into
//! `content/pathways.json` like the exercises. See docs/good-dog-demo.md,
//! "Pathway format".
//!
//! A gate never passes on a count of completed sessions. [`stage_readiness`]
//! says whether the app's own evidence is there yet; the owner then answers
//! the gate's observable check.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::content::ContentError;
use super::types::{ExerciseContent, LifeStage};
use crate::coaching::{
    objective_in_welfare_cooldown, welfare_cooldown_ms, ProgressSnapshot, ProgressionConfig,
    RecentSession, SessionOutcome, SkillState, DAY_MS,
};

/// One goal's route, from where most dogs start to using the skill in
/// everyday life.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pathway {
    pub id: String,
    pub title: String,
    /// One sentence for the goal list.
    pub summary: String,
    /// DogResearch owner-question categories this goal answers (`oq-recall`).
    #[serde(default)]
    pub owner_question_category_ids: Vec<String>,
    /// Everyday phrases an owner might use for this goal ("pulls on the
    /// lead"), matched against what they type in their own words.
    #[serde(default)]
    pub match_phrases: Vec<String>,
    /// Life stages the pathway suits.
    pub life_stages: Vec<LifeStage>,
    pub content_version: u32,
    pub stages: Vec<PathwayStage>,
}

/// One stage of a pathway.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathwayStage {
    pub id: String,
    pub title: String,
    pub what_it_means: String,
    pub why_it_matters: String,
    /// The exercises practised in this stage, easiest first.
    pub exercise_ids: Vec<String>,
    #[serde(default)]
    pub lesson_ids: Vec<String>,
    /// One way to practise in ordinary life, outside a session.
    pub everyday_practice: String,
    /// Earlier stages of the same pathway this one follows. Stages are in
    /// order already; this is kept for DogResearch's course format.
    #[serde(default)]
    pub opens_after: Vec<String>,
    pub gate: StageGate,
    /// What to do when the gate isn't met: the easier route back.
    pub if_gate_fails: String,
    /// DogResearch safety rules that apply (`safe-002-recall-management`).
    #[serde(default)]
    pub safety_rule_ids: Vec<String>,
    /// How to keep the skill once the stage is passed.
    pub maintenance: String,
}

/// What shows a stage is learned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageGate {
    pub summary: String,
    /// DogResearch assessment metrics behind the gate.
    #[serde(default)]
    pub metric_ids: Vec<String>,
    /// The gate as a plain question to the owner, with `{dog}` for the
    /// dog's name: "Does {dog} come back to arm's reach on a 5 m line?"
    pub owner_check: String,
}

impl Pathway {
    pub fn stage(&self, id: &str) -> Option<&PathwayStage> {
        self.stages.iter().find(|s| s.id == id)
    }

    pub fn stage_index(&self, id: &str) -> Option<usize> {
        self.stages.iter().position(|s| s.id == id)
    }
}

pub fn parse_pathways(json: &str) -> Result<Vec<Pathway>, ContentError> {
    Ok(serde_json::from_str(json)?)
}

const PATHWAYS_JSON: &str = include_str!("../../content/pathways.json");

/// The pathways shipped with the app. Empty until DogResearch publishes
/// them; see `content/SOURCE.md`.
///
/// # Panics
///
/// Never in practice: the bundled JSON is fixed at compile time and
/// `tests/pathways.rs` parses and checks it.
pub fn bundled_pathways() -> &'static [Pathway] {
    static PATHWAYS: OnceLock<Vec<Pathway>> = OnceLock::new();
    PATHWAYS.get_or_init(|| parse_pathways(PATHWAYS_JSON).expect("bundled pathways.json"))
}

/// Everything wrong with a set of pathways, checked against the exercises
/// they use. Empty when they are fit to ship.
pub fn check_pathways(pathways: &[Pathway], exercises: &[ExerciseContent]) -> Vec<String> {
    let by_id: HashMap<&str, &ExerciseContent> =
        exercises.iter().map(|e| (e.core.id.as_str(), e)).collect();
    let mut problems = Vec::new();
    let mut pathway_ids = HashSet::new();
    for pathway in pathways {
        let p = &pathway.id;
        if !pathway_ids.insert(p.as_str()) {
            problems.push(format!("{p}: pathway id used twice"));
        }
        if pathway.stages.is_empty() {
            problems.push(format!("{p}: no stages"));
        }
        if pathway.life_stages.is_empty() {
            problems.push(format!("{p}: no life stages"));
        }
        let mut earlier: HashSet<&str> = HashSet::new();
        for stage in &pathway.stages {
            let s = &stage.id;
            if earlier.contains(s.as_str()) {
                problems.push(format!("{p}/{s}: stage id used twice"));
            }
            if stage.exercise_ids.is_empty() {
                problems.push(format!("{p}/{s}: no exercises"));
            }
            for id in &stage.exercise_ids {
                match by_id.get(id.as_str()) {
                    None => problems.push(format!("{p}/{s}: unknown exercise {id}")),
                    Some(e) => {
                        if !pathway
                            .life_stages
                            .iter()
                            .any(|l| e.life_stages.contains(l))
                        {
                            problems.push(format!(
                                "{p}/{s}: {id} suits none of the pathway's life stages"
                            ));
                        }
                    }
                }
            }
            for after in &stage.opens_after {
                if !earlier.contains(after.as_str()) {
                    problems.push(format!(
                        "{p}/{s}: opens after {after}, which is not an earlier stage"
                    ));
                }
            }
            for (field, text) in [
                ("whatItMeans", &stage.what_it_means),
                ("whyItMatters", &stage.why_it_matters),
                ("everydayPractice", &stage.everyday_practice),
                ("gate.ownerCheck", &stage.gate.owner_check),
                ("ifGateFails", &stage.if_gate_fails),
                ("maintenance", &stage.maintenance),
            ] {
                if text.trim().is_empty() {
                    problems.push(format!("{p}/{s}: {field} is empty"));
                }
            }
            earlier.insert(s);
        }
    }
    problems
}

/// Pathways whose phrases appear in what the owner typed, best match first.
/// Nothing is guessed: no match means an empty list, and the owner picks.
pub fn match_pathways<'a>(text: &str, pathways: &'a [Pathway]) -> Vec<&'a Pathway> {
    let text = text.to_lowercase();
    let mut scored: Vec<(usize, usize, &Pathway)> = pathways
        .iter()
        .enumerate()
        .filter_map(|(order, p)| {
            let hits = p
                .match_phrases
                .iter()
                .chain(std::iter::once(&p.title))
                .filter(|phrase| !phrase.trim().is_empty())
                .filter(|phrase| text.contains(&phrase.to_lowercase()))
                .count();
            (hits > 0).then_some((hits, order, p))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, p)| p).collect()
}

/// Why a stage isn't ready for its gate yet. Each names the exercise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StageBlocker {
    /// Not practised at all yet.
    NotStarted { exercise_id: String },
    /// Practised, but not yet with good results on enough different days.
    NeedsMorePractice { exercise_id: String },
    /// The planner is making this skill easier after a hard session.
    MakingItEasier { exercise_id: String },
    /// A welfare concern was reported: the skill rests until `until_ms`.
    WelfareRest { exercise_id: String, until_ms: i64 },
}

/// The app's own evidence for one stage.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StageReadiness {
    pub blockers: Vec<StageBlocker>,
}

impl StageReadiness {
    /// The evidence is there: the owner can be asked the gate's check.
    pub fn ready_for_check(&self) -> bool {
        self.blockers.is_empty()
    }

    /// A welfare rest or a "make it easier" holds the stage: it must not
    /// move on, whatever the owner says.
    pub fn held(&self) -> bool {
        self.blockers.iter().any(|b| {
            matches!(
                b,
                StageBlocker::WelfareRest { .. } | StageBlocker::MakingItEasier { .. }
            )
        })
    }
}

/// Good results on at least this many different days, for one of a stage's
/// exercises, before its gate is asked.
pub const GATE_PRACTICE_DAYS: usize = 2;

/// Whether a stage's exercises have the evidence its gate needs. For each
/// exercise: no welfare rest, the skill not being made easier, the skill at
/// least becoming consistent, and "went well" or "getting there" on
/// [`GATE_PRACTICE_DAYS`] different days. `sessions` is the whole history;
/// days are UTC days, which is enough to tell days apart.
pub fn stage_readiness(
    stage: &PathwayStage,
    exercises: &[ExerciseContent],
    progress_by_objective: &HashMap<String, ProgressSnapshot>,
    sessions: &[RecentSession],
    now: i64,
    config: &ProgressionConfig,
) -> StageReadiness {
    let mut blockers = Vec::new();
    for exercise_id in &stage.exercise_ids {
        let Some(exercise) = exercises.iter().find(|e| &e.core.id == exercise_id) else {
            continue;
        };
        let objective = &exercise.core.learning_objective_id;
        let id = exercise_id.clone();
        if objective_in_welfare_cooldown(objective, sessions, now, config) {
            let last = sessions
                .iter()
                .filter(|s| &s.learning_objective_id == objective && s.welfare_concern)
                .map(|s| s.completed_at)
                .max()
                .unwrap_or(now);
            blockers.push(StageBlocker::WelfareRest {
                exercise_id: id,
                until_ms: last + welfare_cooldown_ms(config),
            });
            continue;
        }
        match progress_by_objective.get(objective).map(|p| p.state) {
            Some(SkillState::NeedsEasier) => {
                blockers.push(StageBlocker::MakingItEasier { exercise_id: id });
                continue;
            }
            None | Some(SkillState::NotIntroduced) => {
                blockers.push(StageBlocker::NotStarted { exercise_id: id });
                continue;
            }
            Some(SkillState::Introduced) | Some(SkillState::Practising) => {
                blockers.push(StageBlocker::NeedsMorePractice { exercise_id: id });
                continue;
            }
            Some(SkillState::BecomingConsistent) | Some(SkillState::ReadyToIncrease) => {}
        }
        let good_days: HashSet<i64> = sessions
            .iter()
            .filter(|s| &s.exercise_id == exercise_id)
            .filter(|s| !s.welfare_concern)
            .filter(|s| {
                matches!(
                    s.outcome,
                    SessionOutcome::Easy | SessionOutcome::GettingThere
                )
            })
            .map(|s| s.completed_at.div_euclid(DAY_MS))
            .collect();
        if good_days.len() < GATE_PRACTICE_DAYS {
            blockers.push(StageBlocker::NeedsMorePractice { exercise_id: id });
        }
    }
    StageReadiness { blockers }
}
