//! The native IA snapshot's record inspection (src/Render/NativeIaSnapshot.cpp, `ia_snapshot::Inspect`): once a
//! source's buffer record is read, its observation takes the record (known, its low type, allocation and alias words),
//! the record's pool slot, a geometry pool's packed identity for a kind-1 pooled vertex buffer without allocation or
//! alias (when that code is known), and the allocation it resolves to (the record's own, else the pool owner, else the
//! alias's allocation).
//!
//! In the Rust build (`CVR_RUST_VRIK`) that step forwards to `cvr_iar_inspect_record` through the .cpp's reader;
//! `tools/single_pass_tests/native_ia_snapshot_test` runs its cases against it in its Rust variant, and the unit test
//! below inspects a fabricated record.

use core::ffi::c_void;

use crate::buffer_read::NativeBuffer;
use crate::ia_snapshot::{read_geometry_pool, GeometryPool, Observation};
use crate::source_cbv_details::{read_alias, read_pool, reader, Reader, Record, KNOWN};

const RECORD_KNOWN: u64 = 1;
const GEOMETRY_POOL_KNOWN: u64 = 4096;

fn field_u64(raw: &[u8; 0xB0], at: usize) -> u64 {
    u64::from_le_bytes(raw[at..at + 8].try_into().unwrap())
}

/// Inspect's record step.
#[allow(clippy::too_many_arguments)]
pub fn inspect_record(
    read: Reader,
    user: *mut c_void,
    base: u64,
    native: &NativeBuffer,
    raw: &[u8; 0xB0],
    a: &mut Observation,
    geometry_compatible: bool,
    geometry_pool: &mut GeometryPool,
) {
    let mem = reader(read, user);
    a.flags |= RECORD_KNOWN;
    a.low_type = (raw[6] & 15) as u64;
    a.allocation = field_u64(raw, 0x18);
    a.alias_payload = field_u64(raw, 0x20);
    a.alias_control = field_u64(raw, 0x28);
    let record = Record { observation: KNOWN, address: a.record_address, bytes: *raw };
    let pool = read_pool(&mem, base, native, &record, a.gpu, a.width);
    a.pool_observation = pool.observation as u64;
    a.pool_owner = pool.owner;
    a.pool_slot = pool.slot as u64;
    a.pool_unit = pool.unit as u64;
    if native.kind == 1 && a.low_type == 5 && a.allocation == 0 && a.alias_payload == 0 && a.alias_control == 0 && geometry_compatible {
        let found = read_geometry_pool(read, user, base, native, raw, a.gpu, a.width, a.view_gpu, a.view_bytes, 5);
        *geometry_pool = found.unwrap_or_default();
        if let Some(gp) = found {
            a.flags |= GEOMETRY_POOL_KNOWN;
            a.pool_observation = KNOWN as u64;
            a.pool_owner = gp.owner;
            a.pool_slot = gp.slot as u64;
            a.pool_unit = gp.unit as u64;
            a.reserved = gp.packed as u64;
        }
    }
    if a.allocation == 0 && a.pool_observation == KNOWN as u64 {
        a.allocation = a.pool_owner;
    }
    if a.allocation == 0 && (a.alias_payload != 0 || a.alias_control != 0) {
        let alias = read_alias(&mem, a.alias_payload, a.alias_control);
        if alias.observation == KNOWN {
            a.allocation = alias.allocation;
        }
    }
}

// ---- the C ABI

/// # Safety
/// `read` reads this process's memory for `user`; the other pointers are valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_iar_inspect_record(
    read: Reader,
    user: *mut c_void,
    base: u64,
    native: *const NativeBuffer,
    raw: *const [u8; 0xB0],
    a: *mut Observation,
    geometry_compatible: bool,
    geometry_pool: *mut GeometryPool,
) {
    unsafe { inspect_record(read, user, base, &*native, &*raw, &mut *a, geometry_compatible, &mut *geometry_pool) };
}

#[cfg(test)]
mod tests {
    //! A fabricated kind-0 record: its words recorded, no pool, no geometry pool, no alias.
    use super::*;

    unsafe extern "C" fn guarded(_: *mut c_void, address: u64, out: *mut c_void, bytes: usize) -> bool {
        crate::instance_layout::read(address, unsafe { core::slice::from_raw_parts_mut(out as *mut u8, bytes) })
    }

    #[test]
    fn record_words() {
        let mut raw = [0u8; 0xB0];
        raw[6] = 0x05;
        raw[0x18..0x20].copy_from_slice(&0x1234u64.to_le_bytes());
        let native = NativeBuffer { kind: 0, ..Default::default() };
        let mut a = Observation { record_address: 0x10, ..Default::default() };
        let mut gp = GeometryPool { owner: 9, ..Default::default() };
        inspect_record(Some(guarded), core::ptr::null_mut(), 0, &native, &raw, &mut a, true, &mut gp);
        assert_eq!((a.flags, a.low_type, a.allocation, a.alias_payload), (RECORD_KNOWN, 5, 0x1234, 0));
        assert_ne!(a.pool_observation, KNOWN as u64);
        assert_eq!(gp.owner, 9);
        let mut b = Observation::default();
        raw[0x18..0x20].copy_from_slice(&0u64.to_le_bytes());
        raw[0x20..0x28].copy_from_slice(&1u64.to_le_bytes());
        inspect_record(Some(guarded), core::ptr::null_mut(), 0, &native, &raw, &mut b, true, &mut gp);
        assert_eq!((b.alias_payload, b.allocation), (1, 0));
    }
}
