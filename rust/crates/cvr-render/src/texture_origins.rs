//! NATIVE DYNAMIC TEXTURE CONSTRUCTION ORIGINS (formerly main's src/Natives/DynamicTextureOrigins.cpp, 730f5d78a):
//! an optional diagnostic, off by default. The D3D creation counters and the resource census (resource_census.rs) see
//! the lazy GPU allocation, whose stack reaches a generic VirtualWindow draw job; this observer records the earlier
//! native factory call (game 2.5 RVA 0x5EA8A4: RCX the 16-byte output Handle, object and control; RDX the 24-byte
//! descriptor -- width, height, mips, samples, format, flags, padding, backing; the output Handle returned) to name
//! the caller that actually constructs a texture.
//!
//! The original runs first, before the publication lock; the observer copies the Handle's object and control and the
//! object's ISerializable serial (+0x28) while the caller still owns it -- no AddRef, Release, native lifetime call or
//! GPU allocation. Off, the detour forwards directly. Rows go to a ring of 1024 fixed 344-byte records the tools read
//! across processes (tools/resource_census/dynamic_origins.py): writers serialize only the copy, an even sequence
//! before and after it for readers. Installed at Boot 84, after MenuSurface and PhysicalOpticsSurface (83) validated
//! the factory's own prologue; a failure sets this observer's status only.
#![allow(non_upper_case_globals, unsafe_op_in_unsafe_fn)]
use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering::*};
use std::sync::Mutex;

pub const CAPACITY: usize = 1024;
pub const STACK_DEPTH: usize = 32;

/// The factory's output: the object and its control block.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Handle {
    pub object: usize,
    pub control: usize,
}
/// The factory's descriptor.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Descriptor {
    pub width: u32,
    pub height: u32,
    pub mips: u8,
    pub samples: u8,
    pub format: u8,
    pub flags: u8,
    pub reserved: u32,
    pub backing: usize,
}
/// One construction.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Record {
    pub ordinal: u64,
    pub qpc: u64,
    pub object: u64,
    pub control: u64,
    pub object_serial: u64,
    pub caller: u64,
    pub descriptor: Descriptor,
    pub thread: u32,
    pub stack_count: u32,
    /// 1 a descriptor, 2 an output Handle, 4 the output returned, 8 the object's serial read.
    pub flags: u32,
    pub reserved: u32,
    pub stack: [u64; STACK_DEPTH],
}
const ZERO: Record = Record {
    ordinal: 0,
    qpc: 0,
    object: 0,
    control: 0,
    object_serial: 0,
    caller: 0,
    descriptor: Descriptor { width: 0, height: 0, mips: 0, samples: 0, format: 0, flags: 0, reserved: 0, backing: 0 },
    thread: 0,
    stack_count: 0,
    flags: 0,
    reserved: 0,
    stack: [0; STACK_DEPTH],
};
const _: () = assert!(
    core::mem::size_of::<Handle>() == 16
        && core::mem::size_of::<Descriptor>() == 24
        && core::mem::size_of::<Record>() == 344
        && core::mem::offset_of!(Record, descriptor) == 48
        && core::mem::offset_of!(Record, stack_count) == 76
        && core::mem::offset_of!(Record, flags) == 80
        && core::mem::offset_of!(Record, stack) == 88
);

#[no_mangle]
pub static CyberpunkVR_DynamicTextureOriginsEnable: AtomicU32 = AtomicU32::new(0);
/// 0 not installed, 1 ready, 2 unsupported build or override, 3 signature mismatch, 4 CreateHook failure, 5 EnableHook
/// failure (MinHook's failures are not told apart here: 4).
#[no_mangle]
pub static CyberpunkVR_DynamicTextureOriginsStatus: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_DynamicTextureOriginsSequence: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DynamicTextureOriginsCount: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DynamicTextureOriginsObserved: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DynamicTextureOriginsInFlight: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DynamicTextureOriginsDroppedReentrant: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DynamicTextureOriginsDroppedBusy: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DynamicTextureOriginsOverwritten: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static mut CyberpunkVR_DynamicTextureOriginsQpcFrequency: u64 = 0;
/// The ring's layout for the readers: version, capacity, row size, the descriptor's, stack count's and stack's offsets,
/// the stack depth, the flags' offset.
#[no_mangle]
pub static CyberpunkVR_DynamicTextureOriginsLayout: [u32; 8] = [1, CAPACITY as u32, 344, 48, 76, 88, STACK_DEPTH as u32, 80];
#[no_mangle]
pub static mut CyberpunkVR_DynamicTextureOriginsRows: [Record; CAPACITY] = [ZERO; CAPACITY];

/// Multi-producer publication into a ring: writers serialize only the fixed-size copy, never the native factory or
/// the stack capture; readers in another process use the sequence (odd while a row is written).
pub struct Writer<'a> {
    lock: Mutex<()>,
    rows: *mut Record,
    capacity: usize,
    sequence: &'a AtomicU64,
    count: &'a AtomicU64,
    dropped_busy: &'a AtomicU64,
    overwritten: &'a AtomicU64,
}
// SAFETY: the rows are written only under the lock.
unsafe impl Sync for Writer<'_> {}
impl<'a> Writer<'a> {
    pub const fn new(rows: *mut Record, capacity: usize, sequence: &'a AtomicU64, count: &'a AtomicU64, dropped_busy: &'a AtomicU64, overwritten: &'a AtomicU64) -> Self {
        Writer { lock: Mutex::new(()), rows, capacity, sequence, count, dropped_busy, overwritten }
    }
    /// The row published (its ordinal set) -> true; false when another writer held the ring (counted, dropped).
    pub fn publish(&self, mut record: Record) -> bool {
        let Ok(_guard) = self.lock.try_lock() else {
            self.dropped_busy.fetch_add(1, Relaxed);
            return false;
        };
        self.sequence.fetch_add(1, AcqRel);
        let ordinal = self.count.load(Relaxed) + 1;
        record.ordinal = ordinal;
        // SAFETY: the ring's slot, written under the lock.
        unsafe { self.rows.add(((ordinal - 1) % self.capacity as u64) as usize).write(record) };
        if ordinal > self.capacity as u64 {
            self.overwritten.fetch_add(1, Relaxed);
        }
        self.count.store(ordinal, Relaxed);
        self.sequence.fetch_add(1, Release);
        true
    }
}

static WRITER: Writer<'static> = Writer::new(
    (&raw mut CyberpunkVR_DynamicTextureOriginsRows).cast(),
    CAPACITY,
    &CyberpunkVR_DynamicTextureOriginsSequence,
    &CyberpunkVR_DynamicTextureOriginsCount,
    &CyberpunkVR_DynamicTextureOriginsDroppedBusy,
    &CyberpunkVR_DynamicTextureOriginsOverwritten,
);

const FACTORY: usize = 0x5E_A8A4;
const FACTORY_BYTES: &str = "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 30 48";
type CreateFn = unsafe extern "C" fn(*mut Handle, *const Descriptor) -> *mut Handle;
static mut ORIGINAL: *mut c_void = core::ptr::null_mut();

thread_local! {
    static RECORDING: Cell<bool> = const { Cell::new(false) };
}

#[inline(never)]
unsafe fn observe(output: *mut Handle, descriptor: *const Descriptor, caller: u64) -> *mut Handle {
    CyberpunkVR_DynamicTextureOriginsInFlight.fetch_add(1, Relaxed);
    struct Flight;
    impl Drop for Flight {
        fn drop(&mut self) {
            CyberpunkVR_DynamicTextureOriginsInFlight.fetch_sub(1, Relaxed);
        }
    }
    let _flight = Flight;
    // The original runs before any publication lock; a nested native creation is legitimate, the guard covers the
    // observer's work only.
    // SAFETY: the caller's descriptor or null.
    let input = unsafe { descriptor.as_ref() }.copied().unwrap_or_default();
    // SAFETY: the trampoline MinHook wrote before the detour was enabled; the caller's arguments.
    let result = unsafe {
        let original: CreateFn = core::mem::transmute(core::ptr::read_volatile(&raw const ORIGINAL));
        original(output, descriptor)
    };
    CyberpunkVR_DynamicTextureOriginsObserved.fetch_add(1, Relaxed);
    if RECORDING.with(Cell::get) {
        CyberpunkVR_DynamicTextureOriginsDroppedReentrant.fetch_add(1, Relaxed);
        return result;
    }
    RECORDING.with(|r| r.set(true));
    let mut row = ZERO;
    row.descriptor = input;
    row.caller = caller;
    // SAFETY: plain Win32 calls.
    row.thread = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
    if !descriptor.is_null() {
        row.flags |= 1;
    }
    // SAFETY: the caller's output Handle, filled by the factory.
    if let Some(h) = unsafe { output.as_ref() } {
        row.object = h.object as u64;
        row.control = h.control as u64;
        row.flags |= 2;
    }
    if result == output {
        row.flags |= 4;
    }
    if row.object != 0 {
        // ISerializable's process-unique serial; the returned Handle still the caller's, no AddRef needed
        if let Some(serial) = cvr_engine::code::read::<u64>(row.object as usize + 0x28) {
            row.object_serial = serial;
            row.flags |= 8;
        }
    }
    let mut now = 0i64;
    // SAFETY: as above.
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut now) };
    row.qpc = now as u64;
    let mut frames = [core::ptr::null_mut::<c_void>(); STACK_DEPTH];
    // SAFETY: a back trace into a local buffer.
    let n = unsafe { windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(1, &mut frames, None) } as usize;
    row.stack_count = n as u32;
    for (slot, frame) in row.stack.iter_mut().zip(&frames[..n]) {
        *slot = *frame as u64;
    }
    WRITER.publish(row);
    RECORDING.with(|r| r.set(false));
    result
}

/// The address the detour returns to (`_ReturnAddress()`): frame 0 is the detour (this inlined), frame 1 its caller.
#[inline(always)]
fn return_address() -> u64 {
    let mut frames = [core::ptr::null_mut::<c_void>(); 1];
    // SAFETY: a back trace into a local buffer.
    let n = unsafe { windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(1, &mut frames, None) };
    if n == 0 {
        0
    } else {
        frames[0] as u64
    }
}

unsafe extern "C" fn create(output: *mut Handle, descriptor: *const Descriptor) -> *mut Handle {
    if CyberpunkVR_DynamicTextureOriginsEnable.load(Relaxed) == 0 {
        let original: CreateFn = core::mem::transmute(core::ptr::read_volatile(&raw const ORIGINAL));
        return original(output, descriptor);
    }
    let caller = return_address();
    observe(output, descriptor, caller)
}

/// Installed from the hook table (Boot, 84): the supported build without a forced override, the factory's verified
/// prologue, then the detour. Its failures set its own status only.
#[no_mangle]
pub extern "C" fn cvr_hook_dynamic_texture_origins_install() -> bool {
    use cvr_engine::code::{expect, image_base, Pattern};
    let status = &CyberpunkVR_DynamicTextureOriginsStatus;
    if !cvr_engine::game_build::supported() || cvr_engine::game_build::overridden() {
        status.store(2, Relaxed);
        return false;
    }
    let base = image_base();
    if base == 0 || !Pattern::parse(FACTORY_BYTES).is_some_and(|p| expect(base + FACTORY, &p)) {
        status.store(3, Relaxed);
        return false;
    }
    let mut frequency = 0i64;
    // SAFETY: a plain Win32 call.
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut frequency) };
    // SAFETY: written before the detour is enabled, read only by the tools.
    unsafe { core::ptr::write_volatile(&raw mut CyberpunkVR_DynamicTextureOriginsQpcFrequency, frequency as u64) };
    let detour = cvr_engine::hooks::Detour {
        name: "DynamicTextureFactory",
        target: FACTORY,
        absolute: false,
        detour: create as *const c_void,
        original: &raw mut ORIGINAL,
        expect: Some(FACTORY_BYTES),
    };
    // SAFETY: the detour has the factory's signature and reads its trampoline from ORIGINAL.
    match unsafe { cvr_engine::hooks::install_chain("dynamic-texture-origins", &[detour]) } {
        Ok(()) => {
            status.store(1, Relaxed);
            true
        }
        Err(_) => {
            status.store(4, Relaxed);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    //! main's tools/dynamic_texture_origin_tests: the ring's publication, ordinals and overwrites.
    use super::*;

    #[test]
    fn ring_publishes_in_order_and_counts_overwrites() {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        static COUNT: AtomicU64 = AtomicU64::new(0);
        static BUSY: AtomicU64 = AtomicU64::new(0);
        static OLD: AtomicU64 = AtomicU64::new(0);
        let mut rows = [ZERO; 4];
        let writer = Writer::new(rows.as_mut_ptr(), 4, &SEQ, &COUNT, &BUSY, &OLD);
        for i in 0..6u64 {
            let mut r = ZERO;
            r.caller = 100 + i;
            assert!(writer.publish(r));
        }
        assert_eq!(COUNT.load(Relaxed), 6);
        assert_eq!(SEQ.load(Relaxed), 12, "even: no write in progress");
        assert_eq!(OLD.load(Relaxed), 2);
        // ordinals 5 and 6 overwrote slots 0 and 1
        assert_eq!((rows[0].ordinal, rows[0].caller), (5, 104));
        assert_eq!((rows[1].ordinal, rows[1].caller), (6, 105));
        assert_eq!((rows[2].ordinal, rows[3].ordinal), (3, 4));
    }

    #[test]
    fn a_busy_ring_drops_the_row() {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        static COUNT: AtomicU64 = AtomicU64::new(0);
        static BUSY: AtomicU64 = AtomicU64::new(0);
        static OLD: AtomicU64 = AtomicU64::new(0);
        let mut rows = [ZERO; 2];
        let writer = Writer::new(rows.as_mut_ptr(), 2, &SEQ, &COUNT, &BUSY, &OLD);
        let held = writer.lock.lock().unwrap();
        assert!(!writer.publish(ZERO));
        drop(held);
        assert_eq!((BUSY.load(Relaxed), COUNT.load(Relaxed)), (1, 0));
        assert!(writer.publish(ZERO));
    }
}
