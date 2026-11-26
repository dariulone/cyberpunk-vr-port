//! THE CAMERA CONSTANT BUFFER'S BINDING STATE (include/Render/NativeCameraBindingState.hpp): the native recording
//! state's CPU descriptor slot of the camera CBV for the vertex / pixel stage -- the root signature layout's parameter
//! for the stage (+0x455 + stage*0x1C), its table entry's offset and first index, the descriptor at that slot of the CPU
//! table -- read twice and equal (the layout is guarded by the complete native 0x336D3C fingerprint); the tracker of
//! the last observed binding per stage with a monotonic serial; the capture of a binding scope for a swap and its
//! restore -- a stage someone else rebound since is preserved, an applied one is rebound to its original handle and
//! verified. The header forwards to these in the Rust build (the tracker's and scope's records stay in C++, mirrored
//! here); tools/single_pass_tests/camera_binding_state_test runs its cases against them.
use core::ffi::c_void;

/// camera_upload::ReadState: (address, out, bytes) -> readable.
pub type ReadState = unsafe extern "C-unwind" fn(usize, *mut c_void, usize) -> bool;
/// camera_upload::RestoreBinding: (user, context, handle, stage bit) -> bound.
pub type RestoreBinding = unsafe extern "C-unwind" fn(*mut c_void, Context, u32, u32) -> bool;

/// camera_upload::Context.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Context {
    pub context: u64,
    pub state: u64,
    pub list: u64,
}
impl Context {
    pub fn valid(&self) -> bool {
        self.context != 0 && self.state != 0 && self.list != 0
    }
}
/// camera_upload::CpuSlot.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct CpuSlot {
    pub layout: u64,
    pub table: u64,
    pub address: u64,
    pub descriptor: u64,
    pub parameter: u32,
    pub index: u32,
}
/// camera_upload::Binding.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Binding {
    pub handle: u32,
    pub serial: u64,
    pub descriptor: u64,
}
/// camera_upload::BindingScope.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct BindingScope {
    pub context: Context,
    pub original: [Binding; 2],
    pub replacement: u64,
    pub requested: u32,
    pub applied: u32,
    pub restored: u32,
    pub preserved: u32,
}
/// camera_upload::BindingTracker's members.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Tracker {
    pub context: Context,
    pub bindings: [Binding; 2],
    pub serial: u64,
}
const _: () = assert!(
    core::mem::size_of::<Context>() == 24
        && core::mem::size_of::<CpuSlot>() == 40
        && core::mem::size_of::<Binding>() == 24
        && core::mem::size_of::<BindingScope>() == 96
        && core::mem::size_of::<Tracker>() == 80
);

fn read_into<T: Copy>(read: ReadState, at: u64, out: &mut T) -> bool {
    unsafe {
        read(
            at as usize,
            out as *mut T as *mut c_void,
            core::mem::size_of::<T>(),
        )
    }
}

/// ResolveCameraSlotOnce.
pub fn resolve_once(read: Option<ReadState>, state: u64, stage: u32) -> Option<CpuSlot> {
    let read = read?;
    let mut out = CpuSlot::default();
    if state == 0
        || state > u64::MAX - 0x70
        || stage > 1
        || !read_into(read, state + 0x68, &mut out.layout)
        || !read_into(read, state + 0x28, &mut out.table)
        || out.layout == 0
        || out.table == 0
        || out.layout > u64::MAX - 0x473
    {
        return None;
    }
    let mut parameter = 0u8;
    if !read_into(
        read,
        out.layout + 0x455 + stage as u64 * 0x1C,
        &mut parameter,
    ) || parameter >= 64
    {
        return None;
    }
    let entry = out.layout + 0x38 + parameter as u64 * 16;
    let (mut offset, mut first) = (0u32, 0u16);
    if !read_into(read, entry + 4, &mut offset)
        || !read_into(read, entry + 8, &mut first)
        || first > 1
        || offset > 8190
    {
        return None;
    }
    out.parameter = parameter as u32;
    out.index = offset + 1 - first as u32;
    if out.table > u64::MAX - out.index as u64 * 8 - 8 {
        return None;
    }
    out.address = out.table + out.index as u64 * 8;
    (read_into(read, out.address, &mut out.descriptor) && out.descriptor != 0).then_some(out)
}
/// ResolveCameraSlot: read twice and equal.
pub fn resolve(read: Option<ReadState>, state: u64, stage: u32) -> Option<CpuSlot> {
    let a = resolve_once(read, state, stage)?;
    let b = resolve_once(read, state, stage)?;
    (a == b).then_some(a)
}

/// RestoreScope.
///
/// # Safety
/// `bind` follows camera_upload::RestoreBinding with `user`.
pub unsafe fn restore_scope(
    read: Option<ReadState>,
    current: Context,
    scope: &mut BindingScope,
    bind: Option<RestoreBinding>,
    user: *mut c_void,
) -> bool {
    if scope.applied == 0 {
        return true;
    }
    let Some(bind) = bind.filter(|_| current.valid() && current == scope.context) else {
        return false;
    };
    let mut complete = true;
    for stage in 0..2u32 {
        let bit = 1u32 << stage;
        if scope.applied & bit == 0 {
            continue;
        }
        if resolve(read, current.state, stage)
            .is_some_and(|slot| slot.descriptor != scope.replacement)
        {
            scope.preserved |= bit;
            scope.applied &= !bit;
            continue;
        }
        if !unsafe { bind(user, current, scope.original[stage as usize].handle, bit) }
            || !resolve(read, current.state, stage)
                .is_some_and(|slot| slot.descriptor == scope.original[stage as usize].descriptor)
        {
            complete = false;
            continue;
        }
        scope.restored |= bit;
        scope.applied &= !bit;
    }
    complete
}

impl Tracker {
    pub fn clear(&mut self) {
        self.context = Context::default();
        self.bindings = [Binding::default(); 2];
    }
    pub fn observe(&mut self, read: Option<ReadState>, context: Context, handle: u32, stages: u32) {
        if stages & 3 == 0 {
            return;
        }
        if !context.valid() {
            self.clear();
            return;
        }
        if self.context != context {
            self.clear();
            self.context = context;
        }
        for stage in 0..2u32 {
            if stages & (1 << stage) == 0 {
                continue;
            }
            self.serial += 1;
            let binding = Binding {
                serial: self.serial,
                ..Binding::default()
            };
            self.bindings[stage as usize] = binding;
            if handle != 0 {
                if let Some(slot) = resolve(read, context.state, stage) {
                    self.bindings[stage as usize].handle = handle;
                    self.bindings[stage as usize].descriptor = slot.descriptor;
                }
            }
        }
    }
    pub fn capture(
        &self,
        read: Option<ReadState>,
        context: Context,
        stages: u32,
        scope: &mut BindingScope,
    ) -> bool {
        *scope = BindingScope::default();
        if !context.valid() || context != self.context || stages == 0 || stages > 3 {
            return false;
        }
        for stage in 0..2u32 {
            if stages & (1 << stage) == 0 {
                continue;
            }
            let saved = &self.bindings[stage as usize];
            if saved.handle == 0
                || saved.serial == 0
                || !resolve(read, context.state, stage)
                    .is_some_and(|slot| slot.descriptor == saved.descriptor)
            {
                return false;
            }
        }
        scope.context = context;
        scope.original = self.bindings;
        scope.requested = stages;
        true
    }
    pub fn changed(&self, scope: &BindingScope, stage: u32) -> bool {
        stage > 1
            || self.context != scope.context
            || self.bindings[stage as usize].serial != scope.original[stage as usize].serial
    }
}

/// ResolveCameraSlotOnce (`twice` false) or ResolveCameraSlot (true); `out` zeroed on a refusal (Once) or untouched
/// (twice), as by the C++ rules.
///
/// # Safety
/// `out` is writable; `read` follows camera_upload::ReadState.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cb_resolve(
    read: Option<ReadState>,
    state: u64,
    stage: u32,
    out: *mut CpuSlot,
    twice: bool,
) -> bool {
    unsafe {
        if twice {
            match resolve(read, state, stage) {
                Some(slot) => {
                    *out = slot;
                    true
                }
                None => false,
            }
        } else {
            let slot = resolve_once(read, state, stage);
            *out = slot.unwrap_or_default();
            slot.is_some()
        }
    }
}

/// RestoreScope.
///
/// # Safety
/// `current` and `scope` are valid; `bind` follows camera_upload::RestoreBinding with `user`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cb_restore(
    read: Option<ReadState>,
    current: *const Context,
    scope: *mut BindingScope,
    bind: Option<RestoreBinding>,
    user: *mut c_void,
) -> bool {
    unsafe { restore_scope(read, *current, &mut *scope, bind, user) }
}

/// BindingTracker::Clear (`step` 0), Observe (1), Capture (2, into `scope`), Changed (3, `value` the stage).
///
/// # Safety
/// `tracker` is valid; `context` for Observe / Capture; `scope` for Capture / Changed; `read` follows
/// camera_upload::ReadState.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cb_tracker(
    tracker: *mut Tracker,
    step: u32,
    read: Option<ReadState>,
    context: *const Context,
    handle: u32,
    value: u32,
    scope: *mut BindingScope,
) -> bool {
    unsafe {
        let t = &mut *tracker;
        match step {
            0 => {
                t.clear();
                false
            }
            1 => {
                t.observe(read, *context, handle, value);
                false
            }
            2 => t.capture(read, *context, value, &mut *scope),
            _ => t.changed(&*scope, value),
        }
    }
}
