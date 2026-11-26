//! THE PORT'S OWN LOADER: bin\x64\powrprof.dll. The game imports the system's powrprof.dll (for one function); this
//! proxy stands in for it -- every export a jump to the real one in System32 -- and, in Cyberpunk2077.exe, loads the
//! port from its folder at the game's root (<game>\cyberpunkvrport\cyberpunkvrport.dll) and hands it the start
//! (CyberpunkVRPort_LoaderAttach: the port hooks WinMain, the game states and quick_exit, as RED4ext would). No
//! other mod loads through powrprof.dll (RED4ext's loader is winmm.dll, CET's version.dll), so they sit side by side.
//! Without the port's folder the proxy only forwards.
//!
//! WHEN: AT WinMain, AS RED4EXT LOADS ITS PLUGINS. The port's own start reads the engine (its statics are made with the
//! game's), so it is loaded only once the game's C runtime has made the game's statics -- not from this DllMain (under
//! the loader's lock, and before them) and not at the entry point (before them: the port's DLL refused to start there,
//! error 1114). DllMain patches the game's entry point (a jump to `entry`, the bytes kept); the entry puts them back,
//! finds WinMain in the address library the game ships (bin\x64\cyberpunk2077_addresses.json, hash 240386859) and
//! patches it the same way, once; WinMain puts its bytes back -- RED4ext's own jump among them, when RED4ext runs --
//! loads the port, lets it start (CyberpunkVRPort_LoaderAttach: the game states and quick_exit hooked, the plugin's
//! Main(Load)), and goes on into the game's WinMain.
#![allow(clippy::missing_safety_doc)]
use core::ffi::c_void;
use windows_sys::Win32::Foundation::{HMODULE, TRUE};
type BOOL = i32;
use windows_sys::Win32::System::LibraryLoader::{
    DisableThreadLibraryCalls, GetModuleFileNameW, GetProcAddress, LoadLibraryExW, LoadLibraryW,
    LOAD_WITH_ALTERED_SEARCH_PATH,
};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

include!(concat!(env!("OUT_DIR"), "/stubs.rs"));

/// The real functions, by the order of `NAMES` (the stubs jump through it).
#[no_mangle]
pub static mut REAL: [usize; COUNT] = [0; COUNT];

/// An export the system's DLL lacks (an older Windows): ERROR_CALL_NOT_IMPLEMENTED as the status.
unsafe extern "system" fn missing() -> u32 {
    120
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// The real powrprof.dll from System32, its functions into the table.
unsafe fn load_real() {
    let mut dir = [0u16; 520];
    // SAFETY: the buffer and its size.
    let n = unsafe { GetSystemDirectoryW(dir.as_mut_ptr(), dir.len() as u32) } as usize;
    if n == 0 || n >= dir.len() {
        return;
    }
    let path = format!("{}\\powrprof.dll", String::from_utf16_lossy(&dir[..n]));
    // SAFETY: a NUL-terminated full path.
    let module = unsafe { LoadLibraryW(wide(&path).as_ptr()) };
    for (i, name) in NAMES.iter().enumerate() {
        let c: Vec<u8> = name.bytes().chain(Some(0)).collect();
        // SAFETY: the real module (or null: every export is then `missing`), a NUL-terminated name.
        let f = if module.is_null() { None } else { unsafe { GetProcAddress(module, c.as_ptr()) } };
        // SAFETY: written once here, before anything can call a stub.
        unsafe { (*(&raw mut REAL))[i] = f.map_or(missing as unsafe extern "system" fn() -> u32 as usize, |f| f as usize) };
    }
}

/// This process's executable: its folder and whether it is the game.
fn executable() -> Option<(std::path::PathBuf, bool)> {
    let mut buf = vec![0u16; 32768];
    // SAFETY: the buffer and its size.
    let n = unsafe { GetModuleFileNameW(core::ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    let exe = std::path::PathBuf::from(String::from_utf16_lossy(&buf[..n]));
    let game = exe.file_name().is_some_and(|f| f.eq_ignore_ascii_case("Cyberpunk2077.exe"));
    Some((exe.parent()?.to_path_buf(), game))
}

/// The port from <game>\cyberpunkvrport, started (at WinMain).
unsafe fn load_port() {
    let Some((bin, true)) = executable() else { return };
    let Some(root) = bin.parent().and_then(|p| p.parent()) else { return };
    let dll = root.join("cyberpunkvrport").join("cyberpunkvrport.dll");
    if !dll.is_file() {
        return;
    }
    let path: Vec<u16> = dll.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect();
    trace("WinMain: loading the port", false);
    // SAFETY: a full path; the port's own folder is searched for what it needs.
    let module = unsafe { LoadLibraryExW(path.as_ptr(), core::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) };
    if module.is_null() {
        // SAFETY: the thread's last error, just set.
        let error = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        trace(&format!("LoadLibrary failed: error {error}"), false);
        note(root, &format!("the port's DLL did not load: error {error} ({})", dll.display()));
        return;
    }
    trace("the port loaded; attaching", false);
    // SAFETY: the port's export of this signature.
    match unsafe { GetProcAddress(module, c"CyberpunkVRPort_LoaderAttach".as_ptr().cast()) } {
        Some(attach) => {
            type Attach = unsafe extern "C" fn(*mut c_void) -> bool;
            // SAFETY: as above.
            let started = unsafe { core::mem::transmute::<_, Attach>(attach)(module) };
            trace(if started { "the port started" } else { "the port declined to start" }, false);
            if !started {
                note(root, "the port declined to start (see cyberpunkvrport\\log\\cyberpunkvrport.log)");
            }
        }
        None => note(root, "the port's DLL has no CyberpunkVRPort_LoaderAttach: not a release made for this loader"),
    }
}

/// Each step of the start, for the launch it belongs to: <game>\cyberpunkvrport\log\loader-trace.txt (made new by
/// DllMain, a line a step; only when the port's folder is there). A start that leaves no port log says here how far
/// it came.
fn trace(text: &str, fresh: bool) {
    use std::io::Write as _;
    let Some((bin, true)) = executable() else { return };
    let Some(root) = bin.parent().and_then(|p| p.parent()) else { return };
    let dir = root.join("cyberpunkvrport").join("log");
    if !dir.is_dir() {
        return;
    }
    // SAFETY: a plain Win32 call.
    let ms = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    let file = std::fs::OpenOptions::new().create(true).write(true).append(!fresh).truncate(fresh).open(dir.join("loader-trace.txt"));
    if let Ok(mut f) = file {
        let _ = write!(f, "{ms} {text}\r\n");
    }
}

/// What went wrong, where the player looks: <game>\cyberpunkvrport\log\loader.txt.
fn note(root: &std::path::Path, text: &str) {
    let dir = root.join("cyberpunkvrport").join("log");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("loader.txt"), format!("{text}\r\n"));
}

/// The game's entry point and WinMain, and the bytes each jump replaced.
static mut ENTRY: usize = 0;
static mut SAVED: [u8; 14] = [0; 14];
static mut WINMAIN: usize = 0;
static mut SAVED_WINMAIN: [u8; 14] = [0; 14];
const WINMAIN_HASH: &[u8] = b"{\"hash\": \"240386859\"";

/// Writes `bytes` over the code at `at` (made writable for it), the instruction cache flushed.
unsafe fn patch(at: usize, bytes: &[u8]) -> bool {
    use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
    use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut old = 0u32;
    // SAFETY: the executable's own code, a few bytes, protection put back.
    unsafe {
        if VirtualProtect(at as *const c_void, bytes.len(), PAGE_EXECUTE_READWRITE, &mut old) == 0 {
            return false;
        }
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), at as *mut u8, bytes.len());
        VirtualProtect(at as *const c_void, bytes.len(), old, &mut old);
        FlushInstructionCache(GetCurrentProcess(), at as *const c_void, bytes.len());
    }
    true
}
/// `jmp [rip+0]` to `to`.
fn jump_to(to: usize) -> [u8; 14] {
    let mut jump = [0u8; 14];
    jump[0] = 0xFF;
    jump[1] = 0x25;
    jump[6..14].copy_from_slice(&(to as u64).to_le_bytes());
    jump
}
fn image_base() -> usize {
    // SAFETY: the process image is always loaded.
    unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as usize }
}
/// WinMain's address from the game's address library: its entry's "offset": "<section>:<hex>" by the image's sections.
fn find_winmain(bin: &std::path::Path) -> Option<usize> {
    let text = std::fs::read(bin.join("cyberpunk2077_addresses.json")).ok()?;
    let at = text.windows(WINMAIN_HASH.len()).position(|w| w == WINMAIN_HASH)?;
    const OFFSET: &[u8] = b"\"offset\": \"";
    let rest = &text[at..text.len().min(at + 512)];
    let o = rest.windows(OFFSET.len()).position(|w| w == OFFSET)? + OFFSET.len();
    let field = &rest[o..];
    let end = field.iter().position(|&b| b == b'"')?;
    let value = core::str::from_utf8(&field[..end]).ok()?;
    let (section, offset) = value.split_once(':')?;
    let section = usize::from_str_radix(section, 16).ok()?;
    let offset = usize::from_str_radix(offset, 16).ok()?;
    let base = image_base();
    // SAFETY: the process image's own headers.
    let va = unsafe {
        let pe = core::ptr::read_unaligned((base + 0x3C) as *const u32) as usize;
        let count = core::ptr::read_unaligned((base + pe + 6) as *const u16) as usize;
        let optional = core::ptr::read_unaligned((base + pe + 20) as *const u16) as usize;
        if section == 0 || section > count {
            return None;
        }
        core::ptr::read_unaligned((base + pe + 24 + optional + (section - 1) * 40 + 12) as *const u32) as usize
    };
    Some(base + va + offset)
}
/// The game's WinMain, reached through the jump: its bytes back, the port loaded and started, then WinMain itself
/// (through RED4ext's jump when RED4ext put one there).
unsafe extern "system" fn winmain(instance: usize, previous: usize, command: usize, show: i32) -> i32 {
    // SAFETY: the bytes saved from this very address.
    unsafe {
        let at = WINMAIN;
        let saved = SAVED_WINMAIN;
        trace(&format!("WinMain reached (its first bytes {:02X?})", &saved[..5]), false);
        patch(at, &saved);
        load_port();
        type F = unsafe extern "system" fn(usize, usize, usize, i32) -> i32;
        core::mem::transmute::<usize, F>(at)(instance, previous, command, show)
    }
}
/// The game's entry, reached through the jump: its bytes back, WinMain's jump made, then the game's own entry.
unsafe extern "system" fn entry(peb: usize) -> u32 {
    // SAFETY: the bytes saved from this very address; WinMain's patched only when it was found.
    unsafe {
        let at = ENTRY;
        let saved = SAVED;
        patch(at, &saved);
        trace("entry point reached", false);
        if let Some((bin, true)) = executable() {
            let root = bin.parent().and_then(|p| p.parent()).map(std::path::Path::to_path_buf);
            match find_winmain(&bin) {
                Some(w) => {
                    WINMAIN = w;
                    core::ptr::copy_nonoverlapping(w as *const u8, (&raw mut SAVED_WINMAIN).cast::<u8>(), 14);
                    let patched = patch(w, &jump_to(winmain as unsafe extern "system" fn(usize, usize, usize, i32) -> i32 as usize));
                    let first = *(&raw const SAVED_WINMAIN);
                    trace(
                        &format!(
                            "WinMain at exe+{:#x} (first bytes {:02X?}), jump {}",
                            w - image_base(),
                            &first[..5],
                            if patched { "made" } else { "NOT made" }
                        ),
                        false,
                    );
                }
                None => {
                    trace("WinMain not found in the address library", false);
                    if let Some(root) = root {
                        if root.join("cyberpunkvrport").join("cyberpunkvrport.dll").is_file() {
                            note(&root, "WinMain (hash 240386859) is not in bin\\x64\\cyberpunk2077_addresses.json: the port does not start");
                        }
                    }
                }
            }
        }
        type Entry = unsafe extern "system" fn(usize) -> u32;
        core::mem::transmute::<usize, Entry>(at)(peb)
    }
}
/// The jump to `entry` written over the game's entry point.
unsafe fn hook_entry() -> bool {
    let base = image_base();
    // SAFETY: the process image's own headers and code.
    unsafe {
        let pe = core::ptr::read_unaligned((base + 0x3C) as *const u32) as usize;
        let rva = core::ptr::read_unaligned((base + pe + 0x28) as *const u32) as usize;
        if rva == 0 {
            return false;
        }
        ENTRY = base + rva;
        core::ptr::copy_nonoverlapping(ENTRY as *const u8, (&raw mut SAVED).cast::<u8>(), 14);
        patch(ENTRY, &jump_to(entry as unsafe extern "system" fn(usize) -> u32 as usize))
    }
}

#[no_mangle]
pub unsafe extern "system" fn DllMain(module: HMODULE, reason: u32, _reserved: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        // SAFETY: plain Win32 calls while the process starts; the port waits for the game's entry.
        unsafe {
            DisableThreadLibraryCalls(module);
            load_real();
            if executable().is_some_and(|(_, game)| game) {
                let hooked = hook_entry();
                trace(&format!("DllMain: the entry point's jump {}", if hooked { "made" } else { "NOT made" }), true);
            }
        }
    }
    TRUE
}
