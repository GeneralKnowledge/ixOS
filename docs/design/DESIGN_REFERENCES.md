# Design references

What the reference products do, why it works, where it falls short, and what ixOS takes from it. The aim is
the reasoning behind an interface (hierarchy, navigation, state, motion, feedback, accessibility and the
unglamorous edge cases), not its look. Nothing here is copied: see [Licences and reuse](#licences-and-reuse).

Study date 2026-11-24. Principles drawn from this page: [DESIGN_PHILOSOPHY.md](DESIGN_PHILOSOPHY.md).

## How each reference was studied

Being explicit about evidence, because a description of an app is not the app:

| Reference | Studied from | Hands-on capture |
|---|---|---|
| Signal iOS | Source (github.com/signalapp/Signal-iOS, main, shallow clone) | Owed |
| Element X iOS | Source (github.com/element-hq/element-x-ios, main), incl. its Compound design system | Owed |
| Telegram iOS | Source tree layout only (github.com/TelegramMessenger/Telegram-iOS); too large for a meaningful read in this pass | Owed |
| Luma iOS | Source (github.com/knp-org/luma-ios, 1.0.1, 2026-10-09) | Not available (simulator screenshots in its README only) |
| iMusic | Source (github.com/LinZong/iMusic, 2026-11-08) | Not available |
| Resona | Source and product specs (github.com/leonfrer/Resona, 2026-08-17) | Not available |
| Plexamp | App Store listing (4.12.4, July 2025) and Plex release notes | Owed |
| Marvis Pro | App Store listing (10.2.4) and the MacStories review (Marvis 3.0, 2019: dated) | Owed |
| Doppler | App Store listing (3.2, Oct 2025) and the MacStories Mac review (2021) | Owed |
| Apple Music, Messages | Apple's own HIG and guides; general platform knowledge | Owed |
| Apple HIG | [Materials](https://developer.apple.com/design/human-interface-guidelines/materials), [Adopting Liquid Glass](https://developer.apple.com/documentation/technologyoverviews/adopting-liquid-glass) (read in full) | — |

**"Owed" means the screenshots and recorded interactions this programme requires have not been made.** The Apple Music, Messages and Signal captures are required before any major visual implementation (V2.3); Marvis Pro and Doppler are optional. No
agent here can install or operate an App Store app. The [capture checklist](#hands-on-capture-checklist) at
the end lists exactly what a person needs to record on the iPhone; until then, observations about closed apps
are from public descriptions and are marked so.

## Maturity

| Reference | Maturity | Weight it carries |
|---|---|---|
| Apple HIG, Apple Music, Messages | Platform owner | Defines native behaviour; follow unless we have a reason |
| Signal | Established, security-audited, years in production | Strong for trust and delivery communication |
| Element X | Established product (Element), current SwiftUI rewrite on a Rust SDK | Strong for architecture and design-system discipline; closest to ixOS's own shape (SwiftUI over Rust) |
| Telegram | Established, very large; custom rendering stack (AsyncDisplayKit-style nodes) | Interaction ideas only; its UI stack is nothing like SwiftUI |
| Plexamp, Marvis Pro, Doppler | Established commercial apps with long histories | Strong for product judgement; we can't see their code |
| Luma | Small open project, v1.0.1, iOS 26 only, 10 tests | An implementation reference, not proof of product quality |
| iMusic | Active open project, no signed releases yet, iOS 17+ | Useful for iOS 17 + iOS 26 dual paths and UI-test isolation |
| Resona | Self-described early-stage learning project, iOS 26 only | Its written specs and quality budgets are useful; the app is not a benchmark |

## Soundscape references

### Plexamp: atmosphere and playback-first thinking (public material only)

- **What it does.** A dedicated music player for a personal server. Now Playing has "UltraBlur backgrounds,
  over a dozen hypnotic visualizers, and four visual themes"; a "background colors setting which pulls from
  currently playing item"; "swipe-up play queue peeking"; radios "built from your library", by era, style
  or mood; a Mix Builder; half-star ratings with haptics in the player; long press on Play to stop.
- **Why it works.** Listening is treated as the experience and the library as its source: radio, mixes and
  the player come first, the file list second. The background colour comes from the music, so the app feels
  different with every album without changing its structure.
- **Where it falls short.** The App Store listing declares no accessibility features. A heavy settings
  surface ("as many settings and tweaks as our resident UXpert let us add"). A 2025 review complains about
  its CarPlay look on iOS 26.
- **Take for Soundscape.** Radio and Continue Listening at the top of Listen; queue peeking from the player
  (our queue sheet should show the current song and stay half-raised over the player); artwork-driven
  background colour (already built: `ArtworkPalette`). Visualisers only as an optional, Reduce Motion-aware
  alternate view.
- **Don't take.** Settings sprawl; themes as a substitute for one good default.

### Marvis Pro: library sophistication (public material, partly dated)

- **What it does.** A customisable Apple Music client. Home is built from about 30 section types that can be
  added, removed and rearranged (long-press a heading to edit); grids of 1 to 4 columns; artwork corners
  curved, flat or circular; Smart Rules ("Filter, Sort, and Limit"); drag a song to drop zones for Play Next,
  Add to End, Shuffle; per-section tap / long-tap / double-tap actions; interactive and extra-large widgets.
- **Why it works.** Density without a settings panel: customisation happens in place, on the thing being
  customised. Smart Rules turn the library into views the person defines.
- **Where it falls short.** MacStories called it "very plain" with colour only from artwork, and its
  raise/dismiss gesture "unrefined" (2019). Gesture-per-section is powerful and undiscoverable. No declared
  accessibility features.
- **Take for Soundscape.** Edit Listen in place (reorder and hide shelves from a shelf's own menu, no
  separate settings screen); Smart Playlists (#428) surfaced as shelves; sort and filter on Library lists
  as a small floating overlay (already planned in Layered Navigation).
- **Don't take.** Hidden multi-tap gestures; 30 section types; customisation as the product.

### Doppler: respect for the collection (public material)

- **What it does.** A focused player for owned music. Recently added albums and search on entry; the person
  chooses whether the library list shows artists, albums or songs and rearranges library options; Album
  Collections; queue editable while listening, grouped by album or playlist; listening reports. The App Store
  listing declares VoiceOver, Voice Control, Larger Text, Dark Interface, Differentiate Without Color
  Alone, Sufficient Contrast and Reduced Motion. 3.2 adopted Liquid Glass.
- **Why it works.** Albums and artists are the units, not tracks; there is no discovery noise. Accessibility
  is declared and specific.
- **Where it falls short.** No radio or discovery at all (by design). Doesn't read Apple Music.
- **Take for Soundscape.** Library as a record collection: albums first-class, queue grouped by album when
  playing an album; declare and meet the same accessibility list (it is our QA list).
- **Don't take.** Its narrowness: Soundscape keeps radio and catalogues.

### Apple Music: native behaviour (platform knowledge; capture owed)

- **Take.** The mini-player above the tab bar opening Now Playing; artwork that shrinks on pause; lyrics in
  place of artwork with the sung line emphasised; the queue as a sheet with shuffle and repeat at the top;
  Now Playing controls on the lock screen and in Control Center; AirPlay and volume where people expect
  them. Soundscape already follows most of this (design.md S3).
- **Don't take.** Its navigation and its look: Soundscape's tabs and Listen home are its own, and it uses
  the system's patterns, not Apple Music's colours or layouts.

### Luma (source read): an engineering reference

- **Licence:** AGPL-3.0. Read for ideas only, never copied.
- **Observed** (`Luma/AlbumTheme.swift`): "Album color belongs to the background; control tint always
  remains silver." The current cover's tint feeds an environment value; the app background is a soft radial
  wash of it at 24% strength, lifted for dark covers so they still get a visible hue, animated over 0.6 s and
  not at all under Reduce Motion; the background is `accessibilityHidden` and ignores hit-testing.
- **Observed** (`Luma/Artwork.swift`, `AudioVisualizerView.swift`): animated artwork and the visualiser
  stop when off screen, when the scene is inactive, or when Reduce Motion is on, and the visualiser says why
  ("Reduce Motion is on").
- **Observed:** custom glass used sparingly: `glassEffect(.regular.interactive(), in: .circle)` on round
  transport buttons, a tinted glass for the primary Play. iOS 26 only, so no fallbacks to learn from.
- **Observed:** `Docs/ArchitectureReview.md`, a prioritised self-review (MusicPlayer has too many
  responsibilities; lyrics re-parsed during view updates). A good habit; our foundation reviews do the same.
- **Take.** Constant control tint over changing artwork colour; stop all decorative animation off screen,
  in the background and under Reduce Motion; background colour hidden from VoiceOver.

### iMusic (source read): iOS 17 and iOS 26 side by side

- **Licence:** MIT. Code could be reused with attribution; we don't need to.
- **Observed:** separate views for the two eras (`GlassMiniPlayer` with `glassEffect(.regular, in:
  .rect(cornerRadius: 32))` vs `MiniPlayer`), chosen with `#available(iOS 26.0, *)` at the call site. Same
  deployment target as ours (iOS 17).
- **Observed** (`Docs/UITesting.md`): UI tests run the production SwiftUI views and models with a silent
  clock in place of audio, an isolated per-scenario data folder, and identifiers that supplement accessible
  labels. `Docs/Validation.md` keeps a list of what still needs real-device acceptance before release.
- **Take.** For screenshot capture, a deterministic Soundscape state (the test catalogue #427 plus a silent
  player) so before/after screenshots compare like with like. Keep "needs real device" as an explicit list,
  as our evidence rules already do.

### Resona (source and specs read): written quality budgets

- **Licence:** none published, so all rights reserved: read only, nothing reused.
- **Observed** (`docs/product-specs/quality-attributes.md`): measurable budgets, e.g. controls acknowledge
  input within 100 ms; work over 500 ms shows progress without blocking navigation or playback; no
  main-thread hang of 100 ms or more while scrolling a large list; playback stays responsive during imports.
- **Observed** (`experience-foundation.md`): a feedback contract (cancellation is not failure; recoverable
  failures name the item and offer the next action; destructive confirmations say whether playback stops;
  unavailable items are marked on their row).
- **Take.** The same kind of budgets and feedback contract in [DESIGN_QA.md](DESIGN_QA.md), written in our
  own words for Soundscape and Relay.

## Relay references

### Signal (source read): trust and restraint

- **Licence:** AGPL-3.0. Ideas only.
- **Observed** (`Signal/ConversationView/Components/CVComponentFooter.swift`): delivery status is a small
  template icon in the bubble's footer beside the time, tinted with the footer text colour, animated only
  while sending. It never competes with the message.
- **Observed** (`CVComponentSystemMessage.swift`): a change in someone's verification or safety number
  appears as a system message in the conversation, centred, worded by who changed it and where ("verified
  on this device", "on another device"), with an action to view the safety number. Trust events are part of
  the conversation's history, not modal alerts.
- **Take for Relay.** Delivery as a quiet footer element; a trust change (device revoked, a new device
  introduced, identity changed) as a system row in the conversation with one action, never only an alert;
  wording that says who did what, where.
- **Don't take.** Its UIKit component system; Signal's colours or bubble shapes.

### Element X (source read): architecture and design-system discipline

- **Licence:** AGPL-3.0 or Element commercial licence. Ideas only.
- **Observed:** Compound, a small in-repo design system: semantic colour tokens (`.compound.textPrimary`,
  `bgSubtleSecondary`, `iconAccentTertiary`), fonts, button and text field styles, a couple of components,
  and layout helpers (`scaledFrame`, `scaledPadding`, `scaledOffset`) that scale sizes with Dynamic Type.
  Previews double as snapshot tests (`TestablePreview`).
- **Observed** (`Screens/Timeline/View/Style/TimelineStyle.swift`, `TimelineItemBubbleBackground.swift`):
  every message knows its place in a run (`single`, `first`, `middle`, `last`), and the bubble rounds only its
  outer corners, so a run reads as one unit. Padding 12 × 8, radius 12.
- **Observed** (`TimelineItemSendInfoLabel.swift`): the bubble's status shows nothing for ordinary success;
  it shows an icon and wording only for failure ("Sending failed") or an encryption-authenticity problem.
  Normal state is silent; problems are explicit.
- **Observed** (`AccessibleLongPress.swift`, `TimelineItemAccessibilityModifier.swift`): long press becomes
  a named accessibility action under VoiceOver, so the menu is reachable without the gesture.
- **Observed:** each screen is a Coordinator, a ViewModel with a protocol, and a View, with state in a
  models file. ixOS already has its split (Rust owns state, Swift view models are UI-free), so the lesson is
  the consistency, not the shape.
- **Take for Relay.** Group-position bubble shapes; silent success, explicit problems (with the caveat in
  the Relay spec: Relay's mailbox state is meaningful and stays visible); long-press actions as VoiceOver
  actions; Dynamic Type scaled metrics. For ForgeUI: a small semantic token set and nothing more, as Compound
  shows a design system can be.

### Messages: native conversation (platform knowledge; capture owed)

- **Take.** Keyboard and composer behaviour (the transcript keeps its place as the keyboard rises; the
  composer grows with text up to a limit, then scrolls); tapbacks over the bubble; swipe a bubble to reveal
  times; reply as a quoted context above the composer; the attachment button opening a sheet of sources.
- **Don't take.** Its colours, its blue-and-grey bubble identity, or iMessage effects.

### Telegram: interaction breadth (tree read; capture owed)

- **Licence:** GPL-2.0 (per its README: "publish your code too in order to comply with the licences").
  Ideas only.
- **Observed (source tree only):** separate modules for the attachment UI (`AttachmentUI`,
  `AttachmentTextInputPanelNode`) and a reaction selector (`ReactionSelectionNode`).
- **Unverified (general knowledge, not read in the code or captured):** that the attachment picker carries
  a caption field and that reactions appear as a bar above the message's context menu. The Telegram capture
  in the checklist confirms or corrects this.
- **Take for Relay.** Attachment staging with a caption before sending; reactions offered at the top of the
  message's menu; search that reaches into conversation content.
- **Don't take.** Feature density; its custom rendering stack.

## Apple's guidance (read in full)

From [Materials](https://developer.apple.com/design/human-interface-guidelines/materials) and
[Adopting Liquid Glass](https://developer.apple.com/documentation/technologyoverviews/adopting-liquid-glass):

- "Don't use Liquid Glass in the content layer." Standard materials are for the content layer.
- "Use Liquid Glass effects sparingly", limited "to the most important functional elements".
- Clear glass only over visually rich backgrounds, with a dark dimming layer of about 35% over bright
  content.
- Prefer system components and the `.glass` / `.glassProminent` button styles to custom glass; reduce
  custom backgrounds in bars, which interfere with the scroll edge effect.
- Don't crowd or layer glass elements; group them (`GlassEffectContainer`).
- Align control shapes concentrically with the hardware (`ConcentricRectangle`).
- Tab bars may minimise on scroll (Relay uses this in #438).
- Match a context menu's top actions to the item's swipe actions.
- Test with Reduce Transparency, Increase Contrast and the person's chosen Liquid Glass look.

## Comparison with ixOS today (main 5b31a175, with #437 and #438 merged)

| Topic | References | ixOS today | Gap |
|---|---|---|---|
| Glass in the content layer | Never (HIG) | None in content. But two passive notices got glass: ForgeAppKit's Undo banner (#437) and Relay's connection notice (#438) | Both move to standard material in V2.1 ([material table](DESIGN_PHILOSOPHY.md#material-by-component)) |
| Custom glass count | Sparingly (HIG); Luma uses it on transport buttons only | Soundscape: mini-player, Now Playing circles, banner. Relay: composer, connection notice, New Messages, Send | Decided: the banner and the connection notice are status, not controls, so they lose glass; the rest keeps it |
| Custom bars vs system bars | System bars and scroll edge effect (HIG) | Relay's composer is a custom glass card | Try iOS 26's `safeAreaBar` so the system's scroll edge effect keeps text legible; keep the card only if it looks better on the phone |
| Grouping glass | `GlassEffectContainer` (HIG) | Not used | Use it where two glass controls sit together (Now Playing's circle buttons) |
| Three copies of the glass helper | One design system (Compound) | `soundscapeGlass`, `relayFloatingBar`, `BannerSurface` | V2.1 |
| Artwork colour vs control tint | Luma: colour in the background, controls constant | Soundscape: palette in the background; controls white over it | Aligned; keep it a rule |
| Off-screen animation | Luma stops it off screen, in background, under Reduce Motion | Soundscape checks Reduce Motion; no visualiser yet | Rule for any future visualiser |
| Bubble shape by group position | Element X, Messages | Relay groups by author and time but every bubble has the same 18 pt corners | V4.2 |
| Delivery state | Signal: quiet icon in footer; Element X: silent unless failed | Relay: text under the bubble ("Delivered", "Read", "In the Mailbox", "Not Delivered. Tap to Try Again.") | V4.2: quieter form, keep In the Mailbox and Not Delivered explicit |
| Trust events | Signal: system rows in the conversation | Relay: revocation and device lists live on Devices | V4.3: show trust changes in the affected conversation too |
| Long press without the gesture | Element X: VoiceOver action | Relay: `.contextMenu` (VoiceOver gets actions from it) | Check on the phone; add named actions if missing |
| Dynamic Type scaled metrics | Element X `scaledFrame` | Soundscape uses `@ScaledMetric` for transport glyphs | V2.1 helper |
| Reduce Motion | Luma, Doppler declare it | Soundscape yes; Relay no checks | V2.1 |
| Quality budgets | Resona writes them down | None written | DESIGN_QA.md |
| Accessibility declaration | Doppler declares seven | Not declared | DESIGN_QA.md lists ours |

## Licences and reuse

| Reference | Licence | Reuse |
|---|---|---|
| Signal iOS | AGPL-3.0 | No code. Reading for ideas only |
| Element X iOS | AGPL-3.0 or commercial | No code |
| Telegram iOS | GPL-2.0 | No code |
| Luma (knp-org) | AGPL-3.0 | No code |
| iMusic (LinZong) | MIT | Allowed with attribution, but not planned; anything taken goes through the External Projects Atlas first |
| Resona | None published (all rights reserved) | No code; ideas only |
| Plexamp, Marvis Pro, Doppler, Apple Music, Messages | Proprietary | No assets, layouts or icons; behaviour and principles only |

ixOS is MIT. AGPL or GPL code in an ixOS app would oblige the app's whole source under those terms, so
none enters, even a snippet. A reference's idea, written fresh in our own code from our own spec, is fine.

Note: another project called "Luma Music" (GimGyumin/Luma-Music, MPL-2.0, a Navidrome/Jellyfin client) is
not the reference meant here.

## Hands-on capture checklist

Needs a person with the iPhone, about 30 minutes. Screen recordings (Control Centre) rather than single
screenshots where an interaction is named. Save into `/mnt/project-files/design-references/<app>/`.

**Apple Music** (installed): mini-player to Now Playing and back; pause (artwork shrink); lyrics on and off;
queue sheet half and full, reorder one song; album page opening from a grid; long press a song.

**Messages** (installed): type a long message (composer growth); keyboard up and down mid-transcript; reply
to a message; tapback; attach a photo (staging); record and play a voice message; swipe left for
timestamps; a conversation while in Airplane Mode (sending failure).

**Plexamp** (free, needs a Plex server; skip if none): Now Playing, a library radio, queue peek.

**Marvis Pro** (paid) and **Doppler** (trial): home or library customisation in place, smart rules,
queue editing, Now Playing. Optional: only if Reuben wants to buy them; the public descriptions above cover
the principles.

**Signal** (free): a conversation's delivery states (sending, sent, delivered, read), a safety-number view,
a disappearing-messages notice, the attachment picker.

**Element X** (free, a matrix.org account): timeline grouping, a failed send, the offline indicator.

**Telegram** (free): attachment picker, message context menu with reactions, chat folders.

Each capture gets a one-line note: what it shows, which spec section it informs.
