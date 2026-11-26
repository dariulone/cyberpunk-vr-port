//! The stereo module's engine detours (formerly the registry and install pass of src/Stereo/DetourRegistry.cpp). Each
//! detour is declared next to its own code -- `CVR_DETOUR("[build] full-build sub_141D43040", FULL_BUILD_RVA,
//! Detour_FullBuild, g_orig_full_build)` (include/Stereo/DetourRegistry.hpp), whose static constructor registers it
//! here -- and the install pass is one loop that neither knows nor needs to know how many there are.
//!
//! These are MinHook detours at a known RVA, not the pattern-scanned patch sites of the hook stages (`stages`): an RVA
//! that no longer holds the expected function is a game-patch problem, a pattern that no longer matches is another,
//! and the two registries keep them apart.
//!
//! The pass refuses outright before the module base is resolved (every detour would land at offset zero) and on a game
//! build the RVAs do not belong to. It installs in ascending RVA, so the log reads in the order of a disassembly of the
//! image; among equal RVAs, the newest registration first, as the C++ list walked them. An RVA patch-day's remap could
//! not carry over is skipped and counted as failed. Every result is one line with the address.
use core::ffi::{CStr, c_char, c_void};
use std::sync::Mutex;

use crate::game_build;

#[allow(non_snake_case)]
mod mh {
    use core::ffi::c_void;
    pub const OK: i32 = 0;
    pub const ERROR_ALREADY_INITIALIZED: i32 = 1;
    extern "system" {
        pub fn MH_Initialize() -> i32;
        pub fn MH_CreateHook(
            target: *mut c_void,
            detour: *mut c_void,
            original: *mut *mut c_void,
        ) -> i32;
        pub fn MH_EnableHook(target: *mut c_void) -> i32;
    }
}

/// Whether a detour is wanted at all: an opt-in path nobody asked for is SKIPPED, not failed. None means always.
pub type WantedFn = extern "C" fn() -> bool;

struct Entry {
    name: &'static str,
    rva: usize,
    detour: usize,
    original: usize,
    wanted: Option<WantedFn>,
    /// The C++ object's `installed` field, which other files read (NativeDefinitionTracking.cpp); null in tests.
    installed_flag: usize,
    installed: bool,
}

static DETOURS: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

fn detours() -> std::sync::MutexGuard<'static, Vec<Entry>> {
    DETOURS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One detour's registration (the EngineDetour constructor).
pub fn register(
    name: &'static str,
    rva: usize,
    detour: *mut c_void,
    original: *mut *mut c_void,
    wanted: Option<WantedFn>,
    installed_flag: *mut bool,
) {
    detours().push(Entry {
        name,
        rva,
        detour: detour as usize,
        original: original as usize,
        wanted,
        installed_flag: installed_flag as usize,
        installed: false,
    });
}

/// What the pass decided for one detour; the log lines are made from these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Disabled,
    Unmapped,
    Ok(usize),
    Failed(usize),
}

/// The loop of InstallEngineDetours: every registered detour not yet installed, by ascending RVA, through `create`
/// (MinHook's create and enable in the plugin). Returns (failed, due).
pub fn install_registered(
    base: usize,
    unmapped: impl Fn(usize) -> bool,
    mut create: impl FnMut(usize, usize, usize) -> bool,
    mut report: impl FnMut(&'static str, usize, Outcome),
) -> (usize, usize) {
    // Collected first and called without the lock: a wanted function is the plugin's own code.
    let mut due: Vec<(usize, &'static str, usize, usize, usize, Option<WantedFn>)> = detours()
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, d)| !d.installed)
        .map(|(i, d)| (i, d.name, d.rva, d.detour, d.original, d.wanted))
        .collect();
    due.sort_by_key(|&(_, _, rva, _, _, _)| rva);
    let mut failed = 0;
    for &(index, name, rva, detour, original, wanted) in &due {
        if wanted.is_some_and(|wanted| !wanted()) {
            report(name, rva, Outcome::Disabled);
            continue;
        }
        // Left at its old value by tools/patchday/remap.py: hooking it would hook a stranger.
        if unmapped(rva) {
            report(name, rva, Outcome::Unmapped);
            failed += 1;
            continue;
        }
        let target = base + rva;
        let ok = create(target, detour, original);
        {
            let mut all = detours();
            let entry = &mut all[index];
            entry.installed = ok;
            if entry.installed_flag != 0 {
                // SAFETY: the C++ object's own field, alive for the process.
                unsafe { *(entry.installed_flag as *mut bool) = ok };
            }
        }
        if !ok {
            failed += 1;
        }
        report(
            name,
            rva,
            if ok {
                Outcome::Ok(target)
            } else {
                Outcome::Failed(target)
            },
        );
    }
    (failed, due.len())
}

/// Whether the detour at `rva` is installed (the C++ pass's instance-witness mask and the source selectors read it).
pub fn installed_at(rva: usize) -> bool {
    detours().iter().any(|d| d.rva == rva && d.installed)
}

fn line(log: extern "C" fn(*const c_char), text: String) {
    let mut bytes = text.into_bytes();
    bytes.retain(|&b| b != 0);
    bytes.push(0);
    log(bytes.as_ptr() as *const c_char);
}

/// InstallEngineDetours: the checks, then `prepare` (the C++ half's code guards and observers, which must run before
/// any detour is live), then the pass. `log` is `cvr::log("%s", text)` (the stereo log: "[stereo] " before, a newline
/// after, its switch and rate cap). Returns the failures, or -1 when nothing was installed.
pub fn install(
    base: usize,
    log: extern "C" fn(*const c_char),
    prepare: Option<extern "C" fn()>,
) -> i32 {
    // THE MODULE BASE IS CHECKED, NOT ASSUMED. Reaching here before it is resolved would put every detour at offset
    // zero -- MinHook would hook whatever is there, and the symptom would be a crash with nothing pointing at the cause.
    if base == 0 {
        line(
            log,
            "[detour] REFUSING to install: module base not resolved yet.".into(),
        );
        return -1;
    }
    // Every RVA belongs to one game build; on another, each is the middle of some unrelated function.
    if !game_build::supported() {
        let id = game_build::identity();
        line(
            log,
            format!(
                "[detour] REFUSING to install: game {}, these addresses are {}.",
                id.running, id.expected
            ),
        );
        return -1;
    }
    // SAFETY: MinHook's own initialisation, idempotent.
    let status = unsafe { mh::MH_Initialize() };
    if status != mh::OK && status != mh::ERROR_ALREADY_INITIALIZED {
        line(
            log,
            format!("[detour] MH_Initialize failed: {status} -- no detour installed."),
        );
        return -1;
    }
    if let Some(prepare) = prepare {
        prepare();
    }
    let (failed, due) = install_registered(
        base,
        game_build::unmapped,
        |target, detour, original| {
            // SAFETY: an engine function of the supported build, the plugin's detour and its trampoline slot.
            unsafe {
                mh::MH_CreateHook(
                    target as *mut c_void,
                    detour as *mut c_void,
                    original as *mut *mut c_void,
                ) == mh::OK
                    && mh::MH_EnableHook(target as *mut c_void) == mh::OK
            }
        },
        |name, rva, outcome| {
            // EVERY result, with the address: an RVA that no longer holds the expected function is a game-patch
            // problem and this line is the whole diagnosis.
            let text = match outcome {
                Outcome::Disabled => format!("[detour] {name:<46} skipped (disabled)"),
                Outcome::Unmapped => {
                    format!(
                        "[detour] {name:<46} skipped (RVA {} not carried to this game build)",
                        alternate_hex(rva)
                    )
                }
                Outcome::Ok(target) => format!("[detour] {name:<46} ok @{target:016X}"),
                Outcome::Failed(target) => format!("[detour] {name:<46} FAILED @{target:016X}"),
            };
            line(log, text);
        },
    );
    if failed > 0 {
        line(
            log,
            format!(
                "[detour] {failed} of {due} FAILED -- the behaviour behind them is absent, not degraded."
            ),
        );
    }
    failed as i32
}

/// printf's `%#zx`: "0x" before a nonzero value, a bare "0" for zero.
fn alternate_hex(value: usize) -> String {
    if value == 0 {
        "0".into()
    } else {
        format!("{value:#x}")
    }
}

/// The EngineDetour constructor (include/Stereo/DetourRegistry.hpp).
///
/// # Safety
/// `name` is null or a NUL-terminated string alive for the process; `installed` is null or the registering object's
/// field, alive for the process.
#[no_mangle]
pub unsafe extern "C" fn cvr_detours_register(
    name: *const c_char,
    rva: usize,
    detour: *mut c_void,
    original: *mut *mut c_void,
    wanted: Option<WantedFn>,
    installed: *mut bool,
) {
    let name = if name.is_null() {
        "?"
    } else {
        // SAFETY: a string literal of the caller, alive for the process.
        let literal: &'static CStr = unsafe { CStr::from_ptr(name) };
        literal.to_str().unwrap_or("?")
    };
    register(name, rva, detour, original, wanted, installed);
}

/// cvr::detail::InstallEngineDetours' checks and pass.
#[no_mangle]
pub extern "C" fn cvr_detours_install(
    base: usize,
    log: extern "C" fn(*const c_char),
    prepare: Option<extern "C" fn()>,
) -> i32 {
    install(base, log, prepare)
}

/// Whether the detour at `rva` is installed.
#[no_mangle]
pub extern "C" fn cvr_detours_installed_at(rva: usize) -> bool {
    installed_at(rva)
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn off() -> bool {
        false
    }

    // One test owns the process-wide registry: the order by RVA, the skips, the failures, the installed flags.
    #[test]
    fn the_pass_installs_by_rva() {
        let mut flags = Box::new([false; 6]);
        let f = flags.as_mut_ptr();
        let none = core::ptr::null_mut();
        // SAFETY: the flags outlive the registry's use of them in this test.
        unsafe {
            register("[c] third", 0x300, 3 as *mut c_void, none, None, f.add(0));
            register("[a] first", 0x100, 1 as *mut c_void, none, None, f.add(1));
            register(
                "[s] same-rva-older",
                0x200,
                2 as *mut c_void,
                none,
                None,
                f.add(2),
            );
            register(
                "[s] same-rva-newer",
                0x200,
                22 as *mut c_void,
                none,
                None,
                f.add(3),
            );
            register(
                "[o] unwanted",
                0x050,
                5 as *mut c_void,
                none,
                Some(off),
                f.add(4),
            );
            register(
                "[u] unmapped",
                0x400,
                4 as *mut c_void,
                none,
                None,
                f.add(5),
            );
        }
        let mut seen = Vec::new();
        let (failed, due) = install_registered(
            0x1_4000_0000,
            |rva| rva == 0x400,
            |target, detour, _| target != 0x1_4000_0300 || detour != 3,
            |name, _, outcome| seen.push((name, outcome)),
        );
        assert_eq!((failed, due), (2, 6));
        assert_eq!(
            seen,
            vec![
                ("[o] unwanted", Outcome::Disabled),
                ("[a] first", Outcome::Ok(0x1_4000_0100)),
                ("[s] same-rva-newer", Outcome::Ok(0x1_4000_0200)),
                ("[s] same-rva-older", Outcome::Ok(0x1_4000_0200)),
                ("[c] third", Outcome::Failed(0x1_4000_0300)),
                ("[u] unmapped", Outcome::Unmapped),
            ]
        );
        assert_eq!(*flags, [false, true, true, true, false, false]);
        assert!(
            installed_at(0x100)
                && installed_at(0x200)
                && !installed_at(0x300)
                && !installed_at(0x050)
        );
        // A second pass retries only what is not installed.
        let mut again = Vec::new();
        let (_, due) = install_registered(
            0x1_4000_0000,
            |_| false,
            |_, _, _| true,
            |name, _, _| again.push(name),
        );
        assert_eq!(due, 3);
        assert_eq!(again, vec!["[o] unwanted", "[c] third", "[u] unmapped"]);
        assert_eq!(flags[0], true);
        assert_eq!(alternate_hex(0), "0");
        assert_eq!(alternate_hex(0x3A9C10), "0x3a9c10");
        assert_eq!(
            format!("{:016X}", 0x7FF7_D259_D6F4usize),
            "00007FF7D259D6F4"
        );
    }
}
