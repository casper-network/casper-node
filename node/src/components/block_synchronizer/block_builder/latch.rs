use std::sync::atomic::{AtomicU8, Ordering};

use datasize::DataSize;

use tracing::error;

use casper_types::{TimeDiff, Timestamp};

#[derive(Debug, Default, DataSize)]
pub(super) struct Latch {
    #[data_size(skip)]
    latch: AtomicU8,
    timestamp: Option<Timestamp>,
}

impl Latch {
    pub(super) fn increment(&mut self, increment_by: u8) {
        match self.latch.get_mut().checked_add(increment_by) {
            Some(val) => {
                self.latch.store(val, Ordering::SeqCst);
            }
            None => {
                error!("latch increment overflowed.");
            }
        }
        self.touch();
    }

    pub(super) fn decrement(&mut self, decrement_by: u8) {
        match self.latch.get_mut().checked_sub(decrement_by) {
            Some(val) => {
                self.latch.store(val, Ordering::SeqCst);
            }
            None => {
                error!("latch decrement overflowed.");
            }
        }
        self.touch();
    }

    pub(super) fn unlatch(&mut self) {
        self.latch.store(0, Ordering::SeqCst);
        self.timestamp = None;
    }

    pub(super) fn check_latch(&mut self, interval: TimeDiff, checked: Timestamp) -> bool {
        match self.timestamp {
            None => false,
            Some(timestamp) => {
                if checked > timestamp + interval {
                    self.unlatch()
                }
                self.count() > 0
            }
        }
    }

    pub(super) fn count(&self) -> u8 {
        self.latch.load(Ordering::SeqCst)
    }

    pub(super) fn touch(&mut self) {
        self.timestamp = Some(Timestamp::now());
    }
}
