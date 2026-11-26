//! The full-body tracking switches (formerly src/Fbt/Game/Body.cpp's ReadSettings, WriteSettings and the
//! Legs/Spine/ArmsEnabled pairs): V's legs following the trackers, V's spine and head following the headset, V's arms
//! following the controllers. All three are off until fbt_settings.ini (the config folder) says otherwise. The file
//! is read once, on first use, through the C runtime as the .cpp read it -- `fopen_s(path, "r")`, `fgets` into 128
//! bytes, `sscanf_s(line, "legs=%d")` and its two siblings on every line, the last match winning, any value other than
//! 0 meaning on -- and written whole whenever one switch changes, as its text-mode `fprintf` wrote it.
//!
//! The C ABI is `cvr_fbt_switch` / `cvr_fbt_set_switch` (include/Rust/CvrRust.h). The unit tests below read and write a
//! file in the temp folder.
use core::ffi::{c_char, CStr};
use cvr_crt::{self as crt, Arg};
use cvr_host::paths::{self, Folder};
use std::ffi::CString;
use std::sync::atomic::{AtomicI32, Ordering::Relaxed};

/// The switches, by index.
pub const LEGS: u32 = 0;
pub const SPINE: u32 = 1;
pub const ARMS: u32 = 2;
const KEYS: [&CStr; 3] = [c"legs=%d", c"spine=%d", c"arms=%d"];
const NAMES: [&str; 3] = ["V's legs follow the trackers", "V's spine and head follow the headset", "V's arms follow the controllers"];
const FILE: &str = "fbt_settings.ini";

/// The switches; the legs' -1 marks the file not read yet (stored last, after the others).
static SWITCHES: [AtomicI32; 3] = [AtomicI32::new(-1), AtomicI32::new(0), AtomicI32::new(0)];

/// One line scanned for the three keys; a key the line does not match leaves its value.
///
/// # Safety
/// `line` is a C string.
pub unsafe fn scan_line(line: *const c_char, values: &mut [i32; 3]) {
    for (key, value) in KEYS.iter().zip(values.iter_mut()) {
        // SAFETY: the caller's C string; `%d` into an i32.
        unsafe { crt::scan(line, key, &[crt::ptr(value)]) };
    }
}
/// The file's values (legs, spine, arms): 0 where it names none, all 0 when it cannot be opened.
pub fn read_file(path: &CStr) -> [i32; 3] {
    let mut values = [0i32; 3];
    // SAFETY: a C string path.
    if let Some(mut file) = unsafe { crt::File::open(path.as_ptr(), c"r") } {
        let mut line = [0 as c_char; 128];
        while file.gets(&mut line) {
            // SAFETY: fgets terminates the line.
            unsafe { scan_line(line.as_ptr(), &mut values) };
        }
    }
    values
}
/// The file written (text mode): a comment, then legs, spine and arms, 1 on and 0 off. A file that cannot be opened
/// is left as it is.
pub fn write_file(path: &CStr, on: [bool; 3]) {
    // SAFETY: a C string path.
    let Some(mut file) = (unsafe { crt::File::open(path.as_ptr(), c"w") }) else { return };
    let [legs, spine, arms] = on.map(|o| Arg::I(i32::from(o)));
    file.puts(&crt::format(c"; Full-body tracking settings (src/Fbt/Game).\nlegs=%d\nspine=%d\narms=%d\n", &[legs, spine, arms]));
}
/// The file's path in the ANSI code page, as cvr::paths::FileA gave it; none when empty.
fn settings_path() -> Option<CString> {
    let ansi = cvr_host::wide::ansi(&paths::file(Folder::Config, FILE));
    if ansi.is_empty() {
        return None;
    }
    CString::new(ansi).ok()
}

fn read_once() {
    if SWITCHES[0].load(Relaxed) >= 0 {
        return;
    }
    let values = settings_path().map_or([0; 3], |path| read_file(&path));
    SWITCHES[1].store(i32::from(values[1] != 0), Relaxed);
    SWITCHES[2].store(i32::from(values[2] != 0), Relaxed);
    SWITCHES[0].store(i32::from(values[0] != 0), Relaxed);
}
/// One switch, the file read first.
pub fn enabled(which: u32) -> bool {
    read_once();
    SWITCHES.get(which as usize).is_some_and(|s| s.load(Relaxed) == 1)
}
/// One switch set, the file written and the change logged.
pub fn set_enabled(which: u32, on: bool) {
    read_once();
    let Some(switch) = SWITCHES.get(which as usize) else { return };
    switch.store(i32::from(on), Relaxed);
    if let Some(path) = settings_path() {
        write_file(&path, [0, 1, 2].map(|i| SWITCHES[i].load(Relaxed) == 1));
    }
    cvr_host::log::write(&format!("[fbt] {}: {}\n", NAMES[which as usize], if on { "on" } else { "off" }));
}

// ---- the C ABI

#[no_mangle]
pub extern "C-unwind" fn cvr_fbt_switch(which: u32) -> bool {
    enabled(which)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_fbt_set_switch(which: u32, on: bool) {
    set_enabled(which, on);
}

#[cfg(test)]
mod tests {
    //! A file in the temp folder.
    use super::*;

    fn temp(name: &str) -> (std::path::PathBuf, CString) {
        let path = std::env::temp_dir().join(format!("cvr_fbt_{}_{name}.ini", std::process::id()));
        let c = CString::new(cvr_host::wide::ansi(&path)).unwrap();
        (path, c)
    }
    #[test]
    fn lines() {
        let mut v = [0i32; 3];
        unsafe {
            scan_line(c"legs=1".as_ptr(), &mut v);
            scan_line(c"spine= -3x".as_ptr(), &mut v);
            // A key not at the line's start, or without digits, leaves the value.
            scan_line(c" arms=1".as_ptr(), &mut v);
            scan_line(c"legs=x".as_ptr(), &mut v);
        }
        assert_eq!(v, [1, -3, 0]);
    }
    #[test]
    fn files() {
        let (path, c) = temp("rw");
        write_file(&c, [true, false, true]);
        assert_eq!(std::fs::read(&path).unwrap(), b"; Full-body tracking settings (src/Fbt/Game).\r\nlegs=1\r\nspine=0\r\narms=1\r\n");
        assert_eq!(read_file(&c), [1, 0, 1]);
        // The last match wins; a line longer than 127 bytes is read in pieces, the second piece a line of its own.
        let mut text = b"legs=1\r\nlegs=0\r\n".to_vec();
        text.extend_from_slice(&[b'x'; 127]);
        text.extend_from_slice(b"spine=4\r\narms=+2\r\n");
        std::fs::write(&path, &text).unwrap();
        assert_eq!(read_file(&c), [0, 4, 2]);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(read_file(&c), [0, 0, 0]);
    }
}
