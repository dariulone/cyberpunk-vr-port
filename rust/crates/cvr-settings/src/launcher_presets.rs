//! The headsets' render-size ladders and the launcher's snap and fit (formerly the tables, `NearestCostEntry`,
//! `SnapLauncherResolution` and `FitLauncherResolution` of src/Overlay/LauncherDialog.cpp). The dialog lists these
//! ladders through `cvr_launcher_presets`, in the C layout it had; the labels are wide strings it hands to its combo
//! boxes as they are.
//!
//! EVERY LADDER IS SHAPED BY ITS HEADSET'S EYE WINDOW, NOT BY ITS PANEL.
//!
//! Each eye renders exactly its own lens window (include/Runtimes/AsymmetricProjection.hpp Windows(): the eye's own
//! horizontal span, one vertical span for both eyes) drawn into the render target edge to edge, so the target must
//! have the window's aspect or the pixels are not square -- src/Stereo/AsymProjection.cpp then grows the window to the
//! target's aspect, which keeps them square at the price of rendering past the lens. The aspect of every entry below
//! is therefore the window's: tan(right) - tan(left) of the wider eye, in the head frame (a canted eye turned by its
//! cant first), over tan(up) - tan(down) of the taller.
//!
//! The windows are the runtimes' own per-eye projections, as hmdq recorded them for the HMD Geometry Database
//! (risa2000.github.io/hmdgdb, raw LRBT tangents). Its Quest 3 record is, to the hundredth of a degree, the window a
//! Quest 3 reports through Virtual Desktop. A runtime FOV cut (Pimax's smaller FOV settings, a headset's reduced-FOV
//! mode) changes the window, not the ladder; the fit keeps the pixels square either way, at the cost of a sliver past
//! the lens.
//!
//! Each entry costs what the entry at its place in the old (panel- or spec-shaped) ladder did, within 0.4%, so a pick
//! keeps its frame time while all of its pixels land inside the lens. Both dimensions are multiples of 8, every aspect
//! is within 0.25% of the window, and each entry needs a matching vrcam_<W>x<H> / vrcam_feed_<W>x<H> asset pair:
//! tools/gen_vrcam_assets.py --prune. A saved resolution that is not in its headset's ladder (one these ladders
//! retired) moves to the entry nearest in cost: in the dialog, and with the dialog off in [`snap`] at startup.
//!
//! THE STEPS BELOW 3500 PX ARE AT MOST 1.12 PER SIDE, the Quest 3 ladder's spacing: wherever two neighbours (by cost)
//! were more than 1.15 apart, the gap is cut into equal steps, keeping the entries whose larger side is 3500 or less
//! (a Pico 4 went 2160 -> 2560 -> 3072 -> 4096). These engine-stereo-only entries have the window's aspect like the
//! rest but no vrcam assets: they are offered only while engine stereo renders both eyes, and a pick of one moves to
//! the nearest entry that has them when it does not.

/// A NUL-terminated UTF-16 string in the image, for the dialog's Win32 calls.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct WStr(*const u16);
// SAFETY: points at an immutable static.
unsafe impl Sync for WStr {}
// SAFETY: as above.
unsafe impl Send for WStr {}
impl WStr {
    /// The NUL-terminated string, for the Win32 calls.
    pub fn as_ptr(&self) -> *const u16 {
        self.0
    }
    /// The text, for Rust's side and the tests.
    pub fn text(&self) -> String {
        let mut n = 0;
        // SAFETY: a NUL-terminated static built by `w!`.
        unsafe {
            while *self.0.add(n) != 0 {
                n += 1;
            }
            String::from_utf16_lossy(core::slice::from_raw_parts(self.0, n))
        }
    }
}

/// An ASCII literal as a NUL-terminated UTF-16 static.
macro_rules! w {
    ($s:literal) => {{
        const S: &str = $s;
        const N: usize = S.len() + 1;
        const W: [u16; N] = {
            let b = S.as_bytes();
            let mut w = [0u16; N];
            let mut i = 0;
            while i < b.len() {
                assert!(b[i] < 0x80);
                w[i] = b[i] as u16;
                i += 1;
            }
            w
        };
        WStr(W.as_ptr())
    }};
}

/// One ladder entry (the dialog's `ResolutionPreset`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Resolution {
    pub width: i32,
    pub height: i32,
    pub label: WStr,
    /// Offered only while engine stereo renders both eyes: the old stereo path draws the second eye through a VRCAM
    /// view of the pick's size, and these entries have no vrcam_<W>x<H> component.
    pub engine_stereo_only: bool,
}
const fn r(width: i32, height: i32, label: WStr) -> Resolution {
    Resolution {
        width,
        height,
        label,
        engine_stereo_only: false,
    }
}
const fn es(width: i32, height: i32, label: WStr) -> Resolution {
    Resolution {
        width,
        height,
        label,
        engine_stereo_only: true,
    }
}

/// The ladder's first entry, in the dialog's layout.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct LadderPtr(*const Resolution);
// SAFETY: points at an immutable static.
unsafe impl Sync for LadderPtr {}
// SAFETY: as above.
unsafe impl Send for LadderPtr {}

/// One headset (the dialog's `HmdPreset`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Hmd {
    /// What the launcher file keeps as `hmd_type`.
    pub mhd_type: i32,
    pub hmd_type: WStr,
    pub label: WStr,
    pub resolutions: LadderPtr,
    pub resolution_count: i32,
}
const fn hmd<const N: usize>(
    mhd_type: i32,
    hmd_type: WStr,
    label: WStr,
    ladder: &'static [Resolution; N],
) -> Hmd {
    Hmd {
        mhd_type,
        hmd_type,
        label,
        resolutions: LadderPtr(ladder.as_ptr()),
        resolution_count: N as i32,
    }
}
impl Hmd {
    pub fn ladder(&self) -> &'static [Resolution] {
        // SAFETY: one of the statics below and its length.
        unsafe { core::slice::from_raw_parts(self.resolutions.0, self.resolution_count as usize) }
    }
}

/// Every headset, in the dialog's order.
pub fn presets() -> &'static [Hmd] {
    &PRESETS
}

/// The entries this launch offers: every one with engine stereo, only those with VRCAM assets without it.
pub fn offered(r: &Resolution, engine_stereo: bool) -> bool {
    !r.engine_stereo_only || engine_stereo
}

/// The offered ladder entry nearest in cost (pixel count) to `w` x `h`; None when the ladder offers none.
pub fn nearest_cost_entry(hmd: &Hmd, w: i32, h: i32, engine_stereo: bool) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, r) in hmd.ladder().iter().enumerate() {
        if !offered(r, engine_stereo) {
            continue;
        }
        let diff = (f64::from(r.width) * f64::from(r.height) / (f64::from(w) * f64::from(h)))
            .ln()
            .abs();
        if best.is_none_or(|(_, d)| diff < d) {
            best = Some((i, diff));
        }
    }
    best.map(|(i, _)| i)
}

/// A resolution that is not in the headset's ladder becomes the offered entry nearest in cost; None when it stays
/// (it is an offered entry, the size is not positive, or the headset has no ladder).
pub fn snap(hmd_type: i32, width: i32, height: i32, engine_stereo: bool) -> Option<(i32, i32)> {
    if width <= 0 || height <= 0 {
        return None;
    }
    let hmd = PRESETS
        .iter()
        .find(|h| h.mhd_type == hmd_type && h.resolution_count > 0)?;
    if hmd
        .ladder()
        .iter()
        .any(|r| r.width == width && r.height == height && offered(r, engine_stereo))
    {
        return None;
    }
    let nearest = &hmd.ladder()[nearest_cost_entry(hmd, width, height, engine_stereo)?];
    Some((nearest.width, nearest.height))
}

/// A resolution larger than `max_width` x `max_height` becomes the largest offered entry of the headset's ladder
/// within them -- or, for a headset without a ladder or one with no entry that small, the same aspect scaled into
/// them (multiples of 8); None when it stays.
pub fn fit(
    hmd_type: i32,
    max_width: i32,
    max_height: i32,
    width: i32,
    height: i32,
    engine_stereo: bool,
) -> Option<(i32, i32)> {
    if width <= 0 || height <= 0 || max_width < 8 || max_height < 8 {
        return None;
    }
    if width <= max_width && height <= max_height {
        return None;
    }
    for hmd in PRESETS.iter().filter(|h| h.mhd_type == hmd_type) {
        let mut best: Option<&Resolution> = None;
        for r in hmd.ladder() {
            if !offered(r, engine_stereo) || r.width > max_width || r.height > max_height {
                continue;
            }
            let area = |r: &Resolution| f64::from(r.width) * f64::from(r.height);
            if best.is_none_or(|b| area(r) > area(b)) {
                best = Some(r);
            }
        }
        if let Some(b) = best {
            return Some((b.width, b.height));
        }
    }
    let scale =
        (f64::from(max_width) / f64::from(width)).min(f64::from(max_height) / f64::from(height));
    Some((
        (f64::from(width) * scale) as i32 / 8 * 8,
        (f64::from(height) * scale) as i32 / 8 * 8,
    ))
}

// ---- the C API (the dialog, include/Overlay/LauncherDialog.hpp) ------------------------------------------------

/// The headsets in the dialog's layout and their count.
///
/// # Safety
/// `count` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_launcher_presets(count: *mut i32) -> *const Hmd {
    if !count.is_null() {
        // SAFETY: the caller's slot.
        unsafe { *count = PRESETS.len() as i32 };
    }
    PRESETS.as_ptr()
}

/// [`offered`] for one of the dialog's entries.
///
/// # Safety
/// `r` is null or one of `cvr_launcher_presets`' entries.
#[no_mangle]
pub unsafe extern "C" fn cvr_launcher_offered(r: *const Resolution, engine_stereo: i32) -> i32 {
    // SAFETY: the caller's entry.
    unsafe { r.as_ref() }.is_some_and(|r| offered(r, engine_stereo != 0)) as i32
}

/// [`nearest_cost_entry`]: the entry's index in the ladder, -1 when the ladder offers none.
///
/// # Safety
/// `hmd` is null or one of `cvr_launcher_presets`' headsets.
#[no_mangle]
pub unsafe extern "C" fn cvr_launcher_nearest_cost_entry(
    hmd: *const Hmd,
    w: i32,
    h: i32,
    engine_stereo: i32,
) -> i32 {
    // SAFETY: the caller's headset.
    let Some(hmd) = (unsafe { hmd.as_ref() }) else {
        return -1;
    };
    nearest_cost_entry(hmd, w, h, engine_stereo != 0).map_or(-1, |i| i as i32)
}

/// SnapLauncherResolution: [`snap`] written back; 1 when it changed.
///
/// # Safety
/// `width` and `height` are null or read- and writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_launcher_snap_resolution(
    hmd_type: i32,
    width: *mut i32,
    height: *mut i32,
    engine_stereo: i32,
) -> i32 {
    if width.is_null() || height.is_null() {
        return 0;
    }
    // SAFETY: the caller's slots.
    unsafe {
        match snap(hmd_type, *width, *height, engine_stereo != 0) {
            Some((w, h)) => {
                *width = w;
                *height = h;
                1
            }
            None => 0,
        }
    }
}

/// FitLauncherResolution: [`fit`] written back; 1 when it changed.
///
/// # Safety
/// `width` and `height` are null or read- and writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_launcher_fit_resolution(
    hmd_type: i32,
    max_width: i32,
    max_height: i32,
    width: *mut i32,
    height: *mut i32,
    engine_stereo: i32,
) -> i32 {
    if width.is_null() || height.is_null() {
        return 0;
    }
    // SAFETY: the caller's slots.
    unsafe {
        match fit(
            hmd_type,
            max_width,
            max_height,
            *width,
            *height,
            engine_stereo != 0,
        ) {
            Some((w, h)) => {
                *width = w;
                *height = h;
                1
            }
            None => 0,
        }
    }
}

// ---- the ladders -----------------------------------------------------------------------------------------------

// QUEST 2: L-52 / R+45 (mirrored for the other eye), U+48 / D-50 (Oculus runtime 1.53, 2020)
//     window AR = (tan 52 + tan 45) / (tan 48 + tan 50) = 2.279942 / 2.302367 = 0.99026
// The old ladder had the panel's 0.954 (1832 x 1920): 3.6% of every target rendered past the lens.
static QUEST2: [Resolution; 11] = [
    r(
        1864,
        1880,
        w!("1864 x 1880 (cost of the 1832 x 1920 panel)"),
    ),
    r(1712, 1728, w!("1712 x 1728")),
    r(2080, 2104, w!("2080 x 2104")),
    es(2328, 2352, w!("2328 x 2352")),
    r(2600, 2624, w!("2600 x 2624")),
    es(2856, 2888, w!("2856 x 2888")),
    es(3144, 3168, w!("3144 x 3168")),
    es(3448, 3480, w!("3448 x 3480")),
    r(4168, 4208, w!("4168 x 4208")),
    r(5088, 5136, w!("5088 x 5136")),
    r(6104, 6168, w!("6104 x 6168")),
];

// QUEST 3S: NOT MEASURED, no record of it exists. It has the Quest 2's Fresnel lenses and 1832 x 1920
// panels, so it takes the Quest 2's window (0.99026); the old ladder was 1.021, wider than tall. The
// runtime's per-eye angles are logged at startup -- if they disagree, rebuild this ladder from them.
static QUEST3S: [Resolution; 11] = [
    r(1888, 1904, w!("1888 x 1904 (cost of the old 1920 x 1880)")),
    r(1656, 1672, w!("1656 x 1672")),
    r(2016, 2040, w!("2016 x 2040")),
    es(2256, 2280, w!("2256 x 2280")),
    r(2520, 2544, w!("2520 x 2544")),
    es(2768, 2792, w!("2768 x 2792")),
    es(3040, 3072, w!("3040 x 3072")),
    es(3344, 3376, w!("3344 x 3376")),
    r(4032, 4072, w!("4032 x 4072")),
    r(4920, 4968, w!("4920 x 4968")),
    r(5912, 5968, w!("5912 x 5968")),
];

// QUEST 3 IS SHAPED BY ITS EYE'S OWN WINDOW, NOT BY ITS PANEL.
//
// The window is drawn into the render target edge to edge, so the target must have the window's aspect or
// the pixels are not square (src/Stereo/AsymProjection.cpp grows the window to fit any other pick, which
// keeps them square at the price of rendering past the lens). The window comes out of the runtime's own
// per-eye numbers, measured on a Quest 3 through Virtual Desktop 1.0.10 (2026-10-15):
//     per eye     L-54 / R+40 (mirrored for the other eye), U+44 / D-55
//     horizontal  each eye its own lens window, nothing past it -> tan 54 + tan 40 = 2.215476
//     vertical    the eyes share it, off-centre                 -> tan 44 + tan 55 = 2.393837
//     window AR = 2.215476 / 2.393837 = 0.92549
//
// Every pixel lands inside the lens: against the old symmetric render (55 deg every way, 2560 x 2848) the
// same pixel count is 33% denser across and 20% denser up and down. Each entry costs what the entry of
// the earlier union ladder (both eyes +-54) at its place did; 2648 x 2864 costs what the old 2560 x 2848 did.
//
// Every entry is within 0.12% of the window, both dimensions multiples of 8. Each one needs a matching
// vrcam_<W>x<H> / vrcam_feed_<W>x<H> asset pair: tools/gen_vrcam_assets.py --prune.
static QUEST3: [Resolution; 10] = [
    r(1864, 2016, w!("1864 x 2016")),
    r(2064, 2232, w!("2064 x 2232")),
    r(2296, 2480, w!("2296 x 2480")),
    r(2408, 2600, w!("2408 x 2600")),
    r(2648, 2864, w!("2648 x 2864")),
    r(2856, 3088, w!("2856 x 3088")),
    r(3216, 3472, w!("3216 x 3472")),
    r(3672, 3968, w!("3672 x 3968")),
    r(4480, 4840, w!("4480 x 4840")),
    r(5384, 5816, w!("5384 x 5816")),
];

// PICO 4 / PICO 4 ULTRA: the runtime reports a symmetric +-52 deg window both ways (Pico 4 through
// SteamVR, 2022), so the square ladder IS the window. The Ultra has no record; it has the same 2160 x 2160
// panels and 105 deg optics and keeps the same ladder.
static PICO4: [Resolution; 11] = [
    r(2160, 2160, w!("2160 x 2160 (Native)")),
    r(1920, 1920, w!("1920 x 1920")),
    r(2048, 2048, w!("2048 x 2048")),
    es(2352, 2352, w!("2352 x 2352")),
    r(2560, 2560, w!("2560 x 2560")),
    es(2808, 2808, w!("2808 x 2808")),
    r(3072, 3072, w!("3072 x 3072")),
    es(3384, 3384, w!("3384 x 3384")),
    r(4096, 4096, w!("4096 x 4096")),
    r(5000, 5000, w!("5000 x 5000")),
    r(6000, 6000, w!("6000 x 6000")),
];

static PICO4_ULTRA: [Resolution; 11] = [
    r(2160, 2160, w!("2160 x 2160 (Native)")),
    r(1920, 1920, w!("1920 x 1920")),
    r(2048, 2048, w!("2048 x 2048")),
    es(2352, 2352, w!("2352 x 2352")),
    r(2560, 2560, w!("2560 x 2560")),
    es(2808, 2808, w!("2808 x 2808")),
    r(3072, 3072, w!("3072 x 3072")),
    es(3384, 3384, w!("3384 x 3384")),
    r(4096, 4096, w!("4096 x 4096")),
    r(5000, 5000, w!("5000 x 5000")),
    r(6000, 6000, w!("6000 x 6000")),
];

// STEAM FRAME (PC): L-59.0 / R+50.4, U+49.5 / D-59.9 and the mirror image for the right eye, within 0.3 deg
// (SteamVR 2.18, 2026)
//     window AR = 2.877668 (the wider eye) / 2.896121 (the taller) = 0.99363
// Near square, but the window sits low and off-centre in the panel; the old square ladder rendered 0.6% past
// the lens.
static STEAM_FRAME: [Resolution; 11] = [
    r(
        2152,
        2168,
        w!("2152 x 2168 (cost of the 2160 x 2160 panel)"),
    ),
    r(1912, 1928, w!("1912 x 1928 (Performance)")),
    r(2040, 2056, w!("2040 x 2056")),
    es(2344, 2360, w!("2344 x 2360")),
    r(2552, 2568, w!("2552 x 2568 (Balanced)")),
    es(2800, 2816, w!("2800 x 2816")),
    r(3064, 3080, w!("3064 x 3080 (High)")),
    es(3368, 3392, w!("3368 x 3392")),
    r(4080, 4112, w!("4080 x 4112 (Ultra)")),
    r(4984, 5016, w!("4984 x 5016")),
    r(5984, 6024, w!("5984 x 6024")),
];

// PIMAX CRYSTAL: L-51.7 / R+41.7 (mirrored), U+51.9 / D-51.9, parallel views (SteamVR 1.26, 2023)
//     window AR = (1.264092 + 0.889501) / (2 x 1.274264) = 0.84503 -- taller than wide, square panel or not
// The old near-square ladder (1.0065) rendered 16% of every target past the lens.
// CRYSTAL LIGHT: no record; it has the Crystal's lenses and 2880 x 2880 panels, so it takes the same window.
static CRYSTAL_OG: [Resolution; 9] = [
    r(2256, 2672, w!("2256 x 2672 (cost of the old 2464 x 2448)")),
    r(1976, 2344, w!("1976 x 2344")),
    r(1880, 2224, w!("1880 x 2224")),
    r(2344, 2776, w!("2344 x 2776")),
    es(2568, 3040, w!("2568 x 3040")),
    r(2816, 3328, w!("2816 x 3328")),
    r(3752, 4440, w!("3752 x 4440")),
    r(4584, 5424, w!("4584 x 5424")),
    r(5496, 6504, w!("5496 x 6504")),
];

static CRYSTAL_LIGHT: [Resolution; 9] = [
    r(2256, 2672, w!("2256 x 2672 (cost of the old 2464 x 2448)")),
    r(1976, 2344, w!("1976 x 2344")),
    r(1880, 2224, w!("1880 x 2224")),
    r(2344, 2776, w!("2344 x 2776")),
    es(2568, 3040, w!("2568 x 3040")),
    r(2816, 3328, w!("2816 x 3328")),
    r(3752, 4440, w!("3752 x 4440")),
    r(4584, 5424, w!("4584 x 5424")),
    r(5496, 6504, w!("5496 x 6504")),
];

// PIMAX CRYSTAL SUPER, 50 PPD optical engine: L-63.5 / R+51.5 (mirrored), U+58.7 / D-58.7 (SteamVR 2.9,
// 2025; 127 deg in all with 103 of overlap -- the engine's published 127)
//     window AR = (2.007429 + 1.258067) / (2 x 1.643018) = 0.99375
static CRYSTAL_SUPER: [Resolution; 10] = [
    r(2448, 2464, w!("2448 x 2464 (cost of the old 2464 x 2448)")),
    r(2144, 2160, w!("2144 x 2160")),
    r(2032, 2048, w!("2032 x 2048")),
    r(2544, 2560, w!("2544 x 2560")),
    es(2792, 2808, w!("2792 x 2808")),
    r(3056, 3072, w!("3056 x 3072")),
    es(3360, 3384, w!("3360 x 3384")),
    r(4072, 4096, w!("4072 x 4096")),
    r(4968, 5000, w!("4968 x 5000")),
    r(5960, 6000, w!("5960 x 6000")),
];

// CRYSTAL SUPER ULTRA-WIDE optical engine: NOT MEASURED, DERIVED. Pimax gives 140 deg in all with 90 of
// overlap, so each eye spans outer 70 / inner 45 = 115 deg -- the measured 50 PPD engine's 115.04, turned
// outward (Pimax: the same lens and panel, moved). The vertical is the 50 PPD engine's.
//     window AR = (tan 70 + tan 45) / (2 x 1.643018) = 1.14042
// The runtime's per-eye angles are logged at startup -- if they disagree, rebuild this ladder from them.
static CRYSTAL_WF: [Resolution; 9] = [
    r(2624, 2304, w!("2624 x 2304 (cost of the old 2464 x 2448)")),
    r(2296, 2016, w!("2296 x 2016")),
    r(2176, 1912, w!("2176 x 1912")),
    r(2728, 2392, w!("2728 x 2392")),
    es(2984, 2616, w!("2984 x 2616")),
    r(3272, 2864, w!("3272 x 2864")),
    r(4360, 3824, w!("4360 x 3824")),
    r(5320, 4664, w!("5320 x 4664")),
    r(6384, 5600, w!("6384 x 5600")),
];

// VALVE INDEX: each eye L-52.0 / R+51.5, U+54.7 / D-54.6 in its own view, and the views CANTED 5 deg
// outward (SteamVR 1.7, 2019). In the head frame, where both eyes are rendered, the windows are
// L-57.0 / R+46.5 (mirrored):
//     window AR = (tan 57.0 + tan 46.5) / 2.822942 = 0.91970
// against the panel's 0.9 (1440 x 1600).
static VALVE_INDEX: [Resolution; 14] = [
    r(
        1456,
        1584,
        w!("1456 x 1584 (cost of the 1440 x 1600 panel)"),
    ),
    es(1176, 1280, w!("1176 x 1280")),
    r(1272, 1384, w!("1272 x 1384")),
    r(1088, 1184, w!("1088 x 1184")),
    es(1624, 1768, w!("1624 x 1768")),
    r(1816, 1976, w!("1816 x 1976")),
    es(2016, 2192, w!("2016 x 2192")),
    es(2232, 2424, w!("2232 x 2424")),
    es(2472, 2688, w!("2472 x 2688")),
    es(2744, 2984, w!("2744 x 2984")),
    es(3040, 3304, w!("3040 x 3304")),
    r(4144, 4504, w!("4144 x 4504")),
    r(5056, 5496, w!("5056 x 5496")),
    r(6064, 6592, w!("6064 x 6592")),
];

// PIMAX DREAM AIR (lighthouse): L-55.1 / R+45.5 (mirrored), U+44.5 / D-44.5 (SteamVR 2.16, 2026)
//     window AR = (1.432247 + 1.019034) / (2 x 0.984291) = 1.24520
// The old ladder took a tester's SteamVR lens figures, 99 x 88 deg, as a symmetric frustum (1.21245); the
// eye's window is wider than that, its lens off the panel's centre.
static PIMAX_DREAM_AIR: [Resolution; 7] = [
    r(2536, 2040, w!("2536 x 2040 (cost of the old 2504 x 2064)")),
    r(2800, 2248, w!("2800 x 2248")),
    r(3112, 2504, w!("3112 x 2504")),
    es(3424, 2752, w!("3424 x 2752")),
    r(4152, 3336, w!("4152 x 3336")),
    r(5072, 4072, w!("5072 x 4072")),
    r(6088, 4888, w!("6088 x 4888")),
];

// PLAYSTATION VR2 (PC adapter, SteamVR/OpenXR): L-61.5 / R+43.4 (mirrored), U+53.0 / D-53.0 (SteamVR 2.8,
// 2025)
//     window AR = (1.841771 + 0.947191) / (2 x 1.328981) = 1.04929
// The runtime's recommended 3400 x 3468 (0.980) is the panel's shape (2000 x 2040), not the window's; the
// old square ladder rendered 4.7% of every target past the lens.
static PSVR2: [Resolution; 11] = [
    r(1968, 1872, w!("1968 x 1872 (Performance)")),
    r(2096, 2000, w!("2096 x 2000 (Near panel resolution)")),
    es(2344, 2232, w!("2344 x 2232")),
    r(2624, 2496, w!("2624 x 2496 (Balanced)")),
    es(2872, 2736, w!("2872 x 2736")),
    r(3144, 3000, w!("3144 x 3000 (High)")),
    es(3400, 3240, w!("3400 x 3240")),
    r(3672, 3496, w!("3672 x 3496 (Near runtime scale)")),
    r(4200, 4000, w!("4200 x 4000 (Ultra)")),
    r(5120, 4880, w!("5120 x 4880")),
    r(6144, 5856, w!("6144 x 5856")),
];

// BIGSCREEN BEYOND 2 / 2e: identical panels and optics on both models. Each unit is calibrated, so the
// numbers move a little from one to the next; this one (Beyond 2e, SteamVR 2.12, 2025):
//     left eye  L-54.8 / R+42.0, U+44.0 / D-52.9      right eye  L-42.5 / R+54.4, U+43.7 / D-53.2
//     window AR = 2.319957 (the wider eye) / 2.294896 (the taller) = 1.01092
// The old ladder (1.2393) was derived from the published 108 x 96 deg -- but the 108 is both eyes'
// horizontal and the 96 one eye's vertical: 18% of every target rendered past the lens.
static BEYOND2: [Resolution; 11] = [
    r(1736, 1720, w!("1736 x 1720")),
    r(1848, 1832, w!("1848 x 1832")),
    es(2064, 2048, w!("2064 x 2048")),
    r(2312, 2288, w!("2312 x 2288 (cost of the old 2560 x 2064)")),
    es(2536, 2504, w!("2536 x 2504")),
    r(2776, 2744, w!("2776 x 2744")),
    es(2992, 2960, w!("2992 x 2960")),
    r(3232, 3200, w!("3232 x 3200")),
    r(3696, 3656, w!("3696 x 3656")),
    r(4512, 4464, w!("4512 x 4464")),
    r(5416, 5360, w!("5416 x 5360")),
];

// HP REVERB G2: L-49.6 / R+44.6 (mirrored), U+46.6 / D-46.6, views turned 0.2 deg (SteamVR 1.15, 2020)
//     window AR = 2.166975 (the wider eye) / 2.113323 (the taller) = 1.02539
// It used to share the Pico 4's square ladder.
static REVERB_G2: [Resolution; 11] = [
    r(
        2192,
        2136,
        w!("2192 x 2136 (cost of the 2160 x 2160 panel)"),
    ),
    r(1944, 1896, w!("1944 x 1896")),
    r(2072, 2024, w!("2072 x 2024")),
    es(2384, 2320, w!("2384 x 2320")),
    r(2592, 2528, w!("2592 x 2528")),
    es(2840, 2768, w!("2840 x 2768")),
    r(3112, 3032, w!("3112 x 3032")),
    es(3424, 3336, w!("3424 x 3336")),
    r(4152, 4048, w!("4152 x 4048")),
    r(5064, 4936, w!("5064 x 4936")),
    r(6080, 5928, w!("6080 x 5928")),
];

// SHIFTALL MEGANEX SUPERLIGHT 8K: no hmdq record of the current drivers; the window is the drivers' own
// projection code. The native driver (CustomHeadsetOpenVR, Shiftall's MeganeXsuperlight8K_Native fork and
// sboys3's 1.3.0) reports a symmetric frustum per eye, parallel views: the "MeganeX8K Default" profile
// reaches 58.0 deg across and 49.7 deg up and down, clamped to the 100 x 96 deg default, plus 0..3 deg
// every session against burn-in (each eye sees its 3552 x 3840 panel turned, 3840 across).
//     window AR = tan 50 / tan 48 = 1.07306, 1.07373 at +3 deg; the ladder aims between, 1.07337
// The stock driver (MeganeX VG3 display redirect, hmdq 2.1.8, January 2025) gave a symmetric 95 x 95 deg,
// a square window; the fit keeps the pixels square there too, at a sliver past the lens. The runtime's
// per-eye angles are logged at startup -- if they disagree, rebuild this ladder from them.
static MEGANEX_8K: [Resolution; 11] = [
    r(
        3824,
        3568,
        w!("3824 x 3568 (cost of the 3552 x 3840 panel)"),
    ),
    r(1992, 1856, w!("1992 x 1856")),
    r(2120, 1976, w!("2120 x 1976")),
    es(2376, 2208, w!("2376 x 2208")),
    r(2656, 2472, w!("2656 x 2472")),
    es(2912, 2712, w!("2912 x 2712")),
    r(3184, 2968, w!("3184 x 2968")),
    es(3488, 3256, w!("3488 x 3256")),
    r(4248, 3960, w!("4248 x 3960")),
    r(5176, 4824, w!("5176 x 4824")),
    r(6216, 5792, w!("6216 x 5792")),
];

// SAMSUNG GALAXY XR: L-54.5 / R+39.9 (mirrored), U+52.6 / D-52.6, parallel views, no hidden-area mask
// (SteamVR 2.13, 2025)
//     window AR = (1.401340 + 0.835410) / (2 x 1.306490) = 0.85601
// The 3552 x 3840 panels (0.925) are wider than the window; a panel-shaped target renders 7.5% past the lens.
// Each entry costs what the Virtual Desktop preset it is named for does (VD's sizes are panel-shaped).
// Rampage, 4224 x 4608, is about 1.19x the panel per side: the native default.
static GALAXY_XR: [Resolution; 3] = [
    r(4080, 4768, w!("4080 x 4768 (VD Rampage, native)")),
    r(2928, 3424, w!("2928 x 3424 (VD Godlike)")),
    r(3552, 4144, w!("3552 x 4144 (VD Monster)")),
];

// HTC VIVE: L-54.4 / R+46.8 (mirrored), U+55.7 / D-55.9, parallel views (SteamVR 1.10, 2020)
//     window AR = (1.397283 + 1.250510) / (1.464221 + 1.477517) = 0.90008
// The window has the 1080 x 1200 panel's shape, so the ladder is the panel's; 1512 x 1680 is what SteamVR asks
// for at 100%.
static VIVE: [Resolution; 8] = [
    r(1080, 1200, w!("1080 x 1200 (panel)")),
    es(1176, 1304, w!("1176 x 1304")),
    r(1296, 1440, w!("1296 x 1440")),
    es(1400, 1552, w!("1400 x 1552")),
    r(1512, 1680, w!("1512 x 1680 (SteamVR 100%)")),
    r(1728, 1920, w!("1728 x 1920")),
    es(1928, 2144, w!("1928 x 2144")),
    r(2160, 2400, w!("2160 x 2400")),
];

// HTC VIVE PRO: L-53.6 / R+45.3 (mirrored), U+53.5 / D-54.3, parallel views (SteamVR 2.9, 2025)
//     window AR = 2.566067 (the wider eye) / 2.851153 (the taller) = 0.90001
// The panel's shape again (1440 x 1600); 2016 x 2240 is SteamVR's 100%. The Pro Eye has no record; it has the
// same panels and lenses and takes the same ladder.
static VIVE_PRO: [Resolution; 10] = [
    r(1440, 1600, w!("1440 x 1600 (panel)")),
    r(1152, 1280, w!("1152 x 1280")),
    es(1288, 1432, w!("1288 x 1432")),
    es(1576, 1752, w!("1576 x 1752")),
    r(1728, 1920, w!("1728 x 1920")),
    es(1864, 2072, w!("1864 x 2072")),
    r(2016, 2240, w!("2016 x 2240 (SteamVR 100%)")),
    r(2304, 2560, w!("2304 x 2560")),
    es(2576, 2864, w!("2576 x 2864")),
    r(2880, 3200, w!("2880 x 3200")),
];

/// The headsets, in the dialog's order: the type saved in the launcher file, the short and the shown name, the ladder.
static PRESETS: [Hmd; 19] = [
    hmd(0, w!("Q2"), w!("Meta Quest2"), &QUEST2),
    hmd(1, w!("Q3S"), w!("Meta Quest3s"), &QUEST3S),
    hmd(2, w!("Q3"), w!("Meta Quest3"), &QUEST3),
    hmd(3, w!("P4"), w!("Pico4"), &PICO4),
    hmd(4, w!("P4U"), w!("Pico4 Ultra"), &PICO4_ULTRA),
    hmd(5, w!("CRYOG"), w!("Crystal OG"), &CRYSTAL_OG),
    hmd(6, w!("GRYLI"), w!("Crystal Light"), &CRYSTAL_LIGHT),
    hmd(7, w!("CRYSU"), w!("Crystal Super"), &CRYSTAL_SUPER),
    hmd(8, w!("CRYWF"), w!("Crystal Utra Wide"), &CRYSTAL_WF),
    hmd(9, w!("VINDEX"), w!("Valve Index"), &VALVE_INDEX),
    hmd(10, w!("PDA"), w!("Pimax Dream Air"), &PIMAX_DREAM_AIR),
    hmd(11, w!("PSVR2"), w!("PlayStation VR2"), &PSVR2),
    hmd(12, w!("BSB2"), w!("Bigscreen Beyond 2/2e"), &BEYOND2),
    hmd(13, w!("SFRAME"), w!("Steam Frame"), &STEAM_FRAME),
    hmd(14, w!("RG2"), w!("HP Reverb G2"), &REVERB_G2),
    hmd(15, w!("MX8K"), w!("MeganeX superlight 8K"), &MEGANEX_8K),
    hmd(16, w!("GXR"), w!("Samsung Galaxy XR"), &GALAXY_XR),
    hmd(17, w!("VIVE"), w!("HTC Vive"), &VIVE),
    hmd(18, w!("VIVEP"), w!("HTC Vive Pro / Pro Eye"), &VIVE_PRO),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn quest3s() -> &'static Hmd {
        presets().iter().find(|h| h.mhd_type == 1).unwrap()
    }

    #[test]
    fn layout_is_the_dialogs() {
        // ResolutionPreset {int, int, const wchar_t*, bool} and HmdPreset {int, 2 x const wchar_t*, ptr, int}.
        assert_eq!(core::mem::size_of::<Resolution>(), 24);
        assert_eq!(core::mem::offset_of!(Resolution, label), 8);
        assert_eq!(core::mem::offset_of!(Resolution, engine_stereo_only), 16);
        assert_eq!(core::mem::size_of::<Hmd>(), 40);
        assert_eq!(core::mem::offset_of!(Hmd, resolutions), 24);
        assert_eq!(core::mem::offset_of!(Hmd, resolution_count), 32);
    }

    #[test]
    fn ladders() {
        assert_eq!(presets().len(), 19);
        assert_eq!(
            presets().iter().map(|h| h.ladder().len()).sum::<usize>(),
            188
        );
        for (i, hmd) in presets().iter().enumerate() {
            assert_eq!(hmd.mhd_type, i as i32);
            assert!(!hmd.ladder().is_empty());
            for r in hmd.ladder() {
                assert_eq!((r.width % 8, r.height % 8), (0, 0), "{}", hmd.label.text());
                // Every label starts with its size.
                assert!(r
                    .label
                    .text()
                    .starts_with(&format!("{} x {}", r.width, r.height)));
                // The engine-stereo-only entries are the steps below 3500 px.
                assert!(!r.engine_stereo_only || r.width.max(r.height) <= 3500);
            }
        }
        assert_eq!(presets()[0].hmd_type.text(), "Q2");
        assert_eq!(presets()[18].label.text(), "HTC Vive Pro / Pro Eye");
        assert_eq!(
            quest3s().ladder()[0].label.text(),
            "1888 x 1904 (cost of the old 1920 x 1880)"
        );
    }

    #[test]
    fn snap_keeps_an_offered_entry_and_moves_the_rest() {
        assert_eq!(snap(1, 1888, 1904, false), None);
        // Engine stereo only: kept with it, moved to the nearest entry with assets without it.
        assert_eq!(snap(1, 2256, 2280, true), None);
        let moved = snap(1, 2256, 2280, false).unwrap();
        assert!(quest3s()
            .ladder()
            .iter()
            .any(|r| (r.width, r.height) == moved && !r.engine_stereo_only));
        assert_eq!(moved, (2520, 2544));
        // A retired size: the nearest in cost.
        assert_eq!(snap(1, 1920, 1880, false), Some((1888, 1904)));
        assert_eq!(snap(1, 0, 1880, false), None);
        // No such headset.
        assert_eq!(snap(99, 1000, 1000, false), None);
    }

    #[test]
    fn nearest_cost_is_by_log_area() {
        let q = quest3s();
        // 2520 x 2544 against 2768 x 2792 (engine stereo only): the area ratio decides.
        assert_eq!(
            nearest_cost_entry(q, 2600, 2620, true).map(|i| q.ladder()[i].width),
            Some(2520)
        );
        assert_eq!(
            nearest_cost_entry(q, 2700, 2720, true).map(|i| q.ladder()[i].width),
            Some(2768)
        );
        assert_eq!(
            nearest_cost_entry(q, 2700, 2720, false).map(|i| q.ladder()[i].width),
            Some(2520)
        );
    }

    #[test]
    fn fit_takes_the_largest_entry_inside_or_scales() {
        assert_eq!(fit(1, 4000, 4000, 3040, 3072, true), None);
        assert_eq!(fit(1, 4000, 4000, 4920, 4968, false), Some((2520, 2544)));
        assert_eq!(fit(1, 4000, 4000, 4920, 4968, true), Some((3344, 3376)));
        // Nothing small enough: the aspect scaled into the box, multiples of 8.
        assert_eq!(fit(1, 1000, 1000, 1888, 1904, false), Some((984, 1000)));
        assert_eq!(fit(99, 1000, 900, 2000, 2000, false), Some((896, 896)));
        assert_eq!(fit(1, 4, 4000, 4920, 4968, false), None);
    }

    #[test]
    fn c_api() {
        let mut n = 0;
        // SAFETY: a local slot.
        let p = unsafe { cvr_launcher_presets(&mut n) };
        assert_eq!((n, p), (19, PRESETS.as_ptr()));
        let (mut w, mut h) = (1920, 1880);
        // SAFETY: local slots.
        assert_eq!(
            unsafe { cvr_launcher_snap_resolution(1, &mut w, &mut h, 0) },
            1
        );
        assert_eq!((w, h), (1888, 1904));
        // SAFETY: local slots.
        assert_eq!(
            unsafe { cvr_launcher_fit_resolution(1, 1000, 1000, &mut w, &mut h, 0) },
            1
        );
        assert_eq!((w, h), (984, 1000));
        // SAFETY: a null entry.
        assert_eq!(unsafe { cvr_launcher_offered(core::ptr::null(), 1) }, 0);
    }
}
