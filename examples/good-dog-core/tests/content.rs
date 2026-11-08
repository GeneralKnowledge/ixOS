//! The bundled content parses and hangs together.

use std::collections::HashSet;

use good_dog_core::coaching::WELFARE_CALM_EXERCISE_IDS;
use good_dog_core::dog_training::{bundled_exercises, bundled_learning_objectives};

#[test]
fn bundled_content_parses() {
    assert_eq!(bundled_exercises().len(), 28);
    assert_eq!(bundled_learning_objectives().len(), 18);
}

#[test]
fn every_reference_points_at_something_that_exists() {
    let exercises = bundled_exercises();
    let ids: HashSet<&str> = exercises.iter().map(|e| e.core.id.as_str()).collect();
    assert_eq!(ids.len(), exercises.len(), "exercise ids are unique");
    let objectives: HashSet<&str> = bundled_learning_objectives()
        .iter()
        .map(|o| o.id.as_str())
        .collect();

    // The web app's learning objectives have no entry for these three yet
    // (found by this port, 2026-11-08). Any new gap fails.
    let known_missing = ["down", "housetraining", "puppy-mouthing"];
    for e in exercises {
        let c = &e.core;
        assert!(
            objectives.contains(c.learning_objective_id.as_str())
                || known_missing.contains(&c.learning_objective_id.as_str()),
            "{}: unknown objective {}",
            c.id,
            c.learning_objective_id
        );
        for id in c
            .prerequisite_ids
            .iter()
            .chain(&c.easier_variation_id)
            .chain(&c.harder_variation_id)
        {
            assert!(ids.contains(id.as_str()), "{}: unknown exercise {id}", c.id);
        }
        assert!(!e.life_stages.is_empty(), "{}: life stages", c.id);
        assert!(!e.steps.is_empty(), "{}: steps", c.id);
        assert!(c.estimated_minutes > 0, "{}: minutes", c.id);
    }
    for id in WELFARE_CALM_EXERCISE_IDS {
        assert!(ids.contains(id), "calm exercise {id} is in the library");
    }
}
