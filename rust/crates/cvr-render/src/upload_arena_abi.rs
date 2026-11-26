//! The C ABI of the native upload arena's fields and its guarded invocation stack (native::upload_arena) for
//! include/Render/NativeUploadArena.hpp: in the Rust build the header's detail functions forward here and its Scopes
//! owns a Rust stack (the runtime keeps one per thread). tools/single_pass_tests/upload_arena_test runs its cases
//! against these in its Rust variant.
use core::ffi::c_void;

use crate::native::upload_arena::{
    current_fields, read_arena, ready_fields, reset_fields, ArenaFields, Invocation, Kind, Scopes,
};

/// upload_arena::MemoryReader: (user, address, out, bytes) -> readable.
pub type MemoryReader = unsafe extern "C-unwind" fn(*mut c_void, u64, *mut c_void, usize) -> bool;

/// detail::ReadArena (`output` zeroed on a refusal).
///
/// # Safety
/// `output` is writable; `read` follows upload_arena::MemoryReader.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ua_read_arena(
    read: Option<MemoryReader>,
    user: *mut c_void,
    arena: u64,
    output: *mut ArenaFields,
) -> bool {
    unsafe {
        let mut adapter = read.map(|read| {
            move |at: u64, out: &mut [u8]| read(user, at, out.as_mut_ptr().cast(), out.len())
        });
        let fields = read_arena(
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
            arena,
        );
        *output = fields.unwrap_or_default();
        fields.is_some()
    }
}

/// detail::ResetFields (`which` 0), ReadyFields (1) or CurrentFields(before, value) (2).
///
/// # Safety
/// `value` is valid, and `before` for CurrentFields.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ua_fields(
    which: u32,
    before: *const ArenaFields,
    value: *const ArenaFields,
) -> bool {
    unsafe {
        match which {
            0 => reset_fields(&*value),
            1 => ready_fields(&*value),
            _ => current_fields(&*before, &*value),
        }
    }
}

fn kind(raw: u64) -> Option<Kind> {
    Some(match raw {
        1 => Kind::Ready,
        2 => Kind::Reset,
        3 => Kind::Destroy,
        4 => Kind::Cancel,
        5 => Kind::Query,
        _ => return None,
    })
}

/// An invocation stack; free it with cvr_ua_scopes_free.
#[no_mangle]
pub extern "C-unwind" fn cvr_ua_scopes_new() -> *mut Scopes {
    Box::into_raw(Box::default())
}

/// # Safety
/// `scopes` comes from cvr_ua_scopes_new (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ua_scopes_free(scopes: *mut Scopes) {
    if !scopes.is_null() {
        drop(unsafe { Box::from_raw(scopes) });
    }
}

/// Scopes::Enter (an unknown kind poisons the stack and yields no invocation).
///
/// # Safety
/// `scopes` is live; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ua_scopes_enter(
    scopes: *mut Scopes,
    nonce: u64,
    arena: u64,
    thread: u64,
    raw_kind: u64,
    out: *mut Invocation,
) {
    unsafe {
        *out = match kind(raw_kind) {
            Some(kind) => (*scopes).enter(nonce, arena, thread, kind),
            None => {
                (*scopes).poison();
                Invocation::default()
            }
        };
    }
}

/// Scopes' steps on an invocation: Top (`step` 0), FinishReset (1, `valid`), FinishReady (2, the reset into `reset`),
/// FinishDestroy (3), Cancel (4), Poison (5), Clear (6).
///
/// # Safety
/// `scopes` is live; `call` is valid for the invocation steps; `reset` is writable for FinishReady.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ua_scopes_step(
    scopes: *mut Scopes,
    step: u32,
    call: *const Invocation,
    thread: u64,
    valid: bool,
    reset: *mut u64,
) -> bool {
    unsafe {
        let s = &mut *scopes;
        match step {
            0 => s.top(&*call, thread),
            1 => s.finish_reset(&*call, thread, valid),
            2 => {
                let (complete, value) = s.finish_ready(&*call, thread);
                *reset = value;
                complete
            }
            3 => s.finish_destroy(&*call, thread),
            4 => {
                s.cancel(&*call, thread);
                false
            }
            5 => {
                s.poison();
                false
            }
            _ => {
                s.clear();
                false
            }
        }
    }
}
