//! The hook stages (formerly src/Hooks/Registry.cpp). Every hook of the plugin declares its own placement next to its
//! own code -- `CVR_HOOK("LodFov", cvr::hooks::Stage::Boot, 40, InstallFixLoDHook)` (include/Hooks/Hook.hpp), whose
//! static constructor registers it here -- and the boot installs a stage at a time: in `order`, one log line per hook,
//! the failures counted. The order is still a decision, but it is written where the hook is and auditable in one line
//! of log rather than by reading a boot function.
//!
//! Registration happens from the C++ half's static constructors, before anything else runs: the list is a const
//! static, so there is no initialisation order to lose a registration to.
//!
//! The walk order is the C++ registry's exactly: its list was pushed at the head, so it was walked newest first, and a
//! stable sort by `order` kept that among equal orders. The same hooks therefore install in the same sequence and the
//! `[hooks]` lines read the same.
use core::ffi::{CStr, c_char};
use cvr_host::log;
use std::sync::Mutex;

use crate::game_build;

/// WHEN a hook may be installed (cvr::hooks::Stage).
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Before the game creates its D3D12 device: anything that must observe creation itself.
    PreDevice = 0,
    /// The pattern-scan pass: the engine image is loaded and settled. Most hooks are here.
    Boot = 1,
    /// After the stereo module has claimed its addresses: anything that would otherwise race it for a site.
    PostStereo = 2,
}

impl Stage {
    pub fn from_raw(raw: i32) -> Option<Self> {
        match raw {
            0 => Some(Self::PreDevice),
            1 => Some(Self::Boot),
            2 => Some(Self::PostStereo),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::PreDevice => "pre-device",
            Self::Boot => "boot",
            Self::PostStereo => "post-stereo",
        }
    }
}

/// True on success; a hook that cannot find its pattern returns false rather than pretend.
pub type InstallFn = extern "C" fn() -> bool;
/// WANTED, which is not SUCCEEDED: a hook switched off is skipped and says so. None means always wanted.
pub type EnabledFn = extern "C" fn() -> bool;

struct Entry {
    name: &'static str,
    stage: i32,
    order: i32,
    install: Option<InstallFn>,
    enabled: Option<EnabledFn>,
    installed: bool,
}

static HOOKS: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

/// The install sequence among equal orders: the sequence the C++ registry's walk gave (newest registration first) when
/// every hook still registered from C++, kept now that some register from Rust (hook_table.rs) -- the same hooks
/// install in the same sequence and the `[hooks]` lines read the same. A hook not named here keeps the old rule.
const SEQUENCE: [&str; 52] = [
    "AftermathDiagnostics",
    "ViewKey",
    "JobTimeline",
    "LocateCamera",
    "PatchCamera",
    "FinalCamera",
    "CameraPitch",
    "CameraFov",
    "ProjectionCommit",
    "ScopeGraphDag",
    "VrcamGbufferPartitions",
    "ProjAspectCopy",
    "ProjAspectCall",
    "ProjStage",
    "RebindCensus",
    "LodFov",
    "LodFovThreshold",
    "BodyYawCensus",
    "MenuMode",
    "ForceHeadingUpdate",
    "OnFootDeltaHead",
    "RoomscaleMovement",
    "FreeDeltaHead",
    "OnFootMoveXY",
    "BodyHeading",
    "WeaponAim",
    "PatchBuffer",
    "CameraPoseTransport",
    "HudPanel",
    "WorldSpaceBlendCamera",
    "PhysicalOpticsSurface",
    "MenuSurface",
    "WorldMarkers",
    "PanzerSteering",
    "VendorRoute",
    "PromptIcons",
    "PanzerWeaponAim",
    "SettingsRes",
    "PhotoModeCapture",
    "BackgroundPad",
    "KeypadCursor",
    "WorldTouch",
    "SurveillanceBody",
    "StoryAttention",
    "NvidiaReflex",
    "DlssRes",
    "LeftHandedAnimation",
    "NativeSetterMetaWrite",
    "NativeSetterMetaConsume",
    "NativeSetterClear",
    "XInput",
    "SwimmingActions",
];
fn rank(name: &str) -> usize {
    SEQUENCE.iter().position(|n| *n == name).unwrap_or(usize::MAX)
}

fn hooks() -> std::sync::MutexGuard<'static, Vec<Entry>> {
    HOOKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One hook's registration (the Hook constructor).
pub fn register(
    name: &'static str,
    stage: i32,
    order: i32,
    install: Option<InstallFn>,
    enabled: Option<EnabledFn>,
) {
    hooks().push(Entry {
        name,
        stage,
        order,
        install,
        enabled,
        installed: false,
    });
}

/// What one stage's pass decided for a hook; the log lines are made from these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Skipped,
    Ok,
    Failed,
}

/// InstallStage without the build check: every registered, not yet installed hook of the stage, in the SEQUENCE among
/// equal orders (newest registration first for a hook it does not name), each reported to `report`. Returns the failures and the number due.
pub fn install_registered(
    stage: Stage,
    mut report: impl FnMut(&'static str, Outcome),
) -> (usize, usize) {
    // Collected first and called without the lock: an install function is the plugin's own code.
    let mut due: Vec<(
        usize,
        &'static str,
        i32,
        Option<InstallFn>,
        Option<EnabledFn>,
    )> = hooks()
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, h)| h.stage == stage as i32 && !h.installed)
        .map(|(i, h)| (i, h.name, h.order, h.install, h.enabled))
        .collect();
    due.sort_by_key(|&(_, name, order, _, _)| (order, rank(name)));
    let mut failed = 0;
    for &(index, name, _, install, enabled) in &due {
        if enabled.is_some_and(|wanted| !wanted()) {
            report(name, Outcome::Skipped);
            continue;
        }
        let ok = install.is_some_and(|install| install());
        hooks()[index].installed = ok;
        if !ok {
            failed += 1;
        }
        report(name, if ok { Outcome::Ok } else { Outcome::Failed });
    }
    (failed, due.len())
}

/// InstallStage: the stage's hooks, or a refusal on a game build these sites do not belong to. Returns the number that
/// failed (-1 when refused).
pub fn install_stage(stage: Stage) -> i32 {
    // Patch sites and prologues of one game build. Main never gets here on another; this is the same refusal for any
    // path that would.
    if !game_build::supported() {
        let id = game_build::identity();
        log!(
            "[hooks] {:<11} REFUSING to install: game {}, these sites are {}.\n",
            stage.name(),
            id.running,
            id.expected
        );
        return -1;
    }
    let (failed, due) = install_registered(stage, |name, outcome| match outcome {
        // Switched off is not broken: its own line, so a feature that is simply not wanted never reads as a hook that
        // stopped matching the game.
        Outcome::Skipped => log!(
            "[hooks] {:<11} {:<24} skipped (disabled)\n",
            stage.name(),
            name
        ),
        // EVERY result, not just the failures: a hook that stops matching after a game patch is the most common way
        // this mod breaks, and the symptom -- some one feature quietly gone -- is unreadable without this line.
        Outcome::Ok => log!("[hooks] {:<11} {:<24} ok\n", stage.name(), name),
        Outcome::Failed => log!("[hooks] {:<11} {:<24} FAILED\n", stage.name(), name),
    });
    if failed > 0 {
        log!(
            "[hooks] {}: {} of {} FAILED -- the features behind them are absent, not degraded.\n",
            stage.name(),
            failed,
            due
        );
    }
    failed as i32
}

/// (installed, registered), for the line that says what the mod actually managed to do.
pub fn installed_count() -> (usize, usize) {
    let hooks = hooks();
    (hooks.iter().filter(|h| h.installed).count(), hooks.len())
}

/// ReportInstalled.
pub fn report_installed() {
    let (ok, total) = installed_count();
    log!("[hooks] {} of {} installed.\n", ok, total);
}

/// The Hook constructor (include/Hooks/Hook.hpp). `name` is a string literal of the C++ half.
///
/// # Safety
/// `name` is null or a NUL-terminated string alive for the process.
#[no_mangle]
pub unsafe extern "C" fn cvr_hooks_register(
    name: *const c_char,
    stage: i32,
    order: i32,
    install: Option<InstallFn>,
    enabled: Option<EnabledFn>,
) {
    let name = if name.is_null() {
        "?"
    } else {
        // SAFETY: a string literal of the caller, alive for the process.
        let literal: &'static CStr = unsafe { CStr::from_ptr(name) };
        literal.to_str().unwrap_or("?")
    };
    register(name, stage, order, install, enabled);
}

/// cvr::hooks::InstallStage.
#[no_mangle]
pub extern "C" fn cvr_hooks_install_stage(stage: i32) -> i32 {
    match Stage::from_raw(stage) {
        Some(stage) => install_stage(stage),
        None => -1,
    }
}

/// cvr::hooks::ReportInstalled.
#[no_mangle]
pub extern "C" fn cvr_hooks_report_installed() {
    report_installed();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CALLS: AtomicUsize = AtomicUsize::new(0);
    extern "C" fn yes() -> bool {
        CALLS.fetch_add(1, Ordering::Relaxed);
        true
    }
    extern "C" fn no() -> bool {
        CALLS.fetch_add(1, Ordering::Relaxed);
        false
    }
    extern "C" fn off() -> bool {
        false
    }

    // One test owns the process-wide registry: the stage pass, its order, the skip, the failure and the counts.
    #[test]
    fn a_stage_installs_in_order_once() {
        register("late", Stage::Boot as i32, 50, Some(yes), None);
        register("first-a", Stage::Boot as i32, 10, Some(yes), None);
        register("first-b", Stage::Boot as i32, 10, Some(yes), None);
        register("broken", Stage::Boot as i32, 20, Some(no), None);
        register("unwanted", Stage::Boot as i32, 5, Some(yes), Some(off));
        register("no-install", Stage::Boot as i32, 30, None, None);
        register("other-stage", Stage::PostStereo as i32, 1, Some(yes), None);
        let mut seen = Vec::new();
        let (failed, due) =
            install_registered(Stage::Boot, |name, outcome| seen.push((name, outcome)));
        assert_eq!((failed, due), (2, 6));
        // Equal orders: the newest registration first, as the C++ list walked them.
        assert_eq!(
            seen,
            vec![
                ("unwanted", Outcome::Skipped),
                ("first-b", Outcome::Ok),
                ("first-a", Outcome::Ok),
                ("broken", Outcome::Failed),
                ("no-install", Outcome::Failed),
                ("late", Outcome::Ok),
            ]
        );
        assert_eq!(CALLS.load(Ordering::Relaxed), 4);
        assert_eq!(installed_count(), (3, 7));
        // A second pass retries only what is not installed: the skipped and the failed.
        let mut again = Vec::new();
        let (failed, due) =
            install_registered(Stage::Boot, |name, outcome| again.push((name, outcome)));
        assert_eq!((failed, due), (2, 3));
        assert_eq!(
            again.iter().map(|&(n, _)| n).collect::<Vec<_>>(),
            vec!["unwanted", "broken", "no-install"]
        );
        let (failed, _) = install_registered(Stage::PostStereo, |_, _| {});
        assert_eq!(failed, 0);
        assert_eq!(installed_count(), (4, 7));
    }
}
