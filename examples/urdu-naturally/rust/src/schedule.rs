//! What you can do with each phrase, and when it comes back.
//!
//! Four abilities are tracked apart, each 0 to 5, because they are different
//! things: recognising a phrase on the page does not mean you can say it.
//!
//! | Skill | The question |
//! |---|---|
//! | Recognition | Do you know what it means when you see it? |
//! | Recall | Can you produce it from the English, without the answer? |
//! | Speaking | Can you say it in a conversation, at your turn? |
//! | Listening | Do you understand it when you hear it, without the text? |
//!
//! The schedule is deterministic application logic, never an AI's call.
//! Each attempt moves the practised skill and sets when the phrase is due
//! again, from the outcome (missed: 10 minutes; struggled: a day; with
//! help: 2 days; on your own: 4 days; confidently: a week, and 14 to 30
//! days once the skill is high). A miss costs one level, never everything.

use serde::{Deserialize, Serialize};

pub const TOP_LEVEL: u32 = 5;

const MINUTE: i64 = 60_000;
pub const DAY: i64 = 24 * 60 * MINUTE;

/// Review events kept for later tuning; the oldest go first.
pub const MAX_EVENTS: usize = 5_000;

#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Skill {
    Recognition,
    Recall,
    Speaking,
    Listening,
}

/// How an attempt went, as the learner judges it.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Didn't get it.
    Missed,
    /// Got there with a struggle.
    Struggled,
    /// Got it after a hint (a peek at the first word, the English).
    WithHelp,
    /// Got it alone.
    Independent,
    /// Got it straight away, without thinking.
    Confident,
}

/// The four abilities for one phrase.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Skills {
    pub recognition: u32,
    pub recall: u32,
    pub speaking: u32,
    pub listening: u32,
}

impl Skills {
    pub fn get(&self, skill: Skill) -> u32 {
        match skill {
            Skill::Recognition => self.recognition,
            Skill::Recall => self.recall,
            Skill::Speaking => self.speaking,
            Skill::Listening => self.listening,
        }
    }

    fn set(&mut self, skill: Skill, level: u32) {
        let level = level.min(TOP_LEVEL);
        match skill {
            Skill::Recognition => self.recognition = level,
            Skill::Recall => self.recall = level,
            Skill::Speaking => self.speaking = level,
            Skill::Listening => self.listening = level,
        }
    }

    /// How well you understand it, seen or heard: what decides how much
    /// English the screen shows.
    pub fn understanding(&self) -> u32 {
        self.recognition.max(self.listening)
    }
}

/// What is saved for one phrase.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct PhraseProgress {
    pub skills: Skills,
    /// Times shown on a card or in a conversation.
    pub times_seen: u32,
    pub times_correct: u32,
    pub times_missed: u32,
    pub last_seen_ms: Option<i64>,
    /// When it is due again; None until it has been practised.
    pub next_review_ms: Option<i64>,
}

/// One attempt, kept so the schedule can be improved later.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ReviewEvent {
    pub phrase: String,
    pub skill: Skill,
    pub outcome: Outcome,
    pub at_ms: i64,
    /// How long the learner took, when the screen measured it.
    pub latency_ms: Option<u32>,
}

impl PhraseProgress {
    /// The phrase was shown (learnt from a card, or met in a conversation).
    pub fn seen(&mut self, now_ms: i64) {
        self.times_seen += 1;
        self.last_seen_ms = Some(now_ms);
    }

    /// Records an attempt at `skill`.
    pub fn attempt(&mut self, skill: Skill, outcome: Outcome, now_ms: i64) {
        let level = self.skills.get(skill);
        let next = match outcome {
            Outcome::Missed => level.saturating_sub(1),
            Outcome::Struggled => level,
            Outcome::WithHelp => level.max(1),
            Outcome::Independent => level + 1,
            Outcome::Confident => level + 2,
        };
        self.skills.set(skill, next);
        if outcome == Outcome::Missed {
            self.times_missed += 1;
        } else if outcome != Outcome::Struggled {
            self.times_correct += 1;
        }
        self.last_seen_ms = Some(now_ms);
        self.next_review_ms = Some(now_ms + rest_ms(outcome, self.skills.get(skill)));
    }

    pub fn is_due(&self, now_ms: i64) -> bool {
        self.next_review_ms.is_some_and(|due| due <= now_ms)
    }
}

/// How long a phrase rests after `outcome`, with the skill now at `level`.
pub fn rest_ms(outcome: Outcome, level: u32) -> i64 {
    match outcome {
        Outcome::Missed => 10 * MINUTE,
        Outcome::Struggled => DAY,
        Outcome::WithHelp => 2 * DAY,
        Outcome::Independent => 4 * DAY,
        Outcome::Confident => match level {
            0..=3 => 7 * DAY,
            4 => 14 * DAY,
            _ => 30 * DAY,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skills_move_apart() {
        let mut p = PhraseProgress::default();
        p.attempt(Skill::Recognition, Outcome::Confident, 0);
        assert_eq!(p.skills.recognition, 2);
        assert_eq!(p.skills.speaking, 0, "recognising is not saying");
        p.attempt(Skill::Speaking, Outcome::Independent, 0);
        assert_eq!(p.skills.speaking, 1);
    }

    #[test]
    fn the_rest_follows_the_outcome() {
        let mut p = PhraseProgress::default();
        p.attempt(Skill::Recall, Outcome::Missed, 0);
        assert_eq!(p.next_review_ms, Some(10 * MINUTE));
        assert!(!p.is_due(9 * MINUTE));
        assert!(p.is_due(10 * MINUTE));
        p.attempt(Skill::Recall, Outcome::Struggled, 0);
        assert_eq!(p.next_review_ms, Some(DAY));
        p.attempt(Skill::Recall, Outcome::WithHelp, 0);
        assert_eq!(p.next_review_ms, Some(2 * DAY));
        p.attempt(Skill::Recall, Outcome::Independent, 0);
        assert_eq!(p.next_review_ms, Some(4 * DAY));
        p.attempt(Skill::Recall, Outcome::Confident, 0);
        assert_eq!(p.next_review_ms, Some(14 * DAY), "level 4 now");
        p.attempt(Skill::Recall, Outcome::Confident, 0);
        assert_eq!(p.skills.recall, TOP_LEVEL);
        assert_eq!(p.next_review_ms, Some(30 * DAY));
    }

    #[test]
    fn a_miss_costs_one_level_not_everything() {
        let mut p = PhraseProgress::default();
        for _ in 0..3 {
            p.attempt(Skill::Speaking, Outcome::Confident, 0);
        }
        assert_eq!(p.skills.speaking, TOP_LEVEL);
        p.attempt(Skill::Speaking, Outcome::Missed, 0);
        assert_eq!(p.skills.speaking, 4);
        assert_eq!(p.times_missed, 1);
        assert_eq!(p.times_correct, 3);
    }
}
