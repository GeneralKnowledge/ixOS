# Saved data from every schema version

One `checklist-v<N>.json` per version of `checklist.json` the app has
shipped, exactly as that version wrote it. `tests/core.rs`
(`every_saved_version_still_opens`) opens each one with today's code.

When `SAVE_VERSION` goes up, add the new version's file here before the
release, and never edit an old one: it stands for data already on people's
phones. See `docs/persistence.md`.
