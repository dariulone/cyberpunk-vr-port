//! RED4ext's plugin API v1 (`include/RED4ext/Api`): what the loader hands a plugin and asks of it.
use crate::assert_layout;
use core::ffi::c_void;

pub type PluginHandle = *mut c_void; // HMODULE

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrereleaseInfo {
    pub kind: u32,
    pub number: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SemVer {
    pub major: u8,
    pub minor: u16,
    pub patch: u32,
    pub prerelease: PrereleaseInfo,
}
assert_layout!(SemVer, size = 16, minor = 2, patch = 4, prerelease = 8);

impl SemVer {
    pub const fn new(major: u8, minor: u16, patch: u32) -> Self {
        SemVer { major, minor, patch, prerelease: PrereleaseInfo { kind: 0, number: 0 } }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileVer {
    pub major: u16,
    pub minor: u16,
    pub build: u16,
    pub revision: u16,
}
assert_layout!(FileVer, size = 8);

/// RED4EXT_V1_RUNTIME_VERSION_INDEPENDENT: the plugin does not tie itself to a game build through RED4ext; the port
/// checks the build itself (cvr-plugin's game_build) so an unknown build runs flat instead of not loading.
pub const RUNTIME_INDEPENDENT: FileVer = FileVer { major: 0xFFFF, minor: 0xFFFF, build: 0xFFFF, revision: 0xFFFF };
/// RED4EXT_V1_RUNTIME_VERSION_2_50.
pub const RUNTIME_2_50: FileVer = FileVer { major: 3, minor: 0, build: 88, revision: 46235 };
/// RED4EXT_V1_SDK_VERSION_1_0_0_COMPAT_0_5_0: what the RED4ext runtime installed with the port (1.29.1) accepts.
pub const SDK_VERSION_COMPAT_0_5_0: SemVer = SemVer::new(0, 5, 0);
/// RED4EXT_API_VERSION_1_COMPAT_0, the value `Supports` returns.
pub const API_VERSION_1_COMPAT_0: u32 = 0;

#[repr(C)]
pub struct PluginInfo {
    pub sdk: SemVer,
    pub name: *const u16,
    pub author: *const u16,
    pub version: SemVer,
    pub runtime: FileVer,
}
assert_layout!(PluginInfo, size = 56, name = 16, author = 24, version = 32, runtime = 48);

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainReason {
    Load = 0,
    Unload = 1,
}

/// RED4ext::EGameStateType.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameStateType {
    BaseInitialization = 0,
    Initialization = 1,
    Running = 2,
    Shutdown = 3,
}

/// RED4ext::CGameApplication, opaque here.
#[repr(C)]
pub struct GameApplication {
    _opaque: [u8; 0],
}

pub type GameStateFn = Option<unsafe extern "C" fn(app: *mut GameApplication) -> bool>;

/// RED4ext::v1::GameState. RED4ext copies it in `GameStates::Add`; it may live on the stack.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GameState {
    pub on_enter: GameStateFn,
    pub on_update: GameStateFn,
    pub on_exit: GameStateFn,
}
assert_layout!(GameState, size = 24);

#[repr(C)]
pub struct GameStates {
    pub add: unsafe extern "C" fn(PluginHandle, GameStateType, *mut GameState) -> bool,
}

#[repr(C)]
pub struct Hooking {
    pub attach: unsafe extern "C" fn(PluginHandle, *mut c_void, *mut c_void, *mut *mut c_void) -> bool,
    pub detach: unsafe extern "C" fn(PluginHandle, *mut c_void) -> bool,
}

#[repr(C)]
pub struct Scripts {
    pub add: unsafe extern "C" fn(PluginHandle, *const u16) -> bool,
    pub register_never_ref_type: unsafe extern "C" fn(*const u8) -> bool,
    pub register_mixed_ref_type: unsafe extern "C" fn(*const u8) -> bool,
}

type LogFn = unsafe extern "C" fn(PluginHandle, *const u8);
type LogFmtFn = unsafe extern "C" fn(PluginHandle, *const u8, ...);
type LogWFn = unsafe extern "C" fn(PluginHandle, *const u16);
type LogWFmtFn = unsafe extern "C" fn(PluginHandle, *const u16, ...);

/// RED4ext::v1::Logger: six levels, each as narrow, narrow-format, wide, wide-format.
#[repr(C)]
pub struct Logger {
    pub trace: LogFn,
    pub trace_f: LogFmtFn,
    pub trace_w: LogWFn,
    pub trace_wf: LogWFmtFn,
    pub debug: LogFn,
    pub debug_f: LogFmtFn,
    pub debug_w: LogWFn,
    pub debug_wf: LogWFmtFn,
    pub info: LogFn,
    pub info_f: LogFmtFn,
    pub info_w: LogWFn,
    pub info_wf: LogWFmtFn,
    pub warn: LogFn,
    pub warn_f: LogFmtFn,
    pub warn_w: LogWFn,
    pub warn_wf: LogWFmtFn,
    pub error: LogFn,
    pub error_f: LogFmtFn,
    pub error_w: LogWFn,
    pub error_wf: LogWFmtFn,
    pub critical: LogFn,
    pub critical_f: LogFmtFn,
    pub critical_w: LogWFn,
    pub critical_wf: LogWFmtFn,
}
assert_layout!(Logger, size = 24 * 8);

/// RED4ext::v1::Sdk, valid for the plugin's whole lifetime.
#[repr(C)]
pub struct Sdk {
    pub runtime: *const SemVer,
    pub logger: *const Logger,
    pub hooking: *const Hooking,
    pub game_states: *const GameStates,
    pub scripts: *const Scripts,
}
assert_layout!(Sdk, size = 40, logger = 8, hooking = 16, game_states = 24, scripts = 32);

/// A NUL-terminated UTF-16 string with static lifetime, for `PluginInfo` and `Scripts::add`.
#[macro_export]
macro_rules! wide {
    ($s:literal) => {{
        const S: &str = $s;
        const N: usize = S.len() + 1;
        const W: [u16; N] = {
            let bytes = S.as_bytes();
            let mut out = [0u16; N];
            let mut i = 0;
            while i < bytes.len() {
                assert!(bytes[i] < 0x80, "wide! takes ASCII");
                out[i] = bytes[i] as u16;
                i += 1;
            }
            out
        };
        &W
    }};
}
