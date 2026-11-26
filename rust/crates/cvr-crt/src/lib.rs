//! The C runtime the C++ half used, called as it called it -- the ini loop: `_fsopen(path, "r", _SH_DENYNO)`, `fgets`,
//! `sscanf_s` (through UCRT's `__stdio_common_vsscanf`, which the inline `sscanf_s` of <stdio.h> calls), `_strnicmp`
//! and `atoi`. On x64 a `va_list` is a pointer to the 8-byte argument slots, so the arguments are an array of pointers.
use core::ffi::{c_char, c_int, c_void, CStr};
use std::ffi::CString;

extern "C" {
    fn __stdio_common_vsscanf(options: u64, buffer: *const c_char, count: usize, format: *const c_char, locale: *mut c_void, args: *const *mut c_void) -> c_int;
    fn _strnicmp(a: *const c_char, b: *const c_char, count: usize) -> c_int;
    fn atoi(text: *const c_char) -> c_int;
    fn strlen(text: *const c_char) -> usize;
    fn _fsopen(name: *const c_char, mode: *const c_char, share: c_int) -> *mut c_void;
    fn fgets(buffer: *mut c_char, count: c_int, file: *mut c_void) -> *mut c_char;
    fn fclose(file: *mut c_void) -> c_int;
    fn ferror(file: *mut c_void) -> c_int;
    fn fopen_s(file: *mut *mut c_void, name: *const c_char, mode: *const c_char) -> c_int;
    fn fputs(text: *const c_char, file: *mut c_void) -> c_int;
    fn __stdio_common_vsprintf(options: u64, buffer: *mut c_char, count: usize, format: *const c_char, locale: *mut c_void, args: *const u64) -> c_int;
}
/// What <stdio.h>'s inline snprintf passes in this module: the C runtime's startup leaves the local printf options at
/// _CRT_INTERNAL_PRINTF_LEGACY_WIDE_SPECIFIERS | _CRT_INTERNAL_PRINTF_STANDARD_ROUNDING (0x24: exact ties round to even,
/// 0.203125 at %.5f is 0.20312), and snprintf adds _CRT_INTERNAL_PRINTF_STANDARD_SNPRINTF_BEHAVIOR (the length wanted when
/// the buffer is short).
const PRINTF_STANDARD_SNPRINTF: u64 = 0x24 | 0x2;

/// A printf argument: a `%f` takes a double (a float is promoted), a `%d` an int, a `%llu` a 64-bit integer, a `%s` a
/// C string.
#[derive(Clone, Copy, Debug)]
pub enum Arg {
    F(f64),
    I(i32),
    U(u64),
    S(*const c_char),
}
/// `snprintf(format, args...)` as the C runtime formats it, into a C string.
pub fn format(format: &CStr, args: &[Arg]) -> CString {
    // On x64 a va_list is the 8-byte slots one after another.
    let slots: Vec<u64> = args
        .iter()
        .map(|a| match *a {
            Arg::F(v) => v.to_bits(),
            Arg::I(v) => v as u32 as u64,
            Arg::U(v) => v,
            Arg::S(p) => p as u64,
        })
        .collect();
    let mut buffer = vec![0 as c_char; 256];
    loop {
        // SAFETY: a buffer of the size passed; the slots as the format reads them (the caller's format and arguments).
        let n = unsafe {
            __stdio_common_vsprintf(PRINTF_STANDARD_SNPRINTF, buffer.as_mut_ptr(), buffer.len(), format.as_ptr(), core::ptr::null_mut(), slots.as_ptr())
        };
        if n < 0 {
            return CString::default();
        }
        if (n as usize) < buffer.len() {
            buffer.truncate(n as usize);
            let bytes: Vec<u8> = buffer.iter().map(|&c| c as u8).collect();
            return CString::new(bytes).unwrap_or_default();
        }
        buffer.resize(n as usize + 1, 0);
    }
}
/// _CRT_INTERNAL_SCANF_SECURECRT: sscanf_s, with the module's local scanf options as the C runtime's startup leaves them
/// (_CRT_INTERNAL_SCANF_LEGACY_WIDE_SPECIFIERS).
const SCANF_SECURE: u64 = 1 | 0x2;
/// share.h _SH_DENYNO.
const SH_DENYNO: c_int = 0x40;

/// An argument slot for `scan`.
pub fn ptr<T>(x: &mut T) -> *mut c_void {
    x as *mut T as *mut c_void
}
/// `sscanf_s(line, format, args...)`: the number of fields assigned, or EOF.
///
/// # Safety
/// `line` is a C string; `args` point to the types `format` converts into.
pub unsafe fn scan(line: *const c_char, format: &CStr, args: &[*mut c_void]) -> i32 {
    // SAFETY: as the caller vouches.
    unsafe { __stdio_common_vsscanf(SCANF_SECURE, line, usize::MAX, format.as_ptr(), core::ptr::null_mut(), args.as_ptr()) }
}
/// PollLiveControls' tryParseIntKey: the key (any case), blanks, '=', blanks, then `atoi`.
///
/// # Safety
/// `text` is a C string.
pub unsafe fn try_parse_int_key(text: *const c_char, key: &CStr, out: &mut i32) -> bool {
    if text.is_null() {
        return false;
    }
    // SAFETY: C strings, read up to their terminators.
    unsafe {
        let key_len = strlen(key.as_ptr());
        if _strnicmp(text, key.as_ptr(), key_len) != 0 {
            return false;
        }
        let mut cursor = text.add(key_len);
        while *cursor == b' ' as c_char || *cursor == b'\t' as c_char {
            cursor = cursor.add(1);
        }
        if *cursor != b'=' as c_char {
            return false;
        }
        cursor = cursor.add(1);
        while *cursor == b' ' as c_char || *cursor == b'\t' as c_char {
            cursor = cursor.add(1);
        }
        *out = atoi(cursor);
    }
    true
}
/// std::clamp(v, lo, hi): `v < lo ? lo : hi < v ? hi : v` (a NaN passes through).
pub fn cpp_clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}
/// src/Core/VrCore.cpp ClampRuntimeMode.
pub fn clamp_runtime_mode(value: i32) -> i32 {
    if value == 1 {
        1
    } else {
        0
    }
}
/// include/Hooks/ReflexOptions.hpp cvr::reflex::NormalizeMode.
pub fn normalize_reflex_mode(mode: i32) -> i32 {
    if (-1..=2).contains(&mode) {
        mode
    } else {
        -1
    }
}

/// `fputs(text, file)` on a FILE the C++ half opened.
///
/// # Safety
/// `file` is an open C runtime FILE.
pub unsafe fn fputs_to(file: *mut c_void, text: &CStr) {
    // SAFETY: the caller's open FILE and a C string.
    unsafe { fputs(text.as_ptr(), file) };
}
/// std::lround(float): the C runtime's lroundf (halfway cases away from zero).
pub fn lroundf(x: f32) -> i32 {
    extern "C" {
        #[link_name = "lroundf"]
        fn crt_lroundf(x: f32) -> c_int;
    }
    // SAFETY: a pure C runtime function.
    unsafe { crt_lroundf(x) }
}

/// A C runtime FILE opened as the C++ loop opened the ini.
pub struct File(*mut c_void);
impl File {
    /// # Safety
    /// `path` is a C string.
    pub unsafe fn open_shared(path: *const c_char) -> Option<File> {
        // SAFETY: a C string path and a literal mode.
        let f = unsafe { _fsopen(path, c"r".as_ptr(), SH_DENYNO) };
        (!f.is_null()).then(|| File(f))
    }
    /// `_fsopen(path, mode, _SH_DENYNO)`, as the C++ half opened the files it writes.
    ///
    /// # Safety
    /// `path` is a C string.
    pub unsafe fn open_shared_mode(path: *const c_char, mode: &CStr) -> Option<File> {
        // SAFETY: a C string path and mode.
        let f = unsafe { _fsopen(path, mode.as_ptr(), SH_DENYNO) };
        (!f.is_null()).then(|| File(f))
    }
    /// `fopen_s(&file, path, mode)`: the C runtime's own sharing and text-mode newline translation.
    ///
    /// # Safety
    /// `path` is a C string.
    pub unsafe fn open(path: *const c_char, mode: &CStr) -> Option<File> {
        let mut f = core::ptr::null_mut();
        // SAFETY: a C string path and mode, the FILE written to `f`.
        let err = unsafe { fopen_s(&mut f, path, mode.as_ptr()) };
        (err == 0 && !f.is_null()).then(|| File(f))
    }
    /// `fputs(text, file)`.
    pub fn puts(&mut self, text: &CStr) {
        // SAFETY: an open FILE and a C string.
        unsafe { fputs(text.as_ptr(), self.0) };
    }
    /// The next `fgets(line, sizeof line, file)`; false at the end of the file.
    pub fn gets(&mut self, line: &mut [c_char; 128]) -> bool {
        // SAFETY: an open FILE and a buffer of the size passed.
        unsafe { !fgets(line.as_mut_ptr(), 128, self.0).is_null() }
    }
    /// The C runtime FILE, for a writer of the C++ half that writes into the same file.
    pub fn as_ptr(&self) -> *mut c_void {
        self.0
    }
    /// The next `fgets(line, sizeof line, file)` into a buffer of another size; false at the end of the file.
    pub fn gets_into(&mut self, line: &mut [c_char]) -> bool {
        // SAFETY: an open FILE and a buffer of the size passed.
        unsafe { !fgets(line.as_mut_ptr(), line.len() as c_int, self.0).is_null() }
    }
}
impl File {
    /// `ferror` then `fclose`: true when every write reached the file and it closed.
    pub fn close(self) -> bool {
        let f = self.0;
        core::mem::forget(self);
        // SAFETY: an open FILE, closed once here.
        unsafe {
            let written = ferror(f) == 0;
            fclose(f) == 0 && written
        }
    }
}
impl Drop for File {
    fn drop(&mut self) {
        // SAFETY: opened by open_shared or open, closed once.
        unsafe { fclose(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scans_as_the_crt_does() {
        let mut v = 0.0f32;
        let mut i = 0i32;
        unsafe {
            assert_eq!(scan(c"xr_world_scale = 1.25".as_ptr(), c"xr_world_scale = %f", &[ptr(&mut v)]), 1);
            assert_eq!(v, 1.25);
            assert_eq!(scan(c"xr_mask=1f".as_ptr(), c"xr_mask=%x", &[ptr(&mut i)]), 1);
            assert_eq!(i, 0x1f);
            assert_eq!(scan(c"xr_other=3".as_ptr(), c"xr_mask=%d", &[ptr(&mut i)]), 0);
            let (mut a, mut b, mut c) = (0.0f32, 0.0f32, 0.0f32);
            assert_eq!(scan(c"xr_scanner_frame=0.5,0.25,1".as_ptr(), c"xr_scanner_frame=%f,%f,%f", &[ptr(&mut a), ptr(&mut b), ptr(&mut c)]), 3);
            assert_eq!((a, b, c), (0.5, 0.25, 1.0));
            assert!(try_parse_int_key(c"XR_NVIDIA_REFLEX \t= -1".as_ptr(), c"xr_nvidia_reflex", &mut i));
            assert_eq!(i, -1);
        }
    }
    #[test]
    fn formats_as_the_crt_does() {
        let s = format(c"scaleR=%.4f\nfirstSetup=%d\n%s", &[Arg::F(1.05f32 as f64), Arg::I(1), Arg::S(c"x".as_ptr())]);
        assert_eq!(s.to_str().unwrap(), "scaleR=1.0500\nfirstSetup=1\nx");
        let long = format(c"%.4f", &[Arg::F(1e300)]);
        assert!(long.to_bytes().len() > 300, "grown past the first buffer");
    }
}

#[cfg(test)]
mod rounding_tests {
    use super::*;
    #[test]
    fn exact_ties_round_as_msvc_printf_does() {
        // An MSVC program: printf("%.5f %.2f %.1f", 0.203125, 0.125, 0.25) prints "0.20312 0.12 0.2".
        let text = format(c"%.5f %.2f %.1f", &[Arg::F(0.203125), Arg::F(0.125), Arg::F(0.25)]);
        assert_eq!(text.to_str().unwrap(), "0.20312 0.12 0.2");
    }
}
