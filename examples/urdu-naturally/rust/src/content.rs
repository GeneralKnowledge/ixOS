//! The curriculum: phrases, topics, characters and scenarios, from
//! `content/*.toml`, compiled into the app. It is the source of linguistic
//! truth; nothing else (and no AI) adds Urdu to the app. [`Content::get`]
//! checks every rule an editor of those files could break, and the tests run
//! the same checks on Linux.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::Deserialize;

const PHRASES_TOML: &str = include_str!("../content/phrases.toml");
const SCENARIOS_TOML: &str = include_str!("../content/scenarios.toml");
const WORDS_TOML: &str = include_str!("../content/words.toml");

/// Where a word goes in a cloze line.
pub const BLANK: &str = "___";

/// Replaced by the learner's name wherever it appears in a phrase.
pub const NAME_SLOT: &str = "{name}";

#[derive(uniffi::Enum, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Formality {
    Informal,
    Neutral,
    Polite,
    Formal,
    ContextDependent,
}

/// How far a phrase has been checked. Everything starts as a draft, and
/// only a person moves it on: the app never certifies its own Urdu.
#[derive(uniffi::Enum, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentStatus {
    /// Written (by a person or an AI), not yet checked.
    Draft,
    /// Grammar, usage and naturalness checked by a competent Pakistani
    /// Urdu speaker.
    Reviewed,
    /// Confirmed natural in a real conversation of its kind.
    FieldTested,
}

/// Whose form a phrase is.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpeakerForm {
    /// The same whoever says it.
    Any,
    /// A man's form (the learner's): its female form is a variant.
    Male,
}

#[derive(uniffi::Enum, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VariantKind {
    /// How a woman says it.
    FemaleSpeaker,
    /// How it is said to a woman.
    FemaleAddressee,
    Polite,
    Formal,
    Informal,
    /// Another way to say the same thing.
    Alternative,
    /// The same words, spelt another common way.
    Spelling,
    /// A describing word's feminine form (achi).
    Feminine,
    /// A describing word's plural form (ache).
    Plural,
}

/// What kind of thing an entry teaches, which decides how it is practised.
#[derive(uniffi::Enum, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// A conversational line (phrases.toml).
    #[default]
    Phrase,
    /// One word, or a pair that works as one (woh log): nahi, mat, bhi.
    Word,
    /// A fixed expression you learn whole: mujhe nahi lagta, koi masla nahi.
    Expression,
    /// A sentence frame with a gap, for making your own sentences:
    /// Main ___ sakta hoon. Practised by building new sentences from it.
    Pattern,
}

/// The foundation (Building Blocks) or the conversations built on it.
#[derive(uniffi::Enum, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Foundation,
    #[default]
    Conversation,
}

/// A common spoken combination of a word ("main bhi").
#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Combo {
    pub roman: String,
    pub english: String,
    /// Urdu script, for the voice. Required for a pattern's examples, which
    /// are practised aloud.
    pub urdu: Option<String>,
}

/// A line with a gap where the word goes, for practising it in use.
#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Cloze {
    /// Roman Urdu with ___ for the gap.
    pub prompt: String,
    /// The whole line in English.
    pub english: String,
}

fn one() -> u32 {
    1
}

fn neutral() -> Formality {
    Formality::Neutral
}

fn any_speaker() -> SpeakerForm {
    SpeakerForm::Any
}

fn draft() -> ContentStatus {
    ContentStatus::Draft
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    pub roman: String,
    pub kind: VariantKind,
    pub note: Option<String>,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub roman: String,
    pub english: String,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PhraseText {
    pub id: String,
    pub roman: String,
    pub english: String,
    /// Urdu script, for the voice only.
    pub urdu: String,
    pub topic: String,
    /// Other topics it belongs to.
    #[serde(default)]
    pub also_in: Vec<String>,
    #[serde(default = "one")]
    pub difficulty: u32,
    #[serde(default = "neutral")]
    pub formality: Formality,
    #[serde(default = "any_speaker")]
    pub speaker: SpeakerForm,
    pub literal: Option<String>,
    pub usage: Option<String>,
    #[serde(default = "draft")]
    pub status: ContentStatus,
    /// Its positive or negative twin, taught alongside it.
    pub opposite: Option<String>,
    #[serde(default, rename = "variant", alias = "variants")]
    pub variants: Vec<Variant>,
    #[serde(default, rename = "part")]
    pub parts: Vec<Part>,
    #[serde(default)]
    pub combos: Vec<Combo>,
    pub cloze: Option<Cloze>,
    /// In words.toml only: word, expression or pattern. Left out, it is a
    /// pattern if roman has a ___, a word if roman is one word, and an
    /// expression otherwise.
    #[serde(default, rename = "kind")]
    declared_kind: Option<EntryKind>,
    /// Set from the file it came from and `kind`.
    #[serde(skip)]
    pub kind: EntryKind,
}

impl PhraseText {
    pub fn in_topic(&self, topic: &str) -> bool {
        self.topic == topic || self.also_in.iter().any(|t| t == topic)
    }
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TopicText {
    pub id: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub level: Level,
}

#[derive(uniffi::Enum, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Gender {
    Male,
    Female,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CharacterText {
    pub id: String,
    pub name: String,
    pub gender: Gender,
    pub description: String,
    /// 1.0 is the voice's normal pace.
    pub speech_rate: f32,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Who {
    Them,
    You,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    pub who: Who,
    pub phrase: String,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ScenarioText {
    pub id: String,
    pub title: String,
    pub description: String,
    pub character: String,
    pub difficulty: u32,
    pub objective: String,
    pub turns: Vec<Turn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PhrasesFile {
    topic: Vec<TopicText>,
    phrase: Vec<PhraseText>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct WordsFile {
    #[serde(default)]
    topic: Vec<TopicText>,
    #[serde(default)]
    word: Vec<PhraseText>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenariosFile {
    character: Vec<CharacterText>,
    scenario: Vec<ScenarioText>,
}

/// Everything the app teaches.
#[derive(Debug)]
pub struct Content {
    pub topics: Vec<TopicText>,
    /// In the order of the file, which is the order they are taught.
    pub phrases: Vec<PhraseText>,
    pub characters: Vec<CharacterText>,
    pub scenarios: Vec<ScenarioText>,
    index: HashMap<String, usize>,
    /// Phrases you say in some scenario.
    spoken: HashSet<String>,
}

impl Content {
    /// The content compiled into the app. Panics if a file breaks a rule;
    /// `cargo test` catches that long before a build does.
    pub fn get() -> &'static Content {
        static CONTENT: OnceLock<Content> = OnceLock::new();
        CONTENT.get_or_init(|| {
            Content::parse(PHRASES_TOML, WORDS_TOML, SCENARIOS_TOML).expect("content/*.toml")
        })
    }

    pub fn parse(phrases: &str, words: &str, scenarios: &str) -> Result<Content, String> {
        let mut phrases: PhrasesFile =
            toml::from_str(phrases).map_err(|e| format!("phrases.toml: {e}"))?;
        let words: WordsFile = toml::from_str(words).map_err(|e| format!("words.toml: {e}"))?;
        // The foundation's topics come first; its words after the phrases.
        let mut topics = words.topic;
        topics.append(&mut phrases.topic);
        phrases.topic = topics;
        if let Some(p) = phrases.phrase.iter().find(|p| p.declared_kind.is_some()) {
            return Err(format!("{}: only words.toml entries have a kind", p.id));
        }
        for mut word in words.word {
            word.kind = match word.declared_kind {
                Some(EntryKind::Phrase) => {
                    return Err(format!("{}: a building block isn't a phrase", word.id))
                }
                Some(kind) => kind,
                None if word.roman.contains(BLANK) => EntryKind::Pattern,
                None if word.roman.split_whitespace().count() == 1 => EntryKind::Word,
                None => EntryKind::Expression,
            };
            phrases.phrase.push(word);
        }
        let scenarios: ScenariosFile =
            toml::from_str(scenarios).map_err(|e| format!("scenarios.toml: {e}"))?;

        let topics: HashSet<&str> = phrases.topic.iter().map(|t| t.id.as_str()).collect();
        unique(phrases.topic.iter().map(|t| &t.id), "topic")?;
        unique(phrases.phrase.iter().map(|p| &p.id), "phrase")?;
        unique(scenarios.character.iter().map(|c| &c.id), "character")?;
        unique(scenarios.scenario.iter().map(|s| &s.id), "scenario")?;

        for phrase in &phrases.phrase {
            check_phrase(phrase, &topics)?;
        }
        let index: HashMap<String, usize> = phrases
            .phrase
            .iter()
            .enumerate()
            .map(|(i, p)| (p.id.clone(), i))
            .collect();
        for phrase in &phrases.phrase {
            if let Some(opposite) = &phrase.opposite {
                let Some(&twin) = index.get(opposite).filter(|_| opposite != &phrase.id) else {
                    return Err(format!(
                        "{}: opposite {opposite} is not another entry",
                        phrase.id
                    ));
                };
                // Twins are taught together, so each names the other.
                if phrases.phrase[twin].opposite.as_ref() != Some(&phrase.id) {
                    return Err(format!(
                        "{}: its opposite {opposite} doesn't name it back",
                        phrase.id
                    ));
                }
            }
        }

        let characters: HashSet<&str> = scenarios.character.iter().map(|c| c.id.as_str()).collect();
        let mut spoken = HashSet::new();
        for scenario in &scenarios.scenario {
            let id = &scenario.id;
            if !characters.contains(scenario.character.as_str()) {
                return Err(format!(
                    "scenario {id}: unknown character {}",
                    scenario.character
                ));
            }
            if !(1..=5).contains(&scenario.difficulty) {
                return Err(format!("scenario {id}: difficulty is 1 to 5"));
            }
            if !scenario.turns.iter().any(|t| t.who == Who::You) {
                return Err(format!("scenario {id} gives you nothing to say"));
            }
            for turn in &scenario.turns {
                if !index.contains_key(&turn.phrase) {
                    return Err(format!("scenario {id}: unknown phrase {}", turn.phrase));
                }
                if turn.who == Who::You {
                    spoken.insert(turn.phrase.clone());
                }
            }
        }
        for character in &scenarios.character {
            if !(0.5..=1.5).contains(&character.speech_rate) {
                return Err(format!("{}: speech_rate is 0.5 to 1.5", character.id));
            }
        }

        Ok(Content {
            topics: phrases.topic,
            phrases: phrases.phrase,
            characters: scenarios.character,
            scenarios: scenarios.scenario,
            index,
            spoken,
        })
    }

    pub fn phrase(&self, id: &str) -> Option<&PhraseText> {
        self.index.get(id).map(|&i| &self.phrases[i])
    }

    pub fn scenario(&self, id: &str) -> Option<&ScenarioText> {
        self.scenarios.iter().find(|s| s.id == id)
    }

    pub fn character(&self, id: &str) -> Option<&CharacterText> {
        self.characters.iter().find(|c| c.id == id)
    }

    pub fn topic(&self, id: &str) -> Option<&TopicText> {
        self.topics.iter().find(|t| t.id == id)
    }

    /// The building blocks used in a scenario's lines, in the order they
    /// first come up: each word and expression whose words appear together
    /// in a line, and each pattern a line follows (Mujhe ___ chahiye in
    /// "Mujhe chai chahiye"). Matched on exact spellings, so "mein" (in)
    /// never counts as "main" (I).
    pub fn words_in(&self, scenario: &ScenarioText) -> Vec<&PhraseText> {
        let lines: Vec<Vec<String>> = scenario
            .turns
            .iter()
            .filter_map(|t| self.phrase(&t.phrase))
            .map(|p| tokens(&p.roman))
            .collect();
        let mut found: Vec<(usize, usize, &PhraseText)> = Vec::new();
        for entry in self.phrases.iter().filter(|p| p.kind != EntryKind::Phrase) {
            let at = lines.iter().enumerate().find_map(|(n, line)| {
                let pos = if entry.kind == EntryKind::Pattern {
                    let (before, after) = entry.roman.split_once(BLANK)?;
                    follows(line, &tokens(before), &tokens(after))
                } else {
                    find(line, &tokens(&entry.roman), 0)
                };
                pos.map(|p| (n, p))
            });
            if let Some((n, p)) = at {
                found.push((n, p, entry));
            }
        }
        found.sort_by_key(|(n, p, _)| (*n, *p));
        found.into_iter().map(|(_, _, e)| e).collect()
    }

    /// True for a phrase you say in some scenario.
    pub fn is_spoken(&self, id: &str) -> bool {
        self.spoken.contains(id)
    }
}

/// Lower-case words, letters only: no spelling is merged.
fn tokens(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Where `needle` starts in `line`, at or after `from`.
fn find(line: &[String], needle: &[String], from: usize) -> Option<usize> {
    if needle.is_empty() || needle.len() > line.len() {
        return None;
    }
    (from..=line.len() - needle.len()).find(|&i| line[i..i + needle.len()] == *needle)
}

/// Where a pattern's frame starts in `line`: its words before the gap, at
/// least one word in the gap, then its words after it.
fn follows(line: &[String], before: &[String], after: &[String]) -> Option<usize> {
    let start = if before.is_empty() {
        Some(0)
    } else {
        find(line, before, 0)
    }?;
    let gap = start + before.len();
    if after.is_empty() {
        return (gap < line.len()).then_some(start);
    }
    find(line, after, gap + 1).map(|_| start)
}

fn unique<'a>(ids: impl Iterator<Item = &'a String>, what: &str) -> Result<(), String> {
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(format!("{what} {id} appears twice"));
        }
    }
    Ok(())
}

fn check_phrase(phrase: &PhraseText, topics: &HashSet<&str>) -> Result<(), String> {
    let id = &phrase.id;
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(format!("{id}: ids are lower-case words joined by -"));
    }
    for (name, text) in [
        ("roman", &phrase.roman),
        ("english", &phrase.english),
        ("urdu", &phrase.urdu),
    ] {
        if text.trim().is_empty() {
            return Err(format!("{id}: {name} is empty"));
        }
    }
    for topic in std::iter::once(&phrase.topic).chain(&phrase.also_in) {
        if !topics.contains(topic.as_str()) {
            return Err(format!("{id}: unknown topic {topic}"));
        }
    }
    let gaps = |text: &str| text.matches(BLANK).count();
    if phrase.kind == EntryKind::Pattern {
        if gaps(&phrase.roman) != 1 || gaps(&phrase.urdu) != 1 {
            return Err(format!(
                "{id}: a pattern has exactly one {BLANK} in roman and in urdu"
            ));
        }
        if phrase.combos.len() < 2 {
            return Err(format!(
                "{id}: a pattern needs at least two examples (combos)"
            ));
        }
        for combo in &phrase.combos {
            let urdu = combo.urdu.as_deref().unwrap_or("");
            if gaps(&combo.roman) > 0 || !is_urdu_script(urdu) {
                return Err(format!(
                    "{id}: each example of a pattern is a whole sentence, with its urdu"
                ));
            }
        }
        if phrase.cloze.is_some() {
            return Err(format!(
                "{id}: a pattern is its own gap line; it has no cloze"
            ));
        }
    } else if gaps(&phrase.roman) > 0 || gaps(&phrase.urdu) > 0 {
        return Err(format!("{id}: only a pattern has a {BLANK} in it"));
    }
    for combo in &phrase.combos {
        if let Some(urdu) = &combo.urdu {
            if !is_urdu_script(urdu) {
                return Err(format!("{id}: a combo's urdu is not in Urdu script"));
            }
        }
    }
    if let Some(cloze) = &phrase.cloze {
        if gaps(&cloze.prompt) != 1 {
            return Err(format!("{id}: a cloze prompt has exactly one {BLANK}"));
        }
    }
    if !(1..=5).contains(&phrase.difficulty) {
        return Err(format!("{id}: difficulty is 1 to 5"));
    }
    if !is_urdu_script(&phrase.urdu.replace(NAME_SLOT, "")) {
        return Err(format!("{id}: urdu is not in Urdu script"));
    }
    if phrase
        .roman
        .chars()
        .any(|c| matches!(c as u32, 0x0600..=0x06FF))
    {
        return Err(format!("{id}: roman has Urdu script in it"));
    }
    let has_name = |text: &str| text.contains(NAME_SLOT);
    if has_name(&phrase.roman) != has_name(&phrase.urdu) {
        return Err(format!("{id}: {NAME_SLOT} must be in both roman and urdu"));
    }
    let female = phrase
        .variants
        .iter()
        .any(|v| v.kind == VariantKind::FemaleSpeaker);
    if (phrase.speaker == SpeakerForm::Male) != female {
        return Err(format!(
            "{id}: a male-form phrase needs a female_speaker variant, and only it"
        ));
    }
    for variant in &phrase.variants {
        if variant.roman.trim().is_empty() || variant.roman == phrase.roman {
            return Err(format!("{id}: a variant must differ from the phrase"));
        }
    }
    Ok(())
}

/// Letters only from the Arabic blocks (Urdu's script), plus spaces and
/// punctuation, and at least one letter.
fn is_urdu_script(text: &str) -> bool {
    let mut letters = 0;
    for c in text.chars().filter(|c| c.is_alphabetic()) {
        if !matches!(c as u32, 0x0600..=0x06FF | 0x0750..=0x077F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF)
        {
            return false;
        }
        letters += 1;
    }
    letters > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_SCENARIO: &str = r#"
        [[character]]
        id = "ali"
        name = "Ali"
        gender = "male"
        description = "d"
        speech_rate = 1.0
        [[scenario]]
        id = "s"
        title = "S"
        description = "d"
        character = "ali"
        difficulty = 1
        objective = "o"
        turns = [{ who = "you", phrase = "x" }]
    "#;

    impl Content {
        fn parse_test(phrases: &str, scenarios: &str) -> Result<Content, String> {
            Content::parse(phrases, "", scenarios)
        }
    }

    fn phrases(urdu: &str, speaker: &str) -> String {
        format!(
            r#"
            [[topic]]
            id = "t"
            title = "T"
            description = "d"
            [[phrase]]
            id = "x"
            roman = "Main ja raha hoon."
            english = "I'm going."
            urdu = "{urdu}"
            topic = "t"
            difficulty = 1
            formality = "neutral"
            speaker = "{speaker}"
            status = "draft"
            "#
        )
    }

    #[test]
    fn the_shipped_content_follows_every_rule() {
        let content = Content::parse(PHRASES_TOML, WORDS_TOML, SCENARIOS_TOML).unwrap();
        assert!(
            content.phrases.len() >= 50,
            "0.1 teaches at least 50 phrases"
        );
        assert!(
            content.scenarios.len() >= 14,
            "conversations are the course"
        );
        assert_eq!(content.characters.len(), 4);
        // Nothing has been checked by a native speaker yet, and the app
        // must not say otherwise.
        assert!(content
            .phrases
            .iter()
            .all(|p| p.status == ContentStatus::Draft));
    }

    #[test]
    fn latin_letters_in_the_urdu_are_refused() {
        let err =
            Content::parse_test(&phrases("main ja raha hoon", "any"), ONE_SCENARIO).unwrap_err();
        assert!(err.contains("Urdu script"), "{err}");
    }

    #[test]
    fn a_male_form_needs_its_female_form() {
        let err =
            Content::parse_test(&phrases("میں جا رہا ہوں۔", "male"), ONE_SCENARIO).unwrap_err();
        assert!(err.contains("female_speaker"), "{err}");
        assert!(Content::parse_test(&phrases("میں جا رہا ہوں۔", "any"), ONE_SCENARIO).is_ok());
    }

    #[test]
    fn a_scenario_must_name_known_phrases() {
        let scenario = ONE_SCENARIO.replace("phrase = \"x\"", "phrase = \"nope\"");
        let err = Content::parse_test(&phrases("میں جا رہا ہوں۔", "any"), &scenario).unwrap_err();
        assert!(err.contains("unknown phrase"), "{err}");
    }

    /// A words.toml with one topic and the given entries, against a
    /// phrases.toml whose one phrase "x" is spoken in ONE_SCENARIO.
    fn with_words(entries: &str) -> Result<Content, String> {
        let words = format!(
            "[[topic]]\nid = \"w\"\ntitle = \"W\"\ndescription = \"d\"\nlevel = \"foundation\"\n{entries}"
        );
        Content::parse(&phrases("میں جا رہا ہوں۔", "any"), &words, ONE_SCENARIO)
    }

    #[test]
    fn twins_must_name_each_other() {
        let one_way = r#"
            [[word]]
            id = "haan"
            roman = "Haan"
            english = "Yes"
            urdu = "ہاں"
            topic = "w"
            opposite = "nahi"
            [[word]]
            id = "nahi"
            roman = "Nahi"
            english = "No"
            urdu = "نہیں"
            topic = "w"
        "#;
        let err = with_words(one_way).unwrap_err();
        assert!(err.contains("doesn't name it back"), "{err}");
        let both = one_way.replace(
            "urdu = \"نہیں\"",
            "urdu = \"نہیں\"\n            opposite = \"haan\"",
        );
        with_words(&both).unwrap();
    }

    #[test]
    fn a_pattern_needs_a_gap_and_spoken_examples() {
        let pattern = |combos: &str| {
            format!(
                r#"
                [[word]]
                id = "p"
                roman = "Mujhe ___ chahiye."
                english = "I need ___."
                urdu = "مجھے ___ چاہیے۔"
                topic = "w"
                combos = [{combos}]
                "#
            )
        };
        let good = r#"{ roman = "Mujhe paani chahiye.", english = "I need water.", urdu = "مجھے پانی چاہیے۔" },
            { roman = "Mujhe madad chahiye.", english = "I need help.", urdu = "مجھے مدد چاہیے۔" }"#;
        let content = with_words(&pattern(good)).unwrap();
        assert_eq!(content.phrase("p").unwrap().kind, EntryKind::Pattern);
        let no_urdu = r#"{ roman = "Mujhe paani chahiye.", english = "I need water." },
            { roman = "Mujhe madad chahiye.", english = "I need help." }"#;
        assert!(with_words(&pattern(no_urdu)).is_err());
        assert!(with_words(&pattern("")).is_err());
        // A gap in anything but a pattern is a mistake.
        let stray = pattern(good).replace(
            "roman = \"Mujhe ___ chahiye.\"",
            "roman = \"Mujhe chahiye.\"",
        );
        assert!(with_words(&stray).is_err());
        // Only words.toml says what kind an entry is.
        let declared = phrases("میں جا رہا ہوں۔", "any").replace(
            "topic = \"t\"",
            "topic = \"t\"\n            kind = \"word\"",
        );
        assert!(Content::parse(&declared, "", ONE_SCENARIO).is_err());
    }
}
