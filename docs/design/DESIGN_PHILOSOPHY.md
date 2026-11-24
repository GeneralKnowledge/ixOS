# Design philosophy

The principles and decisions behind how ixOS apps look and behave, starting with Soundscape and Relay.
Every design PR names the principles it serves; a decision that contradicts this page changes this page
first.

Sources: Reuben's programme brief and design-research direction (2026-11-24), Apple's Human Interface
Guidelines on [Materials](https://developer.apple.com/design/human-interface-guidelines/materials) and
[Adopting Liquid Glass](https://developer.apple.com/documentation/technologyoverviews/adopting-liquid-glass),
and the reference study in [DESIGN_REFERENCES.md](DESIGN_REFERENCES.md).

## The one rule

**The interface must be beautiful with every glass effect removed.** Hierarchy comes from content, type,
spacing and colour. Glass is a material for the functional layer, not the design identity. A screen that only
works on iOS 26 with glass is not finished.

## Two personalities, one set of principles

| | Soundscape | Relay |
|---|---|---|
| Character | Emotional, expressive, immersive: a record collection, an editorial magazine, a listening room | Calm, precise, understated: a private, dependable place to talk |
| Centre | Music and artwork | People and conversation |
| Colour | Changes with the music (artwork palettes) inside a fixed frame | Fixed, restrained; colour carries meaning (delivery, trust), never mood |
| Motion | Fluid and expressive: crossfades, palette transitions, expansion | Restrained: things arrive and settle, nothing performs |
| Type | Rounded display headings, system text for information | System type only, weight differences rather than size |
| Success looks like | You enjoy looking at it while listening | You enjoy using it because it gets out of your way |

They share principles, tokens for spacing, radius, motion and materials, and the quality bar. They do not
share colour, shapes of content (bubbles, tiles), navigation logic or signature moments.

## Six principles

| Principle | Meaning | Test in review |
|---|---|---|
| Intentional | Every element has a visual and a functional purpose | Can each element on the screen be justified in one sentence? |
| Distinctive | Each app has its own personality | Would the screen be mistaken for the other app, or for Apple's? |
| Atmospheric | Colour, depth, imagery and motion create a feeling | Soundscape: does it feel like the music? Relay: does it feel quiet? |
| Restrained | No unnecessary decoration, gradients or glass | Remove the decoration: is anything lost but noise? |
| Responsive | Motion and layout follow the person's action | Does every animation start from what was touched and stop when interrupted? |
| Accessible | Beauty never costs readability or usability | Does it pass [DESIGN_QA.md](DESIGN_QA.md) section 2 at AX5, VoiceOver, Reduce Motion and Reduce Transparency? |

## Three layers

Apple's guidance separates a content layer from a functional layer that floats above it. ixOS adds a surface
layer between them for organisation.

| Layer | What | Material |
|---|---|---|
| Content | Artwork, songs, albums, messages, conversations, devices | Solid. Never Liquid Glass |
| Surface | Shelves, cards, destinations, contextual panels, overlays | Solid app surfaces; standard materials (not glass) for an overlay that must show what is beneath, as Layered Navigation does ([the contract](../ios-ui.md#layered-navigation-the-contract)) |
| Control | Tab bar, toolbars, mini-player, composer, floating buttons | System components first (they get glass on iOS 26 by themselves); custom glass only for the few most important custom controls |

## Glass rules

From Apple's Materials guidance, made concrete for ixOS:

1. **Not in the content layer.** No glass on bubbles, rows, tiles, lyrics or cards.
2. **System first.** Tab bars, navigation bars, toolbars, sheets, menus and `.glass` / `.glassProminent`
   button styles get glass from the system. A custom `glassEffect` is the exception and needs a reason.
3. **Sparingly.** Apple: limit custom glass to "the most important functional elements". In ixOS: the
   Soundscape mini-player, Now Playing's small controls over artwork, Relay's composer. Everything else
   uses system components.
4. **Regular by default, clear only over rich media.** `.clear` only over artwork or photos, with a dark
   dimming layer (about 35%) when the media can be bright. Soundscape's Now Playing controls sit on the
   artwork palette, already clamped for white glyphs, so they need no extra dim.
5. **No glass on glass,** and group neighbouring glass controls in one `GlassEffectContainer` so they read
   as one element.
6. **Let the system keep text legible**: scroll edge effects over custom backgrounds and padding hacks.
7. **Shapes concentric with the device**: a floating control's corner follows the screen's
   (`ConcentricRectangle` on iOS 26), not an arbitrary radius.
8. **Accessibility settings win.** Reduce Transparency and Increase Contrast must leave every control
   readable; our pre-iOS 26 fallbacks follow the same settings.

## Material by component

Glass marks things a person operates. A notice that only informs is not a control, even when it floats.

| Component | Material |
|---|---|
| Soundscape mini-player | Liquid Glass |
| Now Playing's small controls over artwork | Liquid Glass (clear, over the palette clamped for white glyphs) |
| Relay composer (chat and Assistant) | Liquid Glass |
| New Messages button | Liquid Glass (prominent) |
| Floating toolbar controls (Devices' Send) | Liquid Glass, from the system's toolbar and button styles |
| Tab bars, navigation bars, sheets, menus | The system's own (glass on iOS 26) |
| Relay's connection notice (Connecting, Reconnecting, Not connected) | Standard material or solid |
| Notification banners (ForgeAppKit's Undo banner) | Standard material or solid |
| Soundscape overlays (Up Next, Add to Playlist) | Standard thin material; solid under Reduce Transparency |
| Message bubbles, cards, rows, tiles, lyrics | Solid |

#437 and #438 put glass on the Undo banner and on Relay's connection notice. Both move to standard material in
V2.1; anything not in this table defaults to solid until this table says otherwise.

## Shared themes, from the references

| Theme | Soundscape | Relay |
|---|---|---|
| Content first | Artwork and music lead | Conversation leads |
| Clear hierarchy | Track, then album, then controls | People, then messages, then actions |
| Restraint | Don't decorate every surface | Don't interrupt every conversation |
| Continuity | Playback survives navigation, restarts and errors | Messages and connection state stay coherent across reconnects and restarts |
| Feedback | Playback responds at once to every tap | Delivery and connection state are clear, quiet and truthful |
| Personalisation | Library and listening preferences shape the home | Identity and communication preferences, never at the cost of trust |
| Reliability | Queue, imports, background playback | Delivery, reconnection, device trust |

## What we deliberately avoid

- Copying any app's visual identity (Spotify's, Apple Music's, Signal's) or its exact layouts and assets.
- Translucency everywhere because Liquid Glass exists.
- In Relay: gradients, oversized bubbles, decorative animation, alarming alerts for ordinary network events.
- Custom gestures without a visible, accessible alternative.
- Treating screenshots as evidence that a feature works. Screenshots show appearance; tests and phone
  checks show behaviour.
- Beautiful filled screens without designed loading, empty, error, offline and accessibility states.
- Shared components that exist to maximise reuse rather than because both apps need them.
- **A visual redesign PR quietly becoming a feature PR.** If a design needs a new API, a persistence
  mechanism, a Rust model change or a behaviour change, that part is split into its own separately reviewed
  PR first; the visual PR presents only data that already exists. Known cases are listed in the
  [programme](README.md#visual-work-and-feature-work-stay-apart).

## How a design change reaches the product

1. **Reference audit**: what the references do, from their source where open and from hands-on capture
   where not ([DESIGN_REFERENCES.md](DESIGN_REFERENCES.md)). The Apple Music, Messages and Signal captures
   are required before any major visual implementation.
2. **Principle extraction**: why it works, where it falls short, what applies to ixOS (same page).
3. **Design system**: tokens and conventions ([DESIGN_TOKENS.md](DESIGN_TOKENS.md)).
4. **Screen specification**: every important screen with all its states
   ([SOUNDSCAPE_DESIGN_SPEC.md](SOUNDSCAPE_DESIGN_SPEC.md), [RELAY_DESIGN_SPEC.md](RELAY_DESIGN_SPEC.md)),
   then an **approved screen composition** (a wireframe or mock-up Reuben has approved) for each screen
   before it is built. Written descriptions alone are not enough to build from.
5. **Implementation and comparison**: small PRs, screenshots compared against the spec, real interactions
   tested on the phone.
6. **Release-quality review**: consistency, performance, accessibility, navigation, persistence and
   regression checks before a screen is called done ([DESIGN_QA.md](DESIGN_QA.md)).

No code is copied from a reference. Learning from public source is fine; reusing it needs a licence review
first, and AGPL and GPL code (Signal, Element X, Telegram, Luma) is not reused at all (see
[DESIGN_REFERENCES.md](DESIGN_REFERENCES.md#licences-and-reuse)).
