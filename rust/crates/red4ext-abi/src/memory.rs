//! The engine's memory pools (`Memory/Vault.hpp`, `Pool.hpp`, `Allocators.hpp`). Anything the engine will free
//! itself -- an RTTI function, a property, an array buffer -- must come from the pool the engine frees it to.
use crate::assert_layout;
use crate::fnv::fnv1a32;
use crate::hashes;
use crate::reloc::Reloc;
use core::ffi::c_void;
use core::sync::atomic::{AtomicI8, AtomicPtr, Ordering};

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AllocationResult {
    pub memory: *mut c_void,
    pub size: usize,
}
assert_layout!(AllocationResult, size = 16);

#[repr(C)]
pub struct PoolStorage {
    pub allocator_storage: u64,
    pub bytes_allocated: u64,
    pub max_bytes_allocated: u64,
    pub oom_handler: *mut c_void,
    pub allocator_handle: u32,
    pub allocator_id: u32,
}
assert_layout!(PoolStorage, size = 0x28, allocator_handle = 0x20);

#[repr(C)]
pub struct PoolInfo {
    pub budget: u64,
    pub children_budget: u64,
    pub storage: *mut PoolStorage,
    pub child: *mut PoolInfo,
    pub sibling: *mut PoolInfo,
    pub name: [u8; 32],
    pub handle: u32,
    pub contribute_to_parent_metrics: bool,
    pub is_mirrored: bool,
}
assert_layout!(PoolInfo, size = 0x50, storage = 0x10, name = 0x28, handle = 0x48);

const MAX_POOL_COUNT: usize = 768;

#[repr(C)]
pub struct PoolRegistry {
    /// RED4ext::SharedSpinLock: -1 exclusively held, n > 0 held by n readers.
    pub nodes_lock: AtomicI8,
    pub nodes: [PoolInfo; MAX_POOL_COUNT],
}
assert_layout!(PoolRegistry, size = 0xF008, nodes = 8);

#[repr(C)]
pub struct Vault {
    _unk0: [u8; 0xD180],
    pub pool_registry: PoolRegistry,
}
assert_layout!(Vault, size = 0x1C188, pool_registry = 0xD180);

static VAULT: Reloc = Reloc::new(hashes::Memory_Vault);
static VAULT_ALLOC: Reloc = Reloc::new(hashes::Memory_Vault_Alloc);
static VAULT_FREE: Reloc = Reloc::new(hashes::Memory_Vault_Free);

type AllocFn = unsafe extern "C" fn(*mut c_void, *mut AllocationResult, u64);
type FreeFn = unsafe extern "C" fn(*mut c_void, *mut AllocationResult);

impl PoolRegistry {
    /// SharedSpinLock::LockShared / UnlockShared around a scan for `handle`.
    fn find(&self, handle: u32) -> *mut PoolInfo {
        let mut spins = 0u32;
        loop {
            let state = self.nodes_lock.load(Ordering::Relaxed);
            if state != -1
                && self
                    .nodes_lock
                    .compare_exchange(state, state.wrapping_add(1), Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
            {
                break;
            }
            spins = spins.wrapping_add(1);
            if spins & 511 == 0 {
                std::thread::yield_now();
            }
        }
        let found = self
            .nodes
            .iter()
            .find(|n| n.handle == handle)
            .map_or(core::ptr::null_mut(), |n| n as *const PoolInfo as *mut PoolInfo);
        self.nodes_lock.fetch_sub(1, Ordering::Release);
        found
    }
}

/// A named engine pool (`TPoolInfo<T>::Get`), looked up once.
pub struct Pool {
    name: &'static str,
    info: AtomicPtr<PoolInfo>,
}

pub static POOL_RTTI: Pool = Pool::new("PoolRTTI");
pub static POOL_RTTI_FUNCTION: Pool = Pool::new("PoolRTTIFunction");
pub static POOL_RTTI_PROPERTY: Pool = Pool::new("PoolRTTIProperty");
pub static POOL_SCRIPT: Pool = Pool::new("PoolScript");
pub static POOL_DEFAULT: Pool = Pool::new("PoolDefault");

impl Pool {
    pub const fn new(name: &'static str) -> Self {
        Pool { name, info: AtomicPtr::new(core::ptr::null_mut()) }
    }

    fn info(&self) -> Option<*mut PoolInfo> {
        let cached = self.info.load(Ordering::Acquire);
        if !cached.is_null() {
            return Some(cached);
        }
        let vault = VAULT.get()? as *const Vault;
        // SAFETY: Memory_Vault is the engine's vault for the process lifetime.
        let found = unsafe { (*vault).pool_registry.find(fnv1a32(self.name.as_bytes())) };
        if found.is_null() {
            return None;
        }
        self.info.store(found, Ordering::Release);
        Some(found)
    }

    fn storage(&self) -> Option<*mut c_void> {
        let info = self.info()?;
        // SAFETY: a registered pool's storage pointer is set at engine start and never changes.
        let storage = unsafe { (*info).storage };
        if storage.is_null() {
            return None;
        }
        // GetAllocatorStorage: the low three bits are flags.
        Some(unsafe { ((*storage).allocator_storage & !7) as *mut c_void })
    }

    /// `size` bytes from this pool, 8-aligned, uninitialised; `None` when the pool or the allocator is unknown or
    /// the engine is out of memory.
    pub fn alloc(&self, size: usize) -> Option<AllocationResult> {
        let storage = self.storage()?;
        // SAFETY: Memory_Vault_Alloc's signature per Allocators.hpp.
        let alloc: AllocFn = unsafe { VAULT_ALLOC.func()? };
        let mut result = AllocationResult { memory: core::ptr::null_mut(), size: 0 };
        unsafe { alloc(storage, &mut result, size as u64) };
        (!result.memory.is_null()).then_some(result)
    }

    /// Gives back memory this pool handed out.
    ///
    /// # Safety
    /// `memory` must come from `alloc` of this same pool and not be used afterwards.
    pub unsafe fn free(&self, memory: *mut c_void) {
        let (Some(storage), Some(free)) = (self.storage(), unsafe { VAULT_FREE.func::<FreeFn>() }) else { return };
        let mut allocation = AllocationResult { memory, size: 0 };
        unsafe { free(storage, &mut allocation) };
    }
}
