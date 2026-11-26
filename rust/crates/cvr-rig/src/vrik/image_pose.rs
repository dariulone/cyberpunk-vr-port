//! The images' pose labels (formerly src/Camera/ImagePoseIdentity.cpp): the plugin's one image pose ledger
//! (cvr_camera::image_ledger) over PoseIdentity, and the objects' lifetimes behind its addresses.
//!
//! A list or resource the ledger knows carries a small COM object in its private data; the object is released with
//! the D3D12 object itself and then forgets the address, so recycling a resource or command-list address cannot
//! inherit another object's image label. src/Camera/ImagePoseIdentity.cpp keeps the exports, the submission mutex its
//! C++ callers lock and the C++ names as forwarders.
use super::pose_identity::PoseIdentity;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering::*};
use cvr_camera::image_ledger::{Image, ImagePoseLedger};
use std::sync::{LazyLock, Mutex};
use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Graphics::Direct3D12::ID3D12Object;

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_PoseIdImageWrites: [AtomicU64; 2];
}

/// cvr::camera::ImagePoseIdentity.
pub type ImagePose = Image<PoseIdentity>;
const _: () = assert!(core::mem::size_of::<ImagePose>() == 168);

static IMAGES: LazyLock<ImagePoseLedger<PoseIdentity>> = LazyLock::new(ImagePoseLedger::new);
static LIFETIME_MUTEX: Mutex<()> = Mutex::new(());
const POSE_LIFETIME: GUID = GUID::from_values(
    0x8d1748d9,
    0x3642,
    0x43e5,
    [0xa2, 0x41, 0x9b, 0xb6, 0x90, 0x51, 0x47, 0xe3],
);

/// The lifetime record on an object: forgets its address in the ledger when the object releases it.
#[repr(C)]
struct Lifetime {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    address: usize,
    list: bool,
}
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
unsafe extern "system" fn lifetime_query(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return windows::Win32::Foundation::E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live Lifetime.
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || *iid != IID_IUNKNOWN {
            return windows::Win32::Foundation::E_NOINTERFACE;
        }
        *out = this;
        lifetime_add_ref(this);
    }
    HRESULT(0)
}
unsafe extern "system" fn lifetime_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: `this` is a live Lifetime.
    unsafe { (*(this as *const Lifetime)).refs.fetch_add(1, Relaxed) + 1 }
}
unsafe extern "system" fn lifetime_release(this: *mut c_void) -> u32 {
    // SAFETY: `this` is a live Lifetime; the last release frees it (it came from Box::into_raw).
    unsafe {
        let left = (*(this as *const Lifetime)).refs.fetch_sub(1, AcqRel) - 1;
        if left == 0 {
            let record = Box::from_raw(this as *mut Lifetime);
            if record.list {
                IMAGES.forget_list(record.address);
            } else {
                IMAGES.forget_resource(record.address);
            }
        }
        left
    }
}
static LIFETIME_VTBL: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: lifetime_query,
    AddRef: lifetime_add_ref,
    Release: lifetime_release,
};
// SAFETY: the vtable holds plain function pointers.
unsafe impl Sync for Lifetime {}

/// The object carries a lifetime record (one is attached at its first sight).
unsafe fn track_lifetime(object: *mut c_void, list: bool) -> bool {
    let Some(object) = ID3D12Object::from_raw_borrowed(&object) else {
        return false;
    };
    let created;
    let result;
    {
        let _guard = LIFETIME_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let mut existing: *mut c_void = core::ptr::null_mut();
        let mut size = core::mem::size_of::<*mut c_void>() as u32;
        let found = object.GetPrivateData(
            &POSE_LIFETIME,
            &mut size,
            Some(&mut existing as *mut _ as *mut c_void),
        );
        if found.is_ok() && !existing.is_null() {
            // GetPrivateData added a reference.
            drop(IUnknown::from_raw(existing));
            return true;
        }
        let raw = Box::into_raw(Box::new(Lifetime {
            vtbl: &LIFETIME_VTBL,
            refs: AtomicU32::new(1),
            address: object.as_raw() as usize,
            list,
        }));
        created = IUnknown::from_raw(raw as *mut c_void);
        result = object.SetPrivateDataInterface(&POSE_LIFETIME, &created);
    }
    // Our reference goes; the object keeps its own.
    drop(created);
    result.is_ok()
}

#[no_mangle]
pub unsafe extern "C" fn cvr_image_pose_reset_list(list: *mut c_void) {
    if track_lifetime(list, true) {
        IMAGES.reset(list as usize);
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_image_pose_record(
    list: *mut c_void,
    resource: *mut c_void,
    pose: *const PoseIdentity,
    stable: bool,
) {
    let Some(pose) = pose.as_ref() else { return };
    if !track_lifetime(list, true) || !track_lifetime(resource, false) {
        return;
    }
    IMAGES.record(list as usize, resource as usize, *pose, stable);
    if (1..=2).contains(&pose.view) && CyberpunkVR_RuntimeDiagnostics != 0 {
        CyberpunkVR_PoseIdImageWrites[pose.view as usize - 1].fetch_add(1, Relaxed);
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_image_pose_commit(
    queue: *mut c_void,
    count: u32,
    lists: *const *mut c_void,
) {
    if queue.is_null() || lists.is_null() {
        return;
    }
    let now = windows_sys::Win32::System::SystemInformation::GetTickCount64();
    for i in 0..count as usize {
        IMAGES.submit(queue as usize, *lists.add(i) as usize, now);
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_image_pose_read(resource: *mut c_void, out: *mut ImagePose) {
    if let Some(out) = out.as_mut() {
        *out = IMAGES.read(resource as usize);
    }
}
/// The latest stable (vrcam) image submitted: borrowed; stable images have the capture owner's lifetime.
#[no_mangle]
pub extern "C" fn cvr_image_pose_latest_stable() -> *mut c_void {
    IMAGES.latest_stable().resource as *mut c_void
}
