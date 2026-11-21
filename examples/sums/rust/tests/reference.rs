//! The reference's recorded answers as the recreation's test
//! (docs/research/app-as-specification.md, R2). Every sequence in
//! evidence/sequences.toml is pressed on the engine, starting from a cleared
//! calculator as the recording did, and its display and expression line are
//! compared with evidence/reference-results.toml. A difference fails the
//! test unless it is listed in KNOWN, with the reason, which also goes in the
//! reconstruction record.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use sums_core::{Calculator, Key};

#[derive(Deserialize)]
struct Sequences {
    sequence: Vec<Sequence>,
}

#[derive(Deserialize)]
struct Sequence {
    name: String,
    keys: Vec<String>,
}

#[derive(Deserialize)]
struct Results {
    result: Vec<Recorded>,
}

#[derive(Deserialize)]
struct Recorded {
    name: String,
    display: String,
    expression: Option<String>,
}

/// Differences we know about and have not matched yet: (sequence, why).
const KNOWN: &[(&str, &str)] = &[];

fn evidence(file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/research/expeditions/calculator/evidence")
        .join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn matches_the_reference() {
    let sequences: Sequences = toml::from_str(&evidence("sequences.toml")).unwrap();
    let results: Results = toml::from_str(&evidence("reference-results.toml")).unwrap();
    let recorded: BTreeMap<_, _> = results
        .result
        .iter()
        .map(|r| (r.name.as_str(), r))
        .collect();
    let known: BTreeMap<_, _> = KNOWN.iter().copied().collect();

    let mut differences = Vec::new();
    let mut matched = 0;
    for seq in &sequences.sequence {
        let reference = recorded
            .get(seq.name.as_str())
            .unwrap_or_else(|| panic!("no recorded answer for {}", seq.name));
        let mut calc = Calculator::new();
        calc.press(Key::Clear);
        calc.press(Key::Clear);
        for id in &seq.keys {
            calc.press(Key::from_id(id).unwrap_or_else(|| panic!("unknown key {id}")));
        }
        let ours = (calc.display(), calc.expression());
        let theirs = (reference.display.clone(), reference.expression.clone());
        if ours == theirs {
            matched += 1;
            assert!(
                !known.contains_key(seq.name.as_str()),
                "{} now matches: take it out of KNOWN",
                seq.name
            );
        } else if !known.contains_key(seq.name.as_str()) {
            differences.push(format!(
                "{}: ours {:?} / {:?}, reference {:?} / {:?}",
                seq.name, ours.0, ours.1, theirs.0, theirs.1
            ));
        }
    }
    println!(
        "{matched} of {} sequences match the reference",
        sequences.sequence.len()
    );
    assert!(differences.is_empty(), "\n{}", differences.join("\n"));
}
