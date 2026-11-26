//! LeftHandedAnimation (formerly the detours of src/Hooks/LeftHandedAnimation.cpp): while the weapon is alone in the
//! LEFT hand, the names reaching the player's animation controller are mirrored at its two doors -- ApplyFeature
//! (0x53DC14) and PushEvent (0x824614) -- so the left weapon drives the single-wield branch and the empty right hand
//! the other. The script native that turns it on (VRLeftHandedAnimations: the trade's empty handlings, the single
//! wield's graph) stays C++ with the engine's RTTI, calling the original ApplyFeature through
//! `cvr_left_handed_apply_feature`.
use core::ffi::c_void;
use cvr_engine::code::{image_base, read_bytes};
use cvr_engine::hooks::{install_chain, Detour};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

const APPLY_FEATURE: usize = 0x53DC14;
const PUSH_EVENT: usize = 0x824614;

/// CName: FNV-1a, 64 bits.
const fn name_hash(s: &str) -> u64 {
    let b = s.as_bytes();
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    let mut i = 0;
    while i < b.len() {
        h = (h ^ b[i] as u64).wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}
/// Every name of the ranged graph that has a twin on the other branch.
const TWINS: [[u64; 2]; 9] = [
    [name_hash("RightWeaponData"), name_hash("LeftWeaponData")],
    [name_hash("rightHandItemHandling"), name_hash("leftHandItemHandling")],
    [name_hash("RightReloadData"), name_hash("LeftReloadData")],
    [name_hash("WeaponEquipType"), name_hash("LeftWeaponEquipType")],
    [name_hash("RightReload"), name_hash("LeftReload")],
    [name_hash("RightShoot"), name_hash("LeftShoot")],
    [name_hash("RightReloadLoop"), name_hash("LeftReloadLoop")],
    [name_hash("EndRightReload"), name_hash("EndLeftReload")],
    [name_hash("RightCycleRound"), name_hash("LeftCycleRound")],
];

/// The last names mirrored, as received (diagnostics): name hash, then kind (1 feature, 2 event) | mirrored << 8 | an
/// item handling's itemState << 16 | its itemType << 24 (0xFFFF << 16 when not one).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LeftHandedNameRecord {
    pub name: u64,
    pub info: u32,
    pub pad: u32,
}

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_LeftHandedNamesMirrored: AtomicU64;
    static CyberpunkVR_LeftHandedController: AtomicUsize;
    static mut CyberpunkVR_LeftHandedNameLog: [LeftHandedNameRecord; 64];
    static CyberpunkVR_LeftHandedNameLogCount: AtomicU32;
    /// The handling bits of a feature (`cvr_item_handling_bits` below in the plugin, through its C++ seam
    /// src/Hooks/LeftHandedAnimation.cpp; the benches name their own).
    fn cvr_cpp_item_handling_bits(feature: *const c_void) -> u32;
}

static mut APPLY_ORIG: *mut c_void = core::ptr::null_mut();
static mut PUSH_ORIG: *mut c_void = core::ptr::null_mut();
type ApplyFn = unsafe extern "C" fn(usize, u64, *mut c_void) -> u64;
type PushFn = unsafe extern "C" fn(usize, u64) -> u64;

/// For tools/vrik_tests: the two originals the detours call.
#[no_mangle]
pub unsafe extern "C" fn cvr_left_handed_bind(apply: usize, push: usize) {
    // SAFETY: the slots the detours read, set before any call.
    unsafe {
        APPLY_ORIG = apply as *mut c_void;
        PUSH_ORIG = push as *mut c_void;
    }
}

unsafe fn note(name: u64, kind: u32, feature: *const c_void) {
    // SAFETY: a C++ service reading the feature handle, and the exported log.
    unsafe {
        let handling = if feature.is_null() { 0xFFFF } else { cvr_cpp_item_handling_bits(feature) };
        let at = CyberpunkVR_LeftHandedNameLogCount.fetch_add(1, Ordering::Relaxed);
        CyberpunkVR_LeftHandedNameLog[(at % 64) as usize] = LeftHandedNameRecord { name, info: kind | 1 << 8 | handling << 16, pad: 0 };
    }
}
unsafe fn mirrored(controller: usize, name: u64, kind: u32, feature: *const c_void) -> u64 {
    // SAFETY: the exported controller and counters.
    unsafe {
        if controller == 0 || controller != CyberpunkVR_LeftHandedController.load(Ordering::Relaxed) {
            return name;
        }
        for twin in &TWINS {
            if name == twin[0] || name == twin[1] {
                CyberpunkVR_LeftHandedNamesMirrored.fetch_add(1, Ordering::Relaxed);
                note(name, kind, feature);
                return if name == twin[0] { twin[1] } else { twin[0] };
            }
        }
    }
    name
}

#[no_mangle]
pub unsafe extern "C" fn CvrLeftHandedApplyFeature(controller: usize, name: u64, feature: *mut c_void) -> u64 {
    // SAFETY: the engine's original with its own arguments.
    unsafe { core::mem::transmute::<*mut c_void, ApplyFn>(APPLY_ORIG)(controller, mirrored(controller, name, 1, feature), feature) }
}
#[no_mangle]
pub unsafe extern "C" fn CvrLeftHandedPushEvent(controller: usize, name: u64) -> u64 {
    // SAFETY: as CvrLeftHandedApplyFeature.
    unsafe { core::mem::transmute::<*mut c_void, PushFn>(PUSH_ORIG)(controller, mirrored(controller, name, 2, core::ptr::null())) }
}
/// The original ApplyFeature, for the script native's empty handlings (the names it writes are not mirrored).
#[no_mangle]
pub unsafe extern "C" fn cvr_left_handed_apply_feature(controller: usize, name: u64, feature: *mut c_void) -> u64 {
    // SAFETY: the trampoline, once the hook is installed (the native checks cvr_left_handed_installed first).
    unsafe { core::mem::transmute::<*mut c_void, ApplyFn>(APPLY_ORIG)(controller, name, feature) }
}
#[no_mangle]
pub extern "C" fn cvr_left_handed_installed() -> bool {
    // SAFETY: a plain read of the slot.
    unsafe { !(*(&raw const APPLY_ORIG)).is_null() }
}

fn log(text: &str) {
    super::env::log(text);
}
fn matches(at: usize, hex: &str) -> bool {
    let want: Vec<u8> = (0..hex.len() / 2).filter_map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()).collect();
    let mut got = vec![0u8; want.len()];
    read_bytes(at, &mut got) && got == want
}
/// Installed from src/Hooks/LeftHandedAnimation.cpp's registration (Boot, 91).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_left_handed_animation_install() -> bool {
    let base = image_base();
    if base == 0 || !matches(base + APPLY_FEATURE, "488bc4488958084889681048897018") || !matches(base + PUSH_EVENT, "48895c24084889742410574883ec70") {
        log("LeftHandedAnimation: native ABI mismatch; hooks not installed.\n");
        return false;
    }
    let d = |name, target, detour: *const c_void, original: *mut *mut c_void| Detour { name, target, absolute: false, detour, original, expect: None };
    let detours = [
        d("ApplyFeature", APPLY_FEATURE, CvrLeftHandedApplyFeature as *const c_void, &raw mut APPLY_ORIG),
        d("PushEvent", PUSH_EVENT, CvrLeftHandedPushEvent as *const c_void, &raw mut PUSH_ORIG),
    ];
    // SAFETY: both detours have their targets' signatures and read their trampolines from the slots.
    if unsafe { install_chain("LeftHandedAnimation", &detours) }.is_err() {
        return false;
    }
    log("LeftHandedAnimation: animation controller ApplyFeature/PushEvent installed (2.5).\n");
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_hash_as_cnames() {
        assert_eq!(super::name_hash(""), 0xCBF2_9CE4_8422_2325);
        assert_ne!(super::TWINS[0][0], super::TWINS[0][1]);
    }
}

// ---- the script native (formerly in src/Hooks/LeftHandedAnimation.cpp) ----

/// An AnimFeature_EquipUnequipItem's state and type, 0xFFFF for a handle holding anything else.
#[no_mangle]
pub unsafe extern "C" fn cvr_item_handling_bits(feature: *const c_void) -> u32 {
    if feature.is_null() {
        return 0xFFFF;
    }
    let instance = *(feature as *const *const c_void);
    let equip = red4ext_abi::rtti::RttiSystem::get().and_then(|r| r.get_class(red4ext_abi::cname::CName::hashed("animAnimFeature_EquipUnequipItem")));
    match (instance.is_null(), equip) {
        (false, Some(equip)) if red4ext_abi::rtti::object_class(instance).is_some_and(|c| core::ptr::eq(c, equip)) => {
            let state = *((instance as usize + 0x44) as *const i32) as u32;
            let ty = *((instance as usize + 0x48) as *const i32) as u32;
            (state & 0xFF) | (ty & 0xFF) << 8
        }
        _ => 0xFFFF,
    }
}

/// The graph slot natives (2.5 0xD61900 attach, 0xD617E8 detach): controller, slot, graph handle, source.
type SlotGraphFn = unsafe extern "C" fn(usize, red4ext_abi::cname::CName, *mut red4ext_abi::handle::Handle, red4ext_abi::cname::CName) -> u64;
const ATTACH_GRAPH: usize = 0xD61900;
const DETACH_GRAPH: usize = 0xD617E8;
extern "C" {
    static CyberpunkVR_LeftHandedGraphsAttached: AtomicU32;
}
fn graph_slots() -> Option<(SlotGraphFn, SlotGraphFn)> {
    static RESOLVED: std::sync::OnceLock<Option<(usize, usize)>> = std::sync::OnceLock::new();
    let r = *RESOLVED.get_or_init(|| {
        let base = image_base();
        if base != 0 && matches(base + ATTACH_GRAPH, "488bc448895808488968104889701848") && matches(base + DETACH_GRAPH, "488bc448895808488968104889701848") {
            Some((base + ATTACH_GRAPH, base + DETACH_GRAPH))
        } else {
            log("LeftHandedAnimation: graph slot natives mismatch; the left hand's single wield cannot attach its graph.\n");
            None
        }
    });
    // SAFETY: the verified natives' addresses.
    r.map(|(a, d)| unsafe { (core::mem::transmute::<usize, SlotGraphFn>(a), core::mem::transmute::<usize, SlotGraphFn>(d)) })
}
/// Both hands' item handlings written empty through the original ApplyFeature (the trade's names are not mirrored).
unsafe fn empty_handlings(controller: usize) {
    let Some(class) = red4ext_abi::rtti::RttiSystem::get().and_then(|r| r.get_class(red4ext_abi::cname::CName::hashed("animAnimFeature_EquipUnequipItem")))
    else {
        return;
    };
    if controller == 0 || !cvr_left_handed_installed() {
        return;
    }
    for name in ["rightHandItemHandling", "leftHandItemHandling"] {
        let feature = class.create_instance();
        if feature.is_null() {
            return;
        }
        let mut handle = red4ext_abi::handle::Handle::from_instance(feature);
        cvr_left_handed_apply_feature(controller, name_hash(name), &mut handle as *mut red4ext_abi::handle::Handle as *mut c_void);
    }
}
/// The single wield's graph, from the owner entity's graph container, attached to the slot (detached first).
unsafe fn attach_single_wield_graph(controller: usize, graph_name: red4ext_abi::cname::CName, slot: red4ext_abi::cname::CName) {
    use red4ext_abi::cname::CName;
    use red4ext_abi::dynarray::DynArray;
    let Some((attach, detach)) = graph_slots() else { return };
    if graph_name.0 == 0 || slot.0 == 0 {
        return;
    }
    let Some(container_type) = red4ext_abi::rtti::RttiSystem::get().and_then(|r| r.get_class(CName::hashed("entAnimGraphResourceContainer"))) else {
        return;
    };
    // IComponent::owner, the entity's components.
    let entity = *((controller + 0x50) as *const usize);
    if entity == 0 {
        return;
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct RawHandle {
        instance: usize,
        ref_count: usize,
    }
    /// entAnimGraphResourceContainerEntry: graphName, then Ref<animAnimGraph> (path, token SharedPtr).
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Entry {
        graph_name: CName,
        path: u64,
        token: usize,
        token_ref: usize,
    }
    for component in (*((entity + 0xA0) as *const DynArray<RawHandle>)).as_slice() {
        if component.instance == 0 || !red4ext_abi::rtti::object_class(component.instance as *const c_void).is_some_and(|c| core::ptr::eq(c, container_type)) {
            continue;
        }
        for entry in (*((component.instance + 0x90) as *const DynArray<Entry>)).as_slice() {
            if entry.graph_name != graph_name {
                continue;
            }
            // ResourceToken: finished (+0x58) and no error (+0x5C); its resource handle at +0x28.
            let loaded = entry.token != 0 && core::ptr::read_volatile((entry.token + 0x58) as *const i32) != 0 && *((entry.token + 0x5C) as *const u8) == 0;
            if !loaded {
                log("LeftHandedAnimation: the single wield's graph is not loaded; not attached.\n");
                return;
            }
            let mut graph = (*((entry.token + 0x28) as *const red4ext_abi::handle::Handle)).clone();
            let source = CName::hashed("State machine");
            detach(controller, slot, &mut graph, source);
            attach(controller, slot, &mut graph, source);
            CyberpunkVR_LeftHandedGraphsAttached.fetch_add(1, Ordering::Relaxed);
            return;
        }
    }
}
/// VRLeftHandedAnimations(controller, on, graph, slot): while the weapon is alone in the LEFT hand, the controller's
/// names are mirrored (the detours above), both hands' handlings are emptied on the change, and the single wield's graph
/// is attached to the slot.
#[no_mangle]
pub unsafe extern "C" fn VRLeftHandedAnimations(_this: *mut c_void, frame: *mut red4ext_abi::stack::StackFrame, _out: *mut c_void, _unused: i64) {
    let mut controller = red4ext_abi::handle::Handle::null();
    let mut on = false;
    let mut graph = red4ext_abi::cname::CName::default();
    let mut slot = red4ext_abi::cname::CName::default();
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut controller);
        frame.get_parameter(&mut on);
        frame.get_parameter(&mut graph);
        frame.get_parameter(&mut slot);
        frame.finish_params();
    }
    let component = controller.instance as usize;
    let next = if on { component } else { 0 };
    let previous = CyberpunkVR_LeftHandedController.swap(next, Ordering::Relaxed);
    if component != 0 && (if on { previous != component } else { previous != 0 }) {
        empty_handlings(component);
    }
    if component != 0 && on {
        attach_single_wield_graph(component, graph, slot);
    }
}
