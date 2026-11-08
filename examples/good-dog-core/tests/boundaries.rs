//! The engine stays domain-neutral, as the web app's module-boundaries test requires.

use std::fs;
use std::path::Path;

#[test]
fn the_engine_uses_no_dog_training_vocabulary_or_imports() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/coaching");
    let banned = [
        "dog", "dogs", "puppy", "puppies", "breed", "canine", "leash",
    ];
    let mut files = 0;
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        files += 1;
        assert!(
            !text.contains("dog_training"),
            "{} imports the domain",
            path.display()
        );
        for word in text
            .split(|c: char| !c.is_ascii_alphanumeric())
            .map(str::to_ascii_lowercase)
        {
            assert!(
                !banned.contains(&word.as_str()),
                "{} mentions {word:?}",
                path.display()
            );
        }
    }
    assert!(files > 0);
}
