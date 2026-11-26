//! The owner of one native CPU visibility collector (formerly src/Render/NativeCpuCollector.cpp): the engine's own
//! constructor builds it, its own destructor frees it, and its initialiser receives the port's copy of the camera
//! and view parameters. Scene and view data are borrowed only for the caller's current native preparation scope.
//! Create starts no jobs.
use core::ffi::c_void;

/// The native wrapper's layout: the collector storage and its metadata inside the owner (game 2.5).
const STORAGE_AT: usize = 0xC0;
const METADATA_AT: usize = 0xE05C0;

/// CollectorInput (include/Render/NativeCpuCollector.hpp): view data, scene, mode, padding, the camera input
/// (Render/StereoCameraInput.hpp, 0x3E0 bytes) and 0x330 bytes of view flags. Copied whole.
pub const INPUT_BYTES: usize = 0x730;
const VIEW_DATA_AT: usize = 0;
const SCENE_AT: usize = 8;
const MODE_AT: usize = 16;
const CAMERA_AT: usize = 0x20;
const FLAGS_AT: usize = 0x400;

#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub struct Input(pub [u8; INPUT_BYTES]);
impl Input {
    fn address(&self, at: usize) -> *mut c_void {
        usize::from_le_bytes(self.0[at..at + 8].try_into().unwrap_or_default()) as *mut c_void
    }
}

pub type Initialize = unsafe extern "C-unwind" fn(*mut c_void, *mut c_void, u8, *const c_void, *mut c_void, *mut c_void, *const c_void) -> usize;
/// CollectorApi: the engine's constructor, destructor (release flag) and initialiser.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Api {
    pub create: Option<unsafe extern "C-unwind" fn(*mut *mut c_void) -> *mut c_void>,
    pub destroy: Option<unsafe extern "C-unwind" fn(*mut c_void, u32) -> *mut c_void>,
    pub initialize: Option<Initialize>,
}

pub struct Collector {
    api: Api,
    owner: *mut c_void,
    input: Box<Input>,
}
impl Collector {
    pub fn new(api: Api) -> Self {
        Collector {
            api,
            owner: core::ptr::null_mut(),
            input: Box::new(Input([0; INPUT_BYTES])),
        }
    }
    /// Reset: the engine's destructor frees the collector, if one was created.
    pub fn reset(&mut self) {
        if !self.owner.is_null() {
            let owner = core::mem::replace(&mut self.owner, core::ptr::null_mut());
            if let Some(destroy) = self.api.destroy {
                // SAFETY: the engine's destructor for the collector its constructor made.
                unsafe { destroy(owner, 1) };
            }
        }
    }
    pub fn storage(&self) -> *mut c_void {
        if self.owner.is_null() {
            self.owner
        } else {
            self.owner.cast::<u8>().wrapping_add(STORAGE_AT).cast()
        }
    }
    pub fn metadata(&self) -> *mut c_void {
        if self.owner.is_null() {
            self.owner
        } else {
            self.owner.cast::<u8>().wrapping_add(METADATA_AT).cast()
        }
    }
    /// Inputs: the copy the collector was initialised with (only while it exists).
    pub fn inputs(&self) -> Option<&Input> {
        (!self.owner.is_null()).then_some(&*self.input)
    }
    /// Create: one collector at a time, with the whole API and both borrowed pointers.
    pub fn create(&mut self, input: &Input) -> bool {
        let (Some(create), Some(_), Some(initialize)) = (self.api.create, self.api.destroy, self.api.initialize) else {
            return false;
        };
        if !self.owner.is_null() || input.address(VIEW_DATA_AT).is_null() || input.address(SCENE_AT).is_null() {
            return false;
        }
        *self.input = *input;
        let mut allocated = core::ptr::null_mut();
        // SAFETY: the engine's constructor writes the new collector's address.
        unsafe { create(&mut allocated) };
        if allocated.is_null() {
            return false;
        }
        self.owner = allocated;
        let copy = &*self.input;
        // SAFETY: the engine's initialiser on its own collector, with the port's copies of camera and flags.
        unsafe {
            initialize(
                self.storage(),
                copy.address(VIEW_DATA_AT),
                copy.0[MODE_AT],
                copy.0.as_ptr().add(CAMERA_AT).cast(),
                copy.address(SCENE_AT),
                self.metadata(),
                copy.0.as_ptr().add(FLAGS_AT).cast(),
            )
        };
        true
    }
}
impl Drop for Collector {
    fn drop(&mut self) {
        self.reset();
    }
}

// The header's wrapper (NativeCpuCollector.hpp in the Rust build).

/// # Safety
/// `api` points to the engine's collector API.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_cpu_collector_new(api: *const Api) -> *mut Collector {
    // SAFETY: the caller's API table, copied.
    let api = unsafe { api.as_ref() }.copied().unwrap_or(Api {
        create: None,
        destroy: None,
        initialize: None,
    });
    Box::into_raw(Box::new(Collector::new(api)))
}
/// # Safety
/// `c` came from cvr_cpu_collector_new (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn cvr_cpu_collector_free(c: *mut Collector) {
    if !c.is_null() {
        // SAFETY: the wrapper's own collector, dropped once.
        drop(unsafe { Box::from_raw(c) });
    }
}
/// # Safety
/// `c` is a live collector; `input` points to INPUT_BYTES bytes of a CollectorInput.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn cvr_cpu_collector_create(c: *mut Collector, input: *const Input) -> bool {
    // SAFETY: the wrapper's collector and the caller's input.
    match (unsafe { c.as_mut() }, unsafe { input.as_ref() }) {
        (Some(c), Some(input)) => c.create(input),
        _ => false,
    }
}
/// # Safety
/// `c` is a live collector.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn cvr_cpu_collector_reset(c: *mut Collector) {
    // SAFETY: the wrapper's collector.
    if let Some(c) = unsafe { c.as_mut() } {
        c.reset();
    }
}
/// # Safety
/// `c` is a live collector.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_cpu_collector_storage(c: *const Collector) -> *mut c_void {
    // SAFETY: the wrapper's collector.
    unsafe { c.as_ref() }.map_or(core::ptr::null_mut(), Collector::storage)
}
/// # Safety
/// `c` is a live collector.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_cpu_collector_metadata(c: *const Collector) -> *mut c_void {
    // SAFETY: the wrapper's collector.
    unsafe { c.as_ref() }.map_or(core::ptr::null_mut(), Collector::metadata)
}
/// # Safety
/// `c` is a live collector.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_cpu_collector_inputs(c: *const Collector) -> *const Input {
    // SAFETY: the wrapper's collector.
    unsafe { c.as_ref() }
        .and_then(Collector::inputs)
        .map_or(core::ptr::null(), |i| i as *const Input)
}
