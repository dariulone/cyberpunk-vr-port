//! The animation inputs (formerly src/Natives/AnimGraph.cpp's and src/Natives/AnimFeatures.cpp's natives, and the
//! graph-variable and track setters of src/Natives/AnimDumps.cpp): the root graph's variables, the root meta-rig's
//! tracks and their live copies, the player's animated parameters, the anim features and input events handed to the
//! animation controller, and the per-frame UpdateVRIKAnimInputs -- the in-VR overlay's activation of the solve and
//! the persistent presets.
//!
//! AN ANIM FEATURE is how a value reaches the animation graph from outside: a Handle of the right RTTI class, filled,
//! handed to the AnimationControllerComponent. Every part of that is a place to be wrong, and the Test*Route natives
//! exist because being wrong is silent -- the feature is accepted and nothing moves. THE HANDLE REFCOUNT IS THE TRAP:
//! one constructed and dropped without being installed takes the feature with it.
use core::ffi::c_void;
use std::ffi::CString;
use std::fmt::Write as _;
use std::io::Write as _;
use std::sync::Mutex;

use cvr_engine::code::{guarded_call3, read, write};
use red4ext_abi::cname::CName;
use red4ext_abi::fnv::fnv1a64;
use red4ext_abi::function::BaseFunction;
use red4ext_abi::handle::Handle;
use red4ext_abi::rtti::{object_class, Class, RttiSystem};
use red4ext_abi::stack::{CStack, StackFrame, StackType};

use crate::anim_lookup::{
    classify_qword, components, player_animated_object, player_animation_controller, player_entity,
    safe,
};
use crate::native_args::{args, borrowed};

#[allow(non_upper_case_globals)]
extern "C" {
    static mut g_pSharedHands: *mut f32;
    static g_AnimPoseMatchCalls: u64;
    static mut g_VRBind: i32;
    static mut g_VRDiagCapture: i32;
    static g_VRNeutralizeAnimGraph: i32;
    static mut g_chunkDebugEnabled: bool;
    static mut g_chunkDebugWasEnabled: bool;
    fn VRIK_DoArmPlayer() -> i32;
    fn VRIK_PlayerBindingChanged() -> bool;
    fn cvr_anim_wheel_reset();
    fn cvr_ensure_shared_memory();
    fn cvr_poll_vr_calib_from_shared();
    // src/Natives/AnimLookup.cpp and AnimGraph.cpp: the pose hook and the root graph resource.
    fn cvr_cpp_install_anim_pose_hook() -> bool;
    fn cvr_cpp_load_graph(animated_component: *mut c_void) -> *mut c_void;
}

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

type V4 = [f32; 4];

fn name(text: &str) -> CName {
    CName(fnv1a64(text.as_bytes()))
}
fn class(text: &str) -> Option<&'static Class> {
    RttiSystem::get()?.get_class(name(text))
}
fn type_name(object: usize) -> Option<CName> {
    // SAFETY: a live scriptable.
    unsafe { object_class(object as *const c_void) }.map(Class::class_name)
}
fn text(n: CName) -> &'static str {
    n.text()
}
/// C++'s float output (std::ostream's default, %g).
fn g(v: f32) -> String {
    cvr_crt::format(c"%g", &[cvr_crt::Arg::F(v as f64)])
        .to_string_lossy()
        .into_owned()
}
/// std::to_string(float): %f.
fn to_string(v: f32) -> String {
    cvr_crt::format(c"%f", &[cvr_crt::Arg::F(v as f64)])
        .to_string_lossy()
        .into_owned()
}
fn append(file: &str, line: &str) {
    let path = cvr_host::paths::file(cvr_host::paths::Folder::Log, file);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

// ---- the persistent presets (the C++ build's g_root*Persistent*, g_animParam*, g_animInputTestMode)

#[derive(Default)]
struct Presets {
    graph_float: (i32, f32, i32),
    graph_vector: (i32, V4, i32),
    meta_rig: (i32, f32, i32),
    live: (i32, f32, i32, i32), // preset, value, array mode, last result
    param: (i32, f32, i32),
    test_mode: i32,
}
static PRESETS: Mutex<Option<Presets>> = Mutex::new(None);
fn presets<R>(f: impl FnOnce(&mut Presets) -> R) -> R {
    let mut guard = PRESETS.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Presets::default))
}

fn anim_float_preset(mode: i32) -> Option<&'static str> {
    Some(match mode {
        1 => "visibilityLeftArm",
        2 => "visibilityRightArm",
        3 => "renderPlaneLeftArm",
        4 => "renderPlane",
        5 => "renderPlaneInspect",
        _ => return None,
    })
}
fn root_graph_float_preset(mode: i32) -> Option<&'static str> {
    Some(match mode {
        1 => "disable_maya_engine_right_hand",
        2 => "disable_maya_engine_left_hand",
        3 => "pla_right_hand_attach",
        4 => "pla_left_hand_attach",
        5 => "procedural_ironsight_camera",
        6 => "camera_pitch",
        7 => "camera_yaw",
        _ => return None,
    })
}
fn root_graph_vector_preset(mode: i32) -> Option<&'static str> {
    Some(match mode {
        1 => "weapon_offset_shoulder",
        2 => "weapon_offset_aiming",
        3 => "weapon_rotation_shoulder",
        4 => "weapon_rotation_aiming",
        5 => "debug_stand_camera_position",
        6 => "debug_crouch_camera_position",
        _ => return None,
    })
}
/// The root meta-rig's tracks the presets name (AnimDumps.cpp ResolveRootMetaRigTrackPreset).
fn root_meta_rig_track_preset(mode: i32) -> Option<&'static str> {
    Some(match mode {
        1 => "leftArmBodyPartVis",
        2 => "rightArmBodyPartVis",
        3 => "leftHandWorldSpace",
        4 => "rightHandWorldSpace",
        5 => "allowFeetIk",
        6 => "enableLeftFootIk",
        7 => "enableRightFootIk",
        8 => "cameraUpOffset",
        _ => return None,
    })
}

// ---- the root graph's variables

const ANIMATED_PARAMETERS: usize = 0x278; // entAnimatedComponent: DynArray<entAnimTrackParameter> (0x30 each)
const GRAPH_VARIABLES: usize = 0x58; // animAnimGraph: Handle<animAnimVariableContainer>
const BOOL_VARIABLES: usize = 0x38;
const FLOAT_VARIABLES: usize = 0x58;
const VECTOR_VARIABLES: usize = 0x68;
const VARIABLE_NAME: usize = 0x30;
const VARIABLE_VALUE: usize = 0x38;
const COMPONENT_NAME: usize = 0x40;

/// FindPlayerAnimatedComponentByName: the player's entAnimatedComponent of that name.
fn player_animated_component(component_name: &str) -> usize {
    let entity = player_entity();
    if entity == 0 {
        return 0;
    }
    let (kind, wanted) = (name("entAnimatedComponent"), name(component_name));
    // SAFETY: the player's components and their names.
    for component in unsafe { components(entity) } {
        let c = unsafe { borrowed(component) }.instance as usize;
        if c != 0
            && type_name(c) == Some(kind)
            && unsafe { *((c + COMPONENT_NAME) as *const CName) } == wanted
        {
            return c;
        }
    }
    0
}

/// The root graph's variable container (the graph resource loaded when it is not).
fn root_variables() -> usize {
    let animated = player_animated_component("root");
    if animated == 0 {
        return 0;
    }
    // SAFETY: LoadResourceRef(animated->graph), then the graph's variables handle.
    let graph = unsafe { cvr_cpp_load_graph(animated as *mut c_void) } as usize;
    if graph == 0 {
        0
    } else {
        unsafe { *((graph + GRAPH_VARIABLES) as *const usize) }
    }
}

/// The variables of one kind named `wanted`, each written by `write`; how many.
fn set_variables(container: usize, at: usize, wanted: CName, write: impl Fn(usize)) -> i32 {
    // SAFETY: a DynArray<Handle<animAnimVariable*>> of the container.
    let list = unsafe { crate::native_args::handles((container + at) as *const c_void) };
    let mut updated = 0;
    for v in list {
        let v = v[0];
        if v != 0 && unsafe { *((v + VARIABLE_NAME) as *const CName) } == wanted {
            write(v);
            updated += 1;
        }
    }
    updated
}
pub(crate) fn set_root_graph_float(n: CName, value: f32) -> i32 {
    match root_variables() {
        0 => -70,
        c => set_variables(c, FLOAT_VARIABLES, n, |v| unsafe {
            *((v + VARIABLE_VALUE) as *mut f32) = value
        }),
    }
}
fn set_root_graph_bool(n: CName, value: bool) -> i32 {
    match root_variables() {
        0 => -71,
        c => set_variables(c, BOOL_VARIABLES, n, |v| unsafe {
            *((v + VARIABLE_VALUE) as *mut bool) = value
        }),
    }
}
fn set_root_graph_vector(n: CName, value: V4) -> i32 {
    match root_variables() {
        0 => -72,
        c => set_variables(c, VECTOR_VARIABLES, n, |v| unsafe {
            *((v + VARIABLE_VALUE) as *mut V4) = value
        }),
    }
}

// ---- the root meta-rig's tracks

const META_RIG_REFERENCE_TRACKS: usize = 0x40; // animMetaRig: DynArray<float>
const META_RIG_TRACK_NAMES: usize = 0x50; // DynArray<CName>
const ANIMATED_OBJECT_META_RIG: usize = 0x08;

fn dynarray(at: usize) -> (usize, u32) {
    (safe::<usize>(at), safe::<u32>(at + 0xC))
}

fn set_root_meta_rig_track(n: CName, value: f32) -> i32 {
    let object = player_animated_object(c"root");
    let meta_rig = if object == 0 {
        0
    } else {
        safe::<usize>(object + ANIMATED_OBJECT_META_RIG)
    };
    if meta_rig == 0 {
        return -81;
    }
    let (names, name_count) = dynarray(meta_rig + META_RIG_TRACK_NAMES);
    let (tracks, track_count) = dynarray(meta_rig + META_RIG_REFERENCE_TRACKS);
    let mut updated = 0;
    for i in 0..name_count.min(track_count) as usize {
        if safe::<u64>(names + i * 8) == n.0 {
            if !write::<f32>(tracks + i * 4, value) {
                return -86;
            }
            updated += 1;
        }
    }
    updated
}

/// The live runtime track storage of the root AnimatedObject (not the read-only metaRig->referenceTracks resource
/// defaults, which crash on write), found by scan: animatedObject+0x8 -> owner+0x40 and animatedObject+0x18 ->
/// owner+0x18, each a DynArray<float> mirroring the track values. `mode` 0 both, 1 the first, 2 the second.
fn live_track_index(n: CName) -> Result<(usize, usize), i32> {
    let object = player_animated_object(c"root");
    if object == 0 {
        return Err(-90);
    }
    let meta_rig: u64 = safe(object + ANIMATED_OBJECT_META_RIG);
    if meta_rig == 0 || classify_qword(meta_rig) != "HEAP" {
        return Err(-90);
    }
    let (names, count) = dynarray(meta_rig as usize + META_RIG_TRACK_NAMES);
    if count == 0 || count > 8192 {
        return Err(-91);
    }
    (0..count as usize)
        .find(|&i| safe::<u64>(names + i * 8) == n.0)
        .map(|i| (object, i))
        .ok_or(-91)
}
const LIVE_ARRAYS: [(usize, usize); 2] = [(0x8, 0x40), (0x18, 0x18)];

fn set_root_live_track(n: CName, value: f32, mode: i32) -> i32 {
    let (object, index) = match live_track_index(n) {
        Ok(found) => found,
        Err(code) => return code,
    };
    let mut writes = 0;
    for (a, &(owner_at, array_at)) in LIVE_ARRAYS.iter().enumerate() {
        if (mode == 1 && a != 0) || (mode == 2 && a != 1) {
            continue;
        }
        let owner: u64 = safe(object + owner_at);
        if classify_qword(owner) != "HEAP" {
            continue;
        }
        let (entries, size) = dynarray(owner as usize + array_at);
        if classify_qword(entries as u64) != "HEAP" || index as u32 >= size {
            continue;
        }
        if !write::<f32>(entries + index * 4, value) {
            return -92;
        }
        writes += 1;
    }
    writes
}

/// The current live value (the first array, or the second with mode 2), times 1000 as an Int32.
fn read_root_live_track(n: CName, mode: i32) -> i32 {
    let (object, index) = match live_track_index(n) {
        Ok(found) => found,
        Err(code) => return code,
    };
    let (owner_at, array_at) = if mode == 2 {
        LIVE_ARRAYS[1]
    } else {
        LIVE_ARRAYS[0]
    };
    let owner: u64 = safe(object + owner_at);
    if classify_qword(owner) != "HEAP" {
        return -92;
    }
    let (entries, size) = dynarray(owner as usize + array_at);
    if classify_qword(entries as u64) != "HEAP" || index as u32 >= size {
        return -92;
    }
    match read::<f32>(entries + index * 4) {
        Some(v) => (v * 1000.0) as i32,
        None => -92,
    }
}

fn root_meta_rig_log(source: &str, key: CName, value: f32, result: i32) {
    append(
        "root_metarig_track_test_log.txt",
        &format!(
            "source={source} key={} value={} result={result}\n",
            text(key),
            g(value)
        ),
    );
}

// ---- RTTI calls and new instances

/// RED4ext::ExecuteFunction(instance, f, out, args): a null argument type taken from the function's parameter, and a
/// function with a result not called without a place for it (cvr-render world_markers has the same).
///
/// # Safety
/// `instance` is a live instance of the function's class.
unsafe fn execute(
    instance: usize,
    f: &BaseFunction,
    out: *mut c_void,
    args: &mut [StackType],
) -> bool {
    let Some(rtti) = RttiSystem::get() else {
        return false;
    };
    let mut result = StackType {
        ty: core::ptr::null(),
        value: out,
    };
    let result_ptr = match unsafe { f.return_type.as_ref() } {
        Some(_) if out.is_null() => return false,
        Some(r) => {
            result.ty = r.ty;
            &mut result as *mut StackType
        }
        None => core::ptr::null_mut(),
    };
    for (arg, param) in args.iter_mut().zip(f.params.as_slice()) {
        if arg.ty.is_null() {
            if let Some(param) = unsafe { param.as_ref() } {
                arg.ty = param.ty;
            }
        }
    }
    let Some(mut stack) = CStack::new(rtti, instance as *mut c_void, args, result_ptr) else {
        return false;
    };
    let Some(address) = f.execute_address() else {
        return false;
    };
    unsafe {
        guarded_call3(
            address,
            f as *const BaseFunction as *mut c_void,
            &mut stack as *mut CStack as *mut c_void,
            core::ptr::null_mut(),
        )
    }
    .unwrap_or(false)
}
fn arg<T>(value: &T) -> StackType {
    StackType {
        ty: core::ptr::null(),
        value: value as *const T as *mut c_void,
    }
}
fn function(object: usize, short_name: &str) -> Option<&'static BaseFunction> {
    // SAFETY: a live scriptable's class.
    unsafe { object_class(object as *const c_void) }?.function(name(short_name))
}

/// A new instance of a class, its properties made (CreateInstance(true) then InitializeProperties), held by a handle.
fn create(class_name: &str) -> Option<(&'static Class, Handle)> {
    let cls = class(class_name)?;
    let instance = cls.create_instance();
    if instance.is_null() {
        return None;
    }
    // SAFETY: the new instance of this class.
    unsafe { cls.initialize_properties(instance) };
    Some((cls, unsafe { Handle::from_instance(instance) }))
}
/// A property of the instance written.
///
/// # Safety
/// `T` is the property's type, and for a handle the slot holds a null handle.
unsafe fn put<T>(cls: &Class, instance: *mut c_void, property: &str, value: T) {
    if let Some(p) = cls.property(name(property)) {
        unsafe { core::ptr::write(p.value_ptr::<T>(instance), value) };
    }
}

fn weapon_user_feature(left: V4, right: V4) -> Option<Handle> {
    let (cls, h) = create("animAnimFeature_WeaponUser")?;
    unsafe {
        put(cls, h.instance, "ikLeftHandLocalPosition", left);
        put(cls, h.instance, "ikRightHandLocalPosition", right);
    }
    Some(h)
}
fn ik_feature(point: V4, normal: V4, weight: f32) -> Option<Handle> {
    let (cls, h) = create("animAnimFeature_IK")?;
    unsafe {
        put(cls, h.instance, "point", point);
        put(cls, h.instance, "normal", normal);
        put(cls, h.instance, "weight", weight);
    }
    Some(h)
}
fn melee_ik_data_feature(head: V4, chest: V4, offset: V4) -> Option<Handle> {
    let (cls, h) = create("animAnimFeature_MeleeIKData")?;
    unsafe {
        put(cls, h.instance, "isValid", true);
        put(cls, h.instance, "headPosition", head);
        put(cls, h.instance, "chestPosition", chest);
        put(cls, h.instance, "ikOffset", offset);
    }
    Some(h)
}
fn vector_input_event(key: CName, value: V4) -> Option<Handle> {
    let (cls, h) = create("entAnimInputSetterVector")?;
    unsafe {
        put(cls, h.instance, "key", key);
        put(cls, h.instance, "value", value);
    }
    Some(h)
}
fn float_input_event(key: CName, value: f32) -> Option<Handle> {
    let (cls, h) = create("entAnimInputSetterFloat")?;
    unsafe {
        put(cls, h.instance, "key", key);
        put(cls, h.instance, "value", value);
    }
    Some(h)
}
fn feature_input_event(key: CName, feature: Option<&Handle>) -> Option<Handle> {
    let feature = feature.filter(|f| !f.is_null())?;
    let (cls, h) = create("entAnimInputSetterAnimFeature")?;
    unsafe {
        put(cls, h.instance, "key", key);
        put(cls, h.instance, "delay", 0.0f32);
        put(cls, h.instance, "value", feature.clone());
    }
    Some(h)
}

/// QueuePlayerEvent: the player entity's QueueEvent(event).
fn queue_player_event(event: &Handle) -> bool {
    let entity = player_entity();
    if entity == 0 {
        return false;
    }
    let Some(f) = function(entity, "QueueEvent") else {
        return false;
    };
    // SAFETY: QueueEvent(handle:redEvent) on the player.
    unsafe { execute(entity, f, core::ptr::null_mut(), &mut [arg(event)]) }
}
fn set_float_input_direct(key: CName, value: f32) -> i32 {
    let controller = player_animation_controller();
    if controller == 0 {
        return -40;
    }
    let Some(f) = function(controller, "SetInputFloat") else {
        return -41;
    };
    // SAFETY: SetInputFloat(CName, Float) on the controller.
    if unsafe {
        execute(
            controller,
            f,
            core::ptr::null_mut(),
            &mut [arg(&key), arg(&value)],
        )
    } {
        10
    } else {
        -42
    }
}
fn queue_float_input_event(key: CName, value: f32) -> i32 {
    match float_input_event(key, value) {
        None => -43,
        Some(e) => {
            if queue_player_event(&e) {
                11
            } else {
                -44
            }
        }
    }
}
fn queue_vector_input_events(left: V4, right: V4) -> i32 {
    let (Some(l), Some(r)) = (
        vector_input_event(name("ikLeftHandLocalPosition"), left),
        vector_input_event(name("ikRightHandLocalPosition"), right),
    ) else {
        return -20;
    };
    let queued = queue_player_event(&l) as i32 + queue_player_event(&r) as i32;
    if queued == 2 {
        6
    } else {
        -21
    }
}
fn queue_feature(key: CName, feature: Option<&Handle>) -> i32 {
    match feature_input_event(key, feature) {
        None => -22,
        Some(e) => {
            if queue_player_event(&e) {
                7
            } else {
                -23
            }
        }
    }
}
fn apply_feature(controller: usize, key: CName, feature: Option<&Handle>) -> i32 {
    if controller == 0 {
        return -10;
    }
    let Some(f) = function(controller, "ApplyFeature") else {
        return -11;
    };
    if f.params.len() < 2 {
        return -12;
    }
    let Some(feature) = feature.filter(|h| !h.is_null()) else {
        return -13;
    };
    // SAFETY: ApplyFeature(CName, handle:animAnimFeature) on the controller.
    if unsafe {
        execute(
            controller,
            f,
            core::ptr::null_mut(),
            &mut [arg(&key), arg(feature)],
        )
    } {
        2
    } else {
        -14
    }
}

fn anim_test_log(mode: &str, result: i32, l: V4, r: V4) {
    append(
        "vrik_anim_input_test_log.txt",
        &format!(
            "mode={mode} result={result} left=({}, {}, {}, {}) right=({}, {}, {}, {})\n",
            g(l[0]),
            g(l[1]),
            g(l[2]),
            g(l[3]),
            g(r[0]),
            g(r[1]),
            g(r[2]),
            g(r[3])
        ),
    );
}
fn anim_float_test_log(route: &str, key: CName, value: f32, result: i32) {
    append(
        "anim_float_input_test_log.txt",
        &format!(
            "route={route} key={} value={} result={result}\n",
            text(key),
            g(value)
        ),
    );
}
fn root_graph_variable_log(source: &str, key: CName, value: &str, result: i32) {
    append(
        "root_graph_variable_test_log.txt",
        &format!(
            "source={source} key={} value={value} result={result}\n",
            text(key)
        ),
    );
}
fn direct_anim_param_log(source: &str, key: CName, value: f32, result: i32) {
    append(
        "anim_parameter_direct_test_log.txt",
        &format!(
            "source={source} key={} value={} result={result}\n",
            text(key),
            g(value)
        ),
    );
}

/// FillAnimTestPose: the hands from the shared memory (mode 2, XR-local -> game-local as the earlier hand-space
/// conversion did), or an exaggerated pose so a visual response is obvious.
fn anim_test_pose(mode: i32) -> Option<(V4, V4)> {
    if mode == 2 {
        // SAFETY: the shared hands memory.
        unsafe { cvr_ensure_shared_memory() };
        let s = get!(g_pSharedHands);
        if s.is_null() {
            return None;
        }
        let at = |i: usize| unsafe { *s.add(i) };
        return Some(([at(1), -at(3), at(2), 1.0], [at(9), -at(11), at(10), 1.0]));
    }
    Some(([-0.65, 0.75, 0.35, 1.0], [0.65, 0.75, 0.35, 1.0]))
}

/// SetPlayerAnimatedParameterValue: the default value of the player's animated components' parameters of that name.
fn set_player_animated_parameter(key: CName, value: f32) -> i32 {
    let entity = player_entity();
    if entity == 0 {
        return -50;
    }
    let kind = name("entAnimatedComponent");
    let mut updated = 0;
    // SAFETY: the player's components and their parameter arrays.
    for component in unsafe { components(entity) } {
        let c = component[0];
        if c == 0 || type_name(c) != Some(kind) {
            continue;
        }
        let (entries, size) = (
            unsafe { *((c + ANIMATED_PARAMETERS) as *const usize) },
            unsafe { *((c + ANIMATED_PARAMETERS + 0xC) as *const u32) },
        );
        for i in 0..size as usize {
            let p = entries + i * 0x30;
            let (track, parameter) = unsafe { (*(p as *const CName), *((p + 8) as *const CName)) };
            if parameter == key || track == key {
                unsafe { *((p + 0x10) as *mut f32) = value };
                updated += 1;
            }
        }
    }
    updated
}

/// ForceVRNeutralAnimGraphInputs: the weapon and camera stance inputs that visually move the FPP camera, shoulders and
/// hands neutralised (ironsight/ADS inputs untouched: aiming must stay gameplay-correct); the live tracks keep the
/// camera's vertical offset neutral and feet IK on. Applied every frame while VRIK is active.
fn force_neutral_anim_graph_inputs() -> i32 {
    let mut writes = 0;
    let mut add = |r: i32| {
        if r > 0 {
            writes += r;
        }
    };
    add(set_root_graph_float(
        name("disable_maya_engine_right_hand"),
        1.0,
    ));
    add(set_root_graph_float(
        name("disable_maya_engine_left_hand"),
        1.0,
    ));
    add(set_root_graph_float(name("camera_pitch"), 0.0));
    add(set_root_graph_float(name("camera_yaw"), 0.0));
    for v in [
        "weapon_offset_shoulder",
        "weapon_rotation_shoulder",
        "debug_stand_camera_position",
        "debug_crouch_camera_position",
    ] {
        add(set_root_graph_vector(name(v), [0.0; 4]));
    }
    add(set_root_live_track(name("cameraUpOffset"), 0.0, 0));
    add(set_root_live_track(name("allowFeetIk"), 1.0, 0));
    add(set_root_live_track(name("enableLeftFootIk"), 1.0, 0));
    add(set_root_live_track(name("enableRightFootIk"), 1.0, 0));
    writes
}

// ---- the natives

macro_rules! native {
    ($name:ident, |$frame:ident, $out:ident| { $($body:tt)* }) => {
        /// A native of the animation inputs (see the module's documentation).
        ///
        /// # Safety
        /// Called by the script VM.
        #[no_mangle]
        pub unsafe extern "C" fn $name(_context: *mut c_void, $frame: *mut StackFrame, $out: *mut c_void, _a4: i64) {
            #[allow(unused_unsafe, unused_variables)]
            unsafe {
                $($body)*
            }
        }
    };
}
/// # Safety
/// `out` is null or an Int32 result.
unsafe fn out_i32(out: *mut c_void, v: i32) {
    if !out.is_null() {
        unsafe { *(out as *mut i32) = v };
    }
}

native!(SetRootGraphBoolVariableByName, |frame, out| {
    let mut a = args(frame);
    let (n, v) = (a.get::<CName>(), a.get::<bool>());
    a.finish();
    out_i32(out, set_root_graph_bool(n, v));
});
native!(SetRootGraphFloatVariableByName, |frame, out| {
    let mut a = args(frame);
    let (n, v) = (a.get::<CName>(), a.get::<f32>());
    a.finish();
    out_i32(out, set_root_graph_float(n, v));
});
native!(SetRootGraphVectorVariableByName, |frame, out| {
    let mut a = args(frame);
    let (n, v) = (a.get::<CName>(), a.get::<V4>());
    a.finish();
    out_i32(out, set_root_graph_vector(n, v));
});
native!(TestRootGraphFloatPreset, |frame, out| {
    let mut a = args(frame);
    let (mode, v) = (a.get::<i32>(), a.get::<f32>());
    a.finish();
    let Some(n) = root_graph_float_preset(mode).map(name) else {
        return out_i32(out, -75);
    };
    let result = set_root_graph_float(n, v);
    root_graph_variable_log("float_preset", n, &to_string(v), result);
    out_i32(out, result);
});
native!(TestRootGraphVectorPreset, |frame, out| {
    let mut a = args(frame);
    let (mode, v) = (a.get::<i32>(), a.get::<V4>());
    a.finish();
    let Some(n) = root_graph_vector_preset(mode).map(name) else {
        return out_i32(out, -76);
    };
    let result = set_root_graph_vector(n, v);
    let value = format!(
        "{},{},{},{}",
        to_string(v[0]),
        to_string(v[1]),
        to_string(v[2]),
        to_string(v[3])
    );
    root_graph_variable_log("vector_preset", n, &value, result);
    out_i32(out, result);
});
native!(SetRootGraphFloatPresetPersistent, |frame, out| {
    let mut a = args(frame);
    let (mode, v) = (a.get::<i32>(), a.get::<f32>());
    a.finish();
    presets(|p| p.graph_float = (mode, v, 0));
    out_i32(out, 1);
});
native!(SetRootGraphVectorPresetPersistent, |frame, out| {
    let mut a = args(frame);
    let (mode, v) = (a.get::<i32>(), a.get::<V4>());
    a.finish();
    presets(|p| p.graph_vector = (mode, v, 0));
    out_i32(out, 1);
});
native!(GetRootGraphPersistentLastResult, |frame, out| {
    args(frame).finish();
    let r = presets(|p| {
        if p.graph_vector.0 != 0 {
            p.graph_vector.2
        } else {
            p.graph_float.2
        }
    });
    out_i32(out, r);
});
native!(SetVRIKAnimInputTestMode, |frame, _out| {
    let mut a = args(frame);
    let mode = a.get::<i32>();
    a.finish();
    presets(|p| p.test_mode = mode);
});

struct Activation {
    hooks_installed: bool,
    armed: bool,
    last_request: i32,
    last_match_calls: u64,
    stale_frames: i32,
}
static ACTIVATION: Mutex<Activation> = Mutex::new(Activation {
    hooks_installed: false,
    armed: false,
    last_request: 0,
    last_match_calls: 0,
    stale_frames: 0,
});

/// The in-VR overlay's activation: the in-headset menu writes a tracking-request code into shared slot [32] (0 off,
/// 2 position+rotation); CET calls UpdateVRIKAnimInputs every frame on the game thread, so installing the hooks and
/// arming the player here is as safe as the manual "Start VR Tracking" button. Edge-triggered on g_VRBind so the CET
/// button still works independently.
///
/// DESYNC DETECTION instead of a fixed re-arm timer: VRIK_DoArmPlayer walks the animation system's buckets and every
/// metaRig bone name, so it is called only when VRIK falls out of sync -- when the pose-apply hook stops matching the
/// cached track buffer (g_AnimPoseMatchCalls stops counting: weapon draw/holster, save load, area transition, vehicle,
/// scene, ragdoll) for ~10 frames, or when the player's binding changed.
fn overlay_activation() {
    // SAFETY: the shared hands memory and the solve's globals, on the game thread.
    unsafe { cvr_ensure_shared_memory() };
    let shared = get!(g_pSharedHands);
    if shared.is_null() {
        return;
    }
    const STALE_FRAME_THRESHOLD: i32 = 10;
    let request = unsafe { *shared.add(32) } as i32;
    let mut s = ACTIVATION.lock().unwrap_or_else(|e| e.into_inner());
    if request > 0 {
        if !s.hooks_installed {
            // Only the pose-apply hook is needed.
            unsafe { cvr_cpp_install_anim_pose_hook() };
            s.hooks_installed = true;
        }
        let match_calls = get!(g_AnimPoseMatchCalls);
        let mut rearm = !s.armed || unsafe { VRIK_PlayerBindingChanged() };
        if s.armed {
            if match_calls != s.last_match_calls {
                s.stale_frames = 0;
            } else {
                s.stale_frames += 1;
                if s.stale_frames >= STALE_FRAME_THRESHOLD {
                    rearm = true;
                    s.stale_frames = 0;
                }
            }
        }
        if rearm {
            s.armed = unsafe { VRIK_DoArmPlayer() } > 0;
            s.stale_frames = 0;
        }
        s.last_match_calls = match_calls;
        if s.last_request <= 0 {
            set!(g_VRBind, request);
        }
        if get!(g_VRNeutralizeAnimGraph) != 0 {
            force_neutral_anim_graph_inputs();
        }
        // Keep the diag bone snapshot fresh while tracking, so the overlay's "Log VR Diag" works.
        set!(g_VRDiagCapture, 1);
    } else {
        if s.last_request > 0 {
            set!(g_VRBind, 0);
            set!(g_VRDiagCapture, 0);
            unsafe { *shared.add(119) = 0.0 }; // eye-view offset invalid while VRIK is off
                                               // The wheel grab lives inside the mode-4 solve; with tracking off nothing would lower its armed mask.
            unsafe { cvr_anim_wheel_reset() };
        }
        s.armed = false;
        s.stale_frames = 0;
        s.last_match_calls = get!(g_AnimPoseMatchCalls);
    }
    s.last_request = request;
    drop(s);
    // The IK calibration the overlay published, and its one-shot diag requests.
    unsafe { cvr_poll_vr_calib_from_shared() };
}

native!(UpdateVRIKAnimInputs, |frame, out| {
    args(frame).finish();
    overlay_activation();
    let (graph_float, graph_vector, meta_rig, live, param, test_mode) = presets(|p| {
        (
            p.graph_float,
            p.graph_vector,
            p.meta_rig,
            p.live,
            p.param,
            p.test_mode,
        )
    });
    let mut results = (graph_float.2, graph_vector.2, meta_rig.2, live.3, param.2);
    if graph_float.0 != 0 {
        results.0 = root_graph_float_preset(graph_float.0)
            .map_or(-77, |n| set_root_graph_float(name(n), graph_float.1));
    }
    if graph_vector.0 != 0 {
        results.1 = root_graph_vector_preset(graph_vector.0)
            .map_or(-78, |n| set_root_graph_vector(name(n), graph_vector.1));
    }
    if meta_rig.0 != 0 {
        results.2 = root_meta_rig_track_preset(meta_rig.0)
            .map_or(-85, |n| set_root_meta_rig_track(name(n), meta_rig.1));
    }
    if live.0 != 0 {
        results.3 = root_meta_rig_track_preset(live.0)
            .map_or(-93, |n| set_root_live_track(name(n), live.1, live.2));
    }
    if param.0 != 0 {
        results.4 = anim_float_preset(param.0)
            .map_or(-52, |n| set_player_animated_parameter(name(n), param.1));
    }
    presets(|p| {
        p.graph_float.2 = results.0;
        p.graph_vector.2 = results.1;
        p.meta_rig.2 = results.2;
        p.live.3 = results.3;
        p.param.2 = results.4;
    });
    if test_mode == 0 {
        let r = if graph_vector.0 != 0 {
            results.1
        } else if graph_float.0 != 0 {
            results.0
        } else if meta_rig.0 != 0 {
            results.2
        } else if live.0 != 0 {
            results.3
        } else if param.0 != 0 {
            results.4
        } else {
            0
        };
        return out_i32(out, r);
    }
    let Some((left, right)) = anim_test_pose(test_mode) else {
        return out_i32(out, -3);
    };
    if test_mode >= 6 {
        let (mode_name, result) = match test_mode {
            6 => ("queue_vector", queue_vector_input_events(left, right)),
            7 => (
                "queue_feature_WeaponUser",
                queue_feature(
                    name("WeaponUser"),
                    weapon_user_feature(left, right).as_ref(),
                ),
            ),
            8 => (
                "queue_feature_AnimFeature_WeaponUser",
                queue_feature(
                    name("AnimFeature_WeaponUser"),
                    weapon_user_feature(left, right).as_ref(),
                ),
            ),
            9 => (
                "queue_feature_animAnimFeature_WeaponUser",
                queue_feature(
                    name("animAnimFeature_WeaponUser"),
                    weapon_user_feature(left, right).as_ref(),
                ),
            ),
            _ => ("queue_unknown", -99),
        };
        anim_test_log(mode_name, result, left, right);
        return out_i32(out, result);
    }
    let controller = player_animation_controller();
    if controller == 0 {
        return out_i32(out, -1);
    }
    if test_mode >= 3 {
        let feature = match test_mode {
            4 => "AnimFeature_WeaponUser",
            5 => "animAnimFeature_WeaponUser",
            _ => "WeaponUser",
        };
        let result = apply_feature(
            controller,
            name(feature),
            weapon_user_feature(left, right).as_ref(),
        );
        anim_test_log("apply_feature", result, left, right);
        return out_i32(out, result);
    }
    let Some(f) = function(controller, "SetInputVector") else {
        return out_i32(out, -2);
    };
    let (lk, rk) = (
        name("ikLeftHandLocalPosition"),
        name("ikRightHandLocalPosition"),
    );
    execute(
        controller,
        f,
        core::ptr::null_mut(),
        &mut [arg(&lk), arg(&left)],
    );
    execute(
        controller,
        f,
        core::ptr::null_mut(),
        &mut [arg(&rk), arg(&right)],
    );
    anim_test_log("set_input_vector", 1, left, right);
    out_i32(out, 1);
});

native!(TestRootMetaRigTrackPreset, |frame, out| {
    let mut a = args(frame);
    let (mode, v) = (a.get::<i32>(), a.get::<f32>());
    a.finish();
    let Some(n) = root_meta_rig_track_preset(mode).map(name) else {
        return out_i32(out, -84);
    };
    let result = set_root_meta_rig_track(n, v);
    root_meta_rig_log("oneshot", n, v, result);
    out_i32(out, result);
});
native!(SetRootMetaRigTrackPresetPersistent, |frame, out| {
    let mut a = args(frame);
    let (mode, v) = (a.get::<i32>(), a.get::<f32>());
    a.finish();
    presets(|p| p.meta_rig = (mode, v, 0));
    out_i32(out, 1);
});
native!(GetRootMetaRigTrackPersistentLastResult, |frame, out| {
    args(frame).finish();
    out_i32(out, presets(|p| p.meta_rig.2));
});
native!(TestRootLiveTrackPreset, |frame, out| {
    let mut a = args(frame);
    let (mode, v, array_mode) = (a.get::<i32>(), a.get::<f32>(), a.get::<i32>());
    a.finish();
    let Some(n) = root_meta_rig_track_preset(mode).map(name) else {
        return out_i32(out, -84);
    };
    let result = set_root_live_track(n, v, array_mode);
    root_meta_rig_log("live_oneshot", n, v, result);
    out_i32(out, result);
});
native!(SetRootLiveTrackPresetPersistent, |frame, out| {
    let mut a = args(frame);
    let (mode, v, array_mode) = (a.get::<i32>(), a.get::<f32>(), a.get::<i32>());
    a.finish();
    presets(|p| p.live = (mode, v, array_mode, 0));
    out_i32(out, 1);
});
native!(GetRootLiveTrackPersistentLastResult, |frame, out| {
    args(frame).finish();
    out_i32(out, presets(|p| p.live.3));
});
native!(ReadRootLiveTrackPreset, |frame, out| {
    let mut a = args(frame);
    let (mode, array_mode) = (a.get::<i32>(), a.get::<i32>());
    a.finish();
    let Some(n) = root_meta_rig_track_preset(mode).map(name) else {
        return out_i32(out, -84);
    };
    out_i32(out, read_root_live_track(n, array_mode));
});

const ENTITY_TRANSFORM_COMPONENT: usize = 0xB0;
const PLACED_WORLD_POSITION: usize = 0xE0; // entIPlacedComponent::worldTransform.Position (WorldPosition: 3 x 1/2^17 fixed)
const PLACED_LOCAL_POSITION: usize = 0xC0;

/// The player's root world position, as WorldPosition::AsVector4 gives it.
fn player_position() -> Option<V4> {
    let entity = player_entity();
    if entity == 0 {
        return None;
    }
    let transform = unsafe { *((entity + ENTITY_TRANSFORM_COMPONENT) as *const usize) };
    if transform == 0 {
        return None;
    }
    let bits = unsafe { *((transform + PLACED_WORLD_POSITION) as *const [i32; 3]) };
    let f = |b: i32| b as f32 * (1.0 / 131072.0);
    Some([f(bits[0]), f(bits[1]), f(bits[2]), 0.0])
}

native!(RunIKTargetAddTest, |frame, out| {
    let mut a = args(frame);
    let mode = a.get::<i32>();
    a.finish();
    let (label, left_part, right_part) = match mode {
        1 => ("LeftHand_RightHand", "LeftHand", "RightHand"),
        2 => ("l_hand_r_hand", "l_hand", "r_hand"),
        3 => (
            "IK_Pad_Shoulders",
            "IK_Pad_LeftShoulder",
            "IK_Pad_RightShoulder",
        ),
        4 => (
            "LeftShoulder_RightShoulder",
            "LeftShoulder",
            "RightShoulder",
        ),
        _ => return out_i32(out, -30),
    };
    let Some(p) = player_position() else {
        return out_i32(out, -31);
    };
    let player = [p[0], p[1], p[2], p[3]];
    let left = [p[0] - 0.35, p[1] + 0.30, p[2] + 1.15, 1.0];
    let right = [p[0] + 0.35, p[1] + 0.30, p[2] + 1.15, 1.0];
    let provider = |pos: &V4| crate::anim_dumps::static_position_provider(*pos);
    let (lp, rp) = (provider(&left), provider(&right));
    let orientation = create("entStaticOrientationProvider").map(|(cls, h)| {
        put(
            cls,
            h.instance,
            "staticOrientation",
            [0.0f32, 0.0, 0.0, 1.0],
        );
        h
    });
    let v = |q: &V4| format!("({}, {}, {}, {})", g(q[0]), g(q[1]), g(q[2]), g(q[3]));
    let mut log = String::new();
    let _ = writeln!(log, "mode={mode} label={label}");
    let _ = writeln!(log, "playerPos={}", v(&player));
    let _ = writeln!(log, "leftPos={} rightPos={}", v(&left), v(&right));
    if lp.is_null() || rp.is_null() {
        log.push_str("providerCreation=failed\n");
        append("ik_target_add_test_log.txt", &log);
        return out_i32(out, -32);
    }
    let Some(cls) = class("entIKTargetAddEvent") else {
        append("ik_target_add_test_log.txt", &log);
        return out_i32(out, -33);
    };
    let event = |provider: &Handle, part: &str| -> Option<Handle> {
        let instance = cls.create_instance();
        if instance.is_null() {
            return None;
        }
        cls.initialize_properties(instance);
        // entAnimTargetAddEvent / entIKTargetAddEvent fields (SDK offsets, asserted in src/Natives/AnimGraph.cpp).
        let at = |o: usize| (instance as *mut u8).add(o);
        core::ptr::write(at(0x40) as *mut Handle, provider.clone());
        *(at(0x50) as *mut CName) = name(part);
        if let Some(o) = orientation.as_ref() {
            core::ptr::write(at(0x68) as *mut Handle, o.clone());
        }
        let request = at(0x78);
        *(request as *mut f32) = 1.0;
        *(request.add(4) as *mut f32) = 0.0;
        *(request.add(0xC) as *mut f32) = 0.0;
        *(request.add(0x10) as *mut f32) = 0.0;
        *(request.add(0x14) as *mut i32) = 100;
        Some(Handle::from_instance(instance))
    };
    let (Some(le), Some(re)) = (event(&lp, left_part), event(&rp, right_part)) else {
        append("ik_target_add_test_log.txt", &log);
        return out_i32(out, -34);
    };
    let (lq, rq) = (queue_player_event(&le), queue_player_event(&re));
    let reference = |e: &Handle| {
        let at = e.instance as usize + 0x58;
        (*(at as *const i32), *((at + 8) as *const CName))
    };
    let _ = writeln!(log, "leftQueued={} rightQueued={}", lq as i32, rq as i32);
    let (lid, lpart) = reference(&le);
    let (rid, rpart) = reference(&re);
    let _ = writeln!(log, "leftOutRef id={lid} part={}", text(lpart));
    let _ = writeln!(log, "rightOutRef id={rid} part={}", text(rpart));
    crate::anim_dumps::controller_ik_state(&mut log);
    log.push_str("--------------------------------------------------\n");
    append("ik_target_add_test_log.txt", &log);
    out_i32(out, lq as i32 + rq as i32);
});

native!(TestAnimFloatInput, |frame, out| {
    let mut a = args(frame);
    let (key, v, route) = (a.get::<CName>(), a.get::<f32>(), a.get::<i32>());
    a.finish();
    let (route_name, result) = match route {
        0 => ("direct", set_float_input_direct(key, v)),
        1 => ("queue", queue_float_input_event(key, v)),
        _ => ("unknown", -45),
    };
    anim_float_test_log(route_name, key, v, result);
    out_i32(out, result);
});
native!(TestAnimFloatPreset, |frame, out| {
    let mut a = args(frame);
    let (mode, v, route) = (a.get::<i32>(), a.get::<f32>(), a.get::<i32>());
    a.finish();
    let Some(key) = anim_float_preset(mode).map(name) else {
        return out_i32(out, -46);
    };
    let (route_name, result) = match route {
        0 => ("direct_preset", set_float_input_direct(key, v)),
        1 => ("queue_preset", queue_float_input_event(key, v)),
        _ => ("unknown_preset", -47),
    };
    anim_float_test_log(route_name, key, v, result);
    out_i32(out, result);
});
native!(SetPlayerAnimParameter, |frame, out| {
    let mut a = args(frame);
    let (key, v) = (a.get::<CName>(), a.get::<f32>());
    a.finish();
    let result = set_player_animated_parameter(key, v);
    direct_anim_param_log("oneshot", key, v, result);
    out_i32(out, result);
});
native!(SetPlayerAnimParameterPreset, |frame, out| {
    let mut a = args(frame);
    let (mode, v) = (a.get::<i32>(), a.get::<f32>());
    a.finish();
    let Some(key) = anim_float_preset(mode).map(name) else {
        return out_i32(out, -51);
    };
    let result = set_player_animated_parameter(key, v);
    direct_anim_param_log("oneshot_preset", key, v, result);
    out_i32(out, result);
});
native!(SetPlayerAnimParameterPersistentPreset, |frame, out| {
    let mut a = args(frame);
    let (mode, v) = (a.get::<i32>(), a.get::<f32>());
    a.finish();
    presets(|p| p.param = (mode, v, 0));
    out_i32(out, 1);
});
native!(GetPlayerAnimParameterPersistentLastResult, |frame, out| {
    args(frame).finish();
    out_i32(out, presets(|p| p.param.2));
});

// RestoreVRFppArms: the player's FPP arm components (by name) back on, with every chunk, unit scale and no local
// offset; the chunk debug reset so no later call hides them again. ForceHideVRFppArms restores too (the hide path is
// disabled in this build).
const COMPONENT_ENABLED: usize = 0x8B;
const SKINNED_CHUNK_MASK: usize = 0x248;
const MESH_CHUNK_MASK: usize = 0x198;
const MESH_VISUAL_SCALE: usize = 0x178;

fn restore_fpp_arms() -> i32 {
    let entity = player_entity();
    if entity == 0 {
        return -1;
    }
    let mut restored = 0;
    // SAFETY: the player's components and their fields.
    for component in unsafe { components(entity) } {
        let c = component[0];
        if c == 0 {
            continue;
        }
        let component_name = unsafe { *((c + COMPONENT_NAME) as *const CName) };
        let n = CString::new(text(component_name)).unwrap_or_default();
        if !unsafe { crate::hand_natives::cvr_is_likely_fpp_arm_component(n.as_ptr()) } {
            continue;
        }
        let Some(ty) = (unsafe { object_class(c as *const c_void) }) else {
            continue;
        };
        if !ty.is_a(name("entIPlacedComponent")) {
            continue;
        }
        unsafe {
            *((c + COMPONENT_ENABLED) as *mut bool) = true;
            if ty.is_a(name("entSkinnedMeshComponent"))
                || ty.is_a(name("entGarmentSkinnedMeshComponent"))
            {
                *((c + SKINNED_CHUNK_MASK) as *mut u64) = u64::MAX;
            }
            if ty.is_a(name("entMeshComponent")) {
                *((c + MESH_CHUNK_MASK) as *mut u64) = u64::MAX;
                *((c + MESH_VISUAL_SCALE) as *mut [f32; 3]) = [1.0; 3];
            }
            *((c + PLACED_LOCAL_POSITION) as *mut [i32; 3]) = [0; 3];
        }
        restored += 1;
    }
    set!(g_chunkDebugEnabled, false);
    set!(g_chunkDebugWasEnabled, false);
    restored
}
native!(RestoreVRFppArms, |frame, out| {
    args(frame).finish();
    out_i32(out, restore_fpp_arms());
});
native!(ForceHideVRFppArms, |frame, out| {
    args(frame).finish();
    out_i32(out, restore_fpp_arms());
});

native!(TestWeaponUserFeatureRoute, |frame, out| {
    let mut a = args(frame);
    let route = a.get::<i32>();
    a.finish();
    let (left, right) = anim_test_pose(1).unwrap_or_default();
    let key = name("weapon_user");
    let (mode_name, result) = match route {
        0 => (
            "weapon_user_apply",
            apply_feature(
                player_animation_controller(),
                key,
                weapon_user_feature(left, right).as_ref(),
            ),
        ),
        1 => (
            "weapon_user_queue",
            queue_feature(key, weapon_user_feature(left, right).as_ref()),
        ),
        _ => ("weapon_user_unknown", -60),
    };
    anim_test_log(mode_name, result, left, right);
    out_i32(out, result);
});
native!(TestIKFeatureRoute, |frame, out| {
    let mut a = args(frame);
    let (mode, route) = (a.get::<i32>(), a.get::<i32>());
    a.finish();
    let (feature_name, log_name) = match (mode, route == 0) {
        (1, true) => ("playerIK", "playerIK_apply"),
        (1, false) => ("playerIK", "playerIK_queue"),
        (2, true) => ("interactionIK", "interactionIK_apply"),
        (2, false) => ("interactionIK", "interactionIK_queue"),
        _ => return out_i32(out, -61),
    };
    let Some(p) = player_position() else {
        return out_i32(out, -62);
    };
    let point = [p[0] + 0.45, p[1] + 0.35, p[2] + 1.15, 1.0];
    let normal = [0.0, 0.0, 1.0, 0.0];
    let feature = ik_feature(point, normal, 1.0);
    let key = name(feature_name);
    let result = match route {
        0 => apply_feature(player_animation_controller(), key, feature.as_ref()),
        1 => queue_feature(key, feature.as_ref()),
        _ => -63,
    };
    anim_test_log(log_name, result, point, normal);
    out_i32(out, result);
});
native!(TestMeleeIKDataFeatureRoute, |frame, out| {
    let mut a = args(frame);
    let route = a.get::<i32>();
    a.finish();
    let Some(base) = player_position() else {
        return out_i32(out, -64);
    };
    let head = [base[0], base[1], base[2] + 1.55, 1.0];
    let chest = [base[0], base[1], base[2] + 1.15, 1.0];
    let offset = [0.55, 0.35, 0.15, 0.0];
    let feature = melee_ik_data_feature(head, chest, offset);
    let key = name("MeleeIKData");
    let log_name = if route == 0 {
        "MeleeIKData_apply"
    } else {
        "MeleeIKData_queue"
    };
    let result = match route {
        0 => apply_feature(player_animation_controller(), key, feature.as_ref()),
        1 => queue_feature(key, feature.as_ref()),
        _ => -65,
    };
    anim_test_log(log_name, result, head, offset);
    out_i32(out, result);
});
