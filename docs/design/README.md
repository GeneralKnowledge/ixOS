# ixOS Design Programme: Soundscape and Relay

Reuben's brief (2026-11-24): make Relay and Soundscape feel like premium, thoughtfully designed iOS apps,
cohesive, recognisable, beautifully animated and pleasant every day. They **share design principles, not a
look**: Soundscape emotional, expressive and immersive, with music and artwork at the centre; Relay calm,
precise and understated, with conversation at the centre. Both native iOS, Liquid Glass included, and neither
a reskinned Apple app.

His follow-up the same day set the order: **design research first, not another round of visual
implementation.** Study products that have solved the hard parts, extract why they work, and bring those
principles into ixOS selectively.

The outcome every PR is held to: **Soundscape is the app you enjoy looking at while listening. Relay is the
app you enjoy using because it gets out of your way.**

## The documents

| Document | What it holds |
|---|---|
| [DESIGN_PHILOSOPHY.md](DESIGN_PHILOSOPHY.md) | Principles, the three layers, glass rules, what we avoid, how a design change reaches the product |
| [DESIGN_REFERENCES.md](DESIGN_REFERENCES.md) | The reference study: what each product does, why it works, where it falls short, maturity, licences, comparison with ixOS today, and the hands-on capture still owed |
| [DESIGN_TOKENS.md](DESIGN_TOKENS.md) | Shared ForgeUI tokens (existing and proposed) and each app's own colour, type, metrics and motion, with contrast checks |
| [SOUNDSCAPE_DESIGN_SPEC.md](SOUNDSCAPE_DESIGN_SPEC.md) | Soundscape's identity, every screen with its states, signature interactions |
| [RELAY_DESIGN_SPEC.md](RELAY_DESIGN_SPEC.md) | Relay's identity, every screen with its states, signature interactions |
| [DESIGN_QA.md](DESIGN_QA.md) | Evidence rules, accessibility, compatibility, performance budgets, states, screenshots, acceptance criteria, the PR checklist |

The apps' own design docs stay the record of what is built: Soundscape
[design.md](../../examples/soundscape/docs/design.md), Relay
[relay-design.md](../../examples/relay/docs/relay-design.md) (#438).

## Where things stand (main at 5b31a175, 2026-11-24)

This plan is based on main at 5b31a175, the merge of #438, which is also this PR's base.

| PR | What | State |
|---|---|---|
| #418 | Soundscape redesign S1 to S6, Radio, Layered Navigation | Merged; installed earlier (Device Lab 1.29923637); its 11 phone checks not yet reported |
| #427 | Soundscape test catalogue | Merged |
| #428 | Soundscape Smart Playlists | Merged |
| #436 | Mini-player on every screen, Listen title fix | Merged |
| #437 | Soundscape Liquid Glass (incl. ForgeBanner glass) | Merged (d2eac3f9); phone check owed |
| #438 | Relay Liquid Glass (incl. the connection notice as a glass card) | Merged (5b31a175); phone check owed |

Both glass PRs were compiled in the combined Device Lab build with fork xtool 1.24.0-ixos.1, which stamps the
iOS 26 SDK. No glass has been seen on the phone yet: earlier builds came from upstream xtool, which stamps SDK
17.0, so iOS ran the apps in compatibility mode.

### Inconsistencies in the code

| Area | Today |
|---|---|
| Glass | Three private copies of "glass on iOS 26, material before": `soundscapeGlass` (#437), `relayFloatingBar` / `relayProminentButton` (#438), ForgeAppKit's `BannerSurface` (#437), with variants chosen ad hoc. Two of the glass uses are passive notices, not controls: ForgeAppKit's banner (#437) and Relay's connection notice (#438). Under the [material table](DESIGN_PHILOSOPHY.md#material-by-component) they move to standard material in V2.1 |
| Corner radii | ForgeUI 7 / 12 / 18; Soundscape mini-player 16 and overlays 28; Relay composer 24 |
| Fallback shadows | Three hand-written values |
| Motion | Soundscape has `SoundscapeMotion` with Reduce Motion; Relay has 12 animations and no Reduce Motion check |
| Accent | Soundscape ForgeUI purple; Relay system blue. The brief's accents fail 4.5:1 on white, so they need light-mode partners ([tokens](DESIGN_TOKENS.md)) |
| States | Soundscape uses `ForgeStateView` on 36 screens; Relay on 12, hand-made text elsewhere |

### What must not change

Soundscape's Rust library, queue, station mixer and playback engine; Relay's identity, trust, delivery
semantics, outbox, storage and networking; accessibility identifiers used by UI tests. Details in each spec.

### Missing evidence

Phone reports for #418, #437 and #438; any visual-regression baselines for either app; light-mode and AX5
screenshots since the redesigns; a compile of the Xcode 16 path; hands-on captures of the closed-source
references ([checklist](DESIGN_REFERENCES.md#hands-on-capture-checklist)).

## Shared foundation in ForgeUI

Extract only on a real second use (AGENTS.md rule 1). Glass and motion have it now; everything else is a
convention or a token.

| Into ForgeUI | Why |
|---|---|
| `ForgeMaterial` (`forgeGlass`, glass prominent button, glass group, one fallback shadow) | Replaces three copies; encodes Apple's rules (regular vs clear, grouping, Reduce Transparency) once |
| `ForgeMotion` (quick, standard, emphasised, ambient; Reduce Motion aware) | Relay gets Reduce Motion handling; Soundscape's motion names build on it |
| `ForgeRadius.xl` (28), concentric shape for edge-hugging controls | Removes stray radii |
| Scaled padding and frame helpers | Dynamic Type-aware sizes (as Element X's Compound does) |
| `ForgeStateView` offline preset, custom accent values | Both apps need calm offline states and their own accent with contrast checks |

Stays in the apps: artwork palettes, shelves, visualisations, Layered Navigation (Soundscape); bubbles,
grouping, delivery and trust presentation, device cards, the Assistant's identity (Relay); all colour
tokens.

## PR roadmap

Small, single-scope, independently mergeable. Swift is compiled on the laptop through the build queue before
review; phone checks are listed and never assumed.

| PR | Scope | Depends on |
|---|---|---|
| This PR | The six design documents (docs only) | — |
| #437, #438 (V1) | Liquid Glass in both apps; phone check on an SDK 26 binary | Both merged; compiled with fork xtool; phone checks owed |
| V2.1 | ForgeUI `ForgeMaterial` + `ForgeMotion` + radius and scaled metrics; both apps and ForgeBanner move onto them; banner and connection notice to standard material; Relay gains Reduce Motion; parity rule against a fourth private glass helper | #441 merged 2026-11-24 (with #440); built clean for iOS on the laptop; phone checks owed |
| V2.2 | App colour tokens with contrast tests (palettes adopted subject to visual review on the phone); offline state; first visual-regression baselines for both apps | V2.1 |
| V2.3 | Reference captures (Apple Music, Messages, Signal) and approved screen compositions (wireframes) for the first screens of V3 and V4, saved with the specs | Captures need Reuben with the iPhone; no code |
| V3.1 | Soundscape Listen and Library | V2.2, V2.3 |
| V3.2 | Soundscape Now Playing; album and artist anchor | V2.2, V2.3 |
| V3.3 | Soundscape Discover and Search | V2.2, V2.3 |
| V3.4 | Soundscape transitions and polish | V3.1 to V3.3 |
| V4.1 | Relay Chats | V2.2, V2.3 |
| V4.2 | Relay Conversation and composer | V4.1 |
| V4.3 | Relay Assistant and Devices | V2.2, V2.3 |
| V4.4 | Relay Settings and polish | V4.1 to V4.3 |
| V5.1 | Cross-app accessibility and visual QA; Instruments pass | V3.4, V4.4 |

V3 and V4 can run in parallel after V2.2 and V2.3; only V2 touches both apps.

## Visual work and feature work stay apart

**A visual redesign PR must not quietly become a feature PR.** If a design needs a new API, a persistence
mechanism, a change to a Rust model, or a change in behaviour, that part is split into its own PR, reviewed
on its own, and merged before the visual PR that uses it. The visual PR then only presents data that already
exists.

Already known to need separate work:

| Design element | What it needs first | Separate PR |
|---|---|---|
| Edit Listen in place (reorder, hide shelves) | A stored shelf order in Soundscape's collection | Soundscape feature PR before V3.1 uses it |
| Favourite Artists shelf | Confirm play counts by artist are exposed to Swift; if not, a small Rust ranking | Only if the check finds it missing |
| Relay trust rows in conversations | Confirm revoke and introduce events are recorded with who and when; if not, Rust and storage work | Relay feature PR, reviewed against the trust rules, before any trust row is drawn |
| Relay's set-aside data screen | The Rust report exists; the screen is new behaviour | Its own Relay PR |
| Moods, recommendations, pinning, time-of-day shelves | New Rust or services | Not planned unless asked |

Presenting existing data needs no split: avatars from identity keys, Made for You from Smart Playlists'
built-ins, grouped search results, bubble shapes, delivery captions.

## Risks

| Risk | Mitigation |
|---|---|
| The fork xtool build fails, or glass doesn't appear | V2.1 keeps the iOS 26 code paths unchanged, so a glass problem found on the phone is fixed once, in ForgeUI, for both apps |
| Shared helpers flatten the two personalities | ForgeUI gets materials, motion and metrics only; colour and content shapes stay in the apps |
| Radius rescale restyles every app | Separate decision, not part of V2 |
| Artwork palettes hurt readability | Existing 4.5:1 clamp; contrast tests on palette output |
| Main-thread image or palette work | Budgets in DESIGN_QA; Instruments in V5.1 |
| Redesign touches delivery or trust code | V4 PRs change views; model changes need their own reason |
| Identifier renames break UI tests | Add, never rename silently |
| Reference observations are second-hand for closed apps | Marked as such; the capture checklist turns them into first-hand evidence |
| AGPL or GPL code leaks in | No code from Signal, Element X, Telegram or Luma; ideas written fresh |
| "Design" PRs smuggle in features | New Rust API is named and agreed in the PR |

## Decisions (Reuben, 2026-11-24 12:18)

1. **Palettes:** adopted, subject to visual review on the phone (V2.2).
2. **Radius scale:** no global rescale. Add 28 where needed; existing apps keep 7 / 12 / 18.
3. **Xcode 16 checks:** yes, the manual macOS workflow once per major milestone (end of V2, V3, V4, V5).
4. **Feature-adjacent work:** existing data only in visual PRs; new Rust functionality gets separate PRs
   (see [Visual work and feature work stay apart](#visual-work-and-feature-work-stay-apart)).
5. **Hands-on captures:** yes, Apple Music (Now Playing, queue, library), Messages (conversation, composer,
   attachments) and Signal (delivery, identity, privacy) before any major visual implementation (V2.3).
   Buying Marvis Pro or Doppler is not a blocker.
6. **Glass on passive notices:** glass only on controls; notification banners and the connection notice use
   standard material or solid ([material table](DESIGN_PHILOSOPHY.md#material-by-component)).

## The next PR: V2.1, shared glass and motion

Reuben agreed this direction on 2026-11-24. The passive-notice question is settled (decision 6). At 12:24 he
decided V2.1 does not wait for #437 and #438 to pass on the phone: their phone checks and V2.1's are done
together in the next sitting, on one combined Device Lab build that holds both. The "before" for that sitting
is the build on the phone now (1.29925122, made before #437 and #438, so without their glass changes):
screenshots of Now Playing, the mini-player, a Relay chat and its connection notice are taken from it
before the new build is installed. There is no build with #437/#438 but without V2.1 on the phone, so the V1
checks run against the combined build, with the two passive notices expected frosted, not glass.

**Why this one.** It changes almost nothing a person sees on the happy path (only the two passive notices
lose their glass), so it respects "no more visual
implementation yet", and every later screen PR draws floating controls and animates them. Doing V3 and V4
on three private glass helpers, with no Reduce Motion in Relay, would multiply the inconsistency the
programme exists to remove. The reference study sharpened it: Apple's grouping (`GlassEffectContainer`),
concentric shapes, regular vs clear with a dim layer, and Element X's Dynamic Type-scaled metrics all
belong in this one place.

**Scope.** `forgekit/Sources/ForgeUI/Tokens/ForgeMaterial.swift` and `ForgeMotion.swift`; `ForgeRadius.xl`;
scaled padding and frame helpers; Soundscape, Relay and ForgeBanner call them (names that describe each
app's places, like `relayFloatingBar`, stay as thin wrappers); ForgeBanner and Relay's connection notice move
from glass to standard material (solid under Reduce Transparency); Relay's animations go through `ForgeMotion`;
Linux tests for tokens, contrast and the material choices (which variant each surface gets); a Materials and Motion section in docs/forgeui.md; a parity rule flagging
`.glassEffect(` outside ForgeUI.

**Not in scope.** Colours, layouts, new components, other apps' looks.

**Depends on.** #437 and #438 (merged). No phone gate; its phone checks join the next combined Device Lab sitting.

**Acceptance.** `cargo test --workspace`, ForgeKit's Linux Swift tests and `make validate` pass; the laptop's
Device Lab build compiles; on the phone (combined build, against the "before" screenshots above) the controls
from #437 and #438 show glass as their phone checks describe, and the Undo banner and Relay's connection notice
show standard material; with
Reduce Motion on, Relay's New Messages button, staging and connection notice don't animate; with Reduce
Transparency on, every glass surface is solid.
