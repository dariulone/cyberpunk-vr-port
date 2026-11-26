//! The pipeline variant index (formerly include/Render/PipelineVariants.hpp's PipelineVariantIndex<1>): an original
//! PSO owns its variant and its mask twin through COM private data under the index's key, so they die with it and a
//! new PSO at the same address starts without them. Lookups go through an address index that an owner leaves when the
//! original's destruction releases it -- before that address can come back -- so a lookup never calls into the
//! pipeline (a draw hook asks about the pipeline last set on its thread, which can be one destroyed since).
//!
//! Two indexes use it: the optics' glass pipelines (optics.rs: the lens velocity variant and the mask) and the sight
//! pipelines (cvr-render's pso_hooks.rs: the scope variant and the history-mask twin).

use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_NOINTERFACE, E_OUTOFMEMORY, E_POINTER, S_FALSE, S_OK};
use windows::Win32::Graphics::Direct3D12::ID3D12PipelineState;

const SLOTS: usize = 4096;
const LIMIT: usize = SLOTS * 3 / 4;
const EMPTY: usize = 0;
const TOMBSTONE: usize = 1;

/// PipelineVariants<1>: the original's variant and mask (either may be absent), owned through the original's private
/// data.
#[repr(C)]
pub struct Variants {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    pub variant: Option<ID3D12PipelineState>,
    pub mask: Option<ID3D12PipelineState>,
    original: usize,
    index: *const VariantIndex,
    indexed: bool,
}
unsafe extern "system" fn variants_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's output and interface id.
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || *iid != IUnknown::IID {
            return E_NOINTERFACE;
        }
        *out = this;
        variants_add_ref(this);
    }
    S_OK
}
unsafe extern "system" fn variants_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live entry.
    unsafe { (*(this as *const Variants)).refs.fetch_add(1, Ordering::Relaxed) + 1 }
}
unsafe extern "system" fn variants_release(this: *mut c_void) -> u32 {
    // SAFETY: a live entry; the last reference frees it (its index slot left first).
    unsafe {
        let n = (*(this as *const Variants)).refs.fetch_sub(1, Ordering::AcqRel) - 1;
        if n == 0 {
            let variants = Box::from_raw(this as *mut Variants);
            if variants.indexed {
                (*variants.index).remove(variants.original, this as *const Variants);
            }
        }
        n
    }
}
static VARIANTS_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: variants_query, AddRef: variants_add_ref, Release: variants_release };

/// A lock the same thread may take again (SetPrivateDataInterface under it may release an owner, whose destruction
/// removes it).
struct ReentrantLock {
    owner: AtomicUsize,
    depth: Cell<usize>,
    lock: Mutex<()>,
    guard: RefCell<Option<MutexGuard<'static, ()>>>,
}
// SAFETY: depth and guard are touched only by the owning thread.
unsafe impl Sync for ReentrantLock {}
fn thread_key() -> usize {
    thread_local!(static KEY: u8 = const { 0 });
    KEY.with(|k| k as *const u8 as usize)
}
impl ReentrantLock {
    const fn new() -> Self {
        ReentrantLock { owner: AtomicUsize::new(0), depth: Cell::new(0), lock: Mutex::new(()), guard: RefCell::new(None) }
    }
    fn enter(&'static self) {
        let me = thread_key();
        if self.owner.load(Ordering::Acquire) == me {
            self.depth.set(self.depth.get() + 1);
            return;
        }
        let guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        self.owner.store(me, Ordering::Release);
        self.depth.set(1);
        *self.guard.borrow_mut() = Some(guard);
    }
    fn leave(&'static self) {
        let depth = self.depth.get() - 1;
        self.depth.set(depth);
        if depth == 0 {
            self.owner.store(0, Ordering::Release);
            // The guard taken out first, its borrow ended, THEN the mutex released: released inside the borrow's
            // statement, another thread's enter took the lock and found the RefCell still borrowed (a panic, the
            // game aborted while the sight's pipelines were made, 2026-11-23).
            let guard = self.guard.borrow_mut().take();
            drop(guard);
        }
    }
}
/// PipelineVariantIndex<1>: open addressing over the originals' addresses (tombstones keep probes going).
pub struct VariantIndex {
    key: GUID,
    pso: [AtomicUsize; SLOTS],
    entry: [AtomicPtr<Variants>; SLOTS],
    live: AtomicU32,
    generation: AtomicU64,
    used: AtomicUsize,
    lock: ReentrantLock,
}
fn home(v: usize) -> usize {
    let mut v = v as u64;
    v ^= v >> 21;
    v = v.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    ((v >> 40) as usize) & (SLOTS - 1)
}
impl VariantIndex {
    pub const fn new(key: GUID) -> Self {
        VariantIndex {
            key,
            pso: [const { AtomicUsize::new(EMPTY) }; SLOTS],
            entry: [const { AtomicPtr::new(core::ptr::null_mut()) }; SLOTS],
            live: AtomicU32::new(0),
            generation: AtomicU64::new(0),
            used: AtomicUsize::new(0),
            lock: ReentrantLock::new(),
        }
    }
    /// Changes with every store and removal.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
    /// Lock-free: only reads the index; an entry it returns stays valid while the original is alive.
    pub fn find(&self, original: usize) -> Option<&Variants> {
        if original == 0 || self.live.load(Ordering::Acquire) == 0 {
            return None;
        }
        let mut i = home(original);
        for _ in 0..SLOTS {
            let at = self.pso[i].load(Ordering::Acquire);
            if at == original {
                // SAFETY: an indexed owner lives while its original does.
                return unsafe { self.entry[i].load(Ordering::Acquire).as_ref() };
            }
            if at == EMPTY {
                return None;
            }
            i = (i + 1) & (SLOTS - 1);
        }
        None
    }
    /// Consumes both creation references, also on failure or a duplicate registration. S_FALSE: the original
    /// already has variants (a driver may hand back an existing immutable PSO); they stay, for commands recorded with
    /// them.
    ///
    /// # Safety
    /// `original` is a live pipeline or null; `variant` and `mask` are owned references or null.
    pub unsafe fn store(&'static self, original: *mut c_void, variant: *mut c_void, mask: *mut c_void) -> HRESULT {
        // SAFETY: as documented (the references are taken over).
        let variants = Box::into_raw(Box::new(Variants {
            vtbl: &VARIANTS_VTBL,
            refs: AtomicU32::new(1),
            variant: (!variant.is_null()).then(|| unsafe { ID3D12PipelineState::from_raw(variant) }),
            mask: (!mask.is_null()).then(|| unsafe { ID3D12PipelineState::from_raw(mask) }),
            original: original as usize,
            index: self,
            indexed: false,
        }));
        // The creation reference, released on every return.
        // SAFETY: the entry just made, an IUnknown.
        let entry = unsafe { IUnknown::from_raw(variants.cast()) };
        // SAFETY: as documented.
        let Some(pso) = (unsafe { ID3D12PipelineState::from_raw_borrowed(&original) }) else {
            return E_POINTER;
        };
        let address = original as usize;
        self.lock.enter();
        let result = (|| {
            if self.find(address).is_some() {
                return S_FALSE;
            }
            let mut slot = None;
            let mut i = home(address);
            for _ in 0..SLOTS {
                let at = self.pso[i].load(Ordering::Relaxed);
                if at == EMPTY || at == TOMBSTONE {
                    slot = Some(i);
                    break;
                }
                i = (i + 1) & (SLOTS - 1);
            }
            let Some(slot) = slot else {
                return E_OUTOFMEMORY;
            };
            let fresh = self.pso[slot].load(Ordering::Relaxed) == EMPTY;
            if fresh && self.used.load(Ordering::Relaxed) >= LIMIT {
                return E_OUTOFMEMORY;
            }
            // SAFETY: the live original takes a reference to the entry.
            if let Err(e) = unsafe { pso.SetPrivateDataInterface(&self.key, &entry) } {
                return e.code();
            }
            // SAFETY: the entry is ours until published.
            unsafe { (*variants).indexed = true };
            if fresh {
                self.used.fetch_add(1, Ordering::Relaxed);
            }
            self.entry[slot].store(variants, Ordering::Release);
            self.pso[slot].store(address, Ordering::Release);
            self.live.fetch_add(1, Ordering::Release);
            self.generation.fetch_add(1, Ordering::AcqRel);
            S_OK
        })();
        self.lock.leave();
        drop(entry);
        result
    }
    /// The original is being destroyed: its address leaves the index before it can be reused.
    fn remove(&'static self, address: usize, entry: *const Variants) {
        self.lock.enter();
        let mut i = home(address);
        for _ in 0..SLOTS {
            let at = self.pso[i].load(Ordering::Relaxed);
            if at == EMPTY {
                break;
            }
            if at == address && self.entry[i].load(Ordering::Relaxed) as *const Variants == entry {
                self.entry[i].store(core::ptr::null_mut(), Ordering::Release);
                self.pso[i].store(TOMBSTONE, Ordering::Release);
                self.live.fetch_sub(1, Ordering::Release);
                self.generation.fetch_add(1, Ordering::AcqRel);
                break;
            }
            i = (i + 1) & (SLOTS - 1);
        }
        self.lock.leave();
    }
}
