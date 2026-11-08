# Visual regression: noticing when a screen got worse

Status: 2026-11-08. Built and tested on Linux (`forge-uitest`'s `visual` and `baseline` modules, `forge ui run --visual`, `forge ui accept`, the `forge_accept_screenshots` MCP tool). **Not yet run on an iPhone**: no baseline has been recorded from a real `forge ui run` yet, and the thresholds below are chosen from the screen geometry, not tuned on real screenshots.

A UI test that passes says the controls are there and respond. It does not say the screen still looks right: a button pushed off screen by a long label, a list that lost its padding, a dark-mode colour that went unreadable, a toolbar that vanished. An agent changing SwiftUI needs to see that, without a person comparing pictures. This page is item A9 of the [agent capability audit](research/forge-agent-capability-audit.md) and the screenshot half of [Forge UI Testing M4](forge-ui-testing.md#milestones).

## How it works

Every `screenshot = "name"` step in a [UI test file](forge-ui-testing.md) is compared, after the run and on Linux, with that scenario's approved screenshot of the same name:

```
examples/pocket-library/uitests/m0.toml
examples/pocket-library/uitests/baselines/m0/1-library@1290x2796.png        approved screenshot
examples/pocket-library/uitests/baselines/m0/1-library@1290x2796.tree.json  its accessibility tree
```

- Baselines are committed with the app, so a reviewer sees an approved screen change in the same PR as the code that changed it.
- The size is part of the name. One scenario can have baselines for several phones; a phone with no baseline of its size gets `new`, never a false `changed`.
- With each screenshot, `forge ui run` now also saves the accessibility tree (`name.tree.json`), so the comparison can say *what* changed, not only *where*.
- Screenshots taken at a failed step (`failure-stepN.png`) are never compared.
- Accepting a baseline doesn't make the app's other evidence stale: `uitests/baselines/` is left out of `build.tree` ([evidence.md](evidence.md)).

## What counts as a change

Pixel noise must not fail a run, and a changed word must. The rule, in `crates/forge-uitest/src/visual.rs`:

1. A pixel differs when any colour channel differs by more than `tolerance` (16 of 255). That absorbs colour-space rounding but not a colour change.
2. Differing pixels are grouped into areas (4 × 4 pixel cells; touching cells join).
3. An area counts when it is at least `min_region_pt` (6 points) wide **and** tall, or at least 40 points in one direction. A text cursor that blinked between the two screenshots (about 2 pt by 20 pt) doesn't count; a changed word (a 17 pt glyph is about 9 × 12 pt) or a vanished divider line does.
4. Masked areas are not compared at all: the status bar always (clock, battery; 54 pt on Face ID iPhones, 20 pt on the others), and any element the scenario names.

A screen is `changed` when any area counts or its size changed. Areas are reported in points, like the tree's frames, with the elements in them.

From the two trees, the comparison also reports, per element (matched by identifier, else by type and label):

| Change | Means |
|---|---|
| `removed`, `added` | The element is gone, or new |
| `label`, `value` | VoiceOver reads something else; a field holds other text |
| `enabled` | A control was enabled and isn't, or the reverse |
| `moved` | Its frame moved or resized by 2 points or more |

and any **new accessibility findings** (`no_label`, `no_identifier`, `duplicate_identifier`) the baseline's tree didn't have. A new finding fails the run even if the pixels match: losing a VoiceOver label is a regression too.

## Masks and settings

Content that differs between runs on purpose (times, dates, "updated 3 min ago", avatars, the user's own data) is masked by element, using the same locators as steps. Never by coordinates.

```toml
name = "Library"
app = "org.forgemobile.pocketlibrary"

[visual]
mask = [{ id = "library.lastSynced" }, { label_contains = "minutes ago" }]
# status_bar = true      the default: mask the clock and battery
# tolerance = 16         per colour channel, of 255
# min_region_pt = 6      smaller areas don't count
```

A mask is found in the trees saved with the screenshot and its baseline. A mask that matches nothing on a screen is reported as a note, so a renamed identifier doesn't silently stop masking.

The test's own data is best kept stable instead: launch with arguments that load fixture data, so the screen is the same every run. Pocket Library's `m0.toml` shows the real library on the phone, so its library screenshot changes whenever a book is added; mask the list or seed the library before recording baselines for it.

## Commands

```bash
forge ui run examples/pocket-library/uitests/m0.toml               # compares (default: --visual check)
forge ui accept examples/pocket-library/.forge/runs/<id>-ui-run     # approve that run's screenshots
forge ui accept <run> --only 2-new-book-form                        # just one
forge ui run <test> --visual record                                  # make every screenshot the baseline
forge ui run <test> --visual off                                     # skip the comparison
```

`forge ui run` prints a line per screenshot (`match`, `new`, `changed`, `recorded`) and, for a changed one, its areas and element changes:

```
Screenshots against examples/pocket-library/uitests/baselines/m0:
  match    1-library
  changed  2-new-book-form (1 area, 0.41% of pixels), diff visual/2-new-book-form.diff.png
           area 120x44 pt at (16, 214): TextField id=bookForm.author "Author"
           moved TextField id=bookForm.author "Author": 398x44 at (16, 170) -> 398x44 at (16, 214)
```

The diff image is the new screenshot washed out, with differing pixels red, masked areas blue and each counted area outlined. A changed screen fails the command like a failed step (stage `test`, exit 14) and says how to accept it. `new` screens don't fail the run; the command says to look at them and accept them.

`forge ui accept` reads the scenario from the run's `evidence.json` (or `--scenario` for a run outside an app), copies each screenshot and its tree into the baselines folder, and says what to commit. It never compares or judges: accepting is the decision that the picture is right, made by whoever looked at it. Run the test again afterwards so the evidence shows a pass.

## In the evidence bundle

A device bundle (`forge.evidence/1`, [evidence.md](evidence.md#device-runs-forge-ui-run)) gains a `visual` object, and its files are listed under `artifacts` (`ui_tree` for each saved tree, `visual_diff` for each diff image):

```json
"visual": {
  "mode": "check",
  "baselines": "examples/pocket-library/uitests/baselines/m0",
  "screens": [
    { "name": "1-library", "verdict": "match", "screenshot": "1-library.png",
      "baseline": "…/1-library@1290x2796.png", "width": 1290, "height": 2796, "changed_ratio": 0.0 },
    { "name": "2-new-book-form", "verdict": "changed", "changed_ratio": 0.0041,
      "regions": [ { "rect": {"x": 16, "y": 214, "width": 120, "height": 44},
                     "pixels": {"x": 48, "y": 642, "width": 360, "height": 132},
                     "changed": 14520, "elements": ["TextField id=bookForm.author \"Author\""] } ],
      "diff": "visual/2-new-book-form.diff.png",
      "tree_changes": [ { "change": "moved", "element": "…", "before": "…", "after": "…" } ],
      "new_findings": [] }
  ]
}
```

A changed screen (or a new accessibility finding) makes `ok` false and sets the `device` verdict of the requirements this scenario proves to `failed`, with a note per screen ("screenshot 2-new-book-form differs from its baseline"). So `forge verify` keeps the requirement open until the change is fixed, or accepted and the test run again. This adds fields only, so the schema stays `forge.evidence/1`.

## For agents (`forge mcp`)

- `forge_run_ui_test` compares by default. Its result carries `data.visual` as above and `data.out` (the run folder). When no step failed but a screen changed, the image it returns is the first diff instead of a failure screenshot.
- `forge_accept_screenshots` (`run`, optional `only`) runs `forge ui accept`. The conventions tell the agent to accept only after looking: for a new screen, that it is right; for a changed one, that it is the change it meant. Otherwise it fixes the app.
- `forge_ui_act`'s one-step screenshots are never compared (`--visual off`): one action is not a scenario.

## Limits and next steps

- **Not run on a phone yet.** The first device run should record Pocket Library's baselines, run `m0.toml` twice more unchanged (expect `match` both times; this is the check that the cursor, status bar and animation rules hold on real screenshots), then make one deliberate change and expect `changed` on the right element. That run needs Cati and belongs in the coordinated iPhone session.
- **Timing.** A screenshot taken mid-animation differs from the baseline. Put an `expect` before each screenshot step so the screen has settled; WebDriverAgent waits for idle, but SwiftUI transitions can outlast it.
- **Build kind.** Linux xtool builds may render in iOS 26's compatibility mode ([ios-ui.md](ios-ui.md)); baselines must come from the same kind of build as the runs compared with them.
- **Appearance matrix.** Light, dark and the largest text are separate screenshots today (launch arguments in separate scenarios). A matrix that produces all of them per screen (audit item A5) would feed straight into this.
- **No perceptual metric.** The rule is exact pixels plus area size, which is predictable and needs no dependency beyond `png`. odiff-style anti-aliasing detection or a perceptual metric would be the next step if real runs show noise the area rule doesn't absorb.
- **Simulator screenshots.** The macOS CI screenshot jobs could use the same comparison; they don't yet.
