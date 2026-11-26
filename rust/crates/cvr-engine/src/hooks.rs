//! The detour manager (stage 2). Every hook belongs to a CHAIN: the detours one feature needs to work at all. A
//! chain is installed whole or not at all -- a feature with half its detours live is the state that corrupts
//! frames or crashes later, far from the cause -- and the log says which, with the reason:
//!
//!   refused     the game is not the build the addresses belong to (game_build), or an RVA is one remap.py could
//!               not carry over, or the code at a site is not what the port was written against
//!   failed      MinHook could not create or enable a detour; everything this chain created is removed again
//!
//! MinHook is the pinned submodule compiled into this crate (build.rs); the C++ half calls the same instance.
use crate::code::{self, Pattern};
use crate::game_build;
use cvr_host::log;
use core::ffi::c_void;
use std::sync::Mutex;

#[allow(non_snake_case)]
mod mh {
    use core::ffi::c_void;
    pub const OK: i32 = 0;
    pub const ERROR_ALREADY_INITIALIZED: i32 = 1;
    extern "system" {
        pub fn MH_Initialize() -> i32;
        pub fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
        pub fn MH_RemoveHook(target: *mut c_void) -> i32;
        pub fn MH_QueueEnableHook(target: *mut c_void) -> i32;
        pub fn MH_QueueDisableHook(target: *mut c_void) -> i32;
        pub fn MH_ApplyQueued() -> i32;
        pub fn MH_StatusToString(status: i32) -> *const core::ffi::c_char;
    }
}

/// MinHook's name for a status ("MH_ERROR_NOT_EXECUTABLE", ...).
pub fn status_name(status: i32) -> &'static str {
    // SAFETY: MH_StatusToString returns a static string for every value.
    unsafe {
        let p = mh::MH_StatusToString(status);
        if p.is_null() {
            return "?";
        }
        core::ffi::CStr::from_ptr(p).to_str().unwrap_or("?")
    }
}

/// One detour of a chain.
#[derive(Clone, Copy, Debug)]
pub struct Detour {
    pub name: &'static str,
    /// The function's RVA in Cyberpunk2077.exe, or an absolute address with `absolute`.
    pub target: usize,
    pub absolute: bool,
    pub detour: *const c_void,
    /// Where the trampoline to the original function is written (the detour calls through it).
    pub original: *mut *mut c_void,
    /// The bytes expected at the target before it is touched ("48 89 5C 24 ?? 57").
    pub expect: Option<&'static str>,
}

// The pointers are code and static slots, the same for every thread.
unsafe impl Send for Detour {}
unsafe impl Sync for Detour {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookError {
    UnknownBuild,
    Unmapped { detour: &'static str, rva: usize },
    BadPattern { detour: &'static str },
    CodeMismatch { detour: &'static str, address: usize },
    Create { detour: &'static str, status: i32 },
    Apply { status: i32 },
    Initialize { status: i32 },
}

impl core::fmt::Display for HookError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HookError::UnknownBuild => write!(f, "refused: not the game build the port's addresses belong to"),
            HookError::Unmapped { detour, rva } => write!(f, "refused: {detour} at {rva:#x} was not carried to this build"),
            HookError::BadPattern { detour } => write!(f, "refused: {detour} has an unreadable expected-bytes pattern"),
            HookError::CodeMismatch { detour, address } => {
                write!(f, "refused: the code at {detour} ({address:#x}) is not what the port was written against")
            }
            HookError::Create { detour, status } => write!(f, "failed: {detour}: {}", status_name(*status)),
            HookError::Apply { status } => write!(f, "failed: enabling the chain: {}", status_name(*status)),
            HookError::Initialize { status } => write!(f, "failed: MinHook: {}", status_name(*status)),
        }
    }
}

struct Installed {
    chain: &'static str,
    targets: Vec<usize>,
}

static INSTALLED: Mutex<Vec<Installed>> = Mutex::new(Vec::new());
/// MinHook's queue is global: one chain is applied at a time.
static APPLY: Mutex<()> = Mutex::new(());

fn initialize() -> Result<(), HookError> {
    // SAFETY: MinHook's own initialisation; a second call reports ALREADY_INITIALIZED.
    match unsafe { mh::MH_Initialize() } {
        mh::OK | mh::ERROR_ALREADY_INITIALIZED => Ok(()),
        status => Err(HookError::Initialize { status }),
    }
}

/// Installs `detours` as one chain, after checking the build gate.
///
/// # Safety
/// Every detour must have the signature of its target, and `original` must point to a slot the detour reads its
/// trampoline from.
pub unsafe fn install_chain(chain: &'static str, detours: &[Detour]) -> Result<(), HookError> {
    let result = if !game_build::supported() {
        Err(HookError::UnknownBuild)
    } else {
        // SAFETY: forwarded.
        unsafe { install_chain_on_any_build(chain, detours) }
    };
    match &result {
        Ok(()) => log!("[hooks] chain {chain}: {} detour(s) installed\n", detours.len()),
        Err(e) => log!("[hooks] chain {chain}: {e}; nothing of it is installed\n"),
    }
    result
}

/// The chain without the build gate: for code that is not the game's (tests, a module's own exports).
///
/// # Safety
/// As `install_chain`.
pub unsafe fn install_chain_on_any_build(chain: &'static str, detours: &[Detour]) -> Result<(), HookError> {
    initialize()?;
    // Resolve and check everything before touching anything.
    let mut targets = Vec::with_capacity(detours.len());
    for d in detours {
        if !d.absolute && game_build::unmapped(d.target) {
            return Err(HookError::Unmapped { detour: d.name, rva: d.target });
        }
        let address = if d.absolute { d.target } else { code::rva(d.target) };
        if let Some(text) = d.expect {
            let pattern = Pattern::parse(text).ok_or(HookError::BadPattern { detour: d.name })?;
            if !code::expect(address, &pattern) {
                return Err(HookError::CodeMismatch { detour: d.name, address });
            }
        }
        targets.push(address);
    }
    let _apply = APPLY.lock().unwrap_or_else(|p| p.into_inner());
    let mut created: Vec<usize> = Vec::with_capacity(targets.len());
    let rollback = |created: &[usize]| {
        for &t in created {
            // SAFETY: each was created by this call and is not enabled.
            unsafe { mh::MH_RemoveHook(t as *mut c_void) };
        }
    };
    for (d, &address) in detours.iter().zip(&targets) {
        // SAFETY: the caller vouches for the detour's signature and the slot.
        let status = unsafe { mh::MH_CreateHook(address as *mut c_void, d.detour as *mut c_void, d.original) };
        if status != mh::OK {
            rollback(&created);
            return Err(HookError::Create { detour: d.name, status });
        }
        created.push(address);
    }
    for &t in &created {
        // SAFETY: created above.
        let status = unsafe { mh::MH_QueueEnableHook(t as *mut c_void) };
        if status != mh::OK {
            for &u in &created {
                unsafe { mh::MH_QueueDisableHook(u as *mut c_void) };
            }
            rollback(&created);
            return Err(HookError::Apply { status });
        }
    }
    // One ApplyQueued: every thread suspended once, every detour of the chain switched on together.
    // SAFETY: MinHook's queue holds only this chain's hooks (APPLY is held).
    let status = unsafe { mh::MH_ApplyQueued() };
    if status != mh::OK {
        for &t in &created {
            unsafe { mh::MH_QueueDisableHook(t as *mut c_void) };
        }
        unsafe { mh::MH_ApplyQueued() };
        rollback(&created);
        return Err(HookError::Apply { status });
    }
    INSTALLED.lock().unwrap_or_else(|p| p.into_inner()).push(Installed { chain, targets: created });
    Ok(())
}

/// Removes one installed chain (all its detours, together).
pub fn uninstall_chain(chain: &str) -> bool {
    let removed = {
        let mut list = INSTALLED.lock().unwrap_or_else(|p| p.into_inner());
        let Some(i) = list.iter().position(|c| c.chain == chain) else { return false };
        list.remove(i)
    };
    let _apply = APPLY.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: these targets were created and enabled by install_chain.
    unsafe {
        for &t in &removed.targets {
            mh::MH_QueueDisableHook(t as *mut c_void);
        }
        mh::MH_ApplyQueued();
        for &t in &removed.targets {
            mh::MH_RemoveHook(t as *mut c_void);
        }
    }
    true
}

/// One hook removed by its target, whichever chain made it (the loader's plugin `detach`).
///
/// # Safety
/// No thread may be inside the detour's trampoline call when it is removed.
pub unsafe fn remove_one(target: *mut c_void) -> bool {
    {
        let mut list = INSTALLED.lock().unwrap_or_else(|p| p.into_inner());
        for c in list.iter_mut() {
            c.targets.retain(|&t| t != target as usize);
        }
        list.retain(|c| !c.targets.is_empty());
    }
    let _apply = APPLY.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: MinHook ignores a target it does not know (an error status).
    unsafe {
        mh::MH_QueueDisableHook(target);
        mh::MH_ApplyQueued();
        mh::MH_RemoveHook(target) == mh::OK
    }
}

/// The chains installed so far, for the one log line that says what the port managed to do.
pub fn installed() -> Vec<(&'static str, usize)> {
    INSTALLED.lock().unwrap_or_else(|p| p.into_inner()).iter().map(|c| (c.chain, c.targets.len())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicPtr, Ordering};
    use std::hint::black_box;

    // Targets with real bodies, never inlined: MinHook needs whole instructions to relocate.
    #[inline(never)]
    extern "C" fn add_one(x: i64) -> i64 {
        let mut v = black_box(x);
        for _ in 0..black_box(1) {
            v = black_box(v + 1);
        }
        v
    }
    #[inline(never)]
    extern "C" fn add_two(x: i64) -> i64 {
        let mut v = black_box(x);
        for _ in 0..black_box(2) {
            v = black_box(v + 1);
        }
        v
    }
    #[inline(never)]
    extern "C" fn add_three(x: i64) -> i64 {
        let mut v = black_box(x);
        for _ in 0..black_box(3) {
            v = black_box(v + 1);
        }
        v
    }

    static ORIG_ONE: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
    static ORIG_TWO: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
    static ORIG_THREE: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

    extern "C" fn detour_one(x: i64) -> i64 {
        let original: extern "C" fn(i64) -> i64 = unsafe { core::mem::transmute(ORIG_ONE.load(Ordering::Acquire)) };
        original(x) * 1000
    }
    extern "C" fn detour_two(x: i64) -> i64 {
        let original: extern "C" fn(i64) -> i64 = unsafe { core::mem::transmute(ORIG_TWO.load(Ordering::Acquire)) };
        original(x) * 1000
    }
    extern "C" fn detour_three(x: i64) -> i64 {
        x * 7
    }

    fn detour(name: &'static str, target: extern "C" fn(i64) -> i64, detour: extern "C" fn(i64) -> i64,
        original: &AtomicPtr<c_void>, expect: Option<&'static str>) -> Detour {
        Detour {
            name,
            target: target as usize,
            absolute: true,
            detour: detour as *const c_void,
            original: original.as_ptr(),
            expect,
        }
    }

    /// One test, in order: MinHook's state is process-wide.
    #[test]
    fn chains_are_all_or_nothing() {
        let call = |f: extern "C" fn(i64) -> i64, x| black_box(f)(black_box(x));
        assert_eq!((call(add_one, 1), call(add_two, 1), call(add_three, 1)), (2, 3, 4));

        // The build gate: this test exe is not the game.
        let d1 = detour("one", add_one, detour_one, &ORIG_ONE, None);
        if !game_build::supported() {
            assert_eq!(unsafe { install_chain("gated", &[d1]) }, Err(HookError::UnknownBuild));
            assert_eq!(call(add_one, 1), 2, "a refused chain changed code");
        }

        // A chain whose second detour expects other code: refused before anything is created.
        let bad = detour("two", add_two, detour_two, &ORIG_TWO, Some("CC CC CC CC CC CC CC CC"));
        let r = unsafe { install_chain_on_any_build("mismatch", &[d1, bad]) };
        assert!(matches!(r, Err(HookError::CodeMismatch { detour: "two", .. })), "{r:?}");
        assert_eq!((call(add_one, 1), call(add_two, 1)), (2, 3), "half a chain was left installed");

        // A chain whose second detour cannot be created (its target is already hooked by another chain): the first
        // detour, already created, is removed again.
        let three = detour("three", add_three, detour_three, &ORIG_THREE, None);
        unsafe { install_chain_on_any_build("holder", &[three]) }.unwrap();
        assert_eq!(call(add_three, 1), 7);
        let clash = detour("three-again", add_three, detour_three, &ORIG_THREE, None);
        let r = unsafe { install_chain_on_any_build("clash", &[d1, clash]) };
        assert!(matches!(r, Err(HookError::Create { detour: "three-again", .. })), "{r:?}");
        assert_eq!(call(add_one, 1), 2, "the created first detour was not rolled back");

        // A good chain: both detours live together, both trampolines work.
        let good = [d1, detour("two", add_two, detour_two, &ORIG_TWO, None)];
        unsafe { install_chain_on_any_build("good", &good) }.unwrap();
        assert_eq!((call(add_one, 1), call(add_two, 1)), (2000, 3000));
        assert!(installed().contains(&("good", 2)));

        // And removed together.
        assert!(uninstall_chain("good") && uninstall_chain("holder"));
        assert_eq!((call(add_one, 1), call(add_two, 1), call(add_three, 1)), (2, 3, 4));
        assert!(!uninstall_chain("good"));
    }
}
