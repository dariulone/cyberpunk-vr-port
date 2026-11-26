//! The first-person neck cap's skin (formerly src/Natives/NeckCap.cpp; tools/neck_cap/make_neck_cap.py builds the cap
//! and adds it to the player entities). The cap closes the body's neck opening, which the game leaves open in the
//! first person; as a plain skinned mesh component it never gets the character creation's skin tone, so this copies
//! the body's: the full body mesh carries the tone as its mesh appearance ("02_ca_limestone", or a variant of it --
//! "_muscularity", "_censored"), and the cap takes the longest of its own appearance names the body's begins with.
//! With none (the lobby's hologram, a body of another kind, hands only) it is switched off until there is one.
//! Called a few times a second from the game thread (mods/cet/CyberpunkVRPort_VRIK).
use super::env::g_pSharedHands;
use super::rtti_call::{call, property};
use core::ffi::c_void;
use red4ext_abi::cname::CName;
use red4ext_abi::dynarray::DynArray;
use red4ext_abi::stack::StackFrame;

extern "C" {
    fn cvr_anim_player_entity() -> *mut c_void;
}

const CAP: &str = "vrp_neck_cap";
/// The full body mesh component, one per body.
const BODIES: [&str; 2] = ["t0_000_pma_base__full", "t0_000_pwa_base__full"];
/// The port's body in its place while the game has taken a woman's away (tools/body_stand_in/make_body_stand_in.py).
const STAND_IN: &str = "vrp_body_stand_in";
/// Utils/SharedSlots.hpp kBodyHandsOnly (xr_body_hands_only).
const SLOT_BODY_HANDS_ONLY: usize = 173;
/// entEntity::components; IComponent::name.
const ENTITY_COMPONENTS: usize = 0xA0;
const COMPONENT_NAME: usize = 0x40;

#[repr(C)]
#[derive(Clone, Copy)]
struct RawHandle {
    instance: usize,
    ref_count: usize,
}

fn log(text: &str) {
    super::env::log(text);
}
unsafe fn appearance(component: usize) -> CName {
    property(component, "meshAppearance").map_or(CName(0), |p| *p.value_ptr::<CName>(component as *mut c_void))
}
/// The appearances of the cap's mesh, once it is loaded.
unsafe fn cap_appearances<'a>(cap: usize) -> Option<&'a DynArray<RawHandle>> {
    let mesh = &*property(cap, "meshResource")?.value_ptr::<RawHandle>(cap as *mut c_void);
    if mesh.instance == 0 {
        return None;
    }
    let apps = property(mesh.instance, "appearances")?;
    Some(&*apps.value_ptr::<DynArray<RawHandle>>(mesh.instance as *mut c_void))
}
/// The longest of the cap's appearance names that the body's appearance begins with -- a whole name, or a name
/// followed by a variant ("_muscularity", "_censored", ...); empty when none does.
unsafe fn tone_for(cap: usize, body: &str) -> CName {
    let Some(apps) = cap_appearances(cap) else { return CName(0) };
    let body = body.as_bytes();
    let (mut best, mut best_length) = (CName(0), 0usize);
    for app in apps.as_slice() {
        if app.instance == 0 {
            continue;
        }
        let Some(name_prop) = property(app.instance, "name") else { continue };
        let name = *name_prop.value_ptr::<CName>(app.instance as *mut c_void);
        let text = name.text().as_bytes();
        let length = text.len();
        if length > best_length && body.starts_with(text) && (body.len() == length || body[length] == b'_') {
            best = name;
            best_length = length;
        }
    }
    best
}
unsafe fn enabled(component: usize) -> bool {
    property(component, "isEnabled").is_some_and(|p| *p.value_ptr::<bool>(component as *mut c_void))
}

/// VRNeckCapSync() -> Int32: 0 in step, 1 the cap took the body's tone, 2 the cap was switched off (no tone of its own
/// for the body's appearance), 3 the mesh is not loaded yet; -1 no player, -2 no body component, -3 no cap.
#[no_mangle]
pub unsafe extern "C" fn VRNeckCapSync(_this: *mut c_void, frame: *mut StackFrame, out: *mut i32, _unused: i64) {
    if let Some(frame) = frame.as_mut() {
        frame.finish_params();
    }
    let player = cvr_anim_player_entity() as usize;
    let (mut cap, mut body, mut stand_in) = (0usize, 0usize, 0usize);
    if player != 0 {
        for handle in (*((player + ENTITY_COMPONENTS) as *const DynArray<RawHandle>)).as_slice() {
            let component = handle.instance;
            if component == 0 {
                continue;
            }
            let name = (*((component + COMPONENT_NAME) as *const CName)).text();
            if name.is_empty() {
                continue;
            }
            if cap == 0 && name.eq_ignore_ascii_case(CAP) {
                cap = component;
            }
            if stand_in == 0 && name.eq_ignore_ascii_case(STAND_IN) {
                stand_in = component;
            }
            for candidate in BODIES {
                if body == 0 && name.eq_ignore_ascii_case(candidate) {
                    body = component;
                }
            }
        }
    }
    if body == 0 && stand_in != 0 && enabled(stand_in) {
        body = stand_in;
    }
    let mut result = 0;
    if player == 0 {
        result = -1;
    } else if body == 0 {
        result = -2;
    } else if cap == 0 {
        result = -3;
    } else if cap_appearances(cap).is_none() {
        result = 3;
    } else {
        let body_tone = appearance(body);
        // Hands only (xr_body_hands_only, shared[173]): no body to close, the cap stays off with it.
        let shared = g_pSharedHands;
        let hands_only = !shared.is_null() && *shared.add(SLOT_BODY_HANDS_ONLY) > 0.5;
        let tone = if hands_only { CName(0) } else { tone_for(cap, body_tone.text()) };
        if tone.0 == 0 {
            if enabled(cap) {
                let mut off = false;
                call(cap, "Toggle", Some(&mut off));
                log(&format!(
                    "[neckcap] body appearance {}: {}, cap off",
                    body_tone.text(),
                    if hands_only { "hands only" } else { "no cap tone for it" }
                ));
                result = 2;
            }
        } else {
            let was_off = !enabled(cap);
            if appearance(cap) != tone {
                if let Some(p) = property(cap, "meshAppearance") {
                    p.set_value(cap as *mut c_void, &tone);
                }
                call(cap, "RefreshAppearance", None);
                log(&format!("[neckcap] body appearance {} -> cap {}", body_tone.text(), tone.text()));
                result = 1;
            }
            if was_off {
                let mut on = true;
                call(cap, "Toggle", Some(&mut on));
                result = 1;
            }
        }
    }
    if let Some(out) = out.as_mut() {
        *out = result;
    }
}
