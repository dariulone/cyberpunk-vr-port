//! RTV metadata by descriptor handle (formerly include/Render/DescriptorCache.hpp): the resource and size a CPU
//! descriptor names, kept by use rather than by creation age. Metadata only -- no COM references, no GPU allocations.
//! A recycled descriptor publishes resource and size together; a null target erases the entry. The capacity is a
//! floor that Reserve raises to the descriptor slots the engine's heaps hold; past it the least recently used entry
//! whose resource is not pinned (the swapchain's buffers, which the engine never re-creates) is evicted.
//!
//! The checks are tools/descriptor_cache_tests' (all but image_identity, which pairs it with the C++ pose ledger),
//! run as this file's tests.
#![allow(unsafe_op_in_unsafe_fn)]
use cvr_host::fast::{FastMap, FastSet};
use std::collections::BTreeMap;
use std::sync::Mutex;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DescriptorTarget {
    pub resource: usize,
    pub width: u32,
    pub height: u32,
}
struct Inner {
    /// handle -> (target, recency stamp)
    entries: FastMap<usize, (DescriptorTarget, u64)>,
    /// recency stamp -> handle, oldest first
    recent: BTreeMap<u64, usize>,
    pinned: FastSet<usize>,
    capacity: usize,
    clock: u64,
}
impl Inner {
    fn touch(&mut self, handle: usize) {
        self.clock += 1;
        let clock = self.clock;
        if let Some(entry) = self.entries.get_mut(&handle) {
            self.recent.remove(&entry.1);
            entry.1 = clock;
            self.recent.insert(clock, handle);
        }
    }
}
pub struct DescriptorCache {
    inner: Mutex<Inner>,
}
impl DescriptorCache {
    pub fn new(capacity: usize) -> DescriptorCache {
        assert!(capacity > 0);
        DescriptorCache {
            inner: Mutex::new(Inner {
                entries: cvr_host::fast::fast_map(),
                recent: BTreeMap::new(),
                pinned: cvr_host::fast::fast_set(),
                capacity,
                clock: 0,
            }),
        }
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// Raise the capacity to `slots` (never lower it).
    pub fn reserve(&self, slots: usize) {
        let mut i = self.lock();
        if slots > i.capacity {
            i.capacity = slots;
        }
    }
    pub fn limit(&self) -> usize {
        self.lock().capacity
    }
    /// The resources that are never evicted (replacing the previous set: a resized swapchain unpins its old buffers).
    pub fn pin_resources(&self, resources: &[usize]) {
        let mut i = self.lock();
        i.pinned = resources.iter().copied().filter(|&p| p != 0).collect();
    }
    /// Publish `target` for `handle` (a null resource erases it). False when nothing evictable is left; `evicted`
    /// says whether an entry was evicted to make room.
    pub fn store(&self, handle: usize, target: DescriptorTarget, evicted: &mut bool) -> bool {
        *evicted = false;
        if handle == 0 {
            return false;
        }
        let mut i = self.lock();
        if target.resource == 0 {
            if let Some((_, stamp)) = i.entries.remove(&handle) {
                i.recent.remove(&stamp);
            }
            return true;
        }
        if let Some(entry) = i.entries.get_mut(&handle) {
            entry.0 = target;
            i.touch(handle);
            return true;
        }
        if i.entries.len() >= i.capacity {
            let victim = i
                .recent
                .iter()
                .find(|(_, h)| !i.pinned.contains(&i.entries[*h].0.resource))
                .map(|(&stamp, &h)| (stamp, h));
            let Some((stamp, h)) = victim else {
                return false;
            };
            i.recent.remove(&stamp);
            i.entries.remove(&h);
            *evicted = true;
        }
        i.clock += 1;
        let clock = i.clock;
        i.entries.insert(handle, (target, clock));
        i.recent.insert(clock, handle);
        true
    }
    /// The target `handle` names, marked as just used.
    pub fn read(&self, handle: usize) -> Option<DescriptorTarget> {
        let mut i = self.lock();
        let target = i.entries.get(&handle)?.0;
        i.touch(handle);
        Some(target)
    }
    pub fn size(&self) -> usize {
        self.lock().entries.len()
    }
}

/// The stereo module's RTV metadata (Capture.cpp's rtvDescriptors), floor 8192 (RtvDescriptorCapacity).
#[cfg(feature = "rust-build")]
static RTV: std::sync::LazyLock<DescriptorCache> =
    std::sync::LazyLock::new(|| DescriptorCache::new(8192));
#[cfg(feature = "rust-build")]
#[no_mangle]
pub extern "C" fn cvr_rtv_cache_reserve(slots: usize) {
    RTV.reserve(slots)
}
#[cfg(feature = "rust-build")]
#[no_mangle]
pub extern "C" fn cvr_rtv_cache_limit() -> usize {
    RTV.limit()
}
/// # Safety
/// `resources` holds `count` values.
#[cfg(feature = "rust-build")]
#[no_mangle]
pub unsafe extern "C" fn cvr_rtv_cache_pin(resources: *const usize, count: usize) {
    if !resources.is_null() {
        RTV.pin_resources(core::slice::from_raw_parts(resources, count))
    }
}
/// # Safety
/// `evicted` is null or writable.
#[cfg(feature = "rust-build")]
#[no_mangle]
pub unsafe extern "C" fn cvr_rtv_cache_store(
    handle: usize,
    target: DescriptorTarget,
    evicted: *mut bool,
) -> bool {
    let mut e = false;
    let stored = RTV.store(handle, target, &mut e);
    if let Some(out) = evicted.as_mut() {
        *out = e;
    }
    stored
}
/// # Safety
/// `out` is writable.
#[cfg(feature = "rust-build")]
#[no_mangle]
pub unsafe extern "C" fn cvr_rtv_cache_read(handle: usize, out: *mut DescriptorTarget) -> bool {
    match RTV.read(handle) {
        Some(target) => {
            *out = target;
            true
        }
        None => false,
    }
}
#[cfg(feature = "rust-build")]
#[no_mangle]
pub extern "C" fn cvr_rtv_cache_size() -> usize {
    RTV.size()
}

// ---- the C ABI of one cache, for include/Render/DescriptorCache.hpp's template in the transitional build ------------
// (CVR_RUST_VRIK: the template owns a Rust cache made with its Capacity; tools/descriptor_cache_tests runs its cases
// against it in its Rust variant).

/// A cache with the floor `capacity` (released by `cvr_dcache_free`).
#[no_mangle]
pub extern "C-unwind" fn cvr_dcache_new(capacity: usize) -> *mut DescriptorCache {
    Box::into_raw(Box::new(DescriptorCache::new(capacity)))
}
/// # Safety
/// `cache` came from `cvr_dcache_new` (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dcache_free(cache: *mut DescriptorCache) {
    if !cache.is_null() {
        drop(Box::from_raw(cache));
    }
}
/// Reserve (`which` 0, `value` the slots, returns 0), Limit (1), Size (2).
///
/// # Safety
/// `cache` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dcache_count(
    cache: *const DescriptorCache,
    which: u32,
    value: usize,
) -> usize {
    let cache = &*cache;
    match which {
        0 => {
            cache.reserve(value);
            0
        }
        1 => cache.limit(),
        _ => cache.size(),
    }
}
/// PinResources.
///
/// # Safety
/// `cache` is valid; `resources` holds `count` values (or is null with none).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dcache_pin(
    cache: *const DescriptorCache,
    resources: *const usize,
    count: usize,
) {
    let resources = if resources.is_null() || count == 0 {
        &[][..]
    } else {
        core::slice::from_raw_parts(resources, count)
    };
    (*cache).pin_resources(resources)
}
/// Store (`evicted` null or written).
///
/// # Safety
/// `cache` and `target` are valid; `evicted` is null or writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dcache_store(
    cache: *const DescriptorCache,
    handle: usize,
    target: *const DescriptorTarget,
    evicted: *mut bool,
) -> bool {
    let mut e = false;
    let stored = (*cache).store(handle, *target, &mut e);
    if let Some(out) = evicted.as_mut() {
        *out = e;
    }
    stored
}
/// Read (`out` written when found).
///
/// # Safety
/// `cache` is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dcache_read(
    cache: *const DescriptorCache,
    handle: usize,
    out: *mut DescriptorTarget,
) -> bool {
    match (*cache).read(handle) {
        Some(target) => {
            *out = target;
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::*};
    use std::sync::Arc;

    fn t(resource: usize, width: u32, height: u32) -> DescriptorTarget {
        DescriptorTarget {
            resource,
            width,
            height,
        }
    }
    fn put(c: &DescriptorCache, handle: usize, target: DescriptorTarget) -> bool {
        let mut evicted = false;
        c.store(handle, target, &mut evicted)
    }

    #[test]
    fn startup_churn() {
        let cache = DescriptorCache::new(128);
        let buffers = [1usize, 2, 3];
        cache.pin_resources(&buffers);
        for p in buffers {
            put(&cache, p, t(p, 2560, 2560));
        }
        for i in 4..300_000usize {
            assert!(put(&cache, i, t(i, 512, 256)), "churn insert failed");
        }
        for p in buffers {
            assert!(
                cache.read(p).is_some(),
                "never-recreated swapchain descriptor was evicted before first draw"
            );
        }
        assert_eq!(cache.size(), 128, "unbounded metadata growth");
    }
    #[test]
    fn active() {
        let cache = DescriptorCache::new(4);
        put(&cache, 1, t(11, 2560, 2560));
        for i in 2..10_000usize {
            assert!(cache.read(1).is_some(), "live HUD target was evicted");
            put(&cache, i, t(i + 20, 32, 32));
        }
        assert_eq!(
            cache.read(1).unwrap().resource,
            11,
            "active descriptor changed identity"
        );
    }
    #[test]
    fn replacement() {
        let cache = DescriptorCache::new(4);
        cache.pin_resources(&[11]);
        put(&cache, 1, t(11, 2560, 1440));
        put(&cache, 1, t(99, 800, 600));
        assert_eq!(
            cache.read(1),
            Some(t(99, 800, 600)),
            "recycled descriptor retained stale resource or size"
        );
        put(&cache, 1, DescriptorTarget::default());
        assert!(
            cache.read(1).is_none(),
            "null RTV inherited old pose target"
        );
        assert_eq!(cache.size(), 0, "null descriptor retained metadata");
    }
    #[test]
    fn resize() {
        let cache = DescriptorCache::new(4);
        cache.pin_resources(&[1, 2]);
        put(&cache, 1, t(1, 100, 100));
        put(&cache, 2, t(2, 100, 100));
        cache.pin_resources(&[3, 4]);
        put(&cache, 3, t(3, 200, 200));
        put(&cache, 4, t(4, 200, 200));
        for i in 5..1000usize {
            put(&cache, i, t(i, 10, 10));
        }
        assert!(
            cache.read(1).is_none() && cache.read(2).is_none(),
            "resize left obsolete buffers permanently pinned"
        );
        assert!(
            cache.read(3).is_some() && cache.read(4).is_some(),
            "new swapchain buffers were not pinned"
        );
    }
    #[test]
    fn concurrent() {
        let cache = Arc::new(DescriptorCache::new(16));
        let done = Arc::new(AtomicBool::new(false));
        let (failures, reads) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0)));
        put(&cache, 1, t(1, 1, !1));
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let (cache, done, failures, reads) =
                    (cache.clone(), done.clone(), failures.clone(), reads.clone());
                std::thread::spawn(move || {
                    while !done.load(Relaxed) {
                        if let Some(v) = cache.read(1) {
                            if v.resource != v.width as usize || v.height != !v.width {
                                failures.fetch_add(1, Relaxed);
                            }
                            reads.fetch_add(1, Relaxed);
                        }
                    }
                })
            })
            .collect();
        for i in 2..100_000usize {
            put(&cache, 1, t(i, i as u32, !(i as u32)));
        }
        done.store(true, Relaxed);
        for w in workers {
            w.join().unwrap();
        }
        assert!(
            reads.load(Relaxed) > 0 && failures.load(Relaxed) == 0,
            "lookup combined fields from different descriptor generations"
        );
    }
    #[test]
    fn all_pinned() {
        let cache = DescriptorCache::new(2);
        cache.pin_resources(&[1, 2]);
        put(&cache, 1, t(1, 1, 1));
        put(&cache, 2, t(2, 2, 2));
        assert!(!put(&cache, 3, t(3, 3, 3)), "evicted protected descriptor");
        assert!(
            cache.read(1).is_some() && cache.read(2).is_some(),
            "failed insert corrupted existing targets"
        );
        put(&cache, 1, DescriptorTarget::default());
        assert!(
            put(&cache, 3, t(3, 3, 3)),
            "erased slot could not be reused"
        );
    }
    #[test]
    fn reserve() {
        let cache = DescriptorCache::new(4);
        put(&cache, 1, t(1, 10, 10));
        cache.reserve(20_000);
        assert_eq!(cache.limit(), 20_000, "reserve not applied");
        for i in 2..=20_000usize {
            assert!(
                put(&cache, i, t(i, 10, 10)),
                "insert within reserved slots failed"
            );
        }
        assert_eq!(cache.size(), 20_000, "evicted below the reserved capacity");
        assert!(
            cache.read(1).is_some() && cache.read(20_000).is_some(),
            "reserved entries lost"
        );
        cache.reserve(8);
        assert_eq!(cache.limit(), 20_000, "reserve lowered the capacity");
    }
}
