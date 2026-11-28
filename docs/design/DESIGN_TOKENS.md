# Design tokens

The shared visual language of ixOS apps, and the app-level tokens that give Soundscape and Relay their own
identity. Shared tokens live in ForgeUI (`forgekit/Sources/ForgeUI/Tokens/`); app tokens live in each app.
Principles behind them: [DESIGN_PHILOSOPHY.md](DESIGN_PHILOSOPHY.md).

Status per token: **exists** (in main; the PR that added it is named), **proposed** (this programme; built in
the PR named), **deferred** (planned, but not built until a screen needs it), or **decision** (needs Reuben's
choice before it is built). Reconciled with main on 2026-11-28, after #440, #441, #443, #446 and #455.

## 1. Shared tokens (ForgeUI)

### Spacing (exists)

`ForgeSpacing`: 2, 4, 8, 12, 16, 24, 32 (`xxs` to `xxl`). A 4-point scale with one 2-point step for hairline
gaps. Apps use these names, or their own named metrics built from them (`SoundscapeMetrics.screenEdge = 20`
is a named exception for shelves lining up with large titles). No literal padding numbers in views.

Scaled spacing: padding and frames around text use `@ScaledMetric` so they grow with Dynamic Type
(Element X does this throughout with `scaledFrame` / `scaledPadding`). ForgeUI's own components already
use `@ScaledMetric` (button heights, row tiles, badges). A shared `forgeScaledPadding` / `forgeScaledFrame`
helper was planned for V2.1 but not built: **deferred** to the first V3 or V4 screen that needs one, so it
is shaped by a real use.

### Corner radii

| Token | Value | Use | Status |
|---|---|---|---|
| `ForgeRadius.s` | 7 | Badges, icon tiles | exists |
| `ForgeRadius.m` | 12 | Buttons, fields, small cards | exists |
| `ForgeRadius.l` | 18 | Cards, sheets | exists |
| `ForgeRadius.xl` | 28 | Overlays, large floating panels | exists (#441) |
| Concentric | follows the device | Floating controls near the screen edge (mini-player, composer) on iOS 26 | deferred (not in #441; with the mini-player in V3.1 and the composer in V4.2) |

Reuben's brief suggested 12 / 16 / 20 / 28. **Decided 2026-11-24: no global rescale.** 28 is added where
needed; existing apps keep 7 / 12 / 18.

Strays still in main: Soundscape mini-player 16 (and the radio station tile, 16), Relay composer 24. They go
when those controls are redesigned (V3.1, V4.2), not in a sweep. Relay bubbles 18 are content and keep an
app-owned value (see Relay tokens).

### Typography (exists, extended)

Dynamic Type text styles only. `ForgeTextRole` names the roles (screenTitle, sectionTitle, rowTitle, body,
metadata, note, figure, figureCaption). No fixed point sizes except SF Symbol glyphs, which scale with
`@ScaledMetric` relative to a text style. Times and counts use `.monospacedDigit()`.

### Colour (exists)

Semantic and adaptive. `ForgeAccent` gives each app an accent with light, dark and Increase Contrast values;
`ForgeContrast` checks 4.5:1 in Linux tests. Status colours (failure red, success green, warning orange) are
ForgeUI's tones (`danger`, `success`, `warning`) and are never replaced by an app accent.

An app's own accent needs no new API (the `ForgeAccent.custom` proposed here earlier was not needed):
`ForgePalette.standard.withAccent(light:dark:)` takes two `ForgeToneColors`, and
`ForgeTheme.contrastIssues()` checks every pairing ForgeUI draws, in light, dark and both with Increase
Contrast. Relay does exactly this (`RelayColors`, #455), with a test that the list is empty.

Contrast is checked on the colour a person actually sees: a translucent layer (text at 80%, an inset fill
at 20%) is composited over the background it sits on before the ratio is taken. #455's review found two
places where the raw pair passed (6.9:1) and the composed one did not (3.4:1); see
[DESIGN_QA](DESIGN_QA.md#2-accessibility-every-pr-for-every-changed-screen).

### Materials (exists, #441: `ForgeMaterial.swift`, `ForgeMaterial+SwiftUI.swift`)

One implementation replaced the three private copies (`soundscapeGlass`, `relayFloatingBar` /
`relayProminentButton`, ForgeAppKit's `BannerSurface`); the app names that remain are thin wrappers that
keep each app's layout. The parity rule `glass` fails any app that calls `glassEffect` or a `.glass` button
style by hand. `ForgeMaterial.material(for:)` maps each `ForgeSurfaceKind` (floatingControl,
controlOverMedia, notice, overlay, content) to its material, checked on Linux
(`ForgeMaterialMotionTests`). The table below is what was planned; the differences from what shipped are
listed under it.

| API | iOS 26 (SDK 26 binary) | iOS 17 to 25, or Xcode 16 | Reduce Transparency |
|---|---|---|---|
| `forgeGlass(.regular, in: shape)` | `glassEffect(.regular, in:)` | `.regularMaterial` + the fallback shadow | Solid (system on 26; `.background(.background)` fallback) |
| `forgeGlass(.clear, in: shape)` over media | `glassEffect(.clear, in:)`, only over a background already dark enough for white glyphs. Soundscape's Now Playing buttons sit on the artwork palette, which is clamped for white text, so they need no extra dim; over raw media that can be bright, dim it first (about 35%) | white at 14%: over a background clamped so white at 75% reads at 4.5:1 (sRGB grey 0.37 at worst), opaque white glyphs on the washed disc still read at 4.6:1, above the 3:1 for controls | Solid dark |
| `interactive: true` | `.interactive()` | none | — |
| `forgeGlassProminentButton()` | `.buttonStyle(.glassProminent)` | `.borderedProminent` | system |
| `ForgeGlassGroup { }` | `GlassEffectContainer` | plain container | — (not built; deferred) |

Which surface gets which material is decided by the
[material table](DESIGN_PHILOSOPHY.md#material-by-component), not by convenience: passive notices
(the Undo banner, Relay's connection notice and the Chats notices, #446) use `forgeMaterial(in: shape)`, the
standard material, never `forgeGlass`.

As shipped in #441, where it differs from the plan above:

- The standard material is `forgeMaterial(in:)` (no style argument), with a hairline edge.
- Under Reduce Transparency the fallbacks and the standard material turn to the theme's raised surface,
  not `.background(.background)`.
- `ForgeGlassGroup` was not built. No screen yet places two glass controls close enough to need one; it
  comes with the first that does (likely Now Playing's controls in V3.2).
- `ForgeGlassSupport.isAvailable` was added, for layouts that change around glass (Relay's composer is
  inset from the edges only when it floats).

Fallback tokens (exist): shadow black 12%, radius 12, y 4; clear-glass wash white 14%; hairline 60% at
0.5 pt. Shadows exist only in fallbacks and under large artwork.

### Motion (exists, #441: `ForgeMotion.swift`, `ForgeMotion+SwiftUI.swift`)

| Name | Value | Use |
|---|---|---|
| `quick` | ease-out 0.2 s | Small state changes: a symbol, a badge |
| `standard` | ease-in-out 0.35 s | Insertions, removals, banners |
| `emphasised` | spring, response 0.45, damping 0.8 | Things the finger drives: artwork scale, sheet settle |
| `ambient` | ease-in-out 0.6 s | Background colour and palette changes |

`.forgeAnimation(_:value:)` and `withForgeAnimation(_:reduceMotion:)` give no animation at all under
Reduce Motion. Touch feedback is the one exception to the four kinds: a press may be quicker (ForgeUI's
press styles use 0.12 s) and may keep a fade under Reduce Motion, but never moves. Shared transitions with an
opacity fallback were not built; screens that add transitions in V3 and V4 give their reduced form
themselves (DESIGN_QA 2.4). Every animation a person can trigger must be interruptible (springs or
SwiftUI's default animation merging, never a chained timeline that blocks input).

### Haptics (exists)

ForgeAppKit's `ForgeFeedback` vocabulary stays small: success, failure, selection, light tap for a
background event. No new kinds without a decision (Reuben, 2026-11-17).

### States (exists, extended)

`ForgeStateView` covers empty, loading and failed. Proposed: an `offline` preset, because both apps need a
calm "not connected" state that is not an error. Not built in V2.2: Relay's Chats list already says when
messages are waiting or the server can't be reached (#446 put those notices on the standard material), and
its single "Offline" line is part of V4.1. The preset is built when the first screen needs it, which also
gives it its second use.

## 2. Soundscape tokens (app-local, `SoundscapeColors`, `SoundscapeMetrics`, `SoundscapeMotion`)

### Colour (adopted 2026-11-24, subject to visual review on the phone)

The brief's palette is for dark appearance. Each value gets a light partner so the app follows the system
appearance; contrast ratios computed with the WCAG formula.

| Token | Dark | Light | Use | Contrast |
|---|---|---|---|---|
| `ink` | #101114 | #F7F6F3 | Page background | — |
| `surface` | #202329 | #FFFFFF | Raised items, cards | — |
| `ivory` (primary text) | #D8D5CE | #1A1B1F | Text on app surfaces | 12.9:1 on ink (dark), 15.9:1 (light) |
| `secondary` | #9A9890 | #6B6E76 | Metadata | 6.5:1 (dark), 4.7:1 (light) |
| `accent` | #A995D6 | #6B4FA8 | Tints, selected tab, Play, sung lyric | 7.2:1 on ink (dark), 6.4:1 on white (light) |
| `onAccent` | #101114 | #FFFFFF | Text on filled accent buttons | 7.2:1 (dark), 6.4:1 (light) |

Two traps the numbers show: the brief's accent on white is only 2.6:1, and white text on it is 2.6:1. So the
light accent is the darker partner, and dark-mode filled buttons use ink text, not white.

Main still uses `ForgeAccent.purple` (#8A3FBF light, #C58AF5 dark). Adopting these values is the Soundscape half
of V2.2, not yet done: a `SoundscapeColors` built the way `RelayColors` is (#455), with the same contrast
tests.

**Artwork palette** (exists, `ArtworkPalette.swift`, tightened in #440): average and most saturated colour
from a 24-pixel thumbnail. Each gradient stop is darkened until white at 75% (the secondary text) reads at
4.5:1, and the check runs against the average and every distinct thumbnail region, so a white patch in a
dark cover cannot pass on the average alone. The blurred artwork layer under the text is capped at 45%
opacity, and the fit lowers it further when a region would still fail. `ArtworkPaletteTests` cover the
lightest-region and every-region cases. It colours Now Playing, album and artist headers and the Continue
Listening card. Following Luma's rule (album colour belongs to the background; controls keep a
constant tint), artwork colour never tints controls.

### Type

| Role | Style |
|---|---|
| Display (Listen greeting, album / artist names on their pages) | `.largeTitle` bold, rounded design |
| Shelf titles | `.title3` bold, rounded |
| Track title / artist (Now Playing) | `.title3` bold / `.title3` at 75% (`ArtworkPalette.secondaryTextOpacity`, #440) |
| Rows | `.body` / `.subheadline` secondary |
| Metadata, times | `.footnote` / `.caption` `.monospacedDigit()` |

### Metrics (exists)

Row artwork 44, mini-player artwork 40 to 56, shelf artwork 150, screen edge 20, shelf gap 12, section gap 24,
Now Playing edge 28, overlay radius 28 (moves to `ForgeRadius.xl`).

### Motion (exists; `SoundscapeMotion` names, ForgeMotion for scrolling and lyric changes since #441)

Palette 0.6 s (`ambient`), artwork pause scale spring (`emphasised`), zoom transitions iOS 18+, all off
under Reduce Motion.

## 3. Relay tokens (app-local: `RelayColors` exists since #455; `RelayMetrics` proposed)

### Colour (adopted 2026-11-24, subject to visual review on the phone; accent in main since #455)

| Token | Dark | Light | Use | Contrast |
|---|---|---|---|---|
| `midnight` | #111317 | system grouped background | Backgrounds | — |
| `surface` | #20242B | #FFFFFF | Incoming bubbles, device cards | — |
| `text` | #E5E7EB | system label | Primary text | 15.0:1 on midnight |
| `secondary` | #9CA3AF | #646B78 | Timestamps, previews | 7.3:1 (dark); 4.8:1 on the grouped background (the brief's #6B7280 was 4.3:1 there) |
| `accent` | #829BCB | #3E5A8E | Outgoing bubbles, links, selected tab | 6.6:1 on midnight; 6.9:1 on white |
| `onAccent` | #111317 | #FFFFFF | Text in outgoing bubbles | 6.6:1 (dark), 6.9:1 (light) |

As in Soundscape, the brief's accent fails on white (2.8:1) and with white text (2.8:1), hence the darker
light partner and midnight text in dark-mode outgoing bubbles. What #455 put in main:

- `RelayColors.theme`: ForgeUI's standard palette with Relay's accent, applied at the root, so Device Lab's
  sheet gets it too.
- Our bubbles use the accent with onAccent text; a failed bubble uses the danger tone with its own text
  colour; a sending bubble keeps the full accent (at 60% its text fell to 2.8:1).
- Text inside our bubbles (quotes, document and voice captions) is full strength, and insets on our bubbles
  are outlines, not fills.
- `RelayColorsTests` checks `contrastIssues()`, text on the accent and danger bubbles, and the redesign
  tokens in this table.

Midnight, surface, text and secondary are defined and tested but not used yet: the screens keep the
system's list backgrounds and label colours until they are redesigned (V4).

Delivery and trust colours stay semantic: Not Delivered red, trust badges in their existing system colours,
never the accent.

### Metrics (proposed)

| Token | Value | Notes |
|---|---|---|
| Bubble radius | 18 | Existing value, kept; inner corners of grouped bubbles 6 (Element X and Messages both tighten the inner corners of a run) |
| Bubble padding | 12 × 8 | Element X uses the same; scaled with Dynamic Type |
| Group gap | 2 within a run, 8 between runs | Today 0 and 6 |
| Max bubble width | 75% of the column | |
| Avatar | 36 (list), 28 (group conversation) | Initials on a colour derived from the identity key |

### Type

System only. Titles `.headline`; previews `.subheadline` secondary; message `.body`; timestamps and
delivery `.caption2` secondary `.monospacedDigit()`; one timestamp format everywhere.

### Motion

`standard` for message insertion (only when at the bottom), `quick` for delivery state changes; no spring
bounces; everything off under Reduce Motion. Since #441 all seven chat animations go through
`withForgeAnimation` and stop under Reduce Motion.
