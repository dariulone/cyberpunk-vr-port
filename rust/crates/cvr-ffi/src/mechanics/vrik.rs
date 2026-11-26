//! mods/cet/CyberpunkVRPort_VRIK's per-frame pump (the mod's onUpdate, the mirror double and the console's
//! EquipCyberware aside, which stay the Lua host's): the first-person neck cap's skin tone every 0.25 s (VRNeckCapSync,
//! in the main menu's lobby too); the overlay's mouse-Y setting every 0.25 s; a recenter 3.5 s after the player
//! attaches (the Lua observer reports it: VRIKGameAttached), asked by bumping vrik_recenter.ini's counter; then, with a
//! player: its locomotion, detailed locomotion, scanner and paired-animation states to the plugin; the engine-native
//! camera write (the desired world quaternion in shared [100..103], published with [151] under mode [84], made local
//! to the player's orientation and written through the FPP camera's SetLocalOrientation, acknowledged with
//! SetVRCamAck); the sprint animation's input zeroed; the FPP camera's pitch limits (0/0 with the mouse's Y off, else
//! +-80); the FPP arms restored once; UpdateVRIKAnimInputs; the FPP camera's world pose (the engine's
//! Matrix.GetTranslation/ToQuat of its local-to-world) with the player's yaw, position and orientation to
//! SetVRPlayerYaw. The plugin's natives are called through the RTTI as the Lua called them (script_call::call_native).
use crate::script_call::{call_handle, call_native, method, Value};
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

struct State {
    /// The FPS lock's quarter-second clock and the value the settings last took (-1 none).
    fps_lock_timer: f64,
    fps_lock_value: i32,
    neck_cap_timer: f64,
    settings_timer: f64,
    recenter_pending: f64,
    recenter_counter: i64,
    mouse_disable: bool,
    need_restore_arms: bool,
    loco_warned: bool,
    cam_write_last_seq: f32,
    last_cam: Option<([f32; 4], [f32; 4])>,
    reported: bool,
}
static STATE: Mutex<State> = Mutex::new(State {
    fps_lock_timer: 0.0,
    fps_lock_value: -1,
    neck_cap_timer: 0.0,
    settings_timer: 0.0,
    recenter_pending: -1.0,
    recenter_counter: -1,
    mouse_disable: true,
    need_restore_arms: true,
    loco_warned: false,
    cam_write_last_seq: -1.0,
    last_cam: None,
    reported: false,
});
/// The ToggleVRHands hotkey's state (VRIKTrackingEnabled): the diagnostic capture kept fresh while it is on.
pub(in crate::mechanics) static TRACKING: AtomicBool = AtomicBool::new(false);

type R<T> = Result<T, String>;
fn m<T: crate::script_call::Ret>(o: &Handle, name: &str, values: &[Value]) -> R<T> {
    // SAFETY: the caller's live object, held.
    unsafe { method::<T>(o.instance, name, values) }.ok_or_else(|| format!("{name} failed"))
}
fn native<T: crate::script_call::Ret>(name: &str, values: &[Value]) -> R<T> {
    // SAFETY: one of the plugin's own natives, called as the script VM calls it.
    unsafe { call_native::<T>(name, values) }.ok_or_else(|| format!("{name} failed"))
}
fn shared(i: i32) -> f32 {
    // SAFETY: the shared block's read.
    unsafe { cvr_rig::vrik::provider::shared_slot(i) }
}

/// The mod's requestRecenter(): the counter bumped in vrik_recenter.ini (the plugin polls it).
fn request_recenter(s: &mut State) {
    if s.recenter_counter < 0 {
        // Seeded per session (os.time()), so the plugin sees a change.
        s.recenter_counter = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    }
    s.recenter_counter += 1;
    let path = cvr_host::paths::file(cvr_host::paths::Folder::Lua, "CyberpunkVRPort_VRIK\\vrik_recenter.ini");
    if std::fs::write(&path, format!("recenter={}\n", s.recenter_counter)).is_ok() {
        log!("[mechanics] vrik: recenter requested from Rust ({})\n", s.recenter_counter);
    }
}
/// The mod's pushLocomotion(): the world's time dilation and the state machine's states to the plugin.
fn push_locomotion(player: &Handle) -> R<()> {
    use super::boards::{definition, field, get};
    let ts = super::game_system("GetTimeSystem");
    if !ts.is_null() {
        // SAFETY: the time system, held.
        if let Some(scale) = unsafe { crate::script_call::call_float(ts.instance, "GetActiveTimeDilation") } {
            native::<i32>("SetVRWorldTimeScale", &[Value::Float(scale)])?;
        }
    }
    let defs = super::boards::defs()?;
    let d = definition(&defs, "PlayerStateMachine")?;
    // SAFETY: the player, held.
    let bb = unsafe { call_handle(player.instance, "GetPlayerStateMachineBlackboard", &[]) }.ok_or("GetPlayerStateMachineBlackboard failed")?;
    if bb.is_null() {
        return Ok(());
    }
    let int = |name: &str| -> R<i32> { get::<i32>(&bb, "GetInt", field(d, name)?) };
    native::<i32>("SetVRLocomotionState", &[Value::Int32(int("Locomotion")?)])?;
    native::<i32>("SetVRLocomotionDetailed", &[Value::Int32(int("LocomotionDetailed")?)])?;
    native::<i32>("SetVRScannerState", &[Value::Int32(int("Vision")?)])?;
    native::<i32>(
        "SetVRPairedAnimationState",
        &[Value::Int32(int("Takedown")?), Value::Int32(int("BodyCarrying")?), Value::Int32(int("BodyDisposalDetailed")?)],
    )?;
    Ok(())
}
/// The mod's applyCamWrite(): local = conj(parent world) * desired world, through the camera's own setter.
fn apply_cam_write(s: &mut State, player: &Handle) -> R<()> {
    if shared(84) != 1.0 {
        return Ok(());
    }
    let s1 = shared(151);
    if s1 == 0.0 || s1 == s.cam_write_last_seq {
        return Ok(());
    }
    let (qx, qy, qz, qw) = (shared(100) as f64, shared(101) as f64, shared(102) as f64, shared(103) as f64);
    // Torn-publish guard: the sequence is written last, a changed re-read spans two publishes.
    if shared(151) != s1 {
        return Ok(());
    }
    // SAFETY: the player, held.
    let cam = unsafe { call_handle(player.instance, "GetFPPCameraComponent", &[]) }.ok_or("GetFPPCameraComponent failed")?;
    if cam.is_null() {
        return Ok(());
    }
    let pw = m::<[f32; 4]>(player, "GetWorldOrientation", &[])?;
    let (ci, cj, ck, cr) = (-pw[0] as f64, -pw[1] as f64, -pw[2] as f64, pw[3] as f64);
    let li = cr * qx + ci * qw + cj * qz - ck * qy;
    let lj = cr * qy - ci * qz + cj * qw + ck * qx;
    let lk = cr * qz + ci * qy - cj * qx + ck * qw;
    let lr = cr * qw - ci * qx - cj * qy - ck * qz;
    let n = (li * li + lj * lj + lk * lk + lr * lr).sqrt();
    if n < 1e-6 {
        return Ok(());
    }
    let q = [(li / n) as f32, (lj / n) as f32, (lk / n) as f32, (lr / n) as f32];
    m::<()>(&cam, "SetLocalOrientation", &[Value::Ptr(q.as_ptr().cast())])?;
    s.cam_write_last_seq = s1;
    native::<()>("SetVRCamAck", &[Value::Float(s1)])?;
    Ok(())
}
/// The FPP camera's world pose: the engine's Matrix.GetTranslation and Matrix.ToQuat of its local-to-world.
fn camera_world_pose(cam: &Handle) -> R<([f32; 4], [f32; 4])> {
    let l2w = m::<[[f32; 4]; 4]>(cam, "GetLocalToWorld", &[])?;
    let t = crate::script_call::native_class_function("Matrix", "GetTranslation").ok_or("no Matrix.GetTranslation")?;
    let q = crate::script_call::native_class_function("Matrix", "ToQuat").ok_or("no Matrix.ToQuat")?;
    // SAFETY: the statics with the matrix in place.
    unsafe {
        let pos = crate::script_call::invoke::<[f32; 4]>(core::ptr::null_mut(), t, &[Value::Ptr(l2w.as_ptr().cast())]).ok_or("GetTranslation failed")?;
        let quat = crate::script_call::invoke::<[f32; 4]>(core::ptr::null_mut(), q, &[Value::Ptr(l2w.as_ptr().cast())]).ok_or("ToQuat failed")?;
        Ok((pos, quat))
    }
}

fn pump(s: &mut State, dt: f64) -> R<()> {
    s.settings_timer += dt;
    if s.settings_timer >= 0.25 {
        s.settings_timer = 0.0;
        if let Ok(v) = native::<bool>("GetVRMouseYDisabled", &[]) {
            s.mouse_disable = v;
        }
    }
    let mut player = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut player) };
    // A queued recenter once gameplay has settled and the player exists.
    if s.recenter_pending >= 0.0 {
        s.recenter_pending -= dt;
        // Not the main menu's puppet: the lobby places the player itself, and a recenter there landed in the middle
        // of the intro's flight (the eyes jumped, the overlay guard drained, frames went uncaptured). The C++ build's
        // Lua observer was registered too late to see that puppet attach.
        if s.recenter_pending < 0.0 && !player.is_null() && native::<bool>("VRLobbyActive", &[]) != Ok(true) {
            request_recenter(s);
        }
    }
    if player.is_null() {
        return Ok(());
    }
    if push_locomotion(&player).is_err() && !s.loco_warned {
        s.loco_warned = true;
        log!("[VRIK] locomotion state unavailable -- sprint crouch gate off\n");
    }
    let _ = apply_cam_write(s, &player);
    // The sprint animation's input zeroed every frame (the state machine re-asserts it on sprint entry).
    if let Some(f) = crate::script_call::static_function("AnimationControllerComponent", "SetInputFloat") {
        // SAFETY: the static with the player held.
        unsafe {
            crate::script_call::invoke::<()>(core::ptr::null_mut(), f, &[Value::Handle(&player), Value::U64(CName::hashed("sprint").0), Value::Float(0.0)])
        };
    }
    // SAFETY: the player, held.
    let cam = unsafe { call_handle(player.instance, "GetFPPCameraComponent", &[]) }.unwrap_or_else(Handle::null);
    if !cam.is_null() {
        let (max, min) = if s.mouse_disable { (0.0f32, 0.0f32) } else { (80.0, -80.0) };
        // SAFETY: the camera's Float properties.
        unsafe {
            if let Some(p) = crate::script_call::property(cam.instance, "pitchMax") {
                *(p as *mut f32) = max;
            }
            if let Some(p) = crate::script_call::property(cam.instance, "pitchMin") {
                *(p as *mut f32) = min;
            }
        }
    }
    if s.need_restore_arms {
        let _ = native::<i32>("RestoreVRFppArms", &[]);
        s.need_restore_arms = false;
    }
    let _ = native::<i32>("UpdateVRIKAnimInputs", &[]);
    if cam.is_null() {
        return Ok(());
    }
    let (cam_pos, cam_quat) = camera_world_pose(&cam)?;
    s.last_cam = Some((cam_pos, cam_quat));
    if TRACKING.load(Ordering::Relaxed) {
        let _ = native::<i32>("SetVRDiagCapture", &[Value::Int32(1)]);
    }
    // The entity's world orientation (the conjugate is the exact world->model rotation); from the yaw only without it.
    let yaw = m::<f32>(&player, "GetWorldYaw", &[]).unwrap_or(0.0);
    let eq = m::<[f32; 4]>(&player, "GetWorldOrientation", &[]).unwrap_or_else(|_| {
        let h = (yaw as f64).to_radians() * 0.5;
        [0.0, 0.0, h.sin() as f32, h.cos() as f32]
    });
    let ep = m::<[f32; 4]>(&player, "GetWorldPosition", &[]).unwrap_or([0.0; 4]);
    let values = [
        Value::Float(yaw),
        Value::Float(cam_quat[0]),
        Value::Float(cam_quat[1]),
        Value::Float(cam_quat[2]),
        Value::Float(cam_quat[3]),
        Value::Float(cam_pos[0]),
        Value::Float(cam_pos[1]),
        Value::Float(cam_pos[2]),
        Value::Float(ep[0]),
        Value::Float(ep[1]),
        Value::Float(ep[2]),
        Value::Float(eq[0]),
        Value::Float(eq[1]),
        Value::Float(eq[2]),
        Value::Float(eq[3]),
    ];
    native::<i32>("SetVRPlayerYaw", &values)?;
    if !s.reported {
        s.reported = true;
        log!(
            "[mechanics] vrik: pump from Rust (camera ({:.2}, {:.2}, {:.2}), yaw {yaw:.1}, mouse Y off {})\n",
            cam_pos[0],
            cam_pos[1],
            cam_pos[2],
            s.mouse_disable
        );
    }
    Ok(())
}

/// THE ENGINE'S FPS LOCK FOLLOWS THE HEADSET (main's f7a824dd5): every quarter second the game's MaximumFPS setting is
/// switched on and set to the plugin's target (shared [226]: the headset's refresh, half of it under frame generation),
/// clamped to 10..240; a value the settings took is not written again.
fn sync_game_fps_lock(s: &mut State, dt: f64) {
    unsafe extern "C" {
        fn cvr_cpp_xr_get_shared_slot(slot: i32) -> f32;
    }
    s.fps_lock_timer += dt;
    if s.fps_lock_timer < 0.25 {
        return;
    }
    s.fps_lock_timer = 0.0;
    // SAFETY: the shared block's read.
    let target = f64::from(unsafe { cvr_cpp_xr_get_shared_slot(226) });
    if !(target >= 10.0) {
        return;
    }
    let value = (target.clamp(10.0, 240.0) + 0.5).floor() as i32;
    if value == s.fps_lock_value {
        return;
    }
    let settings = super::game_system("GetSettingsSystem");
    if settings.is_null() {
        return;
    }
    let var = |name: &core::ffi::CStr| {
        // SAFETY: the settings system, held for the call.
        unsafe { call_handle(settings.instance, "GetVar", &[Value::U64(CName::add(c"/video/display").0), Value::U64(CName::add(name).0)]) }
            .filter(|h| !h.is_null())
    };
    let (Some(enabled), Some(cap)) = (var(c"MaximumFPS_OnOff"), var(c"MaximumFPS_Value")) else { return };
    // SAFETY: the config vars, held for the calls.
    let ok = unsafe {
        let on = method::<bool>(enabled.instance, "GetValue", &[]) == Some(true)
            || method::<()>(enabled.instance, "SetValue", &[Value::Bool(true)]).is_some();
        let set = method::<i32>(cap.instance, "GetValue", &[]) == Some(value)
            || method::<()>(cap.instance, "SetValue", &[Value::Int32(value)]).is_some();
        on && set
    };
    if ok {
        s.fps_lock_value = value;
        log!("[mechanics] vrik: the game's FPS lock at {value} (the headset's cadence)\n");
    }
}

/// The mod's onUpdate (its pump).
pub(in crate::mechanics) fn tick(dt: f32) {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let dt = f64::from(dt);
    sync_game_fps_lock(&mut s, dt);
    s.neck_cap_timer += dt;
    if s.neck_cap_timer >= 0.25 {
        s.neck_cap_timer = 0.0;
        let _ = native::<i32>("VRNeckCapSync", &[]);
    }
    if let Err(e) = pump(&mut s, dt) {
        static LAST: Mutex<Option<String>> = Mutex::new(None);
        let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
        if last.as_deref() != Some(e.as_str()) {
            log!("[mechanics] vrik: {e}\n");
            *last = Some(e);
        }
    }
}
/// VRIKGameAttached: the player attached to the world (a save load, a spawn); a recenter 3.5 s later.
pub(in crate::mechanics) fn game_attached() {
    if native::<bool>("VRLobbyActive", &[]) == Ok(true) {
        return;
    }
    STATE.lock().unwrap_or_else(|p| p.into_inner()).recenter_pending = 3.5;
}
/// VRIKLogDiag: the LogVRDiag hotkey with the last camera pose.
pub(in crate::mechanics) fn log_diag() {
    let pose = STATE.lock().unwrap_or_else(|p| p.into_inner()).last_cam;
    let _ = native::<i32>("SetVRDiagCapture", &[Value::Int32(1)]);
    if let Some((p, q)) = pose {
        let _ = native::<i32>(
            "LogVRDiag",
            &[Value::Float(p[0]), Value::Float(p[1]), Value::Float(p[2]), Value::Float(q[0]), Value::Float(q[1]), Value::Float(q[2]), Value::Float(q[3])],
        );
    }
}
