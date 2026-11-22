use super::*;
use forge_runtime::block_on;
use schedule::DAY;

const MINUTE: i64 = 60_000;

fn open(dir: &tempfile::TempDir) -> Arc<UrduCore> {
    UrduCore::new(dir.path().to_string_lossy().into_owned()).unwrap()
}

fn yours(scenario: &str) -> Vec<String> {
    Content::get()
        .scenario(scenario)
        .unwrap()
        .turns
        .iter()
        .filter(|t| t.who == Who::You)
        .map(|t| t.phrase.clone())
        .collect()
}

#[test]
fn a_beginner_is_sent_to_the_first_conversation() {
    let saved = Saved::default();
    let home = saved.home(0, 0);
    assert_eq!(
        home.suggestion,
        Suggestion::Conversation {
            scenario_id: "meeting-someone-new".into()
        }
    );
    assert_eq!(home.totals.met, 0);
    assert!(home.scenarios.len() >= 14);
    assert!(home.scenarios.iter().all(|s| s.new_lines == s.your_lines));
}

#[test]
fn finishing_a_conversation_moves_on_and_brings_misses_back() {
    let mut saved = Saved::default();
    let lines = yours("meeting-someone-new");
    for (i, id) in lines.iter().enumerate() {
        let outcome = if i == 0 {
            Outcome::Missed
        } else {
            Outcome::Independent
        };
        saved
            .attempt(id, Skill::Speaking, outcome, None, 0)
            .unwrap();
    }
    let home = saved.home(10 * MINUTE, 0);
    assert_eq!(
        home.suggestion,
        Suggestion::Conversation {
            scenario_id: "catching-up".into()
        }
    );
    assert_eq!(home.totals.due, 1);
    assert_eq!(home.totals.attempts_today, lines.len() as u32);
    let review = saved.review(10, 10 * MINUTE);
    assert_eq!(review.len(), 1);
    assert_eq!(review[0].phrase.id, lines[0]);
    assert_eq!(review[0].skill, Skill::Recognition);
    // Four days on, everything said on your own is due too.
    assert_eq!(saved.review(20, 4 * DAY).len(), lines.len());
}

#[test]
fn english_fades_as_you_understand_and_can_always_come_back() {
    let mut saved = Saved::default();
    let text = Content::get().phrase("kya-haal-hai").unwrap();
    assert_eq!(
        saved.phrase(text, 0).english_display,
        EnglishDisplay::Visible
    );
    saved
        .attempt(
            "kya-haal-hai",
            Skill::Recognition,
            Outcome::Independent,
            None,
            0,
        )
        .unwrap();
    assert_eq!(
        saved.phrase(text, 0).english_display,
        EnglishDisplay::Secondary
    );
    saved
        .attempt(
            "kya-haal-hai",
            Skill::Listening,
            Outcome::Confident,
            None,
            0,
        )
        .unwrap();
    saved
        .attempt(
            "kya-haal-hai",
            Skill::Listening,
            Outcome::Independent,
            None,
            0,
        )
        .unwrap();
    assert_eq!(
        saved.phrase(text, 0).english_display,
        EnglishDisplay::Hidden
    );
    saved.settings.english = EnglishSupport::Always;
    assert_eq!(
        saved.phrase(text, 0).english_display,
        EnglishDisplay::Visible
    );
}

#[test]
fn stages_need_the_right_skill() {
    let mut saved = Saved::default();
    let text = Content::get().phrase("main-theek-hoon").unwrap();
    assert_eq!(saved.phrase(text, 0).stage, Stage::New);
    saved.seen(&["main-theek-hoon".into()], 0).unwrap();
    assert_eq!(saved.phrase(text, 0).stage, Stage::Learning);
    saved
        .attempt(
            "main-theek-hoon",
            Skill::Recognition,
            Outcome::Confident,
            None,
            0,
        )
        .unwrap();
    assert_eq!(saved.phrase(text, 0).stage, Stage::Recognised);
    saved
        .attempt(
            "main-theek-hoon",
            Skill::Speaking,
            Outcome::Confident,
            None,
            0,
        )
        .unwrap();
    assert_eq!(
        saved.phrase(text, 0).stage,
        Stage::Recognised,
        "speaking 2 is not yet 'can use'"
    );
    saved
        .attempt(
            "main-theek-hoon",
            Skill::Speaking,
            Outcome::Independent,
            None,
            0,
        )
        .unwrap();
    assert_eq!(saved.phrase(text, 0).stage, Stage::CanUse);
}

#[test]
fn your_name_fills_the_introduction() {
    let mut saved = Saved::default();
    let text = Content::get().phrase("mera-naam-hai").unwrap();
    assert_eq!(saved.phrase(text, 0).roman, "Mera naam … hai.");
    saved.settings.name = "Sam".into();
    let phrase = saved.phrase(text, 0);
    assert_eq!(phrase.roman, "Mera naam Sam hai.");
    assert!(phrase.speech.contains("Sam"));
    assert_eq!(saved.check(text, "mera naam sam hai"), Verdict::Match);
}

#[test]
fn typed_answers_accept_spellings_and_male_forms_only() {
    let saved = Saved::default();
    let content = Content::get();
    let dunno = content.phrase("mujhe-nahi-pata").unwrap();
    assert_eq!(saved.check(dunno, "mujhe nahi pata"), Verdict::Match);
    let working = content.phrase("main-kaam-kar-raha-hoon").unwrap();
    assert_eq!(
        saved.check(working, "main kaam kar raha hoon"),
        Verdict::Match
    );
    // The female form is understood, but it isn't how the learner says it.
    assert_ne!(
        saved.check(working, "main kaam kar rahi hoon"),
        Verdict::Match
    );
}

#[test]
fn listening_rounds_mix_voices_and_have_one_right_answer() {
    let saved = Saved::default();
    let round = saved.listening(6, 42, 0);
    assert_eq!(round.len(), 6);
    assert!(round.iter().any(|i| i.voice == Gender::Female));
    assert!(round.iter().any(|i| i.voice == Gender::Male));
    for item in &round {
        assert_eq!(item.choices.len(), 3);
        assert_eq!(item.choices[item.answer as usize], item.phrase.english);
        let mut unique = item.choices.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 3);
    }
    assert_eq!(round, saved.listening(6, 42, 0), "same seed, same round");
}

#[test]
fn search_finds_english_roman_and_other_spellings() {
    let saved = Saved::default();
    let ids =
        |q: &str| -> Vec<String> { saved.search(None, q, 0).into_iter().map(|p| p.id).collect() };
    assert!(ids("congratulations").contains(&"mubarak-ho".to_string()));
    assert!(ids("KYA HAAL").contains(&"kya-haal-hai".to_string()));
    assert!(ids("pata nahee").contains(&"pata-nahi".to_string()));
    assert_eq!(saved.search(Some("glue"), "", 0).len(), 11);
    assert_eq!(
        saved.search(None, "", 0).len(),
        Content::get().phrases.len()
    );
}

#[test]
fn progress_and_settings_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    block_on(core.set_name("Sam".into())).unwrap();
    block_on(core.set_english(EnglishSupport::Minimal)).unwrap();
    let phrase = block_on(core.record(
        "main-theek-hoon".into(),
        Skill::Speaking,
        Outcome::Independent,
        Some(1200),
    ))
    .unwrap();
    assert_eq!(phrase.skills.speaking, 1);
    core.shutdown();
    drop(core);

    let reopened = open(&dir);
    assert_eq!(reopened.settings().name, "Sam");
    assert_eq!(reopened.settings().english, EnglishSupport::Minimal);
    let phrase = reopened.phrase("main-theek-hoon".into()).unwrap();
    assert_eq!(phrase.skills.speaking, 1);
    block_on(reopened.start_over()).unwrap();
    assert_eq!(reopened.totals(0).met, 0);
    assert_eq!(reopened.settings().name, "Sam");
}

#[test]
fn bad_input_is_refused_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    assert_eq!(
        block_on(core.record("nope".into(), Skill::Recall, Outcome::Confident, None)),
        Err(UrduError::NotFound { id: "nope".into() })
    );
    assert_eq!(
        block_on(core.set_name("x".repeat(41))),
        Err(UrduError::NameTooLong { max: 40 })
    );
    assert_eq!(
        block_on(core.seen(vec!["main-theek-hoon".into(), "nope".into()])),
        Err(UrduError::NotFound { id: "nope".into() })
    );
    assert_eq!(core.phrase("main-theek-hoon".into()).unwrap().times_seen, 0);
}

#[test]
fn one_core_per_folder() {
    let dir = tempfile::tempdir().unwrap();
    let _core = open(&dir);
    assert_eq!(
        UrduCore::new(dir.path().to_string_lossy().into_owned()).err(),
        Some(UrduError::InUse)
    );
}

#[test]
fn building_blocks_come_first_with_their_three_ways_to_practise() {
    let saved = Saved::default();
    let topics = saved.topics();
    assert_eq!(topics[0].id, "questions");
    assert_eq!(topics[0].level, Level::Foundation);
    assert!(topics.iter().any(|t| t.level == Level::Conversation));

    let kyun = saved.search(Some("questions"), "", 0);
    assert!(
        kyun.iter().any(|p| p.id == "kyun"),
        "a conversation phrase can belong to a foundation topic too"
    );

    let bhi = saved.phrase(Content::get().phrase("bhi").unwrap(), 0);
    assert_eq!(bhi.kind, EntryKind::Word);
    assert!(bhi.combos.iter().any(|c| c.roman == "Main bhi"));
    let cloze = bhi.cloze.unwrap();
    assert_eq!(cloze.before, "Tum ");
    assert_eq!(cloze.after, " aao.");

    let not_going = saved.phrase(Content::get().phrase("main-nahi-ja-raha").unwrap(), 0);
    assert_eq!(not_going.opposite.unwrap().roman, "Main ja raha hoon.");
}

#[test]
fn a_word_with_a_gap_line_is_reviewed_in_use() {
    let mut saved = Saved::default();
    for skill in [Skill::Recognition, Skill::Recall] {
        saved
            .attempt("bhi", skill, Outcome::Confident, None, 0)
            .unwrap();
    }
    let item = saved.review(1, 30 * DAY).remove(0);
    assert_eq!(item.phrase.id, "bhi");
    assert_eq!(item.skill, Skill::Speaking);
}

#[test]
fn building_blocks_are_words_expressions_and_patterns() {
    let saved = Saved::default();
    let kind = |id: &str| saved.phrase(Content::get().phrase(id).unwrap(), 0).kind;
    assert_eq!(kind("nahi"), EntryKind::Word);
    assert_eq!(
        kind("woh-log"),
        EntryKind::Word,
        "a pair can be declared one word"
    );
    assert_eq!(kind("mujhe-nahi-lagta"), EntryKind::Expression);
    assert_eq!(kind("pattern-can"), EntryKind::Pattern);
    assert_eq!(kind("kya-haal-hai"), EntryKind::Phrase);

    let totals = saved.totals(0, 0);
    assert!(totals.words >= 120 && totals.expressions >= 30 && totals.patterns >= 8);
}

#[test]
fn a_pattern_is_practised_by_building_new_sentences() {
    let mut saved = Saved::default();
    for skill in [Skill::Recognition, Skill::Recall] {
        saved
            .attempt("pattern-can", skill, Outcome::Confident, None, 0)
            .unwrap();
    }
    let first = saved.review(1, 30 * DAY).remove(0);
    assert_eq!(first.skill, Skill::Speaking);
    let example = first.example.expect("a sentence to build");
    let pattern = Content::get().phrase("pattern-can").unwrap();
    assert!(pattern.combos.iter().any(|c| c.roman == example.roman));
    assert!(example.speech.is_some());

    // Get it right and the next time round asks for another sentence.
    saved
        .attempt(
            "pattern-can",
            Skill::Speaking,
            Outcome::Independent,
            None,
            30 * DAY,
        )
        .unwrap();
    let next = saved.review(1, 90 * DAY).remove(0);
    if next.skill == Skill::Speaking {
        assert_ne!(next.example.unwrap().roman, example.roman);
    }
}

#[test]
fn listening_never_plays_a_pattern() {
    let saved = Saved::default();
    let content = Content::get();
    for seed in 0..20 {
        for item in saved.listening(8, seed, 0) {
            assert_ne!(item.phrase.kind, EntryKind::Pattern);
            assert!(item.choices.iter().all(|c| !c.contains(BLANK)));
        }
    }
    assert!(content.phrases.iter().any(|p| p.kind == EntryKind::Pattern));
}

#[test]
fn each_conversation_teaches_the_words_in_it() {
    let saved = Saved::default();
    let content = Content::get();
    let tea = saved.scenario(content.scenario("tea-at-khalas").unwrap(), 0);
    let ids: Vec<&str> = tea.words.iter().map(|w| w.id.as_str()).collect();
    assert!(ids.contains(&"nahi-shukriya"), "{ids:?}");
    assert!(ids.contains(&"zaroor"), "{ids:?}");
    // "Raste mein hoon" is "in", not "I": no spelling is merged here.
    let late = saved.scenario(content.scenario("on-my-way").unwrap(), 0);
    assert!(late.words.iter().all(|w| w.id != "main"));
    // Patterns are found by their frame.
    let busy = saved.scenario(content.scenario("what-are-you-up-to").unwrap(), 0);
    assert!(busy.words.iter().any(|w| w.id == "pattern-doing"));
    // Every conversation teaches something.
    for scenario in &content.scenarios {
        assert!(!content.words_in(scenario).is_empty(), "{}", scenario.id);
    }
}

#[test]
fn what_came_up_in_a_conversation_comes_back_the_next_day() {
    let mut saved = Saved::default();
    let words: Vec<String> = Content::get()
        .words_in(Content::get().scenario("tea-at-khalas").unwrap())
        .iter()
        .map(|w| w.id.clone())
        .collect();
    saved.seen(&words, 0).unwrap();
    assert!(saved.review(50, DAY / 2).is_empty(), "not the same day");
    let back: Vec<String> = saved
        .review(50, DAY)
        .into_iter()
        .map(|i| i.phrase.id)
        .collect();
    assert!(words.iter().all(|w| back.contains(w)));
    // Seeing a line doesn't make its conversation stop counting as new.
    let summary = saved.home(DAY, 0).scenarios;
    assert!(summary.iter().all(|s| s.new_lines == s.your_lines));
}

/// Free chat: saying one of your lines counts as speaking it; a line you
/// only meant, or picked from the help, comes back the next day; what you
/// type is never saved.
#[test]
fn a_free_chat_counts_what_you_said() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    assert!(core.chat().is_none());
    let view = block_on(core.chat_start("ali".into(), 11)).unwrap();
    assert_eq!(view.character.name, "Ali");
    assert_eq!(view.lines.len(), 1);
    assert_eq!(view.lines[0].speaker, TurnSpeaker::Character);
    let opener = view.lines[0].phrase.as_ref().unwrap().id.clone();
    assert_eq!(core.saved.get().progress(&opener).times_seen, 1);

    // Say a line that fits, as typed on a phone.
    let line = view.suggestions[0].clone();
    let view = block_on(core.chat_say(format!("  {}  ", line.roman.to_lowercase()), None)).unwrap();
    let yours = &view.lines[1];
    assert_eq!(yours.heard, Some(ChatHeard::Said));
    assert_eq!(yours.phrase.as_ref().unwrap().id, line.id);
    assert!(view.lines.len() >= 3, "Ali answered");
    assert_eq!(view.lines[2].speaker, TurnSpeaker::Character);
    let progress = core.saved.get().progress(&line.id);
    assert!(progress.practised());
    assert!(progress.skills.speaking > 0);

    // Gibberish with no model: Ali waits, and nothing is recorded.
    let before = core.saved.get().events.len();
    let view = block_on(core.chat_say("qwzx".into(), None)).unwrap();
    let last = view.lines.last().unwrap();
    assert_eq!(last.heard, Some(ChatHeard::Missed));
    assert!(last.phrase.is_none());
    assert_eq!(core.saved.get().events.len(), before);

    // Picking from the help: met, not practised.
    let pick = view.suggestions[0].id.clone();
    let view = block_on(core.chat_pick(pick.clone())).unwrap();
    assert!(view
        .lines
        .iter()
        .any(|l| l.heard == Some(ChatHeard::Picked)));
    let progress = core.saved.get().progress(&pick);
    assert!(progress.times_seen > 0 || progress.next_review_ms.is_some());

    // Typed words stay out of the saved file.
    drop(core);
    let file = std::fs::read_to_string(dir.path().join(SAVE_FILE)).unwrap();
    assert!(!file.contains("qwzx"));

    let core = open(&dir);
    assert!(core.chat().is_none(), "a chat isn't saved");
    assert!(block_on(core.chat_say("hi".into(), None)).is_err());
    assert!(block_on(core.chat_start("nobody".into(), 1)).is_err());
    assert!(
        block_on(core.chat_pick("chai-piyoge".into())).is_err(),
        "not your line"
    );
}

/// An answer worked out for one chat never lands in the next one.
#[test]
fn a_late_answer_never_lands_in_a_new_chat() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    let first = block_on(core.chat_start("ali".into(), 1)).unwrap();
    let line = first.suggestions[0].id.clone();
    let serial = lock(&core.chat).as_ref().unwrap().serial;
    let second = block_on(core.chat_start("khala".into(), 2)).unwrap();
    let late = block_on(core.chat_answer(
        Some(serial),
        Some("typed in Ali's chat".into()),
        chat::Understood::picked(&line),
    ))
    .unwrap();
    assert_eq!(late, second, "Khala's chat is unchanged");
    assert!(!core.saved.get().progress(&line).practised());
    core.chat_end();
    let third = block_on(core.chat_start("ali".into(), 1)).unwrap();
    let late =
        block_on(core.chat_answer(Some(serial), None, chat::Understood::picked(&line))).unwrap();
    assert_eq!(late, third, "a new chat with the same person is a new chat");
}

/// A chat line longer than the limit is refused with the limit, never cut
/// short and answered as if you'd said less; the chat is left as it was.
#[test]
fn a_chat_line_over_the_limit_is_refused_not_cut() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    block_on(core.chat_start("ali".into(), 11)).unwrap();
    let max = core.chat_max_typed();
    assert_eq!(max as usize, crate::chat::MAX_TYPED_CHARS);

    let long = "a".repeat(max as usize + 1);
    assert_eq!(
        block_on(core.chat_say(long, None)),
        Err(UrduError::TooLong { max })
    );
    assert_eq!(core.chat().unwrap().lines.len(), 1, "nothing was said");

    // Exactly at the limit (in characters, not bytes, and spaces round it
    // don't count) is taken.
    let at_limit = format!("  {}  ", "ā".repeat(max as usize));
    let view = block_on(core.chat_say(at_limit, None)).unwrap();
    assert_eq!(
        view.lines[1].typed.as_deref().map(|t| t.chars().count()),
        Some(max as usize)
    );
}
