//! RED4ext::DynArray<T> (`Containers/DynArray.hpp`): `{entries, capacity, size}`. With no buffer the entries word
//! holds the allocator (an `IAllocator` is one vtable pointer); with a buffer the allocator sits after its last
//! element, 8-aligned. Growth goes through the engine (DynArray_Realloc) so the engine can free what it gets.
use crate::assert_layout;
use crate::hashes;
use crate::memory::AllocationResult;
use crate::reloc::Reloc;
use core::ffi::c_void;
use core::marker::PhantomData;

#[repr(C)]
pub struct DynArray<T> {
    entries: *mut T,
    capacity: u32,
    size: u32,
    _marker: PhantomData<T>,
}
assert_layout!(DynArray<usize>, size = 0x10);

static REALLOC: Reloc = Reloc::new(hashes::DynArray_Realloc);
type ReallocFn = unsafe extern "C" fn(*mut c_void, u32, u32, u32, *const c_void);

impl<T> Default for DynArray<T> {
    fn default() -> Self {
        DynArray { entries: core::ptr::null_mut(), capacity: 0, size: 0, _marker: PhantomData }
    }
}

impl<T: Copy> DynArray<T> {
    pub fn len(&self) -> usize {
        self.size as usize
    }
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }
    pub fn capacity(&self) -> usize {
        self.capacity as usize
    }

    pub fn as_slice(&self) -> &[T] {
        if self.capacity == 0 || self.entries.is_null() {
            return &[];
        }
        // SAFETY: with a buffer, entries..entries+size are initialised elements.
        unsafe { core::slice::from_raw_parts(self.entries, self.size as usize) }
    }

    /// `DynArray::Clear` for trivially destructible `T`: no elements, the buffer kept.
    pub fn clear(&mut self) {
        self.size = 0;
    }

    /// Appends `value`, growing the buffer through the engine as `DynArray::Reserve` does (geometric 1.5x). Only for
    /// trivially copyable `T` (the move callback is then null, as in the SDK). Returns false when the engine's
    /// realloc cannot be resolved or fails.
    pub fn push(&mut self, value: T) -> bool {
        if self.size == self.capacity {
            let wanted = self.size + 1;
            let geometric = self.capacity + self.capacity / 2;
            let new_capacity = wanted.max(geometric);
            // SAFETY: DynArray_Realloc's signature per DynArray.hpp.
            let Some(realloc) = (unsafe { REALLOC.func::<ReallocFn>() }) else { return false };
            let align = core::mem::align_of::<T>().max(8) as u32;
            unsafe {
                realloc(
                    self as *mut Self as *mut c_void,
                    new_capacity,
                    core::mem::size_of::<T>() as u32,
                    align,
                    core::ptr::null(),
                )
            };
            if self.capacity < wanted {
                return false;
            }
        }
        // SAFETY: size < capacity now.
        unsafe { self.entries.add(self.size as usize).write(value) };
        self.size += 1;
        true
    }

    /// `~DynArray`: the buffer back to the allocator stored after it.
    pub fn free(&mut self) {
        if self.capacity == 0 {
            return;
        }
        let end = unsafe { self.entries.add(self.capacity as usize) } as usize;
        let allocator_at = (end + 7) & !7;
        // SAFETY: the engine keeps the allocator (a vtable pointer) right after a buffer it allocated.
        let mut allocator: *const *const c_void = unsafe { *(allocator_at as *const *const *const c_void) };
        type FreeFn = unsafe extern "C" fn(*mut c_void, *mut AllocationResult);
        unsafe {
            let free: FreeFn = core::mem::transmute(*allocator.add(4)); // IAllocator::Free, vtable +0x20
            let mut allocation = AllocationResult { memory: self.entries as *mut c_void, size: 0 };
            free(&mut allocator as *mut _ as *mut c_void, &mut allocation);
        }
        self.entries = allocator as *mut T;
        self.capacity = 0;
        self.size = 0;
    }
}
