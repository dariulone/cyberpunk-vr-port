//! The native prepared pair's source admission (src/Render/NativePreparedPair.cpp, `StartInternal`): a source joins the
//! work when it has its recorded root (the original draw's), three render targets, a pipeline whose native geometry
//! status is 1, a shader with a root, a captured baseline, a side of 0 or 1 and the first source's non-zero frame and
//! viewport size at the origin, a known shading rate with its rate image held exactly when recorded, and its family's
//! source root (when a family is known); for current work it also has the current frame on its owned and original draws,
//! input checks unless the actual membership is asked, the current queue and a whole recording lease (token,
//! generation, context and registry) on both draws' list; and in its eye it follows the eye's last ordinal and shares
//! the eye's first pass baseline.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_npp_source_shape`, `cvr_npp_source_current` and
//! `cvr_npp_eye_order` (the draw record and the recording lease mirrored by geometry_commands.rs and
//! native_recording.rs; the shader, baseline and pipeline status are the .cpp's inputs). The unit tests below cover
//! each condition.

use crate::geometry_commands::DrawRecord;
use crate::native_recording::Lease;

/// The source's facts besides its draws.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SourceFacts {
    pub root: u64,
    pub family_root: u64,
    pub native_status: u32,
    pub has_family: bool,
    pub shader: bool,
    pub baseline: bool,
    pub rate_known: bool,
    pub rate_image: bool,
    pub source_rate_image: bool,
}
const _: () = assert!(core::mem::size_of::<SourceFacts>() == 32);
/// The current work's facts for one source.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CurrentFacts {
    pub queue: u64,
    pub source_queue: u64,
    pub frame: u32,
    pub membership: bool,
    pub input_checks: bool,
}
const _: () = assert!(core::mem::size_of::<CurrentFacts>() == 24);

/// The source's shape against the work's first owned draw `reference`.
pub fn source_shape(f: &SourceFacts, actual: &DrawRecord, original_root: u64, reference: &DrawRecord) -> bool {
    let view = &actual.viewport;
    f.root != 0
        && f.root == original_root
        && actual.rt_count == 3
        && f.native_status == 1
        && f.shader
        && f.baseline
        && actual.side <= 1
        && actual.frame == reference.frame
        && actual.frame != 0
        && view.Width == reference.viewport.Width
        && view.Height == reference.viewport.Height
        && view.TopLeftX == 0.0
        && view.TopLeftY == 0.0
        && f.rate_known
        && f.rate_image == f.source_rate_image
        && !(f.has_family && f.family_root != f.root)
}
/// The current work's checks of one source.
pub fn source_current(c: &CurrentFacts, recording: &Lease, actual: &DrawRecord, original: &DrawRecord) -> bool {
    actual.frame == c.frame
        && original.frame == c.frame
        && (c.membership || c.input_checks)
        && c.source_queue == c.queue
        && !recording.token.is_null()
        && recording.generation != 0
        && recording.context != 0
        && recording.registry != 0
        && recording.list == original.list
        && recording.list == actual.list
}
/// The eye's order: a source after the eye's first follows its last ordinal and shares its baseline.
pub fn eye_order(has_prior: bool, ordinal: u64, last: u64, baseline: u64, first_baseline: u64) -> bool {
    !has_prior || (ordinal > last && baseline == first_baseline)
}

// ---- the C ABI

/// # Safety
/// The records are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_source_shape(facts: *const SourceFacts, actual: *const DrawRecord, original_root: u64, reference: *const DrawRecord) -> bool {
    source_shape(unsafe { &*facts }, unsafe { &*actual }, original_root, unsafe { &*reference })
}
/// # Safety
/// The records are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_source_current(facts: *const CurrentFacts, recording: *const Lease, actual: *const DrawRecord, original: *const DrawRecord) -> bool {
    source_current(unsafe { &*facts }, unsafe { &*recording }, unsafe { &*actual }, unsafe { &*original })
}
#[no_mangle]
pub extern "C-unwind" fn cvr_npp_eye_order(has_prior: bool, ordinal: u64, last: u64, baseline: u64, first_baseline: u64) -> bool {
    eye_order(has_prior, ordinal, last, baseline, first_baseline)
}

#[cfg(test)]
mod tests {
    //! Each shape, current and order condition.
    use super::*;

    fn draw() -> DrawRecord {
        let mut d: DrawRecord = unsafe { core::mem::zeroed() };
        d.rt_count = 3;
        d.side = 1;
        d.frame = 7;
        d.list = 40;
        d.viewport.Width = 100.0;
        d.viewport.Height = 50.0;
        d
    }
    fn facts() -> SourceFacts {
        SourceFacts { root: 9, family_root: 9, native_status: 1, has_family: true, shader: true, baseline: true, rate_known: true, rate_image: true, source_rate_image: true }
    }
    #[test]
    fn shapes() {
        let r = draw();
        assert!(source_shape(&facts(), &r, 9, &r));
        assert!(source_shape(&SourceFacts { has_family: false, family_root: 3, ..facts() }, &r, 9, &r));
        assert!(source_shape(&SourceFacts { rate_image: false, source_rate_image: false, ..facts() }, &r, 9, &r));
        for f in [
            SourceFacts { root: 0, family_root: 0, ..facts() },
            SourceFacts { family_root: 3, ..facts() },
            SourceFacts { native_status: 2, ..facts() },
            SourceFacts { shader: false, ..facts() },
            SourceFacts { baseline: false, ..facts() },
            SourceFacts { rate_known: false, ..facts() },
            SourceFacts { source_rate_image: false, ..facts() },
        ] {
            assert!(!source_shape(&f, &r, 9, &r), "{f:?}");
        }
        assert!(!source_shape(&SourceFacts { root: 0, family_root: 0, ..facts() }, &r, 0, &r));
        assert!(!source_shape(&facts(), &r, 8, &r));
        let mut a = draw();
        a.rt_count = 2;
        assert!(!source_shape(&facts(), &a, 9, &r));
        let mut a = draw();
        a.side = 0;
        assert!(source_shape(&facts(), &a, 9, &r));
        a.side = 2;
        assert!(!source_shape(&facts(), &a, 9, &r));
        let mut a = draw();
        a.frame = 8;
        assert!(!source_shape(&facts(), &a, 9, &r));
        let (mut a, mut z) = (draw(), draw());
        a.frame = 0;
        z.frame = 0;
        assert!(!source_shape(&facts(), &a, 9, &z));
        let changes: [fn(&mut DrawRecord); 4] =
            [|d| d.viewport.Width = 99.0, |d| d.viewport.Height = 49.0, |d| d.viewport.TopLeftX = 1.0, |d| d.viewport.TopLeftY = f32::NAN];
        for change in changes {
            let mut a = draw();
            change(&mut a);
            assert!(!source_shape(&facts(), &a, 9, &r));
        }
        let mut a = draw();
        a.viewport.TopLeftX = -0.0;
        assert!(source_shape(&facts(), &a, 9, &r));
    }
    #[test]
    fn currents() {
        let d = draw();
        let token = 1usize as *mut core::ffi::c_void;
        let lease = || Lease { token, generation: 2, context: 3, list: 40, registry: 4 };
        let now = CurrentFacts { queue: 5, source_queue: 5, frame: 7, membership: false, input_checks: true };
        assert!(source_current(&now, &lease(), &d, &d));
        assert!(source_current(&CurrentFacts { membership: true, input_checks: false, ..now }, &lease(), &d, &d));
        assert!(!source_current(&CurrentFacts { input_checks: false, ..now }, &lease(), &d, &d));
        assert!(!source_current(&CurrentFacts { source_queue: 6, ..now }, &lease(), &d, &d));
        assert!(!source_current(&CurrentFacts { frame: 8, ..now }, &lease(), &d, &d));
        let mut o = draw();
        o.frame = 8;
        assert!(!source_current(&now, &lease(), &d, &o));
        let mut o = draw();
        o.list = 41;
        assert!(!source_current(&now, &lease(), &d, &o));
        assert!(!source_current(&now, &lease(), &o, &d));
        assert!(!source_current(&now, &Lease { token: core::ptr::null_mut(), ..lease() }, &d, &d));
        assert!(!source_current(&now, &Lease { generation: 0, ..lease() }, &d, &d));
        assert!(!source_current(&now, &Lease { context: 0, ..lease() }, &d, &d));
        assert!(!source_current(&now, &Lease { registry: 0, ..lease() }, &d, &d));
    }
    #[test]
    fn orders() {
        assert!(eye_order(false, 0, 9, 1, 2));
        assert!(eye_order(true, 10, 9, 1, 1));
        assert!(!eye_order(true, 9, 9, 1, 1));
        assert!(!eye_order(true, 10, 9, 1, 2));
    }
}
