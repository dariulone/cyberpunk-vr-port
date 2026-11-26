//! FIRST LAUNCH: the game settings this port was tuned against, and the borderless window of every launch (formerly
//! src/Core/FirstLaunch.cpp; that file keeps the C++ build's copy and the Rust build's seams).
//!
//! Cyberpunk's own settings are a single JSON under %LOCALAPPDATA%\CD Projekt Red\Cyberpunk 2077\UserSettings.json,
//! and what a fresh install puts there is shaped for a monitor. So the port ships the settings it was developed and
//! measured against and installs them ONCE: first_launch in vrport.ini reads the way it is named -- 1 means "do it" --
//! and it is CLEARED to 0 only after the merge has succeeded, so a failure retries next launch. From then on the file
//! belongs to the player. Called from the plugin's load, before the game creates its D3D12 device.
//!
//! Every launch starts in the game's own borderless window mode (/video/display WindowMode = BorderlessWindowed, list
//! index 1): the monitor shows the headset's picture in a window the port fits, and a framed window puts a caption and
//! borders round it.
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use cvr_host::log;
use cvr_settings::game_settings;

/// What the C++ half hands over: the live controls' first_launch and their preparation (InitRuntimePaths,
/// EnsureLiveControlFileExists, PollLiveControls).
#[repr(C)]
pub struct Runtime {
    pub first_launch: *mut i32,
    pub prepare: Option<unsafe extern "C" fn()>,
}

/// Rewrites the one key in vrport.ini: PersistLiveControlsUiState rewrites the whole file, but only when the overlay
/// saves; this has to survive a launch where the player never opens it.
fn write_flag(path: &Path, value: i32) -> bool {
    let raw = std::fs::read(path).unwrap_or_default();
    std::fs::write(path, cvr_settings::first_launch::rewrite_flag(&raw, value)).is_ok()
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().collect()
}

fn exists(path: &Path) -> bool {
    let z: Vec<u16> = wide(path).into_iter().chain(Some(0)).collect();
    // SAFETY: a NUL-terminated path.
    unsafe {
        windows_sys::Win32::Storage::FileSystem::GetFileAttributesW(z.as_ptr())
            != windows_sys::Win32::Storage::FileSystem::INVALID_FILE_ATTRIBUTES
    }
}

/// %LOCALAPPDATA%\CD Projekt Red\Cyberpunk 2077\UserSettings.json, or None without LOCALAPPDATA (or one too long).
fn player_settings() -> Option<PathBuf> {
    let file = game_settings::player_settings_file();
    (!file.is_empty()).then(|| PathBuf::from(OsString::from_wide(&file)))
}

/// ApplyLaunchWindowMode: the game's borderless window, written before the game reads its settings.
#[no_mangle]
pub extern "C" fn cvr_launch_window_mode_apply() {
    let Some(player) = player_settings() else {
        return;
    };
    if !exists(&player) {
        return; // the game writes it first
    }
    let set = game_settings::set(
        &wide(&player),
        b"/video/display",
        b"WindowMode",
        b"BorderlessWindowed",
        1,
    );
    if !set.ok {
        log!("WindowMode: {} -- left as it is\n", set.error);
    } else if set.changed {
        log!(
            "WindowMode: {} -> BorderlessWindowed for this launch\n",
            String::from_utf8_lossy(&set.previous)
        );
    }
}

/// ApplyFirstLaunchGameSettings: the shipped preset merged into the player's settings, once.
///
/// # Safety
/// `runtime` holds the C++ half's live first_launch and their preparation.
#[no_mangle]
pub unsafe extern "C" fn cvr_first_launch_apply(runtime: *const Runtime) {
    // SAFETY: as the caller vouches.
    let Some(rt) = (unsafe { runtime.as_ref() }) else {
        return;
    };
    if let Some(prepare) = rt.prepare {
        // SAFETY: the C++ half's own preparation.
        unsafe { prepare() };
    }
    // Once installed, game settings belong to the player.
    // SAFETY: the live controls' field.
    if rt.first_launch.is_null() || unsafe { core::ptr::read_volatile(rt.first_launch) } == 0 {
        return;
    }
    // The shipped copy sits in the plugin's config folder.
    let src = cvr_host::paths::file(cvr_host::paths::Folder::Config, "UserSettings.json");
    if !exists(&src) {
        log!(
            "FirstLaunch: no shipped UserSettings.json in the plugin's config folder ({}) -- nothing to do, leaving first_launch=1\n",
            src.display()
        );
        return;
    }
    let Some(dst) = player_settings() else {
        log!("FirstLaunch: LOCALAPPDATA unavailable or too long -- settings not installed\n");
        return;
    };
    // Absent means the game has never written its settings here.
    if !exists(&dst) {
        log!(
            "FirstLaunch: {} does not exist yet -- run the game once, then this applies\n",
            dst.display()
        );
        return;
    }
    let installed = game_settings::install(&wide(&src), &wide(&dst));
    if !installed.ok {
        log!(
            "FirstLaunch: {} -- leaving first_launch=1\n",
            installed.error
        );
        return;
    }
    // Clear the one-shot flag only after successful installation.
    let flagged = write_flag(
        &cvr_host::paths::file(cvr_host::paths::Folder::Config, "vrport.ini"),
        0,
    );
    if flagged {
        // SAFETY: as above.
        unsafe { core::ptr::write_volatile(rt.first_launch, 0) };
    }
    let backup = if installed.backup.is_empty() {
        "(already matched)".to_owned()
    } else {
        OsString::from_wide(&installed.backup)
            .to_string_lossy()
            .into_owned()
    };
    log!(
        "FirstLaunch: merged {} VR game settings; personal settings preserved\n             from {}\n             to   {}\n             previous settings kept at {}\n             first_launch=0 {} -- the game may need one more launch to read them\n",
        installed.changed,
        src.display(),
        dst.display(),
        backup,
        if flagged { "written to vrport.ini" } else { "COULD NOT BE WRITTEN (will retry)" }
    );
    for skipped in &installed.skipped {
        log!(
            "FirstLaunch: preset's {} not applied (not in this game build, or a value it would not read)\n",
            String::from_utf8_lossy(skipped)
        );
    }
}
