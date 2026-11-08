use crate::coaching::{CoachingPolicy, PlanBudget, WhyTodayInput};

use super::types::{AvailableTime, DogSubject, ExerciseContent, TrainingExperience};
use super::why_today::explain_plan_item;

const STARTER_EXERCISE_IDS: [&str; 7] = [
    "ex-engagement-easy",
    "ex-name-response",
    "ex-reward-marker",
    "ex-mat-settle",
    "ex-rest-spot",
    "ex-handling-touch",
    "ex-lead-intro",
];

const PUPPY_STARTER_EXERCISE_IDS: [&str; 2] = ["ex-puppy-sounds", "ex-puppy-surfaces"];

/// Matches the free-text "what would you like help with" answer to exercise
/// categories: (keyword, category, boost). Free text is brittle; a closed set
/// of goals would be sturdier.
const GOAL_KEYWORDS: [(&str, &str, i32); 30] = [
    ("recall", "recall", 4),
    ("come when called", "recall", 4),
    ("come back", "recall", 4),
    ("coming back", "recall", 4),
    ("walk", "walking", 4),
    ("lead", "walking", 4),
    ("leash", "walking", 4),
    ("pull", "walking", 4),
    ("pulling", "walking", 4),
    ("settle", "calm", 4),
    ("calm", "calm", 3),
    ("relax", "calm", 3),
    ("restless", "calm", 3),
    ("manners", "manners", 3),
    ("jump", "manners", 4),
    ("jumping", "manners", 4),
    ("greeting", "manners", 3),
    ("door", "manners", 3),
    ("sit", "manners", 2),
    ("leave it", "manners", 3),
    ("attention", "engagement", 3),
    ("listen", "engagement", 3),
    ("focus", "engagement", 3),
    ("name", "engagement", 3),
    ("handling", "confidence", 3),
    ("groom", "confidence", 3),
    ("touch", "confidence", 2),
    ("puppy", "puppy", 4),
    ("noise", "puppy", 3),
    ("sounds", "puppy", 3),
];

/// The dog-training rules the engine asks about.
#[derive(Clone, Copy, Debug, Default)]
pub struct DogTrainingPolicy;

impl CoachingPolicy for DogTrainingPolicy {
    type Exercise = ExerciseContent;
    type Subject = DogSubject;

    fn is_eligible(&self, exercise: &ExerciseContent, dog: &DogSubject) -> bool {
        exercise.life_stages.contains(&dog.life_stage)
    }

    fn daily_budget(&self, dog: &DogSubject) -> PlanBudget {
        match dog.available_time {
            AvailableTime::FewMinutes => PlanBudget {
                max_minutes: 5,
                item_count: 2,
            },
            AvailableTime::About10 => PlanBudget {
                max_minutes: 10,
                item_count: 3,
            },
            AvailableTime::More => PlanBudget {
                max_minutes: 15,
                item_count: 3,
            },
        }
    }

    fn starter_exercises<'a>(
        &self,
        eligible: &[&'a ExerciseContent],
        dog: &DogSubject,
    ) -> Vec<&'a ExerciseContent> {
        let puppy = dog.life_stage.is_puppy();
        eligible
            .iter()
            .copied()
            .filter(|exercise| {
                let id = exercise.core.id.as_str();
                (STARTER_EXERCISE_IDS.contains(&id)
                    || (puppy && PUPPY_STARTER_EXERCISE_IDS.contains(&id)))
                    && exercise.core.difficulty <= 1
                    && exercise.life_stages.contains(&dog.life_stage)
            })
            .collect()
    }

    fn affinity_score(&self, exercise: &ExerciseContent, dog: &DogSubject) -> i32 {
        let mut score = 0;
        if dog.training_experience == TrainingExperience::New && exercise.core.difficulty == 1 {
            score += 3;
        }
        let reason = dog.primary_reason.to_lowercase();
        for (keyword, category, boost) in GOAL_KEYWORDS {
            if reason.contains(keyword) && exercise.core.category == category {
                score += boost;
            }
        }
        score
    }

    fn explain(&self, input: &WhyTodayInput<'_>) -> String {
        explain_plan_item(input)
    }
}
