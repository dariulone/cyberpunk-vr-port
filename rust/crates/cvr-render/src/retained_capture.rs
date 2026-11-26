//! The retained geometry's capture rows (src/Render/NativeRetainedGeometry.cpp, `CaptureMultilayerT5` and
//! `CaptureTables`): the multilayer t5 window the owned b4 bytes select -- the finite non-negative base below 2^32 at
//! byte 128 and the non-zero layer mask at byte 224 give the first element (row 16), the mask (17) and its width (18);
//! inside the view's elements (row 11 the first, 12 the count) the window's byte offset (14) and extent (15) at the
//! 128-byte stride, and whether it lies in the resource (19); and a captured table row's use counts (word 10 every
//! use's descriptors, words 11..14 the CBV, SRV, UAV and other ones).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_nrg_t5_window` and `cvr_nrg_table_uses` (the slot use
//! mirrored); the unit tests below cover each bound and each range type.

/// descriptors::SourceSlotUse.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SlotUse {
    pub offset: u32,
    pub count: u32,
    pub kind: i32,
}
const _: () = assert!(core::mem::size_of::<SlotUse>() == 12);

/// The selected window, written into the t5 row.
pub fn t5_window(b4: &[u8], width: u64, row: &mut [u64; 32]) {
    if b4.len() < 228 {
        return;
    }
    let base = f32::from_le_bytes(b4[128..132].try_into().unwrap());
    let mask = u32::from_le_bytes(b4[224..228].try_into().unwrap());
    if !(base.is_finite() && base >= 0.0 && f64::from(base) < 4294967296.0 && mask != 0) {
        return;
    }
    row[16] = u64::from(base as u32);
    row[17] = u64::from(mask);
    row[18] = u64::from(32 - mask.leading_zeros());
    if row[16] <= row[12] && row[18] <= row[12] - row[16] && row[11] <= u64::MAX - row[16] && row[11] + row[16] <= u64::MAX / 128 {
        row[14] = (row[11] + row[16]) * 128;
        row[15] = row[18] * 128;
        row[19] = u64::from(row[14] <= width && row[15] <= width - row[14]);
    }
}
/// The table row's use counts (D3D12_DESCRIPTOR_RANGE_TYPE: SRV 0, UAV 1, CBV 2).
pub fn table_uses(uses: &[SlotUse], value: &mut [u64; 16]) {
    for u in uses {
        let count = u64::from(u.count);
        value[10] = value[10].wrapping_add(count);
        let at = 11 + match u.kind {
            2 => 0,
            0 => 1,
            1 => 2,
            _ => 3,
        };
        value[at] = value[at].wrapping_add(count);
    }
}

// ---- the C ABI

/// # Safety
/// `b4` holds `bytes` bytes (or is null with 0); `row` holds 32 values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_t5_window(b4: *const u8, bytes: usize, width: u64, row: *mut [u64; 32]) {
    let b4 = if b4.is_null() || bytes == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(b4, bytes) } };
    t5_window(b4, width, unsafe { &mut *row });
}
/// # Safety
/// `uses` holds `count` uses (or is null with 0); `value` holds 16 values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_table_uses(uses: *const SlotUse, count: usize, value: *mut [u64; 16]) {
    let uses = if uses.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(uses, count) } };
    table_uses(uses, unsafe { &mut *value });
}

#[cfg(test)]
mod tests {
    //! Each bound and each range type.
    use super::*;

    fn b4(base: f32, mask: u32) -> Vec<u8> {
        let mut bytes = vec![0u8; 228];
        bytes[128..132].copy_from_slice(&base.to_le_bytes());
        bytes[224..228].copy_from_slice(&mask.to_le_bytes());
        bytes
    }
    fn view(first: u64, count: u64) -> [u64; 32] {
        let mut row = [0u64; 32];
        row[11] = first;
        row[12] = count;
        row
    }
    #[test]
    fn windows() {
        let mut row = view(2, 10);
        t5_window(&b4(3.7, 0b1011), 1 << 20, &mut row);
        assert_eq!(row[14..20], [5 * 128, 4 * 128, 3, 0b1011, 4, 1]);
        let mut row = view(2, 10);
        t5_window(&b4(3.0, 0b1011), 5 * 128 + 4 * 128, &mut row);
        assert_eq!(row[19], 1);
        let mut row = view(2, 10);
        t5_window(&b4(3.0, 0b1011), 5 * 128 + 4 * 128 - 1, &mut row);
        assert_eq!((row[14], row[19]), (640, 0));
        // The window past the view's elements: the selection stays, no window.
        let mut row = view(2, 6);
        t5_window(&b4(3.0, 0b1011), 1 << 20, &mut row);
        assert_eq!((row[16], row[18], row[14], row[15]), (3, 4, 0, 0));
        let mut row = view(2, 7);
        t5_window(&b4(3.0, 0b1011), 1 << 20, &mut row);
        assert_eq!(row[15], 512);
        let mut row = view(2, 2);
        t5_window(&b4(3.0, 1), 1 << 20, &mut row);
        assert_eq!(row[14], 0);
        let mut row = view(u64::MAX / 128, 10);
        t5_window(&b4(1.0, 1), u64::MAX, &mut row);
        assert_eq!(row[14], 0);
        let mut row = view(u64::MAX / 128 - 1, 10);
        t5_window(&b4(1.0, 1), u64::MAX, &mut row);
        assert_eq!(row[14], u64::MAX / 128 * 128);
        for (base, mask) in [(-1.0, 1), (f32::NAN, 1), (f32::INFINITY, 1), (4294967296.0, 1), (1.0, 0)] {
            let mut row = view(0, 10);
            t5_window(&b4(base, mask), 1 << 20, &mut row);
            assert_eq!(row, view(0, 10), "{base} {mask}");
        }
        let mut row = view(0, 10);
        t5_window(&b4(-0.0, 1), 1 << 20, &mut row);
        assert_eq!((row[16], row[17]), (0, 1));
        let mut row = view(0, 10);
        t5_window(&b4(1.0, 1)[..227], 1 << 20, &mut row);
        assert_eq!(row, view(0, 10));
    }
    #[test]
    fn uses() {
        let mut value = [0u64; 16];
        let list = [
            SlotUse { offset: 0, count: 2, kind: 2 },
            SlotUse { offset: 2, count: 3, kind: 0 },
            SlotUse { offset: 5, count: 4, kind: 1 },
            SlotUse { offset: 9, count: 5, kind: 3 },
            SlotUse { offset: 14, count: 1, kind: 0 },
        ];
        table_uses(&list, &mut value);
        assert_eq!(value[10..15], [15, 2, 4, 4, 5]);
    }
}
