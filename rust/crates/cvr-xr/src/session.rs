//! The session (formerly OpenXRManager::InitGraphics' OpenXR calls): the runtime's D3D12 requirements, the session on
//! the game's device and queue, then the tracking and view spaces, the action set attached and the hands' grip and aim
//! spaces. What OpenXRManager does between the two (the device's and queue's references, its events and the submit
//! thread, the finger bones' session) stays in C++, in the same order.
#![cfg_attr(test, allow(dead_code, unused_imports))]
use core::ffi::{c_char, c_void};
use cvr_host::log;
use openxr_sys as xr;
use xr::Handle as _;

/// XrGraphicsRequirementsD3D12KHR (LUID: LowPart, HighPart).
#[repr(C)]
struct GraphicsRequirementsD3D12 {
    ty: xr::StructureType,
    next: *mut c_void,
    adapter_luid_low: u32,
    adapter_luid_high: i32,
    min_feature_level: i32,
}
/// XrGraphicsBindingD3D12KHR.
#[repr(C)]
struct GraphicsBindingD3D12 {
    ty: xr::StructureType,
    next: *const c_void,
    device: *mut c_void,
    queue: *mut c_void,
}
type GetD3D12Requirements = unsafe extern "system" fn(xr::Instance, xr::SystemId, *mut GraphicsRequirementsD3D12) -> xr::Result;

#[cfg(not(test))]
extern "system" {
    fn xrGetInstanceProcAddr(instance: xr::Instance, name: *const c_char, function: *mut Option<xr::pfn::VoidFunction>) -> xr::Result;
    fn xrCreateSession(instance: xr::Instance, info: *const xr::SessionCreateInfo, session: *mut xr::Session) -> xr::Result;
    fn xrCreateReferenceSpace(session: xr::Session, info: *const xr::ReferenceSpaceCreateInfo, space: *mut xr::Space) -> xr::Result;
    fn xrAttachSessionActionSets(session: xr::Session, info: *const xr::SessionActionSetsAttachInfo) -> xr::Result;
    fn xrCreateActionSpace(session: xr::Session, info: *const xr::ActionSpaceCreateInfo, space: *mut xr::Space) -> xr::Result;
}

const IDENTITY: xr::Posef = xr::Posef { orientation: xr::Quaternionf { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }, position: xr::Vector3f { x: 0.0, y: 0.0, z: 0.0 } };

/// The runtime's D3D12 requirements logged, then the session on the game's device and queue; 0 when it cannot be had.
///
/// # Safety
/// `device` and `queue` are the game's live ID3D12Device and ID3D12CommandQueue.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_create_session(instance: u64, system: u64, device: *mut c_void, queue: *mut c_void) -> u64 {
    // SAFETY (whole body): the loader's calls with structures of the types they take.
    unsafe {
        let instance = xr::Instance::from_raw(instance);
        let system = xr::SystemId::from_raw(system);
        let mut function: Option<xr::pfn::VoidFunction> = None;
        xrGetInstanceProcAddr(instance, c"xrGetD3D12GraphicsRequirementsKHR".as_ptr(), &mut function);
        let Some(function) = function else {
            log!("OpenXRManager: xrGetD3D12GraphicsRequirementsKHR not found!\n");
            return 0;
        };
        let requirements_of: GetD3D12Requirements = core::mem::transmute(function);
        let mut reqs = GraphicsRequirementsD3D12 {
            ty: xr::StructureType::GRAPHICS_REQUIREMENTS_D3D12_KHR,
            next: core::ptr::null_mut(),
            adapter_luid_low: 0,
            adapter_luid_high: 0,
            min_feature_level: 0,
        };
        let requirements = requirements_of(instance, system, &mut reqs);
        // The session binds the game's device and queue: neither missing, and the headset's adapter the game's (main's
        // 1cba63ec0).
        if requirements.into_raw() < 0 || device.is_null() || queue.is_null() {
            log!(
                "OpenXRManager: cannot bind D3D12 graphics (requirements={} device={:016X} queue={:016X})\n",
                requirements.into_raw(),
                device as usize,
                queue as usize
            );
            return 0;
        }
        {
            use windows::core::Interface;
            use windows::Win32::Graphics::Direct3D12::ID3D12Device;
            if let Some(d) = ID3D12Device::from_raw_borrowed(&device) {
                let game = d.GetAdapterLuid();
                // A zero LUID names no adapter (no D3D12 adapter has it): SteamVR's null driver -- a headset that is only
                // a desktop window -- answers so, and refusing it left that runtime without a session.
                let any = reqs.adapter_luid_high == 0 && reqs.adapter_luid_low == 0;
                if any {
                    log!("OpenXRManager: the runtime names no adapter (LUID 0); the session binds the game's\n");
                } else if game.HighPart != reqs.adapter_luid_high || game.LowPart != reqs.adapter_luid_low {
                    log!(
                        "OpenXRManager: headset and game require different adapters (XR={:08X}:{:08X} game={:08X}:{:08X})\n",
                        reqs.adapter_luid_high as u32,
                        reqs.adapter_luid_low,
                        game.HighPart as u32,
                        game.LowPart
                    );
                    return 0;
                }
            }
        }
        log!(
            "OpenXRManager: D3D12 graphics requirements minFeatureLevel={:#X} luid=({:#010X},{:#010X})\n",
            reqs.min_feature_level,
            reqs.adapter_luid_high as u32,
            reqs.adapter_luid_low
        );
        let vendor = log_adapter(device);
        // The runtime's own queue (runtime_queue.rs, main's 0.2.4): high priority, so its copies cut in front of the
        // game's long work instead of waiting behind it. xrWaitSwapchainImage guarantees access on the bound queue:
        // keeping the game's DIRECT queue on SteamVR/Radeon also orders the inline and generated eye copies with the
        // compositor's reads.
        let steamvr = crate::runtime_info::RUNTIME.steamvr.load(core::sync::atomic::Ordering::Relaxed);
        let queue = crate::runtime_queue::create(device, queue, vendor == 0x1002 && steamvr, crate::manager_init::test_marker("cvr_xr_game_queue.txt"));
        let binding = GraphicsBindingD3D12 { ty: xr::StructureType::GRAPHICS_BINDING_D3D12_KHR, next: core::ptr::null(), device, queue };
        let info = xr::SessionCreateInfo {
            ty: xr::StructureType::SESSION_CREATE_INFO,
            next: (&binding as *const GraphicsBindingD3D12).cast(),
            create_flags: xr::SessionCreateFlags::EMPTY,
            system_id: system,
        };
        let mut session = xr::Session::NULL;
        let res = xrCreateSession(instance, &info, &mut session);
        if res.into_raw() < 0 {
            log!("OpenXRManager: Failed to create XrSession (res={})\n", res.into_raw());
            return 0;
        }
        session.into_raw()
    }
}

/// The spaces OpenXRManager keeps.
#[repr(C)]
#[derive(Default)]
pub struct SessionSpaces {
    pub local: u64,
    pub view: u64,
    pub hands: [u64; 2],
    pub hand_aims: [u64; 2],
    pub local_is_stage: bool,
}

/// The tracking space (STAGE under SteamVR unless `stage` is false, LOCAL otherwise or when STAGE is refused), the view
/// space, and with an action set the set attached and the hands' grip (and aim) spaces. False when the tracking or the
/// view space cannot be had.
///
/// # Safety
/// `session` is the live session; `hand_paths` two paths; `out` the caller's SessionSpaces.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_create_spaces(
    session: u64,
    stage: bool,
    action_set: u64,
    hand_pose: u64,
    hand_aim_pose: u64,
    hand_paths: *const u64,
    out: *mut SessionSpaces,
) -> bool {
    // SAFETY (whole body): as cvr_xr_create_session; the caller's paths and spaces.
    unsafe {
        let Some(out) = out.as_mut() else { return false };
        *out = SessionSpaces::default();
        let session = xr::Session::from_raw(session);
        let mut local_info = xr::ReferenceSpaceCreateInfo {
            ty: xr::StructureType::REFERENCE_SPACE_CREATE_INFO,
            next: core::ptr::null(),
            reference_space_type: if stage { xr::ReferenceSpaceType::STAGE } else { xr::ReferenceSpaceType::LOCAL },
            pose_in_reference_space: IDENTITY,
        };
        let mut local = xr::Space::NULL;
        let mut res = xrCreateReferenceSpace(session, &local_info, &mut local);
        if res.into_raw() < 0 && stage {
            log!("OpenXRManager: STAGE space unavailable (res={}), tracking in LOCAL\n", res.into_raw());
            local_info.reference_space_type = xr::ReferenceSpaceType::LOCAL;
            res = xrCreateReferenceSpace(session, &local_info, &mut local);
        }
        if res.into_raw() < 0 {
            log!("OpenXRManager: Failed to create local space (res={})\n", res.into_raw());
            return false;
        }
        out.local = local.into_raw();
        out.local_is_stage = local_info.reference_space_type == xr::ReferenceSpaceType::STAGE;
        log!("OpenXRManager: tracking space {}\n", if out.local_is_stage { "STAGE (SteamVR)" } else { "LOCAL" });
        let view_info = xr::ReferenceSpaceCreateInfo {
            ty: xr::StructureType::REFERENCE_SPACE_CREATE_INFO,
            next: core::ptr::null(),
            reference_space_type: xr::ReferenceSpaceType::VIEW,
            pose_in_reference_space: IDENTITY,
        };
        let mut view = xr::Space::NULL;
        let res = xrCreateReferenceSpace(session, &view_info, &mut view);
        if res.into_raw() < 0 {
            log!("OpenXRManager: Failed to create view space (res={})\n", res.into_raw());
            return false;
        }
        out.view = view.into_raw();
        // The handles the head is located with from Rust (crate::frame).
        crate::frame::SESSION.store(session.into_raw(), core::sync::atomic::Ordering::Release);
        crate::frame::LOCAL_SPACE.store(out.local, core::sync::atomic::Ordering::Release);
        crate::frame::VIEW_SPACE.store(out.view, core::sync::atomic::Ordering::Release);
        if action_set != 0 {
            let set = xr::ActionSet::from_raw(action_set);
            let attach = xr::SessionActionSetsAttachInfo {
                ty: xr::StructureType::SESSION_ACTION_SETS_ATTACH_INFO,
                next: core::ptr::null(),
                count_action_sets: 1,
                action_sets: &set,
            };
            xrAttachSessionActionSets(session, &attach);
            for i in 0..2 {
                let path = xr::Path::from_raw(*hand_paths.add(i));
                let space_of = |action: u64| {
                    let info = xr::ActionSpaceCreateInfo {
                        ty: xr::StructureType::ACTION_SPACE_CREATE_INFO,
                        next: core::ptr::null(),
                        action: xr::Action::from_raw(action),
                        subaction_path: path,
                        pose_in_action_space: IDENTITY,
                    };
                    let mut space = xr::Space::NULL;
                    xrCreateActionSpace(session, &info, &mut space);
                    space.into_raw()
                };
                out.hands[i] = space_of(hand_pose);
                if hand_aim_pose != 0 {
                    out.hand_aims[i] = space_of(hand_aim_pose);
                }
            }
        }
        *HAND_SPACES.lock().unwrap_or_else(|p| p.into_inner()) = (out.hands, out.hand_aims);
    }
    true
}

/// The hands' grip and aim spaces as created, for the frame thread (cleared at teardown).
pub static HAND_SPACES: std::sync::Mutex<([u64; 2], [u64; 2])> = std::sync::Mutex::new(([0; 2], [0; 2]));

/// The game device's adapter in the log (formerly LogDxgiAdapterForDevice): its name, ids, memory, whether it is the
/// software rasteriser, and the driver's version.
#[cfg(not(test))]
/// The game's adapter logged; its PCI vendor id (0 when it cannot be had).
fn log_adapter(device: *mut c_void) -> u32 {
    use windows::core::Interface;
    use windows::Win32::Graphics::Direct3D12::ID3D12Device;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIDevice, IDXGIFactory4, DXGI_ADAPTER_FLAG_SOFTWARE};
    // SAFETY (whole body): the live device; DXGI objects created here and released on drop.
    unsafe {
        let Some(device) = ID3D12Device::from_raw_borrowed(&device) else { return 0 };
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory4>() else {
            log!("OpenXRManager: GPU adapter lookup failed (CreateDXGIFactory1).\n");
            return 0;
        };
        let luid = device.GetAdapterLuid();
        let Ok(adapter) = factory.EnumAdapterByLuid::<IDXGIAdapter1>(luid) else {
            log!("OpenXRManager: GPU adapter lookup failed (EnumAdapterByLuid).\n");
            return 0;
        };
        let Ok(desc) = adapter.GetDesc1() else { return 0 };
        let (a, b, c, d) = match adapter.CheckInterfaceSupport(&IDXGIDevice::IID) {
            Ok(v) => {
                let (hi, lo) = ((v >> 32) as u32, v as u32);
                (hi >> 16, hi & 0xFFFF, lo >> 16, lo & 0xFFFF)
            }
            Err(_) => (0, 0, 0, 0),
        };
        let len = desc.Description.iter().position(|&ch| ch == 0).unwrap_or(desc.Description.len());
        let name = String::from_utf16_lossy(&desc.Description[..len]);
        log!(
            "OpenXRManager: GPU adapter=\"{}\" vendor=0x{:04X} device=0x{:04X} subsystem=0x{:08X} dedicatedVRAM={}MB sharedRAM={}MB software={} driver={}.{}.{}.{}\n",
            name,
            desc.VendorId,
            desc.DeviceId,
            desc.SubSysId,
            desc.DedicatedVideoMemory as u64 / (1024 * 1024),
            desc.SharedSystemMemory as u64 / (1024 * 1024),
            (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0) as i32,
            a,
            b,
            c,
            d
        );
        desc.VendorId
    }
}
