//! Checking a typed answer. Roman Urdu has no enforced spelling, so the
//! check compares sounds-alike forms, not letters: case, punctuation and
//! common spelling swaps (nahin/nahi, aa/a, ee/i, oo/u, w/v) don't count
//! against you. A spelling slip is "close", never "wrong"; only different
//! words are. Whether you said it well aloud is not this module's call:
//! nothing here pretends to score pronunciation.

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

/// Lower case, letters and spaces only, with spelling variation folded.
pub fn fold(text: &str) -> String {
    let mut words = Vec::new();
    for raw in text.split_whitespace() {
        let word: String = raw
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        if word.is_empty() {
            continue;
        }
        words.push(fold_word(&word));
    }
    words.join(" ")
}

fn fold_word(word: &str) -> String {
    // Whole-word spellings people use interchangeably.
    let word = match word {
        "nahin" | "nai" | "nhi" | "nahee" => "nahi",
        "mein" | "main" | "mai" | "mn" => "main",
        "hy" | "he" | "hay" => "hai",
        "hun" | "hu" => "hoon",
        "ap" => "aap",
        "kia" => "kya",
        "acha" | "achha" | "accha" => "acha",
        other => other,
    };
    let mut out = String::with_capacity(word.len());
    let mut last = None;
    for c in word.chars() {
        let c = match c {
            'v' => 'w',
            'q' => 'k',
            other => other,
        };
        // Doubled letters (aa, ee, oo, kk) count once.
        if Some(c) == last {
            continue;
        }
        out.push(c);
        last = Some(c);
    }
    // "ee"/"i" and "oo"/"u" after doubling folds.
    out.replace("ey", "e").replace("ay", "e")
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
        assert_eq!(check("MUJHE NAHEE PATAA!", forms), Verdict::Match);
        assert_eq!(check("kia haal hy", ["Kya haal hai?"]), Verdict::Match);
        // The phone scenario uitests/typed-answer.toml types this.
        assert_eq!(
            check("walaikum asalaam", ["Walaikum assalam."]),
            Verdict::Match
        );
        assert_eq!(
            check("Main theek hun", ["Main theek hoon."]),
            Verdict::Match
        );
    }

    #[test]
    fn a_slip_is_close_and_other_words_are_different() {
        let forms = ["Kaam kaisa chal raha hai?"];
        assert_eq!(check("kam kesa chal raha hai", forms), Verdict::Close);
        assert_eq!(check("kya haal hai", forms), Verdict::Different);
        assert_eq!(check("", forms), Verdict::Different);
    }

    #[test]
    fn any_accepted_form_matches() {
        let forms = ["Kya haal hai?", "Aap kaise hain?"];
        assert_eq!(check("aap kaise hain", forms), Verdict::Match);
    }
}
