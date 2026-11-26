//! The OpenXR instance and system (formerly the first half of OpenXRManager::Init): the extensions the port asks for,
//! the instance, the runtime's identity, the head-mounted system and the stereo views.
//!
//! The steps before it stay in C++ for now (the Vive hand-tracking layer's off switch, xr_runtime=1's SteamVR
//! manifest); the extensions of the Vive trackers and the finger bones are chosen by their C++ owners
//! (`cvr_cpp_xr_optional_extensions`), which are told the new instance (`cvr_cpp_xr_on_instance_created`).
#![cfg_attr(test, allow(dead_code, unused_imports))]
use core::ffi::{c_char, CStr};
use cvr_host::log;
use openxr_sys as xr;
use xr::Handle as _;

#[cfg(not(test))]
extern "system" {
    fn xrEnumerateInstanceExtensionProperties(layer: *const c_char, capacity: u32, count: *mut u32, properties: *mut xr::ExtensionProperties) -> xr::Result;
    fn xrCreateInstance(info: *const xr::InstanceCreateInfo, instance: *mut xr::Instance) -> xr::Result;
    fn xrDestroyInstance(instance: xr::Instance) -> xr::Result;
    fn xrGetInstanceProperties(instance: xr::Instance, properties: *mut xr::InstanceProperties) -> xr::Result;
    fn xrGetSystem(instance: xr::Instance, info: *const xr::SystemGetInfo, system: *mut xr::SystemId) -> xr::Result;
    fn xrGetSystemProperties(instance: xr::Instance, system: xr::SystemId, properties: *mut xr::SystemProperties) -> xr::Result;
    fn xrEnumerateViewConfigurationViews(
        instance: xr::Instance,
        system: xr::SystemId,
        kind: xr::ViewConfigurationType,
        capacity: u32,
        count: *mut u32,
        views: *mut xr::ViewConfigurationView,
    ) -> xr::Result;
}
#[cfg(not(test))]
extern "C" {
    /// xr_runtime (src/Core/VrCore.cpp): 1 asks for SteamVR.
    fn GetXrRuntimeMode() -> i32;
    /// The Vive trackers' and the finger bones' own extensions, appended to `out` (static strings); their count.
    fn cvr_cpp_xr_optional_extensions(available: *const xr::ExtensionProperties, count: u32, out: *mut *const c_char, capacity: u32) -> u32;
    fn cvr_cpp_xr_on_instance_created(instance: u64);
}

/// The API the port was written against (the loader's headers: OpenXR-SDK release-1.0.34).
const API_VERSION: xr::Version = xr::Version::new(1, 0, 34);
const D3D12_ENABLE: &CStr = c"XR_KHR_D3D12_enable";
const COMPOSITION_LAYER_DEPTH: &CStr = c"XR_KHR_composition_layer_depth";
/// include/Runtimes/SteamFrameInput.hpp SteamFrameExtension.
const STEAM_FRAME: &CStr = c"XR_VALVE_frame_controller_interaction";
const MAX_VIEWS: usize = 4;

/// What OpenXRManager keeps of the instance and the system.
#[repr(C)]
pub struct InstanceSetup {
    pub instance: u64,
    pub system_id: u64,
    pub view_count: u32,
    pub views: [xr::ViewConfigurationView; MAX_VIEWS],
    pub eye_image_limit: [u32; 2],
    pub system_name: [c_char; xr::MAX_SYSTEM_NAME_SIZE],
    pub depth_layer_supported: bool,
    pub steam_frame_enabled: bool,
    pub runtime_is_steamvr: bool,
    pub runtime_is_virtual_desktop: bool,
    pub runtime_is_simulator: bool,
    pub runtime_is_psvr2: bool,
}

/// OpenXRManager.cpp ClassifyOpenXRRuntime: the runtime by its name.
pub fn classify_runtime(name: &str) -> &'static str {
    if name.is_empty() {
        "Unknown"
    } else if name.contains("SteamVR") {
        "SteamVR"
    } else if name.contains("VirtualDesktop") || name.contains("Virtual Desktop") {
        "Virtual Desktop"
    } else if name.contains("Oculus") || name.contains("Meta") {
        "Meta/Oculus"
    } else if name.contains("Windows Mixed Reality") || name.contains("Mixed Reality") {
        "Windows Mixed Reality"
    } else if name.contains("OpenComposite") {
        "OpenComposite"
    } else {
        "OpenXR"
    }
}
/// include/Hooks/Psvr2Input.hpp IsPsvr2System.
pub fn is_psvr2_system(name: &str) -> bool {
    let value = name.to_ascii_lowercase();
    value.contains("playstation_vr2") || value.contains("playstation vr2") || value.contains("psvr2")
}
fn text(chars: &[c_char]) -> String {
    let bytes: Vec<u8> = chars.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}
fn version(v: xr::Version) -> String {
    format!("{}.{}.{}", v.major(), v.minor(), v.patch())
}

/// The extensions, the instance, the runtime, the system and the views. False (and no instance left behind) when the
/// instance or the system cannot be had.
///
/// # Safety
/// `out` is the caller's InstanceSetup.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_create_instance(out: *mut InstanceSetup) -> bool {
    // SAFETY (whole body): the OpenXR loader's calls with structures of the types they take; the caller's setup.
    unsafe {
        let Some(out) = out.as_mut() else { return false };
        core::ptr::write_bytes(out as *mut InstanceSetup, 0, 1);
        let mut extensions: Vec<*const c_char> = vec![D3D12_ENABLE.as_ptr()];
        // Optional depth submission and native Steam Frame controller bindings.
        let mut count = 0u32;
        xrEnumerateInstanceExtensionProperties(core::ptr::null(), 0, &mut count, core::ptr::null_mut());
        let mut properties = vec![
            xr::ExtensionProperties { ty: xr::StructureType::EXTENSION_PROPERTIES, next: core::ptr::null_mut(), extension_name: [0; xr::MAX_EXTENSION_NAME_SIZE], extension_version: 0 };
            count as usize
        ];
        if count > 0 && xrEnumerateInstanceExtensionProperties(core::ptr::null(), count, &mut count, properties.as_mut_ptr()).into_raw() >= 0 {
            properties.truncate(count as usize);
            for p in &properties {
                let name = CStr::from_ptr(p.extension_name.as_ptr());
                if name == COMPOSITION_LAYER_DEPTH {
                    out.depth_layer_supported = true;
                    extensions.push(COMPOSITION_LAYER_DEPTH.as_ptr());
                } else if name == STEAM_FRAME {
                    out.steam_frame_enabled = true;
                    extensions.push(STEAM_FRAME.as_ptr());
                }
            }
        } else {
            properties.clear();
        }
        let mut optional = [core::ptr::null::<c_char>(); 8];
        let added = cvr_cpp_xr_optional_extensions(properties.as_ptr(), properties.len() as u32, optional.as_mut_ptr(), optional.len() as u32);
        extensions.extend_from_slice(&optional[..(added as usize).min(optional.len())]);
        log!("OpenXRManager: depth-layer (XR_KHR_composition_layer_depth) supported={}\n", out.depth_layer_supported as i32);
        log!("OpenXRManager: Steam Frame controller ({}) supported={}\n", STEAM_FRAME.to_str().unwrap_or(""), out.steam_frame_enabled as i32);

        let mut info = xr::InstanceCreateInfo {
            ty: xr::StructureType::INSTANCE_CREATE_INFO,
            next: core::ptr::null(),
            create_flags: xr::InstanceCreateFlags::EMPTY,
            application_info: xr::ApplicationInfo {
                application_name: [0; xr::MAX_APPLICATION_NAME_SIZE],
                application_version: 0,
                engine_name: [0; xr::MAX_ENGINE_NAME_SIZE],
                engine_version: 0,
                api_version: API_VERSION,
            },
            enabled_api_layer_count: 0,
            enabled_api_layer_names: core::ptr::null(),
            enabled_extension_count: extensions.len() as u32,
            enabled_extension_names: extensions.as_ptr(),
        };
        for (d, s) in info.application_info.application_name.iter_mut().zip(c"CyberpunkVRPort".to_bytes()) {
            *d = *s as c_char;
        }
        let mut instance = xr::Instance::NULL;
        let res = xrCreateInstance(&info, &mut instance);
        if res.into_raw() < 0 {
            log!("OpenXRManager: Failed to create XrInstance (res={})\n", res.into_raw());
            return false;
        }
        cvr_cpp_xr_on_instance_created(instance.into_raw());

        let mut props: xr::InstanceProperties = core::mem::zeroed();
        props.ty = xr::StructureType::INSTANCE_PROPERTIES;
        if xrGetInstanceProperties(instance, &mut props).into_raw() >= 0 {
            let name = text(&props.runtime_name);
            let kind = classify_runtime(&name);
            log!("OpenXRManager: OpenXR runtime name=\"{name}\" kind={kind} version={}\n", version(props.runtime_version));
            let steamvr = kind == "SteamVR";
            out.runtime_is_virtual_desktop = kind == "Virtual Desktop";
            // The pose-pair lock keys off the ACTIVE runtime, independent of xr_runtime.
            out.runtime_is_steamvr = steamvr;
            log!("OpenXRManager: runtimeIsSteamVR={} (pose-pair lock {})\n", steamvr as i32, if steamvr { "ENABLED" } else { "off" });
            let simulator = !name.is_empty() && name.contains("Simulator");
            out.runtime_is_simulator = simulator;
            if simulator {
                log!("OpenXRManager: OpenXR Simulator runtime: grip poses are corrected (aim -> grip)\n");
            }
            if GetXrRuntimeMode() == 1 && !steamvr {
                log!("OpenXRManager: xr_runtime=1 requested SteamVR, but the active runtime identified as {kind}.\n");
            }
        }

        let system_info = xr::SystemGetInfo { ty: xr::StructureType::SYSTEM_GET_INFO, next: core::ptr::null(), form_factor: xr::FormFactor::HEAD_MOUNTED_DISPLAY };
        let mut system = xr::SystemId::NULL;
        let res = xrGetSystem(instance, &system_info, &mut system);
        if res.into_raw() < 0 {
            log!("OpenXRManager: Failed to get XrSystemId (res={})\n", res.into_raw());
            xrDestroyInstance(instance);
            return false;
        }

        let mut sys: xr::SystemProperties = core::mem::zeroed();
        sys.ty = xr::StructureType::SYSTEM_PROPERTIES;
        if xrGetSystemProperties(instance, system, &mut sys).into_raw() >= 0 {
            out.system_name = sys.system_name;
            out.eye_image_limit = [sys.graphics_properties.max_swapchain_image_width, sys.graphics_properties.max_swapchain_image_height];
            let name = text(&sys.system_name);
            out.runtime_is_psvr2 = is_psvr2_system(&name);
            if out.runtime_is_psvr2 {
                log!(
                    "OpenXRManager[PSVR2]: Sense Triangle touch D-pad and Create SystemButton enabled. Haptic output is selected separately in Controls.\n"
                );
            }
            log!(
                "OpenXRManager: OpenXR system vendorId={:#X} systemName=\"{name}\" maxSwapchain={}x{} maxLayerCount={} positionTracking={} orientationTracking={}\n",
                sys.vendor_id,
                sys.graphics_properties.max_swapchain_image_width,
                sys.graphics_properties.max_swapchain_image_height,
                sys.graphics_properties.max_layer_count,
                (sys.tracking_properties.position_tracking != xr::FALSE) as i32,
                (sys.tracking_properties.orientation_tracking != xr::FALSE) as i32
            );
        }

        let mut view_count = 0u32;
        let stereo = xr::ViewConfigurationType::PRIMARY_STEREO;
        if xrEnumerateViewConfigurationViews(instance, system, stereo, 0, &mut view_count, core::ptr::null_mut()).into_raw() >= 0 && view_count > 0 {
            let mut views: Vec<xr::ViewConfigurationView> = (0..view_count)
                .map(|_| {
                    let mut v: xr::ViewConfigurationView = core::mem::zeroed();
                    v.ty = xr::StructureType::VIEW_CONFIGURATION_VIEW;
                    v
                })
                .collect();
            xrEnumerateViewConfigurationViews(instance, system, stereo, view_count, &mut view_count, views.as_mut_ptr());
            for (eye, view) in views.iter().enumerate().take(view_count as usize) {
                log!(
                    "OpenXRManager[FOV]: viewConfig eye={eye} recommended={}x{} max={}x{} samples={}\n",
                    view.recommended_image_rect_width,
                    view.recommended_image_rect_height,
                    view.max_image_rect_width,
                    view.max_image_rect_height,
                    view.recommended_swapchain_sample_count
                );
                // A runtime that leaves a maximum at 0 sets no limit there.
                if view.max_image_rect_width != 0 && (out.eye_image_limit[0] == 0 || view.max_image_rect_width < out.eye_image_limit[0]) {
                    out.eye_image_limit[0] = view.max_image_rect_width;
                }
                if view.max_image_rect_height != 0 && (out.eye_image_limit[1] == 0 || view.max_image_rect_height < out.eye_image_limit[1]) {
                    out.eye_image_limit[1] = view.max_image_rect_height;
                }
                if eye < MAX_VIEWS {
                    out.views[eye] = *view;
                }
            }
            out.view_count = view_count.min(MAX_VIEWS as u32);
        }
        out.instance = instance.into_raw();
        out.system_id = system.into_raw();
        log!("OpenXRManager: OpenXR Initialized. SystemID={}\n", out.system_id);
        // The instance, its system and the views it reported (crate::view_configs).
        crate::view_configs::set_system(out.instance, out.system_id);
        crate::view_configs::record(&out.views[..out.view_count as usize]);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtimes_by_name() {
        assert_eq!(classify_runtime("SteamVR/OpenXR"), "SteamVR");
        assert_eq!(classify_runtime("Oculus"), "Meta/Oculus");
        assert_eq!(classify_runtime("OpenXR Simulator"), "OpenXR");
        assert!(is_psvr2_system("PlayStation VR2"));
        assert!(!is_psvr2_system("Quest 3"));
    }
}
