//! Small natives of the frame budget and the settings (formerly src/Natives/Performance.cpp): whether an entity is worth
//! the Lua body-capsule scan, the Reflex timing report, the mouse-Y switch and the overlay's VRCAM request. The A/B
//! switch `CyberpunkVR_BodyCapsulePrefilter` stays exported by src/Natives/Performance.cpp.
use super::xinput::InputControls;
use core::ffi::c_void;
use red4ext_abi::cname::CName;
use red4ext_abi::cstring::CString;
use red4ext_abi::dynarray::DynArray;
use red4ext_abi::handle::Handle;
use red4ext_abi::stack::StackFrame;
use std::sync::Mutex;

extern "C" {
    static CyberpunkVR_BodyCapsulePrefilter: i32;
    fn cvr_cpp_vrcam_enable_request() -> i32;
}

/// entEntity: its components (DynArray<Handle<IComponent>>) and status (Attached = 4); IComponent's name.
const ENTITY_COMPONENTS: usize = 0xA0;
const ENTITY_STATUS: usize = 0x156;
const STATUS_ATTACHED: u8 = 4;
const COMPONENT_NAME: usize = 0x40;

#[repr(C)]
#[derive(Clone, Copy)]
struct RawHandle {
    instance: usize,
    ref_count: usize,
}
/// The last entity's component set and whether it had a capsule.
struct Last {
    entity: usize,
    set: u64,
    found: bool,
}
static LAST: Mutex<Last> = Mutex::new(Last {
    entity: 0,
    set: 0,
    found: true,
});

/// VRShouldScanBodyCapsules(entity) -> Bool: false for an entity with no "VRPortBody_" component (the Lua scan is
/// then skipped). Loading or unattached entities keep the Lua path. The component names are walked again only when
/// the set of component pointers changes (a name-pool lookup and a search for each of ~240 components, every frame
/// while no capsule exists).
#[no_mangle]
pub unsafe extern "C" fn VRShouldScanBodyCapsules(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    let mut entity = Handle::null();
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut entity);
        frame.finish_params();
    }
    let Some(out) = out.as_mut() else { return };
    *out = should_scan_body_capsules(entity.instance);
}
/// The prefilter itself, for the Rust bridges that scan (rust/crates/cvr-ffi/src/mechanics.rs basketball).
///
/// # Safety
/// `entity` is null or a live entity, read on the game thread.
pub unsafe fn should_scan_body_capsules(entity: *mut c_void) -> bool {
    if CyberpunkVR_BodyCapsulePrefilter == 0 {
        return true;
    }
    if entity.is_null() {
        return false;
    }
    let e = entity as *const u8;
    // Menus too: the main menu's puppet is attached and has no capsules.
    if *e.add(ENTITY_STATUS) != STATUS_ATTACHED {
        return true;
    }
    // CET invokes this on the game thread; the components are read in place.
    let components = (*(e.add(ENTITY_COMPONENTS) as *const DynArray<RawHandle>)).as_slice();
    let mut set = 0xcbf2_9ce4_8422_2325u64;
    for c in components {
        set = (set ^ c.instance as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
    if entity as usize == last.entity && set == last.set {
        return last.found;
    }
    let found = components
        .iter()
        .filter(|c| c.instance != 0)
        .any(|c| (*((c.instance + COMPONENT_NAME) as *const CName)).text().contains("VRPortBody_"));
    *last = Last {
        entity: entity as usize,
        set,
        found,
    };
    found
}

unsafe extern "C" fn into_string(out: *mut c_void, data: *const u8, size: usize) {
    let text = core::slice::from_raw_parts(data, size);
    *(out as *mut String) = String::from_utf8_lossy(text).into_owned();
}
/// VRReflexTiming(request) -> String: the Reflex timing report (reflex_hook.rs).
#[no_mangle]
pub unsafe extern "C" fn VRReflexTiming(_this: *mut c_void, frame: *mut StackFrame, out: *mut CString, _unused: i64) {
    let mut request = false;
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut request);
        frame.finish_params();
    }
    if !out.is_null() {
        let mut text = String::new();
        super::reflex_hook::cvr_reflex_timing_report(request, into_string, &mut text as *mut String as *mut c_void);
        CString::assign(out, &text);
    }
}
/// GetVRMouseYDisabled() -> Bool: xr_disable_mouse_y.
#[no_mangle]
pub unsafe extern "C" fn GetVRMouseYDisabled(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    if let Some(frame) = frame.as_mut() {
        frame.finish_params();
    }
    if let Some(out) = out.as_mut() {
        let mut controls = InputControls::default();
        super::live_reads::input_controls(&mut controls);
        *out = controls.disable_mouse_y != 0;
    }
}
/// VRVrcamEnableRequest() -> Int32: the overlay's last VRCAM enable request: -1 none, 0 disable, 1 enable.
#[no_mangle]
pub unsafe extern "C" fn VRVrcamEnableRequest(_this: *mut c_void, frame: *mut StackFrame, out: *mut i32, _unused: i64) {
    if let Some(frame) = frame.as_mut() {
        frame.finish_params();
    }
    if let Some(out) = out.as_mut() {
        *out = cvr_cpp_vrcam_enable_request();
    }
}
