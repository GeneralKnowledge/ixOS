//! Checking a typed answer. Roman Urdu has no enforced spelling, so case,
//! punctuation and the common spellings of the same word (nahi or nahin,
//! hai or hy, kya or kia) don't count against you, and neither does any
//! spelling the curriculum lists as a variant.
//!
//! What it does not do is fold sounds together. Long and short vowels
//! (kaam, work; kam, less), doubled consonants (pata, know; patta, leaf)
//! and q against k (qadam, step) are real differences in Urdu, so an answer
//! that differs only there is "close", never a match: forgiving, without
//! teaching the wrong word. Whether you said it well aloud is not this
//! module's call: nothing here pretends to score pronunciation.

/// The verdict on a typed answer.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The phrase, or an accepted way of saying it.
    Match,
    /// Nearly: a letter or two out, or a word missing.
    Close,
    /// Something else.
    Different,
}

/// Lower case, letters and spaces only, with the common spellings of the
/// same word made one. Used to check answers.
pub fn fold(text: &str) -> String {
    words(text)
        .map(|w| same_word(&w).to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// [`fold`], then sounds-alike letters made one too (aa and a, v and w, q
/// and k). Too loose to mark an answer; right for search, where "kam"
/// should find "kaam".
pub fn fold_loose(text: &str) -> String {
    words(text)
        .map(|w| {
            let mut out = String::with_capacity(w.len());
            let mut last = None;
            for c in same_word(&w).chars() {
                let c = match c {
                    'v' => 'w',
                    'q' => 'k',
                    other => other,
                };
                if Some(c) != last {
                    out.push(c);
                }
                last = Some(c);
            }
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split_whitespace()
        .map(|raw| {
            raw.chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
}

/// Whole-word spellings people use interchangeably for the same word. Each
/// line is one word, never two words that merely sound alike.
fn same_word(word: &str) -> &str {
    match word {
        "nahin" | "nai" | "nhi" | "nahee" | "nahi" => "nahi",
        "mein" | "main" | "mai" | "mn" => "main",
        "hy" | "he" | "hay" | "hai" => "hai",
        "hun" | "hu" | "hoon" => "hoon",
        "hain" | "hen" | "hein" => "hain",
        "ap" | "aap" => "aap",
        "kia" | "kya" => "kya",
        "acha" | "achha" | "accha" | "achchha" => "acha",
        "theek" | "thik" | "theak" => "theek",
        "kaise" | "kese" | "kaisay" | "kesay" => "kaise",
        "walaikum" | "walekum" | "walaikam" => "walaikum",
        "assalam" | "asalam" | "assalaam" | "salam" | "salaam" => "assalam",
        "shukriya" | "shukria" | "shukriyah" => "shukriya",
        "yaar" | "yar" => "yaar",
        other => other,
    }
}

/// Edit distance between two strings, by characters.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = if ca == cb {
                prev
            } else {
                1 + prev.min(row[j]).min(row[j + 1])
            };
            prev = cur;
        }
    }
    row[b.len()]
}

/// Checks `answer` against every accepted form.
pub fn check<'a>(answer: &str, accepted: impl IntoIterator<Item = &'a str>) -> Verdict {
    let answer = fold(answer);
    if answer.is_empty() {
        return Verdict::Different;
    }
    let mut best = Verdict::Different;
    for form in accepted {
        let form = fold(form);
        if form == answer {
            return Verdict::Match;
        }
        // A slip: about one letter in six, at least one.
        let allowed = (form.chars().count() / 6).max(1);
        if distance(&form, &answer) <= allowed {
            best = Verdict::Close;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spelling_and_punctuation_do_not_count() {
        let forms = ["Mujhe nahin pata."];
        assert_eq!(check("mujhe nahi pata", forms), Verdict::Match);
        assert_eq!(check("MUJHE NAHEE PATA!", forms), Verdict::Match);
        assert_eq!(check("kia haal hy", ["Kya haal hai?"]), Verdict::Match);
        // The phone scenario uitests/typed-answer.toml types this.
        assert_eq!(
            check("walekum assalam", ["Walaikum assalam."]),
            Verdict::Match
        );
        assert_eq!(check("Main thik hun", ["Main theek hoon."]), Verdict::Match);
    }

    #[test]
    fn a_slip_is_close_and_other_words_are_different() {
        let forms = ["Kaam kaisa chal raha hai?"];
        assert_eq!(check("kam kesa chal raha hai", forms), Verdict::Close);
        assert_eq!(check("kya haal hai", forms), Verdict::Different);
        assert_eq!(check("", forms), Verdict::Different);
    }

    /// Words that look alike in Roman Urdu but mean different things are
    /// never a match for each other: close at best, so you see the answer.
    #[test]
    fn look_alikes_are_not_the_same_word() {
        for (typed, answer) in [
            ("kam", "Kaam"),      // less / work
            ("patta", "Pata"),    // leaf / know
            ("kal", "Kaal"),      // tomorrow or yesterday / era
            ("kadam", "Qadam"),   // (not a word) / step
            ("bacha", "Bachcha"), // saved / child
            ("sona", "Soona"),    // sleep or gold / empty
            ("pal", "Paal"),      // a moment / raise
        ] {
            assert_ne!(
                check(typed, [answer]),
                Verdict::Match,
                "{typed} vs {answer}"
            );
        }
    }

    #[test]
    fn search_is_looser_than_checking() {
        assert_eq!(fold_loose("Kaam"), fold_loose("kam"));
        assert_ne!(fold("Kaam"), fold("kam"));
        assert_eq!(fold_loose("pata nahee"), fold_loose("Pata nahin"));
    }

    #[test]
    fn any_accepted_form_matches() {
        let forms = ["Kya haal hai?", "Aap kaise hain?"];
        assert_eq!(check("aap kaise hain", forms), Verdict::Match);
    }
}
