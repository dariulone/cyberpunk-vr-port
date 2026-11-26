//! The loading screen the capture brings up and records (formerly src/Runtimes/OpenXRCapture.cpp's services over
//! OpenXRManager's cvr::loading::Screen): the screen itself is cvr-gpu's (loading_screen.rs), owned here; begun at the
//! boot notice's placement -- its caption and the intro's free-look heading -- or the loading screen's, held when asked,
//! recorded into the capture's list, ended and shut down. The C++ face's state is the heading's: set by the begin that
//! brings the screen up (the placement's heading, none for the loading screen's), kept up by each begin's head.
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::sync::OnceLock;

use cvr_gpu::caption::CaptionLine;
use cvr_gpu::loading_screen::{
    cvr_loading_screen_begin, cvr_loading_screen_end, cvr_loading_screen_hold, cvr_loading_screen_new,
    cvr_loading_screen_record, cvr_loading_screen_shutdown, cvr_loading_screen_up, CaptionText, Placement, Screen,
};
use openxr_sys as xr;
use windows::Win32::Foundation::RECT;

/// The screen (made on first use, as the C++ member was made with OpenXRManager).
static SCREEN: AtomicUsize = AtomicUsize::new(0);
/// The screen came up at a placement with a heading (the boot notice's: the intro's free-look heading).
static HEADING: AtomicBool = AtomicBool::new(false);

fn screen() -> *mut Screen {
    let mut s = SCREEN.load(Relaxed);
    if s == 0 {
        let made = cvr_loading_screen_new() as usize;
        s = match SCREEN.compare_exchange(0, made, Relaxed, Relaxed) {
            Ok(_) => made,
            Err(existing) => {
                // SAFETY: the screen just made, not shared.
                unsafe { cvr_gpu::loading_screen::cvr_loading_screen_free(made as *mut Screen) };
                existing
            }
        };
    }
    s as *mut Screen
}

/// The boot notice's caption (cvr::lobby::notice::Word, taken once as the C++ static was): its lines, NUL-terminated.
struct Notice {
    lines: [Vec<u16>; 2],
    rows: [f32; 2],
    size: f32,
    light: f32,
    locale: Vec<u16>,
    right_to_left: bool,
}
// SAFETY: owned text, read only.
unsafe impl Sync for Notice {}
unsafe impl Send for Notice {}

fn notice() -> &'static Notice {
    static NOTICE: OnceLock<Notice> = OnceLock::new();
    NOTICE.get_or_init(|| {
        let mut w = core::mem::MaybeUninit::<crate::notice::Word>::zeroed();
        // SAFETY: the word filled whole; its texts live for the process (notice.rs's own copies).
        unsafe {
            crate::notice::cvr_notice_word(w.as_mut_ptr());
            let w = w.assume_init();
            let text = |p: *const u16, n: u32| -> Vec<u16> {
                let mut v = if p.is_null() { Vec::new() } else { core::slice::from_raw_parts(p, n as usize).to_vec() };
                v.push(0);
                v
            };
            Notice {
                lines: [text(w.lines[0], w.lengths[0]), text(w.lines[1], w.lengths[1])],
                rows: w.rows,
                size: w.size,
                light: w.light,
                locale: text(w.locale, w.locale_length),
                right_to_left: w.right_to_left,
            }
        }
    })
}

/// The screen ended (the capture no longer shows it).
pub(crate) fn end() {
    // SAFETY: the screen of ours.
    unsafe { cvr_loading_screen_end(screen()) };
}

/// The screen begun at the boot notice's placement (`notice`, at the notice's `opacity`) or the loading screen's (fully
/// shown), and held when asked.
///
/// # Safety
/// `head` is the capture's head pose.
pub(crate) unsafe fn begin(notice_shown: bool, opacity: f32, hold: bool, head: *const xr::Posef) {
    let s = screen();
    // SAFETY: the screen of ours; the caller's pose.
    unsafe {
        let state = if notice_shown {
            // cvr::lobby::intro: kNoticeDistance 3.4 m, kNoticeWidth 2.3 m * 2580 / 1980; up over 0.8 s.
            let at = Placement {
                distance: 3.4,
                height: 0.0,
                width: 2.3f32 * 2580.0f32 / 1980.0f32,
                fade_in: 0.8,
                has_caption: true,
                has_heading: true,
            };
            // The caption's text is offered only while the screen is not up.
            if cvr_loading_screen_up(s) {
                cvr_loading_screen_begin(s, head.cast(), &at, core::ptr::null(), opacity)
            } else {
                let n = notice();
                let lines = [
                    CaptionLine { text: n.lines[0].as_ptr(), length: (n.lines[0].len() - 1) as u32, row: n.rows[0] },
                    CaptionLine { text: n.lines[1].as_ptr(), length: (n.lines[1].len() - 1) as u32, row: n.rows[1] },
                ];
                let caption = CaptionText {
                    lines: lines.as_ptr(),
                    count: 2,
                    size: n.size,
                    light: n.light,
                    locale: n.locale.as_ptr(),
                    right_to_left: n.right_to_left,
                };
                cvr_loading_screen_begin(s, head.cast(), &at, &caption, opacity)
            }
        } else {
            // cvr::loading::kLoadingScreen: 3 m out, 1.8 m tall, up over 0.4 s; no caption, no heading. Fully shown, as
            // Screen::Begin's default had it: `opacity` is the boot notice's fade, 0 whenever the notice is not up -- given
            // to the loading screen it drew the held picture at nothing, and a save's load was black in the headset.
            let at = Placement { distance: 3.0, height: 1.8, width: 0.0, fade_in: 0.4, has_caption: false, has_heading: false };
            cvr_loading_screen_begin(s, head.cast(), &at, core::ptr::null(), 1.0)
        };
        if state & 2 != 0 {
            HEADING.store(notice_shown, Relaxed);
        }
        if state & 1 != 0 && HEADING.load(Relaxed) {
            crate::lobby_intro::cvr_intro_heading_update(head);
        }
        if hold {
            cvr_loading_screen_hold(s);
        }
    }
}

/// The screen recorded into the capture's list over `target` (its view and rectangle) at the eye's pose and field of
/// view, from the window texture's `uv` at `aspect`; at the heading's orientation when the screen keeps one.
///
/// # Safety
/// The capture's list, resources and descriptions.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn record(
    list: *mut c_void,
    target: *mut c_void,
    view: i32,
    rect: *const RECT,
    pose: *const xr::Posef,
    fov: *const xr::Fovf,
    window: *mut c_void,
    window_view: i32,
    uv: *const f32,
    aspect: f32,
) -> bool {
    let heading = HEADING.load(Relaxed);
    let mut orientation = xr::Quaternionf { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };
    // SAFETY: the intro's heading (Rust's), read into a local.
    let valid = heading && unsafe { crate::lobby_intro::cvr_intro_heading(core::ptr::null_mut()) };
    if valid {
        // SAFETY: as above.
        unsafe { crate::lobby_intro::cvr_intro_heading(&mut orientation) };
    }
    // SAFETY: the submit state's device (InitGraphics'); the caller's arguments.
    unsafe {
        let device = *(*cvr_xr::submit_state::cvr_xr_submit_shared()).device.get();
        cvr_loading_screen_record(
            screen(),
            device,
            list,
            target,
            view,
            rect.cast(),
            pose.cast(),
            fov.cast(),
            window,
            window_view,
            uv.cast(),
            aspect,
            if heading { (&raw const orientation).cast() } else { core::ptr::null() },
            valid,
        )
    }
}

/// The screen's GPU objects released (OpenXRManager::Shutdown).
pub(crate) fn shutdown() {
    // SAFETY: the screen of ours.
    unsafe { cvr_loading_screen_shutdown(screen()) };
}
