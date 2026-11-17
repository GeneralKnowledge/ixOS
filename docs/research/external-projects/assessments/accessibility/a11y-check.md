# a11y-check: can its rules become an ixOS quality gate on Linux?

Status: 2026-11-17. Part of the [External Projects Atlas](../../README.md), P0 investigation 1. Research only: nothing is added to ixOS by this page.

## Question

Can the static accessibility checker from CVS Health's SwiftUI accessibility techniques project run in ixOS's Linux workflow, and are its findings on ixOS's apps accurate enough to make it a mandatory check next to the existing ones?

## Evidence

| Field | Value |
|---|---|
| Date | 2026-11-17 |
| Inspected | [cvs-health/ios-swiftui-accessibility-techniques](https://github.com/cvs-health/ios-swiftui-accessibility-techniques) at `3aa074c` (main, 2026-11-12), after tag `0.7.0` of the `a11y-check` CLI |
| Licence | Apache-2.0 (`LICENSE`) |
| Activity | 228 commits since 2026-08-17, almost all by one maintainer; about 390 stars. Tags 0.5.2, 0.6.0, 0.7.0. Its own CI runs the tests on macOS only |
| Read or run | Source read; built on Linux; its tests run on Linux; run on every ixOS app's Swift sources. Nothing runs on the iPhone (it is a source checker) |
| Build evidence | In the `swift:6.2` Docker image (Swift 6.2.4, x86_64 Linux, the same image ixOS's CI uses for Linux Swift tests): `swift build -c release --product A11yCheckCLI` completed in 412 s from cold (most of it is swift-syntax). `swift test`: **340 tests, 0 failures** on Linux |
| Platform evidence | Not applicable to the phone. Whether specific findings are real for VoiceOver users was judged from source; three rules need a short VoiceOver check on the phone to settle (below) |
| ixOS baseline | main at `98a8446`: `examples/*/Sources`, `forgekit/Sources`, `templates/` |

## What it does

A command-line tool built on SwiftSyntax: it parses Swift source (no compilation, no SDK) and applies 47 rules mapped to WCAG 2.2, such as unlabelled images and icon buttons, repeated button labels inside `ForEach`, text fields labelled only by placeholder, fixed font sizes, missing `textContentType`, missing heading traits, and focus return after sheets and alerts. It gives a score and grade, and outputs terminal, JSON, SARIF, HTML or Xcode formats. CI features: `--min-score`, `--diff` (only changed lines), `--baseline-save` and `--baseline` (only new issues fail), inline suppressions, a `.a11ycheck.yml` config to disable rules, and `--fix` for some rules.

Dependencies are swift-syntax 510, swift-argument-parser and Yams. The core imports only Foundation, SwiftSyntax and SwiftParser, so nothing in it is Darwin-only. The core makes no network calls; it starts `git` for `--diff` and the score trend. The CLI can also create GitHub issues (`--create-github-issues`), which ixOS would never use. It writes a trend file by default, so CI would pass `--no-trend`.

## Compared with ixOS

ixOS today enforces accessibility identifiers by convention (AGENTS.md rule 4). `forge inspect` lists literal identifiers, `forge ui` saves the accessibility tree beside each screenshot on the phone, and the [ax audit spike](../../../ax-audit.md) reads the screen from Linux, but its audit call isn't working yet. Nothing checks labels or semantics before the phone.

### Results on ixOS (main at `98a8446`)

| Sources | Score | Errors | Warnings | Most frequent rules |
|---|---|---|---|---|
| forgekit (ForgeUI, ForgeAppKit, ForgeKit) | 96 (A) | 3 | 0 | sheet-focus-return, line-limit-1 |
| weather | 88.2 (B+) | 11 | 15 | hardcoded-color, heading-trait-missing |
| good-dog | 79.4 (C+) | 14 | 34 | heading-trait-missing (30) |
| pocket-library | 75.5 (C) | 8 | 15 | heading-trait-missing |
| device-lab | 73.2 (C) | 9 | 5 | heading-trait-missing, duplicate-button-label |
| templates | 73.1 (C) | 9 | 4 | textfield-missing-label |
| fieldkit | 70.4 (C-) | 9 | 6 | heading-trait-missing, duplicate-button-label |
| sketchbook | 66.7 (D) | 13 | 4 | duplicate-button-label |
| commonplace | 66 (D) | 15 | 6 | duplicate-button-label (8) |
| forgeui-showcase | 60.9 (D-) | 20 | 4 | textfield-missing-label (8) |
| relay | 54.7 (F) | 42 | 15 | duplicate-button-label (14) |
| checklist | 53.1 (F) | 11 | 1 | sheet-focus-return |
| soundscape | 40.7 (F) | 51 | 22 | duplicate-button-label (21) |

Across all of them: 83 heading-trait-missing, 72 duplicate-button-label, 44 sheet-focus-return, 38 textfield-missing-label, 35 missing-accessibility-hint, 22 line-limit-1, 13 fixed-font-size, 8 input-missing-purpose, 6 image-missing-label, and a handful of others. Each app ran in about a second.

### How accurate it is

I read a random sample of three findings for each of the eight most frequent rules, and wrote a small fixture to confirm two suspected false positives.

| Rule | Sampled | Judgement |
|---|---|---|
| duplicate-button-label | "Play Voice Message" per attachment (Relay), "Rename" in a layer's context menu (Sketchbook), "Cancel" in a sheet's toolbar (Soundscape) | **Mostly useful.** The Relay one is real: several identical buttons with no context. The context-menu and toolbar ones are weaker, because VoiceOver reads them with their row or sheet |
| image-missing-label | Compass lubber line, attachment icons beside a file name | **Real but minor.** Decorative images should be hidden from VoiceOver |
| input-missing-purpose | Email field, password field, server URL field | **Mixed.** The password one is real. The email and URL ones are **false positives**: `.textContentType` is inside `#if os(iOS)`, which the rule doesn't see through (confirmed with a fixture) |
| fixed-font-size | 10 pt location icon (Weather), 96 pt trophy (Good Dog), artwork initial scaled to its frame (Soundscape) | **Mixed.** The small icon is real (it won't grow with Dynamic Type); the decorative ones are deliberate |
| line-limit-1 | Reply preview, take label, transfer row name | **Real as a warning:** truncation at large text sizes, but often a design choice. Not a gate |
| textfield-missing-label | `TextField("Optional")` inside `ForgeField("Notes")`, `SecureField("Password")`, the starter's `TextField("New item")` | **Needs a phone check.** In SwiftUI the title becomes the field's accessibility label; the rule treats it as a placeholder that disappears. The ForgeField case is a **false positive**: `ForgeField` hides its visible label and puts it on the field with `.accessibilityLabel` (`forgekit/Sources/ForgeUI/Styles/ForgeFieldStyle.swift`), which a source checker can't see through a custom container |
| sheet-focus-return | `.alert` and `.confirmationDialog` in Relay, Checklist and ForgeAppKit | **Too broad.** It fires on every system alert (confirmed with a fixture). Whether VoiceOver focus returns after a SwiftUI alert or sheet is a phone question |
| heading-trait-missing | "Paused" headline (Pocket Library), a row's monospaced name and an explanation line (Device Lab) | **Noisy.** One of three looked like a real heading |

So the strongest rules (labels in repeated rows, unlabelled images and icon buttons, input purpose without `#if`, fixed sizes on text) find real problems ixOS has today, especially in Relay, Soundscape and Commonplace. Several of the most frequent rules are too noisy to fail a build on.

## Scorecard

| Dimension | Rating | Reason |
|---|---|---|
| Capability | Strong | ixOS has nothing that checks labels or semantics before the phone |
| Product value | Adequate | Real VoiceOver problems found in shipped screens; the value depends on choosing rules well |
| Architecture | Strong | A development tool reading source; no app, ForgeKit or Rust code depends on it |
| Platform | Strong | Builds and passes its tests on Linux; needs no SDK and no iPhone |
| Development | Adequate | Fits the existing `swift:6.2` CI image. A cold build is about 7 minutes, so CI would cache the binary or build it once per pinned version |
| Maintenance | Adequate | Very active and tagged, but almost entirely one person; its CI tests only on macOS, so a Linux break could go unnoticed upstream |
| Licence | Strong | Apache-2.0; ixOS would run it, not ship it |
| Security | Strong | No network in the core; the GitHub-issue feature is never used |
| Integration cost | Adequate | A pinned build in CI plus a config and a baseline. Fixing the real findings across 11 apps is the larger work |
| Reusability | Strong | Every app, the starter template and ForgeUI |

## Verdict

**Adopt, as a development tool in CI, not as a dependency of any app.** Run it pinned to a tag, report-only first. Then gate rules one at a time, each only after a fixture shows it handles ixOS's patterns, and only on new findings.

The reason: it is the only way found so far to catch accessibility problems on Linux before the phone, it builds and passes its own tests on Linux, and its best rules found real problems in ixOS's apps. It is not ready to gate on all 47 rules, because several common ones are noisy or blind to `#if`.

Reopen when: its maintenance stops, a Linux build breaks, or Apple's audit (the ax spike) works well enough from Linux to cover the same ground on the phone.

## Next step

One implementation PR, after Reuben agrees. Its first phase is **report-only: nothing fails a build**.

1. **Report only.** Add a pinned build of `a11y-check` (tag `0.7.0` or later) to the Linux CI job that already uses `swift:6.2`, cached, run with `--no-trend --format json`. It writes a report and never fails. Add a repository `.a11ycheck.yml` that turns off sheet-focus-return and heading-trait-missing until a VoiceOver check on the phone shows whether they're real.
2. **A fixture per rule before it can gate.** For each rule that is a candidate for gating, add a small fixture to ixOS: one real problem the rule must flag, and the ixOS patterns it must not flag. Known false positives a fixture must cover:
   - **input-missing-purpose:** `.textContentType` inside `#if os(iOS)` (confirmed false positive).
   - **fixed-font-size:** deliberately decorative sizes, such as Good Dog's 96 pt trophy or Soundscape's artwork initial, as opposed to text that must follow Dynamic Type.
   - **duplicate-button-label:** a context-menu or swipe action on a row, which VoiceOver reads with the row, as opposed to identical standalone buttons repeated in a `ForEach` (Relay's "Play Voice Message").
   - **icon-button-missing-label:** its only finding in ixOS was a false positive. Pocket Library's `NavigationRow` labels its button through a generic `label()` closure, and the chevron image is already hidden. It was not in the hand-checked sample, so it stays report-only until a fixture shows it is reliable.
   - **textfield-missing-label:** fields inside `ForgeField`, which attaches its label with `.accessibilityLabel`.
   Where a false positive can't be fixed upstream, the rule stays report-only, or the ixOS pattern gets an inline suppression with a reason.
3. **Gate one rule at a time.** A rule becomes a gate only once its fixture passes, in its own small commit, with today's findings for that rule saved as a baseline so only new findings fail. Rule 12 (no app left behind) then works as usual: the baseline only shrinks as apps are fixed. A baseline guards against old findings, not against new false positives, which is why the fixture comes first.
4. **No score gate.** `--min-score` and the grade are not used. A number is not a substitute for testing with VoiceOver on the phone, which stays part of each app's phone check.
5. Run it through `forge` so a person without CI gets the same report (AGENTS.md rule 9), for example as a step of `forge check` that is skipped with a reason when the tool isn't installed.

The phone check that would settle the three uncertain rules takes about five minutes in the next Device Lab sitting: with VoiceOver on, open and close one sheet and one alert and see where focus lands, and read a plain `TextField` with text entered.

The `#if` blind spot is worth reporting upstream; that would be a public comment, so only if Reuben wants it.
