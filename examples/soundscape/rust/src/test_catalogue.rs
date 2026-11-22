//! A small fixed test catalogue: eight tracks Soundscape makes itself, so
//! the player, the library and downloads can be checked without depending
//! on what Jamendo or the Internet Archive happen to have this week
//! (`docs/sources.md`, order of work, step 4).
//!
//! Each track covers one awkward case: very short, very long, no artwork,
//! no tags, two recordings of one work, the same recording in a second
//! format, and one that may only be streamed. The audio is tones written by
//! this module as uncompressed PCM in WAV, AIFF and CAF (the three formats
//! iOS plays that need no encoder), and the artwork is a PNG drawn here too,
//! so the same files come out on every phone and in every test. They are
//! written once, on first use, into `test-catalogue/` in the app's data
//! folder (about 18 MB). Track ids are `test:<slug>` and the licence is CC0:
//! there is nothing to credit.

use std::f32::consts::TAU;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::catalogue::{rights_from_licence, CatalogueTrack};
use crate::library::file_url;
use crate::model::SoundscapeError;

pub const PROVIDER: &str = "Test Catalogue";
/// Prefix of a test track's id: `test:<slug>`.
pub const PREFIX: &str = "test:";
/// The folder in the app's data folder that holds the files.
pub const FOLDER: &str = "test-catalogue";
const LICENCE: &str = "https://creativecommons.org/publicdomain/zero/1.0/";
const ARTIST: &str = "Soundscape Test Ensemble";
const ALBUM: &str = "Soundscape Test Catalogue";
/// Artwork is square, this many pixels a side.
const ARTWORK_PIXELS: u32 = 160;

/// One test track and what it is there to check.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct TestTrack {
    pub track: CatalogueTrack,
    /// What to look for when playing it, in a sentence.
    pub checks: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Format {
    Wav,
    Aiff,
    Caf,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Wav => "wav",
            Format::Aiff => "aiff",
            Format::Caf => "caf",
        }
    }
}

/// What a track sounds like.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tune {
    /// Three rising notes, then quiet.
    Chime,
    /// Two low tones that slowly swell and fade, for a long time.
    Drone,
    /// A five-note theme, each note `beat` seconds long, repeated.
    Theme { beat: f32 },
    /// One note repeated, `beat` seconds apart.
    Pulse { hz: f32, beat: f32 },
}

#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub slug: &'static str,
    pub title: &'static str,
    pub artist: Option<&'static str>,
    pub album: Option<&'static str>,
    pub number: Option<u32>,
    pub genre: Option<&'static str>,
    pub seconds: u32,
    pub rate: u32,
    pub format: Format,
    /// Artwork colour, or no artwork.
    pub colour: Option<[u8; 3]>,
    pub may_download: bool,
    tune: Tune,
    pub checks: &'static str,
}

impl Spec {
    pub fn id(&self) -> String {
        format!("{PREFIX}{}", self.slug)
    }

    pub fn audio_file(&self) -> String {
        format!("{}.{}", self.slug, self.format.extension())
    }

    pub fn artwork_file(&self) -> Option<String> {
        self.colour.map(|_| format!("{}.png", self.slug))
    }
}

const THEME_SLOW: Tune = Tune::Theme { beat: 0.75 };

/// The catalogue, in the order it is shown.
pub const SPECS: &[Spec] = &[
    Spec {
        slug: "short-chime",
        title: "Short Chime",
        artist: Some(ARTIST),
        album: Some(ALBUM),
        number: Some(1),
        genre: Some("Test"),
        seconds: 4,
        rate: 16_000,
        format: Format::Wav,
        colour: Some([52, 120, 246]),
        may_download: true,
        tune: Tune::Chime,
        checks: "Four seconds: the next song starts on its own and the time shown never runs past the end.",
    },
    Spec {
        slug: "long-drone",
        title: "Long Drone",
        artist: Some(ARTIST),
        album: Some(ALBUM),
        number: Some(2),
        genre: Some("Ambient"),
        seconds: 600,
        rate: 8_000,
        format: Format::Wav,
        colour: Some([120, 72, 200]),
        may_download: true,
        tune: Tune::Drone,
        checks: "Ten minutes: scrub to the end, lock the phone, and try the sleep timer.",
    },
    Spec {
        slug: "no-artwork",
        title: "No Artwork",
        artist: Some(ARTIST),
        album: Some(ALBUM),
        number: Some(3),
        genre: Some("Test"),
        seconds: 45,
        rate: 16_000,
        format: Format::Aiff,
        colour: None,
        may_download: true,
        tune: Tune::Pulse {
            hz: 523.25,
            beat: 1.0,
        },
        checks: "AIFF with no artwork: the coloured stand-in shows everywhere, including the lock screen.",
    },
    Spec {
        slug: "untagged",
        title: "Untagged",
        artist: None,
        album: None,
        number: None,
        genre: None,
        seconds: 30,
        rate: 16_000,
        format: Format::Caf,
        colour: Some([46, 160, 90]),
        may_download: true,
        tune: Tune::Pulse {
            hz: 392.0,
            beat: 0.5,
        },
        checks: "CAF with no artist, album or genre: it shows as Unknown Artist and sorts without breaking a list.",
    },
    Spec {
        slug: "theme-slow",
        title: "Theme in C",
        artist: Some(ARTIST),
        album: Some(ALBUM),
        number: Some(4),
        genre: Some("Test"),
        seconds: 60,
        rate: 16_000,
        format: Format::Wav,
        colour: Some([230, 160, 40]),
        may_download: true,
        tune: THEME_SLOW,
        checks: "One work, the slow recording: added with the fast one they stay two songs (their lengths differ).",
    },
    Spec {
        slug: "theme-fast",
        title: "Theme in C",
        artist: Some(ARTIST),
        album: Some(ALBUM),
        number: Some(5),
        genre: Some("Test"),
        seconds: 40,
        rate: 16_000,
        format: Format::Aiff,
        colour: Some([230, 110, 40]),
        may_download: true,
        tune: Tune::Theme { beat: 0.5 },
        checks: "One work, the fast recording: a different length, so it is not offered as the same song.",
    },
    Spec {
        slug: "theme-copy",
        title: "Theme in C",
        artist: Some(ARTIST),
        album: Some(ALBUM),
        number: Some(6),
        genre: Some("Test"),
        seconds: 60,
        rate: 16_000,
        format: Format::Caf,
        colour: Some([230, 160, 40]),
        may_download: true,
        tune: THEME_SLOW,
        checks: "The slow recording again, as CAF: added with it, Your Music offers to merge the two.",
    },
    Spec {
        slug: "stream-only",
        title: "Stream Only",
        artist: Some(ARTIST),
        album: Some(ALBUM),
        number: Some(7),
        genre: Some("Test"),
        seconds: 20,
        rate: 16_000,
        format: Format::Wav,
        colour: Some([220, 70, 70]),
        may_download: false,
        tune: Tune::Pulse {
            hz: 659.25,
            beat: 0.25,
        },
        checks: "May be streamed, not downloaded: Download is not offered and Downloads never lists it.",
    },
];

/// The spec for a test track id, if it is one.
pub fn spec(id: &str) -> Option<&'static Spec> {
    let slug = id.strip_prefix(PREFIX)?;
    SPECS.iter().find(|s| s.slug == slug)
}

/// The catalogue's tracks, their files in `dir` (made by [`ensure`]).
pub fn tracks(dir: &Path) -> Vec<TestTrack> {
    SPECS
        .iter()
        .map(|spec| TestTrack {
            track: track(spec, dir),
            checks: spec.checks.to_string(),
        })
        .collect()
}

fn track(spec: &Spec, dir: &Path) -> CatalogueTrack {
    let credit = format!(
        "{} by {}, {PROVIDER}",
        spec.title,
        spec.artist.unwrap_or("Soundscape")
    );
    CatalogueTrack {
        id: spec.id(),
        provider: PROVIDER.to_string(),
        title: spec.title.to_string(),
        artist: spec.artist.map(str::to_string),
        album: spec.album.map(str::to_string),
        number: spec.number,
        genre: spec.genre.map(str::to_string),
        duration_seconds: Some(f64::from(spec.seconds)),
        artwork_url: spec.artwork_file().map(|f| file_url(&dir.join(f))),
        audio_url: file_url(&dir.join(spec.audio_file())),
        rights: rights_from_licence(Some(LICENCE), spec.may_download, &credit, None),
        added_ms: 0,
    }
}

/// Where a test track's file is now. The app's data folder moves when the
/// app is reinstalled, so a saved `file://` URL is followed by its file
/// name into `dir`. `None` when `url` isn't one of the catalogue's files.
pub fn relocate(url: &str, dir: &Path) -> Option<String> {
    let name = url.strip_prefix("file://")?.rsplit('/').next()?;
    let known = SPECS
        .iter()
        .any(|s| s.audio_file() == name || s.artwork_file().as_deref() == Some(name));
    known.then(|| file_url(&dir.join(name)))
}

/// Writes any of the catalogue's files missing from `dir`. Each file is
/// written beside its place and renamed into it, so a half-written file is
/// never played.
pub fn ensure(dir: &Path) -> Result<(), SoundscapeError> {
    let storage = |what: &str, err: std::io::Error| SoundscapeError::Storage {
        reason: format!("{what} the test catalogue: {err}"),
    };
    fs::create_dir_all(dir).map_err(|e| storage("make", e))?;
    for spec in SPECS {
        let audio = dir.join(spec.audio_file());
        if !audio.exists() {
            write_atomically(&audio, &encode(spec.format, spec.rate, &samples(spec)))
                .map_err(|e| storage("write", e))?;
        }
        if let (Some(name), Some(colour)) = (spec.artwork_file(), spec.colour) {
            let art = dir.join(name);
            if !art.exists() {
                write_atomically(&art, &png(ARTWORK_PIXELS, colour))
                    .map_err(|e| storage("write", e))?;
            }
        }
    }
    Ok(())
}

fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut part = PathBuf::from(path);
    part.set_extension("part");
    {
        let mut file = fs::File::create(&part)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&part, path)
}

// ------------------------------------------------------------ sound

/// The track's audio as 16-bit mono samples.
pub fn samples(spec: &Spec) -> Vec<i16> {
    let rate = spec.rate as f32;
    let count = spec.seconds as usize * spec.rate as usize;
    let mut out = Vec::with_capacity(count);
    for n in 0..count {
        let t = n as f32 / rate;
        let value = match spec.tune {
            Tune::Chime => chime(t),
            Tune::Drone => drone(t, spec.seconds as f32),
            Tune::Theme { beat } => theme(t, beat),
            Tune::Pulse { hz, beat } => note(hz, t % beat, beat * 0.6),
        };
        // Fade the first and last 20 ms, so nothing clicks.
        let edge = (t / 0.02)
            .min((spec.seconds as f32 - t) / 0.02)
            .clamp(0.0, 1.0);
        out.push((value * edge * 0.35 * f32::from(i16::MAX)) as i16);
    }
    out
}

/// A plucked note: `hz` for `length` seconds, `t` seconds in.
fn note(hz: f32, t: f32, length: f32) -> f32 {
    if t >= length {
        return 0.0;
    }
    let attack = (t / 0.01).min(1.0);
    let decay = 1.0 - t / length;
    (TAU * hz * t).sin() * attack * decay * decay
}

fn chime(t: f32) -> f32 {
    [523.25, 659.25, 783.99]
        .iter()
        .enumerate()
        .map(|(i, hz)| {
            let start = i as f32 * 0.4;
            if t >= start {
                note(*hz, t - start, 2.4) / 2.0
            } else {
                0.0
            }
        })
        .sum()
}

fn drone(t: f32, seconds: f32) -> f32 {
    // Swells every 20 seconds, so a scrub lands on something audible.
    let swell = 0.6 + 0.4 * (TAU * t / 20.0).sin();
    let fade_out = ((seconds - t) / 3.0).clamp(0.0, 1.0);
    ((TAU * 110.0 * t).sin() + (TAU * 165.0 * t).sin() * 0.6) * 0.5 * swell * fade_out
}

fn theme(t: f32, beat: f32) -> f32 {
    const NOTES: [f32; 5] = [261.63, 329.63, 392.0, 329.63, 293.66];
    let step = (t / beat) as usize;
    note(NOTES[step % NOTES.len()], t % beat, beat)
}

// ------------------------------------------------------------ files

/// Mono 16-bit PCM in `format`.
pub fn encode(format: Format, rate: u32, samples: &[i16]) -> Vec<u8> {
    let bytes = samples.len() as u32 * 2;
    let mut out = Vec::with_capacity(bytes as usize + 64);
    match format {
        Format::Wav => {
            out.extend_from_slice(b"RIFF");
            out.extend_from_slice(&(36 + bytes).to_le_bytes());
            out.extend_from_slice(b"WAVEfmt ");
            out.extend_from_slice(&16u32.to_le_bytes());
            out.extend_from_slice(&1u16.to_le_bytes()); // PCM
            out.extend_from_slice(&1u16.to_le_bytes()); // mono
            out.extend_from_slice(&rate.to_le_bytes());
            out.extend_from_slice(&(rate * 2).to_le_bytes());
            out.extend_from_slice(&2u16.to_le_bytes());
            out.extend_from_slice(&16u16.to_le_bytes());
            out.extend_from_slice(b"data");
            out.extend_from_slice(&bytes.to_le_bytes());
            for s in samples {
                out.extend_from_slice(&s.to_le_bytes());
            }
        }
        Format::Aiff => {
            out.extend_from_slice(b"FORM");
            out.extend_from_slice(&(4 + 26 + 16 + bytes).to_be_bytes());
            out.extend_from_slice(b"AIFFCOMM");
            out.extend_from_slice(&18u32.to_be_bytes());
            out.extend_from_slice(&1u16.to_be_bytes());
            out.extend_from_slice(&(samples.len() as u32).to_be_bytes());
            out.extend_from_slice(&16u16.to_be_bytes());
            out.extend_from_slice(&extended(rate));
            out.extend_from_slice(b"SSND");
            out.extend_from_slice(&(8 + bytes).to_be_bytes());
            out.extend_from_slice(&0u32.to_be_bytes()); // offset
            out.extend_from_slice(&0u32.to_be_bytes()); // block size
            for s in samples {
                out.extend_from_slice(&s.to_be_bytes());
            }
        }
        Format::Caf => {
            out.extend_from_slice(b"caff");
            out.extend_from_slice(&1u16.to_be_bytes()); // version
            out.extend_from_slice(&0u16.to_be_bytes()); // flags
            out.extend_from_slice(b"desc");
            out.extend_from_slice(&32i64.to_be_bytes());
            out.extend_from_slice(&f64::from(rate).to_be_bytes());
            out.extend_from_slice(b"lpcm");
            out.extend_from_slice(&2u32.to_be_bytes()); // little-endian integers
            out.extend_from_slice(&2u32.to_be_bytes()); // bytes per packet
            out.extend_from_slice(&1u32.to_be_bytes()); // frames per packet
            out.extend_from_slice(&1u32.to_be_bytes()); // channels
            out.extend_from_slice(&16u32.to_be_bytes()); // bits
            out.extend_from_slice(b"data");
            out.extend_from_slice(&(4 + i64::from(bytes)).to_be_bytes());
            out.extend_from_slice(&0u32.to_be_bytes()); // edit count
            for s in samples {
                out.extend_from_slice(&s.to_le_bytes());
            }
        }
    }
    out
}

/// A whole number as the 80-bit extended float AIFF keeps its sample
/// rate in.
fn extended(value: u32) -> [u8; 10] {
    let mut out = [0u8; 10];
    if value == 0 {
        return out;
    }
    let shift = value.leading_zeros();
    let exponent = 16383 + 31 - shift as u16;
    let mantissa = u64::from(value) << (32 + shift);
    out[..2].copy_from_slice(&exponent.to_be_bytes());
    out[2..].copy_from_slice(&mantissa.to_be_bytes());
    out
}

// ------------------------------------------------------------ artwork

/// A square PNG: `colour` lightening towards the top left, with a ring.
/// Uncompressed (stored deflate blocks), so it needs no zlib.
pub fn png(size: u32, colour: [u8; 3]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(((size * 3 + 1) * size) as usize);
    let centre = size as f32 / 2.0;
    for y in 0..size {
        raw.push(0); // no filter
        for x in 0..size {
            let light = 1.0 - (x + y) as f32 / (2.0 * size as f32) * 0.5;
            let distance = ((x as f32 - centre).powi(2) + (y as f32 - centre).powi(2)).sqrt();
            let ring = (distance - size as f32 * 0.3).abs() < size as f32 * 0.04;
            for c in colour {
                let value = if ring { 255.0 } else { f32::from(c) * light };
                raw.push(value as u8);
            }
        }
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(i + 1 == blocks.len()));
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&size.to_be_bytes());
    header.extend_from_slice(&size.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit RGB
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in bytes {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}
