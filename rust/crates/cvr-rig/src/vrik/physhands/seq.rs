//! One writer, any readers: a sequence counter round each copy (PhysHands.cpp's Seq<T>).
use core::cell::UnsafeCell;
use core::sync::atomic::{fence, AtomicU32, Ordering::*};

pub struct Seq<T: Copy> {
    seq: AtomicU32,
    data: UnsafeCell<T>,
}
// SAFETY: a reader copies under the counter and retries a torn copy; one writer at a time (each Seq's documented thread).
unsafe impl<T: Copy + Send> Sync for Seq<T> {}
impl<T: Copy> Seq<T> {
    pub const fn new(v: T) -> Seq<T> {
        Seq { seq: AtomicU32::new(0), data: UnsafeCell::new(v) }
    }
    pub fn write(&self, v: &T) {
        let s = self.seq.load(Relaxed);
        self.seq.store(s.wrapping_add(1), Relaxed);
        fence(Release);
        // SAFETY: the one writer; readers see an odd counter and retry.
        unsafe { core::ptr::write_volatile(self.data.get(), *v) };
        fence(Release);
        self.seq.store(s.wrapping_add(2), Release);
    }
    /// The last whole copy, or None when there is none (never written) or eight tries tore.
    pub fn read(&self) -> Option<T> {
        for _ in 0..8 {
            let a = self.seq.load(Acquire);
            if a & 1 != 0 {
                continue;
            }
            // SAFETY: a copy that is kept only when the counter did not move under it.
            let out = unsafe { core::ptr::read_volatile(self.data.get()) };
            fence(Acquire);
            if self.seq.load(Relaxed) == a {
                return (a != 0).then_some(out);
            }
        }
        None
    }
}
