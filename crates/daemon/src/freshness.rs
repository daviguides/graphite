//! Freshness barrier: queries wait briefly for already-observed file events to be indexed.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// Event sequence numbers: `observed` grows when the watcher sees a change, `indexed` when it is committed.
#[derive(Debug, Default)]
pub struct Freshness {
    observed: AtomicU64,
    indexed: Mutex<u64>,
    cv: Condvar,
}

impl Freshness {
    /// Record a new pending change; returns its sequence number.
    pub fn observe(&self) -> u64 {
        self.observed.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn observed(&self) -> u64 {
        self.observed.load(Ordering::SeqCst)
    }

    /// Mark every change up to `seq` as indexed.
    pub fn mark_indexed(&self, seq: u64) {
        let mut done = self.indexed.lock().unwrap();
        if seq > *done {
            *done = seq;
            self.cv.notify_all();
        }
    }

    pub fn indexed(&self) -> u64 {
        *self.indexed.lock().unwrap()
    }

    /// Wait until everything observed before the call is indexed; false if `timeout` elapsed first (answer is stale).
    pub fn wait_current(&self, timeout: Duration) -> bool {
        let target = self.observed();
        let deadline = Instant::now() + timeout;
        let mut done = self.indexed.lock().unwrap();
        while *done < target {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            done = self.cv.wait_timeout(done, deadline - now).unwrap().0;
        }
        true
    }

    /// Changes observed but not yet indexed.
    pub fn pending(&self) -> u64 {
        self.observed().saturating_sub(self.indexed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn waits_for_indexing_or_times_out() {
        let f = Arc::new(Freshness::default());
        assert!(f.wait_current(Duration::from_millis(1)));
        let seq = f.observe();
        assert!(!f.wait_current(Duration::from_millis(20)));
        let g = f.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            g.mark_indexed(seq);
        });
        assert!(f.wait_current(Duration::from_millis(500)));
        t.join().unwrap();
        assert_eq!(f.pending(), 0);
    }
}
