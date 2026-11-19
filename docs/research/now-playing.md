# Now Playing: one helper for the lock screen, headphones and interruptions

Plan, 17 November 2026. Reuben said yes to it on 2026-11-17 after the [framework candidates](framework-candidates.md) survey. It is phase 5 of [everyday capabilities](everyday-capabilities.md#phase-5-now-playing-helper) and passes the [extraction gates](../roadmap.md): two apps use it, and both passed on the iPhone today. Soundscape's lock screen and interruptions passed with #306, and Pocket Library's Read Aloud showed on the lock screen in build 1.29915595, with details fixed in #313.

## What the apps do today

| | Soundscape (songs) | Pocket Library (Read Aloud) | Relay (voice messages) |
|---|---|---|---|
| Plays with | `AVPlayer` (two, for A/B) | `AVSpeechSynthesizer` | the framework's audio player (`ForgeServices+Audio`) |
| Audio session | sets `AVAudioSession` itself: `.playback`, default mode | `ForgeAudioSession.playback` (spoken audio) | `ForgeAudioSession.playback` |
| Lock screen info | title, artist, album, artwork, duration, position, rate | title, book, cover, estimated duration and position (#313) | none (short clips) |
| Lock screen and headphone buttons | play, pause, toggle, seek, back 10 s, next and previous (or A/B) | play, pause, toggle, next and previous paragraph | none |
| Call, Siri, alarm | pauses, and resumes if iOS says so | **nothing in the code**: whatever iOS does by default (not yet seen on the phone) | nothing in the code |
| Headphones unplugged | pauses | **nothing in the code**: it would carry on out loud from the speaker (not yet seen on the phone) | pauses |

So the two apps hand-write the same three things: the Now Playing info, the remote command targets, and the interruption and unplug rules. Soundscape's copy is complete and passed on the phone. Pocket Library's is missing the system rules, which is the bug found on 2026-11-17 (from the code; not yet seen on the phone).

## What moves into the framework

Swift only, next to `ForgeAudioSession` in `crates/forge-services/swift/helpers/` (copied into each app's `Generated/` the same way), because it wraps MediaPlayer and AVFoundation and Rust never reads it.

1. **`ForgeAudioSession.music`.** Playback in the default mode, for songs. `.playback` stays spoken audio. Soundscape stops setting the session itself (the lag listed in [framework-candidates.md](framework-candidates.md), item 5).
2. **`ForgeNowPlaying`**, one per playing screen, on the main actor:
   - `update(_ info:)` takes a small value (title, artist, album, artwork image, duration, position, rate) and writes `MPNowPlayingInfoCenter`, with the media type and default rate set every time. A missing duration or position was what left Read Aloud's lock screen blank.
   - `commands(...)` takes the handlers the app has (play, pause, toggle, next, previous, seek, skip back and forward with their intervals), turns those on and leaves the rest off, so the lock screen shows only buttons that work.
   - It follows the system on the app's behalf: an interruption that begins calls `pause`. One that ends with "should resume" calls `play`, but only if it was playing before. Headphones or AirPods going away call `pause`. Soundscape's rules, as they passed on the phone.
   - `end()` clears the info, removes the targets and stops observing. The reader and the player call it when they close.
3. **A rule-only part that tests on Linux.** The decision "what to do for this interruption or route change, given whether we were playing" is a plain function with a test in the forge-services Swift package, so the rules are checked without a phone.

What stays in the apps: the players themselves (`AVPlayer`, `AVSpeechSynthesizer`), Soundscape's queue, shuffle, repeat and A/B comparison, Read Aloud's speed, voices, block highlighting and its estimated times. There is no Forge audio player and no queue in the framework.

## The apps move in the same PR

- **Soundscape:** `TakePlayer` uses `ForgeNowPlaying` and `.music`. Its next and previous keep meaning "switch A/B" during a comparison, which the app decides in its handlers. The Studio's `LoopEngine` keeps its own interruption handling until the Studio screens are removed (Soundscape PR 6). It is listed as lag with that reason.
- **Pocket Library:** `ReadAloud` uses `ForgeNowPlaying`, and so gains pausing for calls and Siri, resuming after them, and pausing when headphones come out.
- **Rule `now-playing`** in `scripts/check-app-parity.sh`: no `MPNowPlayingInfoCenter`, `MPRemoteCommandCenter`, or interruption or route-change observer in an app's Swift. Device Lab's `AudioMonitor` only records these events for its dashboard; it is marked not applicable for that reason.
- **Behaviour tests**, per Reuben's rule that a parity rule is paired with a test of correct use:
  - the interruption and route rules (Linux);
  - Read Aloud's lock-screen times, which already exist;
  - a model test that Read Aloud's pause and resume keep its place.

## Read Aloud chapters (Reuben's suggestion, decide before the PR)

Reuben suggested Read Aloud could play as chapters. With the helper this is small:

- the lock screen's next and previous jump a chapter;
- skip forward and back (with an interval of one paragraph) move a paragraph, as next and previous do now;
- the progress bar covers the chapter, as it already does;
- at the end of a chapter, reading carries on into the next one.

Recommended: yes, in the same PR, because carrying on into the next chapter is what a listener expects with the phone locked. Today Read Aloud stops at the end of each chapter.

## Phone check (one sitting, about 10 minutes)

In Device Lab:

1. Soundscape plays a song. Lock the phone: the details are right, and play, pause, seek and next work.
2. Start a call or Siri while it plays: it pauses, then resumes. Unplug headphones (or take out AirPods): it pauses.
3. Pocket Library: the same three for Read Aloud, plus next chapter from the lock screen if chapters are in.

## Order and size

One PR after this plan merges: the helper, both apps, the rule and the tests. The Device Lab build covers both apps. It doesn't touch the photo picker, which still waits for Soundscape's artwork picker to pass on the phone.
