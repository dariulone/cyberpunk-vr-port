#include "Overlay/LauncherDialog.hpp"
#include <windows.h>
#include <commctrl.h>
#include <iterator>
#include <string>
#include <atomic>
#include <algorithm>
#include <cmath>

extern HWND g_gameHwnd;
extern void Log(const char*,...);
static std::atomic<bool> g_launcherOpen{};
static bool g_restoreGameFocus{};

int IsLauncherOpen(){return g_launcherOpen.load(std::memory_order_acquire)?1:0;}

extern "C" void SetWindowResolutionAndPersist(int width, int height);
extern "C" int GetCurrentWindowWidth();
extern "C" int GetCurrentWindowHeight();
extern "C" void SetRuntimeModeAndPersist(int mode);
extern "C" int GetXrRuntimeMode();
extern "C" void SetHmdTypeAndPersist(int hmdType);
extern "C" int GetCurrentHmdType();
extern "C" void SetLauncherDebugAndPersist(int on);
extern "C" int GetLauncherDebug();

#define ID_COMBO_RES     101
#define ID_BUTTON_START  102
#define ID_COMBO_RUNTIME 103
#define ID_COMBO_HMD     104
#define ID_CHECK_DEBUG   105

// --- Visual constants -------------------------------------------------------
static const COLORREF kColBg     = RGB(248, 249, 251); // window background
static const COLORREF kColHeader = RGB(0, 120, 215);   // Windows accent blue
static const COLORREF kColSub    = RGB(120, 124, 130);  // muted gray
static const COLORREF kColLabel  = RGB(45, 48, 54);     // near-black label
static const int kMargin  = 26;
static const int kClientW = 372;
static const int kClientH = 452;   // room for the DEBUG row under the resolution combo
static const int kFieldW  = kClientW - 2 * kMargin;

struct ResolutionPreset {
    int width;
    int height;
    const wchar_t* label;
};

struct HmdPreset {
    int mhdType;
    const wchar_t* hmd_type;
    const wchar_t* label;
    const ResolutionPreset* resolutions;
    int resolutionCount;
};

// --- Per-HMD resolution lists ---
//
// EVERY LADDER IS SHAPED BY ITS HEADSET'S EYE WINDOW, NOT BY ITS PANEL.
//
// Each eye renders exactly its own lens window (include/Runtimes/AsymmetricProjection.hpp Windows(): the
// eye's own horizontal span, one vertical span for both eyes) drawn into the render target edge to edge, so
// the target must have the window's aspect or the pixels are not square -- src/Stereo/AsymProjection.cpp
// then grows the window to the target's aspect, which keeps them square at the price of rendering past the
// lens. The aspect of every entry below is therefore the window's: tan(right) - tan(left) of the wider eye,
// in the head frame (a canted eye turned by its cant first), over tan(up) - tan(down) of the taller.
//
// The windows are the runtimes' own per-eye projections, as hmdq recorded them for the HMD Geometry
// Database (risa2000.github.io/hmdgdb, raw LRBT tangents). Its Quest 3 record is, to the hundredth of a
// degree, the window a Quest 3 reports through Virtual Desktop. A runtime FOV cut (Pimax's
// smaller FOV settings, a headset's reduced-FOV mode) changes the window, not the ladder; the fit keeps the
// pixels square either way, at the cost of a sliver past the lens.
//
// Each entry costs what the entry at its place in the old (panel- or spec-shaped) ladder did, within 0.4%,
// so a pick keeps its frame time while all of its pixels land inside the lens. Both dimensions are
// multiples of 8, every aspect is within 0.25% of the window, and each entry needs a matching
// vrcam_<W>x<H> / vrcam_feed_<W>x<H> asset pair: tools/gen_vrcam_assets.py --prune. A saved resolution
// that is not in its headset's ladder (one these ladders retired) moves to the entry nearest in cost: in
// the dialog, and with the dialog off in SnapLauncherResolution at startup.

// QUEST 2: L-52 / R+45 (mirrored for the other eye), U+48 / D-50 (Oculus runtime 1.53, 2020)
//     window AR = (tan 52 + tan 45) / (tan 48 + tan 50) = 2.279942 / 2.302367 = 0.99026
// The old ladder had the panel's 0.954 (1832 x 1920): 3.6% of every target rendered past the lens.
static const ResolutionPreset kQuest2Resolutions[] = {
    {1864, 1880, L"1864 x 1880 (cost of the 1832 x 1920 panel)"},
    {1712, 1728, L"1712 x 1728"},
    {2080, 2104, L"2080 x 2104"},
    {2600, 2624, L"2600 x 2624"},
    {4168, 4208, L"4168 x 4208"},
    {5088, 5136, L"5088 x 5136"},
    {6104, 6168, L"6104 x 6168"},
};

// QUEST 3S: NOT MEASURED, no record of it exists. It has the Quest 2's Fresnel lenses and 1832 x 1920
// panels, so it takes the Quest 2's window (0.99026); the old ladder was 1.021, wider than tall. The
// runtime's per-eye angles are logged at startup -- if they disagree, rebuild this ladder from them.
static const ResolutionPreset kQuest3SResolutions[] = {
    {1888, 1904, L"1888 x 1904 (cost of the old 1920 x 1880)"},
    {1656, 1672, L"1656 x 1672"},
    {2016, 2040, L"2016 x 2040"},
    {2520, 2544, L"2520 x 2544"},
    {4032, 4072, L"4032 x 4072"},
    {4920, 4968, L"4920 x 4968"},
    {5912, 5968, L"5912 x 5968"},
};

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
static const ResolutionPreset kQuest3Resolutions[] = {
    {1864, 2016, L"1864 x 2016"},
    {2064, 2232, L"2064 x 2232"},
    {2296, 2480, L"2296 x 2480"},
    {2408, 2600, L"2408 x 2600"},
    {2648, 2864, L"2648 x 2864"},
    {2856, 3088, L"2856 x 3088"},
    {3216, 3472, L"3216 x 3472"},
    {3672, 3968, L"3672 x 3968"},
    {4480, 4840, L"4480 x 4840"},
    {5384, 5816, L"5384 x 5816"},
};

// PICO 4 / PICO 4 ULTRA: the runtime reports a symmetric +-52 deg window both ways (Pico 4 through
// SteamVR, 2022), so the square ladder IS the window. The Ultra has no record; it has the same 2160 x 2160
// panels and 105 deg optics and keeps the same ladder.
static const ResolutionPreset kPico4Resolutions[] = {
    {2160, 2160, L"2160 x 2160 (Native)"},
    {1920, 1920, L"1920 x 1920"},
    {2048, 2048, L"2048 x 2048"},
    {2560, 2560, L"2560 x 2560"},
    {3072, 3072, L"3072 x 3072"},
    {4096, 4096, L"4096 x 4096"},
    {5000, 5000, L"5000 x 5000"},
    {6000, 6000, L"6000 x 6000"},
};

static const ResolutionPreset kPico4UltraResolutions[] = {
    {2160, 2160, L"2160 x 2160 (Native)"},
    {1920, 1920, L"1920 x 1920"},
    {2048, 2048, L"2048 x 2048"},
    {2560, 2560, L"2560 x 2560"},
    {3072, 3072, L"3072 x 3072"},
    {4096, 4096, L"4096 x 4096"},
    {5000, 5000, L"5000 x 5000"},
    {6000, 6000, L"6000 x 6000"},
};

// STEAM FRAME (PC): L-59.0 / R+50.4, U+49.5 / D-59.9 and the mirror image for the right eye, within 0.3 deg
// (SteamVR 2.18, 2026)
//     window AR = 2.877668 (the wider eye) / 2.896121 (the taller) = 0.99363
// Near square, but the window sits low and off-centre in the panel; the old square ladder rendered 0.6% past
// the lens.
static const ResolutionPreset kSteamFrameResolutions[] = {
    {2152, 2168, L"2152 x 2168 (cost of the 2160 x 2160 panel)"},
    {1912, 1928, L"1912 x 1928 (Performance)"},
    {2040, 2056, L"2040 x 2056"},
    {2552, 2568, L"2552 x 2568 (Balanced)"},
    {3064, 3080, L"3064 x 3080 (High)"},
    {4080, 4112, L"4080 x 4112 (Ultra)"},
    {4984, 5016, L"4984 x 5016"},
    {5984, 6024, L"5984 x 6024"},
};

// PIMAX CRYSTAL: L-51.7 / R+41.7 (mirrored), U+51.9 / D-51.9, parallel views (SteamVR 1.26, 2023)
//     window AR = (1.264092 + 0.889501) / (2 x 1.274264) = 0.84503 -- taller than wide, square panel or not
// The old near-square ladder (1.0065) rendered 16% of every target past the lens.
// CRYSTAL LIGHT: no record; it has the Crystal's lenses and 2880 x 2880 panels, so it takes the same window.
static const ResolutionPreset kCrystalOGResolutions[] = {
    {2256, 2672, L"2256 x 2672 (cost of the old 2464 x 2448)"},
    {1976, 2344, L"1976 x 2344"},
    {1880, 2224, L"1880 x 2224"},
    {2344, 2776, L"2344 x 2776"},
    {2816, 3328, L"2816 x 3328"},
    {3752, 4440, L"3752 x 4440"},
    {4584, 5424, L"4584 x 5424"},
    {5496, 6504, L"5496 x 6504"},
};

static const ResolutionPreset kCrystalLightResolutions[] = {
    {2256, 2672, L"2256 x 2672 (cost of the old 2464 x 2448)"},
    {1976, 2344, L"1976 x 2344"},
    {1880, 2224, L"1880 x 2224"},
    {2344, 2776, L"2344 x 2776"},
    {2816, 3328, L"2816 x 3328"},
    {3752, 4440, L"3752 x 4440"},
    {4584, 5424, L"4584 x 5424"},
    {5496, 6504, L"5496 x 6504"},
};

// PIMAX CRYSTAL SUPER, 50 PPD optical engine: L-63.5 / R+51.5 (mirrored), U+58.7 / D-58.7 (SteamVR 2.9,
// 2025; 127 deg in all with 103 of overlap -- the engine's published 127)
//     window AR = (2.007429 + 1.258067) / (2 x 1.643018) = 0.99375
static const ResolutionPreset kCrystalSuperResolutions[] = {
    {2448, 2464, L"2448 x 2464 (cost of the old 2464 x 2448)"},
    {2144, 2160, L"2144 x 2160"},
    {2032, 2048, L"2032 x 2048"},
    {2544, 2560, L"2544 x 2560"},
    {3056, 3072, L"3056 x 3072"},
    {4072, 4096, L"4072 x 4096"},
    {4968, 5000, L"4968 x 5000"},
    {5960, 6000, L"5960 x 6000"},
};

// CRYSTAL SUPER ULTRA-WIDE optical engine: NOT MEASURED, DERIVED. Pimax gives 140 deg in all with 90 of
// overlap, so each eye spans outer 70 / inner 45 = 115 deg -- the measured 50 PPD engine's 115.04, turned
// outward (Pimax: the same lens and panel, moved). The vertical is the 50 PPD engine's.
//     window AR = (tan 70 + tan 45) / (2 x 1.643018) = 1.14042
// The runtime's per-eye angles are logged at startup -- if they disagree, rebuild this ladder from them.
static const ResolutionPreset kCrystalWFResolutions[] = {
    {2624, 2304, L"2624 x 2304 (cost of the old 2464 x 2448)"},
    {2296, 2016, L"2296 x 2016"},
    {2176, 1912, L"2176 x 1912"},
    {2728, 2392, L"2728 x 2392"},
    {3272, 2864, L"3272 x 2864"},
    {4360, 3824, L"4360 x 3824"},
    {5320, 4664, L"5320 x 4664"},
    {6384, 5600, L"6384 x 5600"},
};

// VALVE INDEX: each eye L-52.0 / R+51.5, U+54.7 / D-54.6 in its own view, and the views CANTED 5 deg
// outward (SteamVR 1.7, 2019). In the head frame, where both eyes are rendered, the windows are
// L-57.0 / R+46.5 (mirrored):
//     window AR = (tan 57.0 + tan 46.5) / 2.822942 = 0.91970
// against the panel's 0.9 (1440 x 1600).
static const ResolutionPreset kValveIndexResolutions[] = {
    {1456, 1584, L"1456 x 1584 (cost of the 1440 x 1600 panel)"},
    {1272, 1384, L"1272 x 1384"},
    {1088, 1184, L"1088 x 1184"},
    {1816, 1976, L"1816 x 1976"},
    {4144, 4504, L"4144 x 4504"},
    {5056, 5496, L"5056 x 5496"},
    {6064, 6592, L"6064 x 6592"},
};

// PIMAX DREAM AIR (lighthouse): L-55.1 / R+45.5 (mirrored), U+44.5 / D-44.5 (SteamVR 2.16, 2026)
//     window AR = (1.432247 + 1.019034) / (2 x 0.984291) = 1.24520
// The old ladder took a tester's SteamVR lens figures, 99 x 88 deg, as a symmetric frustum (1.21245); the
// eye's window is wider than that, its lens off the panel's centre.
static const ResolutionPreset kPimaxDreamAirResolutions[] = {
    {2536, 2040, L"2536 x 2040 (cost of the old 2504 x 2064)"},
    {2800, 2248, L"2800 x 2248"},
    {3112, 2504, L"3112 x 2504"},
    {4152, 3336, L"4152 x 3336"},
    {5072, 4072, L"5072 x 4072"},
    {6088, 4888, L"6088 x 4888"},
};

// PLAYSTATION VR2 (PC adapter, SteamVR/OpenXR): L-61.5 / R+43.4 (mirrored), U+53.0 / D-53.0 (SteamVR 2.8,
// 2025)
//     window AR = (1.841771 + 0.947191) / (2 x 1.328981) = 1.04929
// The runtime's recommended 3400 x 3468 (0.980) is the panel's shape (2000 x 2040), not the window's; the
// old square ladder rendered 4.7% of every target past the lens.
static const ResolutionPreset kPlayStationVr2Resolutions[] = {
    {1968, 1872, L"1968 x 1872 (Performance)"},
    {2096, 2000, L"2096 x 2000 (Near panel resolution)"},
    {2624, 2496, L"2624 x 2496 (Balanced)"},
    {3144, 3000, L"3144 x 3000 (High)"},
    {3672, 3496, L"3672 x 3496 (Near runtime scale)"},
    {4200, 4000, L"4200 x 4000 (Ultra)"},
    {5120, 4880, L"5120 x 4880"},
    {6144, 5856, L"6144 x 5856"},
};

// BIGSCREEN BEYOND 2 / 2e: identical panels and optics on both models. Each unit is calibrated, so the
// numbers move a little from one to the next; this one (Beyond 2e, SteamVR 2.12, 2025):
//     left eye  L-54.8 / R+42.0, U+44.0 / D-52.9      right eye  L-42.5 / R+54.4, U+43.7 / D-53.2
//     window AR = 2.319957 (the wider eye) / 2.294896 (the taller) = 1.01092
// The old ladder (1.2393) was derived from the published 108 x 96 deg -- but the 108 is both eyes'
// horizontal and the 96 one eye's vertical: 18% of every target rendered past the lens.
static const ResolutionPreset kBigscreenBeyond2Resolutions[] = {
    {1736, 1720, L"1736 x 1720"},
    {1848, 1832, L"1848 x 1832"},
    {2312, 2288, L"2312 x 2288 (cost of the old 2560 x 2064)"},
    {2776, 2744, L"2776 x 2744"},
    {3232, 3200, L"3232 x 3200"},
    {3696, 3656, L"3696 x 3656"},
    {4512, 4464, L"4512 x 4464"},
    {5416, 5360, L"5416 x 5360"},
};

// HP REVERB G2: L-49.6 / R+44.6 (mirrored), U+46.6 / D-46.6, views turned 0.2 deg (SteamVR 1.15, 2020)
//     window AR = 2.166975 (the wider eye) / 2.113323 (the taller) = 1.02539
// It used to share the Pico 4's square ladder.
static const ResolutionPreset kReverbG2Resolutions[] = {
    {2192, 2136, L"2192 x 2136 (cost of the 2160 x 2160 panel)"},
    {1944, 1896, L"1944 x 1896"},
    {2072, 2024, L"2072 x 2024"},
    {2592, 2528, L"2592 x 2528"},
    {3112, 3032, L"3112 x 3032"},
    {4152, 4048, L"4152 x 4048"},
    {5064, 4936, L"5064 x 4936"},
    {6080, 5928, L"6080 x 5928"},
};

// SHIFTALL MEGANEX SUPERLIGHT 8K: no hmdq record of the current drivers; the window is the drivers' own
// projection code. The native driver (CustomHeadsetOpenVR, Shiftall's MeganeXsuperlight8K_Native fork and
// sboys3's 1.3.0) reports a symmetric frustum per eye, parallel views: the "MeganeX8K Default" profile
// reaches 58.0 deg across and 49.7 deg up and down, clamped to the 100 x 96 deg default, plus 0..3 deg
// every session against burn-in (each eye sees its 3552 x 3840 panel turned, 3840 across).
//     window AR = tan 50 / tan 48 = 1.07306, 1.07373 at +3 deg; the ladder aims between, 1.07337
// The stock driver (MeganeX VG3 display redirect, hmdq 2.1.8, January 2025) gave a symmetric 95 x 95 deg,
// a square window; the fit keeps the pixels square there too, at a sliver past the lens. The runtime's
// per-eye angles are logged at startup -- if they disagree, rebuild this ladder from them.
static const ResolutionPreset kMeganeX8KResolutions[] = {
    {3824, 3568, L"3824 x 3568 (cost of the 3552 x 3840 panel)"},
    {1992, 1856, L"1992 x 1856"},
    {2120, 1976, L"2120 x 1976"},
    {2656, 2472, L"2656 x 2472"},
    {3184, 2968, L"3184 x 2968"},
    {4248, 3960, L"4248 x 3960"},
    {5176, 4824, L"5176 x 4824"},
    {6216, 5792, L"6216 x 5792"},
};

// SAMSUNG GALAXY XR: L-54.5 / R+39.9 (mirrored), U+52.6 / D-52.6, parallel views, no hidden-area mask
// (SteamVR 2.13, 2025)
//     window AR = (1.401340 + 0.835410) / (2 x 1.306490) = 0.85601
// The 3552 x 3840 panels (0.925) are wider than the window; a panel-shaped target renders 7.5% past the lens.
// Each entry costs what the Virtual Desktop preset it is named for does (VD's sizes are panel-shaped).
// Rampage, 4224 x 4608, is about 1.19x the panel per side: the native default.
static const ResolutionPreset kGalaxyXrResolutions[] = {
    {4080, 4768, L"4080 x 4768 (VD Rampage, native)"},
    {2928, 3424, L"2928 x 3424 (VD Godlike)"},
    {3504, 4096, L"3504 x 4096 (VD Monster)"},
};

static const HmdPreset kHmdPresets[] = {
    {0, L"Q2",    L"Meta Quest2",     kQuest2Resolutions,       _countof(kQuest2Resolutions)},
    {1, L"Q3S",   L"Meta Quest3s",    kQuest3SResolutions,      _countof(kQuest3SResolutions)},
    {2, L"Q3",    L"Meta Quest3",     kQuest3Resolutions,       _countof(kQuest3Resolutions)},
    {3, L"P4",    L"Pico4",           kPico4Resolutions,        _countof(kPico4Resolutions)},
    {4, L"P4U",   L"Pico4 Ultra",     kPico4UltraResolutions,   _countof(kPico4UltraResolutions)},
    {5, L"CRYOG", L"Crystal OG",      kCrystalOGResolutions,    _countof(kCrystalOGResolutions)},
    {6, L"GRYLI", L"Crystal Light",   kCrystalLightResolutions, _countof(kCrystalLightResolutions)},
    {7, L"CRYSU", L"Crystal Super",   kCrystalSuperResolutions, _countof(kCrystalSuperResolutions)},
    {8, L"CRYWF", L"Crystal Utra Wide", kCrystalWFResolutions,    _countof(kCrystalWFResolutions)},
    {9, L"VINDEX",L"Valve Index",     kValveIndexResolutions,   _countof(kValveIndexResolutions)},
    {10,L"PDA",   L"Pimax Dream Air", kPimaxDreamAirResolutions, _countof(kPimaxDreamAirResolutions)},
    {11,L"PSVR2", L"PlayStation VR2", kPlayStationVr2Resolutions, _countof(kPlayStationVr2Resolutions)},
    {12,L"BSB2",  L"Bigscreen Beyond 2/2e", kBigscreenBeyond2Resolutions, _countof(kBigscreenBeyond2Resolutions)},
    {13,L"SFRAME",L"Steam Frame", kSteamFrameResolutions, _countof(kSteamFrameResolutions)},
    {14,L"RG2",   L"HP Reverb G2", kReverbG2Resolutions, _countof(kReverbG2Resolutions)},
    {15,L"MX8K",  L"MeganeX superlight 8K", kMeganeX8KResolutions, _countof(kMeganeX8KResolutions)},
    {16,L"GXR",   L"Samsung Galaxy XR", kGalaxyXrResolutions, _countof(kGalaxyXrResolutions)},
};

// The ladder entry nearest in cost (pixel count) to w x h.
static int NearestCostEntry(const HmdPreset& hmd, int w, int h) {
    int best = 0;
    double bestDiff = 0.0;
    for (int i = 0; i < hmd.resolutionCount; ++i) {
        const double diff = std::fabs(std::log(double(hmd.resolutions[i].width) * hmd.resolutions[i].height /
                                               (double(w) * h)));
        if (i == 0 || diff < bestDiff) { best = i; bestDiff = diff; }
    }
    return best;
}

int SnapLauncherResolution(int hmdType, int* width, int* height) {
    if (!width || !height || *width <= 0 || *height <= 0) return 0;
    for (const HmdPreset& hmd : kHmdPresets) {
        if (hmd.mhdType != hmdType || hmd.resolutionCount <= 0) continue;
        for (int i = 0; i < hmd.resolutionCount; ++i)
            if (hmd.resolutions[i].width == *width && hmd.resolutions[i].height == *height) return 0;
        const ResolutionPreset& pick = hmd.resolutions[NearestCostEntry(hmd, *width, *height)];
        *width = pick.width;
        *height = pick.height;
        return 1;
    }
    return 0;
}

struct RuntimeOption {
    int mode; // matches xr_runtime: 0 = OpenXR default, 1 = SteamVR
    const wchar_t* label;
};

static const RuntimeOption kRuntimeOptions[] = {
    {1, L"OpenVR  (SteamVR)"},
    {0, L"OpenXR  (VDXR, PimaxOpenXR)"},
};

static HFONT g_fontHeader = nullptr;
static HFONT g_fontSub    = nullptr;
static HFONT g_fontBody   = nullptr;
static HBRUSH g_brushBg   = nullptr;
static HWND g_hHeader  = nullptr;
static HWND g_hSub     = nullptr;
static HWND g_hRuntime = nullptr;
static HWND g_hHmd     = nullptr;   // <-- NUOVO
static HWND g_hRes     = nullptr;
static HWND g_hDebug   = nullptr;

static HFONT MakeFont(int pointSize, int weight) {
    HDC hdc = GetDC(nullptr);
    const int height = -MulDiv(pointSize, GetDeviceCaps(hdc, LOGPIXELSY), 72);
    ReleaseDC(nullptr, hdc);
    return CreateFontW(height, 0, 0, 0, weight, FALSE, FALSE, FALSE,
                       DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                       CLEARTYPE_QUALITY, DEFAULT_PITCH | FF_DONTCARE, L"Segoe UI");
}

static HWND MakeLabel(HWND parent, const wchar_t* text, int x, int y, int w, int h, HFONT font) {
    HWND label = CreateWindowW(L"STATIC", text,
        WS_VISIBLE | WS_CHILD | SS_LEFT,
        x, y, w, h, parent, nullptr, nullptr, nullptr);
    SendMessageW(label, WM_SETFONT, (WPARAM)font, TRUE);
    return label;
}

static HWND MakeCombo(HWND parent, int id, int x, int y, int w) {
    HWND combo = CreateWindowW(WC_COMBOBOXW, L"",
        CBS_DROPDOWNLIST | CBS_HASSTRINGS | WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL,
        x, y, w, 260, parent, (HMENU)(INT_PTR)id, nullptr, nullptr);
    SendMessageW(combo, WM_SETFONT, (WPARAM)g_fontBody, TRUE);
    return combo;
}

// --- NUOVA FUNZIONE: popola la combo risoluzioni in base all'HMD selezionato ---
static void PopulateResolutionCombo(int hmdIndex) {
    if (!g_hRes) return;
    SendMessageW(g_hRes, CB_RESETCONTENT, 0, 0);

    if (hmdIndex < 0 || hmdIndex >= static_cast<int>(std::size(kHmdPresets))) {
        hmdIndex = 0;
    }

    const HmdPreset& hmd = kHmdPresets[hmdIndex];
    const int currentWidth = GetCurrentWindowWidth();
    const int currentHeight = GetCurrentWindowHeight();
    int resSel = 0;
    bool found = false;

    for (int i = 0; i < hmd.resolutionCount; ++i) {
        SendMessageW(g_hRes, CB_ADDSTRING, 0, (LPARAM)hmd.resolutions[i].label);
        if (!found &&
            hmd.resolutions[i].width == currentWidth &&
            hmd.resolutions[i].height == currentHeight) {
            resSel = i;
            found = true;
        }
    }

    // Not in this headset's ladder (another headset's pick, or a size the ladders retired): the entry
    // nearest in cost, so the pick keeps its frame time.
    if (!found && hmd.resolutionCount > 0 && currentWidth > 0 && currentHeight > 0) {
        resSel = NearestCostEntry(hmd, currentWidth, currentHeight);
    }

    SendMessageW(g_hRes, CB_SETCURSEL, resSel, 0);
}

LRESULT CALLBACK LauncherWndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam) {
    static HWND hwndButton;
    switch (msg) {
    case WM_CREATE: {
        g_fontHeader = MakeFont(20, FW_SEMIBOLD);
        g_fontSub    = MakeFont(10, FW_NORMAL);
        g_fontBody   = MakeFont(11, FW_NORMAL);
        int y = kMargin;

        g_hHeader = MakeLabel(hwnd, L"CyberpunkVRPort", kMargin, y, kFieldW, 44, g_fontHeader);
        y += 46;
        g_hSub = MakeLabel(hwnd, L"VR Configuration", kMargin, y, kFieldW, 22, g_fontSub);
        y += 38;

        // --- VR Headset (NUOVO) ---
        MakeLabel(hwnd, L"VR Headset", kMargin, y, kFieldW, 24, g_fontBody);
        y += 26;
        g_hHmd = MakeCombo(hwnd, ID_COMBO_HMD, kMargin, y, kFieldW);
        const int currentHmd = GetCurrentHmdType();
        int hmdSel = 0;
        for (int i = 0; i < static_cast<int>(std::size(kHmdPresets)); ++i) {
            SendMessageW(g_hHmd, CB_ADDSTRING, 0, (LPARAM)kHmdPresets[i].label);
            if (kHmdPresets[i].mhdType == currentHmd) {
                hmdSel = i;
            }
        }
        SendMessageW(g_hHmd, CB_SETCURSEL, hmdSel, 0);
        y += 50;

        // --- VR Runtime ---
        MakeLabel(hwnd, L"VR Runtime", kMargin, y, kFieldW, 24, g_fontBody);
        y += 26;
        g_hRuntime = MakeCombo(hwnd, ID_COMBO_RUNTIME, kMargin, y, kFieldW);
        const int currentRuntime = GetXrRuntimeMode();
        int runtimeSel = 0;
        for (int i = 0; i < static_cast<int>(std::size(kRuntimeOptions)); ++i) {
            SendMessageW(g_hRuntime, CB_ADDSTRING, 0, (LPARAM)kRuntimeOptions[i].label);
            if (kRuntimeOptions[i].mode == currentRuntime) {
                runtimeSel = i;
            }
        }
        SendMessageW(g_hRuntime, CB_SETCURSEL, runtimeSel, 0);
        y += 50;

        // -- Render Resolution ---
        MakeLabel(hwnd, L"Render Resolution (per eye)", kMargin, y, kFieldW, 24, g_fontBody);
        y += 26;
        g_hRes = MakeCombo(hwnd, ID_COMBO_RES, kMargin, y, kFieldW);
        PopulateResolutionCombo(hmdSel);   // <-- Popola in base all'HMD selezionato
        y += 56;

        // --- DEBUG ---
        // Master switch for every probe, census and dump in the mod (see debug_gate.cpp).
        // Off is the normal way to play: with it ticked the log grows by tens of MB and the
        // frame time goes with it, because probes like SightAxisProbe take a mutex thousands
        // of times a frame.
        g_hDebug = CreateWindowW(L"BUTTON", L"DEBUG  (probes + verbose log, slow)",
            WS_TABSTOP | WS_VISIBLE | WS_CHILD | BS_AUTOCHECKBOX,
            kMargin, y, kFieldW, 24,
            hwnd, (HMENU)ID_CHECK_DEBUG, nullptr, nullptr);
        SendMessageW(g_hDebug, WM_SETFONT, (WPARAM)g_fontBody, TRUE);
        SendMessageW(g_hDebug, BM_SETCHECK,
                     GetLauncherDebug() ? BST_CHECKED : BST_UNCHECKED, 0);
        y += 40;

        // --- Start button ---
        hwndButton = CreateWindowW(L"BUTTON", L"Start Game",
            WS_TABSTOP | WS_VISIBLE | WS_CHILD | BS_DEFPUSHBUTTON,
            kMargin, y, kFieldW, 38,
            hwnd, (HMENU)ID_BUTTON_START, nullptr, nullptr);
        SendMessageW(hwndButton, WM_SETFONT, (WPARAM)g_fontBody, TRUE);
        break;
    }
    case WM_CTLCOLORSTATIC: {
        HDC hdc = (HDC)wParam;
        HWND ctrl = (HWND)lParam;
        SetBkMode(hdc, TRANSPARENT);
        if (ctrl == g_hHeader) {
            SetTextColor(hdc, kColHeader);
        } else if (ctrl == g_hSub) {
            SetTextColor(hdc, kColSub);
        } else {
            SetTextColor(hdc, kColLabel);
        }
        return (LRESULT)g_brushBg;
    }
    case WM_COMMAND: {
        const int notif = HIWORD(wParam);
        const int id    = LOWORD(wParam);

        // --- Cambio selezione HMD: aggiorna la lista risoluzioni ---
        if (id == ID_COMBO_HMD && notif == CBN_SELCHANGE) {
            const int hmdIdx = (int)SendMessageW(g_hHmd, CB_GETCURSEL, 0, 0);
            PopulateResolutionCombo(hmdIdx);
            break;
        }

        if (id == ID_BUTTON_START) {
            SetLauncherDebugAndPersist(
                SendMessageW(g_hDebug, BM_GETCHECK, 0, 0) == BST_CHECKED ? 1 : 0);
            // Salva la selezione HMD
            int hmdIdx = (int)SendMessageW(g_hHmd, CB_GETCURSEL, 0, 0);
            if (hmdIdx >= 0 && hmdIdx < static_cast<int>(std::size(kHmdPresets))) {
                SetHmdTypeAndPersist(kHmdPresets[hmdIdx].mhdType);
            }

            int rIdx = (int)SendMessageW(g_hRuntime, CB_GETCURSEL, 0, 0);
            if (rIdx >= 0 && rIdx < static_cast<int>(std::size(kRuntimeOptions))) {
                SetRuntimeModeAndPersist(kRuntimeOptions[rIdx].mode);
            }

            int idx = (int)SendMessageW(g_hRes, CB_GETCURSEL, 0, 0);
            if (idx >= 0) {
                // Recupera l'HMD corrente per leggere la sua lista risoluzioni
                int curHmdIdx = (int)SendMessageW(g_hHmd, CB_GETCURSEL, 0, 0);
                if (curHmdIdx >= 0 && curHmdIdx < static_cast<int>(std::size(kHmdPresets))) {
                    const HmdPreset& hmd = kHmdPresets[curHmdIdx];
                    if (idx < hmd.resolutionCount) {
                        SetWindowResolutionAndPersist(
                            hmd.resolutions[idx].width,
                            hmd.resolutions[idx].height);
                    }
                }
            }
            g_restoreGameFocus=GetForegroundWindow()==hwnd;
            DestroyWindow(hwnd);
        } else if(id==IDCANCEL) {
            g_restoreGameFocus=GetForegroundWindow()==hwnd;
            DestroyWindow(hwnd);
        }
        break;
    }
    case WM_CLOSE:
        g_restoreGameFocus=GetForegroundWindow()==hwnd;
        DestroyWindow(hwnd);
        return 0;
    case WM_DESTROY:
        if (g_fontHeader) { DeleteObject(g_fontHeader); g_fontHeader = nullptr; }
        if (g_fontSub)    { DeleteObject(g_fontSub);    g_fontSub = nullptr; }
        if (g_fontBody)   { DeleteObject(g_fontBody);   g_fontBody = nullptr; }
        break;
    default:
        return DefWindowProcW(hwnd, msg, wParam, lParam);
    }
    return 0;
}

void WaitForLauncherStartup(unsigned int milliseconds) {
    if (!milliseconds) return;
    if (milliseconds > 10000) milliseconds = 10000;
    const ULONGLONG deadline = GetTickCount64() + milliseconds;
    // The visible launcher naturally pumps startup/resize messages. Preserve
    // that opportunity in unattended mode instead of sleeping the UI thread.
    while (GetTickCount64() < deadline) {
        MSG message{};
        for (unsigned int n = 0; n < 64 && PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE); ++n) {
            if (message.message == WM_QUIT) {
                PostQuitMessage(static_cast<int>(message.wParam));
                return;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
            if (GetTickCount64() >= deadline) return;
        }
        const auto now = GetTickCount64();
        if (now >= deadline) return;
        const DWORD remaining = static_cast<DWORD>(deadline - now);
        if (MsgWaitForMultipleObjectsEx(0, nullptr, remaining, QS_ALLINPUT, MWMO_INPUTAVAILABLE) == WAIT_FAILED)
            Sleep(remaining < 10 ? remaining : 10);
    }
}

void ShowLauncherDialog() {
    if(g_launcherOpen.exchange(true,std::memory_order_acq_rel))return;
    g_restoreGameFocus=false;
    struct StartupWindow {
        HWND game{};bool visible{};bool sameThread{};unsigned cursorShows{};
        StartupWindow() {
            DWORD pid{};
            const auto thread=GetWindowThreadProcessId(g_gameHwnd,&pid);
            if(thread && pid==GetCurrentProcessId())game=g_gameHwnd;
            sameThread=thread==GetCurrentThreadId();
            visible=game && IsWindowVisible(game) && !IsIconic(game);
            // Unmap the fullscreen owner while waiting. Hiding preserves its
            // size, unlike minimizing, and releases Wine/X11 fullscreen grabs.
            if(visible) {if(sameThread)ShowWindow(game,SW_HIDE);else ShowWindowAsync(game,SW_HIDE);}
            ClipCursor(nullptr);ReleaseCapture();
            do {++cursorShows;}while(ShowCursor(TRUE)<0 && cursorShows<16);
        }
        ~StartupWindow() {
            g_launcherOpen.store(false,std::memory_order_release);
            if(visible && IsWindow(game)) {
                if(sameThread)ShowWindow(game,SW_SHOWNA);else ShowWindowAsync(game,SW_SHOWNA);
                if(g_restoreGameFocus)SetForegroundWindow(game);
            }
            while(cursorShows){--cursorShows;ShowCursor(FALSE);}
        }
    } startup;
    g_brushBg = CreateSolidBrush(kColBg);
    WNDCLASSW wc = {};
    wc.lpfnWndProc = LauncherWndProc;
    wc.hInstance = GetModuleHandleW(nullptr);
    wc.lpszClassName = L"CyberpunkVRPortLauncherClass";
    wc.hbrBackground = g_brushBg;
    wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
    const auto registered=RegisterClassW(&wc);
    if(!registered && GetLastError()!=ERROR_CLASS_ALREADY_EXISTS) {
        Log("Launcher: RegisterClassW failed (%lu)\n",GetLastError());
        DeleteObject(g_brushBg);g_brushBg=nullptr;return;
    }

    const DWORD style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    const DWORD extended=WS_EX_TOPMOST|WS_EX_APPWINDOW;
    RECT rc = {0, 0, kClientW, kClientH};
    AdjustWindowRectEx(&rc, style, FALSE, extended);
    const int winW = rc.right - rc.left;
    const int winH = rc.bottom - rc.top;
    MONITORINFO monitor{sizeof(monitor)};
    RECT work{};
    if(GetMonitorInfoW(MonitorFromWindow(startup.game,MONITOR_DEFAULTTONEAREST),&monitor))work=monitor.rcWork;
    else SystemParametersInfoW(SPI_GETWORKAREA,0,&work,0);
    const int xPos=work.left+std::max(0L,(work.right-work.left-winW)/2);
    const int yPos=work.top+std::max(0L,(work.bottom-work.top-winH)/2);

    HWND hwnd = CreateWindowExW(
        extended,
        L"CyberpunkVRPortLauncherClass",
        L"CyberpunkVRPort Configuration",
        style,
        xPos, yPos, winW, winH,
        nullptr, nullptr, wc.hInstance, nullptr
    );

    if (hwnd == nullptr) {
        Log("Launcher: CreateWindowExW failed (%lu)\n",GetLastError());
        if(registered)UnregisterClassW(wc.lpszClassName,wc.hInstance);
        if (g_brushBg) { DeleteObject(g_brushBg); g_brushBg = nullptr; }
        return;
    }

    ShowWindow(hwnd, SW_SHOW);
    SetWindowPos(hwnd,HWND_TOPMOST,0,0,0,0,SWP_NOMOVE|SWP_NOSIZE);
    SetForegroundWindow(hwnd);
    SetFocus(g_hHmd);
    UpdateWindow(hwnd);

    bool quit=false;int quitCode=0;
    while(IsWindow(hwnd) && !quit) {
        if(MsgWaitForMultipleObjectsEx(0,nullptr,INFINITE,QS_ALLINPUT,MWMO_INPUTAVAILABLE)==WAIT_FAILED)break;
        MSG msg{};
        while(PeekMessageW(&msg,nullptr,0,0,PM_REMOVE)) {
            if(msg.message==WM_QUIT){quit=true;quitCode=static_cast<int>(msg.wParam);break;}
            if(!IsDialogMessageW(hwnd,&msg)){TranslateMessage(&msg);DispatchMessageW(&msg);}
            if(!IsWindow(hwnd))break;
        }
    }
    if(IsWindow(hwnd))DestroyWindow(hwnd);
    if(quit)PostQuitMessage(quitCode);
    if(registered)UnregisterClassW(wc.lpszClassName,wc.hInstance);
    if (g_brushBg) { DeleteObject(g_brushBg); g_brushBg = nullptr; }
}
