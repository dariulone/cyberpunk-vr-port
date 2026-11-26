//! Pose identity (formerly src/Camera/PoseIdentity.cpp): which tracked head sample a camera's pose bytes were composed
//! from, carried from the component the locate site wrote through every engine copy and blend down to the render
//! node, so a rendered eye can name the sample it shows.
//!
//! The ledger itself is cvr_camera::pose_ledger; this module owns the one ledger of the plugin, reads the pose bytes
//! (position 12 bytes at the address, rotation 16 bytes at +16) under SEH, and asks OpenXR for the rendered pose of a
//! sample and for the current origin. src/Camera/PoseIdentityRust.cpp forwards cvr::camera::* here.
use super::camera_link::HeadPose;
use super::fingergrip::Posef;
use core::cell::Cell;
use cvr_camera::pose_ledger::{BlendInput, Fingerprint, PoseAddressLedger, Receipt};
use cvr_engine::code::read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;

/// cvr::camera::PoseIdentity.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PoseIdentity {
    pub pose_id: u64,
    pub write_id: u64,
    pub component: usize,
    pub view: u32,
    pub head: HeadPose,
    pub local_head: Posef,
    /// Rendering stamp, populated only by the native node dispatcher. It is not a pose-sample id: several rendered
    /// frames can use the same tracked pose.
    pub render_frame_id: u32,
    pub has_render_frame_id: bool,
    /// The selected MAIN is a detached camera, not the player's placed FPP.
    pub external_view: bool,
}
const _: () = assert!(core::mem::size_of::<PoseIdentity>() == 136);
pub type PoseReceipt = Receipt<PoseIdentity>;
const _: () = assert!(core::mem::size_of::<PoseReceipt>() == 184);
/// cvr::camera::WeightedPoseReceipt.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct WeightedPoseReceipt {
    pub source: PoseReceipt,
    pub weight: f32,
}
const _: () = assert!(core::mem::size_of::<WeightedPoseReceipt>() == 192);

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_PoseIdComponent: [AtomicU64; 2];
    static CyberpunkVR_PoseIdCopied: AtomicU64;
    static CyberpunkVR_PoseIdCopyChanged: AtomicU64;
    fn cvr_cpp_xr_tracking_origin_serial() -> u64;
    fn cvr_cpp_xr_resolve_rendered_pose(head: *const HeadPose, out: *mut Posef) -> bool;
    fn cvr_cpp_xr_external_pose_reset_pending() -> bool;
}

static ADDRESSES: LazyLock<PoseAddressLedger<PoseIdentity>> = LazyLock::new(|| PoseAddressLedger::new(8192));
static POSE_ID: AtomicU64 = AtomicU64::new(0);
static WRITE_ID: AtomicU64 = AtomicU64::new(0);
thread_local! {
    static NODE_POSE: Cell<PoseIdentity> = Cell::new(PoseIdentity::default());
}

fn diagnostics() -> bool {
    // SAFETY: a plain exported int.
    unsafe { CyberpunkVR_RuntimeDiagnostics != 0 }
}
fn fingerprint(address: usize) -> Option<Fingerprint> {
    if address < 0x10000 {
        return None;
    }
    let position = read::<[i32; 3]>(address)?;
    let rotation = read::<[u32; 4]>(address + 16)?;
    Some(Fingerprint { position, rotation })
}
fn same_local_head(a: &Posef, b: &Posef) -> bool {
    // memcmp: the bits, so -0 and NaN payloads differ as they do there
    a.orientation.iter().zip(b.orientation.iter()).all(|(x, y)| x.to_bits() == y.to_bits())
        && a.position.x.to_bits() == b.position.x.to_bits()
        && a.position.y.to_bits() == b.position.y.to_bits()
        && a.position.z.to_bits() == b.position.z.to_bits()
}

/// Native packets already use bit 63; XR-latched samples use neither bit. Legacy, separately located samples keep
/// their own identity domain.
#[no_mangle]
pub extern "C" fn cvr_pose_next_identity() -> u64 {
    (POSE_ID.fetch_add(1, Ordering::Relaxed) + 1) | (1u64 << 62)
}
#[no_mangle]
pub extern "C" fn cvr_pose_invalidate(address: usize) {
    ADDRESSES.invalidate(address);
}
fn capture(address: usize) -> PoseReceipt {
    fingerprint(address).and_then(|f| ADDRESSES.capture(address, f)).unwrap_or_default()
}
#[no_mangle]
pub unsafe extern "C" fn cvr_pose_capture(address: usize, out: *mut PoseReceipt) {
    if !out.is_null() {
        // SAFETY: the caller's receipt.
        unsafe { *out = capture(address) };
    }
}
fn transfer(source: &PoseReceipt, destination: usize) -> bool {
    let (Some(source_now), Some(destination_now)) = (if source.valid() { fingerprint(source.source) } else { None }, fingerprint(destination))
    else {
        ADDRESSES.invalidate(destination);
        return false;
    };
    let copied = ADDRESSES.transfer(source, source_now, destination, destination_now);
    if diagnostics() {
        // SAFETY: exported counters.
        unsafe { if copied { &CyberpunkVR_PoseIdCopied } else { &CyberpunkVR_PoseIdCopyChanged }.fetch_add(1, Ordering::Relaxed) };
    }
    copied
}
#[no_mangle]
pub unsafe extern "C" fn cvr_pose_transfer(source: *const PoseReceipt, destination: usize) -> bool {
    // SAFETY: the caller's receipt.
    let Some(source) = (unsafe { source.as_ref() }) else { return false };
    transfer(source, destination)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_pose_publish_setup(address: usize, component: usize, view: u32, pose_id: u64, head: *const HeadPose, external_view: bool) {
    // SAFETY: the caller's sample.
    let Some(head) = (unsafe { head.as_ref() }) else {
        ADDRESSES.invalidate(address);
        return;
    };
    let mut value = PoseIdentity::default();
    let fp = if pose_id != 0 && (1..=2).contains(&view) { fingerprint(address) } else { None };
    // SAFETY: C++ service writing one pose.
    let Some(fp) = fp.filter(|_| unsafe { cvr_cpp_xr_resolve_rendered_pose(head, &mut value.local_head) }) else {
        ADDRESSES.invalidate(address);
        return;
    };
    value.pose_id = pose_id;
    value.write_id = WRITE_ID.fetch_add(1, Ordering::Relaxed) + 1;
    value.component = component;
    value.view = view;
    value.head = *head;
    value.external_view = external_view;
    ADDRESSES.publish(address, fp, value);
    if diagnostics() {
        // SAFETY: exported counters.
        unsafe { CyberpunkVR_PoseIdComponent[view as usize - 1].fetch_add(1, Ordering::Relaxed) };
    }
}
/// Serialisation on the component path preserves these fields. A camera override changing them must supply its own
/// label, not inherit this one.
#[no_mangle]
pub unsafe extern "C" fn cvr_pose_publish_serialized(destination: usize, source: *const PoseReceipt) {
    // SAFETY: the caller's receipt.
    let source = unsafe { source.as_ref() };
    match (source, fingerprint(destination)) {
        (Some(s), Some(output)) if s.valid() && output == s.fingerprint => {
            transfer(s, destination);
        }
        _ => ADDRESSES.invalidate(destination),
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_pose_read_camera(camera: usize, view: u32, out: *mut PoseIdentity) -> bool {
    // SAFETY: C++ service.
    if out.is_null() || !(1..=2).contains(&view) || unsafe { cvr_cpp_xr_external_pose_reset_pending() } {
        return false;
    }
    let record = capture(camera);
    // SAFETY: C++ service.
    if !record.valid() || record.payload.view != view || record.payload.head.origin_serial != unsafe { cvr_cpp_xr_tracking_origin_serial() } {
        return false;
    }
    // SAFETY: the caller's identity.
    unsafe { *out = record.payload };
    true
}
#[no_mangle]
pub unsafe extern "C" fn cvr_pose_publish_blended(destination: usize, inputs: *const WeightedPoseReceipt, count: usize) -> bool {
    let Some(output) = fingerprint(destination) else {
        ADDRESSES.invalidate(destination);
        return false;
    };
    let inputs: &[WeightedPoseReceipt] = if inputs.is_null() || count == 0 {
        &[]
    } else {
        // SAFETY: the caller's span.
        unsafe { core::slice::from_raw_parts(inputs, count) }
    };
    let mut verified = Vec::with_capacity(inputs.len());
    for input in inputs {
        let mut current = Fingerprint::default();
        if input.weight != 0.0 {
            let read = if input.source.valid() && input.source.payload.view == 1 { fingerprint(input.source.source) } else { None };
            let Some(f) = read else {
                ADDRESSES.invalidate(destination);
                return false;
            };
            current = f;
        }
        verified.push(BlendInput { source: input.source, current, weight: input.weight });
    }
    let copied = ADDRESSES.transfer_blend(&verified, destination, output, |a, b| {
        a.view == 1
            && b.view == 1
            && a.pose_id == b.pose_id
            && a.head.origin_serial == b.head.origin_serial
            && same_local_head(&a.local_head, &b.local_head)
    });
    if copied {
        let external = inputs.iter().any(|i| i.weight > 0.0 && i.source.payload.external_view);
        if let Some(receipt) = ADDRESSES.capture(destination, output) {
            if receipt.payload.external_view != external {
                let mut value = receipt.payload;
                value.external_view = external;
                ADDRESSES.publish(destination, output, value);
            }
        }
    }
    copied
}
/// Populated at node entry, restored at node exit. Copies after node exit pass their saved value explicitly instead of
/// reading an unrelated parent scope.
#[no_mangle]
pub unsafe extern "C" fn cvr_pose_node_current(out: *mut PoseIdentity) {
    if !out.is_null() {
        // SAFETY: the caller's identity.
        unsafe { *out = NODE_POSE.with(|p| p.get()) };
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_pose_node_set(pose: *const PoseIdentity, previous: *mut PoseIdentity) {
    // SAFETY: the caller's identities.
    let next = unsafe { pose.as_ref() }.copied().unwrap_or_default();
    let before = NODE_POSE.with(|p| p.replace(next));
    if !previous.is_null() {
        // SAFETY: as above.
        unsafe { *previous = before };
    }
}
