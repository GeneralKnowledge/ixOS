//! What a project is, what each change does, what each setting means in
//! AVAudioEngine's units, and how the tracks mix down. No files and no
//! threads: the core loads recordings and hands them in.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::dsp::{self, EchoLine, Frame, LowPass};
use crate::model::{
    clean_name, Echo, SoundscapeError, TrackMix, TrackSettings, MAX_BARS, MAX_GAIN_DB, MAX_TEMPO,
    MAX_TRACKS, MIN_GAIN_DB, MIN_TEMPO,
};

/// The saved document's version.
pub const VERSION: u32 = 1;
/// `AVAudioUnitDelay` delays at most two seconds.
pub const MAX_DELAY_SECONDS: f64 = 2.0;
/// The tone control sweeps the low-pass cutoff from here...
const LOW_PASS_MIN_HZ: f64 = 200.0;
/// ...to here, on a log scale.
const LOW_PASS_MAX_HZ: f64 = 20_000.0;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Projects {
    #[serde(default = "first_id")]
    pub next_id: u64,
    #[serde(default)]
    pub projects: Vec<Project>,
}

fn first_id() -> u64 {
    1
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Project {
    pub id: u64,
    pub title: String,
    pub created_ms: i64,
    pub modified_ms: i64,
    pub tempo_bpm: f64,
    pub bars: u32,
    pub tracks: Vec<SavedTrack>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SavedTrack {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub clip: Option<SavedClip>,
    #[serde(default)]
    pub settings: TrackSettings,
}

/// A recording: its file in the recordings store (named by SHA-256).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SavedClip {
    pub file: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
}

impl Projects {
    pub fn get(&self, id: u64) -> Result<&Project, SoundscapeError> {
        self.projects
            .iter()
            .find(|p| p.id == id)
            .ok_or(SoundscapeError::NotFound { id })
    }

    fn get_mut(&mut self, id: u64) -> Result<&mut Project, SoundscapeError> {
        self.projects
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or(SoundscapeError::NotFound { id })
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    /// A new project with one empty track; returns its id.
    pub fn create(
        &mut self,
        title: &str,
        tempo_bpm: f64,
        bars: u32,
        now_ms: i64,
    ) -> Result<u64, SoundscapeError> {
        let title = clean_name(title)?;
        check_timing(tempo_bpm, bars)?;
        let id = self.take_id();
        let track = self.take_id();
        self.projects.push(Project {
            id,
            title,
            created_ms: now_ms,
            modified_ms: now_ms,
            tempo_bpm,
            bars,
            tracks: vec![new_track(track, "Track 1".into())],
        });
        Ok(id)
    }

    pub fn delete(&mut self, id: u64) -> Result<(), SoundscapeError> {
        let before = self.projects.len();
        self.projects.retain(|p| p.id != id);
        if before == self.projects.len() {
            return Err(SoundscapeError::NotFound { id });
        }
        Ok(())
    }

    /// Changes a project and marks it modified.
    pub fn edit<T>(
        &mut self,
        id: u64,
        now_ms: i64,
        change: impl FnOnce(&mut Project) -> Result<T, SoundscapeError>,
    ) -> Result<T, SoundscapeError> {
        let project = self.get_mut(id)?;
        let value = change(project)?;
        project.modified_ms = now_ms;
        Ok(value)
    }

    /// Adds an empty track; returns its id.
    pub fn add_track(
        &mut self,
        id: u64,
        name: Option<&str>,
        now_ms: i64,
    ) -> Result<u64, SoundscapeError> {
        let track = self.next_id.max(1);
        self.edit(id, now_ms, |p| {
            if p.tracks.len() >= MAX_TRACKS as usize {
                return Err(SoundscapeError::TooManyTracks { max: MAX_TRACKS });
            }
            let name = match name {
                Some(name) => clean_name(name)?,
                None => next_track_name(p),
            };
            p.tracks.push(new_track(track, name));
            Ok(())
        })?;
        self.next_id = track + 1;
        Ok(track)
    }

    /// Every recording a project uses.
    pub fn files(&self) -> HashSet<String> {
        self.projects
            .iter()
            .flat_map(|p| p.tracks.iter())
            .filter_map(|t| t.clip.as_ref().map(|c| c.file.clone()))
            .collect()
    }
}

impl Project {
    pub fn track_mut(&mut self, id: u64) -> Result<&mut SavedTrack, SoundscapeError> {
        self.tracks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(SoundscapeError::NotFound { id })
    }

    pub fn remove_track(&mut self, id: u64) -> Result<(), SoundscapeError> {
        let before = self.tracks.len();
        self.tracks.retain(|t| t.id != id);
        if before == self.tracks.len() {
            return Err(SoundscapeError::NotFound { id });
        }
        Ok(())
    }

    pub fn set_timing(&mut self, tempo_bpm: f64, bars: u32) -> Result<(), SoundscapeError> {
        check_timing(tempo_bpm, bars)?;
        self.tempo_bpm = tempo_bpm;
        self.bars = bars;
        Ok(())
    }

    /// One pass of the loop, in seconds (four beats to the bar).
    pub fn loop_seconds(&self) -> f64 {
        loop_seconds(self.tempo_bpm, self.bars)
    }

    fn any_solo(&self) -> bool {
        self.tracks.iter().any(|t| t.settings.solo)
    }

    /// Whether `track` is heard: not muted, and soloed if anything is.
    pub fn audible(&self, track: &SavedTrack) -> bool {
        !track.settings.muted && (track.settings.solo || !self.any_solo())
    }
}

pub fn loop_seconds(tempo_bpm: f64, bars: u32) -> f64 {
    f64::from(bars) * 4.0 * 60.0 / tempo_bpm
}

fn check_timing(tempo_bpm: f64, bars: u32) -> Result<(), SoundscapeError> {
    if !(MIN_TEMPO..=MAX_TEMPO).contains(&tempo_bpm) {
        return Err(SoundscapeError::range("tempo"));
    }
    if !(1..=MAX_BARS).contains(&bars) {
        return Err(SoundscapeError::range("bars"));
    }
    Ok(())
}

/// Checks a track's settings.
pub fn check_settings(s: &TrackSettings) -> Result<(), SoundscapeError> {
    if !(MIN_GAIN_DB..=MAX_GAIN_DB).contains(&s.gain_db) {
        return Err(SoundscapeError::range("gain"));
    }
    if !(-1.0..=1.0).contains(&s.pan) {
        return Err(SoundscapeError::range("pan"));
    }
    if !(0.0..=1.0).contains(&s.tone) {
        return Err(SoundscapeError::range("tone"));
    }
    if let Some(echo) = &s.echo {
        if !(echo.beats > 0.0 && echo.beats <= 4.0) {
            return Err(SoundscapeError::range("echo time"));
        }
        if !(0.0..=0.9).contains(&echo.feedback) {
            return Err(SoundscapeError::range("echo feedback"));
        }
        if !(0.0..=1.0).contains(&echo.mix) {
            return Err(SoundscapeError::range("echo mix"));
        }
    }
    Ok(())
}

fn new_track(id: u64, name: String) -> SavedTrack {
    SavedTrack {
        id,
        name,
        clip: None,
        settings: TrackSettings::default(),
    }
}

fn next_track_name(project: &Project) -> String {
    (1..)
        .map(|n| format!("Track {n}"))
        .find(|name| project.tracks.iter().all(|t| &t.name != name))
        .expect("an unused name")
}

/// A track's settings in AVAudioEngine's units.
pub fn mix(settings: &TrackSettings, audible: bool, tempo_bpm: f64) -> TrackMix {
    let echo = settings.echo.as_ref();
    TrackMix {
        volume: if audible {
            10f64.powf(settings.gain_db / 20.0) as f32
        } else {
            0.0
        },
        pan: settings.pan as f32,
        low_pass_hz: low_pass_hz(settings.tone),
        delay_seconds: echo.map(|e| delay_seconds(e, tempo_bpm)),
        delay_feedback_percent: echo.map_or(0.0, |e| (e.feedback * 100.0) as f32),
        delay_wet_dry_percent: echo.map_or(0.0, |e| (e.mix * 100.0) as f32),
    }
}

/// The cutoff for a tone from 0 to 1, or `None` at 1 (no filter).
fn low_pass_hz(tone: f64) -> Option<f32> {
    (tone < 1.0).then(|| (LOW_PASS_MIN_HZ * (LOW_PASS_MAX_HZ / LOW_PASS_MIN_HZ).powf(tone)) as f32)
}

/// An echo's time in seconds at a tempo, within `AVAudioUnitDelay`'s range.
pub fn delay_seconds(echo: &Echo, tempo_bpm: f64) -> f64 {
    (echo.beats * 60.0 / tempo_bpm).min(MAX_DELAY_SECONDS)
}

/// A loaded recording for the mixdown.
pub struct Loaded {
    pub track: u64,
    pub frames: Vec<Frame>,
    pub sample_rate: u32,
}

/// What the mix came to, before writing.
pub struct Mixed {
    pub frames: Vec<Frame>,
    pub peak: f32,
    pub limited: bool,
}

/// Mixes the audible tracks of `project` for `passes` passes of the loop
/// at `sample_rate`. Each recording loops from its start, cut or padded
/// with silence to the loop's length, then goes through its tone, echo,
/// gain and pan, as live playback does. A mix over full scale is turned
/// down to fit. `stopped` is asked between tracks.
pub fn mixdown(
    project: &Project,
    clips: &[Loaded],
    sample_rate: u32,
    passes: u32,
    stopped: impl Fn() -> bool,
) -> Result<Mixed, SoundscapeError> {
    let loop_len = (project.loop_seconds() * f64::from(sample_rate)).round() as usize;
    let total = loop_len * passes.max(1) as usize;
    let mut out = vec![[0.0f32; 2]; total];
    let mut heard = false;
    for track in project.tracks.iter().filter(|t| project.audible(t)) {
        if stopped() {
            return Err(SoundscapeError::Stopped {
                kind: forge_app::ErrorKind::Cancelled,
            });
        }
        let Some(clip) = clips.iter().find(|c| c.track == track.id) else {
            continue;
        };
        heard = true;
        let source = dsp::resample(&clip.frames, clip.sample_rate, sample_rate);
        let m = mix(&track.settings, true, project.tempo_bpm);
        let (left, right) = dsp::balance(m.pan);
        let mut low_pass = m.low_pass_hz.map(|hz| LowPass::new(hz, sample_rate));
        let mut echo = track
            .settings
            .echo
            .as_ref()
            .map(|e| EchoLine::new(e, delay_seconds(e, project.tempo_bpm), sample_rate));
        for (i, frame) in out.iter_mut().enumerate() {
            let at = i % loop_len.max(1);
            let mut x = source.get(at).copied().unwrap_or([0.0; 2]);
            if let Some(lp) = &mut low_pass {
                x = lp.process(x);
            }
            if let Some(e) = &mut echo {
                x = e.process(x);
            }
            frame[0] += x[0] * m.volume * left;
            frame[1] += x[1] * m.volume * right;
        }
    }
    if !heard {
        return Err(SoundscapeError::Silent);
    }
    let peak = dsp::peak(&out);
    let limited = peak > 1.0;
    if limited {
        // Just under full scale, so 16-bit rounding can't clip.
        let scale = 0.98 / peak;
        for frame in &mut out {
            frame[0] *= scale;
            frame[1] *= scale;
        }
    }
    Ok(Mixed {
        frames: out,
        peak,
        limited,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (Projects, u64) {
        let mut all = Projects::default();
        let id = all.create("Night bus", 120.0, 1, 0).unwrap();
        (all, id)
    }

    fn constant(track: u64, value: f32, frames: usize) -> Loaded {
        Loaded {
            track,
            frames: vec![[value, value]; frames],
            sample_rate: 1_000,
        }
    }

    #[test]
    fn timing_and_tracks_stay_in_range() {
        let (mut all, id) = project();
        assert_eq!(all.get(id).unwrap().loop_seconds(), 2.0);
        assert!(all.create("Too slow", 20.0, 4, 0).is_err());
        assert!(all.create("Too long", 120.0, 17, 0).is_err());
        for _ in 1..MAX_TRACKS {
            all.add_track(id, None, 1).unwrap();
        }
        assert_eq!(all.get(id).unwrap().tracks[7].name, "Track 8");
        assert_eq!(
            all.add_track(id, None, 1),
            Err(SoundscapeError::TooManyTracks { max: MAX_TRACKS })
        );
    }

    #[test]
    fn solo_and_mute_decide_who_is_heard() {
        let (mut all, id) = project();
        let b = all.add_track(id, None, 1).unwrap();
        let c = all.add_track(id, None, 1).unwrap();
        all.edit(id, 2, |p| {
            p.track_mut(b)?.settings.solo = true;
            p.track_mut(c)?.settings.solo = true;
            p.track_mut(c)?.settings.muted = true;
            Ok(())
        })
        .unwrap();
        let p = all.get(id).unwrap();
        let heard: Vec<bool> = p.tracks.iter().map(|t| p.audible(t)).collect();
        assert_eq!(heard, vec![false, true, false]);
    }

    #[test]
    fn settings_map_to_audio_engine_units() {
        let mut s = TrackSettings {
            gain_db: -6.0,
            pan: -0.5,
            tone: 0.0,
            echo: Some(Echo {
                beats: 8.0 / 4.0,
                feedback: 0.4,
                mix: 0.25,
            }),
            ..TrackSettings::default()
        };
        let m = mix(&s, true, 120.0);
        assert!((m.volume - 0.501).abs() < 0.001);
        assert_eq!(m.low_pass_hz, Some(200.0));
        assert_eq!(m.delay_seconds, Some(1.0));
        assert_eq!(m.delay_feedback_percent, 40.0);
        assert_eq!(m.delay_wet_dry_percent, 25.0);
        // Slow tempos stop at AVAudioUnitDelay's two seconds.
        assert_eq!(mix(&s, true, 40.0).delay_seconds, Some(2.0));
        assert_eq!(mix(&s, false, 120.0).volume, 0.0);
        s.tone = 1.0;
        assert_eq!(mix(&s, true, 120.0).low_pass_hz, None);
        s.tone = 0.5;
        assert!((mix(&s, true, 120.0).low_pass_hz.unwrap() - 2_000.0).abs() < 1.0);
    }

    #[test]
    fn bad_settings_are_refused() {
        for s in [
            TrackSettings {
                gain_db: 7.0,
                ..TrackSettings::default()
            },
            TrackSettings {
                pan: 1.5,
                ..TrackSettings::default()
            },
            TrackSettings {
                echo: Some(Echo {
                    beats: 1.0,
                    feedback: 0.95,
                    mix: 0.5,
                }),
                ..TrackSettings::default()
            },
        ] {
            assert!(check_settings(&s).is_err(), "{s:?}");
        }
    }

    #[test]
    fn mixdown_loops_pans_and_limits() {
        let (mut all, id) = project();
        let first = all.get(id).unwrap().tracks[0].id;
        let second = all.add_track(id, None, 1).unwrap();
        all.edit(id, 1, |p| {
            p.track_mut(second)?.settings.pan = 1.0;
            Ok(())
        })
        .unwrap();
        let p = all.get(id).unwrap();
        // A loop is 2 s, so 2000 frames at 1 kHz. The first clip is half
        // that long: it is followed by silence.
        let clips = [constant(first, 0.25, 1_000), constant(second, 0.5, 2_000)];
        let mixed = mixdown(p, &clips, 1_000, 2, || false).unwrap();
        assert_eq!(mixed.frames.len(), 4_000);
        assert_eq!(mixed.frames[10], [0.25, 0.75]);
        assert_eq!(mixed.frames[1_500], [0.0, 0.5]);
        assert_eq!(mixed.frames[2_010], [0.25, 0.75], "the second pass loops");
        assert!(!mixed.limited);

        let loud = [constant(first, 0.9, 2_000), constant(second, 0.9, 2_000)];
        let mixed = mixdown(p, &loud, 1_000, 1, || false).unwrap();
        assert!(mixed.limited);
        assert!((mixed.peak - 1.8).abs() < 0.001);
        assert!(dsp::peak(&mixed.frames) <= 0.98 + 1e-6);
    }

    #[test]
    fn nothing_audible_is_silent_and_stopping_is_honoured() {
        let (all, id) = project();
        let p = all.get(id).unwrap();
        assert!(matches!(
            mixdown(p, &[], 1_000, 1, || false),
            Err(SoundscapeError::Silent)
        ));
        let clips = [constant(p.tracks[0].id, 0.1, 10)];
        assert!(matches!(
            mixdown(p, &clips, 1_000, 1, || true),
            Err(SoundscapeError::Stopped { .. })
        ));
    }
}
