//! Keeps the forearm readout from sitting there as a black rectangle (formerly src/Natives/WristGuard.cpp, where the
//! fault is written up): the readout is a WorldWidgetComponent (`vrp_latd_ui`) on a skinned surface of the left arm
//! (`vrp_latd_screen`), and the engine drops its UI render target when the surface changes rendering plane on a draw.
//! The repair is the component's own scripted Toggle (off and on in one frame); the whole recipe freezes the surface in
//! the weapon plane (its `renderingPlaneAnimationParam` cleared) and re-arms once per player object, then keeps
//! watching. Called per frame from the game thread: the components are cached and checked by three plain reads
//! (VirtualQuery here cost 13.5 ms a call), and re-fetched through the player's component array every 120 calls.
use super::rtti_call::{function, property};
use core::ffi::c_void;
use red4ext_abi::cname::CName;
use red4ext_abi::dynarray::DynArray;
use red4ext_abi::stack::StackFrame;
use std::sync::Mutex;

extern "C" {
    static g_hasWeaponEquipped: bool;
    fn cvr_anim_player_entity() -> *mut c_void;
}

const WIDGET_COMPONENT: &str = "vrp_latd_ui";
const WIDGET_CLASS: &str = "WorldWidgetComponent";
const SURFACE_COMPONENT: &str = "vrp_latd_screen";
/// The handle to the UI render target (inside the 928-byte WorldWidgetComponent; the class is checked first).
const TARGET_HANDLE: usize = 0x220;
/// Frames to stay quiet after a repair, so a wrong premise cannot become a per-frame strobe.
const COOLDOWN_FRAMES: i32 = 20;
/// entIComponent: name (CName) and id (CRUID, nonzero once constructed); entEntity::components.
const NAME: usize = 0x40;
const ID: usize = 0x60;
const ENTITY_COMPONENTS: usize = 0xA0;
/// Calls a weapon must already have been in hand, the target standing still, before the freeze without a transition.
const SETTLE_FRAMES: i32 = 45;
const REWALK_PERIOD: i32 = 120;
/// Frames with a target before a swap without a weapon in hand is taken for a plane transition (the spawn's swaps
/// come earlier).
const UNARMED_AFTER_FRAMES: i32 = 300;

/// The file's state (its last result is kept, unread, as the C++ kept it).
#[allow(dead_code)]
struct Wrist {
    last_target: usize,
    component: usize,
    component_vtbl: usize,
    cooldown: i32,
    repairs: i32,
    last_result: i32,
    walks: i32,
    frozen: bool,
    frozen_for: usize,
    settle: i32,
    surface: usize,
    surface_vtbl: usize,
    since_rewalk: i32,
    /// The working target's vptr (read once the readout was re-armed): a target of another class at the same address
    /// is the dead one's wrapper -- black -- even though the pointer did not move.
    good_target_vtbl: usize,
    /// Frames since the last re-arm (the target's class read once it settled).
    since_rearm: i32,
    /// Frames the unfrozen readout has had a target: a swap without a weapon counts only once the spawn's own swaps
    /// are over.
    watched: i32,
}
static WRIST: Mutex<Wrist> = Mutex::new(Wrist {
    last_target: 0,
    component: 0,
    component_vtbl: 0,
    cooldown: 0,
    repairs: 0,
    last_result: 0,
    walks: 0,
    frozen: false,
    frozen_for: 0,
    settle: 0,
    surface: 0,
    surface_vtbl: 0,
    since_rewalk: 0,
    good_target_vtbl: 0,
    since_rearm: 0,
    watched: 0,
});

#[repr(C)]
#[derive(Clone, Copy)]
struct RawHandle {
    instance: usize,
    ref_count: usize,
}

fn log(text: &str) {
    super::env::log(&format!("{text}\n"));
}
unsafe fn at<T: Copy>(address: usize) -> T {
    core::ptr::read_unaligned(address as *const T)
}
/// The cached component, checked cheaply: the same vptr, our name, a nonzero id.
unsafe fn still_valid(object: usize, vtbl: usize, name: &str) -> bool {
    object != 0 && vtbl != 0 && at::<usize>(object) == vtbl && at::<u64>(object + NAME) == CName::hashed(name).0 && at::<u64>(object + ID) != 0
}
unsafe fn player_components<'a>() -> Option<&'a [RawHandle]> {
    let entity = cvr_anim_player_entity() as usize;
    if entity == 0 {
        return None;
    }
    Some((*((entity + ENTITY_COMPONENTS) as *const DynArray<RawHandle>)).as_slice())
}
unsafe fn named(component: usize, name: &str) -> bool {
    let text = at::<CName>(component + NAME).text();
    !text.is_empty() && text.eq_ignore_ascii_case(name)
}
unsafe fn class_is_a(object: usize, class: &str) -> bool {
    red4ext_abi::rtti::object_class(object as *const c_void).is_some_and(|c| c.is_a(CName::hashed(class)))
}

impl Wrist {
    /// The expensive path: the player through the script VM and a walk of its components.
    unsafe fn walk_for_widget(&mut self) -> usize {
        let Some(components) = player_components() else { return 0 };
        self.walks += 1;
        for handle in components {
            let component = handle.instance;
            if component == 0 || !named(component, WIDGET_COMPONENT) {
                continue;
            }
            // The name is ours but the layout is not: refuse the offset.
            return if class_is_a(component, WIDGET_CLASS) { component } else { 0 };
        }
        0
    }
    unsafe fn find_widget(&mut self) -> usize {
        self.since_rewalk += 1;
        if self.since_rewalk >= REWALK_PERIOD {
            self.since_rewalk = 0;
            self.component = 0;
            self.surface = 0;
        }
        if still_valid(self.component, self.component_vtbl, WIDGET_COMPONENT) {
            return self.component;
        }
        let component = self.walk_for_widget();
        if component == 0 {
            self.component = 0;
            self.component_vtbl = 0;
            return 0;
        }
        self.component = component;
        self.component_vtbl = at::<usize>(component);
        component
    }
    unsafe fn find_surface(&mut self) -> usize {
        if still_valid(self.surface, self.surface_vtbl, SURFACE_COMPONENT) {
            return self.surface;
        }
        let Some(components) = player_components() else { return 0 };
        self.walks += 1;
        for handle in components {
            let component = handle.instance;
            if component == 0 || !named(component, SURFACE_COMPONENT) {
                continue;
            }
            self.surface = component;
            self.surface_vtbl = at::<usize>(component);
            return component;
        }
        self.surface = 0;
        self.surface_vtbl = 0;
        0
    }
}
/// Clearing the plane param freezes the surface where the draw event left it (the weapon plane).
unsafe fn set_plane_param(surface: usize, value: &str) -> bool {
    let Some(p) = property(surface, "renderingPlaneAnimationParam") else {
        return false;
    };
    p.set_value(surface as *mut c_void, &CName::hashed(value));
    true
}
/// The component's own scripted Toggle, off and on in the same frame (writing isEnabled would not run the attach
/// path that acquires a target).
unsafe fn toggle(component: usize) -> bool {
    let Some(f) = function(component, "Toggle") else { return false };
    let (mut off, mut on) = (false, true);
    let a = super::rtti_call::execute(component, f, Some(&mut off));
    let b = super::rtti_call::execute(component, f, Some(&mut on));
    a && b
}

/// VRWristGuard(mode) -> Int32.
/// mode 0 tick: look, and repair if the target was swapped; 1 adopt whatever is there now; 2 the repairs so far;
/// 3 the component walks so far; 4 detect only (1 when the target changed); 5 the whole recipe, once, then watch;
/// 6 forget the recipe was done.
/// Returns 0 nothing to do, 1 repaired, 2 adopted the first sample, 3 in cooldown; -1 no player, -2 no component (or
/// the wrong class), -4 Toggle is not on the class, -5 the plane param is not.
#[no_mangle]
pub unsafe extern "C" fn VRWristGuard(_this: *mut c_void, frame: *mut StackFrame, out: *mut i32, _unused: i64) {
    let mut mode = 0i32;
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut mode);
        frame.finish_params();
    }
    let result = wrist_guard(mode);
    if let Some(out) = out.as_mut() {
        *out = result;
    }
}

unsafe fn wrist_guard(mode: i32) -> i32 {
    let mut w = WRIST.lock().unwrap_or_else(|p| p.into_inner());
    if mode == 2 {
        return w.repairs;
    }
    if mode == 3 {
        return w.walks;
    }
    if mode == 5 {
        let current = w.find_widget();
        if w.frozen && current != 0 && current != w.frozen_for {
            // A different component object: a new player entity, so the freeze is owed again.
            w.frozen = false;
            w.frozen_for = 0;
            w.last_target = 0;
            w.settle = 0;
            w.cooldown = 0;
            w.good_target_vtbl = 0;
            log("[wrist] the player entity was replaced -> the freeze is owed again");
        }
        if w.frozen {
            if w.cooldown > 0 {
                w.cooldown -= 1;
                return 2;
            }
            if current == 0 {
                return 2;
            }
            let target = at::<usize>(current + TARGET_HANDLE);
            // the working target's class, learnt once the re-arm settled
            w.since_rearm = w.since_rearm.saturating_add(1);
            if w.good_target_vtbl == 0 && target != 0 && w.since_rearm >= COOLDOWN_FRAMES {
                w.good_target_vtbl = at::<usize>(target);
            }
            let moved = w.last_target != 0 && target != w.last_target;
            let swapped = !moved && target != 0 && w.good_target_vtbl != 0 && at::<usize>(target) != w.good_target_vtbl;
            if moved || swapped {
                toggle(current);
                w.repairs += 1;
                w.cooldown = COOLDOWN_FRAMES;
                w.since_rearm = 0;
                w.last_target = at::<usize>(current + TARGET_HANDLE);
                log(&format!(
                    "[wrist] the target died again after the freeze ({}) -> re-armed ({})",
                    if moved { "a new target" } else { "a target of another class in its place" },
                    w.repairs
                ));
                return 3;
            }
            w.last_target = target;
            return 2;
        }
        let widget = current;
        let surface = w.find_surface();
        if widget == 0 || surface == 0 {
            w.last_target = 0;
            w.watched = 0;
            return -2;
        }
        let target = at::<usize>(widget + TARGET_HANDLE);
        w.watched = if target != 0 { w.watched.saturating_add(1) } else { 0 };
        let first = w.last_target == 0;
        let moved = !first && target != w.last_target;
        w.last_target = target;
        // A weapon already in hand counts as the transition, once the picture has settled (never on the first
        // frame: the graph must have reached the weapon-out state).
        let equipped = core::ptr::read_volatile(&raw const g_hasWeaponEquipped);
        // ANY TRANSITION IS THE ONE (2026-11-24): the game moves the arms into the weapon plane for a climb or a vault
        // too, and the readout went black there until a weapon was drawn -- the first transition from the spawn's
        // scene plane goes into the weapon plane whatever made it, so it is frozen there and re-armed at once.
        let mut ready = moved && (equipped || w.watched >= UNARMED_AFTER_FRAMES);
        let mut how = if equipped { "caught the plane transition" } else { "caught a plane transition without a weapon (a climb or a vault)" };
        if !ready {
            if equipped && !moved && target != 0 && {
                w.settle += 1;
                w.settle >= SETTLE_FRAMES
            } {
                ready = true;
                how = "the weapon was already in hand";
            } else if !equipped || target == 0 {
                w.settle = 0;
            }
        }
        if !ready {
            return 0;
        }
        if !set_plane_param(surface, "None") {
            return -5;
        }
        if !toggle(widget) {
            return -4;
        }
        w.frozen = true;
        w.frozen_for = widget;
        w.settle = 0;
        w.since_rearm = 0;
        w.good_target_vtbl = 0;
        w.last_target = at::<usize>(widget + TARGET_HANDLE);
        log(&format!("[wrist] {how} -> frozen in the weapon plane and re-armed"));
        return 1;
    }
    if mode == 6 {
        w.frozen = false;
        w.frozen_for = 0;
        w.last_target = 0;
        w.settle = 0;
        return 0;
    }
    if mode == 4 {
        let c = w.find_widget();
        if c == 0 {
            w.last_target = 0;
            return -2;
        }
        let target = at::<usize>(c + TARGET_HANDLE);
        let first = w.last_target == 0;
        let changed = !first && target != w.last_target;
        w.last_target = target;
        return changed as i32;
    }
    let component = w.find_widget();
    if component == 0 {
        // A load screen or a respawn: the component will be a different object, so nothing carries over.
        w.last_target = 0;
        w.component = 0;
        w.component_vtbl = 0;
        w.surface = 0;
        w.surface_vtbl = 0;
        w.frozen = false;
        w.frozen_for = 0;
        w.cooldown = 0;
        w.last_result = -1;
        return if cvr_anim_player_entity().is_null() { -1 } else { -2 };
    }
    let slot = component + TARGET_HANDLE;
    let target = at::<usize>(slot);
    if mode == 1 || component != w.component || w.last_target == 0 {
        w.component = component;
        w.last_target = target;
        w.cooldown = 0;
        w.last_result = 2;
        return 2;
    }
    if w.cooldown > 0 {
        w.cooldown -= 1;
        // Adopt whatever settled during the cooldown, so the next change is measured from here.
        w.last_target = target;
        w.last_result = 3;
        return 3;
    }
    if target == w.last_target {
        w.last_result = 0;
        return 0;
    }
    // The target was swapped -- this is the frame the readout went black.
    if !toggle(component) {
        w.last_target = target;
        w.last_result = -4;
        return -4;
    }
    w.repairs += 1;
    w.cooldown = COOLDOWN_FRAMES;
    w.last_target = at::<usize>(slot);
    w.last_result = 1;
    log(&format!("[wrist] render target swapped -> repaired ({} this session)", w.repairs));
    1
}
