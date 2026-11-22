//! Urdu, Naturally: Pakistani conversational Urdu, learnt the way you'd pick
//! it up from friends. Less studying, more talking.
//!
//! Three things stay separate (examples/urdu-naturally/docs/plan.md):
//!
//! - **The curriculum is the source of linguistic truth**: [`content`],
//!   compiled from `content/*.toml`, every phrase with its status (draft,
//!   reviewed, field-tested).
//! - **The learning engine is the source of progress truth**: [`schedule`],
//!   four skills per phrase and a deterministic review schedule.
//! - **Conversation variety** is free chat ([`chat`]): Ali, Ayesha, Omar
//!   and Khala answer whatever you say, in lines from the curriculum only.
//!   The phone's model may say which of your lines you meant; it never
//!   writes Urdu and never adds to the curriculum.
//!
//! This file is the API the SwiftUI app calls, on the application runtime
//! (`forge_app`): recording an attempt is a command that saves before it
//! returns; everything else is a query on published state that never waits.
//! The learner speaks male forms (the Urdu shown for "you" lines), and hears
//! men and women.

pub mod answer;
pub mod chat;
pub mod content;
pub mod schedule;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use forge_app::{DataDirectory, Document, ErrorKind, Published, Runtime};
use serde::{Deserialize, Serialize};

pub use answer::Verdict;
use content::{
    CharacterText, Content, PhraseText, ScenarioText, SpeakerForm, TopicText, Who, BLANK, NAME_SLOT,
};
pub use content::{ContentStatus, EntryKind, Formality, Gender, Level, VariantKind};
pub use schedule::{Outcome, Skill, Skills};
use schedule::{PhraseProgress, ReviewEvent, MAX_EVENTS};

uniffi::setup_scaffolding!();

// Forge AI: the phone's on-device model, which free chat asks which of your
// lines you meant (`native::ai::AiModel`; Swift passes `ForgeAppleModel`).
forge_services::native_services!(ai);
use native::ai::{AiModel, HostModel};

/// Version of `progress.json`. Bump it (and migrate) when [`Saved`] changes
/// shape.
const SAVE_VERSION: u32 = 1;
const SAVE_FILE: &str = "progress.json";
/// The longest name the learner can give.
pub const MAX_NAME_CHARS: u32 = 40;

// MARK: Settings

/// How much English the app shows.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EnglishSupport {
    /// Shown for new phrases, fading as you understand them.
    #[default]
    Gradual,
    /// Always shown.
    Always,
    /// Hidden as soon as you've met a phrase once.
    Minimal,
}

#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// Your name, for "Mera naam … hai". Empty until you give it.
    pub name: String,
    pub english: EnglishSupport,
}

// MARK: What the screens show

/// How the English for a phrase is shown. A tap always reveals it.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnglishDisplay {
    /// New material: shown in full.
    Visible,
    /// Familiar: shown small, under the Urdu.
    Secondary,
    /// Understood: hidden until tapped.
    Hidden,
}

/// Where a phrase is for you, from the four skills.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// Not met yet.
    New,
    /// Met, still learning.
    Learning,
    /// You know it when you see or hear it.
    Recognised,
    /// You can come up with it from the English.
    CanRecall,
    /// You can say it at your turn in a conversation.
    CanUse,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct VariantView {
    pub roman: String,
    pub kind: VariantKind,
    pub note: Option<String>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct PartView {
    pub roman: String,
    pub english: String,
}

/// A spoken combination of a word, or an example sentence of a pattern.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ExampleView {
    pub roman: String,
    pub english: String,
    /// Urdu script for the voice; always there for a pattern's examples.
    pub speech: Option<String>,
}

/// A line with a gap to fill with the word.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ClozeView {
    /// The Roman Urdu before the gap.
    pub before: String,
    /// The Roman Urdu after the gap.
    pub after: String,
    /// The whole line in English.
    pub english: String,
}

/// Another entry, named from this one (its opposite).
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct EntryLink {
    pub id: String,
    pub roman: String,
    pub english: String,
}

/// A phrase or a building-block word, with your progress and how to show it.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct PhraseView {
    pub id: String,
    pub kind: EntryKind,
    /// Roman Urdu, with your name filled in.
    pub roman: String,
    pub english: String,
    pub literal: Option<String>,
    pub usage: Option<String>,
    pub topic: String,
    pub difficulty: u32,
    pub formality: Formality,
    /// True when this is a man's form (yours); its female form is among
    /// the variants.
    pub male_form: bool,
    pub status: ContentStatus,
    pub variants: Vec<VariantView>,
    pub parts: Vec<PartView>,
    /// Common spoken combinations (for words and expressions), or the
    /// example sentences of a pattern.
    pub combos: Vec<ExampleView>,
    pub cloze: Option<ClozeView>,
    /// Its positive or negative twin.
    pub opposite: Option<EntryLink>,
    /// Urdu script for the voice to read (never shown).
    pub speech: String,
    pub english_display: EnglishDisplay,
    pub skills: Skills,
    pub stage: Stage,
    pub is_due: bool,
    pub times_seen: u32,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct CharacterView {
    pub id: String,
    pub name: String,
    pub gender: Gender,
    pub description: String,
    /// 1.0 is the voice's normal pace.
    pub speech_rate: f32,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnSpeaker {
    /// The scenario's character: listen.
    Character,
    /// You: say it out loud.
    You,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct TurnView {
    pub speaker: TurnSpeaker,
    pub phrase: PhraseView,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ScenarioSummary {
    pub id: String,
    pub title: String,
    pub description: String,
    pub objective: String,
    pub character: CharacterView,
    pub difficulty: u32,
    /// Lines you say in it.
    pub your_lines: u32,
    /// Of those, never practised.
    pub new_lines: u32,
    /// Of those, due again.
    pub due_lines: u32,
    /// Of those, ones you can use (speaking 3 or more).
    pub can_use: u32,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ScenarioView {
    pub summary: ScenarioSummary,
    pub turns: Vec<TurnView>,
    /// The building blocks (words, expressions, patterns) used in its lines,
    /// in the order they come up. This is how you learn words: from the
    /// conversations you have, not from lists.
    pub words: Vec<PhraseView>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct TopicView {
    pub id: String,
    pub level: Level,
    pub title: String,
    pub description: String,
    pub phrases: u32,
    /// Of those, met at least once.
    pub met: u32,
}

/// What to do next. A suggestion: you can always do something else.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum Suggestion {
    /// Phrases are due again.
    Review { count: u32 },
    /// A conversation with something new to say.
    Conversation { scenario_id: String },
    /// Nothing due and nothing new. Go and talk to someone.
    AllDone,
}

/// Your progress in what you can actually do.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct Totals {
    pub phrases: u32,
    pub met: u32,
    pub recognised: u32,
    pub can_recall: u32,
    pub can_use: u32,
    pub due: u32,
    /// The Building Blocks, by kind.
    pub words: u32,
    pub expressions: u32,
    pub patterns: u32,
    /// Attempts since `day_start_ms`.
    pub attempts_today: u32,
    /// The phrases you've met that you find hardest to say, hardest first.
    pub hardest_to_say: Vec<PhraseView>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Home {
    pub suggestion: Suggestion,
    pub totals: Totals,
    pub scenarios: Vec<ScenarioSummary>,
    pub topics: Vec<TopicView>,
}

/// A phrase to review, and the skill to practise with it.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ReviewItem {
    pub phrase: PhraseView,
    pub skill: Skill,
    /// For a pattern practised by speaking: the sentence to build from its
    /// English. Check a typed answer with `check_sentence`.
    #[uniffi(default = None)]
    pub example: Option<ExampleView>,
}

/// Hear a phrase, pick what it meant.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ListeningItem {
    pub phrase: PhraseView,
    /// English meanings, one of them right.
    pub choices: Vec<String>,
    pub answer: u32,
    /// Whose voice says it: rounds alternate, so you hear men and women.
    pub voice: Gender,
}

// MARK: Free chat

/// How what you said in a chat was understood.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatHeard {
    /// You said the line.
    Said,
    /// Nearly: a letter or two out.
    Nearly,
    /// You meant it (in English or other words): this is how to say it.
    Meant,
    /// You picked it from the help.
    Picked,
    /// They didn't catch it.
    Missed,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ChatLine {
    pub speaker: TurnSpeaker,
    /// Their line; or yours, as said or as you could have said it. None
    /// when they didn't catch what you said.
    pub phrase: Option<PhraseView>,
    /// What you typed.
    pub typed: Option<String>,
    /// For your lines.
    pub heard: Option<ChatHeard>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ChatView {
    pub character: CharacterView,
    pub lines: Vec<ChatLine>,
    /// Your lines that would fit now: the help.
    pub suggestions: Vec<PhraseView>,
    /// Why the phone's model couldn't help with your last line, for the
    /// screen.
    pub note: Option<String>,
    /// They've run out of things to talk about.
    pub ended: bool,
}

// MARK: Errors

/// What can go wrong. Arrives in Swift as a thrown `UrduError`.
#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq)]
pub enum UrduError {
    #[error("no phrase with id {id}")]
    NotFound { id: String },
    #[error("names are at most {max} characters")]
    NameTooLong { max: u32 },
    #[error("the app is open in another copy of the app")]
    InUse,
    #[error("the saved progress can't be read: {reason}")]
    Corrupt { reason: String },
    #[error("the progress was saved by a newer version of the app")]
    NewerData,
    #[error("storage error: {reason}")]
    Storage { reason: String },
    /// Stopped by the runtime: cancelled, interrupted or shut down.
    #[error("stopped ({kind:?})")]
    Stopped { kind: ErrorKind },
}

forge_app::app_error_conversions!(UrduError: Stopped, OpenError, DocumentError);

/// The framework kind of an error, so ForgeKit can present any of them.
#[uniffi::export]
pub fn urdu_error_kind(error: UrduError) -> ErrorKind {
    match error {
        UrduError::NotFound { .. } => ErrorKind::NotFound,
        UrduError::NameTooLong { .. } => ErrorKind::Invalid,
        UrduError::InUse => ErrorKind::Unavailable,
        UrduError::Corrupt { .. } => ErrorKind::Corrupt,
        UrduError::NewerData => ErrorKind::NewerData,
        UrduError::Storage { .. } => ErrorKind::Internal,
        UrduError::Stopped { kind } => kind,
    }
}

// MARK: The saved state and its rules

/// What is saved: settings, progress per phrase, and the attempts.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Saved {
    pub settings: Settings,
    pub phrases: BTreeMap<String, PhraseProgress>,
    pub events: Vec<ReviewEvent>,
}

impl Saved {
    fn progress(&self, id: &str) -> PhraseProgress {
        self.phrases.get(id).cloned().unwrap_or_default()
    }

    fn fill_name(&self, text: &str, blank: &str) -> String {
        let name = self.settings.name.trim();
        text.replace(NAME_SLOT, if name.is_empty() { blank } else { name })
    }

    pub fn phrase(&self, text: &PhraseText, now_ms: i64) -> PhraseView {
        let progress = self.progress(&text.id);
        let skills = progress.skills;
        let met = progress.times_seen > 0 || progress.next_review_ms.is_some();
        let stage = if skills.speaking >= 3 {
            Stage::CanUse
        } else if skills.recall >= 2 {
            Stage::CanRecall
        } else if skills.understanding() >= 2 {
            Stage::Recognised
        } else if met {
            Stage::Learning
        } else {
            Stage::New
        };
        let understanding = skills.understanding();
        let english_display = match self.settings.english {
            EnglishSupport::Always => EnglishDisplay::Visible,
            EnglishSupport::Minimal if met => EnglishDisplay::Hidden,
            EnglishSupport::Minimal => EnglishDisplay::Secondary,
            EnglishSupport::Gradual => match understanding {
                0 => EnglishDisplay::Visible,
                1 | 2 => EnglishDisplay::Secondary,
                _ => EnglishDisplay::Hidden,
            },
        };
        let content = Content::get();
        PhraseView {
            id: text.id.clone(),
            kind: text.kind,
            roman: self.fill_name(&text.roman, "…"),
            english: self.fill_name(&text.english, "…"),
            literal: text.literal.clone(),
            usage: text.usage.clone(),
            topic: text.topic.clone(),
            difficulty: text.difficulty,
            formality: text.formality,
            male_form: text.speaker == SpeakerForm::Male,
            status: text.status,
            variants: text
                .variants
                .iter()
                .map(|v| VariantView {
                    roman: v.roman.clone(),
                    kind: v.kind,
                    note: v.note.clone(),
                })
                .collect(),
            parts: text
                .parts
                .iter()
                .map(|p| PartView {
                    roman: p.roman.clone(),
                    english: p.english.clone(),
                })
                .collect(),
            combos: text.combos.iter().map(example_view).collect(),
            cloze: text.cloze.as_ref().and_then(|cloze| {
                let (before, after) = cloze.prompt.split_once(BLANK)?;
                Some(ClozeView {
                    before: before.to_string(),
                    after: after.to_string(),
                    english: cloze.english.clone(),
                })
            }),
            opposite: text
                .opposite
                .as_deref()
                .and_then(|id| content.phrase(id))
                .map(|o| EntryLink {
                    id: o.id.clone(),
                    roman: self.fill_name(&o.roman, "…"),
                    english: self.fill_name(&o.english, "…"),
                }),
            speech: self.fill_name(&text.urdu, ""),
            english_display,
            skills,
            stage,
            is_due: progress.is_due(now_ms),
            times_seen: progress.times_seen,
        }
    }

    fn scenario_summary(&self, scenario: &ScenarioText, now_ms: i64) -> ScenarioSummary {
        let content = Content::get();
        let mut yours: Vec<&str> = scenario
            .turns
            .iter()
            .filter(|t| t.who == Who::You)
            .map(|t| t.phrase.as_str())
            .collect();
        yours.sort_unstable();
        yours.dedup();
        let mut summary = ScenarioSummary {
            id: scenario.id.clone(),
            title: scenario.title.clone(),
            description: scenario.description.clone(),
            objective: scenario.objective.clone(),
            character: character_view(
                content
                    .character(&scenario.character)
                    .expect("checked when the content was read"),
            ),
            difficulty: scenario.difficulty,
            your_lines: yours.len() as u32,
            new_lines: 0,
            due_lines: 0,
            can_use: 0,
        };
        for id in yours {
            match self.phrases.get(id) {
                Some(p) if p.practised() => {
                    summary.due_lines += p.is_due(now_ms) as u32;
                    summary.can_use += (p.skills.speaking >= 3) as u32;
                }
                _ => summary.new_lines += 1,
            }
        }
        summary
    }

    pub fn scenario(&self, scenario: &ScenarioText, now_ms: i64) -> ScenarioView {
        let content = Content::get();
        ScenarioView {
            summary: self.scenario_summary(scenario, now_ms),
            turns: scenario
                .turns
                .iter()
                .map(|turn| TurnView {
                    speaker: match turn.who {
                        Who::Them => TurnSpeaker::Character,
                        Who::You => TurnSpeaker::You,
                    },
                    phrase: self.phrase(
                        content.phrase(&turn.phrase).expect("checked when read"),
                        now_ms,
                    ),
                })
                .collect(),
            words: content
                .words_in(scenario)
                .into_iter()
                .map(|w| self.phrase(w, now_ms))
                .collect(),
        }
    }

    fn topic(&self, topic: &TopicText) -> TopicView {
        let phrases: Vec<&PhraseText> = Content::get()
            .phrases
            .iter()
            .filter(|p| p.in_topic(&topic.id))
            .collect();
        TopicView {
            id: topic.id.clone(),
            level: topic.level,
            title: topic.title.clone(),
            description: topic.description.clone(),
            phrases: phrases.len() as u32,
            met: phrases
                .iter()
                .filter(|p| self.progress(&p.id).times_seen > 0)
                .count() as u32,
        }
    }

    pub fn topics(&self) -> Vec<TopicView> {
        Content::get()
            .topics
            .iter()
            .map(|t| self.topic(t))
            .collect()
    }

    pub fn totals(&self, now_ms: i64, day_start_ms: i64) -> Totals {
        let content = Content::get();
        let views: Vec<PhraseView> = content
            .phrases
            .iter()
            .map(|p| self.phrase(p, now_ms))
            .collect();
        let at_least = |stage: Stage| views.iter().filter(|v| v.stage >= stage).count() as u32;
        let of_kind = |kind: EntryKind| views.iter().filter(|v| v.kind == kind).count() as u32;
        let mut hardest: Vec<PhraseView> = views
            .iter()
            .filter(|v| content.is_spoken(&v.id) && v.stage > Stage::New && v.skills.speaking < 3)
            .cloned()
            .collect();
        hardest.sort_by_key(|v| {
            let p = self.progress(&v.id);
            (v.skills.speaking, std::cmp::Reverse(p.times_missed))
        });
        hardest.truncate(5);
        Totals {
            phrases: views.len() as u32,
            met: at_least(Stage::Learning),
            recognised: at_least(Stage::Recognised),
            can_recall: at_least(Stage::CanRecall),
            can_use: at_least(Stage::CanUse),
            due: views.iter().filter(|v| v.is_due).count() as u32,
            words: of_kind(EntryKind::Word),
            expressions: of_kind(EntryKind::Expression),
            patterns: of_kind(EntryKind::Pattern),
            attempts_today: self
                .events
                .iter()
                .filter(|e| e.at_ms >= day_start_ms)
                .count() as u32,
            hardest_to_say: hardest,
        }
    }

    pub fn home(&self, now_ms: i64, day_start_ms: i64) -> Home {
        let content = Content::get();
        let totals = self.totals(now_ms, day_start_ms);
        // Easiest first; the file's order within a difficulty.
        let mut scenarios: Vec<ScenarioSummary> = content
            .scenarios
            .iter()
            .map(|s| self.scenario_summary(s, now_ms))
            .collect();
        scenarios.sort_by_key(|s| s.difficulty);
        let topics = self.topics();
        // Always a conversation, or practising what came up in one. Word
        // lists are for looking things up, never the next thing to do.
        let suggestion = if totals.due >= 5 {
            Suggestion::Review { count: totals.due }
        } else if let Some(s) = scenarios.iter().find(|s| s.new_lines > 0) {
            Suggestion::Conversation {
                scenario_id: s.id.clone(),
            }
        } else if totals.due > 0 {
            Suggestion::Review { count: totals.due }
        } else {
            Suggestion::AllDone
        };
        Home {
            suggestion,
            totals,
            scenarios,
            topics,
        }
    }

    /// Phrases in `topic` (all when None) whose Roman Urdu, English or
    /// accepted variants contain `query`, ignoring case and spelling.
    pub fn search(&self, topic: Option<&str>, query: &str, now_ms: i64) -> Vec<PhraseView> {
        let folded = answer::fold_loose(query);
        let lower = query.trim().to_lowercase();
        Content::get()
            .phrases
            .iter()
            .filter(|p| topic.is_none_or(|t| p.in_topic(t)))
            .filter(|p| {
                lower.is_empty()
                    || p.english.to_lowercase().contains(&lower)
                    || std::iter::once(&p.roman)
                        .chain(p.variants.iter().map(|v| &v.roman))
                        .any(|r| answer::fold_loose(r).contains(&folded))
            })
            .map(|p| self.phrase(p, now_ms))
            .collect()
    }

    /// Due phrases, the longest overdue first, each with the skill to
    /// practise: what it means while you don't recognise it, then saying it
    /// from the English.
    pub fn review(&self, limit: u32, now_ms: i64) -> Vec<ReviewItem> {
        let content = Content::get();
        let mut due: Vec<(&PhraseText, i64)> = content
            .phrases
            .iter()
            .filter_map(|p| {
                let progress = self.phrases.get(&p.id)?;
                let due = progress.next_review_ms?;
                (due <= now_ms).then_some((p, due))
            })
            .collect();
        due.sort_by_key(|(_, due)| *due);
        due.into_iter()
            .take(limit as usize)
            .map(|(p, _)| {
                let phrase = self.phrase(p, now_ms);
                let skill = review_skill(&phrase);
                let example =
                    (skill == Skill::Speaking && phrase.kind == EntryKind::Pattern).then(|| {
                        // A different sentence each time round.
                        let turn = self.progress(&p.id).times_correct as usize;
                        example_view(&p.combos[turn % p.combos.len()])
                    });
                ReviewItem {
                    phrase,
                    skill,
                    example,
                }
            })
            .collect()
    }

    /// A listening round: phrases you've met, weakest listening first, then
    /// new ones; each with three meanings to choose from. `seed` varies the
    /// order of the choices and the distractors.
    pub fn listening(&self, count: u32, seed: u64, now_ms: i64) -> Vec<ListeningItem> {
        let content = Content::get();
        // A pattern has a gap, so there is nothing whole to hear.
        let hearable = |p: &&PhraseText| p.kind != EntryKind::Pattern;
        let mut pool: Vec<&PhraseText> = content.phrases.iter().filter(hearable).collect();
        pool.sort_by_key(|p| {
            let progress = self.progress(&p.id);
            (progress.times_seen == 0, progress.skills.listening)
        });
        let mut rng = Lcg(seed);
        pool.into_iter()
            .take(count as usize)
            .enumerate()
            .map(|(i, p)| {
                let mut others: Vec<&str> = content
                    .phrases
                    .iter()
                    .filter(hearable)
                    .filter(|o| o.english != p.english && !o.english.contains(NAME_SLOT))
                    .map(|o| o.english.as_str())
                    .collect();
                let mut choices = vec![self.fill_name(&p.english, "…")];
                for _ in 0..2 {
                    if others.is_empty() {
                        break;
                    }
                    let k = rng.below(others.len());
                    choices.push(others.swap_remove(k).to_string());
                }
                let answer = rng.below(choices.len());
                choices.swap(0, answer);
                ListeningItem {
                    phrase: self.phrase(p, now_ms),
                    choices,
                    answer: answer as u32,
                    voice: if i % 2 == 0 {
                        Gender::Female
                    } else {
                        Gender::Male
                    },
                }
            })
            .collect()
    }

    /// Checks a typed answer against the phrase and its accepted spellings
    /// and wordings, in your (male) form.
    pub fn check(&self, text: &PhraseText, typed: &str) -> Verdict {
        let roman = self.fill_name(&text.roman, "");
        let forms = std::iter::once(roman.as_str()).chain(
            text.variants
                .iter()
                .filter(|v| {
                    !matches!(
                        v.kind,
                        VariantKind::FemaleSpeaker | VariantKind::FemaleAddressee
                    )
                })
                .map(|v| v.roman.as_str()),
        );
        answer::check(typed, forms)
    }

    fn chat(&self, now: &ChatNow) -> ChatView {
        let content = Content::get();
        let at = forge_runtime::now_ms();
        let view = |id: &String| self.phrase(content.phrase(id).expect("checked when read"), at);
        let chat = &now.chat;
        ChatView {
            character: character_view(
                content
                    .character(&chat.character)
                    .expect("checked when the chat started"),
            ),
            lines: chat
                .lines
                .iter()
                .map(|line| ChatLine {
                    speaker: match line.who {
                        Who::Them => TurnSpeaker::Character,
                        Who::You => TurnSpeaker::You,
                    },
                    phrase: line.phrase.as_ref().map(view),
                    typed: line.typed.clone(),
                    heard: line.heard.map(|heard| match heard {
                        chat::Heard::Said => ChatHeard::Said,
                        chat::Heard::Nearly => ChatHeard::Nearly,
                        chat::Heard::Meant => ChatHeard::Meant,
                        chat::Heard::Picked => ChatHeard::Picked,
                        chat::Heard::Missed => ChatHeard::Missed,
                    }),
                })
                .collect(),
            suggestions: chat.suggestions().iter().map(view).collect(),
            note: now.note.clone(),
            ended: chat.ended,
        }
    }

    pub fn seen(&mut self, ids: &[String], now_ms: i64) -> Result<(), UrduError> {
        for id in ids {
            Content::get()
                .phrase(id)
                .ok_or(UrduError::NotFound { id: id.clone() })?;
        }
        for id in ids {
            self.phrases.entry(id.clone()).or_default().seen(now_ms);
        }
        Ok(())
    }

    pub fn attempt(
        &mut self,
        id: &str,
        skill: Skill,
        outcome: Outcome,
        latency_ms: Option<u32>,
        now_ms: i64,
    ) -> Result<(), UrduError> {
        Content::get()
            .phrase(id)
            .ok_or(UrduError::NotFound { id: id.to_string() })?;
        self.phrases
            .entry(id.to_string())
            .or_default()
            .attempt(skill, outcome, now_ms);
        self.events.push(ReviewEvent {
            phrase: id.to_string(),
            skill,
            outcome,
            at_ms: now_ms,
            latency_ms,
        });
        if self.events.len() > MAX_EVENTS {
            let extra = self.events.len() - MAX_EVENTS;
            self.events.drain(..extra);
        }
        Ok(())
    }
}

fn character_view(c: &CharacterText) -> CharacterView {
    CharacterView {
        id: c.id.clone(),
        name: c.name.clone(),
        gender: c.gender,
        description: c.description.clone(),
        speech_rate: c.speech_rate,
    }
}

/// A tiny deterministic generator, so a round is the same for a seed.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as usize) % n.max(1)
    }
}

// MARK: The core

/// What the command thread owns.
struct State {
    saved: Document<Saved>,
    published: Arc<Published<Saved>>,
}

impl State {
    fn change<T>(
        &mut self,
        edit: impl FnOnce(&mut Saved) -> Result<T, UrduError>,
    ) -> Result<T, UrduError> {
        let value = self.saved.change(edit)?;
        self.published.set(self.saved.value().clone());
        Ok(value)
    }
}

/// The app. ForgeKit's `ForgeHost` opens one per process and keeps it.
#[derive(uniffi::Object)]
pub struct UrduCore {
    rt: Runtime<State>,
    saved: Arc<Published<Saved>>,
    /// The chat going on, if any. In memory only: what you type is never
    /// saved; what you practised is.
    chat: Arc<Mutex<Option<ChatNow>>>,
    /// Chats started, for each chat's serial.
    chats: AtomicU64,
}

/// A chat and the note from your last line.
struct ChatNow {
    /// Which chat this is, so an answer worked out for an earlier chat
    /// never lands in this one.
    serial: u64,
    chat: chat::Chat,
    note: Option<String>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

#[uniffi::export]
impl UrduCore {
    /// Opens the progress saved in `storage_directory`, or starts fresh.
    /// Fails with `InUse` if another core has that folder open.
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Result<Arc<Self>, UrduError> {
        forge_runtime::logging::init();
        // Read (and check) the curriculum now, not on the first screen.
        let content = Content::get();
        forge_runtime::logging::info!(
            "curriculum: {} phrases, {} conversations",
            content.phrases.len(),
            content.scenarios.len()
        );
        let directory = DataDirectory::open(&storage_directory)?;
        let saved = Document::<Saved>::open(directory.path().join(SAVE_FILE), SAVE_VERSION)?;
        let published = Arc::new(Published::new(saved.value().clone()));
        let state = State {
            saved,
            published: Arc::clone(&published),
        };
        Ok(Arc::new(Self {
            rt: Runtime::with_directory("urdu-naturally", directory, state),
            saved: published,
            chat: Arc::new(Mutex::new(None)),
            chats: AtomicU64::new(0),
        }))
    }

    /// The home screen. `day_start_ms` is when today began on the phone's
    /// clock (Swift knows the time zone).
    pub fn home(&self, day_start_ms: i64) -> Home {
        self.saved.get().home(forge_runtime::now_ms(), day_start_ms)
    }

    pub fn totals(&self, day_start_ms: i64) -> Totals {
        self.saved
            .get()
            .totals(forge_runtime::now_ms(), day_start_ms)
    }

    pub fn topics(&self) -> Vec<TopicView> {
        self.saved.get().topics()
    }

    /// Phrases in a topic (every topic when None) matching `query` (all
    /// when empty).
    pub fn phrases(&self, topic: Option<String>, query: String) -> Vec<PhraseView> {
        self.saved
            .get()
            .search(topic.as_deref(), &query, forge_runtime::now_ms())
    }

    pub fn phrase(&self, id: String) -> Option<PhraseView> {
        let text = Content::get().phrase(&id)?;
        Some(self.saved.get().phrase(text, forge_runtime::now_ms()))
    }

    pub fn characters(&self) -> Vec<CharacterView> {
        Content::get()
            .characters
            .iter()
            .map(character_view)
            .collect()
    }

    pub fn scenario(&self, id: String) -> Option<ScenarioView> {
        let scenario = Content::get().scenario(&id)?;
        Some(self.saved.get().scenario(scenario, forge_runtime::now_ms()))
    }

    /// Up to `limit` phrases due again.
    pub fn review(&self, limit: u32) -> Vec<ReviewItem> {
        self.saved.get().review(limit, forge_runtime::now_ms())
    }

    /// A listening round of `count` phrases.
    pub fn listening(&self, count: u32, seed: u64) -> Vec<ListeningItem> {
        self.saved
            .get()
            .listening(count, seed, forge_runtime::now_ms())
    }

    /// Checks a typed answer against one sentence: a pattern's example.
    pub fn check_sentence(&self, sentence: String, typed: String) -> Verdict {
        answer::check(&typed, [sentence.as_str()])
    }

    /// Checks a typed answer. Unknown ids are `Different`.
    pub fn check_answer(&self, phrase_id: String, typed: String) -> Verdict {
        match Content::get().phrase(&phrase_id) {
            Some(text) => self.saved.get().check(text, &typed),
            None => Verdict::Different,
        }
    }

    pub fn settings(&self) -> Settings {
        self.saved.get().settings.clone()
    }

    /// Records that phrases were shown (a card, a conversation).
    pub async fn seen(&self, phrase_ids: Vec<String>) -> Result<(), UrduError> {
        self.rt
            .command("seen", move |state| {
                let now = forge_runtime::now_ms();
                state.change(|saved| saved.seen(&phrase_ids, now))
            })
            .await
    }

    /// Records an attempt at a skill, and returns the phrase as it is now.
    pub async fn record(
        &self,
        phrase_id: String,
        skill: Skill,
        outcome: Outcome,
        latency_ms: Option<u32>,
    ) -> Result<PhraseView, UrduError> {
        self.rt
            .command("record", move |state| {
                let now = forge_runtime::now_ms();
                state.change(|saved| saved.attempt(&phrase_id, skill, outcome, latency_ms, now))?;
                let text = Content::get()
                    .phrase(&phrase_id)
                    .ok_or(UrduError::NotFound {
                        id: phrase_id.clone(),
                    })?;
                Ok(state.saved.value().phrase(text, now))
            })
            .await
    }

    pub async fn set_name(&self, name: String) -> Result<(), UrduError> {
        let name = name.trim().to_string();
        if name.chars().count() > MAX_NAME_CHARS as usize {
            return Err(UrduError::NameTooLong {
                max: MAX_NAME_CHARS,
            });
        }
        self.rt
            .command("set-name", move |state| {
                state.change(|saved| {
                    saved.settings.name = name;
                    Ok(())
                })
            })
            .await
    }

    pub async fn set_english(&self, english: EnglishSupport) -> Result<(), UrduError> {
        self.rt
            .command("set-english", move |state| {
                state.change(|saved| {
                    saved.settings.english = english;
                    Ok(())
                })
            })
            .await
    }

    // MARK: Free chat

    /// Starts a free chat with a character: they open with something to
    /// talk about. `seed` makes each chat different.
    pub async fn chat_start(&self, character_id: String, seed: u64) -> Result<ChatView, UrduError> {
        let chat = chat::Chat::start(&character_id, seed)
            .ok_or(UrduError::NotFound { id: character_id })?;
        let theirs = their_lines(&chat, 0);
        {
            let mut now = lock(&self.chat);
            let serial = self.chats.fetch_add(1, Ordering::Relaxed);
            *now = Some(ChatNow {
                serial,
                chat,
                note: None,
            });
        }
        self.chat_record(theirs, None).await
    }

    /// The chat going on, if any.
    pub fn chat(&self) -> Option<ChatView> {
        let now = lock(&self.chat);
        now.as_ref().map(|now| self.saved.get().chat(now))
    }

    /// Ends the chat and forgets what you typed in it.
    pub fn chat_end(&self) {
        lock(&self.chat).take();
    }

    /// You say something (in Roman Urdu, English or both) and they answer.
    /// `model` is the phone's model, which may say which of your lines you
    /// meant; without one only your lines are understood. Saying one of
    /// your lines counts as practising it.
    pub async fn chat_say(
        &self,
        text: String,
        model: Option<Arc<dyn AiModel>>,
    ) -> Result<ChatView, UrduError> {
        let text: String = text.trim().chars().take(chat::MAX_TYPED_CHARS).collect();
        let (serial, current) = lock(&self.chat)
            .as_ref()
            .map(|now| (now.serial, now.chat.clone()))
            .ok_or(UrduError::NotFound { id: "chat".into() })?;
        if text.is_empty() {
            return Ok(self.chat().expect("a chat is going on"));
        }
        let saved = Arc::clone(&self.saved);
        let typed = text.clone();
        let understood = self
            .rt
            .run("chat-understand", self.rt.foreground(None), move |cancel| {
                let cancel = cancel.clone();
                let model = model.map(HostModel);
                let saved = saved.get();
                Ok::<_, UrduError>(
                    current.understand(
                        &typed,
                        |phrase, typed| saved.check(phrase, typed),
                        model
                            .as_ref()
                            .map(|m| m as &dyn forge_services::ai::LanguageModel),
                        move || cancel.is_cancelled(),
                    ),
                )
            })
            .await?;
        self.chat_answer(Some(serial), Some(text), understood).await
    }

    /// You pick a line from the help, and they answer.
    pub async fn chat_pick(&self, phrase_id: String) -> Result<ChatView, UrduError> {
        if !Content::get().is_spoken(&phrase_id) {
            return Err(UrduError::NotFound { id: phrase_id });
        }
        self.chat_answer(None, None, chat::Understood::picked(&phrase_id))
            .await
    }

    /// Forgets all progress and attempts (keeps the settings).
    pub async fn start_over(&self) -> Result<(), UrduError> {
        self.rt
            .command("start-over", |state| {
                state.change(|saved| {
                    saved.phrases.clear();
                    saved.events.clear();
                    Ok(())
                })
            })
            .await
    }
}

// events(), set_phase(), phase(), revision() and shutdown(), for ForgeKit.
forge_app::export_runtime!(UrduCore, rt);

impl UrduCore {
    /// Applies your turn to the chat, if it is still `serial` (the chat it
    /// was worked out for): a chat started meanwhile is left alone, and
    /// nothing is recorded.
    async fn chat_answer(
        &self,
        serial: Option<u64>,
        typed: Option<String>,
        understood: chat::Understood,
    ) -> Result<ChatView, UrduError> {
        let (theirs, yours) = {
            let mut now = lock(&self.chat);
            let now = now
                .as_mut()
                .ok_or(UrduError::NotFound { id: "chat".into() })?;
            if serial.is_some_and(|s| s != now.serial) {
                return Ok(self.saved.get().chat(now));
            }
            let before = now.chat.lines.len() + 1;
            now.chat.answer(typed, &understood);
            now.note = understood.note.clone();
            let yours = understood.phrase.clone().map(|id| (id, understood.heard));
            (their_lines(&now.chat, before), yours)
        };
        self.chat_record(theirs, yours).await
    }

    /// Saves what a chat turn practised: their lines (and the line you
    /// meant or picked) count as met; a line you said counts as speaking
    /// it, on your own or nearly. Then shows the chat as it is now.
    async fn chat_record(
        &self,
        mut seen: Vec<String>,
        yours: Option<(String, chat::Heard)>,
    ) -> Result<ChatView, UrduError> {
        let spoken = match yours {
            Some((id, chat::Heard::Said)) => Some((id, Outcome::Independent)),
            Some((id, chat::Heard::Nearly)) => Some((id, Outcome::WithHelp)),
            Some((id, chat::Heard::Meant | chat::Heard::Picked)) => {
                seen.push(id);
                None
            }
            Some((_, chat::Heard::Missed)) | None => None,
        };
        self.rt
            .command("chat", move |state| {
                let now = forge_runtime::now_ms();
                state.change(|saved| {
                    saved.seen(&seen, now)?;
                    if let Some((id, outcome)) = &spoken {
                        saved.attempt(id, Skill::Speaking, *outcome, None, now)?;
                    }
                    Ok(())
                })
            })
            .await?;
        self.chat().ok_or(UrduError::NotFound { id: "chat".into() })
    }
}

/// Their lines from `from` on.
fn their_lines(chat: &chat::Chat, from: usize) -> Vec<String> {
    chat.lines
        .iter()
        .skip(from)
        .filter(|l| l.who == Who::Them)
        .filter_map(|l| l.phrase.clone())
        .collect()
}

#[cfg(test)]
mod tests;

/// What to practise a due entry with. What it means first; then saying it
/// from the English. A word with a gap line is also used in the line, and a
/// pattern is mostly practised by building new sentences with it.
fn review_skill(phrase: &PhraseView) -> Skill {
    let skills = &phrase.skills;
    if skills.recognition < 2 {
        Skill::Recognition
    } else if phrase.kind == EntryKind::Pattern {
        if skills.recall < 2 || skills.speaking >= skills.recall + 2 {
            Skill::Recall
        } else {
            Skill::Speaking
        }
    } else if phrase.cloze.is_some() && skills.speaking < skills.recall {
        Skill::Speaking
    } else {
        Skill::Recall
    }
}

fn example_view(combo: &content::Combo) -> ExampleView {
    ExampleView {
        roman: combo.roman.clone(),
        english: combo.english.clone(),
        speech: combo.urdu.clone(),
    }
}
