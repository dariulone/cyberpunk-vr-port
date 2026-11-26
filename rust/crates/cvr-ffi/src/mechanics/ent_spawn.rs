//! The mechanics' own entities (the VR menu's and the weapon wheel's quads, the holsters' copies on the bones, a thrown
//! grenade, the reach marker, magazines): spawned through Cyber Engine Tweaks' exEntitySpawner when CET is installed
//! (CET registers that class), else through the plugin's own spawner (spawn_system: the template's own SpawnableObject
//! record in the population system), which a player without CET has. CVR_NO_EX_SPAWNER=1 (a development switch) takes
//! the plugin's own with CET installed too.
use crate::script_call::{invoke, Value};
use red4ext_abi::handle::Handle;
use red4ext_abi::function::BaseFunction;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

type V3 = [f64; 3];
type Q = [f64; 4];

fn ex(name: &str) -> Option<&'static BaseFunction> {
    static OFF: OnceLock<bool> = OnceLock::new();
    if *OFF.get_or_init(|| std::env::var("CVR_NO_EX_SPAWNER").is_ok_and(|v| v == "1")) {
        return None;
    }
    // CET's class, looked up without the missing-class audit: a clean game has no CET, and the plugin spawns itself
    let rtti = red4ext_abi::rtti::RttiSystem::get()?;
    rtti.get_class(red4ext_abi::cname::CName::hashed("exEntitySpawner")).and_then(|c| c.function(red4ext_abi::cname::CName::hashed(name)))
}

/// Which spawner was taken, said once each.
fn say(ex: bool) {
    static SAID: AtomicU8 = AtomicU8::new(0);
    let bit = if ex { 1 } else { 2 };
    if SAID.fetch_or(bit, Ordering::Relaxed) & bit == 0 {
        cvr_host::log!(
            "[spawn] the mechanics' entities through {}\n",
            if ex { "Cyber Engine Tweaks' exEntitySpawner" } else { "the plugin's own spawner (no Cyber Engine Tweaks)" }
        );
    }
}

/// Whether entities can be spawned now.
pub(super) fn available() -> bool {
    ex("Spawn").is_some() || crate::spawn_system::available()
}

/// The template (an .ent path) at the place, in an appearance (0 its default); its entity id once asked.
pub(super) fn spawn(path: &str, p: V3, q: Q, appearance: u64) -> Option<u64> {
    if let Some(f) = ex("Spawn") {
        say(true);
        let mut t = [0u32; 8];
        for k in 0..3 {
            t[k] = ((p[k] * 131072.0) as i32) as u32;
        }
        for k in 0..4 {
            t[4 + k] = (q[k] as f32).to_bits();
        }
        let res = red4ext_abi::resource::resource_path(path);
        // SAFETY: the spawner's static: the entity's resource path, its WorldTransform and its appearance.
        return unsafe { invoke::<u64>(core::ptr::null_mut(), f, &[Value::U64(res), Value::Ptr(t.as_ptr().cast()), Value::U64(appearance)]) }
            .filter(|id| *id != 0);
    }
    say(false);
    let position = [p[0] as f32, p[1] as f32, p[2] as f32, 1.0];
    let orientation = [q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32];
    let id = crate::spawn_system::spawn_template(path, appearance, &position, &orientation);
    (id != 0).then_some(id)
}

/// The entity let go, by the spawner that made it.
pub(super) fn despawn(e: &Handle) -> bool {
    if e.is_null() {
        return false;
    }
    if let Some(f) = ex("Despawn") {
        // SAFETY: the spawner's static with the entity held.
        unsafe { invoke::<()>(core::ptr::null_mut(), f, &[Value::Handle(e)]) };
        return true;
    }
    let id = super::lobby_rtti::call(e, "GetEntityID", &[]).map(|o| {
        // SAFETY: an EntityID result (its hash first).
        unsafe { *(o.as_ptr() as *const u64) }
    });
    id.is_some_and(crate::spawn_system::delete_spawned)
}
