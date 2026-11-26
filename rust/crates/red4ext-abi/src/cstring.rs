//! RED4ext::CString (`CString.hpp`): 0x14 bytes inline, or a pointer to engine-allocated text once the length has
//! bit 30 set. Built and destroyed by the engine's own constructor and destructor.
use crate::assert_layout;
use crate::hashes;
use crate::reloc::Reloc;
use core::ffi::{c_char, c_void, CStr};

#[repr(C)]
pub struct CString {
    text: [u8; 0x14],
    length: u32,
    allocator: *mut c_void,
}
assert_layout!(CString, size = 0x20, length = 0x14, allocator = 0x18);

static CTOR_STR: Reloc = Reloc::new(hashes::CString_ctor_str);
static DTOR: Reloc = Reloc::new(hashes::CString_dtor);

impl CString {
    /// An empty, uninitialised-by-the-engine string: the state `CString()` leaves.
    pub const fn empty() -> Self {
        CString { text: [0; 0x14], length: 0, allocator: core::ptr::null_mut() }
    }

    /// `CString(const char*)`. `None` when the engine's constructor cannot be resolved.
    pub fn new(text: &CStr) -> Option<Self> {
        type Ctor = unsafe extern "C" fn(*mut CString, *const c_char) -> *mut CString;
        // SAFETY: CString_ctor_str's signature per CString-inl.hpp.
        let ctor = unsafe { CTOR_STR.func::<Ctor>()? };
        let mut s = CString::empty();
        unsafe { ctor(&mut s, text.as_ptr()) };
        Some(s)
    }

    /// `*out = CString(text)` (the move assignment: the old value is overwritten, not destroyed -- a result slot
    /// the VM hands a native is a fresh empty string). False when the engine's constructor cannot be resolved or the
    /// text has a NUL.
    ///
    /// # Safety
    /// `out` must point at a CString slot.
    pub unsafe fn assign(out: *mut CString, text: &str) -> bool {
        let Ok(c) = std::ffi::CString::new(text) else { return false };
        match CString::new(&c) {
            Some(s) => {
                unsafe { core::ptr::write(out, s) };
                true
            }
            None => false,
        }
    }

    fn is_inline(&self) -> bool {
        self.length < 0x4000_0000
    }

    /// `c_str()`: the inline text or the heap text's pointer (null for a heap string without one).
    pub fn c_str(&self) -> *const c_char {
        if self.is_inline() {
            self.text.as_ptr().cast()
        } else {
            // SAFETY: a heap string keeps its pointer in the first word.
            unsafe { *(self.text.as_ptr() as *const *const c_char) }
        }
    }

    /// The text, up to its NUL. Empty for text that is not UTF-8.
    pub fn as_str(&self) -> &str {
        let p = self.c_str();
        if p.is_null() {
            return "";
        }
        // SAFETY: the engine NUL-terminates both forms.
        unsafe { CStr::from_ptr(p) }.to_str().unwrap_or("")
    }
}

impl Drop for CString {
    fn drop(&mut self) {
        type Dtor = unsafe extern "C" fn(*mut CString) -> *mut CString;
        if self.length == 0 && self.allocator.is_null() {
            return;
        }
        // SAFETY: CString_dtor's signature; only reached for a string the engine built.
        if let Some(dtor) = unsafe { DTOR.func::<Dtor>() } {
            unsafe { dtor(self) };
        }
    }
}
