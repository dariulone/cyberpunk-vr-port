//! AFW -- ALTERNATE FRAME WARPING (formerly src/Render/Afw.cpp; include/Render/Afw.hpp says what the launch is): the
//! engine draws one eye per frame, the eyes taking turns, and the other eye is made for the same moment out of the drawn
//! eye's colour, depth and motion -- by the port's own OSR (cvr_gpu::osr), or by PureDark's plugin (PDAFWPlugin.dll,
//! not shipped; include/Render/AfwPlugin.hpp is its ABI, mirrored here as it is).
//!
//! The exports the tools read and write (CyberpunkVR_AfwStats, CyberpunkVR_AfwTune, CyberpunkVR_AfwMotionScale) stay
//! the C++ half's, defined in Afw.cpp.
use core::ffi::{c_char, c_void, CStr};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU32, Ordering::Relaxed};
use std::sync::Mutex;

use cvr_crt::{format, Arg};
use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, RECT};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, IDXGIAdapter1, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use crate::afw_cameras::{inverse, to_mat, view_to_clip, world_to_view, Mat4, M4};
use crate::framegen_inputs::Inputs;

extern "C" {
    /// [0] frames warped, [1] fallbacks, [2] plugin state (0 not tried, 1 loaded, -1 failed); of the warped frames whose
    /// renderer frame directly follows the last warped one, [3] drawn for the other eye, [4] for the same eye; [5]
    /// warped frames after a gap.
    static CyberpunkVR_AfwStats: [AtomicI64; 6];
    /// [0] warp mode, [1] motion vectors' type (3: from the frame before), [2] IgnoreMotionThreshold x100, [3] the
    /// plugin's debug view, [4] clear before warping, [5] motion vectors off, [6] 0 the plugin / 1 OSR, [7] OSR's depth
    /// jitter (0 subtracted, 1 added, 2 none). Written by tools into the running game.
    static CyberpunkVR_AfwTune: [AtomicI32; 8];
    static CyberpunkVR_AfwMotionScale: [f32; 2];
    static CyberpunkVR_OsrStats: [AtomicI64; 3];
    static CyberpunkVR_OsrTune: [i32; 4];
}
fn tune(i: usize) -> i32 {
    // SAFETY: an export of the C++ half.
    unsafe { CyberpunkVR_AfwTune[i].load(Relaxed) }
}
fn stats(i: usize) -> &'static AtomicI64 {
    // SAFETY: an export of the C++ half.
    unsafe { &CyberpunkVR_AfwStats[i] }
}
fn motion_scale(i: usize) -> f32 {
    // SAFETY: an export of the C++ half, written by tools.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_AfwMotionScale[i]) }
}
fn log(text: &str) {
    cvr_host::log::write(text);
}

// ---- PureDark's plugin ABI (include/Render/AfwPlugin.hpp) -------------------------------------------------------

#[repr(C)]
struct DeviceParams {
    d3d11_device: *mut c_void,
    d3d11_context: *mut c_void,
    d3d12_device: *mut c_void,
    d3d12_queue: *mut c_void,
    dxgi_adapter: *mut c_void,
}
#[repr(C)]
struct FrameWarpInitParams {
    hmd_width: i32,
    hmd_height: i32,
    eye_format: DXGI_FORMAT,
    backbuffer_format: DXGI_FORMAT,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct TextureDesc {
    kind: i32,
    texture: *mut c_void,
    srv_pos: i32,
    uav_pos: i32,
    srv_handle: u64,
    uav_handle: u64,
    rtv_or_dsv: usize,
    initial_state: D3D12_RESOURCE_STATES,
}
impl Default for TextureDesc {
    fn default() -> Self {
        TextureDesc {
            kind: 0,
            texture: core::ptr::null_mut(),
            srv_pos: -1,
            uav_pos: -1,
            srv_handle: 0,
            uav_handle: 0,
            rtv_or_dsv: 0,
            initial_state: D3D12_RESOURCE_STATE_COMMON,
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FrameBufferDesc {
    color: TextureDesc,
    depth: TextureDesc,
    motion_vectors: TextureDesc,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct EyeFrameBuffers {
    eye_frame_buffers: [FrameBufferDesc; 2],
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct PdCameraData {
    dest_world_to_view: Mat4,
    dest_view_to_world: Mat4,
    dest_view_to_clip: Mat4,
    dest_clip_to_view: Mat4,
    src_world_to_view: Mat4,
    src_view_to_world: Mat4,
    src_view_to_clip: Mat4,
    src_clip_to_view: Mat4,
    cam_world_to_view: Mat4,
    cam_view_to_world: Mat4,
    cam_view_to_clip: Mat4,
    cam_clip_to_view: Mat4,
}
// pd::MVType
const MV_NORMAL: i32 = 0;
const MV_FROM_OTHER_EYE: i32 = 1;
#[repr(C)]
struct FrameWarpEvaluateParams {
    in_cmd_list: *mut c_void,
    in_eye_frame_buffer: *mut FrameBufferDesc,
    out_eye_frame_buffer: *mut FrameBufferDesc,
    in_ui_color_alpha: *mut TextureDesc,
    in_ui_scale: [f32; 2],
    in_ui_pos: [f32; 3],
    in_motion_scale: [f32; 2],
    mode: i32,
    eye_index: i32,
    camera_data: *mut PdCameraData,
    clear_before_warping: bool,
    ignore_motion_threshold: f32,
    is_hudless_color: bool,
    motion_vectors_type: i32,
    debug: bool,
    is_foveated: bool,
    foveated_area: RECT,
    in_ue_velocity_buffer: *mut TextureDesc,
    use_uint64: bool,
    shading_rate: u32,
    reserved: [f32; 12],
}
impl Default for FrameWarpEvaluateParams {
    /// `FrameWarpEvaluateParams p{}`: the member initialisers, everything else zero.
    fn default() -> Self {
        FrameWarpEvaluateParams {
            in_cmd_list: core::ptr::null_mut(),
            in_eye_frame_buffer: core::ptr::null_mut(),
            out_eye_frame_buffer: core::ptr::null_mut(),
            in_ui_color_alpha: core::ptr::null_mut(),
            in_ui_scale: [1.0, 1.0],
            in_ui_pos: [0.0, 0.0, -1.0],
            in_motion_scale: [0.0, 0.0],
            mode: 0,
            eye_index: 0,
            camera_data: core::ptr::null_mut(),
            clear_before_warping: false,
            ignore_motion_threshold: 2.5,
            is_hudless_color: true,
            motion_vectors_type: MV_NORMAL,
            debug: false,
            is_foveated: false,
            foveated_area: RECT::default(),
            in_ue_velocity_buffer: core::ptr::null_mut(),
            use_uint64: false,
            shading_rate: (1 << 3) | 1,
            reserved: [0.0; 12],
        }
    }
}
const _: () = assert!(
    core::mem::size_of::<TextureDesc>() == 56
        && core::mem::size_of::<FrameBufferDesc>() == 168
        && core::mem::size_of::<EyeFrameBuffers>() == 336
        && core::mem::size_of::<PdCameraData>() == 768
        && core::mem::offset_of!(FrameWarpEvaluateParams, camera_data) == 72
        && core::mem::offset_of!(FrameWarpEvaluateParams, foveated_area) == 100
        && core::mem::offset_of!(FrameWarpEvaluateParams, shading_rate) == 132
        && core::mem::size_of::<FrameWarpEvaluateParams>() == 184
);

/// pd::D3D12RendererAPI's virtual table, up to the one call made here (its 17th slot).
#[repr(C)]
struct RendererVtbl {
    _before: [usize; 16],
    create_texture: unsafe extern "system" fn(
        this: *mut RendererApi,
        width: i32,
        height: i32,
        format: DXGI_FORMAT,
        initial_state: D3D12_RESOURCE_STATES,
        texture: *mut TextureDesc,
        create_uav: bool,
    ) -> bool,
}
#[repr(C)]
struct RendererApi {
    vtbl: *const RendererVtbl,
}
type InitDeviceFn = unsafe extern "system" fn(DeviceParams) -> *mut RendererApi;
type InitFrameWarpFn = unsafe extern "system" fn(FrameWarpInitParams) -> EyeFrameBuffers;
type EvaluateFrameWarpFn = unsafe extern "system" fn(*mut FrameWarpEvaluateParams);

// ---- the state --------------------------------------------------------------------------------------------------

/// cvr::afw::Stats.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Stats {
    pub warped: u64,
    pub fallbacks: u64,
    /// The last failure: a static string (this module's, or the C++ caller's literal).
    pub last: *const c_char,
}

struct Plugin {
    module: HMODULE,
    init_device: Option<InitDeviceFn>,
    init_warp: Option<InitFrameWarpFn>,
    evaluate: Option<EvaluateFrameWarpFn>,
    api: *mut RendererApi,
    device: *mut c_void,
    /// The plugin's eye buffers (InitFrameWarp), sized to the eyes.
    eyes: EyeFrameBuffers,
    width: u32,
    height: u32,
    /// The drawn eye's depth and motion, one pair per eye kept across frames (the plugin reads the other eye's from
    /// the frame before), sized to the inputs.
    depth: [TextureDesc; 2],
    motion: [TextureDesc; 2],
    depth_w: u32,
    depth_h: u32,
    motion_w: u32,
    motion_h: u32,
    depth_format: DXGI_FORMAT,
    motion_format: DXGI_FORMAT,
    stats: Stats,
    /// The fallback reason's run (logged when it changes and at powers of two).
    run: u64,
    /// One camera per eye, kept: the plugin may read the other eye's from the frame before, as UEVR keeps them.
    cameras: [PdCameraData; 2],
    frame: FrameBufferDesc,
    last_frame: u32,
    last_eye: i32,
}
// SAFETY: the plugin's objects and the static strings are only used under the mutex.
unsafe impl Send for Plugin {}
static G: Mutex<Plugin> = Mutex::new(Plugin {
    module: HMODULE(core::ptr::null_mut()),
    init_device: None,
    init_warp: None,
    evaluate: None,
    api: core::ptr::null_mut(),
    device: core::ptr::null_mut(),
    eyes: EyeFrameBuffers {
        eye_frame_buffers: [FrameBufferDesc {
            color: TD0,
            depth: TD0,
            motion_vectors: TD0,
        }; 2],
    },
    width: 0,
    height: 0,
    depth: [TD0; 2],
    motion: [TD0; 2],
    depth_w: 0,
    depth_h: 0,
    motion_w: 0,
    motion_h: 0,
    depth_format: DXGI_FORMAT_UNKNOWN,
    motion_format: DXGI_FORMAT_UNKNOWN,
    stats: Stats {
        warped: 0,
        fallbacks: 0,
        last: c"".as_ptr(),
    },
    run: 0,
    cameras: [PD_CAMERA0; 2],
    frame: FrameBufferDesc {
        color: TD0,
        depth: TD0,
        motion_vectors: TD0,
    },
    last_frame: 0,
    last_eye: -1,
});
const TD0: TextureDesc = TextureDesc {
    kind: 0,
    texture: core::ptr::null_mut(),
    srv_pos: -1,
    uav_pos: -1,
    srv_handle: 0,
    uav_handle: 0,
    rtv_or_dsv: 0,
    initial_state: D3D12_RESOURCE_STATE_COMMON,
};
const M0: Mat4 = Mat4 { m: [0.0; 16] };
const PD_CAMERA0: PdCameraData = PdCameraData {
    dest_world_to_view: M0,
    dest_view_to_world: M0,
    dest_view_to_clip: M0,
    dest_clip_to_view: M0,
    src_world_to_view: M0,
    src_view_to_world: M0,
    src_view_to_clip: M0,
    src_clip_to_view: M0,
    cam_world_to_view: M0,
    cam_view_to_world: M0,
    cam_view_to_clip: M0,
    cam_clip_to_view: M0,
};
/// Loading the plugin failed: the launch stays one picture for both eyes (OSR makes the other eye).
static FAILED: AtomicBool = AtomicBool::new(false);
static LAUNCH: AtomicBool = AtomicBool::new(false);
fn lock() -> std::sync::MutexGuard<'static, Plugin> {
    G.lock().unwrap_or_else(|p| p.into_inner())
}

fn fail(g: &mut Plugin, why: &'static CStr) {
    FAILED.store(true, Relaxed);
    g.stats.last = why.as_ptr();
    stats(2).store(-1, Relaxed);
    log(&format!(
        "[afw] {}: the launch stays one picture for both eyes\n",
        why.to_string_lossy()
    ));
}

/// Fallback with the state held.
fn fallback(g: &mut Plugin, why: *const c_char) {
    g.stats.fallbacks += 1;
    // SAFETY: both are static NUL-terminated strings (or the caller's literal).
    let same = unsafe {
        !why.is_null()
            && !g.stats.last.is_null()
            && CStr::from_ptr(why) == CStr::from_ptr(g.stats.last)
    };
    g.run = if same { g.run + 1 } else { 1 };
    if g.run & (g.run - 1) == 0 {
        // SAFETY: as above.
        let text = if why.is_null() {
            "(null)".into()
        } else {
            unsafe { CStr::from_ptr(why) }.to_string_lossy()
        };
        log(&format!(
            "[afw] fallback x{}: {} (warped {} so far)\n",
            g.run, text, g.stats.warped
        ));
    }
    g.stats.last = why;
    stats(1).fetch_add(1, Relaxed);
}

unsafe fn load(g: &mut Plugin, device: *mut c_void, queue: *mut c_void) -> bool {
    if !g.api.is_null() {
        return g.device == device;
    }
    if FAILED.load(Relaxed) {
        return false;
    }
    let path = cvr_host::paths::file(cvr_host::paths::Folder::Root, "PDAFWPlugin.dll");
    let wide: Vec<u16> = path.as_os_str().encode_wide_nul();
    // SAFETY: a NUL-terminated path; the module stays loaded for the process.
    unsafe {
        let module = LoadLibraryW(windows::core::PCWSTR(wide.as_ptr()));
        let Ok(module) = module else {
            log(&format!(
                "[afw] PDAFWPlugin.dll not found at {} (error {}) -- it comes with PureDark's UEVR AFW release\n",
                path.display(),
                windows::Win32::Foundation::GetLastError().0
            ));
            fail(g, c"no PDAFWPlugin.dll");
            return false;
        };
        g.module = module;
        g.init_device = GetProcAddress(module, windows::core::s!("InitDevice"))
            .map(|f| core::mem::transmute(f));
        g.init_warp = GetProcAddress(module, windows::core::s!("InitFrameWarp"))
            .map(|f| core::mem::transmute(f));
        g.evaluate = GetProcAddress(module, windows::core::s!("EvaluateFrameWarp"))
            .map(|f| core::mem::transmute(f));
        let (Some(init_device), Some(_), Some(_)) = (g.init_device, g.init_warp, g.evaluate) else {
            fail(
                g,
                c"PDAFWPlugin.dll lacks InitDevice / InitFrameWarp / EvaluateFrameWarp",
            );
            return false;
        };
        // The adapter the game's device runs on.
        let mut adapter: Option<IDXGIAdapter1> = None;
        if let Ok(factory) = CreateDXGIFactory2::<IDXGIFactory4>(DXGI_CREATE_FACTORY_FLAGS(0)) {
            if let Some(d) = ID3D12Device::from_raw_borrowed(&device) {
                adapter = factory
                    .EnumAdapterByLuid::<IDXGIAdapter1>(d.GetAdapterLuid())
                    .ok();
            }
        }
        let params = DeviceParams {
            d3d11_device: core::ptr::null_mut(),
            d3d11_context: core::ptr::null_mut(),
            d3d12_device: device,
            d3d12_queue: queue,
            dxgi_adapter: adapter
                .as_ref()
                .map_or(core::ptr::null_mut(), |a| a.as_raw()),
        };
        g.api = init_device(params);
        if g.api.is_null() {
            fail(g, c"InitDevice failed");
            return false;
        }
        // The plugin keeps the adapter.
        core::mem::forget(adapter);
        g.device = device;
        stats(2).store(1, Relaxed);
        log(&format!(
            "[afw] PDAFWPlugin.dll loaded from {}\n",
            path.display()
        ));
        true
    }
}

trait EncodeWideNul {
    fn encode_wide_nul(&self) -> Vec<u16>;
}
impl EncodeWideNul for std::ffi::OsStr {
    fn encode_wide_nul(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain(core::iter::once(0)).collect()
    }
}

unsafe fn ensure_eyes(g: &mut Plugin, width: u32, height: u32) -> bool {
    if g.width == width
        && g.height == height
        && !g.eyes.eye_frame_buffers[0].color.texture.is_null()
    {
        return true;
    }
    let Some(init_warp) = g.init_warp else {
        return false;
    };
    let params = FrameWarpInitParams {
        hmd_width: width as i32,
        hmd_height: height as i32,
        eye_format: DXGI_FORMAT_R8G8B8A8_UNORM,
        backbuffer_format: DXGI_FORMAT_R8G8B8A8_UNORM,
    };
    // SAFETY: the plugin's export, loaded above.
    g.eyes = unsafe { init_warp(params) };
    if g.eyes.eye_frame_buffers[0].color.texture.is_null()
        || g.eyes.eye_frame_buffers[1].color.texture.is_null()
    {
        fail(g, c"InitFrameWarp gave no eye buffers");
        return false;
    }
    g.width = width;
    g.height = height;
    log(&format!("[afw] eye buffers {width}x{height}\n"));
    true
}

fn typed(f: DXGI_FORMAT) -> DXGI_FORMAT {
    match f {
        DXGI_FORMAT_R32_TYPELESS => DXGI_FORMAT_R32_FLOAT,
        DXGI_FORMAT_R16G16_TYPELESS => DXGI_FORMAT_R16G16_FLOAT,
        _ => f,
    }
}

/// `textures` (one per eye) the size and typed format of `source`: kept when they are, else made again.
unsafe fn ensure_input(
    api: *mut RendererApi,
    textures: &mut [TextureDesc; 2],
    w: &mut u32,
    h: &mut u32,
    format: &mut DXGI_FORMAT,
    source: &ID3D12Resource,
) -> bool {
    // SAFETY: the caller's live resource and the plugin's renderer.
    unsafe {
        let desc = source.GetDesc();
        let want = typed(desc.Format);
        if !textures[0].texture.is_null()
            && *w == desc.Width as u32
            && *h == desc.Height
            && *format == want
        {
            return true;
        }
        for t in textures.iter_mut() {
            if !t.texture.is_null() {
                drop(windows::core::IUnknown::from_raw(t.texture));
            }
            *t = TextureDesc::default();
            if !((*(*api).vtbl).create_texture)(
                api,
                desc.Width as i32,
                desc.Height as i32,
                want,
                D3D12_RESOURCE_STATE_ALL_SHADER_RESOURCE,
                t,
                true,
            ) {
                return false;
            }
        }
        *w = desc.Width as u32;
        *h = desc.Height;
        *format = want;
        true
    }
}

fn borrowed(r: *mut c_void) -> core::mem::ManuallyDrop<Option<ID3D12Resource>> {
    // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
    unsafe {
        core::mem::transmute::<*mut c_void, core::mem::ManuallyDrop<Option<ID3D12Resource>>>(r)
    }
}

unsafe fn barrier(
    list: &ID3D12GraphicsCommandList,
    r: *mut c_void,
    a: D3D12_RESOURCE_STATES,
    b: D3D12_RESOURCE_STATES,
) {
    if a == b || r.is_null() {
        return;
    }
    let v = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: borrowed(r),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: a,
                StateAfter: b,
            }),
        },
    };
    // SAFETY: the caller's recording list.
    unsafe { list.ResourceBarrier(&[v]) };
}

unsafe fn copy_region(list: &ID3D12GraphicsCommandList, dst: *mut c_void, src: *mut c_void) {
    // SAFETY: live resources of the caller's and the plugin's.
    unsafe {
        let (Some(d), Some(s)) = (
            ID3D12Resource::from_raw_borrowed(&dst),
            ID3D12Resource::from_raw_borrowed(&src),
        ) else {
            return;
        };
        let (dd, sd) = (d.GetDesc(), s.GetDesc());
        let to = D3D12_TEXTURE_COPY_LOCATION {
            pResource: borrowed(dst),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: 0,
            },
        };
        let from = D3D12_TEXTURE_COPY_LOCATION {
            pResource: borrowed(src),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: 0,
            },
        };
        let b = D3D12_BOX {
            left: 0,
            top: 0,
            front: 0,
            right: dd.Width.min(sd.Width) as u32,
            bottom: dd.Height.min(sd.Height),
            back: 1,
        };
        list.CopyTextureRegion(&to, 0, 0, 0, &from, Some(&b));
    }
}

// ---- the C API (include/Render/Afw.hpp, forwarded by Afw.cpp) ---------------------------------------------------

/// The eyes take turns this launch: xr_afw or xr_osr as engine stereo read them.
#[no_mangle]
pub extern "C" fn cvr_afw_set_launch(on: bool) {
    LAUNCH.store(on, Relaxed);
}
#[no_mangle]
pub extern "C" fn cvr_afw_launch() -> bool {
    LAUNCH.load(Relaxed)
}
/// Without the plugin the port's own reconstruction makes the other eye: the launch takes turns either way.
#[no_mangle]
pub extern "C" fn cvr_afw_active() -> bool {
    LAUNCH.load(Relaxed)
}
/// The other eye comes from OSR now: the OSR mode, or AFW without its plugin.
#[no_mangle]
pub extern "C" fn cvr_afw_uses_osr() -> bool {
    tune(6) != 0 || FAILED.load(Relaxed)
}

/// The eye MAIN draws this frame: it turns over whenever the renderer's clock has moved since the last ask -- every new
/// frame the camera is placed for, whatever the clock's step. Asked on the game thread only (Hooks/PatchCamera.cpp),
/// as often as MAIN's camera is written in one frame: all of those writes get the same eye.
#[no_mangle]
pub extern "C" fn cvr_afw_frame_eye() -> i32 {
    static CLOCK: AtomicU32 = AtomicU32::new(0);
    static EYE: AtomicI32 = AtomicI32::new(1);
    let mut clock = 0u32;
    // SAFETY: a plain query of the renderer's clock.
    if unsafe { crate::framegen_inputs::cvr_framegen_read_render_frame_index(&mut clock) }
        && clock != CLOCK.load(Relaxed)
    {
        CLOCK.store(clock, Relaxed);
        EYE.store(EYE.load(Relaxed) ^ 1, Relaxed);
    }
    EYE.load(Relaxed)
}

/// Diagnostics: frames warped, frames that fell back, the last failure.
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_afw_statistics(out: *mut Stats) {
    let s = lock().stats;
    // SAFETY: the caller's slot.
    if let Some(out) = unsafe { out.as_mut() } {
        *out = s;
    }
}

/// Within a launch that takes turns, a change between the two applies at once.
#[no_mangle]
pub extern "C" fn cvr_afw_set_mode(mode: i32) {
    if mode != 0 {
        // SAFETY: an export of the C++ half.
        unsafe { CyberpunkVR_AfwTune[6].store(if mode == 2 { 1 } else { 0 }, Relaxed) };
    }
}

/// A frame the capture could not hand to Warp: counted with Warp's own fallbacks. `why` is a static string.
#[no_mangle]
pub extern "C" fn cvr_afw_fallback(why: *const c_char) {
    fallback(&mut lock(), why);
}

/// cvr::afw::WarpRequest.
#[repr(C)]
pub struct WarpRequest {
    pub device: *mut c_void,
    pub queue: *mut c_void,
    pub list: *mut c_void,
    /// The presented picture (the drawn eye), in PRESENT state.
    pub drawn: *mut c_void,
    /// Its depth and motion, and the camera it was drawn with.
    pub inputs: *const Inputs,
    pub eye: u32,
    /// Each eye's window as submitted (XrFovf: left, right, up, down).
    pub fov: [[f32; 4]; 2],
    pub ipd: f32,
    /// The second eye's slot, in COPY_SOURCE state.
    pub out: *mut c_void,
}
const _: () = assert!(
    core::mem::size_of::<WarpRequest>() == 88 && core::mem::offset_of!(WarpRequest, out) == 80
);

/// Records the other eye into `out`; false (nothing recorded) when the plugin is not there or the inputs do not fit.
///
/// # Safety
/// `request` is null or a WarpRequest whose objects are the caller's live ones.
#[no_mangle]
pub unsafe extern "C" fn cvr_afw_warp(request: *const WarpRequest) -> bool {
    let mut g = lock();
    // SAFETY: the caller's request and objects.
    match unsafe { warp(&mut g, request.as_ref()) } {
        Ok(()) => true,
        Err(why) => {
            fallback(&mut g, why.as_ptr());
            false
        }
    }
}

unsafe fn warp(g: &mut Plugin, q: Option<&WarpRequest>) -> Result<(), &'static CStr> {
    let Some(q) = q.filter(|q| {
        cvr_afw_active()
            && !q.device.is_null()
            && !q.queue.is_null()
            && !q.list.is_null()
            && !q.drawn.is_null()
            && !q.inputs.is_null()
            && !q.out.is_null()
            && q.eye <= 1
    }) else {
        return Err(c"no request");
    };
    // SAFETY: the caller's inputs.
    let input = unsafe { &*q.inputs };
    let (Some(depth), Some(motion)) = (input.depth.as_ref(), input.motion.as_ref()) else {
        return Err(c"no depth or motion");
    };
    if !input.has_depth || !input.has_motion {
        return Err(c"no depth or motion");
    }
    // The inputs were copied on the game's queue ahead of this list: same-queue order is their readiness, as for the
    // depth layer (Render/StereoDepth.cpp). No wait on another queue is introduced here.
    if input.queue != q.queue || !input.submitted {
        return Err(c"inputs on another queue or not submitted");
    }
    let cam = &input.camera;
    let n = q.eye as usize;
    let other = n ^ 1;
    let side: f32 = if n == 0 { 1.0 } else { -1.0 }; // from the left eye the right one is to the right
    let other_pos = [
        cam.position[0] + side * q.ipd * cam.right[0],
        cam.position[1] + side * q.ipd * cam.right[1],
        cam.position[2] + side * q.ipd * cam.right[2],
    ];
    if cvr_afw_uses_osr() {
        let camera = |position: [f32; 3], fov: [f32; 4]| cvr_gpu::osr::Camera {
            position,
            right: cam.right,
            up: cam.up,
            forward: cam.forward,
            fov,
            near_plane: cam.near_plane,
            far_plane: cam.far_plane,
            inverted: cam.inverted,
            infinite: cam.infinite,
            jitter: cam.jitter,
        };
        let r = cvr_gpu::osr::Request {
            device: q.device,
            list: q.list,
            drawn: q.drawn,
            depth: depth.as_raw(),
            source: camera(cam.position, q.fov[n]),
            dest: camera(other_pos, q.fov[other]),
            eye: n as u32,
            out: q.out,
            history: tune(0) != 1, // "alternate eye" mode: the stereo warp alone
            debug: if tune(3) != 0 { 1 } else { 0 },
            jitter_sign: match tune(7) {
                1 => 1.0,
                2 => 0.0,
                _ => -1.0,
            },
        };
        // SAFETY: the caller's live objects; the C++ half's exports.
        if !unsafe {
            cvr_gpu::osr::cvr_gpu_osr_record(
                &r,
                &raw const CyberpunkVR_OsrStats,
                &raw const CyberpunkVR_OsrTune,
            )
        } {
            return Err(c"OSR could not record");
        }
        g.stats.warped += 1;
        stats(0).fetch_add(1, Relaxed);
        return Ok(());
    }
    // SAFETY: the caller's live objects; the plugin's renderer once loaded.
    unsafe {
        if !load(g, q.device, q.queue) {
            return Err(c"no plugin");
        }
        let Some(out) = ID3D12Resource::from_raw_borrowed(&q.out) else {
            return Err(c"no request");
        };
        let out_desc = out.GetDesc();
        if !ensure_eyes(g, out_desc.Width as u32, out_desc.Height) {
            return Err(c"no eye buffers");
        }
        let api = g.api;
        let Plugin {
            depth: gd,
            depth_w,
            depth_h,
            depth_format,
            motion: gm,
            motion_w,
            motion_h,
            motion_format,
            ..
        } = g;
        if !ensure_input(api, gd, depth_w, depth_h, depth_format, depth)
            || !ensure_input(api, gm, motion_w, motion_h, motion_format, motion)
        {
            return Err(c"no input copies");
        }
        let color = g.eyes.eye_frame_buffers[n].color;
        let made = g.eyes.eye_frame_buffers[other].color;
        let Some(list) = ID3D12GraphicsCommandList::from_raw_borrowed(&q.list) else {
            return Err(c"no request");
        };

        // the drawn eye, its depth and motion, into the plugin's buffers
        barrier(
            list,
            q.drawn,
            D3D12_RESOURCE_STATE_PRESENT,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
        );
        barrier(
            list,
            color.texture,
            color.initial_state,
            D3D12_RESOURCE_STATE_COPY_DEST,
        );
        copy_region(list, color.texture, q.drawn);
        barrier(
            list,
            color.texture,
            D3D12_RESOURCE_STATE_COPY_DEST,
            color.initial_state,
        );
        barrier(
            list,
            q.drawn,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
            D3D12_RESOURCE_STATE_PRESENT,
        );
        let inputs = [(depth.as_raw(), g.depth[n]), (motion.as_raw(), g.motion[n])];
        for (source, target) in inputs {
            barrier(
                list,
                source,
                D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            );
            barrier(
                list,
                target.texture,
                target.initial_state,
                D3D12_RESOURCE_STATE_COPY_DEST,
            );
            if let (Some(t), Some(s)) = (
                ID3D12Resource::from_raw_borrowed(&target.texture),
                ID3D12Resource::from_raw_borrowed(&source),
            ) {
                list.CopyResource(t, s);
            }
            barrier(
                list,
                target.texture,
                D3D12_RESOURCE_STATE_COPY_DEST,
                target.initial_state,
            );
            barrier(
                list,
                source,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
                D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
            );
        }

        // the two cameras: the drawn eye where it was drawn, the other one IPD along the camera's right
        let sv = world_to_view(&cam.position, cam);
        let dv = world_to_view(&other_pos, cam);
        let sp = view_to_clip(&q.fov[n], cam);
        let dp = view_to_clip(&q.fov[other], cam);
        let camera = &mut g.cameras[n];
        camera.src_world_to_view = to_mat(&sv);
        camera.src_view_to_world = to_mat(&inverse(&sv));
        camera.src_view_to_clip = to_mat(&sp);
        camera.src_clip_to_view = to_mat(&inverse(&sp));
        camera.dest_world_to_view = to_mat(&dv);
        camera.dest_view_to_world = to_mat(&inverse(&dv));
        camera.dest_view_to_clip = to_mat(&dp);
        camera.dest_clip_to_view = to_mat(&inverse(&dp));
        let mut id = M4::default();
        for i in 0..4 {
            id.set(i, i, 1.0);
        }
        let id = to_mat(&id);
        camera.cam_world_to_view = id;
        camera.cam_view_to_world = id;
        camera.cam_view_to_clip = id;
        camera.cam_clip_to_view = id;

        g.frame.color = color;
        g.frame.depth = g.depth[n];
        g.frame.motion_vectors = g.motion[n];
        let mut p = FrameWarpEvaluateParams {
            in_cmd_list: q.list,
            in_eye_frame_buffer: &raw mut g.frame,
            is_hudless_color: true,
            ignore_motion_threshold: tune(2) as f32 / 100.0,
            debug: tune(3) != 0,
            clear_before_warping: tune(4) != 0,
            ..Default::default()
        };
        // THE ENGINE'S MOTION VECTORS POINT AT ITS PREVIOUS FRAME, whichever eye that was drawn for: the other eye when
        // the eyes took turns (FromOtherEye), this eye when the turn did not come round -- two camera placements under
        // one move of the renderer's clock (FrameEye). The frame before is known when it was warped too.
        let follows = g.last_eye >= 0 && input.frame_id == g.last_frame.wrapping_add(1);
        let same_eye = follows && g.last_eye as usize == n;
        stats(if follows {
            if same_eye {
                4
            } else {
                3
            }
        } else {
            5
        })
        .fetch_add(1, Relaxed);
        g.last_frame = input.frame_id;
        g.last_eye = n as i32;
        let mv_type = tune(1);
        p.motion_vectors_type = if mv_type == 3 {
            if same_eye {
                MV_NORMAL
            } else {
                MV_FROM_OTHER_EYE
            }
        } else if (0..=2).contains(&mv_type) {
            mv_type
        } else {
            1
        };
        let mv_on = if tune(5) != 0 { 0.0f32 } else { 1.0 };
        p.in_motion_scale[0] =
            mv_on * motion_scale(0) * cam.motion_scale[0] * out_desc.Width as f32;
        p.in_motion_scale[1] =
            mv_on * motion_scale(1) * cam.motion_scale[1] * out_desc.Height as f32;
        let mode = tune(0);
        p.mode = if (1..=3).contains(&mode) { mode } else { 3 };
        p.eye_index = n as i32;
        p.camera_data = &raw mut g.cameras[n];
        if let Some(evaluate) = g.evaluate {
            evaluate(&mut p);
        }

        // the made eye into the second eye's slot
        barrier(
            list,
            made.texture,
            made.initial_state,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
        );
        barrier(
            list,
            q.out,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
            D3D12_RESOURCE_STATE_COPY_DEST,
        );
        copy_region(list, q.out, made.texture);
        barrier(
            list,
            q.out,
            D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
        );
        barrier(
            list,
            made.texture,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
            made.initial_state,
        );
        g.stats.warped += 1;
        stats(0).fetch_add(1, Relaxed);
        if g.stats.warped == 1 || g.stats.warped % 5000 == 0 {
            log(&format(
                c"[afw] warped %llu (eye %u made from %u; colour %ux%u, depth %ux%u fmt %d, motion %ux%u fmt %d, scale %.4f %.4f)\n",
                &[
                    Arg::U(g.stats.warped),
                    Arg::U(other as u64),
                    Arg::U(n as u64),
                    Arg::U(u64::from(out_desc.Width as u32)),
                    Arg::U(u64::from(out_desc.Height)),
                    Arg::U(u64::from(g.depth_w)),
                    Arg::U(u64::from(g.depth_h)),
                    Arg::I(g.depth_format.0),
                    Arg::U(u64::from(g.motion_w)),
                    Arg::U(u64::from(g.motion_h)),
                    Arg::I(g.motion_format.0),
                    Arg::F(f64::from(cam.motion_scale[0])),
                    Arg::F(f64::from(cam.motion_scale[1])),
                ],
            )
            .to_string_lossy());
        }
        Ok(())
    }
}
