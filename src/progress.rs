use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc,
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
