//! Streamline's structures as the engine hands them over (externals/streamline/include: sl_struct.h, sl_consts.h,
//! sl_core_types.h, sl_dlss.h; Ray Reconstruction's options as src/Stereo/EngineStereoDlss.cpp declares them from
//! sl_dlss_d.h). Plain `repr(C)` mirrors; the C++ half static_asserts their sizes and offsets against the headers.
use core::ffi::c_void;

/// sl::StructType: a GUID.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct StructType {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}
const fn guid(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> StructType {
    StructType {
        data1,
        data2,
        data3,
        data4,
    }
}

/// sl::BaseStructure: the chain link, the type and its version.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BaseStructure {
    pub next: *mut BaseStructure,
    pub struct_type: StructType,
    pub struct_version: usize,
}
const _: () = assert!(core::mem::size_of::<BaseStructure>() == 32);

pub const RESOURCE: StructType = guid(
    0x3a9d70cf,
    0x2418,
    0x4b72,
    [0x83, 0x91, 0x13, 0xf8, 0x72, 0x1c, 0x72, 0x61],
);
pub const RESOURCE_TAG: StructType = guid(
    0x4c6a5aad,
    0xb445,
    0x496c,
    [0x87, 0xff, 0x1a, 0xf3, 0x84, 0x5b, 0xe6, 0x53],
);
pub const CONSTANTS: StructType = guid(
    0xdcd35ad7,
    0x4e4a,
    0x4bad,
    [0xa9, 0x0c, 0xe0, 0xc4, 0x9e, 0xb2, 0x3a, 0xfe],
);
pub const VIEWPORT_HANDLE: StructType = guid(
    0x171b6435,
    0x9b3c,
    0x4fc8,
    [0x99, 0x94, 0xfb, 0xe5, 0x25, 0x69, 0xaa, 0xa4],
);
pub const DLSS_OPTIONS: StructType = guid(
    0x6ac826e4,
    0x4c61,
    0x4101,
    [0xa9, 0x2d, 0x63, 0x8d, 0x42, 0x10, 0x57, 0xb8],
);
pub const DLSSD_OPTIONS: StructType = guid(
    0x0ad87504,
    0x774e,
    0x4bf3,
    [0x96, 0x33, 0xa4, 0x4d, 0x1f, 0x7f, 0x9c, 0xb8],
);

pub type BufferType = u32;
pub const BUFFER_DEPTH: BufferType = 0;
pub const BUFFER_MOTION_VECTORS: BufferType = 1;
pub const BUFFER_SCALING_INPUT_COLOR: BufferType = 3;
pub const BUFFER_SCALING_OUTPUT_COLOR: BufferType = 4;
pub const BUFFER_EXPOSURE: BufferType = 13;
pub type Feature = u32;
pub const FEATURE_DLSS: Feature = 0;
pub const FEATURE_DLSS_RR: Feature = 1001;

/// sl::Result (an int enum).
pub type SlResult = i32;
pub const RESULT_OK: SlResult = 0;
pub const RESULT_ERROR_INVALID_PARAMETER: SlResult = 7;

/// sl::Extent.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Extent {
    pub top: u32,
    pub left: u32,
    pub width: u32,
    pub height: u32,
}

/// sl::Resource.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Resource {
    pub base: BaseStructure,
    /// ResourceType (char): 0 eTex2d.
    pub ty: u8,
    pub native: *mut c_void,
    pub memory: *mut c_void,
    pub view: *mut c_void,
    pub state: u32,
    pub width: u32,
    pub height: u32,
    pub native_format: u32,
    pub mip_levels: u32,
    pub array_layers: u32,
    pub gpu_virtual_address: u64,
    pub flags: u32,
    pub usage: u32,
    pub internal_flags: u16,
    pub reserved: u16,
}
impl Resource {
    /// `sl::Resource(eTex2d, native, state)`.
    pub fn tex2d(native: *mut c_void, state: u32) -> Resource {
        Resource {
            base: BaseStructure {
                next: core::ptr::null_mut(),
                struct_type: RESOURCE,
                struct_version: 1,
            },
            ty: 0,
            native,
            memory: core::ptr::null_mut(),
            view: core::ptr::null_mut(),
            state,
            width: 0,
            height: 0,
            native_format: 0,
            mip_levels: 0,
            array_layers: 0,
            gpu_virtual_address: 0,
            flags: 0,
            usage: 0,
            internal_flags: 0,
            reserved: 0,
        }
    }
}

/// sl::ResourceTag.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ResourceTag {
    pub base: BaseStructure,
    pub resource: *mut Resource,
    pub ty: BufferType,
    pub lifecycle: u32,
    pub extent: Extent,
}

/// sl::ViewportHandle.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ViewportHandle {
    pub base: BaseStructure,
    pub value: u32,
}
impl ViewportHandle {
    pub fn new(value: u32) -> ViewportHandle {
        ViewportHandle {
            base: BaseStructure {
                next: core::ptr::null_mut(),
                struct_type: VIEWPORT_HANDLE,
                struct_version: 1,
            },
            value,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Float2 {
    pub x: f32,
    pub y: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Float3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Float4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}
pub type Float4x4 = [Float4; 4];

/// sl::Constants (version 2).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Constants {
    pub base: BaseStructure,
    pub camera_view_to_clip: Float4x4,
    pub clip_to_camera_view: Float4x4,
    pub clip_to_lens_clip: Float4x4,
    pub clip_to_prev_clip: Float4x4,
    pub prev_clip_to_clip: Float4x4,
    pub jitter_offset: Float2,
    pub mvec_scale: Float2,
    pub camera_pinhole_offset: Float2,
    pub camera_pos: Float3,
    pub camera_up: Float3,
    pub camera_right: Float3,
    pub camera_fwd: Float3,
    pub camera_near: f32,
    pub camera_far: f32,
    pub camera_fov: f32,
    pub camera_aspect_ratio: f32,
    pub motion_vectors_invalid_value: f32,
    pub depth_inverted: u8,
    pub camera_motion_included: u8,
    pub motion_vectors_3d: u8,
    pub reset: u8,
    pub orthographic_projection: u8,
    pub motion_vectors_dilated: u8,
    pub motion_vectors_jittered: u8,
    pub min_relative_linear_depth_object_separation: f32,
}

/// sl::DLSSMode.
pub const DLSS_MODE_OFF: u32 = 0;
pub const DLSS_MODE_MAX_PERFORMANCE: u32 = 1;
pub const DLSS_MODE_BALANCED: u32 = 2;
pub const DLSS_MODE_MAX_QUALITY: u32 = 3;
pub const DLSS_MODE_ULTRA_PERFORMANCE: u32 = 4;
pub const DLSS_MODE_ULTRA_QUALITY: u32 = 5;
pub const DLSS_MODE_DLAA: u32 = 6;

/// sl::Boolean (char).
pub const FALSE: u8 = 0;
pub const TRUE: u8 = 1;

/// sl::DLSSOptimalSettings (version 1): what DLSS renders for an output.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DlssOptimalSettings {
    pub base: BaseStructure,
    pub optimal_render_width: u32,
    pub optimal_render_height: u32,
    pub optimal_sharpness: f32,
    pub render_width_min: u32,
    pub render_height_min: u32,
    pub render_width_max: u32,
    pub render_height_max: u32,
}
const _: () = assert!(core::mem::offset_of!(DlssOptimalSettings, optimal_render_width) == 32);
const _: () = assert!(core::mem::offset_of!(DlssOptimalSettings, render_height_max) == 56);

/// sl::DLSSOptions (version 3).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DlssOptions {
    pub base: BaseStructure,
    pub mode: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub sharpness: f32,
    pub pre_exposure: f32,
    pub exposure_scale: f32,
    pub color_buffers_hdr: u8,
    pub indicator_invert_axis_x: u8,
    pub indicator_invert_axis_y: u8,
    pub dlaa_preset: u32,
    pub quality_preset: u32,
    pub balanced_preset: u32,
    pub performance_preset: u32,
    pub ultra_performance_preset: u32,
    pub ultra_quality_preset: u32,
    pub use_auto_exposure: u8,
    pub alpha_upscaling_enabled: u8,
}
impl Default for DlssOptions {
    /// `sl::DLSSOptions{}`: its member initialisers.
    fn default() -> DlssOptions {
        DlssOptions {
            base: BaseStructure {
                next: core::ptr::null_mut(),
                struct_type: DLSS_OPTIONS,
                struct_version: 3,
            },
            mode: DLSS_MODE_OFF,
            output_width: u32::MAX,
            output_height: u32::MAX,
            sharpness: 0.0,
            pre_exposure: 1.0,
            exposure_scale: 1.0,
            color_buffers_hdr: TRUE,
            indicator_invert_axis_x: FALSE,
            indicator_invert_axis_y: FALSE,
            dlaa_preset: 0,
            quality_preset: 0,
            balanced_preset: 0,
            performance_preset: 0,
            ultra_performance_preset: 0,
            ultra_quality_preset: 0,
            use_auto_exposure: FALSE,
            alpha_upscaling_enabled: FALSE,
        }
    }
}

/// sl::DLSSDOptions (Ray Reconstruction, version 3) as EngineStereoDlss.cpp declares it.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DlssdOptions {
    pub base: BaseStructure,
    pub mode: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub sharpness: f32,
    pub pre_exposure: f32,
    pub exposure_scale: f32,
    pub color_buffers_hdr: u8,
    pub indicator_invert_axis_x: u8,
    pub indicator_invert_axis_y: u8,
    pub normal_roughness_mode: u32,
    pub world_to_camera_view: Float4x4,
    pub camera_view_to_world: Float4x4,
    pub alpha_upscaling_enabled: u8,
    pub dlaa_preset: u32,
    pub quality_preset: u32,
    pub balanced_preset: u32,
    pub performance_preset: u32,
    pub ultra_performance_preset: u32,
    pub ultra_quality_preset: u32,
}

/// The sizes and offsets the C++ half asserts against the headers (src/Stereo/EngineStereoDlss.cpp).
pub const SIZES: [usize; 6] = [
    core::mem::size_of::<Resource>(),
    core::mem::size_of::<ResourceTag>(),
    core::mem::size_of::<ViewportHandle>(),
    core::mem::size_of::<Constants>(),
    core::mem::size_of::<DlssOptions>(),
    core::mem::size_of::<DlssdOptions>(),
];
const _: () = assert!(
    core::mem::size_of::<Resource>() == 112
        && core::mem::size_of::<ResourceTag>() == 64
        && core::mem::size_of::<ViewportHandle>() == 40
        && core::mem::size_of::<Constants>() == 456
        && core::mem::size_of::<DlssOptions>() == 88
        && core::mem::size_of::<DlssdOptions>() == 224
);
