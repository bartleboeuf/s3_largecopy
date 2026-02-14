use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

/// Progress tracking structure
#[derive(Clone)]
pub struct CopyProgress {
    pub copied_bytes: Arc<AtomicU64>,
    pub completed_parts: Arc<AtomicUsize>,
    pub total_parts: usize,
}

impl CopyProgress {
    pub fn new(total_parts: usize) -> Self {
        Self {
            copied_bytes: Arc::new(AtomicU64::new(0)),
            completed_parts: Arc::new(AtomicUsize::new(0)),
            total_parts,
        }
    }

    pub fn add_completed(&self, bytes: u64) {
        self.copied_bytes.fetch_add(bytes, Ordering::SeqCst);
        self.completed_parts.fetch_add(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies constructor initializes counters and total part count correctly.
    #[test]
    fn new_initializes_zeroed_counters() {
        let progress = CopyProgress::new(12);
        assert_eq!(progress.total_parts, 12);
        assert_eq!(progress.copied_bytes.load(Ordering::SeqCst), 0);
        assert_eq!(progress.completed_parts.load(Ordering::SeqCst), 0);
    }

    /// Ensures add_completed updates both copied bytes and completed part counters.
    #[test]
    fn add_completed_increments_bytes_and_parts() {
        let progress = CopyProgress::new(3);
        progress.add_completed(1024);
        progress.add_completed(2048);

        assert_eq!(progress.copied_bytes.load(Ordering::SeqCst), 3072);
        assert_eq!(progress.completed_parts.load(Ordering::SeqCst), 2);
        assert_eq!(progress.total_parts, 3);
    }

    /// Confirms cloned progress handles share the same atomic state.
    #[test]
    fn clone_shares_progress_state() {
        let progress = CopyProgress::new(2);
        let clone = progress.clone();

        progress.add_completed(500);
        clone.add_completed(700);

        assert_eq!(progress.copied_bytes.load(Ordering::SeqCst), 1200);
        assert_eq!(clone.copied_bytes.load(Ordering::SeqCst), 1200);
        assert_eq!(progress.completed_parts.load(Ordering::SeqCst), 2);
        assert_eq!(clone.completed_parts.load(Ordering::SeqCst), 2);
    }
}
