# Where this content comes from

`exercises.json` and `learning-objectives.json` are exported from the Good Dog web app
([GeneralKnowledge/Good-Dog](https://github.com/GeneralKnowledge/Good-Dog)) at commit
`7033d14` (2026-10-16) by `scripts/export-from-web.sh`, which also re-records
`tests/fixtures/`. Do not edit them by hand: change the web app, re-export, and run
`cargo test -p good-dog-core`.

Exercise text uses glossary links written `[[term-id]]` or `[[term-id|shown text]]`.
The glossary, guides, referrals and Ask answers are not exported yet; they come with
the milestones that need them (docs/good-dog-demo.md).

Known gap in the web app's content: exercises `ex-down-comfort`, `ex-toilet-routine`
and `ex-puppy-mouthing` use learning objectives (`down`, `housetraining`,
`puppy-mouthing`) that have no entry in `learning-objectives.json`, so they have no
label. `tests/content.rs` allows exactly these three.
