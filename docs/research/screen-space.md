# Screen space: bars that float over content

Plan, 17 November 2026. Reuben asked after Soundscape's mini-player hid the last song of a long album: "Does this not become perceived screen space versus actual screen space?" It does. A screen's layout believes an area is visible when something floating on top of it covers it. This plan surveys every app for that kind of bug and proposes one framework way for a floating bar to claim its space.

## How SwiftUI decides what is visible

A list or scroll view leaves room only for the **safe area**: the status bar, the home indicator, the navigation and tab bars, and anything added with `.safeAreaInset`. An `.overlay` or a `ZStack` layer takes no room. Content scrolls under it, and the last rows can never come out from under it.

The catch Soundscape found: **an inset added to a `NavigationStack` or `TabView` doesn't reliably reach the screens inside it.** The Library's mini-player was a `.safeAreaInset` on the stack. On the iPhone, a pushed album's list still ran under it (fixed in #320, merged 2026-11-18; from the code comment and the phone report). The inset reached the stack's own frame but not the lists of the screens pushed onto it.

So there are two separate mistakes:

1. **An overlay instead of an inset.** The content never knows about the bar.
2. **An inset on the container.** The content of pushed screens, and of a paged `TabView`'s pages, may not know about the bar.

## Survey of the apps (2026-11-17, from the code)

| App | What floats | How | Covers content? |
|---|---|---|---|
| Soundscape | the mini-player over every Library screen | was an inset on the stack; #320 (merged) made it an overlay that measures itself, puts its height in the environment, and adds `.miniPlayerClearance()` to each of the 11 screens | Was (seen on the phone); fixed by #320. Each new screen must now remember the modifier |
| Commonplace | the Undo banner after Mark All as Read (ForgeAppKit's `forgeBanner`) | an inset on the `NavigationStack`, while Mark All as Read is on a **pushed** timeline | **Likely**: the same arrangement as Soundscape before #320. While the banner shows, the timeline's last articles probably can't scroll clear of it. Not seen on the phone |
| Weather | the bottom bar (places, the page dots) | an inset on the paged `TabView`; each page's scroll view pads 24 points at its end | **To check**: whether a paged `TabView` passes the inset to its pages. If not, the footer and the end of the details sit under the bar |
| Relay | the "new messages" button in a chat | an overlay at the bottom of the transcript | No: it shows only when the newest messages are scrolled out of view, and it scrolls to them |
| Relay, Pocket Library, Good Dog, Soundscape's Studio | composers, the reader's progress bar and player, session and goal footers, the transport | an inset on the scrolling view itself | No: the right way |
| Device Lab | the workshop button over every hosted app | an overlay, on purpose | Accepted: it can be moved to another edge, it is lab-only, and UI tests mask it |
| Sketchbook, Device Lab's compass | a note over the canvas, the lubber line | overlays on views that don't scroll | No |

Live instances:
- **Soundscape:** fixed by #320, but by hand.
- **Commonplace's Undo banner:** likely. This is in the framework itself: `forgeBanner`'s documentation says lists get room, and that holds only for the screen it is attached to.
- **Weather's bottom bar:** to check on the phone.

## What changes

### 1. ForgeUI: `forgeBar`, one way to float a bar

`.forgeBar(edge: .bottom) { NowPlayingBar(...) }` goes on the container (the stack, or each tab's content):

- It draws the bar over the content, measures the bar's height, and **gives that height to every scroll view below as a content margin** (`.contentMargins(.bottom, height, for: .scrollContent)` and the same for the scroll indicators). SwiftUI passes content margins down through the environment, so they also reach pushed screens and pages. A screen added later is covered without anyone remembering a modifier.
- With no bar (nothing playing), the margin is 0.
- `forgeBanner` is built on it, which fixes Commonplace's Undo banner and every later banner user.

**First, a spike on the phone.** Whether content margins set on a stack reach a pushed `List` on iOS 26 is a guess until the iPhone shows it. The spike is one screen in ForgeUI's showcase: a stack with a bar, a pushed long list, a paged `TabView` and a horizontal shelf inside a list. If margins don't pass down, the fallback is #320's design moved into ForgeUI: the bar publishes its height in the environment, and `.forgeBarClearance()` is added by each screen, with a rule to check it.

**One known side effect to check in the spike:** content margins reach *every* scroll view below, including a horizontal shelf inside a list (Weather's hourly strip, Soundscape's album rows). A horizontal scroller would gain empty space under it. If the spike shows that, `forgeBar` limits the margin to vertical scrolling, or ForgeUI's horizontal shelf resets it.

### 2. Apple's own mini-player slot where it fits

iOS 26 has a slot for exactly this: `.tabViewBottomAccessory`, the bar Music shows above the tab bar, with the insets handled by the system. Soundscape's player sits in a `TabView` today. But the phone runs iOS 26 while the apps support iOS 17, and Soundscape's Studio tab goes away in its PR 6, which may leave the Library without a `TabView`. So `forgeBar` uses the accessory on iOS 26 when it is on a `TabView`, and its own margin otherwise. That is decided in the spike, not assumed.

### 3. The apps move

- **Soundscape:** the mini-player becomes `.forgeBar`, and the 11 `.miniPlayerClearance()` calls and the environment key from #320 are removed.
- **Commonplace:** nothing to change in the app. `forgeBanner` is fixed underneath it.
- **Weather:** if the phone shows the bottom bar covering the page, the bar becomes `.forgeBar`.

### 4. Keeping it from coming back

- **Parity rule `floating-bars`:** no `.safeAreaInset` on a `NavigationStack` or `TabView` in an app, and no app-made clearance (an environment height plus padding). Use `forgeBar`. A bottom- or top-aligned `.overlay` holding controls is listed with its reason where it is meant to cover: Relay's "new messages" button and Device Lab's workshop button.
- **A behaviour test, on the phone through `forge ui`:** scroll a long list under a bar to its end, tap the last row, and expect its screen to open. If the bar covers the row, the tap lands on the bar (Undo, or Now Playing) and the scenario fails.
  - Soundscape: a long album, with a song playing.
  - Commonplace: the timeline, with the Undo banner showing. This extends `uitests/mark-all-read.toml`.

  It uses only the scroll, tap and expect steps scenarios already have, so it needs nothing new in the frozen UI-test tooling. A static rule proves `forgeBar` is used; this test proves the last row can be reached.

## Order and size

1. **Spike in the ForgeUI showcase:** one screen, one phone look of about 5 minutes. It decides between content margins and the fallback, and whether to use the iOS 26 accessory.
2. **One PR after the spike:**
   - `forgeBar`, with `forgeBanner` moved onto it;
   - Soundscape (and Weather if needed) moved onto it;
   - the rule and the two UI scenarios.

   Phone: the two scenarios, plus a look at Weather's last card.

Neither step touches the Now Playing or coming-back work.

## Phone checks now (before any code)

These would confirm the two suspected instances. They are items 21 and 22 on the shared checklist:

1. **Commonplace:** open a feed with many articles, then Mark All as Read. While the Undo banner shows, scroll to the end. Is the last article fully visible above the banner?
2. **Weather:** on a page, scroll to the very end. Is the footer fully visible above the bottom bar?
