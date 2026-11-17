use std::sync::atomic::{AtomicU64, Ordering};

pub static TICKS: AtomicU64 = AtomicU64::new(0);
pub static OPENS: AtomicU64 = AtomicU64::new(0);
pub static BUSY: AtomicU64 = AtomicU64::new(0);
pub static CAPTURES: AtomicU64 = AtomicU64::new(0);

pub fn note_tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}
pub fn note_open() {
    OPENS.fetch_add(1, Ordering::Relaxed);
}
pub fn note_busy() {
    BUSY.fetch_add(1, Ordering::Relaxed);
}
pub fn note_capture() {
    CAPTURES.fetch_add(1, Ordering::Relaxed);
}
