//! Ethernet frames waiting for the utun writer.
//!
//! 256 slots. Each slot holds one frame of at most 2048 bytes, plus its length.
//! `push` copies bytes into a free slot. A longer frame is not stored. A full
//! ring returns [`PushError::Full`] and does not block. `pop` copies the oldest
//! frame out and frees that slot.
//!
//! The event thread and the utun writer share one ring. The lock is a [`Mutex`].

use std::sync::Mutex;

pub const SLOT_COUNT: usize = 256;
pub const MAX_FRAME_LEN: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushError {
    TooLong,
    Full,
}

struct Slot {
    len: usize,
    buf: [u8; MAX_FRAME_LEN],
}

struct Inner {
    slots: Vec<Slot>,
    /// Next frame to pop.
    head: usize,
    /// Next slot to fill.
    tail: usize,
    count: usize,
}

pub struct Ring {
    inner: Mutex<Inner>,
}

impl Ring {
    pub fn new() -> Self {
        let slots = (0..SLOT_COUNT)
            .map(|_| Slot {
                len: 0,
                buf: [0; MAX_FRAME_LEN],
            })
            .collect();
        Self {
            inner: Mutex::new(Inner {
                slots,
                head: 0,
                tail: 0,
                count: 0,
            }),
        }
    }

    pub fn push(&self, frame: &[u8]) -> Result<(), PushError> {
        if frame.len() > MAX_FRAME_LEN {
            return Err(PushError::TooLong);
        }
        let mut inner = self.inner.lock().unwrap();
        if inner.count == SLOT_COUNT {
            return Err(PushError::Full);
        }
        let tail = inner.tail;
        let slot = &mut inner.slots[tail];
        slot.buf[..frame.len()].copy_from_slice(frame);
        slot.len = frame.len();
        inner.tail = (tail + 1) % SLOT_COUNT;
        inner.count += 1;
        Ok(())
    }

    pub fn pop(&self) -> Option<Vec<u8>> {
        let mut inner = self.inner.lock().unwrap();
        if inner.count == 0 {
            return None;
        }
        let head = inner.head;
        let slot = &inner.slots[head];
        let frame = slot.buf[..slot.len].to_vec();
        inner.head = (head + 1) % SLOT_COUNT;
        inner.count -= 1;
        Some(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    fn frame_bytes(i: usize) -> Vec<u8> {
        let n = u16::try_from(i).unwrap();
        let mut bytes = n.to_le_bytes().to_vec();
        let pad = (i % 23) + 1;
        bytes.extend(std::iter::repeat((n as u8).wrapping_add(9)).take(pad));
        bytes
    }

    #[test]
    fn ring_keeps_frames_in_order_and_rejects_a_full_or_long_push() {
        let ring = Ring::new();
        assert_eq!(ring.pop(), None);

        let too_long = vec![0x5A; MAX_FRAME_LEN + 1];
        assert_eq!(ring.push(&too_long), Err(PushError::TooLong));
        assert_eq!(ring.pop(), None);

        let maxed = vec![0x11; MAX_FRAME_LEN];
        ring.push(&maxed).unwrap();
        assert_eq!(ring.pop(), Some(maxed));
        assert_eq!(ring.pop(), None);

        let frames: Vec<Vec<u8>> = (0..SLOT_COUNT).map(frame_bytes).collect();
        for frame in &frames {
            ring.push(frame).unwrap();
        }
        assert_eq!(ring.push(b"one past full"), Err(PushError::Full));
        assert_eq!(ring.push(&too_long), Err(PushError::TooLong));

        assert_eq!(ring.pop().as_deref(), Some(frames[0].as_slice()));
        let extra = frame_bytes(SLOT_COUNT);
        ring.push(&extra).unwrap();
        assert_eq!(ring.push(&extra), Err(PushError::Full));

        for frame in frames.iter().skip(1) {
            assert_eq!(ring.pop().as_deref(), Some(frame.as_slice()));
        }
        assert_eq!(ring.pop().as_deref(), Some(extra.as_slice()));
        assert_eq!(ring.pop(), None);
    }

    #[test]
    fn two_threads_push_and_pop_in_order() {
        let ring = Arc::new(Ring::new());
        let writer = {
            let ring = Arc::clone(&ring);
            thread::spawn(move || {
                for i in 0..100 {
                    let frame = frame_bytes(i);
                    ring.push(&frame).unwrap();
                }
            })
        };

        let mut got = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while got.len() < 100 {
            if let Some(frame) = ring.pop() {
                got.push(frame);
            } else if Instant::now() > deadline {
                panic!("timed out with {} frames", got.len());
            } else {
                thread::yield_now();
            }
        }
        writer.join().unwrap();

        for (i, frame) in got.iter().enumerate() {
            assert_eq!(frame, &frame_bytes(i));
        }
        assert_eq!(ring.pop(), None);
    }
}
