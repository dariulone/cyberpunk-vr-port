//! The C ABI of vrcam.json's scanners (`vrcam_config`, already Rust's) for `include/Stereo/VrcamConfig.hpp`: in the Rust
//! build (`CVR_RUST_VRIK`) its pure helpers -- the CName hash, the string / string-array scanners, the derived camera
//! name, the enable request's value and the resolution tail -- forward here, writing their outputs where and when the C++
//! ones did; the file and bridge I/O stays with the caller. `tools/vrcam_config_tests` runs its cases against them in
//! its Rust variant.

use crate::vrcam_config as vc;

unsafe fn c_bytes<'a>(text: *const u8) -> &'a [u8] {
    if text.is_null() {
        return &[];
    }
    unsafe { core::ffi::CStr::from_ptr(text.cast()).to_bytes() }
}
unsafe fn bytes<'a>(text: *const u8, len: usize) -> &'a [u8] {
    if text.is_null() || len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(text, len) }
    }
}

/// cname_hash (a null name hashes as empty).
///
/// # Safety
/// `name` is null or a C string.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_vrcam_cname_hash(name: *const u8) -> u64 {
    unsafe { vc::cname_hash(c_bytes(name)) }
}
/// json_find_string_span (`value_pos` / `value_len` written on success).
///
/// # Safety
/// `text` holds `len` bytes; `key` is a UTF-8 C string; the outputs are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_vrcam_string_span(
    text: *const u8,
    len: usize,
    key: *const u8,
    value_pos: *mut usize,
    value_len: *mut usize,
) -> bool {
    unsafe {
        let Ok(key) = core::str::from_utf8(c_bytes(key)) else {
            return false;
        };
        match vc::json_string_span(bytes(text, len), key) {
            Some((at, length)) => {
                *value_pos = at;
                *value_len = length;
                true
            }
            None => false,
        }
    }
}
/// json_find_string_array: the strings' spans (`out` gets up to `capacity` (start, length) pairs); -1 for no array.
///
/// # Safety
/// `text` holds `len` bytes; `key` is a UTF-8 C string; `out` has room for `capacity` pairs.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_vrcam_string_array(
    text: *const u8,
    len: usize,
    key: *const u8,
    out: *mut [usize; 2],
    capacity: usize,
) -> isize {
    unsafe {
        let Ok(key) = core::str::from_utf8(c_bytes(key)) else {
            return -1;
        };
        let Some(spans) = vc::json_string_array_spans(bytes(text, len), key) else {
            return -1;
        };
        for (i, &(at, length)) in spans.iter().take(capacity).enumerate() {
            *out.add(i) = [at, length];
        }
        spans.len() as isize
    }
}
/// vrcam_derive_camera with `_snprintf_s(out, out_size, _TRUNCATE, "vrcam_feed_%s", ...)`'s writes.
///
/// # Safety
/// `component` is null or a C string; `out` has room for `out_size` bytes.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_vrcam_derive_camera(
    component: *const u8,
    out: *mut u8,
    out_size: usize,
) -> bool {
    unsafe {
        if component.is_null() {
            return false;
        }
        let Some(suffix) = c_bytes(component).strip_prefix(b"vrcam_") else {
            return false;
        };
        if out_size == 0 {
            return false;
        }
        let mut camera = b"vrcam_feed_".to_vec();
        camera.extend_from_slice(suffix);
        let fits = camera.len() < out_size;
        let written = if fits { camera.len() } else { out_size - 1 };
        core::ptr::copy_nonoverlapping(camera.as_ptr(), out, written);
        *out.add(written) = 0;
        // The UCRT's _snprintf_s also terminates the buffer's last byte, whatever the length.
        *out.add(out_size - 1) = 0;
        fits
    }
}
/// vrcam_enable_value: 0 for a body with a '0', else 1 for one with a '1', else -1 (and -1 for none).
///
/// # Safety
/// `body` is null or a C string.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_vrcam_enable_value(body: *const u8) -> i32 {
    if body.is_null() {
        return -1;
    }
    let body = unsafe { c_bytes(body) };
    if body.contains(&b'0') {
        0
    } else if body.contains(&b'1') {
        1
    } else {
        -1
    }
}
/// vrcam_parse_resolution (`w`, `h` written on success).
///
/// # Safety
/// `component` is null or a C string; `w` and `h` are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_vrcam_parse_resolution(
    component: *const u8,
    w: *mut i32,
    h: *mut i32,
) -> bool {
    unsafe {
        if component.is_null() {
            return false;
        }
        match vc::parse_resolution(c_bytes(component)) {
            Some((a, b)) => {
                *w = a;
                *h = b;
                true
            }
            None => false,
        }
    }
}
