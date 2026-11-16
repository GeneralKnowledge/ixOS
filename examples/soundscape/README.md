# Soundscape

A small multi-track sound sketchpad: record loops, layer them, shape each
with tone and echo, and mix down to a WAV file to share. Its **Listen** tab
plays, compares and picks song takes kept on another computer (see below). The third of the
remaining demo apps ([friction log](../../docs/research/app-friction-log.md));
it runs inside Device Lab (Apps, then Soundscape) and on its own.

## Where it is

| Step | What | State |
|---|---|---|
| SS1 | Rust core (`rust/`): projects, tracks, settings checked and mapped to AVAudioEngine units, waveforms, offline mixdown | Done, tested on Linux |
| SS2 | SwiftUI app: project list, recorder, mixer with Canvas waveforms, AVAudioEngine playback, background audio, hosted in Device Lab | Written; not yet built with xtool or run on a phone |
| L1 | Listen tab: songs and takes from a take server, A/B at matched loudness, stars, notes and picks, lock-screen playback | Rust tested on Linux, also against the real server; Swift written, not yet built with xtool or run on a phone |
| S4 | Spike: one effect in Rust on the real-time audio thread, measured on the phone | Not started; decides whether live effects move to Rust |

## How the work is split

- **AVAudioEngine records and plays.** The recorder writes a WAV file of
  `loop_frames(project, sampleRate)` frames, so it loops cleanly, and hands
  it to `set_clip`. Playback is one `AVAudioPlayerNode` per track, looping
  its clip through an `AVAudioUnitEQ` low-pass band and an
  `AVAudioUnitDelay`, set from `Track.mix` (volume, pan, cutoff in hertz,
  delay in seconds and percent).
- **Rust owns the project and the mixdown.** Settings are checked and saved
  in Rust and mapped to Apple's units in one place
  (`project::mix`), and `mixdown` renders the same chain offline (one-pole
  low-pass, feedback echo, gain, balance), so the exported file sounds like
  playback. A mix over full scale is turned down to fit and says so.
- **A metronome counts in.** Rust renders one pass of the loop's clicks
  (`click_track`, a tick on every beat, the bar's first accented) to a WAV
  file in the caches. The app loops it on its own player straight into the
  main mixer, so it is never recorded or mixed down, and plays its first
  bar as a count-in before every recording. The on/off switch is saved
  with the project.
- Until spike S4 says Rust is safe on the real-time audio thread, live
  effects are Apple's audio units. The Rust filters in `dsp.rs` allocate
  nothing per sample, so they could move there.
- Recordings are files named by their SHA-256 (with `.wav`, which
  AVAudioFile wants). One nothing uses any more is deleted after the change
  that dropped it.

## Rust

```bash
cargo test --manifest-path examples/soundscape/rust/Cargo.toml
```

## Swift

| File | What |
|---|---|
| `Sources/AppHost/SoundscapeModel.swift` | The screens' model: asks the core, keeps alerts, runs waveforms and mixdowns with cancellation. Foundation only |
| `Sources/AppHost/ContentView.swift` | The project list, and the new-project sheet (name, tempo, length) |
| `Sources/AppHost/ProjectView.swift` | One project: the transport (play, stop), tracks, tempo and length, mixdown and sharing |
| `Sources/AppHost/TrackStrip.swift` | One track: waveform, record, mute, solo, echo, level, pan, tone |
| `Sources/AppHost/LoopEngine.swift` | AVAudioEngine: one player, EQ and delay per track from `Track.mix`; records one pass while the others play |

Recording starts with the loop and skips the input and output latency
`AVAudioSession` reports, so a part played along to the loop lines up. It
writes 16-bit PCM at the microphone's rate, which Rust reads.

## Listen

Listen is for generated songs that live on another computer, such as the
band laptop's renders (GeneralKnowledge/JewGrass). That computer runs a
**take server** (`python3 tools/player/serve.py` in JewGrass, standard
library only), which finds the takes, streams them, measures their loudness
and saves picks to `~/band/picks.json`.

- **Rust (`rust/src/listen.rs`)** keeps the server's address (in
  `listen.json`, apart from the projects), asks the server for songs and
  takes through the `http` native service (URLSession on iOS, so it follows
  Tailscale), sends picks, stars and notes, and works out the volumes for
  an A/B comparison at matched loudness (`loudness_match`: the louder take
  is turned down, never by more than 24 dB).
- **Swift** streams with AVPlayer (`TakePlayer.swift`). Comparing plays both
  takes in step and switching only changes which one you hear, so A and B
  are heard at the same moment; Blind hides which is which. Playback goes on
  with the screen locked, with lock-screen controls; "next track" on
  headphones switches A and B.
- The song list is in sections by act when the server says which act a
  song belongs to (`group_songs` in Rust; the act comes off the title if the
  server put it in front), and **Unpicked Only** keeps the songs still
  waiting for a pick. Search matches songs and acts.
- The address must be numeric for plain http (iOS refuses plain http to a
  host name): use the computer's Tailscale address, which starts with 100.

| File | What |
|---|---|
| `Sources/AppHost/ListenModel.swift` | Songs, a song's takes, picks and ratings, the server address. Foundation only |
| `Sources/AppHost/ListenView.swift` | The song list, the server form, a song's takes, the comparison panel, the now-playing bar |
| `Sources/AppHost/TakePlayer.swift` | AVPlayer: one take, or two in step for A/B; the lock screen |

Build and install on its own (it uses the free team's sandbox id):

```bash
forge run ios --project examples/soundscape
forge ui run examples/soundscape/uitests/smoke.toml   # phone plugged in
```
