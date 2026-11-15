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

`pathways.json` holds goal pathways (docs/good-dog-demo.md, "Pathway format"). They are
written in [GeneralKnowledge/DogResearch](https://github.com/GeneralKnowledge/DogResearch)
first (Reuben, 2026-11-08) and exported here. The current file is the `pathways` array of
DogResearch's `dist/good-dog-pathways.json` (`npm run export:good-dog`) from DogResearch PR #9
(branch `claude/goal-pathways`, commit `ad15376`, 2026-11-15): recall, loose lead, settling and
visitors, each stage with its lesson plan (`lessons`). `safety-rules.json` is the `safetyRules` array of the same export: the DogResearch safety rules that goal
stages name, each with `appEnforcement` (how the app applies it), from DogResearch PR #10 (branch
`claude/goal-safety-rules`). Re-export both when the wording changes. `tests/fixtures/pathways-sample.json` is a
test-only sample in the same format. `tests/pathways.rs` checks every shipped pathway
against the exercises.
