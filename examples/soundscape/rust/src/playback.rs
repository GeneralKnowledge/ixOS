//! What was playing, kept so it survives the app closing: the queue (its
//! songs, play order, shuffle and repeat), where in it playback was, and
//! how far into the song. Saved in `playback.json` as it changes; opened
//! paused where it was left.

use serde::{Deserialize, Serialize};

use crate::queue::{PlayQueue, QueueTrack};

pub const PLAYBACK_VERSION: u32 = 1;
/// The most songs kept in a saved queue.
pub const MAX_SAVED_TRACKS: usize = 5_000;

/// `playback.json`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct PlaybackFile {
    #[serde(default)]
    pub saved: Option<SavedPlayback>,
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
