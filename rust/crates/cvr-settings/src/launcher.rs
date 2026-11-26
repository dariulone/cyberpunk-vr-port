//! What the launcher decided (formerly LoadLauncherConfig and SaveLauncherConfig in src/Core/LauncherConfig.cpp): one
//! eye's render size, the headset kind, the debug switch, whether the launcher shows and how long it waits, in
//! config\vrport-launcher.ini. Read through the C runtime as the C++ half read it -- `_fsopen`, `fgets` into 128 bytes,
//! `sscanf_s` with and without blanks around '=' -- and written the same way.
//!
//! The values stay the C++ half's globals (g_launcherWidth ... in Core/VrCore.cpp, which the rest of the plugin reads);
//! they and the file's path are attached here.
use core::ffi::{CStr, c_char};
use core::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

use crate::crt;

/// The launcher's values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Launcher {
    pub width: i32,
    pub height: i32,
    pub hmd_type: i32,
    pub debug: i32,
    pub show: i32,
    pub delay_ms: i32,
    /// 1: width x height was typed or slid in the launcher, not a ladder entry -- kept as it is (engine stereo).
    pub custom_res: i32,
    /// 0: the launcher chose the desktop game; the plugin declines to load.
    pub vr_enabled: i32,
    /// cvr::MonitorOutput: 0 the game's output, 1 the left eye, 2 the right eye.
    pub monitor_output: i32,
    /// The launcher's weapon-plane pick, until the live ini has saved it; -1 uses xr_weapon_plane_as_scene.
    pub weapon_plane: i32,
}

/// Before the file says otherwise.
pub const DEFAULTS: Launcher = Launcher {
    width: 2048,
    height: 2048,
    hmd_type: 0,
    debug: 0,
    show: 1,
    delay_ms: 1000,
    custom_res: 0,
    vr_enabled: 1,
    monitor_output: 0,
    weapon_plane: -1,
};

/// g_launcherVrEnabled, cvr::GetMonitorOutput, g_pendingLauncherWeaponPlane, g_launcherCustomRes (main's VrCore.cpp and
/// LauncherConfig.cpp): Rust's own.
pub static VR_ENABLED: AtomicI32 = AtomicI32::new(1);
pub static MONITOR_OUTPUT: AtomicI32 = AtomicI32::new(0);
pub static PENDING_WEAPON_PLANE: AtomicI32 = AtomicI32::new(-1);
pub static CUSTOM_RES: AtomicI32 = AtomicI32::new(0);

/// cvr::ParseMonitorOutput: missing and unrecognized values use the game's output.
pub fn parse_monitor_output(value: i32) -> i32 {
    if value == 1 || value == 2 { value } else { 0 }
}

/// `sscanf_s(line, "key=%d")` or `"key = %d"`, as the C++ loop tried both.
///
/// # Safety
/// `line` is a C string.
unsafe fn key(line: *const c_char, tight: &CStr, spaced: &CStr) -> Option<i32> {
    let mut v = 0i32;
    // SAFETY: the caller's C string and an int slot.
    unsafe {
        if crt::scan(line, tight, &[crt::ptr(&mut v)]) == 1
            || crt::scan(line, spaced, &[crt::ptr(&mut v)]) == 1
        {
            return Some(v);
        }
    }
    None
}

/// One line of the file into `l`; the first key that matches takes the line.
///
/// # Safety
/// `line` is a C string.
pub unsafe fn parse_line(line: *const c_char, l: &mut Launcher) {
    // SAFETY: the caller's C string.
    unsafe {
        let (mut v, mut trailing) = (0i32, 0 as c_char);
        // "monitor_output = %d %c" assigning one field: the number and nothing after it.
        if crt::scan(line, c"monitor_output = %d %c", &[crt::ptr(&mut v), crt::ptr(&mut trailing), 1usize as *mut core::ffi::c_void]) == 1 {
            l.monitor_output = parse_monitor_output(v);
        } else if crt::scan(line, c"vr_enabled = %d", &[crt::ptr(&mut v)]) == 1 {
            l.vr_enabled = (v != 0) as i32;
        } else if crt::scan(line, c"weapon_plane = %d", &[crt::ptr(&mut v)]) == 1 {
            l.weapon_plane = (v != 0) as i32;
        } else if let Some(v) = key(line, c"launcher_delay_ms=%d", c"launcher_delay_ms = %d") {
            l.delay_ms = v.clamp(0, 10000);
        } else if let Some(v) = key(line, c"show_launcher=%d", c"show_launcher = %d") {
            l.show = (v != 0) as i32;
        } else if let Some(v) = key(line, c"width=%d", c"width = %d") {
            if v > 0 {
                l.width = v;
            }
        } else if let Some(v) = key(line, c"height=%d", c"height = %d") {
            if v > 0 {
                l.height = v;
            }
        } else if let Some(v) = key(line, c"hmd_type=%d", c"hmd_type = %d") {
            l.hmd_type = v;
        } else if let Some(v) = key(line, c"custom_res=%d", c"custom_res = %d") {
            l.custom_res = (v != 0) as i32;
        } else if let Some(v) = key(line, c"debug=%d", c"debug = %d") {
            l.debug = (v != 0) as i32;
        }
    }
}

/// The file as `SaveLauncherConfig` writes it.
pub fn text(l: &Launcher) -> String {
    let plane = if l.weapon_plane >= 0 { format!("weapon_plane={}\n", l.weapon_plane) } else { String::new() };
    format!(
        "width={}\nheight={}\nhmd_type={}\ncustom_res={}\ndebug={}\nshow_launcher={}\nvr_enabled={}\nmonitor_output={}\n{plane}launcher_delay_ms={}\n",
        l.width, l.height, l.hmd_type, l.custom_res, l.debug, l.show, l.vr_enabled, l.monitor_output, l.delay_ms
    )
}

/// The C++ half's globals and the file's path.
#[repr(C)]
pub struct Globals {
    pub width: *mut i32,
    pub height: *mut i32,
    pub hmd_type: *mut i32,
    pub debug: *mut i32,
    pub show: *mut i32,
    pub delay_ms: *mut i32,
    /// g_launcherConfigPath (ANSI, filled by InitRuntimePaths).
    pub path: *const c_char,
}
// SAFETY: globals of the C++ half, alive for the process.
unsafe impl Sync for Globals {}
unsafe impl Send for Globals {}

/// The plugin's table, built here (formerly the C++ half's); a test program attaches its own.
#[cfg(feature = "plugin-tables")]
mod plugin {
    use core::ffi::c_char;
    unsafe extern "C" {
        #[link_name = "?g_launcherWidth@@3HA"]
        static mut g_launcherWidth: i32;
        #[link_name = "?g_launcherHeight@@3HA"]
        static mut g_launcherHeight: i32;
        #[link_name = "?g_launcherHmdType@@3HA"]
        static mut g_launcherHmdType: i32;
        #[link_name = "?g_launcherDebug@@3HA"]
        static mut g_launcherDebug: i32;
        #[link_name = "?g_launcherShow@@3HA"]
        static mut g_launcherShow: i32;
        #[link_name = "?g_launcherDelayMs@@3HA"]
        static mut g_launcherDelayMs: i32;
        /// char g_launcherConfigPath[MAX_PATH] (src/Core/VrCore.cpp).
        #[link_name = "?g_launcherConfigPath@@3PADA"]
        static g_launcherConfigPath: [c_char; 260];
    }
    pub(super) static RT: super::Globals = super::Globals {
        width: &raw mut g_launcherWidth,
        height: &raw mut g_launcherHeight,
        hmd_type: &raw mut g_launcherHmdType,
        debug: &raw mut g_launcherDebug,
        show: &raw mut g_launcherShow,
        delay_ms: &raw mut g_launcherDelayMs,
        path: (&raw const g_launcherConfigPath).cast(),
    };
}
#[cfg(feature = "plugin-tables")]
static GLOBALS: AtomicPtr<Globals> = AtomicPtr::new((&raw const plugin::RT).cast_mut());
#[cfg(not(feature = "plugin-tables"))]
static GLOBALS: AtomicPtr<Globals> = AtomicPtr::new(core::ptr::null_mut());

#[no_mangle]
pub extern "C" fn cvr_launcher_attach(globals: *mut Globals) {
    GLOBALS.store(globals, Ordering::Release);
}

pub fn globals() -> Option<&'static Globals> {
    // SAFETY: attached once with a pointer the C++ half keeps for the process.
    unsafe { GLOBALS.load(Ordering::Acquire).as_ref() }
}

/// # Safety
/// The attached globals.
unsafe fn read(g: &Globals) -> Launcher {
    // SAFETY: the C++ half's ints.
    unsafe {
        Launcher {
            width: *g.width,
            height: *g.height,
            hmd_type: *g.hmd_type,
            debug: *g.debug,
            show: *g.show,
            delay_ms: *g.delay_ms,
            custom_res: CUSTOM_RES.load(Ordering::Relaxed),
            vr_enabled: VR_ENABLED.load(Ordering::Relaxed),
            monitor_output: MONITOR_OUTPUT.load(Ordering::Relaxed),
            weapon_plane: PENDING_WEAPON_PLANE.load(Ordering::Relaxed),
        }
    }
}
/// # Safety
/// The attached globals.
unsafe fn write(g: &Globals, l: &Launcher) {
    // SAFETY: the C++ half's ints.
    unsafe {
        *g.width = l.width;
        *g.height = l.height;
        *g.hmd_type = l.hmd_type;
        *g.debug = l.debug;
        *g.show = l.show;
        *g.delay_ms = l.delay_ms;
    }
    CUSTOM_RES.store(l.custom_res, Ordering::Relaxed);
    VR_ENABLED.store(l.vr_enabled, Ordering::Relaxed);
    MONITOR_OUTPUT.store(l.monitor_output, Ordering::Relaxed);
    PENDING_WEAPON_PLANE.store(l.weapon_plane, Ordering::Relaxed);
}

/// LoadLauncherConfig: the defaults, then the file's lines over them (a missing file leaves the defaults).
#[no_mangle]
pub extern "C" fn cvr_launcher_load() {
    let Some(g) = globals() else { return };
    let mut l = DEFAULTS;
    // SAFETY: the attached globals and path; the C runtime's file.
    unsafe {
        write(g, &l);
        let Some(mut file) = crt::File::open_shared(g.path) else {
            return;
        };
        let mut line = [0 as c_char; 128];
        while file.gets(&mut line) {
            parse_line(line.as_ptr(), &mut l);
        }
        write(g, &l);
    }
}

/// SaveLauncherConfig(width, height): the render size the launcher chose (a non-positive one keeps the current), and
/// the file written whole.
#[no_mangle]
pub extern "C" fn cvr_launcher_save(width: i32, height: i32) {
    let Some(g) = globals() else { return };
    // SAFETY: the attached globals and path; the C runtime's file.
    unsafe {
        let mut l = read(g);
        if width > 0 {
            l.width = width;
        }
        if height > 0 {
            l.height = height;
        }
        write(g, &l);
        let Some(mut file) = crt::File::open_shared_mode(g.path, c"w") else {
            return;
        };
        let mut bytes = text(&l).into_bytes();
        bytes.push(0);
        file.puts(CStr::from_bytes_with_nul_unchecked(&bytes));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(lines: &[&CStr]) -> Launcher {
        let mut l = DEFAULTS;
        for line in lines {
            // SAFETY: C string literals.
            unsafe { parse_line(line.as_ptr(), &mut l) };
        }
        l
    }

    #[test]
    fn the_file_reads_as_sscanf_read_it() {
        let l = parse(&[
            c"width=2064\n",
            c"height = 2232\n",
            c"hmd_type=3\n",
            c"debug=7\n",
            c"show_launcher=0\n",
            c"launcher_delay_ms=25000\n",
        ]);
        assert_eq!(
            l,
            Launcher {
                width: 2064,
                height: 2232,
                hmd_type: 3,
                debug: 1,
                show: 0,
                delay_ms: 10000,
                ..DEFAULTS
            }
        );
        // A non-positive size keeps the one before; a negative delay is none.
        let l = parse(&[c"width=0\n", c"height=-5\n", c"launcher_delay_ms=-1\n"]);
        assert_eq!((l.width, l.height, l.delay_ms), (2048, 2048, 0));
        // The key at the line's start only; "%d" stops at the first non-digit.
        let l = parse(&[c" width=100\n", c"width=1500px\n"]);
        assert_eq!(l.width, 1500);
        // main's keys: the VR/desktop pick, the monitor's eye (a trailing character refuses it), the weapon plane.
        let l = parse(&[c"vr_enabled=0\n", c"monitor_output=2\n", c"weapon_plane = 1\n", c"custom_res=5\n"]);
        assert_eq!((l.vr_enabled, l.monitor_output, l.weapon_plane, l.custom_res), (0, 2, 1, 1));
        let l = parse(&[c"monitor_output=1x\n", c"monitor_output=7\n"]);
        assert_eq!(l.monitor_output, 0);
        let l = parse(&[c"monitor_output = 1  \n"]);
        assert_eq!(l.monitor_output, 1);
    }

    #[test]
    fn the_file_writes_as_fprintf_wrote_it() {
        let l = Launcher {
            width: 2064,
            height: 2232,
            hmd_type: 2,
            debug: 0,
            show: 1,
            delay_ms: 1000,
            ..DEFAULTS
        };
        assert_eq!(
            text(&l),
            "width=2064\nheight=2232\nhmd_type=2\ncustom_res=0\ndebug=0\nshow_launcher=1\nvr_enabled=1\nmonitor_output=0\nlauncher_delay_ms=1000\n"
        );
        let l = Launcher { weapon_plane: 0, monitor_output: 1, ..l };
        assert!(text(&l).contains("monitor_output=1\nweapon_plane=0\nlauncher_delay_ms"));
    }
}
