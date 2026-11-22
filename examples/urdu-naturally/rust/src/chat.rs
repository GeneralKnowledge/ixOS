//! Free chat with Ali, Ayesha, Omar and Khala: you say anything, they
//! answer.
//!
//! The curriculum stays the source of truth. A character only ever says a
//! line from the conversations (`content/scenarios.toml`), never Urdu a
//! model wrote: Apple's on-device model can't write Pakistani Urdu
//! reliably, and nothing unchecked reaches the learner as Urdu. What makes
//! it free is the order. They answer what you said, drawing on every
//! conversation at once, and change the subject when a thread runs out, so
//! no two chats go the same way.
//!
//! What you said is understood in three steps:
//!
//! 1. **You said one of your lines.** The strict answer check (`answer`),
//!    against the lines that would fit here first, then any line you know
//!    how to say. Nearly right counts only for a line that fits here.
//! 2. **You meant one of them.** The phone's model, when there is one,
//!    picks which of your lines means what you said (Urdu, English or a
//!    mix) from a numbered list. It answers with a number and nothing else;
//!    you then see the line you could have said. Typing a line's English
//!    exactly also counts, with no model.
//! 3. **They didn't catch it.** They wait, and you can ask for help: the
//!    lines that would fit here.
//!
//! What you type is kept for this chat only, never saved and never logged.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use forge_services::ai::{ask, AiError, ChatRequest, LanguageModel};

use crate::answer::Verdict;
use crate::content::{Content, Gender, PhraseText, SpeakerForm, Who};

/// The most of your lines the model chooses between: enough for every
/// line that fits and plenty that don't, short enough for the phone's
/// model to read quickly.
pub const MAX_CANDIDATES: usize = 60;
/// The longest thing you can type in one go.
pub const MAX_TYPED_CHARS: usize = 300;

/// Who follows whom, across every conversation.
#[derive(Debug, Default)]
pub struct Graph {
    /// Their line, and your lines that answer it somewhere.
    replies: BTreeMap<String, Vec<String>>,
    /// Your line, and their lines that answer it: (character, phrase).
    answers: BTreeMap<String, Vec<(String, String)>>,
    /// Their line, and their lines that follow it straight on ("Main Ayesha
    /// hoon." then "Aap ka naam kya hai?"): said together, as one turn.
    then: BTreeMap<String, Vec<String>>,
    /// Your line, and your lines that follow it straight on ("Main theek
    /// hoon." then "Tum sunao?"): they wait for the rest.
    you_then: BTreeMap<String, Vec<String>>,
    /// How each conversation starts: (character, their first line).
    openers: Vec<(String, String)>,
    /// Every line you say somewhere, in the order the course meets them.
    yours: Vec<String>,
}

impl Graph {
    pub fn get() -> &'static Graph {
        static GRAPH: OnceLock<Graph> = OnceLock::new();
        GRAPH.get_or_init(|| Graph::new(Content::get()))
    }

    pub fn new(content: &Content) -> Graph {
        let mut graph = Graph::default();
        let mut scenarios: Vec<_> = content.scenarios.iter().collect();
        scenarios.sort_by_key(|s| s.difficulty);
        for scenario in scenarios {
            if let Some(first) = scenario.turns.first().filter(|t| t.who == Who::Them) {
                push(
                    (scenario.character.clone(), first.phrase.clone()),
                    &mut graph.openers,
                );
            }
            for turn in &scenario.turns {
                if turn.who == Who::You {
                    push(turn.phrase.clone(), &mut graph.yours);
                }
            }
            for pair in scenario.turns.windows(2) {
                match (pair[0].who, pair[1].who) {
                    (Who::Them, Who::You) => push(
                        pair[1].phrase.clone(),
                        graph.replies.entry(pair[0].phrase.clone()).or_default(),
                    ),
                    (Who::You, Who::Them) => push(
                        (scenario.character.clone(), pair[1].phrase.clone()),
                        graph.answers.entry(pair[0].phrase.clone()).or_default(),
                    ),
                    (Who::Them, Who::Them) => push(
                        pair[1].phrase.clone(),
                        graph.then.entry(pair[0].phrase.clone()).or_default(),
                    ),
                    (Who::You, Who::You) => push(
                        pair[1].phrase.clone(),
                        graph.you_then.entry(pair[0].phrase.clone()).or_default(),
                    ),
                }
            }
        }
        graph
    }

    /// The last of the lines they say straight on from `their_line`.
    fn run_end<'a>(&'a self, mut their_line: &'a str) -> &'a str {
        for _ in 0..8 {
            match self.then.get(their_line).and_then(|next| next.first()) {
                Some(next) => their_line = next,
                None => break,
            }
        }
        their_line
    }

    /// Your lines that answer `their_line` in some conversation.
    pub fn replies_to(&self, their_line: &str) -> &[String] {
        self.replies.get(their_line).map_or(&[], Vec::as_slice)
    }
}

fn push<T: PartialEq>(item: T, list: &mut Vec<T>) {
    if !list.contains(&item) {
        list.push(item);
    }
}

/// How what you said was understood.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heard {
    /// You said the line: typed it right.
    Said,
    /// You nearly said it: a letter or two out.
    Nearly,
    /// You meant it: in English, or in other words.
    Meant,
    /// You picked it from the help.
    Picked,
    /// They didn't catch it.
    Missed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub who: Who,
    /// The curriculum line: theirs, or the one you said or could have
    /// said. None when they didn't catch what you said.
    pub phrase: Option<String>,
    /// What you typed, as you typed it. Never saved or logged.
    pub typed: Option<String>,
    /// For your lines.
    pub heard: Option<Heard>,
}

/// One chat. Held in memory while it lasts; only progress is saved.
#[derive(Clone, Debug)]
pub struct Chat {
    pub character: String,
    pub lines: Vec<Line>,
    /// Their lines already said, so they don't repeat themselves.
    said: BTreeSet<String>,
    seed: u64,
    /// They have nothing left to talk about.
    pub ended: bool,
}

impl Chat {
    /// Starts a chat: they open with how one of their conversations
    /// starts. None for a character who isn't in the course.
    pub fn start(character: &str, seed: u64) -> Option<Chat> {
        Content::get().character(character)?;
        let mut chat = Chat {
            character: character.to_string(),
            lines: Vec::new(),
            said: BTreeSet::new(),
            seed,
            ended: false,
        };
        chat.change_subject();
        Some(chat)
    }

    /// Their last line.
    pub fn last_theirs(&self) -> Option<&str> {
        self.lines
            .iter()
            .rev()
            .find(|l| l.who == Who::Them)
            .and_then(|l| l.phrase.as_deref())
    }

    /// Your lines that would fit now: the help.
    pub fn suggestions(&self) -> Vec<String> {
        let graph = Graph::get();
        if self.ended {
            return Vec::new();
        }
        // Half way through what you were saying: the rest of it, even after
        // a try they didn't catch. Only since their last line.
        let yours = self
            .lines
            .iter()
            .rev()
            .take_while(|l| l.who == Who::You)
            .find_map(|l| l.phrase.as_ref());
        if let Some(rest) = yours.and_then(|id| graph.you_then.get(id)) {
            return rest.clone();
        }
        match self.last_theirs() {
            Some(theirs) => graph.replies_to(theirs).to_vec(),
            None => Vec::new(),
        }
    }

    /// What the model chooses from: the lines that fit first, then the
    /// rest of your lines.
    pub fn candidates(&self) -> Vec<String> {
        let mut lines = self.suggestions();
        for id in &Graph::get().yours {
            if lines.len() >= MAX_CANDIDATES {
                break;
            }
            push(id.clone(), &mut lines);
        }
        lines
    }

    /// Understands what you typed (module docs), asking `model` only when
    /// the words aren't one of your lines. `check` is the learner's answer
    /// check (their name filled in). Returns the line and how it was heard,
    /// with a note when the model couldn't help.
    pub fn understand(
        &self,
        typed: &str,
        check: impl Fn(&PhraseText, &str) -> Verdict,
        model: Option<&dyn LanguageModel>,
        stopped: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Understood {
        let content = Content::get();
        let fits = self.suggestions();
        let candidates = self.candidates();
        let phrase = |id: &String| content.phrase(id).expect("checked when read");
        // 1. Said: a line that fits, exactly or nearly; or any line, exactly.
        let mut nearly = None;
        for id in &candidates {
            match check(phrase(id), typed) {
                Verdict::Match => return Understood::heard(id, Heard::Said),
                Verdict::Close if nearly.is_none() && fits.contains(id) => nearly = Some(id),
                _ => {}
            }
        }
        if let Some(id) = nearly {
            return Understood::heard(id, Heard::Nearly);
        }
        if let Some(id) = candidates
            .iter()
            .find(|id| same_english(&phrase(id).english, typed))
        {
            return Understood::heard(id, Heard::Meant);
        }
        // 2. Meant: the model picks a number.
        let Some(model) = model else {
            return Understood::missed(None);
        };
        let info = model.describe();
        if !info.available {
            return Understood::missed(Some(info.unavailable_reason.unwrap_or_else(|| {
                format!("{} is off, so only your lines are understood.", info.name)
            })));
        }
        let them = self.last_theirs().and_then(|id| content.phrase(id));
        let request = meaning_request(self.name(), them, typed, &candidates, phrase);
        match ask(model, request, stopped) {
            Ok(answer) => match pick(&answer, candidates.len()) {
                Some(n) => Understood::heard(&candidates[n], Heard::Meant),
                None => Understood::missed(None),
            },
            Err(AiError::Stopped) => Understood::missed(None),
            Err(err) => {
                forge_runtime::logging::warn!("chat: the model gave no answer ({err})");
                Understood::missed(Some(
                    "The phone's model couldn't help with that one.".into(),
                ))
            }
        }
    }

    /// Takes your turn as `understood`, and they answer. Returns their
    /// line, if they said one.
    pub fn answer(&mut self, typed: Option<String>, understood: &Understood) -> Option<String> {
        self.lines.push(Line {
            who: Who::You,
            phrase: understood.phrase.clone(),
            typed,
            heard: Some(understood.heard),
        });
        let yours = understood.phrase.as_deref()?;
        let graph = Graph::get();
        let next: Vec<&(String, String)> = graph
            .answers
            .get(yours)
            .into_iter()
            .flatten()
            .filter(|(_, id)| self.can_say(id))
            .collect();
        let reply = match self.closest(&next) {
            Some(id) => {
                self.say(id.clone());
                Some(id)
            }
            // You have more to say ("Main theek hoon." ... "Tum sunao?").
            None if graph.you_then.contains_key(yours) => return None,
            None => self.change_subject(),
        };
        // A line that ends a conversation: they carry on with something new
        // rather than leave you nothing to answer.
        if reply.is_some() && self.suggestions().is_empty() {
            self.change_subject();
        }
        reply
    }

    /// They start something new: how one of their own conversations
    /// starts, else one of their circle's. Ends the chat when there is
    /// nothing left.
    fn change_subject(&mut self) -> Option<String> {
        let graph = Graph::get();
        let fresh: Vec<&(String, String)> = graph
            .openers
            .iter()
            .filter(|(_, id)| self.can_say(id) && !graph.replies_to(graph.run_end(id)).is_empty())
            .collect();
        match self.closest(&fresh) {
            Some(id) => {
                self.say(id.clone());
                Some(id)
            }
            None => {
                self.ended = true;
                None
            }
        }
    }

    /// Chooses among (character, line) pairs: their own lines first, then
    /// lines of someone in their circle, never anyone else's.
    fn closest(&self, lines: &[&(String, String)]) -> Option<String> {
        let content = Content::get();
        let circle = |id: &str| content.character(id).and_then(|c| c.circle.as_deref());
        let mine = circle(&self.character);
        let own: Vec<&String> = lines
            .iter()
            .filter(|(c, _)| *c == self.character)
            .map(|(_, id)| id)
            .collect();
        let pool = if own.is_empty() {
            lines
                .iter()
                .filter(|(c, _)| mine.is_some() && circle(c) == mine)
                .map(|(_, id)| id)
                .collect()
        } else {
            own
        };
        self.choose(&pool).cloned()
    }

    /// Whether they can say this line now: not said yet in this chat, and
    /// in their form (a woman never says a line written as a man's).
    fn can_say(&self, id: &str) -> bool {
        if self.said.contains(id) {
            return false;
        }
        let content = Content::get();
        let female = content
            .character(&self.character)
            .is_some_and(|c| c.gender == Gender::Female);
        content
            .phrase(id)
            .is_some_and(|p| !(female && p.speaker == SpeakerForm::Male))
    }

    fn choose<'a>(&self, pool: &[&'a String]) -> Option<&'a String> {
        if pool.is_empty() {
            return None;
        }
        let n = mix(self.seed ^ (self.lines.len() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        Some(pool[(n % pool.len() as u64) as usize])
    }

    /// Says a line, and the lines they say straight on from it.
    fn say(&mut self, id: String) {
        let graph = Graph::get();
        let mut next = Some(id);
        while let Some(id) = next.take() {
            next = graph
                .then
                .get(&id)
                .and_then(|after| after.iter().find(|a| *a != &id && self.can_say(a)))
                .cloned();
            self.said.insert(id.clone());
            self.lines.push(Line {
                who: Who::Them,
                phrase: Some(id),
                typed: None,
                heard: None,
            });
        }
    }

    fn name(&self) -> &str {
        Content::get()
            .character(&self.character)
            .map_or("They", |c| c.name.as_str())
    }
}

/// What you said, as understood.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Understood {
    pub phrase: Option<String>,
    pub heard: Heard,
    /// Why the model didn't help, in words for the screen.
    pub note: Option<String>,
}

impl Understood {
    fn heard(id: &str, heard: Heard) -> Self {
        Self {
            phrase: Some(id.to_string()),
            heard,
            note: None,
        }
    }

    fn missed(note: Option<String>) -> Self {
        Self {
            phrase: None,
            heard: Heard::Missed,
            note,
        }
    }

    pub fn picked(id: &str) -> Self {
        Self::heard(id, Heard::Picked)
    }
}

/// The question for the model: which numbered line did the learner mean?
/// It answers with a number; it is never asked to write Urdu.
pub fn meaning_request<'a>(
    name: &str,
    them: Option<&PhraseText>,
    typed: &str,
    candidates: &[String],
    phrase: impl Fn(&String) -> &'a PhraseText,
) -> ChatRequest {
    let instructions = "You help someone learning Pakistani Urdu, who is chatting with a friend. \
         The learner writes Urdu in English letters, or English, or a mix. You are given what \
         they wrote and a numbered list of lines. Answer with the number of the line that means \
         the same as what they wrote, as a reply in this conversation. If none of the lines \
         means that, answer 0. Answer with the number only.";
    let mut prompt = String::new();
    if let Some(them) = them {
        prompt.push_str(&format!(
            "{name} said: \"{}\" ({})\n\n",
            them.roman, them.english
        ));
    }
    prompt.push_str(&format!(
        "The learner wrote: \"{}\"\n\nLines:\n",
        typed.trim()
    ));
    for (i, id) in candidates.iter().enumerate() {
        let p = phrase(id);
        prompt.push_str(&format!("{}. {} ({})\n", i + 1, p.roman, p.english));
    }
    prompt.push_str("\nNumber:");
    ChatRequest::new(instructions, prompt, 8).temperature(0.0)
}

/// The line the model picked: the first number in its answer, from 1.
/// None for 0, no number, or one out of range.
pub fn pick(answer: &str, count: usize) -> Option<usize> {
    let digits: String = answer
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    let n: usize = digits.parse().ok()?;
    (1..=count).contains(&n).then(|| n - 1)
}

/// Whether two English sentences are the same words, ignoring case and
/// punctuation ("How are you?" and "how are you").
fn same_english(a: &str, b: &str) -> bool {
    let words = |s: &str| -> Vec<String> {
        s.split(|c: char| !c.is_alphanumeric() && c != '\'')
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let (a, b) = (words(a), words(b));
    !a.is_empty() && a == b
}

/// A cheap, well-spread hash for choosing (SplitMix64).
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Checks a typed answer against a phrase's accepted forms, without a
/// learner's name: for tests.
#[cfg(test)]
fn plain_check(text: &PhraseText, typed: &str) -> Verdict {
    crate::Saved::default().check(text, typed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_services::ai::{ModelInfo, ScriptedModel};

    fn content() -> &'static Content {
        Content::get()
    }

    fn roman(id: &str) -> String {
        content().phrase(id).unwrap().roman.clone()
    }

    fn their_lines() -> BTreeSet<String> {
        content()
            .scenarios
            .iter()
            .flat_map(|s| &s.turns)
            .filter(|t| t.who == Who::Them)
            .map(|t| t.phrase.clone())
            .collect()
    }

    #[test]
    fn every_character_can_start_a_chat() {
        for c in &content().characters {
            let chat = Chat::start(&c.id, 7).unwrap();
            assert_eq!(chat.lines.len(), 1, "{}", c.id);
            assert_eq!(chat.lines[0].who, Who::Them);
            assert!(!chat.ended);
        }
        assert!(Chat::start("nobody", 1).is_none());
    }

    #[test]
    fn saying_a_line_gets_an_answer_from_the_course() {
        let chat = Chat::start("ali", 3).unwrap();
        let fits = chat.suggestions();
        assert!(!fits.is_empty());
        let mut chat = chat;
        let typed = roman(&fits[0]).to_lowercase();
        let understood = chat.understand(&typed, plain_check, None, || false);
        assert_eq!(understood.heard, Heard::Said);
        assert_eq!(understood.phrase.as_deref(), Some(fits[0].as_str()));
        let reply = chat.answer(Some(typed), &understood).unwrap();
        assert!(
            their_lines().contains(&reply),
            "{reply} is not one of their lines"
        );
        assert_eq!(chat.lines.len(), 3);
    }

    /// Lines said straight on stay together: Ayesha says who she is and
    /// asks your name in one turn; after "Main theek hoon" you get to ask
    /// back before Ali answers.
    #[test]
    fn lines_said_straight_on_stay_together() {
        let mut chat = Chat::start("ayesha", 0).unwrap();
        chat.lines.clear();
        chat.said.clear();
        chat.say("assalam-o-alaikum".into());
        chat.answer(None, &Understood::picked("walaikum-assalam"));
        let said: Vec<_> = chat
            .lines
            .iter()
            .filter_map(|l| l.phrase.as_deref())
            .collect();
        assert_eq!(
            said,
            [
                "assalam-o-alaikum",
                "walaikum-assalam",
                "main-ayesha-hoon",
                "aap-ka-naam-kya-hai"
            ]
        );
        assert_eq!(chat.suggestions(), ["mera-naam-hai"]);
        let mut chat = Chat::start("ali", 0).unwrap();
        chat.lines.clear();
        chat.said.clear();
        chat.say("kya-haal-hai".into());
        assert_eq!(
            chat.lines
                .iter()
                .filter_map(|l| l.phrase.as_deref())
                .collect::<Vec<_>>(),
            ["kya-haal-hai", "bohat-din-ho-gaye"]
        );
        assert!(chat.suggestions().contains(&"main-theek-hoon".to_string()));
        assert_eq!(
            chat.answer(None, &Understood::picked("main-theek-hoon")),
            None
        );
        assert_eq!(chat.suggestions(), ["tum-sunao"]);
        chat.answer(Some("qwzx".into()), &Understood::missed(None));
        assert_eq!(chat.suggestions(), ["tum-sunao"], "still, after a miss");
        assert!(chat
            .answer(None, &Understood::picked("tum-sunao"))
            .is_some());
    }

    #[test]
    fn nearly_right_counts_only_for_a_line_that_fits() {
        let chat = Chat::start("ali", 3).unwrap();
        let fit = chat.suggestions()[0].clone();
        let mut typed = roman(&fit);
        typed.pop();
        typed.push('x');
        let understood = chat.understand(&typed, plain_check, None, || false);
        assert_eq!(understood.heard, Heard::Nearly);
        assert_eq!(understood.phrase, Some(fit));
    }

    #[test]
    fn english_is_understood_by_the_model_picking_a_number() {
        let chat = Chat::start("ali", 3).unwrap();
        let candidates = chat.candidates();
        assert!(candidates.len() <= MAX_CANDIDATES);
        let model = ScriptedModel::new(["2"]);
        let understood = chat.understand("im good thanks bro", plain_check, Some(&model), || false);
        assert_eq!(understood.heard, Heard::Meant);
        assert_eq!(understood.phrase.as_ref(), Some(&candidates[1]));
        let asked = &model.requests()[0];
        let prompt = asked.prompt().unwrap();
        assert!(prompt.contains("im good thanks bro"));
        assert!(prompt.contains(&format!("2. {}", roman(&candidates[1]))));
        assert!(asked.instructions().contains("number only"));
        assert_eq!(asked.temperature, 0.0);
    }

    #[test]
    fn typing_the_english_exactly_needs_no_model() {
        let chat = Chat::start("ali", 3).unwrap();
        let fit = chat.suggestions()[0].clone();
        let english = content().phrase(&fit).unwrap().english.to_uppercase();
        let understood = chat.understand(&english, plain_check, None, || false);
        assert_eq!(understood.heard, Heard::Meant);
        assert_eq!(understood.phrase, Some(fit));
    }

    #[test]
    fn they_wait_when_nothing_fits() {
        let mut chat = Chat::start("omar", 5).unwrap();
        for answer in ["0", "I have no idea", "999"] {
            let model = ScriptedModel::new([answer]);
            let understood = chat.understand("blah blah", plain_check, Some(&model), || false);
            assert_eq!(understood, Understood::missed(None), "{answer}");
        }
        let understood = chat.understand("blah blah", plain_check, None, || false);
        assert_eq!(chat.answer(Some("blah blah".into()), &understood), None);
        assert_eq!(chat.lines.len(), 2);
        assert!(
            !chat.suggestions().is_empty(),
            "the help still offers lines"
        );
    }

    #[test]
    fn a_model_that_cannot_help_says_why() {
        let chat = Chat::start("ali", 3).unwrap();
        let off = ScriptedModel::new(["1"]).info(ModelInfo {
            id: "off".into(),
            name: "Apple Intelligence".into(),
            available: false,
            unavailable_reason: Some("Turn on Apple Intelligence.".into()),
            context_tokens: 4096,
        });
        let understood = chat.understand("hello", plain_check, Some(&off), || false);
        assert_eq!(understood.heard, Heard::Missed);
        assert_eq!(
            understood.note.as_deref(),
            Some("Turn on Apple Intelligence.")
        );
        assert!(off.requests().is_empty());
        let refusing = ScriptedModel::with_results([Err(AiError::Refused {
            reason: "no".into(),
        })]);
        let understood = chat.understand("hello", plain_check, Some(&refusing), || false);
        assert_eq!(understood.heard, Heard::Missed);
        assert!(understood.note.is_some());
    }

    #[test]
    fn the_model_picks_only_a_number_in_range() {
        assert_eq!(pick("3", 5), Some(2));
        assert_eq!(pick(" 12.", 20), Some(11));
        assert_eq!(pick("Line 4", 5), Some(3));
        assert_eq!(pick("0", 5), None);
        assert_eq!(pick("6", 5), None);
        assert_eq!(pick("none", 5), None);
    }

    /// Long chats with everyone, always taking the help: they only ever say
    /// lines from the course, never repeat one, a woman never says a man's
    /// form, and the chat ends rather than loops.
    #[test]
    fn long_chats_stay_inside_the_course() {
        let theirs = their_lines();
        for c in &content().characters {
            for seed in 0..5 {
                let mut chat = Chat::start(&c.id, seed).unwrap();
                for turn in 0..400 {
                    if chat.ended {
                        break;
                    }
                    let fits = chat.suggestions();
                    let understood = match fits.get(turn % fits.len().max(1)) {
                        Some(id) => Understood::picked(id),
                        None => Understood::missed(None),
                    };
                    assert!(
                        understood.phrase.is_some(),
                        "{} left nothing to answer",
                        c.id
                    );
                    chat.answer(None, &understood);
                }
                assert!(chat.ended, "{} never ran out of things to say", c.id);
                let circle = |id: &str| content().character(id).unwrap().circle.clone();
                let allowed: BTreeSet<&String> = content()
                    .scenarios
                    .iter()
                    .filter(|s| s.character == c.id || circle(&s.character) == c.circle)
                    .flat_map(|s| &s.turns)
                    .filter(|t| t.who == Who::Them)
                    .map(|t| &t.phrase)
                    .collect();
                let said: Vec<&String> = chat
                    .lines
                    .iter()
                    .filter(|l| l.who == Who::Them)
                    .filter_map(|l| l.phrase.as_ref())
                    .collect();
                assert!(said.len() > 5, "{} said only {} lines", c.id, said.len());
                let unique: BTreeSet<_> = said.iter().collect();
                assert_eq!(unique.len(), said.len(), "{} repeated a line", c.id);
                for id in said {
                    assert!(theirs.contains(id), "{id} is not one of their lines");
                    assert!(
                        allowed.contains(id),
                        "{} said {id}, outside their circle",
                        c.id
                    );
                    if c.gender == Gender::Female {
                        assert_ne!(
                            content().phrase(id).unwrap().speaker,
                            SpeakerForm::Male,
                            "{} said {id}, a man's form",
                            c.id
                        );
                    }
                }
            }
        }
    }
}
