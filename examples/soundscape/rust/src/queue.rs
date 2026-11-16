//! The play queue: what Listen plays one after another, in order or
//! shuffled, and what happens at the end of each song (repeat).
//!
//! The queue is a value. Swift keeps the current one and asks these
//! functions for the next. Each track plays one recording, normally its
//! preferred one (the library decides which).

use crate::library::Recording;

/// What happens when a song ends.
#[derive(uniffi::Enum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RepeatMode {
    /// Stop after the last song.
    #[default]
    Off,
    /// Start the queue again after the last song.
    All,
    /// Play the same song again.
    One,
}

/// One track in the queue, with the recording that plays for it.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct QueueTrack {
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub artwork_url: Option<String>,
    pub recording: Recording,
}

/// Songs to play, in the order they play.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PlayQueue {
    /// Every song added, in the order it was added.
    pub tracks: Vec<QueueTrack>,
    /// The order they play in, as indexes into `tracks`.
    pub order: Vec<u32>,
    /// Where in `order` playback is.
    pub position: u32,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

/// One entry of the Up Next list: where it is in the play order (to jump
/// to it or remove it) and the song.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct QueueEntry {
    pub position: u32,
    pub track: QueueTrack,
}

/// A queue of `tracks` that starts at `tracks[start]`. Shuffled, that song
/// plays first and the rest follow in a random order from `seed`.
#[uniffi::export]
pub fn queue_start(
    tracks: Vec<QueueTrack>,
    start: u32,
    shuffle: bool,
    repeat: RepeatMode,
    seed: u64,
) -> PlayQueue {
    let count = tracks.len() as u32;
    let start = start.min(count.saturating_sub(1));
    let mut queue = PlayQueue {
        tracks,
        order: (0..count).collect(),
        position: start,
        shuffle: false,
        repeat,
    };
    if shuffle {
        queue = queue_set_shuffle(queue, true, seed);
    }
    queue
}

/// The song playing now.
#[uniffi::export]
pub fn queue_current(queue: PlayQueue) -> Option<QueueTrack> {
    let index = *queue.order.get(queue.position as usize)?;
    queue.tracks.into_iter().nth(index as usize)
}

/// The queue moved on to the next song, or `None` when it has ended.
/// `by_user` is a tap on Next: it always moves on, while a song that ends
/// by itself plays again with repeat One.
#[uniffi::export]
pub fn queue_advance(queue: PlayQueue, by_user: bool) -> Option<PlayQueue> {
    if queue.order.is_empty() {
        return None;
    }
    if !by_user && queue.repeat == RepeatMode::One {
        return Some(queue);
    }
    let next = queue.position + 1;
    if (next as usize) < queue.order.len() {
        return Some(PlayQueue {
            position: next,
            ..queue
        });
    }
    match queue.repeat {
        RepeatMode::All | RepeatMode::One => Some(PlayQueue {
            position: 0,
            ..queue
        }),
        RepeatMode::Off => None,
    }
}

/// The queue moved back a song. From the first song it goes to the last
/// with repeat All, and stays put otherwise.
#[uniffi::export]
pub fn queue_back(queue: PlayQueue) -> PlayQueue {
    let position = if queue.position > 0 {
        queue.position - 1
    } else if queue.repeat == RepeatMode::All && !queue.order.is_empty() {
        queue.order.len() as u32 - 1
    } else {
        0
    };
    PlayQueue { position, ..queue }
}

/// Plays from `position` in the play order.
#[uniffi::export]
pub fn queue_jump(queue: PlayQueue, position: u32) -> PlayQueue {
    if (position as usize) < queue.order.len() {
        PlayQueue { position, ..queue }
    } else {
        queue
    }
}

/// Turns shuffle on or off without interrupting the song playing. On, the
/// other songs follow in a random order from `seed`; off, every song goes
/// back to the order it was added in.
#[uniffi::export]
pub fn queue_set_shuffle(queue: PlayQueue, on: bool, seed: u64) -> PlayQueue {
    let current = queue.order.get(queue.position as usize).copied();
    let mut order = queue.order.clone();
    let position = if on {
        let rest: Vec<u32> = order
            .iter()
            .copied()
            .filter(|i| Some(*i) != current)
            .collect();
        order = current.into_iter().chain(shuffled(rest, seed)).collect();
        0
    } else {
        order.sort_unstable();
        current
            .and_then(|c| order.iter().position(|i| *i == c))
            .unwrap_or(0) as u32
    };
    PlayQueue {
        order,
        position,
        shuffle: on,
        ..queue
    }
}

#[uniffi::export]
pub fn queue_set_repeat(queue: PlayQueue, repeat: RepeatMode) -> PlayQueue {
    PlayQueue { repeat, ..queue }
}

/// The repeat mode after a tap on the repeat button: Off, All, One, Off.
#[uniffi::export]
pub fn repeat_after(repeat: RepeatMode) -> RepeatMode {
    match repeat {
        RepeatMode::Off => RepeatMode::All,
        RepeatMode::All => RepeatMode::One,
        RepeatMode::One => RepeatMode::Off,
    }
}

/// Adds songs to play next (right after the current one) or last.
#[uniffi::export]
pub fn queue_add(queue: PlayQueue, tracks: Vec<QueueTrack>, next: bool) -> PlayQueue {
    let mut queue = queue;
    let first = queue.tracks.len() as u32;
    let added: Vec<u32> = (first..first + tracks.len() as u32).collect();
    queue.tracks.extend(tracks);
    if next && !queue.order.is_empty() {
        let at = queue.position as usize + 1;
        queue.order.splice(at..at, added);
    } else {
        queue.order.extend(added);
    }
    queue
}

/// Takes the song at `position` out of the play order. The song playing
/// now can't be removed.
#[uniffi::export]
pub fn queue_remove(queue: PlayQueue, position: u32) -> PlayQueue {
    let mut queue = queue;
    if position == queue.position || position as usize >= queue.order.len() {
        return queue;
    }
    queue.order.remove(position as usize);
    if position < queue.position {
        queue.position -= 1;
    }
    queue
}

/// The songs after the current one, in play order. With repeat All the
/// list doesn't wrap: it shows what's left of this time through.
#[uniffi::export]
pub fn queue_upcoming(queue: PlayQueue) -> Vec<QueueEntry> {
    queue
        .order
        .iter()
        .enumerate()
        .skip(queue.position as usize + 1)
        .filter_map(|(position, index)| {
            queue.tracks.get(*index as usize).map(|track| QueueEntry {
                position: position as u32,
                track: track.clone(),
            })
        })
        .collect()
}

/// A Fisher-Yates shuffle driven by xorshift64*, so a seed always gives
/// the same order (tests pass a fixed one; the app a random one).
fn shuffled(mut items: Vec<u32>, seed: u64) -> Vec<u32> {
    let mut state = (seed ^ 0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = move || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };
    for i in (1..items.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
    items
}
