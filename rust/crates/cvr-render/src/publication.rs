//! Owned publications of GPU resources between the capture hooks and the mirror (main's 8255f379b PublishedResource and
//! c41b2e23d ResourceCaptureSet): a publication owns only what it currently publishes, a consumer acquires a reference of
//! its own and keeps it until its GPU work is done, and nothing is referenced once per frame.
use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering::*};
use std::sync::{Mutex, MutexGuard};

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::ID3D12Resource;

/// THE SECOND VIEW'S OUTPUT, PUBLISHED (main's 8255f379b, PublishedResource): the publication owns only the current
/// output -- a reference of its own, the previous one let go when it changes. Menu graph rebuilds replace the output;
/// the 64 references the capture used to keep for the session pinned retired placed-resource heaps (and stopped
/// retaining after 64). A consumer acquires its own reference and keeps it until its GPU work is done (the mirror's
/// copy: its slot's fence). g_captured_vrcam_res carries the identity for comparisons only (never dereferenced).
pub struct Published {
    current: Mutex<Option<ID3D12Resource>>,
    identity: &'static AtomicPtr<c_void>,
}
impl Published {
    pub const fn new(identity: &'static AtomicPtr<c_void>) -> Self {
        Published { current: Mutex::new(None), identity }
    }
    /// `resource` the current output -> false when it already was.
    pub fn publish(&self, resource: *mut c_void) -> bool {
        // SAFETY: the caller's live resource or null.
        let next = unsafe { ID3D12Resource::from_raw_borrowed(&resource) }.cloned();
        let previous;
        {
            let mut current = self.current.lock().unwrap_or_else(|e| e.into_inner());
            if current.as_ref().map_or(null_mut(), Interface::as_raw) == resource {
                return false;
            }
            previous = core::mem::replace(&mut *current, next);
            self.identity.store(resource, Release);
        }
        // released outside the publication's lock: a driver's destruction can be slow
        drop(previous);
        true
    }
    /// A reference of the caller's own to the current output.
    pub fn acquire(&self) -> Option<ID3D12Resource> {
        self.current.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// One captured tonemap-chain input (c41b2e23d's ResourceCaptureSet entry): a reference of the set's own, its last
/// state, its format and the node's work RVA.
#[derive(Clone)]
pub struct TmEntry {
    pub resource: ID3D12Resource,
    pub state: u32,
    pub format: u32,
    pub tag: u32,
}
/// THE TONEMAP INPUT CAPTURE (main's c41b2e23d, ResourceCaptureSet<ID3D12Resource, 24>): an optional diagnostic of the
/// luma probe. Switched off or reset, the set lets go of its references (outside its lock); the mirror acquires its own
/// copy of the entries and keeps the sources it sampled until its slot's fence. A resource is compared by pointer
/// (never dereferenced while only borrowed) and referenced once, not each frame.
pub struct TmInputs {
    entries: Mutex<Vec<TmEntry>>,
    enabled: AtomicBool,
    count: AtomicUsize,
}
impl Default for TmInputs {
    fn default() -> Self {
        Self::new()
    }
}
impl TmInputs {
    pub const CAPACITY: usize = 24;
    pub const fn new() -> Self {
        TmInputs { entries: Mutex::new(Vec::new()), enabled: AtomicBool::new(false), count: AtomicUsize::new(0) }
    }
    fn entries(&self) -> MutexGuard<'_, Vec<TmEntry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn set_enabled(&self, next: bool) -> bool {
        if self.enabled.load(Acquire) == next {
            return next;
        }
        let retired;
        {
            let mut entries = self.entries();
            self.enabled.store(next, Release);
            retired = if next { Vec::new() } else { core::mem::take(&mut *entries) };
            if !next {
                self.count.store(0, Release);
            }
        }
        drop(retired);
        next
    }
    pub fn observe(&self, resource: *mut c_void, state: u32, format: u32, tag: u32) {
        if resource.is_null() || !self.enabled.load(Acquire) {
            return;
        }
        let mut entries = self.entries();
        if !self.enabled.load(Relaxed) {
            return;
        }
        if let Some(e) = entries.iter_mut().find(|e| e.resource.as_raw() == resource) {
            e.state = state;
            return;
        }
        if entries.len() == Self::CAPACITY {
            return;
        }
        // SAFETY: a live resource from the barrier being recorded.
        if let Some(r) = unsafe { ID3D12Resource::from_raw_borrowed(&resource) } {
            entries.push(TmEntry { resource: r.clone(), state, format, tag });
            self.count.store(entries.len(), Release);
        }
    }
    /// The last state of a resource the set holds (any list, any thread).
    pub fn update(&self, resource: *mut c_void, state: u32) {
        if resource.is_null() || self.count.load(Acquire) == 0 {
            return;
        }
        if let Some(e) = self.entries().iter_mut().find(|e| e.resource.as_raw() == resource) {
            e.state = state;
        }
    }
    pub fn acquire(&self) -> Vec<TmEntry> {
        self.entries().clone()
    }
    pub fn is_empty(&self) -> bool {
        self.count.load(Acquire) == 0
    }
    /// A rebuilt graph: a new publication; the mirror's leases keep only what its submitted readbacks read.
    pub fn reset(&self) {
        let retired;
        {
            let mut entries = self.entries();
            retired = core::mem::take(&mut *entries);
            self.count.store(0, Release);
        }
        drop(retired);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cvr_gpu::bench::Bench;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;

    fn references(r: &ID3D12Resource) -> u32 {
        // SAFETY: an AddRef and a Release of a live object: the count after the Release.
        unsafe {
            let raw = r.as_raw();
            let vtbl = *(raw as *const *const usize);
            let add_ref: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(*vtbl.add(1));
            let release: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(*vtbl.add(2));
            add_ref(raw);
            release(raw)
        }
    }

    #[test]
    fn a_publication_owns_only_its_current_resource() {
        static IDENTITY: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
        let mut b = Bench::warp();
        let first = b.texture(DXGI_FORMAT_R8G8B8A8_UNORM, 4, 4, false).resource;
        let second = b.texture(DXGI_FORMAT_R8G8B8A8_UNORM, 4, 4, false).resource;
        let base = references(&first);
        let p = Published::new(&IDENTITY);
        assert!(p.publish(first.as_raw()));
        assert!(!p.publish(first.as_raw()), "the same output published twice");
        assert_eq!(references(&first), base + 1, "one reference for the publication, not one per frame");
        assert_eq!(IDENTITY.load(Acquire), first.as_raw());
        let lease = p.acquire().unwrap();
        assert!(p.publish(second.as_raw()));
        // the first is let go by the publication; the consumer's lease still holds it
        assert_eq!(references(&first), base + 1);
        drop(lease);
        assert_eq!(references(&first), base);
        assert_eq!(IDENTITY.load(Acquire), second.as_raw());
    }

    #[test]
    fn the_capture_set_is_bounded_and_retires_when_off() {
        let mut b = Bench::warp();
        let textures: Vec<ID3D12Resource> = (0..30).map(|_| b.texture(DXGI_FORMAT_R8G8B8A8_UNORM, 4, 4, false).resource).collect();
        let base = references(&textures[0]);
        let set = TmInputs::new();
        set.observe(textures[0].as_raw(), 1, 28, 7);
        assert!(set.is_empty(), "observed while off");
        assert!(set.set_enabled(true));
        for t in &textures {
            set.observe(t.as_raw(), 0x40, 28, 7);
            set.observe(t.as_raw(), 0x80, 28, 7);
        }
        let entries = set.acquire();
        assert_eq!(entries.len(), TmInputs::CAPACITY);
        assert_eq!(entries[0].state, 0x80, "a second observation updates, not adds");
        set.update(textures[0].as_raw(), 0x800);
        assert_eq!(set.acquire()[0].state, 0x800);
        assert_eq!(references(&textures[0]), base + 2, "the set's reference and the acquired copy's");
        drop(entries);
        assert!(!set.set_enabled(false));
        assert!(set.is_empty());
        assert_eq!(references(&textures[0]), base, "switched off, the set let go");
        set.set_enabled(true);
        set.observe(textures[1].as_raw(), 0x40, 28, 7);
        set.reset();
        assert!(set.is_empty());
    }
}
