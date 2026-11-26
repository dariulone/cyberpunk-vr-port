//! THE MULTILAYER PIXEL SHADER'S SELECTOR READ PLAN (src/Render/MultilayerSelectorPlan.cpp): for the exact original
//! multilayer family (root / VS / PS hashes) and its exact b4 logical bytes (228) and explicit structured-128 t5 view,
//! the possible selector records -- the FPToui of the b4 base (finite, nonnegative, below 2^32, truncated) plus every
//! bit of the b4 mask, inside the view -- and their compact COM subview (at most 32 records, 4 KiB); then, over exactly
//! that subview, the ten audited selector sites of every possible record (raw, low or high 16 bits of fields +100,
//! +104, +108), each below the declared 32768 SRVs, and the table offsets they select with their contiguous uses.
//! A conservative superset only: no wave / pixel simulation, ownership, state or copy permission. The C++ module keeps
//! its strings and assembles its results; tools/single_pass_tests/multilayer_selector_plan_test runs its cases against
//! these in its Rust variant.
use std::collections::BTreeSet;

pub const ROOT_SHA256: &str = "030ea7831db8493ad9b03f05ae54aa8d70e27211b65a93f9c9e155a925110a7c";
pub const VERTEX_SHA256: &str = "d13385d2f6f7206586c8927d9df03e9b2c6b83adb27e6d23c7589f7f496e31aa";
pub const PIXEL_SHA256: &str = "9a4947a3ec0eb3d4605d88b663bb27a0bc809fe8a3d91c4ae67886bd77148db8";
pub const B4_BYTES: usize = 228;
pub const STRIDE: u64 = 128;
pub const MAX_RECORDS: u32 = 32;
pub const MAX_READ_BYTES: u32 = 4096;
pub const DECLARED_EXTENT: u32 = 32768;

#[derive(Clone, Copy)]
enum Transform {
    Raw,
    Low16,
    High16,
}
/// The audited sites: (field offset, transform), in the C++ order.
const SITES: [(usize, Transform); 10] = [
    (100, Transform::Raw),
    (108, Transform::High16),
    (108, Transform::Low16),
    (104, Transform::Low16),
    (104, Transform::High16),
    (100, Transform::Raw),
    (108, Transform::High16),
    (108, Transform::Low16),
    (104, Transform::Low16),
    (104, Transform::High16),
];

/// A string lent by C++.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Str {
    pub ptr: *const u8,
    pub len: usize,
}
impl Str {
    unsafe fn bytes<'a>(self) -> &'a [u8] {
        if self.len == 0 || self.ptr.is_null() {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
        }
    }
}
/// StructuredView, lent.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct View {
    pub root: Str,
    pub visibility: u32,
    pub shader_register: u32,
    pub space: u32,
    pub parameter: u32,
    pub table_offset: u32,
    pub num_elements: u32,
    pub first_element: u64,
    pub resource_width: u64,
    pub structure_byte_stride: u32,
    pub format: u32,
    pub dimension: u32,
    pub component_mapping: u32,
    pub flags: u32,
    pub explicit_description: bool,
}
/// The family and b4 key, lent.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Identity {
    pub family_root: Str,
    pub family_vertex: Str,
    pub family_pixel: Str,
    pub b4_root: Str,
    pub b4_visibility: u32,
    pub b4_register: u32,
    pub b4_space: u32,
    pub b4_parameter: u32,
    pub b4_table_offset: u32,
    pub b4: *const u8,
    pub b4_len: usize,
}
/// A read plan's numbers.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PlanNumbers {
    pub base_record: u32,
    pub mask: u32,
    pub first_record: u32,
    pub record_span: u32,
    pub resource_offset: u64,
    pub byte_extent: u32,
}
const _: () = assert!(
    core::mem::size_of::<Str>() == 16
        && core::mem::size_of::<View>() == 80
        && core::mem::size_of::<Identity>() == 104
        && core::mem::size_of::<PlanNumbers>() == 32
);

type Fail = &'static str;
fn require(value: bool, message: Fail) -> Result<(), Fail> {
    if value {
        Ok(())
    } else {
        Err(message)
    }
}
fn dword(bytes: &[u8], offset: usize) -> Result<u32, Fail> {
    require(
        offset <= bytes.len() && 4 <= bytes.len() - offset,
        "Selector DWORD exceeds its exact view bytes",
    )?;
    Ok(u32::from_le_bytes(
        bytes[offset..offset + 4].try_into().unwrap(),
    ))
}
unsafe fn same_view(a: &View, b: &View) -> bool {
    let same_root = unsafe { a.root.bytes() == b.root.bytes() };
    same_root
        && a.visibility == b.visibility
        && a.shader_register == b.shader_register
        && a.space == b.space
        && a.parameter == b.parameter
        && a.table_offset == b.table_offset
        && a.first_element == b.first_element
        && a.resource_width == b.resource_width
        && a.num_elements == b.num_elements
        && a.structure_byte_stride == b.structure_byte_stride
        && a.format == b.format
        && a.dimension == b.dimension
        && a.component_mapping == b.component_mapping
        && a.flags == b.flags
        && a.explicit_description == b.explicit_description
}

unsafe fn check_view(view: &View) -> Result<(), Fail> {
    require(
        unsafe { view.root.bytes() } == ROOT_SHA256.as_bytes()
            && view.visibility == 5
            && view.shader_register == 5
            && view.space == 0
            && view.parameter == 23
            && view.table_offset == 5,
        "Multilayer source is not exact original PS t5 binding",
    )?;
    require(
        view.explicit_description
            && view.dimension == 1
            && view.format == 0
            && view.component_mapping == 5768
            && view.flags == 0
            && view.structure_byte_stride as u64 == STRIDE
            && view.num_elements != 0
            && view.resource_width != 0,
        "Multilayer source is not an explicit structured128 SRV",
    )?;
    require(
        view.first_element <= u64::MAX / STRIDE,
        "Structured FirstElement byte offset overflows",
    )?;
    let start = view.first_element * STRIDE;
    let size = view.num_elements as u64 * STRIDE;
    require(
        start <= view.resource_width && size <= view.resource_width - start,
        "Full structured SRV view exceeds its current COM buffer",
    )
}

/// Make: the plan's numbers and its possible records.
///
/// # Safety
/// The lent strings and bytes hold their lengths.
pub unsafe fn make(identity: &Identity, view: &View) -> Result<(PlanNumbers, Vec<u32>), Fail> {
    unsafe {
        let exact_family = identity.family_root.bytes() == ROOT_SHA256.as_bytes()
            && identity.family_vertex.bytes() == VERTEX_SHA256.as_bytes()
            && identity.family_pixel.bytes() == PIXEL_SHA256.as_bytes();
        let exact_b4 = identity.b4_root.bytes() == ROOT_SHA256.as_bytes()
            && identity.b4_visibility == 5
            && identity.b4_register == 4
            && identity.b4_space == 0
            && identity.b4_parameter == 19
            && identity.b4_table_offset == 0;
        require(
            exact_family && exact_b4,
            "Multilayer original root/VS/PS/b4 identity differs",
        )?;
        require(
            identity.b4_len == B4_BYTES && !identity.b4.is_null(),
            "Multilayer b4 is not the exact original228 logical bytes",
        )?;
        check_view(view)?;
        let b4 = core::slice::from_raw_parts(identity.b4, B4_BYTES);
        let raw = f32::from_bits(dword(b4, 128)?);
        require(
            raw.is_finite() && raw >= 0.0 && (raw as f64) < 4294967296.0,
            "Multilayer base has unsupported FPToui domain",
        )?;
        let mut n = PlanNumbers {
            base_record: (raw as f64).trunc() as u32,
            mask: dword(b4, 224)?,
            ..PlanNumbers::default()
        };
        // A wave OR of (t4 masks AND this uniform b4 mask) cannot introduce new bits; both original load paths guard
        // the nonzero first-bit selection.
        let mut records = Vec::new();
        for bit in 0..MAX_RECORDS {
            if n.mask & (1u32 << bit) != 0 {
                let record = n.base_record as u64 + bit as u64;
                require(
                    record <= u32::MAX as u64 && record < view.num_elements as u64,
                    "Possible multilayer record exceeds original structured view",
                )?;
                records.push(record as u32);
            }
        }
        if let (Some(&first), Some(&last)) = (records.first(), records.last()) {
            n.first_record = first;
            n.record_span = last - first + 1;
            require(
                n.record_span <= MAX_RECORDS
                    && view.first_element <= u64::MAX - n.first_record as u64,
                "Multilayer record/FirstElement span overflows",
            )?;
            let start = view.first_element + n.first_record as u64;
            require(
                start <= u64::MAX / STRIDE,
                "Multilayer COM byte start overflows",
            )?;
            n.resource_offset = start * STRIDE;
            n.byte_extent = n.record_span * STRIDE as u32;
            require(
                n.byte_extent <= MAX_READ_BYTES
                    && n.resource_offset <= view.resource_width
                    && n.byte_extent as u64 <= view.resource_width - n.resource_offset,
                "Multilayer compact read span exceeds COM backing",
            )?;
        }
        Ok((n, records))
    }
}

/// One evaluated selector site.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Selection {
    pub site: u32,
    pub value: u32,
    pub selected: u32,
}
/// A contiguous run of selected table offsets.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Use {
    pub offset: u32,
    pub count: u32,
}
#[derive(Default)]
pub struct Enumeration {
    pub selections: Vec<Selection>,
    pub offsets: Vec<u32>,
    pub uses: Vec<Use>,
}

/// Enumerate: the plan rechecked against its own metadata and the snapshot, then every site of every record.
///
/// # Safety
/// The lent strings and bytes hold their lengths.
pub unsafe fn enumerate(
    identity: &Identity,
    plan_view: &View,
    numbers: &PlanNumbers,
    records: &[u32],
    snapshot_view: &View,
    snapshot_offset: u64,
    bytes: &[u8],
    bytes_present: bool,
) -> Result<Enumeration, Fail> {
    unsafe {
        let (checked, checked_records) = make(identity, plan_view)?;
        require(
            checked == *numbers && checked_records == records,
            "Multilayer read-plan metadata is torn",
        )?;
        require(
            same_view(snapshot_view, plan_view)
                && snapshot_offset == checked.resource_offset
                && bytes.len() as u64 == checked.byte_extent as u64
                && (checked.byte_extent == 0 || bytes_present),
            "Multilayer record snapshot is not its exact planned COM subview",
        )?;
        let mut out = Enumeration::default();
        let mut offsets = BTreeSet::new();
        for &record in &checked_records {
            let at = (record - checked.first_record) as usize * STRIDE as usize;
            for (index, &(field, transform)) in SITES.iter().enumerate() {
                let value = dword(bytes, at + field)?;
                let selected = match transform {
                    Transform::Raw => value,
                    Transform::Low16 => value & 65535,
                    Transform::High16 => value >> 16,
                };
                require(
                    selected < DECLARED_EXTENT,
                    "Conservative multilayer selector candidate exceeds declared32768 SRVs",
                )?;
                out.selections.push(Selection {
                    site: index as u32,
                    value,
                    selected,
                });
                offsets.insert(selected);
            }
        }
        out.offsets = offsets.into_iter().collect();
        for &offset in &out.offsets {
            match out.uses.last_mut() {
                Some(last) if last.offset + last.count == offset => last.count += 1,
                _ => out.uses.push(Use { offset, count: 1 }),
            }
        }
        Ok(out)
    }
}

// ---- the C ABI ------------------------------------------------------------------------------------------------------

/// A refusal's message (static).
#[repr(C)]
pub struct Error {
    pub message: *const u8,
    pub length: usize,
}
fn error(out: *mut Error, message: Fail) {
    unsafe {
        *out = Error {
            message: message.as_ptr(),
            length: message.len(),
        }
    };
}

/// PlanRead: the records (read them with cvr_ml_records, free with cvr_ml_free) and the numbers, or null with the
/// error.
///
/// # Safety
/// The identity and view are valid with their lent strings and bytes; `numbers` and `failure` are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ml_plan(
    identity: *const Identity,
    view: *const View,
    numbers: *mut PlanNumbers,
    failure: *mut Error,
) -> *mut Vec<u32> {
    unsafe {
        match make(&*identity, &*view) {
            Ok((n, records)) => {
                *numbers = n;
                Box::into_raw(Box::new(records))
            }
            Err(message) => {
                error(failure, message);
                core::ptr::null_mut()
            }
        }
    }
}

/// A plan's records.
///
/// # Safety
/// `records` comes from cvr_ml_plan; `count` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ml_records(
    records: *const Vec<u32>,
    count: *mut usize,
) -> *const u32 {
    unsafe {
        *count = (*records).len();
        (*records).as_ptr()
    }
}

/// # Safety
/// `records` comes from cvr_ml_plan (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ml_free(records: *mut Vec<u32>) {
    if !records.is_null() {
        drop(unsafe { Box::from_raw(records) });
    }
}

/// The enumeration's arrays.
#[repr(C)]
pub struct EnumerationView {
    pub selections: *const Selection,
    pub selection_count: usize,
    pub offsets: *const u32,
    pub offset_count: usize,
    pub uses: *const Use,
    pub use_count: usize,
}

/// Enumerate: the enumeration (its arrays through `view`, free it with cvr_ml_enumeration_free) or null with the
/// error.
///
/// # Safety
/// Every input is valid with its lent strings and arrays; `view` and `failure` are writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_ml_enumerate(
    identity: *const Identity,
    plan_view: *const View,
    numbers: *const PlanNumbers,
    records: *const u32,
    record_count: usize,
    snapshot_view: *const View,
    snapshot_offset: u64,
    bytes: *const u8,
    byte_count: usize,
    out: *mut EnumerationView,
    failure: *mut Error,
) -> *mut Enumeration {
    unsafe {
        let records = if record_count == 0 || records.is_null() {
            &[][..]
        } else {
            core::slice::from_raw_parts(records, record_count)
        };
        let data = if byte_count == 0 || bytes.is_null() {
            &[][..]
        } else {
            core::slice::from_raw_parts(bytes, byte_count)
        };
        match enumerate(
            &*identity,
            &*plan_view,
            &*numbers,
            records,
            &*snapshot_view,
            snapshot_offset,
            data,
            !bytes.is_null(),
        ) {
            Ok(e) => {
                let e = Box::into_raw(Box::new(e));
                *out = EnumerationView {
                    selections: (*e).selections.as_ptr(),
                    selection_count: (*e).selections.len(),
                    offsets: (*e).offsets.as_ptr(),
                    offset_count: (*e).offsets.len(),
                    uses: (*e).uses.as_ptr(),
                    use_count: (*e).uses.len(),
                };
                e
            }
            Err(message) => {
                error(failure, message);
                core::ptr::null_mut()
            }
        }
    }
}

/// # Safety
/// `enumeration` comes from cvr_ml_enumerate (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ml_enumeration_free(enumeration: *mut Enumeration) {
    if !enumeration.is_null() {
        drop(unsafe { Box::from_raw(enumeration) });
    }
}

// ---- the prepared membership bank's bitmap (include/Render/PreparedMembershipBank.hpp, detail::Mark) ---------------

/// detail::Mark: one SourcePass offset set in the 32768-bit membership bitmap -- inside the copied storage extent and
/// the declared SRVs -- counted once, with the maximum set bit (u32::MAX while none).
pub fn mark(
    bits: &mut [u32; 1024],
    offset: u32,
    copied_extent: u32,
    count: &mut u32,
    maximum: &mut u32,
) -> bool {
    if copied_extent == 0 || offset >= copied_extent || offset >= DECLARED_EXTENT {
        return false;
    }
    let word = &mut bits[(offset / 32) as usize];
    let mask = 1u32 << (offset % 32);
    if *word & mask == 0 {
        *word |= mask;
        *count += 1;
        *maximum = if *maximum == u32::MAX {
            offset
        } else {
            (*maximum).max(offset)
        };
    }
    true
}

/// detail::Mark.
///
/// # Safety
/// `bits` holds 1024 words; `count` and `maximum` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ml_mark(
    bits: *mut [u32; 1024],
    offset: u32,
    copied_extent: u32,
    count: *mut u32,
    maximum: *mut u32,
) -> bool {
    unsafe {
        mark(
            &mut *bits,
            offset,
            copied_extent,
            &mut *count,
            &mut *maximum,
        )
    }
}
