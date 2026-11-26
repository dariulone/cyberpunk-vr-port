//! The native instance observer's engine layout reads (src/Render/NativeInstanceObserver.cpp,
//! `instance_observer::`): an instance ring's header read twice (handle at +0, mapped data at +8, capacity at +0x10,
//! stride at +0x14, tail at +0x38, head at +0x40; an 8-aligned ring whose identity fields agree across both reads, the
//! cursors of the second kept); the two ring classes (0x20000 entries of 48 bytes, 0x8000 of 64); a context's upload
//! stamp (its slot at context +0x50 indexing the registry's version table at +0x1408960, 24 bytes a slot, read twice:
//! (version + 1) << 10 | slot); and a ring handle's 0xB0-byte buffer record in the registry (+0x5C0AF8, 0xB0 a handle
//! from 1, at most 0x100000 handles, two agreeing reads).
//!
//! In the Rust build (`CVR_RUST_VRIK`) Fields, Class, Stamp, Record and Current (the thread's render context from its first
//! TLS slot) forward to `cvr_nio_*`; the unit tests below read fabricated layouts. The observer's state and its D3D12
//! checks stay the .cpp's.

use core::ffi::c_void;

/// RingFields.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RingFields {
    pub handle: u32,
    pub capacity: u32,
    pub stride: u32,
    pub mapped: u64,
    pub head: u64,
    pub tail: u64,
}
const _: () = assert!(core::mem::size_of::<RingFields>() == 40);

const RECORD_BYTES: usize = 0xB0;

/// The .cpp's Read: a guarded read of this process's memory.
pub(crate) fn read(address: u64, out: &mut [u8]) -> bool {
    use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut copied = 0usize;
    address != 0
        && address <= u64::MAX - out.len() as u64
        // SAFETY: a guarded read of this process's memory.
        && unsafe { ReadProcessMemory(GetCurrentProcess(), address as *const c_void, out.as_mut_ptr().cast(), out.len(), Some(&mut copied)) }.is_ok()
        && copied == out.len()
}
pub(crate) fn read_u32(address: u64) -> Option<u32> {
    let mut b = [0u8; 4];
    read(address, &mut b).then(|| u32::from_le_bytes(b))
}
pub(crate) fn read_u64(address: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    read(address, &mut b).then(|| u64::from_le_bytes(b))
}

fn fields_once(ring: u64) -> Option<RingFields> {
    if ring == 0 || ring & 7 != 0 || ring > u64::MAX - 0x60 {
        return None;
    }
    Some(RingFields {
        handle: read_u32(ring)?,
        mapped: read_u64(ring + 8)?,
        capacity: read_u32(ring + 0x10)?,
        stride: read_u32(ring + 0x14)?,
        tail: read_u64(ring + 0x38)?,
        head: read_u64(ring + 0x40)?,
    })
}
/// Fields: the ring's header, its identity agreeing across two reads (the second's cursors kept).
pub fn fields(ring: u64) -> Option<RingFields> {
    let first = fields_once(ring)?;
    let again = fields_once(ring)?;
    (first.handle == again.handle && first.mapped == again.mapped && first.capacity == again.capacity && first.stride == again.stride).then_some(again)
}
/// Class.
pub fn class(value: &RingFields) -> bool {
    (value.capacity == 0x20000 && value.stride == 48) || (value.capacity == 0x8000 && value.stride == 64)
}
/// Stamp: the context's upload stamp in the registry.
pub fn stamp(registry: u64, context: u64) -> Option<u64> {
    if registry == 0 || context == 0 || context > u64::MAX - 0x54 {
        return None;
    }
    let slot = read_u32(context + 0x50)?;
    let offset = 0x1408960 + (slot & 0x3FF) as u64 * 24;
    if registry > u64::MAX - offset - 8 {
        return None;
    }
    let address = registry + offset;
    let version = read_u64(address)?;
    let again = read_u64(address)?;
    if again != version || version >= u64::MAX / 1024 {
        return None;
    }
    let output = ((version + 1) << 10) | (slot & 0x3FF) as u64;
    (output != 0).then_some(output)
}
/// Record: the handle's buffer record and its address.
pub fn record(registry: u64, handle: u32, raw: &mut [u8; RECORD_BYTES]) -> Option<u64> {
    if registry == 0 || handle == 0 || handle > 0x100000 {
        return None;
    }
    let offset = 0x5C0AF8 + (handle - 1) as u64 * RECORD_BYTES as u64;
    if registry > u64::MAX - offset - RECORD_BYTES as u64 {
        return None;
    }
    let address = registry + offset;
    let mut again = [0u8; RECORD_BYTES];
    (read(address, raw) && read(address, &mut again) && *raw == again).then_some(address)
}

/// Current from the thread's TLS slot array at `slots`: the first slot initialized (+0x14), its context (+0x198) and the
/// context's state (+0x60) and list (+0x30); all zero when any is missing.
pub fn current_from(slots: u64) -> [u64; 3] {
    let found = (|| {
        let tls = read_u64(slots)?;
        if tls == 0 || tls > u64::MAX - 0x1A0 {
            return None;
        }
        let mut initialized = [0u8; 1];
        if !read(tls + 0x14, &mut initialized) || initialized[0] == 0 {
            return None;
        }
        let context = read_u64(tls + 0x198)?;
        if context == 0 || context > u64::MAX - 0x68 {
            return None;
        }
        Some([context, read_u64(context + 0x60)?, read_u64(context + 0x30)?])
    })();
    found.unwrap_or([0; 3])
}
/// Current: the thread's engine render context.
pub fn current() -> [u64; 3] {
    let slots: u64;
    // SAFETY: the TEB's thread-local storage pointer of the current thread (x64 Windows).
    unsafe { core::arch::asm!("mov {}, gs:[0x58]", out(reg) slots, options(nostack, readonly, preserves_flags)) };
    current_from(slots)
}

// ---- the C ABI

/// # Safety
/// `out` has room for three values (context, state, list).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nio_current(out: *mut u64) {
    let value = current();
    unsafe { core::ptr::copy_nonoverlapping(value.as_ptr(), out, 3) };
}
/// Fields: false leaves `out` as the .cpp's would be (the first read when it succeeded, else zero).
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nio_fields(ring: u64, out: *mut RingFields) -> bool {
    match fields(ring) {
        Some(value) => {
            unsafe { *out = value };
            true
        }
        None => {
            unsafe { *out = fields_once(ring).unwrap_or_default() };
            false
        }
    }
}
/// # Safety
/// `value` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nio_class(value: *const RingFields) -> bool {
    class(unsafe { &*value })
}
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nio_stamp(registry: u64, context: u64, out: *mut u64) -> bool {
    let value = stamp(registry, context);
    unsafe { *out = value.unwrap_or(0) };
    value.is_some()
}
/// # Safety
/// `raw` has room for 0xB0 bytes; `address` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nio_record(registry: u64, handle: u32, raw: *mut u8, address: *mut u64) -> bool {
    let raw = unsafe { &mut *(raw as *mut [u8; RECORD_BYTES]) };
    let found = record(registry, handle, raw);
    let mut at = 0;
    if registry != 0 && handle != 0 && handle <= 0x100000 {
        let offset = 0x5C0AF8 + (handle - 1) as u64 * RECORD_BYTES as u64;
        if registry <= u64::MAX - offset - RECORD_BYTES as u64 {
            at = registry + offset;
        }
    }
    unsafe { *address = at };
    found.is_some()
}

#[cfg(test)]
mod tests {
    //! Fabricated ring headers, a context's stamp and a buffer record read back.
    use super::*;

    #[test]
    fn current_context_from_slots() {
        let [context, state, list] = current();
        assert!(context != 0 || (state == 0 && list == 0));
        let mut ctx = vec![0u64; 16];
        ctx[0x60 / 8] = 0x5151;
        ctx[0x30 / 8] = 0x3030;
        let mut tls = vec![0u8; 0x1A0];
        tls[0x14] = 1;
        tls[0x198..0x1A0].copy_from_slice(&(ctx.as_ptr() as u64).to_le_bytes());
        let slots = [tls.as_ptr() as u64];
        assert_eq!(current_from(slots.as_ptr() as u64), [ctx.as_ptr() as u64, 0x5151, 0x3030]);
        tls[0x14] = 0;
        assert_eq!(current_from(slots.as_ptr() as u64), [0; 3]);
        drop((ctx, tls));
    }
    #[test]
    fn reads_ring_headers() {
        let mut ring = vec![0u64; 16];
        let base = ring.as_mut_ptr() as u64;
        unsafe {
            *(base as *mut u32) = 7;
            *((base + 8) as *mut u64) = 0xABC0;
            *((base + 0x10) as *mut u32) = 0x20000;
            *((base + 0x14) as *mut u32) = 48;
            *((base + 0x38) as *mut u64) = 5;
            *((base + 0x40) as *mut u64) = 9;
        }
        let f = fields(base).unwrap();
        assert_eq!(f, RingFields { handle: 7, capacity: 0x20000, stride: 48, mapped: 0xABC0, head: 9, tail: 5 });
        assert!(class(&f));
        assert!(!class(&RingFields { capacity: 0x8000, stride: 48, ..f }));
        assert!(class(&RingFields { capacity: 0x8000, stride: 64, ..f }));
        assert!(fields(base + 4).is_none());
        assert!(fields(0).is_none());
        drop(ring);
    }
    #[test]
    fn reads_stamps_and_records() {
        // A registry window large enough for slot 3's version and handle 2's record, addressed back from the buffer.
        let mut memory = vec![0u8; 0x1000];
        let base = memory.as_mut_ptr() as u64;
        let mut context = vec![0u8; 0x60];
        let context_at = context.as_mut_ptr() as u64;
        unsafe { *((context_at + 0x50) as *mut u32) = 0x403 };
        let registry = base.wrapping_sub(0x1408960 + 3 * 24);
        unsafe { *(base as *mut u64) = 41 };
        assert_eq!(stamp(registry, context_at), Some((42 << 10) | 3));
        assert_eq!(stamp(0, context_at), None);
        let registry = base.wrapping_sub(0x5C0AF8 + 0xB0);
        unsafe { *((base + 0x100 + 6) as *mut u8) = 0x51 };
        let mut raw = [0u8; RECORD_BYTES];
        assert_eq!(record(registry.wrapping_add(0x100), 2, &mut raw), Some(base + 0x100));
        assert_eq!(raw[6], 0x51);
        assert_eq!(record(registry, 0, &mut raw), None);
        assert_eq!(record(0, 2, &mut raw), None);
        assert_eq!(record(registry, 0x100001, &mut raw), None);
        drop((memory, context));
    }
}
