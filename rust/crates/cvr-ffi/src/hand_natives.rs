//! The hand natives over the shared hands memory, the calibration the in-headset overlay publishes there, the FPP
//! component dump and the chunk-debug setters (formerly src/Natives/Natives.cpp). The helpers the C++ natives still
//! share (ContainsInsensitive, EqualsInsensitive, ClassIsA, IsLikelyFppArmComponent, IsReadable, EnsureSharedMemory,
//! PollVRCalibFromShared) keep their C++ names in src/Natives/Natives.cpp and forward here.
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicI32, AtomicPtr, Ordering};
use std::ffi::CStr;
use std::fmt::Write as _;

use red4ext_abi::cname::CName;
use red4ext_abi::fnv::fnv1a64;
use red4ext_abi::handle::{Handle, WeakHandle};
use red4ext_abi::rtti::{object_class, Class};
use red4ext_abi::stack::StackFrame;
use std::sync::Mutex;

use crate::native_args::{args, borrowed, handles};

#[allow(non_upper_case_globals)]
extern "C" {
    // Anim/VrikState.hpp (C linkage) and Natives/NativeState.hpp.
    static mut g_pSharedHands: *mut f32;
    static g_handsStableValid: bool;
    static mut g_VRShoulderRX: f32;
    static mut g_VRShoulderRY: f32;
    static mut g_VRShoulderRZ: f32;
    static mut g_VRShoulderLX: f32;
    static mut g_VRShoulderLY: f32;
    static mut g_VRShoulderLZ: f32;
    static mut g_VRUserArmLenR: f32;
    static mut g_VRUserArmLenL: f32;
    static mut g_VRUserEyeHeight: f32;
    static mut g_VRScaleR: f32;
    static mut g_VRScaleL: f32;
    static mut g_VROffRZ: f32;
    static mut g_VROffLZ: f32;
    static mut g_VRElbowSwingR: f32;
    static mut g_VRElbowSwingL: f32;
    static mut g_VRElbowPoleR: f32;
    static mut g_VRElbowPoleL: f32;
    static mut g_VRWristR_I: f32;
    static mut g_VRWristR_J: f32;
    static mut g_VRWristR_K: f32;
    static mut g_VRWristR_R: f32;
    static mut g_VRWristL_I: f32;
    static mut g_VRWristL_J: f32;
    static mut g_VRWristL_K: f32;
    static mut g_VRWristL_R: f32;
    static g_VRCamI: f32;
    static g_VRCamJ: f32;
    static g_VRCamK: f32;
    static g_VRCamR: f32;
    static mut g_chunkDebugEnabled: bool;
    static mut g_chunkDebugComponentIndex: i32;
    static mut g_chunkDebugHand: i32;
    static mut g_chunkDebugBitSlots: [i32; 4];
    fn SharedPose(i: i32) -> f32;
    fn RefreshHandsSnapshot();
    fn WriteVRDiagCore(cam_x: f32, cam_y: f32, cam_z: f32, qi: f32, qj: f32, qk: f32, qr: f32);
    // src/Natives/Natives.cpp: GetPlayer(GameInstance).
}

// The globals are the C++ half's, written by one thread at a time as there; volatile reads and writes keep them so.
macro_rules! get {
    ($g:ident) => {{
        #[allow(unused_unsafe)]
        let value = unsafe { core::ptr::read_volatile(core::ptr::addr_of!($g)) };
        value
    }};
}
macro_rules! set {
    ($g:ident, $v:expr) => {{
        let value = $v;
        #[allow(unused_unsafe)]
        unsafe {
            core::ptr::write_volatile(core::ptr::addr_of_mut!($g), value)
        }
    }};
}

// ---- the shared hands memory

const SHARED_NAME: &CStr = c"CyberpunkVR_Hands_Shared";
const SHARED_BYTES: u32 = 1024;
static MAPPING: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

/// EnsureSharedMemory: 1024 bytes = 256 floats. [0..127] is the legacy crowded region (hands seqlock at [127], HMD pos
/// [124..126], entity [96..99], view offsets...); [128..131] carries the clean camera-local pair (see SetVRPlayerYaw).
#[no_mangle]
pub extern "C" fn cvr_ensure_shared_memory() {
    use windows_sys::Win32::System::Memory::{
        CreateFileMappingA, MapViewOfFile, FILE_MAP_ALL_ACCESS, PAGE_READWRITE,
    };
    if !get!(g_pSharedHands).is_null() {
        return;
    }
    // SAFETY: a named page-file mapping and a view of it, as the C++ made them.
    unsafe {
        let mapping = CreateFileMappingA(
            windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE,
            core::ptr::null(),
            PAGE_READWRITE,
            0,
            SHARED_BYTES,
            SHARED_NAME.as_ptr().cast(),
        );
        if mapping.is_null() {
            return;
        }
        MAPPING.store(mapping, Ordering::Release);
        let view = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, SHARED_BYTES as usize);
        set!(g_pSharedHands, view.Value as *mut f32);
    }
}

fn slot(i: usize) -> f32 {
    // SAFETY: a slot of the 256-float shared block, mapped.
    unsafe { core::ptr::read_volatile(get!(g_pSharedHands).add(i)) }
}

/// PollVRCalibFromShared: the IK calibration the in-headset overlay published into shared memory (slots [33..48])
/// applied to the live globals. When [33] (valid) is 0 the plugin keeps its baked defaults so the CET sliders still
/// work standalone. Also polls the one-shot diag-request counter ([48]) and dumps a diag file when it changes, so the
/// overlay's "Log VR Diag" button works in-headset.
#[no_mangle]
pub extern "C" fn cvr_poll_vr_calib_from_shared() {
    if get!(g_pSharedHands).is_null() {
        return;
    }
    // Shoulder anatomical offsets from auto-calibration (slots 70..75, validity in 76).
    if slot(76) != 0.0 {
        set!(g_VRShoulderRX, slot(70));
        set!(g_VRShoulderRY, slot(71));
        set!(g_VRShoulderRZ, slot(72));
        set!(g_VRShoulderLX, slot(73));
        set!(g_VRShoulderLY, slot(74));
        set!(g_VRShoulderLZ, slot(75));
    }
    // T-pose measured anatomy: [77]/[78] real arm length R/L (m), [79] HMD eye height (m), [80] valid. Drives the
    // gizmo-path arm-bone scaling (a straight real arm -> straight avatar arm).
    if slot(80) != 0.0 {
        set!(g_VRUserArmLenR, slot(77));
        set!(g_VRUserArmLenL, slot(78));
        set!(g_VRUserEyeHeight, slot(79));
    }
    if slot(33) != 0.0 {
        // scaleR, scaleL, heightR, heightL, swingR, swingL, poleR, poleL, wRpyr(3), wLpyr(3)
        let c = |i: usize| slot(34 + i);
        set!(g_VRScaleR, c(0));
        set!(g_VRScaleL, c(1));
        set!(g_VROffRZ, c(2));
        set!(g_VROffLZ, c(3));
        set!(g_VRElbowSwingR, c(4));
        set!(g_VRElbowSwingL, c(5));
        set!(g_VRElbowPoleR, c(6));
        set!(g_VRElbowPoleL, c(7));
        // Wrist corrections: euler(pitch,yaw,roll) deg -> quat (same XYZ convention as SetVRHandOffset).
        let d2r = 0.017_453_292_52_f32 * 0.5;
        for side in 0..2 {
            let (p, y, r) = (c(8 + side * 3), c(9 + side * 3), c(10 + side * 3));
            let (cp, sp) = ((p * d2r).cos(), (p * d2r).sin());
            let (cy, sy) = ((y * d2r).cos(), (y * d2r).sin());
            let (cr, sr) = ((r * d2r).cos(), (r * d2r).sin());
            let qi = sp * cy * cr + cp * sy * sr;
            let qj = cp * sy * cr - sp * cy * sr;
            let qk = cp * cy * sr + sp * sy * cr;
            let qr = cp * cy * cr - sp * sy * sr;
            if side == 0 {
                set!(g_VRWristR_I, qi);
                set!(g_VRWristR_J, qj);
                set!(g_VRWristR_K, qk);
                set!(g_VRWristR_R, qr);
            } else {
                set!(g_VRWristL_I, qi);
                set!(g_VRWristL_J, qj);
                set!(g_VRWristL_K, qk);
                set!(g_VRWristL_R, qr);
            }
        }
    }
    // One-shot diag request (monotonic counter from the overlay).
    static LAST_DIAG_REQUEST: AtomicI32 = AtomicI32::new(0);
    let request = slot(48) as i32;
    if LAST_DIAG_REQUEST.swap(request, Ordering::Relaxed) != request {
        // Camera position isn't published; the decisive diag lines don't need it.
        // SAFETY: the calibration family's diag dump.
        unsafe {
            WriteVRDiagCore(
                0.0,
                0.0,
                0.0,
                get!(g_VRCamI),
                get!(g_VRCamJ),
                get!(g_VRCamK),
                get!(g_VRCamR),
            )
        };
    }
}

// ---- the helpers the C++ natives share

fn lower(text: &[u8]) -> Vec<u8> {
    text.iter().map(u8::to_ascii_lowercase).collect()
}
/// # Safety
/// Both null or NUL-terminated.
unsafe fn bytes<'a>(text: *const c_char) -> Option<&'a [u8]> {
    // SAFETY: the caller's.
    (!text.is_null()).then(|| unsafe { CStr::from_ptr(text) }.to_bytes())
}

fn contains_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    let (h, n) = (lower(haystack), lower(needle));
    n.is_empty() || h.windows(n.len()).any(|w| w == n.as_slice())
}

/// ContainsInsensitive (ASCII case, as std::tolower in the "C" locale).
///
/// # Safety
/// Null or NUL-terminated text.
#[no_mangle]
pub unsafe extern "C" fn cvr_contains_insensitive(
    haystack: *const c_char,
    needle: *const c_char,
) -> bool {
    match unsafe { (bytes(haystack), bytes(needle)) } {
        (Some(h), Some(n)) => contains_insensitive(h, n),
        _ => false,
    }
}

/// EqualsInsensitive.
///
/// # Safety
/// Null or NUL-terminated text.
#[no_mangle]
pub unsafe extern "C" fn cvr_equals_insensitive(a: *const c_char, b: *const c_char) -> bool {
    match unsafe { (bytes(a), bytes(b)) } {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
    }
}

pub(crate) fn likely_fpp_arm(name: &[u8]) -> bool {
    [
        "_fpp_",
        "fpp_lights",
        "strongarms_holstered",
        "personal_link_default_holstered",
        "injection_mark",
        "_pma_base__",
        "_pwa_base__",
        "_pma_fpp__neck",
        "_pwa_fpp__neck",
        "holstered_arms_data",
    ]
    .iter()
    .any(|n| contains_insensitive(name, n.as_bytes()))
}

/// IsLikelyFppArmComponent.
///
/// # Safety
/// Null or NUL-terminated text.
#[no_mangle]
pub unsafe extern "C" fn cvr_is_likely_fpp_arm_component(name: *const c_char) -> bool {
    unsafe { bytes(name) }.is_some_and(likely_fpp_arm)
}

/// ClassIsA: the class or one of its ancestors named `name`.
///
/// # Safety
/// `class` is null or a registered class.
#[no_mangle]
pub unsafe extern "C" fn cvr_class_is_a(class: *const Class, name: u64) -> bool {
    unsafe { class.as_ref() }.is_some_and(|c| c.is_a(CName(name)))
}

/// IsReadable: a VirtualQuery'd readability check (avoids the AV that crashed the game when a garbage float like
/// 0xF51C006E was treated as a pointer). A kernel transition: never on a per-ray or per-pass path.
#[no_mangle]
pub extern "C" fn cvr_is_readable(address: usize, length: usize) -> bool {
    use windows_sys::Win32::System::Memory::{
        VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_EXECUTE_READ,
        PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY,
        PAGE_READWRITE, PAGE_WRITECOPY,
    };
    if address < 0x10000 {
        return false;
    }
    // SAFETY: VirtualQuery of any address into a structure of ours.
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { core::mem::zeroed() };
    if unsafe {
        VirtualQuery(
            address as *const c_void,
            &mut info,
            core::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    } == 0
    {
        return false;
    }
    if info.State != MEM_COMMIT {
        return false;
    }
    let protection = info.Protect;
    if protection & (PAGE_NOACCESS | PAGE_GUARD) != 0 {
        return false;
    }
    let readable = PAGE_READONLY
        | PAGE_READWRITE
        | PAGE_EXECUTE_READ
        | PAGE_EXECUTE_READWRITE
        | PAGE_WRITECOPY
        | PAGE_EXECUTE_WRITECOPY;
    if protection & readable == 0 {
        return false;
    }
    address.wrapping_add(length) <= info.BaseAddress as usize + info.RegionSize
}

// ---- the hand natives

/// The hands snapshot latched once (the seqlock reader in Anim/VrikHook): its slots when it is stable.
fn snapshot() -> Option<impl Fn(i32) -> f32> {
    cvr_ensure_shared_memory();
    // SAFETY: the snapshot latch, then its slots.
    unsafe { RefreshHandsSnapshot() };
    get!(g_handsStableValid).then_some(|i| unsafe { SharedPose(i) })
}

/// # Safety
/// `frame` is the VM's.
unsafe fn no_args(frame: *mut StackFrame) {
    unsafe { args(frame) }.finish();
}

unsafe fn hand_valid(frame: *mut StackFrame, out: *mut c_void, at: i32) {
    unsafe { no_args(frame) };
    let valid = snapshot().is_some_and(|pose| pose(at) > 0.0);
    if !out.is_null() {
        unsafe { *(out as *mut bool) = valid };
    }
}
unsafe fn hand_pos(frame: *mut StackFrame, out: *mut c_void, at: i32) {
    unsafe { no_args(frame) };
    let p = snapshot().map(|pose| [pose(at), pose(at + 1), pose(at + 2)]);
    if !out.is_null() {
        let [x, y, z] = p.unwrap_or([0.0; 3]);
        unsafe { *(out as *mut [f32; 4]) = [x, y, z, 1.0] };
    }
}
unsafe fn hand_rot(frame: *mut StackFrame, out: *mut c_void, at: i32) {
    unsafe { no_args(frame) };
    let q = snapshot().map(|pose| [pose(at), pose(at + 1), pose(at + 2), pose(at + 3)]);
    if !out.is_null() {
        unsafe { *(out as *mut [f32; 4]) = q.unwrap_or([0.0, 0.0, 0.0, 1.0]) };
    }
}

/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn GetLeftVRHandValid(
    _c: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { hand_valid(frame, out, 0) };
}
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn GetRightVRHandValid(
    _c: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { hand_valid(frame, out, 8) };
}
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn GetLeftVRHandPos(
    _c: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { hand_pos(frame, out, 1) };
}
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn GetRightVRHandPos(
    _c: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { hand_pos(frame, out, 9) };
}
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn GetLeftVRHandRot(
    _c: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { hand_rot(frame, out, 4) };
}
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn GetRightVRHandRot(
    _c: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { hand_rot(frame, out, 12) };
}

/// IsVRHandLinked(): whether the shared hands memory is there.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn IsVRHandLinked(
    _c: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { no_args(frame) };
    cvr_ensure_shared_memory();
    if !out.is_null() {
        unsafe { *(out as *mut bool) = !get!(g_pSharedHands).is_null() };
    }
}

// Held as the C++ held it (g_rightHandEntity); nothing reads it back.
#[allow(dead_code)]
struct HeldEntity(WeakHandle);
// SAFETY: set and dropped on the script thread's natives only.
unsafe impl Send for HeldEntity {}
static RIGHT_HAND_ENTITY: Mutex<Option<HeldEntity>> = Mutex::new(None);

/// SetVRRightHandEntity(entity): kept as a weak handle.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn SetVRRightHandEntity(
    _c: *mut c_void,
    frame: *mut StackFrame,
    _out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let entity = a.get::<Handle>();
    a.finish();
    *RIGHT_HAND_ENTITY.lock().unwrap_or_else(|e| e.into_inner()) =
        Some(HeldEntity(WeakHandle::from_handle(&entity)));
}

// ---- the FPP component dump

const ENTITY_TEMPLATE_PATH: usize = 0x60;
const ENTITY_COMPONENTS: usize = 0xA0;
const COMPONENT_NAME: usize = 0x40;
const COMPONENT_ENABLED: usize = 0x8B; // entIComponent::isEnabled
                                       // The meshes' fields (asserted in src/Natives/Natives.cpp).
const SKINNED_MESH: usize = 0x228;
const SKINNED_APPEARANCE: usize = 0x230;
const SKINNED_CHUNK_MASK: usize = 0x248;
const MESH_MESH: usize = 0x150;
const MESH_APPEARANCE: usize = 0x190;
const MESH_CHUNK_MASK: usize = 0x198;
const MESH_VISUAL_SCALE: usize = 0x178;

fn name_text(name: u64) -> &'static str {
    CName(name).text()
}
/// # Safety
/// `object` is live and `at` one of its fields of type `T`.
unsafe fn read<T: Copy>(object: *const c_void, at: usize) -> T {
    unsafe { core::ptr::read_unaligned((object as *const u8).add(at) as *const T) }
}
/// C++'s float output (std::ostream's default: %g, six significant digits).
fn g(v: f32) -> String {
    cvr_crt::format(c"%g", &[cvr_crt::Arg::F(v as f64)])
        .to_string_lossy()
        .into_owned()
}

/// DumpVRFppComponents(): the player's components, with their mesh and chunk mask, into fpp_components_dump.txt; how
/// many (-1 no player, -2 no entity, -3 no file).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn DumpVRFppComponents(
    _c: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { no_args(frame) };
    let result = unsafe { dump_fpp_components() };
    if !out.is_null() {
        unsafe { *(out as *mut i32) = result };
    }
}

unsafe fn dump_fpp_components() -> i32 {
    let mut player = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut player) };
    if player.is_null() {
        return -1;
    }
    let entity = player.instance;
    // VRDiagPath: beside cyberpunkvrport.log.
    let path = cvr_host::paths::file(cvr_host::paths::Folder::Log, "fpp_components_dump.txt");
    let mut text = String::new();
    // SAFETY: the player entity's fields and components.
    unsafe {
        let _ = writeln!(
            text,
            "Player entity templatePathHash=0x{:x}",
            read::<u64>(entity, ENTITY_TEMPLATE_PATH)
        );
        text.push_str("Components:\n");
        let mut count = 0;
        for component in handles((entity as *const u8).add(ENTITY_COMPONENTS).cast()) {
            let component = borrowed(component);
            if component.is_null() {
                continue;
            }
            let c = component.instance;
            let class = object_class(c);
            let type_name = class.map_or("<null>", |t| t.class_name().text());
            let name = name_text(read::<u64>(c, COMPONENT_NAME));
            let _ = write!(
                text,
                "[{count}] ptr=0x{:x} name={name} type={type_name} enabled={}",
                c as usize,
                u8::from(read::<u8>(c, COMPONENT_ENABLED) != 0)
            );
            let is = |n: &str| class.is_some_and(|t| t.is_a(CName(fnv1a64(n.as_bytes()))));
            if is("entSkinnedMeshComponent") {
                let _ = write!(
                    text,
                    " meshAppearance={} meshPathHash=0x{:x} chunkMask=0x{:x}",
                    name_text(read(c, SKINNED_APPEARANCE)),
                    read::<u64>(c, SKINNED_MESH),
                    read::<u64>(c, SKINNED_CHUNK_MASK)
                );
            } else if is("entMeshComponent") {
                let scale: [f32; 3] = read(c, MESH_VISUAL_SCALE);
                let _ = write!(
                    text,
                    " meshAppearance={} meshPathHash=0x{:x} chunkMask=0x{:x} visualScale=({}, {}, {})",
                    name_text(read(c, MESH_APPEARANCE)),
                    read::<u64>(c, MESH_MESH),
                    read::<u64>(c, MESH_CHUNK_MASK),
                    g(scale[0]),
                    g(scale[1]),
                    g(scale[2])
                );
            }
            if likely_fpp_arm(name.as_bytes()) {
                text.push_str(" [LIKELY_FPP_ARM]");
            }
            text.push('\n');
            count += 1;
        }
        if std::fs::write(&path, text).is_err() {
            return -3;
        }
        count
    }
}

// ---- the chunk-debug setters (disabled in this build: each takes its arguments and leaves the debug off)

unsafe fn ints(frame: *mut StackFrame, n: usize) {
    let mut a = unsafe { args(frame) };
    for _ in 0..n {
        a.get::<i32>();
    }
    a.finish();
}
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn SetVRFppChunkDebugEnabled(
    _c: *mut c_void,
    frame: *mut StackFrame,
    _out: *mut c_void,
    _a4: i64,
) {
    unsafe { ints(frame, 1) };
    set!(g_chunkDebugEnabled, false);
}
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn SetVRFppChunkDebugComponentIndex(
    _c: *mut c_void,
    frame: *mut StackFrame,
    _out: *mut c_void,
    _a4: i64,
) {
    unsafe { ints(frame, 1) };
    set!(g_chunkDebugComponentIndex, -1);
}
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn SetVRFppChunkDebugHand(
    _c: *mut c_void,
    frame: *mut StackFrame,
    _out: *mut c_void,
    _a4: i64,
) {
    unsafe { ints(frame, 1) };
    set!(g_chunkDebugHand, 1);
}
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn SetVRFppChunkDebugBits(
    _c: *mut c_void,
    frame: *mut StackFrame,
    _out: *mut c_void,
    _a4: i64,
) {
    unsafe { ints(frame, 4) };
    set!(g_chunkDebugBitSlots, [-1; 4]);
}
