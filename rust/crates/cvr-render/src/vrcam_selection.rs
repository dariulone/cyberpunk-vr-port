//! Which second-eye camera this session uses, and noticing when the answer changes (formerly
//! src/Stereo/VrcamSelection.cpp, with the parts of include/Stereo/VrcamConfig.hpp it used; the C++ keeps the
//! selection's globals, its `load_vrcam_selection` / `vrcam_active_watcher` forward here).
//!
//! Four coupled fields -- component name, virtual-camera name, the view key hashed from it and the resolution -- are
//! resolved together from vrcam.json and the launcher ini, because resolving any one of them alone is how the render
//! target and the component came to disagree. The watcher exists because the CET side can enable a different camera
//! after the file was read, and because the launcher pick arrives after the init-time read: vrport-launcher.ini is
//! written by the proxy's dialog when the swapchain is created, long after this plugin initialised, so the first read
//! returns the PREVIOUS session's resolution and the watcher repeats it every poll.
//!
//! vrcam.json is scanned, not parsed (a handful of fields and a comment block), exactly as the header's scanners do:
//! a ':' is required between a key and its value, so a key name quoted inside a comment string is not the field.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::vrcam_config::*;
use core::ffi::{c_char, c_int, c_void};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::*};
use std::ffi::CString;
use std::path::PathBuf;

#[allow(non_upper_case_globals)]
extern "C" {
    static mut g_vrcam_component: [u8; 96];
    static mut g_vrcam_camera: [u8; 96];
    static g_vrcam_sel_w: AtomicU32;
    static g_vrcam_sel_h: AtomicU32;
    static g_vrcam_ctx_key: AtomicU64;
    static g_vrcam_pick_authoritative: AtomicBool;
    static CyberpunkVR_DebugVrcamConfigLoaded: AtomicU32;
    static CyberpunkVR_DebugVrcamNodeHits: AtomicU64;

    /// This launch's eye size once decided (src/Core/VrCore.cpp).
    fn GetLaunchEyeSize(width: *mut c_int, height: *mut c_int) -> c_int;
    /// `cvr::log("%s", text)`: the stereo module's switch and rate cap, "[stereo] " before and a newline after.
    fn cvr_cpp_stereo_log(text: *const c_char);
}
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct FileTime {
    low: u32,
    high: u32,
}
#[repr(C)]
#[derive(Default)]
struct FileAttributeData {
    attributes: u32,
    creation: FileTime,
    access: FileTime,
    write: FileTime,
    size_high: u32,
    size_low: u32,
}
#[link(name = "kernel32")]
extern "system" {
    fn GetProcessTimes(
        process: *mut c_void,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
    fn GetFileAttributesExW(name: *const u16, level: i32, data: *mut FileAttributeData) -> i32;
    fn GetTickCount64() -> u64;
    fn Sleep(ms: u32);
}

/// MAX_PATH: the C++ copied every path into buffers of this size and gave up on one that did not fit.
const MAX_PATH: usize = 260;

fn log(text: &str) {
    if let Ok(text) = CString::new(text) {
        // SAFETY: a C++ service taking a NUL-terminated string.
        unsafe { cvr_cpp_stereo_log(text.as_ptr()) };
    }
}

/// The Stereo mod's copy of vrcam.json, the one vrcam_select.lua reads (reported even when it does not exist yet).
fn config_path() -> Option<PathBuf> {
    let path = cvr_host::paths::file(
        cvr_host::paths::Folder::Lua,
        "CyberpunkVRPort_Stereo\\vrcam.json",
    );
    let len = path.as_os_str().len();
    (len != 0 && len < MAX_PATH).then_some(path)
}
/// The WHOLE file (a truncated one loses its closing ']' and every resolution then reports "not in the authored
/// list"); a megabyte of selection file is a corrupt file. Also the path it looked at.
fn config_read() -> (Option<Vec<u8>>, String) {
    let Some(path) = config_path() else {
        return (None, String::new());
    };
    let shown = path.display().to_string();
    let Ok(mut file) = std::fs::File::open(&path) else {
        return (None, shown);
    };
    use std::io::Read;
    let mut text = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match file.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                text.extend_from_slice(&buf[..n]);
                if text.len() > 1 << 20 {
                    break;
                }
            }
        }
    }
    if text.is_empty() {
        return (None, shown);
    }
    (Some(text), shown)
}
/// The "component" value replaced, and "virtualCamera" with it whenever present (a stale camera field means the render
/// path never recognises the VRCAM view while every log line shows the freshly picked component); comments,
/// formatting and the authored list are left untouched.
fn config_write_component(component: &[u8]) -> bool {
    let (Some(mut text), _) = config_read() else {
        return false;
    };
    let Some(path) = config_path() else {
        return false;
    };
    let camera = derive_camera(component);
    let Some((cpos, clen)) = json_string_span(&text, "component") else {
        return false;
    };
    let vcam = camera
        .as_ref()
        .and_then(|_| json_string_span(&text, "virtualCamera"));
    match (vcam, camera.as_ref()) {
        (Some((vpos, vlen)), Some(camera)) if vpos > cpos => {
            // The later field first, so the earlier replacement cannot shift its offsets.
            text.splice(vpos..vpos + vlen, camera.iter().copied());
            text.splice(cpos..cpos + clen, component.iter().copied());
        }
        _ => {
            text.splice(cpos..cpos + clen, component.iter().copied());
            if let (Some(_), Some(camera)) = (vcam, camera.as_ref()) {
                if let Some((vpos, vlen)) = json_string_span(&text, "virtualCamera") {
                    text.splice(vpos..vpos + vlen, camera.iter().copied());
                }
            }
        }
    }
    std::fs::write(&path, &text).is_ok()
}
/// PLUGIN\lua\CyberpunkVRPort_Stereo\bridge\<name>, the folder the Lua mod polls.
fn bridge_path(name: &str) -> Option<PathBuf> {
    let cfg = config_path()?;
    let folder = cfg.parent()?;
    let path = PathBuf::from(format!("{}\\bridge\\{name}", folder.display()));
    (path.as_os_str().len() < MAX_PATH).then_some(path)
}
/// The last VRCAM enable request (bridge/vrcam_enable.txt; formerly the header's g_vrcamEnableRequest): -2 not read
/// yet, -1 none, 0 disable, 1 enable.
static ENABLE_REQUEST: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(-2);
/// The request file's last-write time when last read, and when it is next looked at.
static ENABLE_STAMP: AtomicU64 = AtomicU64::new(0);
static ENABLE_NEXT_CHECK: AtomicU64 = AtomicU64::new(0);
/// vrcam_enable_request (the CET mod's VRVrcamEnableRequest, formerly AnimLookup.cpp's service): the request file's
/// content, re-read when its last-write time changes, looked at twice a second at most (a sandboxed CET open costs
/// milliseconds). Never the second view under engine stereo, which draws the second eye itself.
#[no_mangle]
pub extern "C" fn cvr_cpp_vrcam_enable_request() -> c_int {
    if crate::engine_stereo::cvr_engine_stereo_enabled() {
        return 0;
    }
    // SAFETY: a plain clock read.
    let now = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    let due = ENABLE_NEXT_CHECK.load(SeqCst);
    if now >= due && ENABLE_NEXT_CHECK.compare_exchange(due, now + 500, SeqCst, SeqCst).is_ok() {
        use std::os::windows::fs::MetadataExt;
        match bridge_path("vrcam_enable.txt").and_then(|p| std::fs::metadata(&p).ok().map(|m| (p, m))) {
            Some((path, meta)) => {
                let stamp = meta.last_write_time();
                if stamp != ENABLE_STAMP.swap(stamp, SeqCst) {
                    let mut body = [0u8; 15];
                    if let Ok(mut f) = std::fs::File::open(&path) {
                        use std::io::Read;
                        let _ = f.read(&mut body);
                    }
                    let text = body.split(|&b| b == 0).next().unwrap_or_default();
                    let value = if text.is_empty() { -1 } else { enable_value(&String::from_utf8_lossy(text)) };
                    ENABLE_REQUEST.store(value, SeqCst);
                }
            }
            None => {
                let _ = ENABLE_REQUEST.compare_exchange(-2, -1, SeqCst, SeqCst);
            }
        }
    }
    ENABLE_REQUEST.load(SeqCst)
}
fn enable_value(body: &str) -> c_int {
    if body.contains('0') {
        0
    } else if body.contains('1') {
        1
    } else {
        -1
    }
}
/// vrcam.json in the Stereo mod's folder.
pub fn config_file() -> Option<PathBuf> {
    config_path()
}
/// `body` written to the Stereo mod's bridge/<name> (the folder made when missing).
pub fn bridge_write(name: &str, body: &str) -> bool {
    let Some(path) = bridge_path(name) else {
        return false;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir(dir);
    }
    if std::fs::write(&path, body.as_bytes()).is_err() {
        return false;
    }
    if name == "vrcam_enable.txt" {
        ENABLE_REQUEST.store(enable_value(body), SeqCst);
    }
    true
}

/// The launcher's pick: this launch's eye size once decided (the pick may have been fitted to what the runtime
/// takes, and the file keeps the pick), else the first 511 bytes of vrport-launcher.ini.
fn launcher_resolution() -> Option<(i32, i32)> {
    let (mut w, mut h) = (0, 0);
    // SAFETY: two out-parameters.
    if unsafe { GetLaunchEyeSize(&mut w, &mut h) } != 0 {
        return Some((w, h));
    }
    let path = cvr_host::paths::file(cvr_host::paths::Folder::Config, "vrport-launcher.ini");
    if path.as_os_str().len() >= MAX_PATH {
        return None;
    }
    use std::io::Read;
    let mut file = std::fs::File::open(&path).ok()?;
    let mut buf = [0u8; 511];
    let mut n = 0;
    while n < buf.len() {
        match file.read(&mut buf[n..]) {
            Ok(0) | Err(_) => break,
            Ok(k) => n += k,
        }
    }
    let text = &buf[..buf[..n].iter().position(|&b| b == 0).unwrap_or(n)];
    let after = |key: &[u8]| {
        text.windows(key.len())
            .position(|w| w == key)
            .map(|p| atoi(&text[p + key.len()..]))
    };
    let lw = after(b"width=").unwrap_or(0);
    let lh = after(b"height=").unwrap_or(0);
    (lw > 0 && lh > 0).then_some((lw, lh))
}

fn c_string(buf: &[u8; 96]) -> Vec<u8> {
    buf[..buf.iter().position(|&b| b == 0).unwrap_or(96)].to_vec()
}
unsafe fn component() -> Vec<u8> {
    c_string(&*(&raw const g_vrcam_component))
}
unsafe fn camera() -> Vec<u8> {
    c_string(&*(&raw const g_vrcam_camera))
}
fn s(b: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(b)
}
unsafe fn store(buf: *mut [u8; 96], value: &[u8]) {
    let n = value.len().min(95);
    let buf = &mut *buf;
    buf[..n].copy_from_slice(&value[..n]);
    buf[n] = 0;
}

unsafe fn apply_selection(component_name: &[u8], camera_name: &[u8]) {
    store(&raw mut g_vrcam_component, component_name);
    store(&raw mut g_vrcam_camera, camera_name);
    g_vrcam_ctx_key.store(cname_hash(&camera()), SeqCst);
    // The resolution off the COMPONENT name, falling back to the camera's: both carry <W>x<H>.
    let (mut w, mut h) = (0, 0);
    if let Some((a, b)) = parse_resolution(&component()).or_else(|| parse_resolution(&camera())) {
        (w, h) = (a, b);
        g_vrcam_sel_w.store(w as u32, SeqCst);
        g_vrcam_sel_h.store(h as u32, SeqCst);
    }
    log(&format!(
        "[vrcam] selection applied: component={} camera={} key=0x{:016X} res={w}x{h}{}",
        s(&component()),
        s(&camera()),
        g_vrcam_ctx_key.load(SeqCst),
        if g_vrcam_pick_authoritative.load(Relaxed) {
            " (from launcher resolution)"
        } else {
            ""
        }
    ));
}

/// Reads the selection file and derives the view key from it; called once from sync_stereo_init, before any hook can
/// observe a view.
///
/// # Safety
/// Called by the stereo module's init, in the game's process.
#[no_mangle]
pub unsafe extern "C" fn cvr_load_vrcam_selection() {
    let (text, path) = config_read();
    let Some(text) = text else {
        log(&format!(
            "[vrcam] no vrcam.json (looked at {path}) -> keeping default component={} camera={} key=0x{:016X}",
            s(&component()),
            s(&camera()),
            g_vrcam_ctx_key.load(SeqCst)
        ));
        return;
    };
    let Some(mut comp) = json_string(&text, "component") else {
        log(&format!(
            "[vrcam] {path} has no \"component\" field -> keeping default {}",
            s(&component())
        ));
        return;
    };
    // THE LAUNCHER PICK WINS over a stale "component" field, when the entity has the component it names (it must be
    // in the authored "components" list -- one never imported would render nothing), and is written back so the CET
    // side, which reads the same file to decide what to enable, agrees.
    if let Some((lw, lh)) = launcher_resolution().filter(|&(w, h)| w > 0 && h > 0) {
        let wanted = format!("vrcam_{lw}x{lh}").into_bytes();
        if comp != wanted {
            let authored = json_string_array(&text, "components").unwrap_or_default();
            if authored.contains(&wanted) {
                log(&format!(
                    "[vrcam] launcher picked {lw}x{lh} -> switching component {} -> {}",
                    s(&comp),
                    s(&wanted)
                ));
                comp = wanted.clone();
                // Authoritative from here on: the watcher must not adopt a stale live camera over this.
                g_vrcam_pick_authoritative.store(true, Relaxed);
                if config_write_component(&wanted) {
                    log("[vrcam] vrcam.json updated so the CET side enables the same one");
                } else {
                    log("[vrcam] WARNING: could not write vrcam.json -- the CET side may still enable something else, \
                         leaving no matching view");
                }
            } else {
                log(&format!(
                    "[vrcam] launcher picked {lw}x{lh} but {} is not in the authored list -> keeping {} (import the \
                     component, then add it to \"components\")",
                    s(&wanted),
                    s(&comp)
                ));
            }
        }
    }
    // The camera name is what the view key is hashed from, so it MUST match the component: an explicit
    // "virtualCamera" is honoured only when it agrees.
    let derived = derive_camera(&comp);
    let explicit = json_string(&text, "virtualCamera");
    let cam = match (explicit, derived) {
        (Some(cam), Some(derived)) if cam != derived => {
            log(&format!(
                "[vrcam] WARNING: \"virtualCamera\"={} does not match component {} (expected {}) -> using {}",
                s(&cam),
                s(&comp),
                s(&derived),
                s(&derived)
            ));
            derived
        }
        (Some(cam), _) => cam,
        (None, Some(derived)) => derived,
        (None, None) => {
            log(&format!(
                "[vrcam] component {} has no \"vrcam_\" prefix and no \"virtualCamera\" given -> cannot derive the \
                 camera name, keeping default {}",
                s(&comp),
                s(&camera())
            ));
            return;
        }
    };
    apply_selection(&comp, &cam);
    CyberpunkVR_DebugVrcamConfigLoaded.store(1, SeqCst);
    log(&format!("[vrcam] selection source: {path}"));
}

fn wide(path: &std::path::Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
fn ft(t: FileTime) -> u64 {
    ((t.high as u64) << 32) | t.low as u64
}

/// The watcher: the launcher pick re-read every poll, and the live camera vrcam_select.lua reports in
/// bridge/vrcam_active.txt adopted when no launcher pick decided the selection (the camera name is DERIVED from the
/// component's, which assumes the asset follows the vrcam_feed_<suffix> convention; the live entity is the ground
/// truth). Never returns.
///
/// # Safety
/// Run on its own thread, started by the stereo module's init.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrcam_active_watcher() {
    let Some(path) = bridge_path("vrcam_active.txt") else {
        return;
    };
    let path_w = wide(&path);
    let mut last: Vec<u8> = Vec::new();
    let mut disagree_since = 0u64; // first tick of a disagreement that produced no nodes
    let mut last_request = 0u64; // the re-request writes, rate-limited
    let mut last_node_hits = 0u64;
    let (mut seen_w, mut seen_h) = (0, 0); // the last launcher resolution acted on
                                           // A name the previous session wrote is not a live camera: until vrcam_select.lua writes this session's, the file
                                           // still holds the last one.
    let (mut start, mut unused) = (FileTime::default(), [FileTime::default(); 3]);
    let have_start = GetProcessTimes(
        GetCurrentProcess(),
        &mut start,
        &mut unused[0],
        &mut unused[1],
        &mut unused[2],
    ) != 0;
    loop {
        Sleep(500);

        // THE LAUNCHER PICK, re-read: here the swapchain exists and the file is current.
        if let Some((lw, lh)) = launcher_resolution().filter(|&(w, h)| w != seen_w || h != seen_h) {
            (seen_w, seen_h) = (lw, lh);
            let wanted = format!("vrcam_{lw}x{lh}").into_bytes();
            if wanted != component() {
                let exists = config_read()
                    .0
                    .and_then(|text| json_string_array(&text, "components"))
                    .is_some_and(|a| a.contains(&wanted));
                if let Some(cam) = derive_camera(&wanted).filter(|_| exists) {
                    log(&format!(
                        "[vrcam] launcher resolution is {lw}x{lh} (read after the swapchain existed) -> switching {} -> {}",
                        s(&component()),
                        s(&wanted)
                    ));
                    apply_selection(&wanted, &cam);
                    g_vrcam_pick_authoritative.store(true, Relaxed);
                    config_write_component(&wanted);
                    bridge_write("vrcam_enable.txt", "1");
                    last.clear();
                    disagree_since = 0;
                    continue;
                }
                if !exists {
                    log(&format!(
                        "[vrcam] launcher resolution is {lw}x{lh} but {} is not in the authored \"components\" list -> \
                         keeping {}",
                        s(&wanted),
                        s(&component())
                    ));
                }
            }
        }
        if have_start {
            let mut data = FileAttributeData::default();
            if GetFileAttributesExW(path_w.as_ptr(), 0, &mut data) == 0
                || ft(data.write) < ft(start)
            {
                continue;
            }
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let mut buf = bytes[..bytes.len().min(127)].to_vec();
        if let Some(nul) = buf.iter().position(|&b| b == 0) {
            buf.truncate(nul);
        }
        // Trimmed at the first whitespace or newline.
        if let Some(cut) = buf
            .iter()
            .position(|&b| matches!(b, b'\r' | b'\n' | b' ' | b'\t'))
        {
            buf.truncate(cut);
        }
        if buf.is_empty() || last == buf {
            continue; // empty = VRCAM disabled, the key kept
        }
        last = buf.clone();
        let key = cname_hash(&buf);
        if key == g_vrcam_ctx_key.load(SeqCst) {
            log(&format!("[vrcam] live camera confirms key: {}", s(&buf)));
            continue;
        }
        if g_vrcam_pick_authoritative.load(Relaxed) {
            // The launcher resolution decided this, so CET is the one out of step: NOT adopted (adopting made the
            // selection oscillate). The request is re-stated, and given up on EVIDENCE, never on a stopwatch alone:
            // the clock only counts while our own view produces NOTHING and something else is live.
            let node_hits = CyberpunkVR_DebugVrcamNodeHits.load(Relaxed);
            if node_hits != last_node_hits {
                last_node_hits = node_hits;
                disagree_since = 0;
            }
            if node_hits != 0 {
                last.clear(); // our view IS rendering
                continue;
            }
            let now = GetTickCount64();
            if disagree_since == 0 {
                disagree_since = now;
            }
            if now - disagree_since < 30000 {
                if now - last_request >= 2000 {
                    last_request = now;
                    log(&format!(
                        "[vrcam] live camera is {} but the launcher picked {} -> NOT adopting; re-requesting {}",
                        s(&buf),
                        s(&camera()),
                        s(&component())
                    ));
                    config_write_component(&component());
                    bridge_write("vrcam_enable.txt", "1");
                }
                last.clear(); // re-evaluated on the next poll
                continue;
            }
            log(&format!(
                "[vrcam] GIVING UP on {} -- 30 s of gameplay with another camera live and zero nodes of our own, so the \
                 entity most likely has no such component (listing it in vrcam.json does not create it; the asset has \
                 to be imported). Falling back to the live camera {}.",
                s(&component()),
                s(&buf)
            ));
            g_vrcam_pick_authoritative.store(false, Relaxed);
        }
        disagree_since = 0;
        log(&format!(
            "[vrcam] live camera is {} (key 0x{key:016X}), not {} -> adopting the live one (no launcher pick to \
             honour); check virtualCameraName on component {}",
            s(&buf),
            s(&camera()),
            s(&component())
        ));
        let comp = component();
        apply_selection(&comp, &buf);
    }
}
