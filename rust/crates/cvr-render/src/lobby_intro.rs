//! THE LOBBY'S INTRO (formerly src/Lobby/Intro.cpp's picture state, src/Render/IntroOverlay.cpp and the FreeLookHeading of
//! include/Render/SpaceQuad.hpp): the picture the intro's script gives (a WorldWidgetComponent's surface, leased from the
//! engine), the heading it hangs at, and its drawing into the captured frame -- the lobby's curtain over each eye's
//! image, and the intro's picture seen through that eye.
//!
//! THE INTRO'S HEADING, the notice's and the logo's: the logo comes up where the notice was. The head looks about within
//! 90 degrees of it and the pictures stay where they are; turned farther, they are brought round after it. Pinned to the
//! gaze, the intro moved with every turn of the head; hung once, it was lost behind a head turned away. Capture thread.
use core::ffi::c_void;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::Instant;

use cvr_gpu::space_quad::{Blend, Fov, Pose, SpaceQuad, SpaceQuads, Target};
use openxr_sys as xr;
use windows::core::Interface as _;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::{
    ID3D12Device, ID3D12GraphicsCommandList, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_TYPELESS, DXGI_FORMAT_B8G8R8A8_UNORM,
    DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_R10G10B10A2_TYPELESS,
    DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT,
    DXGI_FORMAT_R16G16B16A16_TYPELESS, DXGI_FORMAT_R8G8B8A8_TYPELESS, DXGI_FORMAT_R8G8B8A8_UNORM,
    DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
};

extern "C" {
    fn cvr_menu_surface_texture(component: *mut c_void) -> *mut c_void;
}
extern "system" {
    fn GetTickCount64() -> u64;
}
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ---- the heading

/// The intro's free-look cone (rad) either side of its heading.
pub const FREE_LOOK_CONE: f32 = 1.5707963;

/// A head pose that was located (SpaceQuad.hpp LocatedPose): the first frames of a session can be presented before any
/// locate, with the zero pose.
fn located_pose(pose: &xr::Posef) -> bool {
    let q = &pose.orientation;
    let length = q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
    if !(0.5..=1.5).contains(&length) {
        return false;
    }
    let p = &pose.position;
    !(p.x == 0.0 && p.y == 0.0 && p.z == 0.0 && q.x == 0.0 && q.y == 0.0 && q.z == 0.0)
}

/// A HEADING THE HEAD LOOKS ABOUT FROM: level, and still while the head turns up to `cone` either side of it; a head
/// turned farther has it caught up, the menus' way (cvr-body hud.rs Follow), until it is ahead again. Set by the first
/// located head.
pub struct FreeLookHeading {
    cone: f32,
    follow: cvr_body::hud::Follow,
    last: Option<Instant>,
}
impl FreeLookHeading {
    pub fn new(cone: f32) -> Self {
        Self {
            cone,
            follow: cvr_body::hud::Follow::default(),
            last: None,
        }
    }
    /// A frame with `head`.
    pub fn update(&mut self, head: &xr::Posef) {
        let now = Instant::now();
        let dt = self
            .last
            .map_or(0.0, |last| now.duration_since(last).as_secs_f32());
        self.last = Some(now);
        if !located_pose(head) {
            return;
        }
        let q = &head.orientation;
        let yaw = cvr_body::hud::cvr_hud_head_yaw(q.x, q.y, q.z, q.w, self.follow.yaw);
        self.follow.update(yaw, self.cone, dt, false);
    }
    pub fn valid(&self) -> bool {
        self.follow.valid
    }
    pub fn orientation(&self) -> xr::Quaternionf {
        let half = self.follow.yaw * 0.5;
        xr::Quaternionf {
            x: 0.0,
            y: half.sin(),
            z: 0.0,
            w: half.cos(),
        }
    }
}
static HEADING: LazyLock<Mutex<FreeLookHeading>> =
    LazyLock::new(|| Mutex::new(FreeLookHeading::new(FREE_LOOK_CONE)));

/// The intro's heading kept up by a frame's head.
///
/// # Safety
/// `head` is valid or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_intro_heading_update(head: *const xr::Posef) {
    // SAFETY: the caller's pose.
    if let Some(head) = unsafe { head.as_ref() } {
        lock(&HEADING).update(head);
    }
}
/// Whether the intro's heading is set, and (into `out`, when not null) its orientation.
///
/// # Safety
/// `out` is valid or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_intro_heading(out: *mut xr::Quaternionf) -> bool {
    let h = lock(&HEADING);
    // SAFETY: the caller's out-parameter.
    if let Some(out) = unsafe { out.as_mut() } {
        *out = h.orientation();
    }
    h.valid()
}

// ---- the picture

/// The picture the intro's script gave: its surface leased from the engine, where it hangs and how it shows.
pub struct Picture {
    handle: u32,
    texture: ID3D12Resource,
    distance: f32,
    height: f32,
    opacity: f32,
    /// Linear light a texel must exceed to show: a bink's dark backdrop is clear air too.
    black_point: f32,
    /// One per intro: a new one is logged as its first picture comes.
    intro: u64,
}
impl Drop for Picture {
    fn drop(&mut self) {
        cvr_gpu::texture_lease::cvr_hud_texture_release(self.handle);
    }
}
struct Pictures {
    picture: Option<Arc<Picture>>,
    tick: u64,
    intro: u64,
    running: bool,
}
static PICTURES: Mutex<Pictures> = Mutex::new(Pictures {
    picture: None,
    tick: 0,
    intro: 0,
    running: false,
});
/// A picture not renewed for this long (ms) is the intro's end.
const STALE_MS: u64 = 2000;

/// The script's picture: `widget` a WorldWidgetComponent (null: the intro ended). False when it has none to show.
pub fn set(
    widget: *mut c_void,
    distance: f32,
    height: f32,
    opacity: f32,
    black_point: f32,
) -> bool {
    if widget.is_null() {
        let mut p = lock(&PICTURES);
        p.picture = None;
        p.running = false;
        return false;
    }
    // SAFETY: the engine's component, its surface.
    let texture = unsafe { cvr_menu_surface_texture(widget) };
    let (mut handle, mut resource) = (0u32, core::ptr::null_mut());
    // SAFETY: the surface's DynamicTexture or null; out-parameters.
    if !unsafe {
        cvr_gpu::texture_lease::cvr_hud_texture_lease(texture, true, &mut handle, &mut resource)
    } {
        return false;
    }
    // SAFETY: the lease's own reference.
    let texture = unsafe { ID3D12Resource::from_raw(resource) };
    let mut picture = Picture {
        handle,
        texture,
        distance: distance.clamp(0.3, 50.0),
        height: height.clamp(0.05, 20.0),
        opacity: opacity.clamp(0.0, 1.0),
        black_point: black_point.clamp(0.0, 0.9),
        intro: 0,
    };
    // SAFETY: a plain Win32 call.
    let now = unsafe { GetTickCount64() };
    let mut p = lock(&PICTURES);
    if !p.running || now.wrapping_sub(p.tick) > STALE_MS {
        p.intro += 1;
    }
    p.running = true;
    picture.intro = p.intro;
    p.picture = Some(Arc::new(picture));
    p.tick = now;
    true
}
/// The picture the script gave last; none once the intro has ended, or the script has given none for 2 s.
pub fn latest() -> Option<Arc<Picture>> {
    let mut p = lock(&PICTURES);
    // SAFETY: a plain Win32 call.
    if p.picture.is_some() && unsafe { GetTickCount64() }.wrapping_sub(p.tick) > STALE_MS {
        p.picture = None;
        p.running = false;
    }
    p.picture.clone()
}

/// The script's picture (VRLobbyIntro): `widget` a WorldWidgetComponent, or null.
#[no_mangle]
pub extern "C" fn cvr_intro_set(
    widget: *mut c_void,
    distance: f32,
    height: f32,
    opacity: f32,
    black_point: f32,
) -> bool {
    set(widget, distance, height, opacity, black_point)
}
/// The intro is running: a picture given within the last 2 s.
#[no_mangle]
pub extern "C" fn cvr_intro_running() -> bool {
    latest().is_some()
}

// ---- the overlay

/// The view an eye's image is written through: its sRGB form, so the picture's linear light is encoded on write.
fn linear_view(format: DXGI_FORMAT) -> DXGI_FORMAT {
    match format {
        DXGI_FORMAT_R8G8B8A8_UNORM | DXGI_FORMAT_R8G8B8A8_TYPELESS => {
            DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
        }
        DXGI_FORMAT_B8G8R8A8_UNORM | DXGI_FORMAT_B8G8R8A8_TYPELESS => {
            DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
        }
        DXGI_FORMAT_R10G10B10A2_TYPELESS => DXGI_FORMAT_R10G10B10A2_UNORM,
        DXGI_FORMAT_R16G16B16A16_TYPELESS => DXGI_FORMAT_R16G16B16A16_FLOAT,
        other => other,
    }
}
/// SpaceQuad.hpp Ahead: `distance` in front of the eye along `heading`, facing back along it.
fn ahead(eye: &xr::Posef, heading: &xr::Quaternionf, distance: f32) -> Pose {
    use cvr_body::xr::{rotate_vector, Quat, Vec3};
    let q = Quat {
        x: heading.x,
        y: heading.y,
        z: heading.z,
        w: heading.w,
    };
    let a = rotate_vector(
        q,
        Vec3 {
            x: 0.0,
            y: 0.0,
            z: -distance,
        },
    );
    Pose {
        orientation: cvr_gpu::space_quad::Quat {
            x: heading.x,
            y: heading.y,
            z: heading.z,
            w: heading.w,
        },
        position: cvr_gpu::space_quad::Vec3 {
            x: eye.position.x + a.x,
            y: eye.position.y + a.y,
            z: eye.position.z + a.z,
        },
    }
}
fn pose_of(p: &xr::Posef) -> Pose {
    Pose {
        orientation: cvr_gpu::space_quad::Quat {
            x: p.orientation.x,
            y: p.orientation.y,
            z: p.orientation.z,
            w: p.orientation.w,
        },
        position: cvr_gpu::space_quad::Vec3 {
            x: p.position.x,
            y: p.position.y,
            z: p.position.z,
        },
    }
}

struct Overlay {
    quads: SpaceQuads,
    /// The picture per capture slot: alive until that slot's work is done.
    held: [Option<Arc<Picture>>; 3],
    shown: Option<Arc<Picture>>,
    curtain: f32,
    /// The intro last logged.
    logged_intro: u64,
}
// SAFETY: the capture thread's own drawer and pictures, used under the lock.
unsafe impl Send for Overlay {}
static OVERLAY: LazyLock<Mutex<Overlay>> = LazyLock::new(|| {
    Mutex::new(Overlay {
        quads: SpaceQuads::default(),
        held: [None, None, None],
        shown: None,
        curtain: 0.0,
        logged_intro: 0,
    })
});

/// What this frame has to draw: the curtain's closure (0 open .. 1 shut) and the intro's picture. `head` is the frame's
/// head pose in the tracking space: it keeps the intro's heading up. False when there is nothing to draw.
///
/// # Safety
/// `head` is valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_intro_overlay_begin(curtain: f32, head: *const xr::Posef) -> bool {
    // SAFETY: the caller's pose.
    let Some(head) = (unsafe { head.as_ref() }) else {
        return false;
    };
    let mut o = lock(&OVERLAY);
    o.curtain = curtain.clamp(0.0, 1.0);
    o.shown = latest();
    let picture = o.shown.as_ref().is_some_and(|p| p.opacity > 0.0);
    let mut heading = lock(&HEADING);
    heading.update(head);
    if let (true, true, Some(shown)) = (picture, heading.valid(), o.shown.clone()) {
        if o.logged_intro != shown.intro {
            o.logged_intro = shown.intro;
            // SAFETY: a live resource.
            let desc = unsafe { shown.texture.GetDesc() };
            cvr_host::log::write(&format!(
                "[lobby-intro] intro {}: picture {}x{} format {}, drawn into the frame at the intro's heading, eyes at ({:.2}, {:.2}, {:.2})\n",
                shown.intro, desc.Width, desc.Height, desc.Format.0 as u32, head.position.x, head.position.y, head.position.z
            ));
        }
    }
    o.curtain > 0.0 || picture
}

/// One eye: `target` its image, a render target now, holding sRGB-encoded colour; `pose` and `fov` the eye it was
/// rendered for. `picture` false draws the curtain alone (one picture shared by both eyes: a menu's window). Recorded on
/// `list`, which runs before the frame is read; the picture is kept until `slot` comes round again.
///
/// # Safety
/// The device and list are live; `target`, `rect`, `pose` and `fov` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_intro_overlay_record(
    device: *mut c_void,
    list: *mut c_void,
    target: *mut c_void,
    rect: *const RECT,
    pose: *const xr::Posef,
    fov: *const xr::Fovf,
    picture: bool,
    slot: u32,
) -> bool {
    // SAFETY: the caller's objects.
    let (Some(resource), Some(rect), Some(pose), Some(fov)) = (
        unsafe { ID3D12Resource::from_raw_borrowed(&target) },
        unsafe { rect.as_ref() },
        unsafe { pose.as_ref() },
        unsafe { fov.as_ref() },
    ) else {
        return false;
    };
    // SAFETY: the caller's live device and list, borrowed.
    let (device, list) = unsafe {
        (
            ID3D12Device::from_raw_borrowed(&device),
            ID3D12GraphicsCommandList::from_raw_borrowed(&list),
        )
    };
    // SAFETY: a live resource.
    let view = linear_view(unsafe { resource.GetDesc() }.Format);
    let eye = Target {
        resource: target,
        view: view.0,
        rect: *rect,
        mip: 0,
    };
    let mut guard = lock(&OVERLAY);
    let o = &mut *guard;
    let mut ok = true;
    if o.curtain > 0.0 {
        ok &= o
            .quads
            .fill(device, list, &eye, &[0.0, 0.0, 0.0, o.curtain]);
    }
    let heading = lock(&HEADING);
    if let (true, Some(shown), true) = (picture, o.shown.clone(), heading.valid()) {
        if shown.opacity > 0.0 {
            // SAFETY: a live resource.
            let desc = unsafe { shown.texture.GetDesc() };
            let quad = SpaceQuad {
                pose: ahead(pose, &heading.orientation(), shown.distance),
                width: shown.height * desc.Width as f32 / desc.Height.max(1) as f32,
                height: shown.height,
                uv: [0.0, 0.0, 1.0, 1.0],
            };
            let eye_fov = Fov {
                angle_left: fov.angle_left,
                angle_right: fov.angle_right,
                angle_up: fov.angle_up,
                angle_down: fov.angle_down,
            };
            ok &= o.quads.draw(
                device,
                list,
                &eye,
                &pose_of(pose),
                &eye_fov,
                Some(&shown.texture),
                linear_view(desc.Format).0,
                &quad,
                Blend::Light,
                shown.opacity,
                shown.black_point,
            );
            o.held[slot as usize % 3] = Some(shown);
        }
    }
    ok
}

/// The overlay's GPU objects and pictures released.
#[no_mangle]
pub extern "C" fn cvr_intro_overlay_shutdown() {
    let mut o = lock(&OVERLAY);
    o.quads.shutdown();
    o.held = [None, None, None];
    o.shown = None;
    o.logged_intro = 0;
}
