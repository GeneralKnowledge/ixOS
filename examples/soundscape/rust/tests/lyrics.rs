//! Lyrics laid out to read along.

use soundscape_core::{lyric_lines, LyricKind};

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
