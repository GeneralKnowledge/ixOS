//! Reading, shaping and writing sound, with no threads and no state beyond
//! a buffer, so every rule is tested on Linux.
//!
//! Runs offline (waveforms, the mixdown), never on the real-time audio
//! thread: live playback uses AVAudioEngine's own units with the numbers
//! from [`crate::project::mix`]. Whether Rust may run on the audio thread is
//! spike S4's question; these filters are written so they could.

use std::f64::consts::PI;
use std::path::Path;

use crate::model::{Echo, SoundscapeError};

/// The longest recording Soundscape takes, in seconds.
pub const MAX_CLIP_SECONDS: u64 = 5 * 60;

/// A stereo frame: left, right.
pub type Frame = [f32; 2];

/// What a WAV file holds, before reading its samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
}

/// Checks that `path` is a WAV file Soundscape can use and says what it is.
pub fn inspect(path: &Path) -> Result<ClipInfo, SoundscapeError> {
    let reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let info = ClipInfo {
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        frames: u64::from(reader.duration()),
    };
    let bad = |reason: &str| {
        Err(SoundscapeError::Audio {
            reason: reason.into(),
        })
    };
    if !(8_000..=192_000).contains(&info.sample_rate) {
        return bad("its sample rate isn't one Soundscape plays");
    }
    if info.channels == 0 {
        return bad("it has no channels");
    }
    if info.frames == 0 {
        return bad("it is empty");
    }
    if info.frames / u64::from(info.sample_rate) > MAX_CLIP_SECONDS {
        return bad("it is longer than five minutes");
    }
    Ok(info)
}

/// Reads a WAV file as stereo frames: mono is copied to both sides, and
/// past two channels only the first two are kept.
pub fn read(path: &Path) -> Result<(Vec<Frame>, u32), SoundscapeError> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels.max(1));
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|s| s as f32 * scale))
                .collect::<Result<_, _>>()?
        }
    };
    let frames = samples
        .chunks_exact(channels)
        .map(|f| [f[0], if channels > 1 { f[1] } else { f[0] }])
        .collect();
    Ok((frames, spec.sample_rate))
}

/// Writes stereo 16-bit WAV.
pub fn write(path: &Path, frames: &[Frame], sample_rate: u32) -> Result<(), SoundscapeError> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for frame in frames {
        for sample in frame {
            let s = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
            writer.write_sample(s)?;
        }
    }
    writer.finalize()?;
    Ok(())
}

/// Changes the sample rate by straight-line interpolation: plain, and good
/// enough for sketching. Same rate is a copy.
pub fn resample(frames: &[Frame], from: u32, to: u32) -> Vec<Frame> {
    if from == to || frames.is_empty() {
        return frames.to_vec();
    }
    let ratio = f64::from(from) / f64::from(to);
    let out_len = ((frames.len() as f64) / ratio).round().max(1.0) as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let at = (pos.floor() as usize).min(frames.len() - 1);
            let next = (at + 1).min(frames.len() - 1);
            let t = (pos - at as f64) as f32;
            [
                frames[at][0] + (frames[next][0] - frames[at][0]) * t,
                frames[at][1] + (frames[next][1] - frames[at][1]) * t,
            ]
        })
        .collect()
}

/// The loudest sample in each of `buckets` equal slices, from 0 to 1: a
/// waveform to draw.
pub fn peaks(frames: &[Frame], buckets: usize) -> Vec<f32> {
    if frames.is_empty() || buckets == 0 {
        return Vec::new();
    }
    let buckets = buckets.min(frames.len());
    (0..buckets)
        .map(|b| {
            let start = b * frames.len() / buckets;
            let end = ((b + 1) * frames.len() / buckets).max(start + 1);
            frames[start..end]
                .iter()
                .map(|f| f[0].abs().max(f[1].abs()))
                .fold(0.0, f32::max)
                .min(1.0)
        })
        .collect()
}

/// Left and right gains for a pan from -1 to 1. A stereo source is
/// balanced, as `AVAudioPlayerNode.pan` does: the far side fades, the near
/// side stays.
pub fn balance(pan: f32) -> (f32, f32) {
    let pan = pan.clamp(-1.0, 1.0);
    ((1.0 - pan).min(1.0), (1.0 + pan).min(1.0))
}

/// A one-pole low-pass filter per side.
pub struct LowPass {
    a: f32,
    state: Frame,
}

impl LowPass {
    pub fn new(cutoff_hz: f32, sample_rate: u32) -> Self {
        let a = 1.0 - (-2.0 * PI * f64::from(cutoff_hz) / f64::from(sample_rate)).exp();
        Self {
            a: a as f32,
            state: [0.0; 2],
        }
    }

    pub fn process(&mut self, frame: Frame) -> Frame {
        for (s, x) in self.state.iter_mut().zip(frame) {
            *s += self.a * (x - *s);
        }
        self.state
    }
}

/// A feedback echo, like `AVAudioUnitDelay`: the output is the dry signal
/// and the delay line mixed by `wet` (0 to 1).
pub struct EchoLine {
    buffer: Vec<Frame>,
    at: usize,
    feedback: f32,
    wet: f32,
}

impl EchoLine {
    pub fn new(echo: &Echo, delay_seconds: f64, sample_rate: u32) -> Self {
        let len = ((delay_seconds * f64::from(sample_rate)).round() as usize).max(1);
        Self {
            buffer: vec![[0.0; 2]; len],
            at: 0,
            feedback: echo.feedback as f32,
            wet: echo.mix as f32,
        }
    }

    pub fn process(&mut self, frame: Frame) -> Frame {
        let delayed = self.buffer[self.at];
        let mut out = [0.0; 2];
        for side in 0..2 {
            self.buffer[self.at][side] = frame[side] + delayed[side] * self.feedback;
            out[side] = frame[side] * (1.0 - self.wet) + delayed[side] * self.wet;
        }
        self.at = (self.at + 1) % self.buffer.len();
        out
    }
}

/// The loudest sample, as a fraction of full scale.
pub fn peak(frames: &[Frame]) -> f32 {
    frames
        .iter()
        .map(|f| f[0].abs().max(f[1].abs()))
        .fold(0.0, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f64, rate: u32, len: usize) -> Vec<Frame> {
        (0..len)
            .map(|i| {
                let s = (2.0 * PI * hz * i as f64 / f64::from(rate)).sin() as f32;
                [s, s]
            })
            .collect()
    }

    #[test]
    fn wav_round_trips_and_mono_fills_both_sides() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mono.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 22_050,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for s in [0i16, 16_384, -16_384] {
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
        let info = inspect(&path).unwrap();
        assert_eq!((info.channels, info.frames), (1, 3));
        let (frames, rate) = read(&path).unwrap();
        assert_eq!(rate, 22_050);
        assert_eq!(frames[1], [0.5, 0.5]);

        let out = dir.path().join("out.wav");
        write(&out, &frames, rate).unwrap();
        let (back, _) = read(&out).unwrap();
        assert!((back[2][1] + 0.5).abs() < 0.001);
    }

    #[test]
    fn not_a_wav_is_explained() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.wav");
        std::fs::write(&path, b"not audio").unwrap();
        assert!(matches!(inspect(&path), Err(SoundscapeError::Audio { .. })));
    }

    #[test]
    fn resampling_keeps_the_length_in_time() {
        let frames = sine(440.0, 48_000, 48_000);
        let out = resample(&frames, 48_000, 44_100);
        assert_eq!(out.len(), 44_100);
        assert!((peak(&out) - 1.0).abs() < 0.01);
    }

    #[test]
    fn low_pass_cuts_highs_and_keeps_lows() {
        let rate = 44_100;
        let mut lp = LowPass::new(500.0, rate);
        let high: Vec<Frame> = sine(8_000.0, rate, 4_410)
            .into_iter()
            .map(|f| lp.process(f))
            .collect();
        let mut lp = LowPass::new(500.0, rate);
        let low: Vec<Frame> = sine(50.0, rate, 4_410)
            .into_iter()
            .map(|f| lp.process(f))
            .collect();
        assert!(peak(&high[2_000..]) < 0.1, "{}", peak(&high[2_000..]));
        assert!(peak(&low[2_000..]) > 0.9);
    }

    #[test]
    fn echo_repeats_after_the_delay() {
        let echo = Echo {
            beats: 1.0,
            feedback: 0.5,
            mix: 0.5,
        };
        let mut line = EchoLine::new(&echo, 0.001, 10_000); // 10 frames
        let mut input = vec![[0.0f32; 2]; 30];
        input[0] = [1.0, 1.0];
        let out: Vec<Frame> = input.into_iter().map(|f| line.process(f)).collect();
        assert_eq!(out[0][0], 0.5);
        assert_eq!(out[10][0], 0.5);
        assert_eq!(out[20][0], 0.25);
        assert_eq!(out[5][0], 0.0);
    }

    #[test]
    fn balance_and_peaks() {
        assert_eq!(balance(0.0), (1.0, 1.0));
        assert_eq!(balance(-1.0), (1.0, 0.0));
        assert_eq!(balance(0.5), (0.5, 1.0));
        let frames = vec![[0.1, -0.2], [0.9, 0.0], [0.0, 0.3], [0.0, 0.0]];
        assert_eq!(peaks(&frames, 2), vec![0.9, 0.3]);
        assert_eq!(peaks(&frames, 100).len(), 4);
    }
}
