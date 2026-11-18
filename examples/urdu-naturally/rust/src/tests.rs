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
    assert_eq!(home.scenarios.len(), 5);
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
