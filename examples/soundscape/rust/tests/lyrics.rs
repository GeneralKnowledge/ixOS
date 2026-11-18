//! Lyrics laid out to read along.

use soundscape_core::{lyric_line_at, lyric_lines, lyric_word_at, LyricKind};

fn shown(text: &str) -> Vec<(LyricKind, String)> {
    lyric_lines(text.into())
        .into_iter()
        .map(|l| (l.kind, l.text))
        .collect()
}

#[test]
fn section_tags_become_headings_and_blank_lines_one_gap() {
    use LyricKind::*;
    let text = "\n[verse]\nNeon on the water\nCity in my bones\n\n\n[pre-chorus]\nHold on\n[Chorus 2 - big]\nOne more night\n\n";
    assert_eq!(
        shown(text),
        [
            (Section, "Verse".to_string()),
            (Line, "Neon on the water".into()),
            (Line, "City in my bones".into()),
            (Gap, "".into()),
            (Section, "Pre-Chorus".into()),
            (Line, "Hold on".into()),
            (Gap, "".into()),
            (Section, "Chorus 2".into()),
            (Line, "One more night".into()),
        ]
    );
}

#[test]
fn tags_without_words_are_no_lyrics() {
    assert!(lyric_lines("[instrumental]\n\n[outro]".into()).is_empty());
    assert!(lyric_lines("   ".into()).is_empty());
}

#[test]
fn timed_lyrics_are_in_time_order_and_say_which_line_is_sung() {
    use LyricKind::*;
    let text = "[ar:AFTER HOURS]\n[00:12.00] [verse]\n[00:12.50] Neon on the water\n\n\
                [00:16.20]City in my bones\n[00:31.00] [chorus]\n[00:31.40][01:10.00] One more night\n";
    let lines = lyric_lines(text.into());
    let shown: Vec<_> = lines
        .iter()
        .map(|l| (l.kind, l.text.as_str(), l.at_seconds))
        .collect();
    assert_eq!(
        shown,
        [
            (Section, "Verse", Some(12.0)),
            (Line, "Neon on the water", Some(12.5)),
            (Line, "City in my bones", Some(16.2)),
            (Gap, "", None),
            (Section, "Chorus", Some(31.0)),
            (Line, "One more night", Some(31.4)),
            (Line, "One more night", Some(70.0)),
        ]
    );
    assert_eq!(
        lyric_line_at(lines.clone(), 5.0),
        None,
        "before the first line"
    );
    assert_eq!(lyric_line_at(lines.clone(), 12.5), Some(1));
    assert_eq!(
        lyric_line_at(lines.clone(), 30.0),
        Some(2),
        "held through the break"
    );
    assert_eq!(lyric_line_at(lines.clone(), 200.0), Some(6));
    // plain lyrics have no line to follow
    assert_eq!(
        lyric_line_at(lyric_lines("[verse]\nhello".into()), 10.0),
        None
    );
}

#[test]
fn enhanced_lrc_times_each_word() {
    let lines = lyric_lines(
        "[00:12.30] <00:12.30>One <00:12.61>more <00:12.90>night<00:14.00>\n[00:15.00] plain line"
            .into(),
    );
    let first = &lines[0];
    assert_eq!(first.text, "One more night");
    let words: Vec<_> = first
        .words
        .iter()
        .map(|w| (w.text.as_str(), w.at_seconds))
        .collect();
    assert_eq!(words, [("One ", 12.3), ("more ", 12.61), ("night", 12.9)]);
    assert_eq!(lyric_word_at(first.clone(), 12.0), None);
    assert_eq!(lyric_word_at(first.clone(), 12.7), Some(1));
    assert_eq!(lyric_word_at(first.clone(), 13.5), Some(2));
    assert_eq!(lines[1].text, "plain line");
    assert!(lines[1].words.is_empty());
    assert_eq!(lyric_word_at(lines[1].clone(), 16.0), None);
}
