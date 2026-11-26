//! The animation dumps (formerly src/Natives/AnimDumps.cpp's natives and helpers and AnimGraph.cpp's
//! DumpRootGraphVariables and DumpAnimMemory): what the engine's animation objects actually contain, written to the
//! plugin's log folder -- which graph variables exist and what they are named, which meta-rig tracks a rig carries,
//! what a controller's listeners are, which properties an anim class really has as against what the .reds dump
//! claims. THE .reds DUMP IS INCOMPLETE (functions that exist and are callable are missing from it), so "does this API
//! exist" is answered by the live RTTI. Each is hundreds of lines read afterwards, side by side: files, not the log.
use core::ffi::c_void;
use std::fmt::Write as _;

use red4ext_abi::cname::CName;
use red4ext_abi::fnv::fnv1a64;
use red4ext_abi::function::{BaseFunction, Property};
use red4ext_abi::handle::Handle;
use red4ext_abi::rtti::{object_class, Class, IType, RttiSystem};
use red4ext_abi::stack::StackFrame;

use crate::anim_lookup::{
    classify_qword, components, find_world_animation_system, player_animated_object, player_entity,
    safe,
};
use crate::native_args::args;

extern "C" {
    /// LoadResourceRef(animated->graph) and LoadResourceRef(animated->rig) (src/Natives/AnimGraph.cpp).
    fn cvr_cpp_load_graph(animated_component: *mut c_void) -> *mut c_void;
    fn cvr_cpp_load_rig(animated_component: *mut c_void) -> *mut c_void;
}

fn name(text: &str) -> CName {
    CName(fnv1a64(text.as_bytes()))
}
fn class(text: &str) -> Option<&'static Class> {
    RttiSystem::get()?.get_class(name(text))
}
/// CName::ToString as an ostream prints it ("" for a name the pool does not have).
fn t(n: CName) -> &'static str {
    n.text()
}
/// std::ostream's float output (%g, six digits).
fn g(v: f32) -> String {
    cvr_crt::format(c"%g", &[cvr_crt::Arg::F(v as f64)])
        .to_string_lossy()
        .into_owned()
}
fn at<T: Copy>(object: usize, offset: usize) -> T {
    // SAFETY: a live engine object's field of type T (the SDK's offsets).
    unsafe { core::ptr::read_unaligned((object + offset) as *const T) }
}
/// A DynArray's entries and size.
fn array(object: usize, offset: usize) -> (usize, u32) {
    (at::<usize>(object, offset), at::<u32>(object, offset + 0xC))
}
fn names(object: usize, offset: usize) -> Vec<CName> {
    let (entries, size) = array(object, offset);
    (0..size as usize)
        .map(|i| at::<CName>(entries, i * 8))
        .collect()
}
fn type_name(object: usize) -> Option<CName> {
    // SAFETY: a live scriptable.
    unsafe { object_class(object as *const c_void) }.map(Class::class_name)
}
/// A span the scans read is committed and readable (VirtualQuery). The C++ dumps read such spans under __try, but
/// REDEngine's vectored handler takes an access violation before any __try does: a qword in the heap's range that is
/// not mapped took the game down (2026-11-09, DumpAnimMemory at 0x14800000100). The dumps skip what they cannot read.
fn readable(address: usize, len: usize) -> bool {
    cvr_engine::code::readable_range(address, len)
}
fn write_file(file: &str, text: &str) -> bool {
    std::fs::write(
        cvr_host::paths::file(cvr_host::paths::Folder::Log, file),
        text,
    )
    .is_ok()
}

/// GetTypeNameForDump: a type's computed name, or its native name, "<unnamed>" or "<null>".
fn type_text(ty: *const IType) -> String {
    // SAFETY: a registered type or null.
    let Some(ty) = (unsafe { ty.as_ref() }) else {
        return "<null>".into();
    };
    let computed = ty.computed_name().text();
    if !computed.is_empty() {
        return computed.to_owned();
    }
    let native = ty.name().text();
    if native.is_empty() {
        "<unnamed>".into()
    } else {
        native.to_owned()
    }
}

/// IsInterestingAnimName: arms, hands, IK, weapons, zoom, render, visibility, camera, left and right.
fn interesting(n: &str) -> bool {
    let l = n.to_ascii_lowercase();
    [
        "arm",
        "hand",
        "ik",
        "weapon",
        "zoom",
        "render",
        "visibility",
        "camera",
        "left",
        "right",
    ]
    .iter()
    .any(|k| l.contains(k))
}
fn interesting_list(n: &str) -> bool {
    let l = n.to_ascii_lowercase();
    [
        "arm",
        "hand",
        "ik",
        "weapon",
        "zoom",
        "render",
        "visibility",
    ]
    .iter()
    .any(|k| l.contains(k))
}

// ---- RTTI

const PROPERTY_IS_OUT: u64 = 1 << 9;
const PROPERTY_IS_OPTIONAL: u64 = 1 << 10;
const PROPERTY_IN_VALUE_HOLDER: u64 = 1 << 0x15;
const PROPERTY_IS_HANDLE: u64 = 1 << 0x1B;
const CLASS_LISTENERS: usize = 0x1B0; // CClass: DynArray<CClass::Listener> (0x28 each)
const CLASS_EVENT_TYPE_ID: usize = 0x2C0; // int16

fn function_details(out: &mut String, f: Option<&BaseFunction>) {
    let Some(f) = f else { return };
    let _ = writeln!(out, "{}", t(f.full_name));
    let _ = writeln!(out, "  shortName={}", t(f.short_name));
    // SAFETY: the function's own properties.
    let ret = unsafe { f.return_type.as_ref() }.map_or("Void".to_owned(), |r| type_text(r.ty));
    let _ = writeln!(out, "  return={ret}");
    let params = f.params.as_slice();
    let _ = writeln!(out, "  params({})", params.len());
    for (i, &p) in params.iter().enumerate() {
        let p = unsafe { p.as_ref() };
        let _ = writeln!(
            out,
            "    [{i}] name={} type={} out={} optional={}",
            p.map_or("<null>", |p| t(p.name)),
            p.map_or("<null>".to_owned(), |p| type_text(p.ty)),
            p.is_some_and(|p| p.flags & PROPERTY_IS_OUT != 0) as i32,
            p.is_some_and(|p| p.flags & PROPERTY_IS_OPTIONAL != 0) as i32
        );
    }
    out.push('\n');
}

fn class_properties(out: &mut String, class_name: &str) {
    out.push_str("==================================================\n");
    let _ = writeln!(out, "CLASS {class_name}");
    out.push_str("==================================================\n");
    let Some(cls) = class(class_name) else {
        out.push_str("<missing>\n\n");
        return;
    };
    let props = cls.properties();
    // SAFETY: a class is a type.
    let ty = unsafe { &*(cls as *const Class as *const IType) };
    let _ = writeln!(
        out,
        "size={} alignment={} propCount={}",
        ty.size(),
        ty.alignment(),
        props.len()
    );
    for (i, p) in props.iter().enumerate() {
        let _ = writeln!(
            out,
            "  [{i}] name={} type={} offset=0x{:x} inHolder={} handle={}",
            t(p.name),
            type_text(p.ty),
            p.value_offset,
            (p.flags & PROPERTY_IN_VALUE_HOLDER != 0) as i32,
            (p.flags & PROPERTY_IS_HANDLE != 0) as i32
        );
    }
    out.push('\n');
}

/// FindFirstPropertyByType: the first property at or past `min_offset` whose type's name has `ty` in it.
fn first_property_by_type(cls: &Class, ty: &str, min_offset: u32) -> Option<&'static Property> {
    let wanted = ty.to_ascii_lowercase();
    cls.properties().into_iter().find(|p| {
        p.value_offset >= min_offset && type_text(p.ty).to_ascii_lowercase().contains(&wanted)
    })
}

/// CreateStaticPositionProvider: an entStaticPositionProvider at a position (its position is the first Vector4, or
/// WorldPosition, property past the base class; with neither there is none).
pub(crate) fn static_position_provider(position: [f32; 4]) -> Handle {
    let Some(cls) = class("entStaticPositionProvider") else {
        return Handle::null();
    };
    let vector = first_property_by_type(cls, "Vector4", 0x50);
    let world = first_property_by_type(cls, "WorldPosition", 0x50);
    if vector.is_none() && world.is_none() {
        return Handle::null();
    }
    let instance = cls.create_instance();
    if instance.is_null() {
        return Handle::null();
    }
    // SAFETY: the new instance of this class and its properties.
    unsafe {
        cls.initialize_properties(instance);
        if let Some(p) = vector {
            p.set_value(instance, &position);
        } else if let Some(p) = world {
            // WorldPosition(Vector4): 1/2^17 fixed point.
            let fixed = |v: f32| (v * 131072.0) as i32;
            p.set_value(
                instance,
                &[fixed(position[0]), fixed(position[1]), fixed(position[2])],
            );
        }
        Handle::from_instance(instance)
    }
}

// ---- the controller's IK state

const CONTROLLER_IK_TARGETS: usize = 0x1A8 + 0x10; // ikTargetController.targetData: DynArray<IKTargetData> (0x30 each)
const CONTROLLER_IK_PARAMS: usize = 0x1A8 + 0x20;
const CONTROLLER_BINDING: usize = 0x1E0;
const COMPONENT_NAME: usize = 0x40;
const COMPONENT_ENABLED: usize = 0x8B;

/// AppendPlayerControllerIKState: the player's controllers' IK targets.
pub(crate) fn controller_ik_state(out: &mut String) {
    let entity = player_entity();
    if entity == 0 {
        out.push_str("playerEntity=<null>\n");
        return;
    }
    let kind = name("entAnimationControllerComponent");
    // SAFETY: the player's components.
    for component in unsafe { components(entity) } {
        let c = component[0];
        if c == 0 || type_name(c) != Some(kind) {
            continue;
        }
        let (targets, count) = array(c, CONTROLLER_IK_TARGETS);
        let _ = writeln!(
            out,
            "controller name={} ptr=0x{c:x} targetData.size={count} ikParams=0x{:x}",
            t(at(c, COMPONENT_NAME)),
            at::<usize>(c, CONTROLLER_IK_PARAMS)
        );
        for i in 0..count as usize {
            let target = targets + i * 0x30;
            let _ = writeln!(
                out,
                "  [{i}] id={} part={} posProvider=0x{:x} orientProvider=0x{:x}",
                at::<i32>(target, 0),
                t(at(target, 8)),
                at::<usize>(target, 0x10),
                at::<usize>(target, 0x20)
            );
        }
    }
}

// ---- graphs, meta-rigs, entities

const GRAPH_VARIABLES: usize = 0x58;

/// DumpAnimGraphVariables: the interesting variables of a graph, by kind.
fn graph_variables(out: &mut String, graph: usize, prefix: &str) {
    let vars = if graph == 0 {
        0
    } else {
        at::<usize>(graph, GRAPH_VARIABLES)
    };
    if vars == 0 {
        let _ = writeln!(out, "{prefix}graphVariables=<null>");
        return;
    }
    let kinds = [0x38usize, 0x48, 0x58, 0x68, 0x78, 0x88].map(|o| array(vars, o));
    let _ = writeln!(
        out,
        "{prefix}graphVariables bool={} int={} float={} vector={} quat={} transform={}",
        kinds[0].1, kinds[1].1, kinds[2].1, kinds[3].1, kinds[4].1, kinds[5].1
    );
    let each = |(entries, size): (usize, u32), f: &mut dyn FnMut(usize, usize, &str)| {
        for i in 0..size as usize {
            let v = at::<usize>(entries, i * 16);
            if v == 0 {
                continue;
            }
            let n = t(at(v, 0x30));
            if n.is_empty() || !interesting(n) {
                continue;
            }
            f(i, v, n);
        }
    };
    each(kinds[0], &mut |i, v, n| {
        let _ = writeln!(
            out,
            "{prefix}bool[{i}] name={n} value={} default={}",
            (at::<u8>(v, 0x38) != 0) as i32,
            (at::<u8>(v, 0x39) != 0) as i32
        );
    });
    each(kinds[1], &mut |i, v, n| {
        let _ = writeln!(
            out,
            "{prefix}int[{i}] name={n} value={} default={} min={} max={}",
            at::<i32>(v, 0x38),
            at::<i32>(v, 0x3C),
            at::<i32>(v, 0x40),
            at::<i32>(v, 0x44)
        );
    });
    each(kinds[2], &mut |i, v, n| {
        let _ = writeln!(
            out,
            "{prefix}float[{i}] name={n} value={} default={} min={} max={}",
            g(at(v, 0x38)),
            g(at(v, 0x3C)),
            g(at(v, 0x40)),
            g(at(v, 0x44))
        );
    });
    each(kinds[3], &mut |i, v, n| {
        let value: [f32; 4] = at(v, 0x38);
        let default: [f32; 4] = at(v, 0x50);
        let _ = writeln!(
            out,
            "{prefix}vector[{i}] name={n} value=({}, {}, {}, {}) default=({}, {}, {}, {})",
            g(value[0]),
            g(value[1]),
            g(value[2]),
            g(value[3]),
            g(default[0]),
            g(default[1]),
            g(default[2]),
            g(default[3])
        );
    });
    each(kinds[4], &mut |i, v, n| {
        let _ = writeln!(
            out,
            "{prefix}quat[{i}] name={n} value=(roll={}, pitch={}, yaw={})",
            g(at(v, 0x38)),
            g(at(v, 0x3C)),
            g(at(v, 0x40))
        );
    });
    each(kinds[5], &mut |i, _, n| {
        let _ = writeln!(out, "{prefix}transform[{i}] name={n}");
    });
}

const META_RIG_BONE_NAMES: usize = 0x20;
const META_RIG_REFERENCE_TRACKS: usize = 0x40;
const META_RIG_TRACK_NAMES: usize = 0x50;
const META_RIG_HASH: usize = 0x60;

/// DumpMetaRigTracks: the interesting tracks of a meta-rig with their reference values.
fn meta_rig_tracks(out: &mut String, meta_rig: usize, prefix: &str) {
    if meta_rig == 0 {
        let _ = writeln!(out, "{prefix}metaRig=<null>");
        return;
    }
    let track_names = names(meta_rig, META_RIG_TRACK_NAMES);
    let (tracks, track_count) = array(meta_rig, META_RIG_REFERENCE_TRACKS);
    let _ = writeln!(
        out,
        "{prefix}metaRig hash=0x{:x} boneCount={} trackCount={} refTrackCount={track_count}",
        at::<u64>(meta_rig, META_RIG_HASH),
        array(meta_rig, META_RIG_BONE_NAMES).1,
        track_names.len()
    );
    for i in 0..track_names.len().min(track_count as usize) {
        let n = t(track_names[i]);
        if n.is_empty() || !interesting(n) {
            continue;
        }
        let _ = writeln!(
            out,
            "{prefix}track[{i}] name={n} value={}",
            g(at(tracks, i * 4))
        );
    }
}

fn name_list_filtered(out: &mut String, label: &str, list: &[CName]) {
    let _ = writeln!(out, "{label} count={}", list.len());
    for n in list {
        let n = t(*n);
        if !n.is_empty() && interesting_list(n) {
            let _ = writeln!(out, "  - {n}");
        }
    }
}

const BINDING_ENABLED: usize = 0x30;
const BINDING_NAME: usize = 0x68;

fn binding_info(out: &mut String, binding: usize, prefix: &str) {
    if binding == 0 {
        let _ = writeln!(out, "{prefix}binding=<null>");
        return;
    }
    let _ = writeln!(
        out,
        "{prefix}binding ptr=0x{binding:x} type={} enabled={} bindName={}",
        type_name(binding).map_or("<null>", t),
        (at::<u8>(binding, BINDING_ENABLED) != 0) as i32,
        t(at(binding, BINDING_NAME))
    );
}

const ANIMATED_RIG: usize = 0x138; // Ref<animRig>: its path first
const ANIMATED_GRAPH: usize = 0x150;
const ANIMATED_SETUP_CINEMATICS: usize = 0x190;
const ANIMATED_SETUP_GAMEPLAY: usize = 0x1A0;
const ANIMATED_PARAMETERS: usize = 0x278;
const ANIMATED_BINDING: usize = 0x288;

fn track_parameters(out: &mut String, animated: usize, prefix: &str) {
    let (entries, size) = array(animated, ANIMATED_PARAMETERS);
    for i in 0..size as usize {
        let p = entries + i * 0x30;
        let _ = writeln!(
            out,
            "{prefix}param[{i}] track={} name={} default={}",
            t(at(p, 0)),
            t(at(p, 8)),
            g(at(p, 0x10))
        );
    }
}

fn animated_resource_details(out: &mut String, animated: usize, prefix: &str) {
    // SAFETY: the component's graph resource, loaded as the C++ loaded it.
    let graph = unsafe { cvr_cpp_load_graph(animated as *mut c_void) } as usize;
    if graph != 0 {
        let (features, feature_count) = array(graph, 0xD8);
        let (databases, database_count) = array(graph, 0xF8);
        let _ = writeln!(
            out,
            "{prefix}graphLoaded=1 animFeatures={feature_count} additionalAnimDatabases={database_count} useAnimCommands={} hasMixerSlot={}",
            (at::<u8>(graph, 0xA0) != 0) as i32,
            (at::<u8>(graph, 0xEE) != 0) as i32
        );
        graph_variables(out, graph, prefix);
        for i in 0..feature_count as usize {
            let f = features + i * 0x18;
            let n = |o: usize| {
                let s = t(at(f, o));
                if s.is_empty() {
                    "<null>"
                } else {
                    s
                }
            };
            let _ = writeln!(
                out,
                "{prefix}feature[{i}] name={} class={} forceAllocate={}",
                n(0),
                n(8),
                (at::<u8>(f, 0x10) != 0) as i32
            );
        }
        for i in 0..database_count as usize {
            let d = databases + i * 0x38;
            let _ = writeln!(
                out,
                "{prefix}animDb[{i}] name={} dbHash=0x{:x} overrideHash=0x{:x}",
                t(at(d, 0)),
                at::<u64>(d, 0x08),
                at::<u64>(d, 0x20)
            );
        }
    } else {
        let _ = writeln!(out, "{prefix}graphLoaded=0");
    }
    // SAFETY: the component's rig resource.
    let rig = unsafe { cvr_cpp_load_rig(animated as *mut c_void) } as usize;
    if rig != 0 {
        let bones = names(rig, 0x50);
        let tracks = names(rig, 0xE8);
        let _ = writeln!(
            out,
            "{prefix}rigLoaded=1 boneCount={} trackCount={} ikSetups={}",
            bones.len(),
            tracks.len(),
            array(rig, 0x150).1
        );
        name_list_filtered(out, "    rig.trackNames", &tracks);
        name_list_filtered(out, "    rig.boneNames", &bones);
    } else {
        let _ = writeln!(out, "{prefix}rigLoaded=0");
    }
    for (label, offset, list_label) in [
        (
            "gameplay",
            ANIMATED_SETUP_GAMEPLAY,
            "      gameplay.variables",
        ),
        (
            "cinematics",
            ANIMATED_SETUP_CINEMATICS,
            "      cinematic.variables",
        ),
    ] {
        let (entries, size) = array(animated, offset);
        let _ = writeln!(out, "{prefix}setup.{label} count={size}");
        for i in 0..size as usize {
            let e = entries + i * 0x20;
            let vars = names(e, 0x08);
            let _ = writeln!(
                out,
                "{prefix}{label}[{i}] animSetHash=0x{:x} priority={} varCount={}",
                at::<u64>(e, 0),
                at::<u8>(e, 0x18) as i32,
                vars.len()
            );
            name_list_filtered(out, list_label, &vars);
        }
    }
}

const ENTITY_APPEARANCE: usize = 0x50;
const ENTITY_TEMPLATE: usize = 0x60;
const ENTITY_STATUS: usize = 0x156;

/// DumpEntityAnimationInfo: an entity's controllers and animated components, their bindings, parameters and
/// resources.
fn entity_animation_info(out: &mut String, entity: usize, reason: &str) {
    if entity == 0 {
        return;
    }
    let _ = writeln!(
        out,
        "ENTITY reason={reason} ptr=0x{entity:x} appearance={} templateHash=0x{:x} status={}",
        t(at(entity, ENTITY_APPEARANCE)),
        at::<u64>(entity, ENTITY_TEMPLATE),
        at::<u8>(entity, ENTITY_STATUS) as i32
    );
    let (controller, animated) = (
        name("entAnimationControllerComponent"),
        name("entAnimatedComponent"),
    );
    // SAFETY: the entity's components.
    for component in unsafe { components(entity) } {
        let c = component[0];
        let Some(ty) = (if c == 0 { None } else { type_name(c) }) else {
            continue;
        };
        if ty != controller && ty != animated {
            continue;
        }
        let component_name = t(at(c, COMPONENT_NAME));
        let arm = crate::hand_natives::likely_fpp_arm(component_name.as_bytes());
        let _ = writeln!(
            out,
            "  component name={component_name} type={} ptr=0x{c:x} enabled={}{}",
            t(ty),
            (at::<u8>(c, COMPONENT_ENABLED) != 0) as i32,
            if arm { " [LIKELY_FPP_ARM]" } else { "" }
        );
        if ty == controller {
            let binding = at::<usize>(c, CONTROLLER_BINDING);
            let _ = writeln!(
                out,
                "    controlBinding=0x{binding:x} ikTargets={} ikParams=0x{:x}",
                array(c, CONTROLLER_IK_TARGETS).1,
                at::<usize>(c, CONTROLLER_IK_PARAMS)
            );
            binding_info(out, binding, "    ");
        } else {
            let binding = at::<usize>(c, ANIMATED_BINDING);
            let _ = writeln!(
                out,
                "    rigHash=0x{:x} graphHash=0x{:x} controlBinding=0x{binding:x} animParams={}",
                at::<u64>(c, ANIMATED_RIG),
                at::<u64>(c, ANIMATED_GRAPH),
                array(c, ANIMATED_PARAMETERS).1
            );
            binding_info(out, binding, "    ");
            track_parameters(out, c, "    ");
            animated_resource_details(out, c, "    ");
        }
    }
    out.push('\n');
}

// ---- the natives

macro_rules! dump {
    ($name:ident, $file:literal, $fail:expr, |$out:ident| $body:block) => {
        /// A dump native (see the module's documentation): its file in the plugin's log folder.
        ///
        /// # Safety
        /// Called by the script VM.
        #[no_mangle]
        pub unsafe extern "C" fn $name(
            _context: *mut c_void,
            frame: *mut StackFrame,
            result: *mut c_void,
            _a4: i64,
        ) {
            unsafe { args(frame) }.finish();
            let mut text = String::new();
            let mut code: i32 = {
                let $out: &mut String = &mut text;
                (|| -> i32 { $body })()
            };
            if !write_file($file, &text) {
                code = $fail;
            }
            if !result.is_null() {
                unsafe { *(result as *mut i32) = code };
            }
        }
    };
}

dump!(
    DumpAnimControllerListeners,
    "anim_controller_listeners.txt",
    -1,
    |out| {
        let Some(controller) = class("entAnimationControllerComponent") else {
            return -2;
        };
        let (listeners, count) = array(controller as *const Class as usize, CLASS_LISTENERS);
        out.push_str("CLASS entAnimationControllerComponent\n");
        let _ = writeln!(out, "listenerCount={count}\n");
        out.push_str("Known event type ids:\n");
        for event in [
            "redEvent",
            "entAnimInputSetter",
            "entAnimInputSetterVector",
            "entAnimInputSetterAnimFeature",
            "entIKTargetAddEvent",
        ] {
            let id = class(event).map_or(-1, |c| {
                at::<i16>(c as *const Class as usize, CLASS_EVENT_TYPE_ID) as i32
            });
            let _ = writeln!(out, "  {event} -> eventTypeId={id}");
        }
        out.push_str("\nListeners:\n");
        for i in 0..count as usize {
            let l = listeners + i * 0x28;
            let _ = writeln!(
                out,
                "  [{i}] callback={} eventTypeId={} scripted={}",
                t(at(l, 0x18)),
                at::<i16>(l, 0x20),
                (at::<u8>(l, 0x22) != 0) as i32
            );
        }
        count as i32
    }
);

dump!(
    DumpAnimControllerFunctionDetails,
    "anim_controller_function_details.txt",
    -1,
    |out| {
        out.push_str("CLASS entAnimationControllerComponent\n\n");
        if let Some(c) = class("entAnimationControllerComponent") {
            for f in [
                "ApplyFeature",
                "PushEvent",
                "SetInputVector",
                "SetInputFloat",
                "SetInputBool",
                "SetInputQuaternion",
                "OnSetInputVectorEvent",
            ] {
                function_details(out, c.function(name(f)));
            }
        }
        out.push_str("CLASS entEntity\n\n");
        if let Some(c) = class("entEntity") {
            for f in ["QueueEvent", "QueueEventForNodeID", "QueueEventForEntityID"] {
                function_details(out, c.function(name(f)));
            }
        }
        1
    }
);

dump!(
    DumpInterestingAnimClassProperties,
    "anim_interesting_class_properties.txt",
    -1,
    |out| {
        for c in [
            "entIBinding",
            "entAnimationControlBinding",
            "entStaticPositionProvider",
            "entStaticOrientationProvider",
            "entIKTargetAddEvent",
            "entIKTargetRemoveEvent",
            "entAnimInputSetterVector",
            "entAnimInputSetterAnimFeature",
            "entAnimatedComponent",
            "entAnimationControllerComponent",
        ] {
            class_properties(out, c);
        }
        1
    }
);

dump!(
    DumpAnimationSystemCandidates,
    "animation_system_candidates.txt",
    -1,
    |out| {
        let entity = player_entity();
        if entity == 0 {
            return -2;
        }
        entity_animation_info(out, entity, "player_entity");
        1
    }
);

dump!(
    DumpPlayerAnimatedObjectRuntime,
    "player_animated_object_runtime.txt",
    -79,
    |out| {
        let entity = player_entity();
        let objects = crate::anim_lookup::engine_objects();
        let system = if entity == 0 {
            0
        } else {
            find_world_animation_system(safe::<usize>(entity + 0xB8), objects[3], Some(out))
        };
        if entity == 0 || system == 0 {
            return -80;
        }
        let l = crate::anim_lookup::layout();
        let (buckets, stride, count) = (l.buckets, l.bucket_stride, l.bucket_count);
        let (objects_at, components_at, entities_at, entities_size) =
            (l.objects, l.components, l.entities, l.entities_size);
        let mut matches = 0;
        for b in 0..count as usize {
            let bucket = system + buckets as usize + b * stride as usize;
            let size = at::<u32>(bucket, entities_size as usize);
            for i in 0..size as usize {
                if at::<usize>(bucket, entities_at as usize + i * 16) != entity {
                    continue;
                }
                let component = at::<usize>(bucket, components_at as usize + i * 8);
                let object = at::<usize>(bucket, objects_at as usize + i * 8);
                let _ = write!(out, "bucket={b} entry={i} animatedObject=0x{object:x} animatedComponent=0x{component:x}");
                if component != 0 {
                    let _ = write!(out, " componentName={}", t(at(component, COMPONENT_NAME)));
                }
                out.push('\n');
                if object != 0 {
                    let _ = writeln!(
                    out,
                    "  metaRigID={} metaRig=0x{:x} metaRigInfo=0x{:x} distance={} cameraLevel={} lastLevel={}",
                    at::<u64>(object, 0),
                    at::<usize>(object, 8),
                    at::<usize>(object, 0x10),
                    g(at(object, 0xE4)),
                    at::<u32>(object, 0x98),
                    at::<u32>(object, 0xA4)
                );
                    meta_rig_tracks(out, at::<usize>(object, 8), "  ");
                }
                let bindings_size = at::<u32>(bucket, l.bindings_size as usize);
                if (i as u32) < bindings_size {
                    let (entries, n) =
                        array(bucket, l.bindings as usize + i * l.binding_stride as usize);
                    let _ = writeln!(out, "  componentBindings={n}");
                    for k in 0..n as usize {
                        let e = entries + k * 0x28;
                        let anim = at::<usize>(e, 0x10);
                        let _ = write!(
                            out,
                            "    [{k}] placed=0x{:x} animComp=0x{anim:x} attachment=0x{:x}",
                            at::<usize>(e, 0),
                            at::<usize>(e, 0x18)
                        );
                        if anim != 0 {
                            let _ = write!(out, " animCompName={}", t(at(anim, COMPONENT_NAME)));
                        }
                        out.push('\n');
                    }
                }
                out.push('\n');
                matches += 1;
            }
        }
        matches
    }
);

dump!(
    DumpRootMetaRigTracks,
    "root_metarig_tracks.txt",
    -82,
    |out| {
        let object = player_animated_object(c"root");
        let meta_rig = if object == 0 {
            0
        } else {
            at::<usize>(object, 8)
        };
        if meta_rig == 0 {
            return -83;
        }
        meta_rig_tracks(out, meta_rig, "");
        1
    }
);

dump!(
    DumpRootAnimatedObjectFloatArrayCandidates,
    "root_animated_object_float_candidates.txt",
    -87,
    |out| {
        let object = player_animated_object(c"root");
        let meta_rig = if object == 0 {
            0
        } else {
            at::<usize>(object, 8)
        };
        if object == 0 || meta_rig == 0 {
            return -88;
        }
        let expected = array(meta_rig, META_RIG_TRACK_NAMES)
            .1
            .min(array(meta_rig, META_RIG_REFERENCE_TRACKS).1);
        let _ = writeln!(
            out,
            "animatedObject=0x{object:x} metaRig=0x{meta_rig:x} expectedTrackCount={expected}\n"
        );
        let mut candidates = 0;
        let mut candidate = |out: &mut String,
                             label: &str,
                             owner_off: usize,
                             base: usize,
                             off: usize| {
            let entries: u64 = safe(base + off);
            let capacity: u32 = safe(base + off + 8);
            let size: u32 = safe(base + off + 0xC);
            if classify_qword(entries) != "HEAP"
                || !readable(entries as usize, 12 * 4)
                || size == 0
                || size > 4096
                || capacity < size
                || capacity > 8192
            {
                return;
            }
            if size != expected && size != expected * 2 && size != expected * 4 {
                return;
            }
            let _ = writeln!(out, "{label} ownerOff=0x{owner_off:x} arrOff=0x{off:x} entries=0x{entries:x} size={size} capacity={capacity}");
            for i in 0..size.min(12) as usize {
                let _ = writeln!(out, "  [{i}]={}", g(safe(entries as usize + i * 4)));
            }
            out.push('\n');
            candidates += 1;
        };
        let mut off = 0;
        while off + 0x10 <= 0x180 {
            candidate(out, "direct", off, object, off);
            off += 4;
        }
        let mut ptr_off = 0;
        while ptr_off + 8 <= 0x180 {
            let p: u64 = safe(object + ptr_off);
            if classify_qword(p) == "HEAP" && readable(p as usize, 0x200) {
                let mut off = 0;
                while off + 0x10 <= 0x200 {
                    candidate(out, "nested", ptr_off, p as usize, off);
                    off += 4;
                }
            }
            ptr_off += 8;
        }
        candidates
    }
);

/// The player's entAnimatedComponent "root".
fn root_animated_component() -> usize {
    let entity = player_entity();
    if entity == 0 {
        return 0;
    }
    let (kind, root) = (name("entAnimatedComponent"), name("root"));
    // SAFETY: the player's components.
    unsafe { components(entity) }
        .iter()
        .map(|c| c[0])
        .find(|&c| c != 0 && type_name(c) == Some(kind) && at::<CName>(c, COMPONENT_NAME) == root)
        .unwrap_or(0)
}

dump!(
    DumpRootGraphVariables,
    "root_graph_variables.txt",
    -73,
    |out| {
        let animated = root_animated_component();
        // SAFETY: the root graph resource.
        let graph = if animated == 0 {
            0
        } else {
            (unsafe { cvr_cpp_load_graph(animated as *mut c_void) }) as usize
        };
        if graph == 0 {
            return -74;
        }
        graph_variables(out, graph, "");
        1
    }
);

dump!(DumpAnimMemory, "anim_memory_dump.txt", -3, |out| {
    let entity = player_entity();
    if entity == 0 {
        return -1;
    }
    let (kind, root) = (name("entAnimatedComponent"), name("root"));
    let mut dumped = 0;
    let score = |base: usize| {
        if !readable(base, 10 * 32) {
            return 0;
        }
        (0..10)
            .filter(|i| (0.99..=1.01).contains(&safe::<f32>(base + i * 32 + 12)))
            .count()
    };
    // SAFETY: the player's components.
    for component in unsafe { components(entity) } {
        let c = component[0];
        if c == 0 || type_name(c) != Some(kind) || at::<CName>(c, COMPONENT_NAME) != root {
            continue;
        }
        out.push_str("==================================================\n");
        let _ = writeln!(
            out,
            "COMPONENT name=root ptr=0x{c:x} type=entAnimatedComponent"
        );
        out.push_str("==================================================\n");
        let mut off1 = 0x130;
        while off1 < 0x2B0 {
            let p1: u64 = safe(c + off1);
            if classify_qword(p1) == "HEAP" && readable(p1 as usize, 0x400) {
                let mut off2 = 0;
                while off2 < 0x400 {
                    let p2: u64 = safe(p1 as usize + off2);
                    if classify_qword(p2) == "HEAP" {
                        let s = score(p2 as usize);
                        if s >= 3 {
                            out.push_str("!!! FOUND BONE ARRAY CANDIDATE !!!\n");
                            let _ = writeln!(
                                out,
                                "Component Base + 0x{off1:x} -> + 0x{off2:x} -> ARRAY!"
                            );
                            let _ = writeln!(out, "Score: {s}/10 bones matched W=1.0\n");
                        }
                    }
                    off2 += 8;
                }
                let s = score(p1 as usize);
                if s >= 3 {
                    out.push_str("!!! FOUND BONE ARRAY CANDIDATE (Direct) !!!\n");
                    let _ = writeln!(out, "Component Base + 0x{off1:x} -> ARRAY!");
                    let _ = writeln!(out, "Score: {s}/10 bones matched W=1.0\n");
                }
            }
            off1 += 8;
        }
        dumped += 1;
    }
    dumped
});
