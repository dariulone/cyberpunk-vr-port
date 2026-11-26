//! VRPhysicalOpticsHinge(weapon, enabled) -> Bool (main's src/Anim/OpticHinge.cpp Request, 0aa233e0a): the request
//! the optics make every frame they aim the Osprey through its screen. Resolved here -- the player the roomscale owner,
//! the weapon its active one, its Barrel (an entAnimatedComponent) and the graph that drives it (the Receiver's, through
//! an enabled control binding, or the Barrel itself), the animation system's bucket entry of the weapon's animated
//! object -- and handed to cvr-rig's optic_hinge.rs, which writes the hinge in that object's pose pass.
use core::ffi::c_void;
use cvr_rig::vrik::optic_hinge::{self as hinge, Binding, Buckets, Handles, Stage};
use cvr_body::optic_hinge::Identity;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::{Handle, WeakHandle};
use red4ext_abi::stack::StackFrame;

use crate::native_args::args;

// entEntity: components (DynArray<Handle<entIComponent>>) +0xA0, runtimeScene +0xB8; entIComponent: name +0x40,
// owner +0x50.
const ENTITY_COMPONENTS: usize = 0xA0;
const ENTITY_RUNTIME_SCENE: usize = 0xB8;
const COMPONENT_NAME: usize = 0x40;
const COMPONENT_OWNER: usize = 0x50;

fn rd<T: Copy>(address: usize) -> Option<T> {
    if address < 0x10000 {
        return None;
    }
    cvr_engine::code::read::<T>(address)
}
fn is_a(object: usize, class: &str) -> bool {
    // SAFETY: a live engine object (held by the weapon's component array).
    object != 0 && unsafe { red4ext_abi::rtti::object_class(object as *const c_void) }.is_some_and(|c| c.is_a(CName::hashed(class)))
}
/// A handle stored in engine memory, taken (a strong reference of ours).
fn handle_at(address: usize) -> Handle {
    // SAFETY: a Handle the engine keeps at `address` (its instance and counts), cloned.
    unsafe { (*(address as *const Handle)).clone() }
}
/// The weapon's component named `name`, owned by it and an entAnimatedComponent.
fn animated_component(weapon: &Handle, name: u64) -> Handle {
    let w = weapon.instance as usize;
    let (Some(entries), Some(count)) = (rd::<usize>(w + ENTITY_COMPONENTS), rd::<u32>(w + ENTITY_COMPONENTS + 0xC)) else {
        return Handle::null();
    };
    for i in 0..count as usize {
        let Some(c) = rd::<usize>(entries + i * 16) else { break };
        if c != 0 && rd::<u64>(c + COMPONENT_NAME) == Some(name) && rd::<usize>(c + COMPONENT_OWNER) == Some(w) && is_a(c, "entAnimatedComponent") {
            return handle_at(entries + i * 16);
        }
    }
    Handle::null()
}
/// IsActiveWeapon: ScriptedPuppet.GetActiveWeapon(player) is the weapon.
fn is_active_weapon(player: &Handle, weapon: &Handle) -> bool {
    let Some(f) = crate::script_call::static_function("ScriptedPuppet", "GetActiveWeapon") else {
        hinge::mark(Stage::Signature);
        return false;
    };
    // SAFETY: the scripted static with the player held; its wref result locked.
    let Some(current) = (unsafe { crate::script_call::invoke_handle(core::ptr::null_mut(), f, &[crate::script_call::Value::Handle(player)]) }) else {
        hinge::mark(Stage::Query);
        return false;
    };
    let matches = !current.is_null() && current.instance == weapon.instance;
    if !matches {
        hinge::mark(Stage::ActiveMismatch);
    }
    matches
}
/// The update buckets' layout (anim_lookup's, from the SDK).
fn buckets() -> Buckets {
    let l = crate::anim_lookup::layout();
    let max = if l.objects_size > l.objects { (l.objects_size - l.objects) / 8 } else { 0 };
    Buckets {
        buckets: l.buckets as usize,
        stride: l.bucket_stride as usize,
        count: l.bucket_count,
        entities: l.entities as usize,
        components: l.components as usize,
        objects: l.objects as usize,
        entities_size: l.entities_size as usize,
        components_size: l.components_size as usize,
        objects_size: l.objects_size as usize,
        max_entries: max,
    }
}
/// Resolve: the binding the last request found while it still reads the same, or a scan of the weapon's scene's
/// animation system.
fn resolve(player: &Handle, weapon: &Handle, component: &Handle, graph: &Handle, control: &Handle) -> Option<Binding> {
    let w = weapon.instance as usize;
    let scene = rd::<usize>(w + ENTITY_RUNTIME_SCENE).unwrap_or(0);
    let mut old = hinge::current_binding();
    old.identity.player = player.instance as usize;
    if old.identity.weapon == w
        && old.identity.component == component.instance as usize
        && old.identity.graph_component == graph.instance as usize
        && old.identity.control_binding == control.instance as usize
        && old.scene == scene
    {
        if let Some(identity) = hinge::read_binding(&old) {
            old.identity = identity;
            return Some(old);
        }
    }
    let system = crate::anim_lookup::find_world_animation_system(scene, crate::anim_lookup::engine_objects()[3], None);
    if system == 0 {
        return None;
    }
    let identity = Identity {
        player: player.instance as usize,
        weapon: w,
        component: component.instance as usize,
        graph_component: graph.instance as usize,
        control_binding: control.instance as usize,
        ..Identity::default()
    };
    hinge::scan(system, scene, buckets(), identity)
}
/// Request: the hinge asked for (or let go) on this weapon.
pub(crate) fn request(weapon: &Handle, enabled: bool) -> bool {
    hinge::stat(0);
    if !enabled {
        hinge::mark(Stage::Disabled);
        hinge::clear();
        return true;
    }
    if weapon.is_null() || !hinge::context() {
        hinge::mark(Stage::Context);
        hinge::clear();
        return false;
    }
    let mut player = Handle::null();
    // SAFETY: GetPlayer into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut player) };
    if player.is_null() || player.instance as usize != cvr_rig::vrik::roomscale_move::cvr_roomscale_player_identity() {
        hinge::mark(Stage::Player);
        hinge::clear();
        return false;
    }
    if !is_active_weapon(&player, weapon) {
        hinge::clear();
        return false;
    }
    let component = animated_component(weapon, CName::hashed("Barrel").0);
    if component.is_null() {
        hinge::mark(Stage::Barrel);
        hinge::clear();
        return false;
    }
    let mut control = handle_at(component.instance as usize + hinge::ANIMATED_CONTROL_BINDING);
    let graph;
    if !control.is_null() && rd::<u8>(control.instance as usize + hinge::BINDING_ENABLED).unwrap_or(0) != 0 {
        let bind_name = rd::<u64>(control.instance as usize + hinge::BINDING_NAME).unwrap_or(0);
        if bind_name != CName::hashed("Receiver").0 {
            hinge::mark(Stage::Control);
            hinge::clear();
            return false;
        }
        graph = animated_component(weapon, bind_name);
    } else {
        graph = component.clone();
        control = Handle::null();
    }
    if graph.is_null() {
        hinge::mark(Stage::Graph);
        hinge::clear();
        return false;
    }
    let Some(binding) = resolve(&player, weapon, &component, &graph, &control) else {
        hinge::mark(Stage::Binding);
        hinge::clear();
        return false;
    };
    let handles = Handles {
        player: WeakHandle::from_handle(&player),
        weapon: WeakHandle::from_handle(weapon),
        component: WeakHandle::from_handle(&component),
        graph: WeakHandle::from_handle(&graph),
        control: WeakHandle::from_handle(&control),
    };
    hinge::accept(binding, handles);
    true
}

/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRPhysicalOpticsHinge(_context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _a4: i64) {
    // SAFETY: the VM's frame of this native (a handle:entEntity and a Bool).
    let mut a = unsafe { args(frame) };
    let weapon = a.get::<Handle>();
    let enabled = a.get::<bool>();
    a.finish();
    let result = request(&weapon, enabled);
    if !out.is_null() {
        // SAFETY: the VM's Bool result.
        unsafe { *(out as *mut bool) = result };
    }
}
