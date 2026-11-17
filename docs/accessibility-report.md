# Accessibility report

A list of likely accessibility problems in every app's Swift sources, the starter templates and ForgeKit, made on Linux before anything reaches the phone. **Report only: a finding never fails a build.** It doesn't replace testing with VoiceOver on the iPhone, which stays part of each app's phone check ([device-validation.md](device-validation.md)).

The checker is [a11y-check](https://github.com/cvs-health/ios-swiftui-accessibility-techniques) from CVS Health (Apache-2.0), a source checker built on SwiftSyntax with 47 rules mapped to WCAG 2.2. It runs as a development tool. No app, ForgeKit or Rust crate depends on it. Why it was chosen, and how accurate it was on ixOS's apps, is in its [assessment](research/external-projects/assessments/accessibility/a11y-check.md).

## Running it

```bash
scripts/a11y-report.sh                  # needs Swift 6 on PATH
FORGE_SWIFT_DOCKER=swift:6.2 scripts/a11y-report.sh   # or build it in Docker
```

The first run builds the pinned version (`0.7.0`), which takes about 7 minutes, into `~/.cache/forge/a11y-check/0.7.0`. After that a run takes about 12 seconds. It prints a table per app, and writes one JSON report per app to `target/a11y-report/`. It exits 0 whatever it finds, and non-zero only if the tool can't be fetched, built or run.

In CI, the [Accessibility report](../.github/workflows/a11y-report.yml) workflow runs it when Swift sources change, puts the table on the run's page, and keeps the JSON as an artifact. The workflow is marked continue-on-error and nothing requires it.

## Reading a finding

Each finding names a rule, a file and a line. Treat it as a question, not a verdict. A source checker sees the code, not the accessibility tree that VoiceOver reads, so some findings are known to be wrong for ixOS:

| Rule | Known false positive in ixOS |
|---|---|
| input-missing-purpose | `.textContentType` inside `#if os(iOS)` isn't seen |
| textfield-missing-label | Fields inside `ForgeField`, which puts its label on the field with `.accessibilityLabel` |
| icon-button-missing-label | Buttons that take their label from a generic closure (Pocket Library's `NavigationRow`) |
| fixed-font-size | Deliberately decorative sizes (an icon drawn at a size, an artwork initial scaled to its frame) |
| duplicate-button-label | Context-menu and swipe actions on a row, which VoiceOver reads with the row |

Two rules are turned off in [`.a11ycheck.yml`](../.a11ycheck.yml) because most of their findings were noise: **sheet-focus-return** (it fires on every system alert) and **heading-trait-missing** (it flags ordinary styled text). A short VoiceOver check on the phone decides whether either comes back.

## How a rule becomes a gate

Not yet, and one rule at a time:

1. **A fixture first.** A small Swift fixture with one real problem the rule must flag, and the ixOS patterns above that it must not flag. A rule that can't pass its fixture stays report-only, or the ixOS pattern gets an inline suppression with a reason.
2. **A baseline for that rule.** Today's findings for that rule are saved, so only new ones fail. The baseline only shrinks as apps are fixed (AGENTS.md rule 12).
3. **Its own small change**, so a rule that turns out noisy can be turned back to report-only without touching the others.

The score and grade a11y-check prints are not used, and no build will gate on a number.
