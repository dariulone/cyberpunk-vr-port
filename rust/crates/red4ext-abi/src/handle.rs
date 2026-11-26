//! Script handles (`Handle.hpp`, `Memory/SharedPtr.hpp`): a strong reference to an ISerializable -- the instance and its
//! reference counts. A handle read from a native's arguments holds one strong reference of its own, released on drop
//! exactly as the SDK's ~Handle does: the strong count goes down; at zero the weak count is dropped through the engine's
//! own function (which frees the counts at zero), and an instance that agrees (`CanBeDestructed`) is destroyed through
//! its allocator.
use crate::assert_layout;
use crate::hashes;
use crate::reloc::Reloc;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};

/// RefCnt.
#[repr(C)]
pub struct RefCnt {
    pub strong: AtomicU32,
    pub weak: AtomicU32,
}
assert_layout!(RefCnt, size = 8);

/// Handle<T> (any ISerializable): `instance` at +0, `ref_count` at +8.
#[repr(C)]
pub struct Handle {
    pub instance: *mut c_void,
    pub ref_count: *mut RefCnt,
}
assert_layout!(Handle, size = 0x10, ref_count = 8);

static DEC_WEAK_REF: Reloc = Reloc::new(hashes::Handle_DecWeakRef);
static HANDLE_CTOR: Reloc = Reloc::new(hashes::Handle_ctor);

/// ISerializable's virtual slots used here.
const SLOT_GET_ALLOCATOR: usize = 0x10 / 8;
const SLOT_DESTRUCTOR: usize = 0x18 / 8;
const SLOT_CAN_BE_DESTRUCTED: usize = 0xD0 / 8;
/// IAllocator::Free(AllocationResult&).
const ALLOCATOR_SLOT_FREE: usize = 0x20 / 8;

#[repr(C)]
struct Allocation {
    memory: *mut c_void,
    size: u64,
}

/// The function in `object`'s vtable slot.
///
/// # Safety
/// `object` is a live polymorphic engine object with at least `slot + 1` virtual functions.
unsafe fn virtual_fn(object: *mut c_void, slot: usize) -> *const c_void {
    // SAFETY: as the caller vouches.
    unsafe { *(*(object as *const *const *const c_void)).add(slot) }
}

impl Handle {
    pub const fn null() -> Handle {
        Handle {
            instance: core::ptr::null_mut(),
            ref_count: core::ptr::null_mut(),
        }
    }
    pub fn is_null(&self) -> bool {
        self.instance.is_null()
    }

    /// Memory::Delete(instance): its own allocator, the (non-deleting) virtual destructor, then the allocator's Free.
    ///
    /// # Safety
    /// `instance` is a live ISerializable nobody references any more.
    unsafe fn destroy(instance: *mut c_void) {
        type GetAllocator = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
        type Destructor = unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void;
        type Free = unsafe extern "C" fn(*mut c_void, *mut Allocation);
        // SAFETY: ISerializable's slots, as the SDK declares them; the destructor runs with no delete flag, as an
        // explicit `aInstance->~T()` does.
        unsafe {
            let allocator = core::mem::transmute::<*const c_void, GetAllocator>(virtual_fn(instance, SLOT_GET_ALLOCATOR))(instance);
            core::mem::transmute::<*const c_void, Destructor>(virtual_fn(instance, SLOT_DESTRUCTOR))(instance, 0);
            if !allocator.is_null() {
                let mut allocation = Allocation { memory: instance, size: 0 };
                core::mem::transmute::<*const c_void, Free>(virtual_fn(allocator, ALLOCATOR_SLOT_FREE))(allocator, &mut allocation);
            }
        }
    }
}

impl Handle {
    /// `Handle<T>(T* instance)`: the engine's constructor -- the instance's own reference counts (its self weak handle)
    /// reused or made -- holding one strong reference. A null instance gives a null handle.
    ///
    /// # Safety
    /// `instance` is a live ISerializable (or null).
    pub unsafe fn from_instance(instance: *mut c_void) -> Handle {
        let mut h = Handle::null();
        if instance.is_null() {
            return h;
        }
        type Ctor = unsafe extern "C" fn(*mut Handle, *mut c_void) -> *mut Handle;
        // SAFETY: Handle_ctor's signature per Handle.hpp; it fills the pair in place.
        if let Some(ctor) = unsafe { HANDLE_CTOR.func::<Ctor>() } {
            unsafe { ctor(&mut h, instance) };
        }
        h
    }
}

impl Default for Handle {
    fn default() -> Handle {
        Handle::null()
    }
}

/// A new strong reference to the same instance (the SDK's copy constructor: the strong count goes up).
impl Clone for Handle {
    fn clone(&self) -> Handle {
        // SAFETY: a handle's counts are live while it holds its strong reference.
        if let Some(counts) = unsafe { self.ref_count.as_ref() } {
            counts.strong.fetch_add(1, Ordering::AcqRel);
        }
        Handle {
            instance: self.instance,
            ref_count: self.ref_count,
        }
    }
}

/// WeakHandle<T>: the same pair, holding a weak reference -- the counts stay allocated, the instance may be gone.
/// Copies raise the weak count; drop gives it back through the engine's own function (which frees the counts at zero).
#[repr(C)]
pub struct WeakHandle {
    pub instance: *mut c_void,
    pub ref_count: *mut RefCnt,
}
assert_layout!(WeakHandle, size = 0x10, ref_count = 8);

impl WeakHandle {
    pub const fn null() -> WeakHandle {
        WeakHandle {
            instance: core::ptr::null_mut(),
            ref_count: core::ptr::null_mut(),
        }
    }
    /// A weak reference to what `handle` holds.
    pub fn from_handle(handle: &Handle) -> WeakHandle {
        // SAFETY: a strong handle's counts are live.
        if let Some(counts) = unsafe { handle.ref_count.as_ref() } {
            counts.weak.fetch_add(1, Ordering::AcqRel);
        }
        WeakHandle {
            instance: handle.instance,
            ref_count: handle.ref_count,
        }
    }
    /// A strong reference while the instance is alive (RefCnt::IncRefIfNotZero), else a null handle.
    pub fn lock(&self) -> Handle {
        // SAFETY: a weak reference keeps its counts allocated.
        let Some(counts) = (unsafe { self.ref_count.as_ref() }) else { return Handle::null() };
        let mut uses = counts.strong.load(Ordering::Acquire);
        while uses != 0 {
            match counts.strong.compare_exchange(uses, uses + 1, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => {
                    return Handle {
                        instance: self.instance,
                        ref_count: self.ref_count,
                    }
                }
                Err(now) => uses = now,
            }
        }
        Handle::null()
    }
    /// The instance while it is alive, without taking a strong reference: for a `WeakPtr<T>` to an object that is not an
    /// ISerializable (a strong reference released here could not destroy it the way its own SharedPtr would). The
    /// caller must not outlive the strong owner's frame.
    pub fn alive_instance(&self) -> *mut c_void {
        // SAFETY: a weak reference keeps its counts allocated.
        match unsafe { self.ref_count.as_ref() } {
            Some(counts) if counts.strong.load(Ordering::Acquire) != 0 => self.instance,
            _ => core::ptr::null_mut(),
        }
    }
    /// `*destination = source` (the SDK's copy assignment: the new reference first, then the old one released).
    ///
    /// # Safety
    /// `destination` is a valid WeakHandle in memory (null or holding a weak reference).
    pub unsafe fn assign(destination: *mut WeakHandle, source: &WeakHandle) {
        // SAFETY: as the caller vouches; the old value is read out and dropped, the new one written in its place.
        unsafe { drop(core::ptr::replace(destination, source.clone())) }
    }
}

impl Default for WeakHandle {
    fn default() -> WeakHandle {
        WeakHandle::null()
    }
}

impl Clone for WeakHandle {
    fn clone(&self) -> WeakHandle {
        // SAFETY: a weak reference keeps its counts allocated.
        if let Some(counts) = unsafe { self.ref_count.as_ref() } {
            counts.weak.fetch_add(1, Ordering::AcqRel);
        }
        WeakHandle {
            instance: self.instance,
            ref_count: self.ref_count,
        }
    }
}

impl Drop for WeakHandle {
    fn drop(&mut self) {
        if self.ref_count.is_null() {
            return;
        }
        if let Some(dec_weak) = DEC_WEAK_REF.get() {
            // SAFETY: the engine's DecWeakRef takes the pair (SharedPtrBase) and frees the counts at zero.
            unsafe { core::mem::transmute::<usize, unsafe extern "C" fn(*mut WeakHandle)>(dec_weak)(self) };
        }
        self.instance = core::ptr::null_mut();
        self.ref_count = core::ptr::null_mut();
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the counts and the instance this handle holds a strong reference to.
        unsafe {
            let Some(counts) = self.ref_count.as_ref() else { return };
            if counts.strong.fetch_sub(1, Ordering::AcqRel) != 1 {
                return;
            }
            if let Some(dec_weak) = DEC_WEAK_REF.get() {
                core::mem::transmute::<usize, unsafe extern "C" fn(*mut Handle)>(dec_weak)(self);
            }
            type CanBeDestructed = unsafe extern "C" fn(*mut c_void) -> bool;
            if !self.instance.is_null()
                && core::mem::transmute::<*const c_void, CanBeDestructed>(virtual_fn(self.instance, SLOT_CAN_BE_DESTRUCTED))(self.instance)
            {
                Handle::destroy(self.instance);
            }
        }
        self.instance = core::ptr::null_mut();
        self.ref_count = core::ptr::null_mut();
    }
}
