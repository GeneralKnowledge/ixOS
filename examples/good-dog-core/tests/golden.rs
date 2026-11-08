//! The web app's own golden fixture, copied unchanged from
//! `src/lib/domains/dog-training/golden-plans.fixture.json`: every 9th scenario,
//! 100 plans. A failure means the port plans differently from the web app.

mod common;

use std::collections::BTreeMap;

use common::{build_scenarios, plan_for, record, RecordedPlan};

const SAMPLE_EVERY: usize = 9;

#[test]
fn reproduces_the_web_apps_100_golden_plans() {
    let recorded: BTreeMap<String, RecordedPlan> =
        serde_json::from_str(include_str!("fixtures/golden-plans.fixture.json")).unwrap();
    let sampled: Vec<_> = build_scenarios()
        .into_iter()
        .enumerate()
        .filter(|(i, _)| i % SAMPLE_EVERY == 0)
        .map(|(_, s)| s)
        .collect();

    assert_eq!(sampled.len(), 100);
    assert_eq!(recorded.len(), 100);

    let mut mismatches = Vec::new();
    for scenario in &sampled {
        let expected = recorded
            .get(&scenario.key)
            .unwrap_or_else(|| panic!("fixture has no scenario {}", scenario.key));
        let actual = record(&plan_for(scenario));
        if &actual != expected {
            mismatches.push(format!(
                "{}\n  expected {expected:?}\n  actual   {actual:?}",
                scenario.key
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of 100 plans differ:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}
