//! RED4ext::Callback<void (*)()> (`Callback.hpp`): a 64-byte inline target and a handler table. The engine copies,
//! moves and destroys it through the handler, so an unbound function is all a plugin needs.
use crate::assert_layout;
use core::ffi::c_void;

#[repr(C)]
pub struct CallbackHandler {
    pub invoke: unsafe extern "C" fn(target: *const c_void),
    pub copy: unsafe extern "C" fn(dst: *mut c_void, src: *mut c_void),
    pub moved: unsafe extern "C" fn(dst: *mut c_void, src: *mut c_void),
    pub destruct: unsafe extern "C" fn(target: *mut c_void),
}
assert_layout!(CallbackHandler, size = 0x20);

/// `Callback<void (*)(), 64>`. Passed by value in C++, which the Win64 ABI turns into a pointer to a copy the callee
/// destroys: hand the engine `&mut callback` and do not use the value again.
#[repr(C)]
pub struct VoidCallback {
    pub buffer: [u8; 64],
    pub handler: *const CallbackHandler,
}
assert_layout!(VoidCallback, size = 72, handler = 64);

unsafe extern "C" fn invoke(target: *const c_void) {
    // SAFETY: the buffer's first word is the unbound function `new` stored.
    let f = unsafe { *(target as *const Option<extern "C" fn()>) };
    if let Some(f) = f {
        f();
    }
}
unsafe extern "C" fn copy(dst: *mut c_void, src: *mut c_void) {
    unsafe { *(dst as *mut usize) = *(src as *const usize) };
}
unsafe extern "C" fn moved(dst: *mut c_void, src: *mut c_void) {
    unsafe {
        *(dst as *mut usize) = *(src as *const usize);
        *(src as *mut usize) = 0;
    }
}
unsafe extern "C" fn destruct(target: *mut c_void) {
    unsafe { *(target as *mut usize) = 0 };
}

static UNBOUND: CallbackHandler = CallbackHandler { invoke, copy, moved, destruct };

impl VoidCallback {
    /// A callback calling `f`, which must not unwind (wrap its body in a panic guard).
    pub fn new(f: extern "C" fn()) -> Self {
        let mut buffer = [0u8; 64];
        buffer[..8].copy_from_slice(&(f as usize).to_le_bytes());
        VoidCallback { buffer, handler: &UNBOUND }
    }
}
