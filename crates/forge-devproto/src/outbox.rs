//! The app's numbered record buffer, which makes a session survive the app
//! going to the background: the host reconnects with the last sequence
//! number it saw and gets the rest.

use std::collections::VecDeque;

use crate::Record;

/// The newest `capacity` records, numbered from 1.
pub struct Outbox {
    capacity: usize,
    records: VecDeque<(u64, Record)>,
    next_seq: u64,
}

impl Outbox {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            records: VecDeque::new(),
            next_seq: 1,
        }
    }

    /// Adds a record, dropping the oldest when full. Returns its number.
    pub fn push(&mut self, record: Record) -> u64 {
        if self.records.len() == self.capacity {
            self.records.pop_front();
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.records.push_back((seq, record));
        seq
    }

    /// The number the next record will get.
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// The oldest record still held, or `next_seq` when empty.
    pub fn oldest_seq(&self) -> u64 {
        self.records.front().map_or(self.next_seq, |(seq, _)| *seq)
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// What a host that last saw `ack` is missing: how many records it can
    /// no longer get (the gap), and the ones still held after `ack`, oldest
    /// first. An `ack` from the future (an app restart reset the numbers)
    /// counts as having seen nothing.
    pub fn after(&self, ack: u64) -> (u64, Vec<(u64, Record)>) {
        let ack = if ack >= self.next_seq { 0 } else { ack };
        let gap = self.oldest_seq().saturating_sub(ack + 1);
        let records = self
            .records
            .iter()
            .filter(|(seq, _)| *seq > ack)
            .cloned()
            .collect();
        (gap, records)
    }

    /// The newest `n` records, for a host attaching for the first time.
    pub fn tail(&self, n: usize) -> Vec<(u64, Record)> {
        let skip = self.records.len().saturating_sub(n);
        self.records.iter().skip(skip).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phase(n: u64) -> Record {
        Record::Phase {
            phase: n.to_string(),
        }
    }

    fn seqs(records: &[(u64, Record)]) -> Vec<u64> {
        records.iter().map(|(s, _)| *s).collect()
    }

    #[test]
    fn replays_what_the_host_missed() {
        let mut out = Outbox::new(10);
        for n in 1..=5 {
            assert_eq!(out.push(phase(n)), n);
        }
        let (gap, rest) = out.after(3);
        assert_eq!((gap, seqs(&rest)), (0, vec![4, 5]));
        assert_eq!(out.after(5).1.len(), 0);
        assert_eq!(seqs(&out.after(0).1), [1, 2, 3, 4, 5]);
        assert_eq!(seqs(&out.tail(2)), [4, 5]);
    }

    #[test]
    fn reports_a_gap_when_the_buffer_moved_on() {
        let mut out = Outbox::new(3);
        for n in 1..=10 {
            out.push(phase(n));
        }
        assert_eq!((out.oldest_seq(), out.next_seq(), out.len()), (8, 11, 3));
        let (gap, rest) = out.after(4);
        assert_eq!((gap, seqs(&rest)), (3, vec![8, 9, 10]));
        let (gap, rest) = out.after(0);
        assert_eq!((gap, seqs(&rest)), (7, vec![8, 9, 10]));
    }

    #[test]
    fn an_ack_from_a_previous_run_starts_over() {
        let mut out = Outbox::new(5);
        out.push(phase(1));
        let (gap, rest) = out.after(500);
        assert_eq!((gap, seqs(&rest)), (0, vec![1]));
    }
}
