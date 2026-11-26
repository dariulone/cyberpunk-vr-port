//! Resource paths (`ResourcePath.hpp`): a depot path's hash, taken the way the SDK's constructor takes it -- quotes and
//! leading separators dropped, runs of separators made one backslash, letters lowercased, at most 216 characters --
//! then FNV-1a 64. `RaRef<T>` (an async resource reference) is that hash alone.
//!
//! And the resource loader's tokens (`ResourceLoader.hpp`): [`TokenRef`], a `SharedPtr<ResourceToken<T>>` from LoadAsync
//! or FindToken, given back as the SDK's `~SharedPtr` does.
use crate::fnv::fnv1a64;

const MAX_PATH_LENGTH: usize = 216;

fn separator(c: u8) -> bool {
    c == b'/' || c == b'\\'
}

/// `ResourcePath::HashSanitized(path)`; 0 for an empty path.
pub fn resource_path(path: &str) -> u64 {
    let input = path.as_bytes();
    let mut i = 0;
    if i < input.len() && (input[i] == b'"' || input[i] == b'\'') {
        i += 1;
    }
    while i < input.len() && separator(input[i]) {
        i += 1;
    }
    let mut buffer = Vec::with_capacity(MAX_PATH_LENGTH);
    while i < input.len() && input[i] != 0 && input[i] != b'"' && input[i] != b'\'' {
        if separator(input[i]) {
            buffer.push(b'\\');
            i += 1;
            while i < input.len() && separator(input[i]) {
                i += 1;
            }
        } else {
            buffer.push(input[i].to_ascii_lowercase());
            i += 1;
        }
        if buffer.len() == MAX_PATH_LENGTH {
            break;
        }
    }
    if buffer.is_empty() {
        return 0;
    }
    fnv1a64(&buffer)
}

// ---------------------------------------------------------------- the resource loader's tokens
use crate::dynarray::DynArray;
use crate::handle::{Handle, RefCnt, WeakHandle};
use crate::memory::Pool;
use crate::reloc::Reloc;
use core::ffi::c_void;
use core::sync::atomic::{AtomicI8, Ordering};

static LOADER: Reloc = Reloc::new(crate::hashes::ResourceLoader);
static LOAD_BY_PATH: Reloc = Reloc::new(crate::hashes::ResourceLoader_IssueLoadingRequestByPath);
static FIND_TOKEN: Reloc = Reloc::new(crate::hashes::ResourceLoader_FindTokenFast);
static CANCEL_UNK38: Reloc = Reloc::new(crate::hashes::ResourceToken_CancelUnk38);
static DESTRUCT_UNK38: Reloc = Reloc::new(crate::hashes::ResourceToken_DestructUnk38);
static JOB_DTOR: Reloc = Reloc::new(crate::hashes::JobHandle_dtor);
/// ResourceToken's allocator (`Memory::EngineAllocator`) and a SharedPtr's counts' (`Memory::RefCountAllocator`).
static POOL_ENGINE: Pool = Pool::new("PoolEngine");

// ResourceToken<T> (0x60): self (a WeakPtr) +0, dependencies (DynArray<SharedPtr<ResourceToken>>) +0x10, lock +0x20,
// resource (a Handle) +0x28, unk38 (a SharedPtr's instance and counts) +0x38 / +0x40, path +0x48, job (a JobHandle)
// +0x50, finished +0x58, error +0x5C.
const TOKEN_SELF: usize = 0x00;
const TOKEN_DEPENDENCIES: usize = 0x10;
const TOKEN_RESOURCE: usize = 0x28;
const TOKEN_UNK38: usize = 0x38;
const TOKEN_PATH: usize = 0x48;
const TOKEN_JOB: usize = 0x50;
const TOKEN_FINISHED: usize = 0x58;
const TOKEN_ERROR: usize = 0x5C;
/// ResourceLoader::tokenLock (a SharedSpinLock: -1 held, else the readers).
const LOADER_TOKEN_LOCK: usize = 0x40;

/// `SharedPtr<ResourceToken<T>>`: the token and its counts. Dropping it is the SDK's `~SharedPtr`: the strong count
/// given back, and at zero the counts' weak share given back (the engine frees them at zero) and the token destroyed
/// as `Memory::Delete` does -- `~ResourceToken` (its pending load cancelled, its unk38 destructed, then its members:
/// the job, the resource's handle, the dependencies, the self reference) and its memory back to the engine pool.
#[repr(C)]
pub struct TokenRef {
    pub instance: *mut c_void,
    pub ref_count: *mut RefCnt,
}
crate::assert_layout!(TokenRef, size = 0x10, ref_count = 8);
// SAFETY: the counts are atomic and the token is the engine's, shared between its threads.
unsafe impl Send for TokenRef {}
unsafe impl Sync for TokenRef {}

fn loader() -> *mut u8 {
    // SAFETY: the resolved global holds the loader's pointer.
    LOADER.get().map_or(core::ptr::null_mut(), |at| unsafe { *(at as *const *mut u8) })
}
impl TokenRef {
    pub const fn null() -> TokenRef {
        TokenRef { instance: core::ptr::null_mut(), ref_count: core::ptr::null_mut() }
    }
    pub fn is_null(&self) -> bool {
        self.instance.is_null()
    }
    fn at<T: Copy>(&self, offset: usize) -> T {
        // SAFETY: a live token is 0x60 bytes.
        unsafe { core::ptr::read_volatile((self.instance as *const u8).add(offset) as *const T) }
    }
    /// `IsLoaded()`: finished and no error.
    pub fn loaded(&self) -> bool {
        !self.is_null() && self.at::<i32>(TOKEN_FINISHED) != 0 && self.at::<u8>(TOKEN_ERROR) == 0
    }
    /// `IsFailed()`.
    pub fn failed(&self) -> bool {
        !self.is_null() && self.at::<u8>(TOKEN_ERROR) != 0
    }
    /// `IsFinished()`: loaded or failed.
    pub fn finished(&self) -> bool {
        self.loaded() || self.failed()
    }
    /// The loaded resource's instance (`Get()`), null before.
    pub fn resource(&self) -> *mut c_void {
        if self.is_null() {
            return core::ptr::null_mut();
        }
        self.at::<*mut c_void>(TOKEN_RESOURCE)
    }
    /// The token's path.
    pub fn path(&self) -> u64 {
        if self.is_null() {
            0
        } else {
            self.at::<u64>(TOKEN_PATH)
        }
    }
    /// `ResourceLoader::Get()->LoadAsync<T>(path)`: the token of the resource, loading if it is not.
    pub fn load_async(path: u64) -> TokenRef {
        type F = unsafe extern "C" fn(*mut u8, *mut TokenRef, u64) -> usize;
        let mut out = TokenRef::null();
        let l = loader();
        // SAFETY: IssueLoadingRequestByPath's signature per ResourceLoader.hpp; it writes a SharedPtr into `out`.
        if let (false, Some(load)) = (l.is_null(), unsafe { LOAD_BY_PATH.func::<F>() }) {
            unsafe { load(l, &mut out, path) };
        }
        out
    }
    /// `ResourceLoader::Get()->FindToken<T>(path)`: the token of a resource the loader knows, without loading; null
    /// for one it does not. Under the loader's token lock, shared.
    pub fn find(path: u64) -> TokenRef {
        type F = unsafe extern "C" fn(*mut u8, *mut TokenRef, u64) -> usize;
        let mut out = TokenRef::null();
        let l = loader();
        // SAFETY: FindTokenFast's signature per ResourceLoader.hpp; the lock is the loader's own.
        let Some(find) = (unsafe { FIND_TOKEN.func::<F>() }) else { return out };
        if l.is_null() {
            return out;
        }
        let lock = unsafe { &*(l.add(LOADER_TOKEN_LOCK) as *const AtomicI8) };
        let mut spins = 0u32;
        loop {
            let current = lock.load(Ordering::Acquire);
            if current != -1 && lock.compare_exchange(current, current + 1, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                break;
            }
            spins += 1;
            if spins % 512 == 0 {
                std::thread::yield_now();
            }
        }
        unsafe { find(l, &mut out, path) };
        lock.fetch_sub(1, Ordering::AcqRel);
        out
    }
    /// The token for its resource's path once more (a new strong reference), as `SharedPtr(const SharedPtr&)`.
    pub fn share(&self) -> TokenRef {
        // SAFETY: a strong reference keeps the counts alive.
        if let Some(c) = unsafe { self.ref_count.as_ref() } {
            c.strong.fetch_add(1, Ordering::AcqRel);
        }
        TokenRef { instance: self.instance, ref_count: self.ref_count }
    }
}
/// `~ResourceToken()` in place, its members after its body (the SDK's declared order, reversed).
///
/// # Safety
/// A token whose last strong reference is gone.
unsafe fn destroy_token(t: *mut u8) {
    unsafe {
        let finished = core::ptr::read_volatile(t.add(TOKEN_FINISHED) as *const i32) != 0;
        let error = *t.add(TOKEN_ERROR) != 0;
        if !((finished && !error) || error) {
            if let Some(cancel) = CANCEL_UNK38.func::<unsafe extern "C" fn(*mut c_void)>() {
                cancel(*(t.add(TOKEN_UNK38) as *const *mut c_void));
            }
        }
        if let Some(destruct) = DESTRUCT_UNK38.func::<unsafe extern "C" fn(*mut *mut c_void)>() {
            destruct(t.add(TOKEN_UNK38) as *mut *mut c_void);
        }
        if let Some(job) = JOB_DTOR.func::<unsafe extern "C" fn(*mut c_void)>() {
            job(t.add(TOKEN_JOB) as *mut c_void);
        }
        core::ptr::drop_in_place(t.add(TOKEN_RESOURCE) as *mut Handle);
        let dependencies = &mut *(t.add(TOKEN_DEPENDENCIES) as *mut DynArray<[usize; 2]>);
        for d in dependencies.as_slice().to_vec() {
            drop(TokenRef { instance: d[0] as *mut c_void, ref_count: d[1] as *mut RefCnt });
        }
        dependencies.free();
        core::ptr::drop_in_place(t.add(TOKEN_SELF) as *mut WeakHandle);
    }
}
impl Drop for TokenRef {
    fn drop(&mut self) {
        // SAFETY: the counts this reference holds a strong share of.
        unsafe {
            let Some(counts) = self.ref_count.as_ref() else { return };
            if counts.strong.fetch_sub(1, Ordering::AcqRel) == 1 {
                // DecWeakRef: the engine's own, which frees the counts at zero (as Handle's drop uses it)
                let pair = WeakHandle { instance: self.instance, ref_count: self.ref_count };
                drop(pair);
                let t = self.instance as *mut u8;
                if !t.is_null() {
                    destroy_token(t);
                    POOL_ENGINE.free(t as *mut c_void);
                }
            }
        }
        self.instance = core::ptr::null_mut();
        self.ref_count = core::ptr::null_mut();
    }
}

#[cfg(test)]
mod tests {
    use super::resource_path;
    #[test]
    fn sanitizes_like_the_sdk() {
        assert_eq!(resource_path(""), 0);
        assert_eq!(resource_path("vrport\\ui\\vr_menu.inkwidget"), resource_path("\"/VRPORT//ui/vr_menu.inkwidget\""));
        assert_eq!(resource_path("a\\b"), crate::fnv::fnv1a64(b"a\\b"));
    }
}
