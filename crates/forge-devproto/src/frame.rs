//! Length-prefixed frames: a 4-byte big-endian length, then that many
//! bytes. One frame carries one Noise message, so the cap is Noise's own
//! 65,535-byte message limit.

use crate::{ProtoError, Result};

/// The largest frame either side sends or accepts.
pub const MAX_FRAME: usize = 65_535;

/// `payload` with its length prefix.
pub fn encode(payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() > MAX_FRAME {
        return Err(ProtoError::FrameTooLarge(payload.len()));
    }
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// Collects bytes as they arrive and hands back whole frames.
#[derive(Default)]
pub struct FrameReader {
    buf: Vec<u8>,
}

impl FrameReader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds bytes read from the stream.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// The next whole frame, `Ok(None)` if more bytes are needed, or an
    /// error if the peer announced a frame over the cap (the stream cannot
    /// be trusted after that).
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>> {
        if self.buf.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_be_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
        if len > MAX_FRAME {
            return Err(ProtoError::FrameTooLarge(len));
        }
        if self.buf.len() < 4 + len {
            return Ok(None);
        }
        let frame = self.buf[4..4 + len].to_vec();
        self.buf.drain(..4 + len);
        Ok(Some(frame))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_survive_any_split() {
        let mut wire = encode(b"hello").unwrap();
        wire.extend(encode(b"").unwrap());
        wire.extend(encode(&[7u8; 300]).unwrap());
        for chunk in 1..wire.len() {
            let mut reader = FrameReader::new();
            let mut got = Vec::new();
            for piece in wire.chunks(chunk) {
                reader.push(piece);
                while let Some(frame) = reader.next_frame().unwrap() {
                    got.push(frame);
                }
            }
            assert_eq!(
                got,
                [b"hello".to_vec(), vec![], vec![7u8; 300]],
                "chunk {chunk}"
            );
        }
    }

    #[test]
    fn oversized_frames_are_refused_both_ways() {
        assert!(matches!(
            encode(&vec![0u8; MAX_FRAME + 1]),
            Err(ProtoError::FrameTooLarge(_))
        ));
        let mut reader = FrameReader::new();
        reader.push(&(u32::MAX).to_be_bytes());
        assert!(matches!(
            reader.next_frame(),
            Err(ProtoError::FrameTooLarge(_))
        ));
    }
}
