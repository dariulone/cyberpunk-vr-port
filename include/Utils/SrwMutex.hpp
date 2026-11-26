#pragma once
// A mutex both halves of the port can take: a Windows slim reader/writer lock, held exclusively. Its layout is one
// pointer (SRWLOCK), zero when free, so Rust holds the same lock over the same memory (rust/crates/cvr-xr/src/srw.rs).
// BasicLockable and Lockable, so std::lock_guard and std::unique_lock take it like a std::mutex.
#include <windows.h>

namespace cvr {
struct SrwMutex {
    SRWLOCK srw = SRWLOCK_INIT;
    void lock() { AcquireSRWLockExclusive(&srw); }
    bool try_lock() { return TryAcquireSRWLockExclusive(&srw) != 0; }
    void unlock() { ReleaseSRWLockExclusive(&srw); }
};
static_assert(sizeof(SrwMutex) == sizeof(void*));
}  // namespace cvr
