//! The descriptor audit's binding lookup and value admissions (src/Render/StereoDescriptorAudit.cpp, `descriptors::`):
//! FindBinding's search of a supported root's ranges for the one table slot a shader register of a type, space and
//! visibility lands in (a finite range whose registers stay inside 32 bits, or an unbounded one; exactly one match below
//! parameter 64 and inside 32 bits), and the canonical values a CBV address (kind 1: a 256-aligned address and a
//! 256-aligned size of at most 64 KiB) or a descriptor stamp (that CBV, an explicit SRV of a tracked resource, or a
//! sampler) admits.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp's FindBinding search, CbvAddress and DescriptorAddress value checks
//! forward to `cvr_sda_*`; `tools/single_pass_tests/descriptor_audit_test` runs its cases against them in its Rust variant.

use crate::descriptor_store::Value;
use crate::source_layout::Binding;

/// FindBinding's search: (parameter, table offset) of the one range holding the register, or None.
pub fn find_binding(ranges: &[Binding], kind: u32, space: u32, register: u32, visibility: u32) -> Option<(u32, u32)> {
    const ALL: u32 = 0;
    let mut found: Option<(u32, u32)> = None;
    for range in ranges {
        if range.kind != kind || range.space != space || (range.visibility != ALL && range.visibility != visibility) || register < range.shader_register {
            continue;
        }
        let relative = register as u64 - range.shader_register as u64;
        if range.count != u32::MAX {
            if range.shader_register as u64 + range.count as u64 > u32::MAX as u64 + 1 {
                return None;
            }
            if relative >= range.count as u64 {
                continue;
            }
        }
        let offset = range.table_offset as u64 + relative;
        if range.parameter >= 64 || offset > u32::MAX as u64 || found.is_some() {
            return None;
        }
        found = Some((range.parameter, offset as u32));
    }
    found
}

/// A CBV value an address may resolve to (CbvAddress, DescriptorAddress).
pub fn cbv_admitted(v: &Value) -> bool {
    v[0] == 1 && v[4] != 0 && v[5] != 0 && v[5] <= 65536 && v[5] & 255 == 0 && v[4] & 255 == 0
}
/// DescriptorAddress's admission: that CBV, an explicit SRV of a tracked resource (`tracked` its cookie's), a sampler.
pub fn descriptor_admitted(v: &Value, tracked: impl Fn(u64) -> bool) -> bool {
    match v[0] {
        1 => cbv_admitted(v),
        2 => v[1] & 3 == 1 && v[2] != 0 && tracked(v[2]),
        4 => true,
        _ => false,
    }
}

// ---- the C ABI

/// # Safety
/// `ranges` holds `count` ranges (or is null); the outputs are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_find_binding(
    ranges: *const Binding,
    count: usize,
    kind: u32,
    space: u32,
    register: u32,
    visibility: u32,
    parameter: *mut u32,
    table_offset: *mut u32,
) -> bool {
    let ranges = if ranges.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(ranges, count) } };
    match find_binding(ranges, kind, space, register, visibility) {
        Some((p, o)) => {
            unsafe {
                *parameter = p;
                *table_offset = o;
            }
            true
        }
        None => false,
    }
}
/// `which` 0: a CBV value an address may resolve to; 1: DescriptorAddress's admission (`tracked` the resource's).
///
/// # Safety
/// `value` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_admitted(which: u32, value: *const Value, tracked: bool) -> bool {
    let v = unsafe { &*value };
    if which == 0 {
        cbv_admitted(v)
    } else {
        descriptor_admitted(v, |_| tracked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(parameter: u32, kind: u32, register: u32, count: u32, offset: u32, visibility: u32) -> Binding {
        Binding { parameter, kind, shader_register: register, count, table_offset: offset, visibility, ..Binding::default() }
    }

    #[test]
    fn one_match_or_none() {
        let ranges = [range(0, 0, 2, 4, 1, 0), range(1, 2, 0, u32::MAX, 3, 5)];
        assert_eq!(find_binding(&ranges, 0, 0, 3, 5), Some((0, 2)));
        assert_eq!(find_binding(&ranges, 0, 0, 6, 5), None, "past the finite range");
        assert_eq!(find_binding(&ranges, 2, 0, 100, 5), Some((1, 103)), "an unbounded range");
        assert_eq!(find_binding(&ranges, 2, 0, 100, 4), None, "another stage's range");
        let twice = [range(0, 0, 0, 4, 0, 0), range(1, 0, 2, 4, 0, 0)];
        assert_eq!(find_binding(&twice, 0, 0, 3, 5), None, "two matches");
        let wrapping = [range(0, 0, u32::MAX - 1, 4, 0, 0)];
        assert_eq!(find_binding(&wrapping, 0, 0, u32::MAX, 5), None, "registers past 32 bits");
    }
    #[test]
    fn admissions() {
        let mut v: Value = [0; 20];
        v[0] = 1;
        v[4] = 0x1000;
        v[5] = 256;
        assert!(cbv_admitted(&v));
        v[5] = 257;
        assert!(!cbv_admitted(&v));
        let mut srv: Value = [0; 20];
        srv[0] = 2;
        srv[1] = 1 | 4;
        srv[2] = 7;
        assert!(descriptor_admitted(&srv, |c| c == 7) && !descriptor_admitted(&srv, |_| false));
        srv[1] = 3;
        assert!(!descriptor_admitted(&srv, |_| true), "an implicit view");
    }
}
