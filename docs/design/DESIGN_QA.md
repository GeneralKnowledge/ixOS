# Design QA

The visual and behavioural acceptance criteria for every design PR in this programme. A screen is done when
it meets these with evidence, not when it looks good in a screenshot. Screenshots show appearance; tests and
phone checks show behaviour.

## 1. Evidence rules

- A claim needs its evidence: a build log for "compiles", a test run for "tested", a person's report from
  the iPhone for "works on the phone". Nothing is recorded as passing that wasn't run.
- Swift that hasn't been compiled for iOS is said to be uncompiled. Glass that hasn't been seen on an SDK 26
  binary on the phone is said to be unseen.
- The iOS 17 and 18 appearance can only be compiled, not seen (no older device): it is recorded as
  "compiled, not seen".

## 2. Accessibility (every PR, for every changed screen)

1. Dynamic Type to AX5: no clipped or overlapping text; layouts go vertical at accessibility sizes;
   fixed-height bars cap at their documented size.
2. VoiceOver: every interactive element has a label a person understands and an identifier
   (`screen.element`); reading order follows the visual hierarchy; long-press menus are reachable as
   actions; delivery, trust and playback states are read aloud.
3. Contrast: 4.5:1 for text, 3:1 for large text and control outlines, in light, dark and Increase
   Contrast. Token pairs and clamped artwork palettes are checked by Linux tests with `ForgeContrast`.
4. Reduce Motion: every animation has its stated reduced form.
5. Reduce Transparency: every glass and translucent surface turns solid and stays readable.
6. Bold Text, Button Shapes, Differentiate Without Colour: layouts hold; no state shown by colour alone.
7. Every gesture has a visible button.
8. Voice Control: visible labels match accessibility labels.

`forge a11y` runs on screens a UI test reaches ([accessibility-report.md](../accessibility-report.md)); the
phone confirms.

## 3. Compatibility

| Requirement | Check |
|---|---|
| Deployment target iOS 17 | Every newer API behind `#available` |
| Xcode 16 (Swift 6.0, iOS 18 SDK) | iOS 26 APIs also behind `#if compiler(>=6.2)`; the manual macOS workflow (`macos-15`) compiles this path, once per milestone if agreed |
| Linux, xtool fork 1.24.0-ixos.1, iOS 26 SDK | `scripts/check-ios-xtool.sh affected main` on the laptop through the build queue |
| Liquid Glass appears | Device Lab on the iPhone (iOS 26.3.1) |
| Light and dark | Phone screenshots in both |

## 4. Responsiveness and performance budgets

On the iPhone 15 Pro Max, a debug Device Lab build unless the PR says release:

| Budget | Target |
|---|---|
| Tap acknowledgement | A control visibly responds within 100 ms |
| Slow work | Anything over 500 ms shows progress and never blocks navigation, playback controls or the composer |
| Scrolling | No visible hitch scrolling Soundscape's Songs with the test catalogue, Listen's shelves, or a 1,000-message Relay conversation |
| Track change | Artwork and palette change without a stutter in the scrubber |
| Main thread | No image decoding, palette extraction, Markdown parsing or JSON work on the main thread (code review; Instruments in V5.1) |
| Off screen | No animation or visualisation runs off screen, in the background, or under Reduce Motion |

## 5. States

Every changed screen is designed and checked in: filled with realistic data, empty, loading, offline,
failed, and its unusual cases (very long names, one item, thousands of items, missing artwork, deleted
message, revoked peer). Realistic data looks as good as demo data.

Feedback contract (both apps): cancellation is never shown as failure; a recoverable failure names what
failed and offers the next action; destructive actions say what will be removed (and, in Soundscape, whether
playback stops); network loss is information, not an alert.

## 6. Screenshots (before and after)

Every PR from V2.2 on carries, for each changed screen:

- Before and after from the iPhone (Device Lab), 1290 × 2796.
- Light and dark.
- Default text size and AX5 for the main state.
- Empty, loading, offline and failed states where the screen has them (the test catalogue #427, and a
  stopped relay or chat server, produce these).
- Realistic data: the test catalogue for Soundscape; a conversation with a photo, a voice message, a reply
  and a reaction for Relay.

Where a UI test reaches the screen, the screenshot is a `screenshot =` step so it becomes a visual-regression
baseline ([visual-regression.md](../visual-regression.md)); neither app has baselines yet, and V2.2 adds the
first. Screenshots a person takes are attached to the PR and labelled as such.

## 7. Acceptance criteria per milestone

| Area | Standard | Evidence |
|---|---|---|
| Visual consistency | Spacing, radii and type from tokens; no literal values outside the app's metrics files | Review; a grep for literal `cornerRadius:` and `padding(` numbers in changed files |
| Responsiveness | Section 4 | A person's phone report |
| Motion | Interruptible; reduced forms work | Phone, Reduce Motion on and off |
| Accessibility | Section 2 | `forge a11y`; phone with VoiceOver and AX5 |
| Compatibility | Section 3 | Laptop build log; macOS run per milestone; phone |
| Content and states | Section 5 | Screenshots per section 6 |
| Reliability | No regression in playback, queue, radio, Smart Playlists, messaging, delivery, trust or networking | `cargo test --workspace`, `make validate`, the apps' UI test scenarios, their phone checks |

## 8. PR checklist

Before a screen is built: its required reference captures exist and its composition (wireframe or mock-up)
is approved.

Each design PR description has:

- The principles it serves (one line each) and a short design rationale.
- Changed files and who owns them (ForgeUI, the app).
- Build and test evidence (commands and results), or "not compiled" said plainly.
- Before and after screenshots per section 6, or "owed" with the reason.
- The manual phone checks, numbered, each with what it proves.
- A statement that it presents only existing data. Any new API, persistence or behaviour belongs in a separate PR merged first, linked here.
- The material each new or changed surface uses, matching the [material table](DESIGN_PHILOSOPHY.md#material-by-component).
