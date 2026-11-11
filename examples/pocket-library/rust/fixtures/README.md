# Saved data from every schema version

One `library-v<N>.json` per version of `library.json` the app has
shipped, exactly as that version wrote it (`library-v1.json` came from
`examples/seed_screenshots.rs`). `src/store.rs`
(`every_saved_version_still_opens`) opens each one with today's code.

When `STORE_VERSION` goes up, add the new version's file here before the
release, and never edit an old one: it stands for data already on people's
phones. See `docs/persistence.md`.
