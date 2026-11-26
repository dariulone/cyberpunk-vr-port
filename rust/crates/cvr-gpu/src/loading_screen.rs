//! THE LOADING SCREEN, HUNG IN THE DARK (formerly src/Render/LoadingScreen.cpp; Render/LoadingScreen.hpp keeps the C++
//! face, its placement's heading and caption text).
//!
//! While the game loads, the eyes see the dark and, straight ahead of where the player looked as it came up, level at
//! the eyes' height, the screen the game shows in its window: a picture of the window's part made with all its mips
//! (the window's text and edges, about three times smaller on the eye than in the window, do not alias), fading in
//! from the first frame it holds. Capture thread only.
use core::ffi::c_void;

use cvr_body::cmath;
use cvr_host::log;
use windows::core::Interface;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::System::SystemInformation::GetTickCount64;

use crate::caption::{self, Caption, CaptionLine};
use crate::space_quad::{Blend, Fov, Pose, Quat, SpaceQuad, SpaceQuads, Target, Vec3};

/// cvr::loading::Placement as the C++ face hands it over: the distance, the height (or the width, when set), the fade
/// in, and whether there is a caption and a heading (the C++ face keeps those).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Placement {
    pub distance: f32,
    pub height: f32,
    pub width: f32,
    pub fade_in: f32,
    pub has_caption: bool,
    pub has_heading: bool,
}

impl Default for Placement {
    fn default() -> Self {
        Placement {
            distance: 3.0,
            height: 1.8,
            width: 0.0,
            fade_in: 0.4,
            has_caption: false,
            has_heading: false,
        }
    }
}

/// The caption's text for the screen's coming up (cvr_gpu_caption_set's arguments).
#[repr(C)]
pub struct CaptionText {
    pub lines: *const CaptionLine,
    pub count: u32,
    pub size: f32,
    pub light: f32,
    pub locale: *const u16,
    pub right_to_left: bool,
}

/// cvr::loading::Screen.
pub struct Screen {
    quads: SpaceQuads,
    caption: Caption,
    picture: Option<ID3D12Resource>,
    retired: Vec<ID3D12Resource>,
    picture_width: u32,
    picture_height: u32,
    levels: u32,
    picture_format: i32,
    at: Placement,
    anchor: Pose,
    first_frame: u64,
    up: bool,
    held: bool,
    fresh: bool,
    ready: bool,
    opacity: f32,
    shown: f32,
}

impl Default for Screen {
    fn default() -> Self {
        Screen {
            quads: SpaceQuads::default(),
            caption: Caption::default(),
            picture: None,
            retired: Vec::new(),
            picture_width: 0,
            picture_height: 0,
            levels: 0,
            picture_format: DXGI_FORMAT_UNKNOWN.0,
            at: Placement::default(),
            anchor: Pose {
                orientation: Quat {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 1.0,
                },
                position: Vec3::default(),
            },
            first_frame: 0,
            up: false,
            held: false,
            fresh: false,
            ready: false,
            opacity: 0.0,
            shown: 1.0,
        }
    }
}

/// cvr::render::LocatedPose: a pose a runtime located (a unit orientation, not the all-zero one).
fn located_pose(pose: &Pose) -> bool {
    let q = &pose.orientation;
    let length = q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
    if !(0.5..=1.5).contains(&length) {
        return false;
    }
    let p = &pose.position;
    !(p.x == 0.0 && p.y == 0.0 && p.z == 0.0 && q.x == 0.0 && q.y == 0.0 && q.z == 0.0)
}

fn multiply(a: &Quat, b: &Quat) -> Quat {
    Quat {
        x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
        y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
        z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
        w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
    }
}

/// Utils/XrMath.hpp RotateVector: q v q*.
fn rotate_vector(q: &Quat, v: Vec3) -> Vec3 {
    let pure = Quat {
        x: v.x,
        y: v.y,
        z: v.z,
        w: 0.0,
    };
    let conjugate = Quat {
        x: -q.x,
        y: -q.y,
        z: -q.z,
        w: q.w,
    };
    let r = multiply(&multiply(q, &pure), &conjugate);
    Vec3 {
        x: r.x,
        y: r.y,
        z: r.z,
    }
}

/// cvr::render::Ahead: `distance` ahead of the eye along the heading, facing back along it.
fn ahead(eye: &Pose, heading: &Quat, distance: f32) -> Pose {
    let a = rotate_vector(
        heading,
        Vec3 {
            x: 0.0,
            y: 0.0,
            z: -distance,
        },
    );
    Pose {
        orientation: *heading,
        position: Vec3 {
            x: eye.position.x + a.x,
            y: eye.position.y + a.y,
            z: eye.position.z + a.z,
        },
    }
}

fn transition(
    resource: &ID3D12Resource,
    subresource: u32,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                pResource: unsafe { core::mem::transmute_copy(resource) },
                Subresource: subresource,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

const WHOLE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

impl Screen {
    /// Begin: placed by a located head only (until there is one, the dark alone). True when it came up now.
    fn begin(
        &mut self,
        head: &Pose,
        at: &Placement,
        caption_text: Option<&CaptionText>,
        opacity: f32,
    ) -> bool {
        // SAFETY: a plain Win32 call.
        let now = unsafe { GetTickCount64() };
        let mut came_up = false;
        if !self.up && located_pose(head) {
            // Straight ahead of where the player looks as it comes up, level, at the eyes' height.
            let q = &head.orientation;
            let fx = -2.0 * (q.x * q.z + q.y * q.w);
            let fz = 2.0 * (q.x * q.x + q.y * q.y) - 1.0;
            let yaw = cmath::atan2(-fx, -fz);
            self.at = *at;
            if let (true, Some(text)) = (at.has_caption, caption_text) {
                // SAFETY: the caption is this screen's; the text the C++ face's, alive for the call.
                unsafe {
                    caption::cvr_gpu_caption_set(
                        &mut self.caption,
                        text.lines,
                        text.count,
                        text.size,
                        text.light,
                        text.locale,
                        text.right_to_left,
                    )
                };
            }
            self.anchor.orientation = Quat {
                x: 0.0,
                y: cmath::sin(yaw * 0.5),
                z: 0.0,
                w: cmath::cos(yaw * 0.5),
            };
            self.anchor.position = Vec3 {
                x: head.position.x - cmath::sin(yaw) * at.distance,
                y: head.position.y,
                z: head.position.z - cmath::cos(yaw) * at.distance,
            };
            self.up = true;
            self.held = false;
            came_up = true;
            log!(
                "[loading-screen] up: {} {:.1} m ahead at yaw {:.0} deg, eyes at ({:.2}, {:.2}, {:.2})\n",
                if at.has_heading { "at the intro's heading" } else { "hung" },
                at.distance as f64,
                (yaw * 57.29578) as f64,
                head.position.x as f64,
                head.position.y as f64,
                head.position.z as f64
            );
        }
        let age = if self.held {
            now.wrapping_sub(self.first_frame) as f32 / 1000.0
        } else {
            0.0
        };
        let k = cmath::clamp(age / cmath::max(self.at.fade_in, 0.01), 0.0, 1.0);
        self.opacity = k * k * (3.0 - 2.0 * k);
        self.shown = cmath::clamp(opacity, 0.0, 1.0);
        came_up
    }

    fn hold(&mut self) {
        if !self.held {
            // SAFETY: a plain Win32 call.
            self.first_frame = unsafe { GetTickCount64() };
        }
        self.held = true;
        self.fresh = true;
    }

    fn ensure_picture(
        &mut self,
        device: &ID3D12Device,
        width: u32,
        height: u32,
        format: i32,
    ) -> bool {
        if self.picture.is_some()
            && width == self.picture_width
            && height == self.picture_height
            && format == self.picture_format
        {
            return true;
        }
        // A picture of another size: the old one may still be read by a capture list in flight, so it is kept till the
        // end.
        if let Some(old) = self.picture.take() {
            self.retired.push(old);
        }
        let mut levels = 1u32;
        while (width.max(height) >> levels) != 0 {
            levels += 1;
        }
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: levels as u16,
            Format: DXGI_FORMAT(format),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
            ..Default::default()
        };
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };
        let mut picture: Option<ID3D12Resource> = None;
        // SAFETY: the device's own call with descriptions made here.
        let made = unsafe {
            device.CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &desc,
                D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                None,
                &mut picture,
            )
        }
        .is_ok();
        let Some(picture) = picture.filter(|_| made) else {
            log!(
                "[loading-screen] picture {}x{} ({} levels) not made\n",
                width,
                height,
                levels
            );
            return false;
        };
        self.picture = Some(picture);
        self.picture_width = width;
        self.picture_height = height;
        self.picture_format = format;
        self.levels = levels;
        log!(
            "[loading-screen] picture {}x{}, {} levels\n",
            width,
            height,
            levels
        );
        true
    }

    /// The screen's part of the window, at its own size, into the picture's top level, and each level below the mean of
    /// the one above.
    fn build_picture(
        &mut self,
        device: &ID3D12Device,
        list: &ID3D12GraphicsCommandList,
        window: &ID3D12Resource,
        window_view: i32,
        uv: &[f32; 4],
    ) -> bool {
        // SAFETY: the caller's live resource.
        let source = unsafe { window.GetDesc() };
        let width = 1u32.max(((uv[2] - uv[0]) as f64 * source.Width as f64).round() as i64 as u32);
        let height =
            1u32.max(((uv[3] - uv[1]) as f64 * source.Height as f64).round() as i64 as u32);
        if !self.ensure_picture(device, width, height, window_view) {
            return false;
        }
        let picture = self.picture.clone().expect("made above");
        for level in 0..self.levels {
            let barrier = [transition(
                &picture,
                level,
                D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
            )];
            // SAFETY: the recording list and the picture made here.
            unsafe { list.ResourceBarrier(&barrier) };
            let w = 1u32.max(width >> level) as i32;
            let h = 1u32.max(height >> level) as i32;
            let target = Target {
                resource: picture.as_raw(),
                view: window_view,
                rect: RECT {
                    left: 0,
                    top: 0,
                    right: w,
                    bottom: h,
                },
                mip: level,
            };
            let ok = if level == 0 {
                self.quads.resample(
                    Some(device),
                    Some(list),
                    &target,
                    Some(window),
                    window_view,
                    0,
                    uv,
                )
            } else {
                self.quads.resample(
                    Some(device),
                    Some(list),
                    &target,
                    Some(&picture),
                    window_view,
                    level - 1,
                    &WHOLE,
                )
            };
            let back = [transition(
                &picture,
                level,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
            )];
            // SAFETY: as above.
            unsafe { list.ResourceBarrier(&back) };
            if !ok {
                return false;
            }
        }
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &mut self,
        device: Option<&ID3D12Device>,
        list: Option<&ID3D12GraphicsCommandList>,
        eye: &Target,
        pose: &Pose,
        fov: &Fov,
        window: Option<&ID3D12Resource>,
        window_view: i32,
        uv: &[f32; 4],
        aspect: f32,
        heading: Option<&Quat>,
        heading_valid: bool,
    ) -> bool {
        let dark = [0.0f32, 0.0, 0.0, 1.0];
        if let (true, Some(window)) = (self.fresh, window) {
            // A new picture of the window this frame: the first eye drawn makes it, before its own target is bound.
            self.fresh = false;
            self.ready = match (device, list) {
                (Some(device), Some(list)) => {
                    self.build_picture(device, list, window, window_view, uv)
                }
                _ => false,
            };
        }
        if !self.quads.fill(device, list, eye, &dark) {
            return false;
        }
        if !self.up || !self.held || !self.ready {
            return true;
        }
        if self.at.has_heading && !heading_valid {
            return true;
        }
        let mut screen = SpaceQuad {
            uv: WHOLE,
            ..Default::default()
        };
        screen.pose = match (self.at.has_heading, heading) {
            (true, Some(heading)) => ahead(pose, heading, self.at.distance),
            _ => self.anchor,
        };
        let aspect = cmath::max(aspect, 0.1);
        screen.height = if self.at.width > 0.0 {
            self.at.width / aspect
        } else {
            self.at.height
        };
        screen.width = screen.height * aspect;
        let opacity = self.opacity * self.shown;
        if !self.quads.draw(
            device,
            list,
            eye,
            pose,
            fov,
            self.picture.as_ref(),
            self.picture_format,
            &screen,
            Blend::Over,
            opacity,
            0.0,
        ) {
            return false;
        }
        if !self.at.has_caption {
            return true;
        }
        let raw = match (device, list) {
            // SAFETY: the caption is this screen's; the device and list the caller's.
            (Some(d), Some(l)) => unsafe {
                caption::cvr_gpu_caption_picture(&mut self.caption, d.as_raw(), l.as_raw())
            },
            // SAFETY: as above, with nothing to record into.
            _ => unsafe {
                caption::cvr_gpu_caption_picture(
                    &mut self.caption,
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                )
            },
        };
        // SAFETY: the caption keeps its reference; borrowed for this call.
        let Some(lines) = (unsafe { ID3D12Resource::from_raw_borrowed(&raw) }) else {
            return true;
        };
        let mut extent = [0.0f32; 3];
        // SAFETY: the caption is this screen's.
        unsafe { caption::cvr_gpu_caption_extent(&self.caption, &mut extent) };
        let [top, bottom, width] = extent;
        // The lines under what the screen shows, at its scale: their band of it, as wide as they were set across it, down
        // the screen's own down.
        let px = screen.height / 2160.0;
        let drop = 0.5 * (top + bottom) * px;
        let down = rotate_vector(
            &screen.pose.orientation,
            Vec3 {
                x: 0.0,
                y: -drop,
                z: 0.0,
            },
        );
        let mut band = SpaceQuad {
            uv: WHOLE,
            ..Default::default()
        };
        band.pose = screen.pose;
        band.pose.position = Vec3 {
            x: band.pose.position.x + down.x,
            y: band.pose.position.y + down.y,
            z: band.pose.position.z + down.z,
        };
        band.width = width * px;
        band.height = (bottom - top) * px;
        self.quads.draw(
            device,
            list,
            eye,
            pose,
            fov,
            Some(lines),
            DXGI_FORMAT_R8G8B8A8_UNORM_SRGB.0,
            &band,
            Blend::Light,
            opacity,
            0.0,
        )
    }

    fn shutdown(&mut self) {
        self.quads.shutdown();
        // SAFETY: the caption is this screen's.
        unsafe { caption::cvr_gpu_caption_shutdown(&mut self.caption) };
        self.picture = None;
        self.retired.clear();
        self.picture_width = 0;
        self.picture_height = 0;
        self.levels = 0;
        self.picture_format = DXGI_FORMAT_UNKNOWN.0;
        self.first_frame = 0;
        self.up = false;
        self.held = false;
        self.fresh = false;
        self.ready = false;
        self.opacity = 0.0;
        self.shown = 1.0;
    }
}

// ---- the C ABI (include/Render/LoadingScreen.hpp)

#[no_mangle]
pub extern "C" fn cvr_loading_screen_new() -> *mut Screen {
    Box::into_raw(Box::default())
}
/// # Safety
/// `screen` came from cvr_loading_screen_new (freed once) or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_loading_screen_free(screen: *mut Screen) {
    if !screen.is_null() {
        // SAFETY: as the caller vouches.
        drop(unsafe { Box::from_raw(screen) });
    }
}
/// Begin; bit 0 of the result: the screen is up, bit 1: it came up now (the C++ face takes the placement's heading).
///
/// # Safety
/// `screen` is live and used by one thread; `head` and `at` are readable; `caption` is null or its lines are readable.
#[no_mangle]
pub unsafe extern "C" fn cvr_loading_screen_begin(
    screen: *mut Screen,
    head: *const Pose,
    at: *const Placement,
    caption: *const CaptionText,
    opacity: f32,
) -> u32 {
    // SAFETY: as the caller vouches.
    let (Some(s), Some(head), Some(at)) = (
        unsafe { screen.as_mut() },
        unsafe { head.as_ref() },
        unsafe { at.as_ref() },
    ) else {
        return 0;
    };
    // SAFETY: as above.
    let came_up = s.begin(head, at, unsafe { caption.as_ref() }, opacity);
    u32::from(s.up) | (u32::from(came_up) << 1)
}
/// # Safety
/// `screen` is live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_loading_screen_up(screen: *const Screen) -> bool {
    // SAFETY: as the caller vouches.
    unsafe { screen.as_ref() }.is_some_and(|s| s.up)
}
/// # Safety
/// `screen` is live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_loading_screen_end(screen: *mut Screen) {
    // SAFETY: as the caller vouches.
    if let Some(s) = unsafe { screen.as_mut() } {
        s.up = false;
        s.held = false;
    }
}
/// # Safety
/// `screen` is live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_loading_screen_hold(screen: *mut Screen) {
    // SAFETY: as the caller vouches.
    if let Some(s) = unsafe { screen.as_mut() } {
        s.hold();
    }
}
/// # Safety
/// `screen` is live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_loading_screen_held(screen: *const Screen) -> bool {
    // SAFETY: as the caller vouches.
    unsafe { screen.as_ref() }.is_some_and(|s| s.held)
}
/// Record: the dark over the eye's rectangle, and the screen (and its caption) when it is up, held and made.
///
/// # Safety
/// `screen` is live and used by one thread; the D3D12 objects are the caller's live ones or null; `rect`, `pose`, `fov`
/// and `uv` are readable; `heading` is null (no heading) or readable.
#[no_mangle]
pub unsafe extern "C" fn cvr_loading_screen_record(
    screen: *mut Screen,
    device: *mut c_void,
    list: *mut c_void,
    target: *mut c_void,
    view: i32,
    rect: *const RECT,
    pose: *const Pose,
    fov: *const Fov,
    window: *mut c_void,
    window_view: i32,
    uv: *const [f32; 4],
    aspect: f32,
    heading: *const Quat,
    heading_valid: bool,
) -> bool {
    // SAFETY: as the caller vouches.
    let (Some(s), Some(rect), Some(pose), Some(fov), Some(uv)) = (
        unsafe { screen.as_mut() },
        unsafe { rect.as_ref() },
        unsafe { pose.as_ref() },
        unsafe { fov.as_ref() },
        unsafe { uv.as_ref() },
    ) else {
        return false;
    };
    if target.is_null() {
        return false;
    }
    let eye = Target {
        resource: target,
        view,
        rect: *rect,
        mip: 0,
    };
    // SAFETY: the caller's objects, borrowed for the call.
    unsafe {
        s.record(
            ID3D12Device::from_raw_borrowed(&device),
            ID3D12GraphicsCommandList::from_raw_borrowed(&list),
            &eye,
            pose,
            fov,
            ID3D12Resource::from_raw_borrowed(&window),
            window_view,
            uv,
            aspect,
            heading.as_ref(),
            heading_valid,
        )
    }
}
/// # Safety
/// `screen` is live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_loading_screen_shutdown(screen: *mut Screen) {
    // SAFETY: as the caller vouches.
    if let Some(s) = unsafe { screen.as_mut() } {
        s.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn located_pose_needs_a_unit_orientation() {
        let mut pose = Pose {
            orientation: Quat {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 1.0,
            },
            position: Vec3 {
                x: 0.0,
                y: 1.6,
                z: 0.0,
            },
        };
        assert!(located_pose(&pose));
        pose.position = Vec3::default();
        assert!(!located_pose(&pose));
        pose.orientation = Quat {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 0.1,
        };
        pose.position.y = 1.0;
        assert!(!located_pose(&pose));
    }

    #[test]
    fn ahead_goes_along_minus_z_of_the_heading() {
        let half = core::f32::consts::FRAC_PI_4;
        let heading = Quat {
            x: 0.0,
            y: half.sin(),
            z: 0.0,
            w: half.cos(),
        }; // 90 degrees about +Y
        let eye = Pose {
            orientation: heading,
            position: Vec3 {
                x: 1.0,
                y: 1.5,
                z: 0.0,
            },
        };
        let p = ahead(&eye, &heading, 2.0);
        assert!(
            (p.position.x - -1.0).abs() < 1e-5
                && (p.position.y - 1.5).abs() < 1e-6
                && p.position.z.abs() < 1e-5
        );
    }
}
