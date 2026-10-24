//! USB slot table and the OUT free stack.
//!
//! 16 IN slots and 64 OUT slots. OUT slot 63 is reserved for ARP. The other 63
//! OUT slots are data slots. Each slot owns one 32768-byte buffer, allocated
//! once in [`Pool::new`].
//!
//! One Ethernet frame per RNDIS message. This pool does not batch frames.
//!
//! Keepalive: the event thread will later send it as an async control transfer.
//! This module does not start a thread and does not call libusb.

use std::sync::{Condvar, Mutex};

use crate::rndis::OUR_MAX_TRANSFER_SIZE;

pub const IN_SLOTS: usize = 16;
pub const OUT_SLOTS: usize = 64;
pub const DATA_SLOTS: usize = 63;
pub const ARP_SLOT: usize = 63;
pub const BUF_LEN: usize = OUR_MAX_TRANSFER_SIZE as usize;

struct Slot {
    /// Unread until a USB transfer is linked.
    #[allow(dead_code)]
    buf: Vec<u8>,
}

impl Slot {
    fn new() -> Self {
        Self {
            buf: vec![0; BUF_LEN],
        }
    }
}

struct Inner {
    /// Unread until a USB transfer is linked.
    #[allow(dead_code)]
    in_slots: Vec<Slot>,
    #[allow(dead_code)]
    out_slots: Vec<Slot>,
    free_data: Vec<usize>,
    data_out: [bool; DATA_SLOTS],
    arp_out: bool,
    closed: bool,
}

pub struct Pool {
    inner: Mutex<Inner>,
    data_ready: Condvar,
}

impl Pool {
    pub fn new() -> Self {
        let in_slots = (0..IN_SLOTS).map(|_| Slot::new()).collect();
        let out_slots = (0..OUT_SLOTS).map(|_| Slot::new()).collect();
        let free_data = (0..DATA_SLOTS).collect();
        Self {
            inner: Mutex::new(Inner {
                in_slots,
                out_slots,
                free_data,
                data_out: [false; DATA_SLOTS],
                arp_out: false,
                closed: false,
            }),
            data_ready: Condvar::new(),
        }
    }

    /// Data slot index. Waits until one is free. `None` after [`Pool::close`].
    pub fn checkout_data(&self) -> Option<usize> {
        let mut inner = self.inner.lock().unwrap();
        loop {
            if inner.closed {
                return None;
            }
            if let Some(index) = take_data(&mut inner) {
                return Some(index);
            }
            inner = self.data_ready.wait(inner).unwrap();
        }
    }

    /// Wake every waiter. Later checkouts return `None`.
    pub fn close(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.closed = true;
        self.data_ready.notify_all();
    }

    /// Data slot index, or `None` when every data slot is out.
    pub fn checkout_data_now(&self) -> Option<usize> {
        let mut inner = self.inner.lock().unwrap();
        if inner.closed {
            return None;
        }
        take_data(&mut inner)
    }

    /// The ARP slot, or `None` when it is already out.
    pub fn checkout_arp(&self) -> Option<usize> {
        let mut inner = self.inner.lock().unwrap();
        if inner.arp_out {
            return None;
        }
        inner.arp_out = true;
        Some(ARP_SLOT)
    }

    pub fn release(&self, index: usize) {
        let mut inner = self.inner.lock().unwrap();
        if index == ARP_SLOT {
            inner.arp_out = false;
            return;
        }
        if index >= DATA_SLOTS || !inner.data_out[index] {
            return;
        }
        inner.data_out[index] = false;
        inner.free_data.push(index);
        self.data_ready.notify_one();
    }
}

fn take_data(inner: &mut Inner) -> Option<usize> {
    let index = inner.free_data.pop()?;
    inner.data_out[index] = true;
    Some(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    fn buf_ptrs(slots: &[Slot]) -> Vec<*const u8> {
        slots.iter().map(|slot| slot.buf.as_ptr()).collect()
    }

    #[test]
    fn data_slots_stay_separate_from_the_arp_slot() {
        assert_eq!(BUF_LEN, 32_768);
        assert_eq!(IN_SLOTS, 16);
        assert_eq!(OUT_SLOTS, 64);
        assert_eq!(DATA_SLOTS, 63);
        assert_eq!(ARP_SLOT, 63);

        let pool = Pool::new();
        let (in_ptrs, out_ptrs) = {
            let inner = pool.inner.lock().unwrap();
            assert_eq!(inner.in_slots.len(), IN_SLOTS);
            assert_eq!(inner.out_slots.len(), OUT_SLOTS);
            assert!(inner.in_slots.iter().all(|slot| slot.buf.len() == BUF_LEN));
            assert!(inner.out_slots.iter().all(|slot| slot.buf.len() == BUF_LEN));
            (buf_ptrs(&inner.in_slots), buf_ptrs(&inner.out_slots))
        };

        assert_eq!(pool.checkout_arp(), Some(ARP_SLOT));
        assert_eq!(pool.checkout_arp(), None);

        let mut data = Vec::new();
        for _ in 0..DATA_SLOTS {
            let index = pool.checkout_data_now().expect("data slot");
            assert!(index < DATA_SLOTS, "data checkout returned {index}");
            assert!(!data.contains(&index), "slot {index} checked out twice");
            data.push(index);
        }
        assert_eq!(data.len(), DATA_SLOTS);
        assert_eq!(pool.checkout_data_now(), None);
        assert_eq!(pool.checkout_arp(), None);

        let returned = data[0];
        pool.release(returned);
        assert_eq!(pool.checkout_data_now(), Some(returned));
        assert_eq!(pool.checkout_data_now(), None);

        pool.release(ARP_SLOT);
        assert_eq!(pool.checkout_data_now(), None);
        assert_eq!(pool.checkout_arp(), Some(ARP_SLOT));

        let (in_after, out_after) = {
            let inner = pool.inner.lock().unwrap();
            (buf_ptrs(&inner.in_slots), buf_ptrs(&inner.out_slots))
        };
        assert_eq!(in_after, in_ptrs);
        assert_eq!(out_after, out_ptrs);
    }

    #[test]
    fn checkout_data_waits_until_a_slot_is_released() {
        let pool = Arc::new(Pool::new());
        let mut held = Vec::new();
        for _ in 0..DATA_SLOTS {
            held.push(pool.checkout_data_now().unwrap());
        }
        assert_eq!(pool.checkout_data_now(), None);

        let (started_tx, started_rx) = mpsc::channel();
        let (got_tx, got_rx) = mpsc::channel();
        let waiting = Arc::clone(&pool);
        let handle = thread::spawn(move || {
            started_tx.send(()).unwrap();
            got_tx.send(waiting.checkout_data()).unwrap();
        });

        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        thread::sleep(Duration::from_millis(50));
        assert!(got_rx.try_recv().is_err());

        pool.release(held[0]);
        let got = got_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(got, Some(held[0]));
        handle.join().unwrap();
    }

    #[test]
    fn close_unblocks_a_waiting_checkout() {
        let pool = Arc::new(Pool::new());
        for _ in 0..DATA_SLOTS {
            pool.checkout_data_now().unwrap();
        }
        let waiting = Arc::clone(&pool);
        let handle = thread::spawn(move || waiting.checkout_data());
        thread::sleep(Duration::from_millis(50));
        pool.close();
        assert_eq!(handle.join().unwrap(), None);
        assert_eq!(pool.checkout_data_now(), None);
    }
}
