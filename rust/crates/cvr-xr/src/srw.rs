//! The lock both halves take (include/Utils/SrwMutex.hpp cvr::SrwMutex): a Windows slim reader/writer lock held
//! exclusively, one pointer wide and zero when free, with the data it guards beside it.
use core::cell::UnsafeCell;
use windows_sys::Win32::System::Threading::{
    AcquireSRWLockExclusive, ReleaseSRWLockExclusive, TryAcquireSRWLockExclusive, SRWLOCK,
};

#[repr(transparent)]
pub struct SrwLock(UnsafeCell<SRWLOCK>);
// SAFETY: an SRWLOCK is made to be shared between threads; it is only touched through the Win32 calls.
unsafe impl Sync for SrwLock {}
impl SrwLock {
    pub const fn new() -> SrwLock {
        SrwLock(UnsafeCell::new(SRWLOCK { Ptr: core::ptr::null_mut() }))
    }
    /// Held until the guard drops.
    pub fn lock(&self) -> SrwGuard<'_> {
        // SAFETY: a live SRWLOCK.
        unsafe { AcquireSRWLockExclusive(self.0.get()) };
        SrwGuard(self)
    }
    /// Held until the guard drops, if it was free.
    pub fn try_lock(&self) -> Option<SrwGuard<'_>> {
        // SAFETY: a live SRWLOCK.
        unsafe { TryAcquireSRWLockExclusive(self.0.get()) }.then(|| SrwGuard(self))
    }
    /// Releases a hold whose guard was forgotten (a lock taken and released across a call boundary, as C++'s
    /// lock() / unlock()).
    ///
    /// # Safety
    /// This thread holds the lock, and no guard of that hold will drop.
    pub unsafe fn force_unlock(&self) {
        unsafe { ReleaseSRWLockExclusive(self.0.get()) };
    }
}
impl Default for SrwLock {
    fn default() -> Self {
        SrwLock::new()
    }
}
pub struct SrwGuard<'a>(&'a SrwLock);
impl Drop for SrwGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: acquired by this guard.
        unsafe { ReleaseSRWLockExclusive(self.0 .0.get()) };
    }
}

/// Data the C++ half keeps as a plain member guarded by a lock: read and written only while that lock is held.
#[repr(transparent)]
pub struct Guarded<T>(UnsafeCell<T>);
// SAFETY: every access goes through `get`, whose caller holds the guarding lock.
unsafe impl<T: Send> Sync for Guarded<T> {}
impl<T> Guarded<T> {
    pub const fn new(v: T) -> Guarded<T> {
        Guarded(UnsafeCell::new(v))
    }
    /// The value, while `_guard` (of the lock that guards it) is held.
    #[allow(clippy::mut_from_ref)]
    pub fn get<'a>(&'a self, _guard: &'a SrwGuard<'_>) -> &'a mut T {
        // SAFETY: the guard proves the lock is held; the C++ half takes the same lock.
        unsafe { &mut *self.0.get() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_lock_is_one_pointer_and_excludes() {
        assert_eq!(core::mem::size_of::<SrwLock>(), core::mem::size_of::<usize>());
        static LOCK: SrwLock = SrwLock::new();
        static VALUE: Guarded<u32> = Guarded::new(0);
        let threads: Vec<_> = (0..4)
            .map(|_| {
                std::thread::spawn(|| {
                    for _ in 0..1000 {
                        let g = LOCK.lock();
                        *VALUE.get(&g) += 1;
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let g = LOCK.lock();
        assert_eq!(*VALUE.get(&g), 4000);
    }
}
