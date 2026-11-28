//! What was playing, kept so it survives the app closing: the queue (its
//! songs, play order, shuffle and repeat), where in it playback was, and
//! how far into the song. Saved in `playback.json` as it changes; opened
//! paused where it was left.

use serde::{Deserialize, Serialize};

use crate::queue::{PlayQueue, QueueTrack};

pub const PLAYBACK_VERSION: u32 = 1;
/// The most songs kept in a saved queue.
pub const MAX_SAVED_TRACKS: usize = 5_000;

/// Items at least this long (a DJ mix, a concert, later a podcast episode)
/// carry on where they were left when played again. Songs start again.
pub const LONG_ITEM_SECONDS: f64 = 600.0;
/// Nearer the start than this, a kept position stays as it was: playing
/// a long item from the top for a moment doesn't lose the place.
pub const RESUME_MIN_SECONDS: f64 = 30.0;
/// This near the end counts as finished, and the position is forgotten.
pub const FINISHED_SECONDS_LEFT: f64 = 30.0;
/// The most positions kept; the oldest go first.
pub const MAX_POSITIONS: usize = 200;

/// `playback.json`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct PlaybackFile {
    #[serde(default)]
    pub saved: Option<SavedPlayback>,
    /// Where long items were left, newest first.
    #[serde(default)]
    pub positions: Vec<KeptPosition>,
}

/// Where a long item was left.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct KeptPosition {
    pub recording_id: String,
    pub seconds: f64,
    pub at_ms: i64,
}

/// Whether an item `length_seconds` long carries on where it was left.
#[uniffi::export]
pub fn keeps_position(length_seconds: f64) -> bool {
    length_seconds.is_finite() && length_seconds >= LONG_ITEM_SECONDS
}

/// The position kept for `recording_id`, if any.
pub fn position(positions: &[KeptPosition], recording_id: &str) -> Option<f64> {
    positions
        .iter()
        .find(|p| p.recording_id == recording_id)
        .map(|p| p.seconds)
}

/// Notes that `recording_id`, `length` seconds long, was heard up to
/// `seconds`: kept for a long item, forgotten once finished. Returns
/// whether anything changed.
pub fn heard(
    positions: &mut Vec<KeptPosition>,
    recording_id: &str,
    seconds: f64,
    length: f64,
    now: i64,
) -> bool {
    if recording_id.is_empty() || !seconds.is_finite() || !keeps_position(length) {
        return false;
    }
    let at = positions
        .iter()
        .position(|p| p.recording_id == recording_id);
    if seconds >= length - FINISHED_SECONDS_LEFT {
        return match at {
            Some(i) => {
                positions.remove(i);
                true
            }
            None => false,
        };
    }
    if seconds < RESUME_MIN_SECONDS {
        return false;
    }
    if let Some(i) = at {
        if positions[i].seconds == seconds {
            return false;
        }
        positions.remove(i);
    }
    positions.insert(
        0,
        KeptPosition {
            recording_id: recording_id.to_string(),
            seconds,
            at_ms: now,
        },
    );
    positions.truncate(MAX_POSITIONS);
    true
}

/// The queue and how far into its current song playback was.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SavedPlayback {
    pub queue: PlayQueue,
    /// Seconds into the current song.
    pub seconds: f64,
    pub saved_ms: i64,
}

/// A saved queue made safe to use: indexes in range, the time a real
/// number, too many songs cut. `None` when nothing is left to play.
pub fn tidy(mut saved: SavedPlayback) -> Option<SavedPlayback> {
    let queue = &mut saved.queue;
    queue.tracks.truncate(MAX_SAVED_TRACKS);
    let count = queue.tracks.len() as u32;
    queue.order.retain(|i| *i < count);
    if queue.order.is_empty() {
        return None;
    }
    queue.position = queue.position.min(queue.order.len() as u32 - 1);
    if !saved.seconds.is_finite() || saved.seconds < 0.0 {
        saved.seconds = 0.0;
    }
    Some(saved)
}

/// Keeps the songs `keep` returns (each possibly replaced), dropping the
/// rest from the play order. Playback stays on the same song, or moves to
/// the next one kept when its own was dropped (from its start).
pub fn refresh(
    saved: SavedPlayback,
    mut keep: impl FnMut(&QueueTrack) -> Option<QueueTrack>,
) -> Option<SavedPlayback> {
    let SavedPlayback {
        queue,
        mut seconds,
        saved_ms,
    } = saved;
    let mut index_map = vec![None; queue.tracks.len()];
    let mut tracks = Vec::with_capacity(queue.tracks.len());
    for (i, track) in queue.tracks.iter().enumerate() {
        if let Some(kept) = keep(track) {
            index_map[i] = Some(tracks.len() as u32);
            tracks.push(kept);
        }
    }
    let mut order = Vec::with_capacity(queue.order.len());
    let mut position = None;
    for (at, old) in queue.order.iter().enumerate() {
        let Some(new) = index_map.get(*old as usize).copied().flatten() else {
            continue;
        };
        if position.is_none() && at as u32 >= queue.position {
            if at as u32 != queue.position {
                seconds = 0.0;
            }
            position = Some(order.len() as u32);
        }
        order.push(new);
    }
    if order.is_empty() {
        return None;
    }
    let position = position.unwrap_or_else(|| {
        seconds = 0.0;
        order.len() as u32 - 1
    });
    Some(SavedPlayback {
        queue: PlayQueue {
            tracks,
            order,
            position,
            shuffle: queue.shuffle,
            repeat: queue.repeat,
        },
        seconds,
        saved_ms,
    })
}
