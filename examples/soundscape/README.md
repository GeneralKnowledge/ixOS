# Soundscape

A small multi-track sound sketchpad: record loops, layer them, shape each
with tone and echo, and mix down to a WAV file to share. The third of the
remaining demo apps ([friction log](../../docs/research/app-friction-log.md));
it runs inside Device Lab once its SwiftUI app exists.

## Where it is

| Step | What | State |
|---|---|---|
| SS1 | Rust core (`rust/`): projects, tracks, settings checked and mapped to AVAudioEngine units, waveforms, offline mixdown | Done, tested on Linux |
| SS2 | SwiftUI app: project list, recorder, mixer with Canvas waveforms, AVAudioEngine playback, background audio, hosted in Device Lab | Next |
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
