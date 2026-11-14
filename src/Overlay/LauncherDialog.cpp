#include "Overlay/LauncherDialog.hpp"
#include <windows.h>
#include <commctrl.h>
#include <dwmapi.h>
#include <algorithm>
#include <objidl.h>   // gdiplus.h needs IUnknown and IStream, which WIN32_LEAN_AND_MEAN leaves out
// gdiplus.h uses min and max unqualified; the build defines NOMINMAX.
namespace Gdiplus { using std::min; using std::max; }
#include <gdiplus.h>
#include <iterator>
#include <string>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <utility>

#pragma comment(lib, "comctl32.lib")
#pragma comment(lib, "dwmapi.lib")
#pragma comment(lib, "gdiplus.lib")

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
// 1 when this launch renders both eyes through engine stereo (src/Core/VrCore.cpp).
extern "C" int GetLauncherEngineStereo();
// The saved pick is the player's own size, not a ladder entry (custom_res in vrport-launcher.ini).
extern "C" int GetLauncherCustomResolution();
extern "C" void SetLauncherCustomResolution(int on);
// The VR lobby: the main menu in its own world (xr_lobby in vrport.ini).
extern "C" int GetLauncherLobby();
extern "C" void SetLauncherLobbyAndPersist(int on);
extern "C" int GetLauncherInterface();
extern "C" void SetLauncherInterfaceAndPersist(int on);
extern "C" int GetLauncherHandPhysics();
extern "C" void SetLauncherHandPhysicsAndPersist(int on);

#define ID_COMBO_RES     101
#define ID_BUTTON_START  102
#define ID_COMBO_RUNTIME 103
#define ID_COMBO_HMD     104
#define ID_CHECK_DEBUG   105
#define ID_BUTTON_RES_DOWN 106
#define ID_BUTTON_RES_UP   107
#define ID_SLIDER_RES      108
#define ID_EDIT_WIDTH      109
#define ID_EDIT_HEIGHT     110
#define ID_EDIT_ASPECT     111
#define ID_CHECK_LOBBY     112
#define ID_CHECK_INTERFACE 113
#define ID_CHECK_PHYSICS   114

struct ResolutionPreset {
    int width;
    int height;
    const wchar_t* label;
    // Offered only while engine stereo renders both eyes: the old stereo path draws the second eye through a
    // VRCAM view of the pick's size, and these entries have no vrcam_<W>x<H> component.
    bool engineStereoOnly = false;
};
constexpr bool kEngineStereoOnly = true;

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
//
// THE STEPS BELOW 3500 PX ARE AT MOST 1.12 PER SIDE, the Quest 3 ladder's spacing: wherever two neighbours (by
// cost) were more than 1.15 apart, the gap is cut into equal steps, keeping the entries whose larger side is
// 3500 or less (a Pico 4 went 2160 -> 2560 -> 3072 -> 4096). These kEngineStereoOnly entries have the window's
// aspect like the rest but no vrcam assets: they are offered only while engine stereo renders both eyes, and a
// pick of one moves to the nearest entry that has them when it does not.

// QUEST 2: L-52 / R+45 (mirrored for the other eye), U+48 / D-50 (Oculus runtime 1.53, 2020)
//     window AR = (tan 52 + tan 45) / (tan 48 + tan 50) = 2.279942 / 2.302367 = 0.99026
// The old ladder had the panel's 0.954 (1832 x 1920): 3.6% of every target rendered past the lens.
static const ResolutionPreset kQuest2Resolutions[] = {
    {1864, 1880, L"1864 x 1880 (cost of the 1832 x 1920 panel)"},
    {1712, 1728, L"1712 x 1728"},
    {2080, 2104, L"2080 x 2104"},
    {2328, 2352, L"2328 x 2352", kEngineStereoOnly},
    {2600, 2624, L"2600 x 2624"},
    {2856, 2888, L"2856 x 2888", kEngineStereoOnly},
    {3144, 3168, L"3144 x 3168", kEngineStereoOnly},
    {3448, 3480, L"3448 x 3480", kEngineStereoOnly},
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
    {2256, 2280, L"2256 x 2280", kEngineStereoOnly},
    {2520, 2544, L"2520 x 2544"},
    {2768, 2792, L"2768 x 2792", kEngineStereoOnly},
    {3040, 3072, L"3040 x 3072", kEngineStereoOnly},
    {3344, 3376, L"3344 x 3376", kEngineStereoOnly},
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
    {2352, 2352, L"2352 x 2352", kEngineStereoOnly},
    {2560, 2560, L"2560 x 2560"},
    {2808, 2808, L"2808 x 2808", kEngineStereoOnly},
    {3072, 3072, L"3072 x 3072"},
    {3384, 3384, L"3384 x 3384", kEngineStereoOnly},
    {4096, 4096, L"4096 x 4096"},
    {5000, 5000, L"5000 x 5000"},
    {6000, 6000, L"6000 x 6000"},
};

static const ResolutionPreset kPico4UltraResolutions[] = {
    {2160, 2160, L"2160 x 2160 (Native)"},
    {1920, 1920, L"1920 x 1920"},
    {2048, 2048, L"2048 x 2048"},
    {2352, 2352, L"2352 x 2352", kEngineStereoOnly},
    {2560, 2560, L"2560 x 2560"},
    {2808, 2808, L"2808 x 2808", kEngineStereoOnly},
    {3072, 3072, L"3072 x 3072"},
    {3384, 3384, L"3384 x 3384", kEngineStereoOnly},
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
    {2344, 2360, L"2344 x 2360", kEngineStereoOnly},
    {2552, 2568, L"2552 x 2568 (Balanced)"},
    {2800, 2816, L"2800 x 2816", kEngineStereoOnly},
    {3064, 3080, L"3064 x 3080 (High)"},
    {3368, 3392, L"3368 x 3392", kEngineStereoOnly},
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
    {2568, 3040, L"2568 x 3040", kEngineStereoOnly},
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
    {2568, 3040, L"2568 x 3040", kEngineStereoOnly},
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
    {2792, 2808, L"2792 x 2808", kEngineStereoOnly},
    {3056, 3072, L"3056 x 3072"},
    {3360, 3384, L"3360 x 3384", kEngineStereoOnly},
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
    {2984, 2616, L"2984 x 2616", kEngineStereoOnly},
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
    {1176, 1280, L"1176 x 1280", kEngineStereoOnly},
    {1272, 1384, L"1272 x 1384"},
    {1088, 1184, L"1088 x 1184"},
    {1624, 1768, L"1624 x 1768", kEngineStereoOnly},
    {1816, 1976, L"1816 x 1976"},
    {2016, 2192, L"2016 x 2192", kEngineStereoOnly},
    {2232, 2424, L"2232 x 2424", kEngineStereoOnly},
    {2472, 2688, L"2472 x 2688", kEngineStereoOnly},
    {2744, 2984, L"2744 x 2984", kEngineStereoOnly},
    {3040, 3304, L"3040 x 3304", kEngineStereoOnly},
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
    {3424, 2752, L"3424 x 2752", kEngineStereoOnly},
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
    {2344, 2232, L"2344 x 2232", kEngineStereoOnly},
    {2624, 2496, L"2624 x 2496 (Balanced)"},
    {2872, 2736, L"2872 x 2736", kEngineStereoOnly},
    {3144, 3000, L"3144 x 3000 (High)"},
    {3400, 3240, L"3400 x 3240", kEngineStereoOnly},
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
    {2064, 2048, L"2064 x 2048", kEngineStereoOnly},
    {2312, 2288, L"2312 x 2288 (cost of the old 2560 x 2064)"},
    {2536, 2504, L"2536 x 2504", kEngineStereoOnly},
    {2776, 2744, L"2776 x 2744"},
    {2992, 2960, L"2992 x 2960", kEngineStereoOnly},
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
    {2384, 2320, L"2384 x 2320", kEngineStereoOnly},
    {2592, 2528, L"2592 x 2528"},
    {2840, 2768, L"2840 x 2768", kEngineStereoOnly},
    {3112, 3032, L"3112 x 3032"},
    {3424, 3336, L"3424 x 3336", kEngineStereoOnly},
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
    {2376, 2208, L"2376 x 2208", kEngineStereoOnly},
    {2656, 2472, L"2656 x 2472"},
    {2912, 2712, L"2912 x 2712", kEngineStereoOnly},
    {3184, 2968, L"3184 x 2968"},
    {3488, 3256, L"3488 x 3256", kEngineStereoOnly},
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
    {3552, 4144, L"3552 x 4144 (VD Monster)"},
};

// HTC VIVE: L-54.4 / R+46.8 (mirrored), U+55.7 / D-55.9, parallel views (SteamVR 1.10, 2020)
//     window AR = (1.397283 + 1.250510) / (1.464221 + 1.477517) = 0.90008
// The window has the 1080 x 1200 panel's shape, so the ladder is the panel's; 1512 x 1680 is what SteamVR asks
// for at 100%.
static const ResolutionPreset kViveResolutions[] = {
    {1080, 1200, L"1080 x 1200 (panel)"},
    {1176, 1304, L"1176 x 1304", kEngineStereoOnly},
    {1296, 1440, L"1296 x 1440"},
    {1400, 1552, L"1400 x 1552", kEngineStereoOnly},
    {1512, 1680, L"1512 x 1680 (SteamVR 100%)"},
    {1728, 1920, L"1728 x 1920"},
    {1928, 2144, L"1928 x 2144", kEngineStereoOnly},
    {2160, 2400, L"2160 x 2400"},
};

// HTC VIVE PRO: L-53.6 / R+45.3 (mirrored), U+53.5 / D-54.3, parallel views (SteamVR 2.9, 2025)
//     window AR = 2.566067 (the wider eye) / 2.851153 (the taller) = 0.90001
// The panel's shape again (1440 x 1600); 2016 x 2240 is SteamVR's 100%. The Pro Eye has no record; it has the
// same panels and lenses and takes the same ladder.
static const ResolutionPreset kViveProResolutions[] = {
    {1440, 1600, L"1440 x 1600 (panel)"},
    {1152, 1280, L"1152 x 1280"},
    {1288, 1432, L"1288 x 1432", kEngineStereoOnly},
    {1576, 1752, L"1576 x 1752", kEngineStereoOnly},
    {1728, 1920, L"1728 x 1920"},
    {1864, 2072, L"1864 x 2072", kEngineStereoOnly},
    {2016, 2240, L"2016 x 2240 (SteamVR 100%)"},
    {2304, 2560, L"2304 x 2560"},
    {2576, 2864, L"2576 x 2864", kEngineStereoOnly},
    {2880, 3200, L"2880 x 3200"},
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
    {17,L"VIVE",  L"HTC Vive", kViveResolutions, _countof(kViveResolutions)},
    {18,L"VIVEP", L"HTC Vive Pro / Pro Eye", kViveProResolutions, _countof(kViveProResolutions)},
};

// The entries this launch offers: every one with engine stereo, only those with VRCAM assets without it.
static bool Offered(const ResolutionPreset& r) {
    return !r.engineStereoOnly || GetLauncherEngineStereo() != 0;
}

// The offered ladder entry nearest in cost (pixel count) to w x h; -1 when the ladder offers none.
static int NearestCostEntry(const HmdPreset& hmd, int w, int h) {
    int best = -1;
    double bestDiff = 0.0;
    for (int i = 0; i < hmd.resolutionCount; ++i) {
        if (!Offered(hmd.resolutions[i])) continue;
        const double diff = std::fabs(std::log(double(hmd.resolutions[i].width) * hmd.resolutions[i].height /
                                               (double(w) * h)));
        if (best < 0 || diff < bestDiff) { best = i; bestDiff = diff; }
    }
    return best;
}

int SnapLauncherResolution(int hmdType, int* width, int* height) {
    if (!width || !height || *width <= 0 || *height <= 0) return 0;
    for (const HmdPreset& hmd : kHmdPresets) {
        if (hmd.mhdType != hmdType || hmd.resolutionCount <= 0) continue;
        for (int i = 0; i < hmd.resolutionCount; ++i)
            if (hmd.resolutions[i].width == *width && hmd.resolutions[i].height == *height &&
                Offered(hmd.resolutions[i]))
                return 0;
        const int nearest = NearestCostEntry(hmd, *width, *height);
        if (nearest < 0) return 0;
        *width = hmd.resolutions[nearest].width;
        *height = hmd.resolutions[nearest].height;
        return 1;
    }
    return 0;
}

int FitLauncherResolution(int hmdType, int maxWidth, int maxHeight, int* width, int* height) {
    if (!width || !height || *width <= 0 || *height <= 0 || maxWidth < 8 || maxHeight < 8) return 0;
    if (*width <= maxWidth && *height <= maxHeight) return 0;
    for (const HmdPreset& hmd : kHmdPresets) {
        if (hmd.mhdType != hmdType) continue;
        int best = -1;
        for (int i = 0; i < hmd.resolutionCount; ++i) {
            const ResolutionPreset& r = hmd.resolutions[i];
            if (!Offered(r) || r.width > maxWidth || r.height > maxHeight) continue;
            if (best < 0 || double(r.width) * r.height >
                                double(hmd.resolutions[best].width) * hmd.resolutions[best].height)
                best = i;
        }
        if (best >= 0) {
            *width = hmd.resolutions[best].width;
            *height = hmd.resolutions[best].height;
            return 1;
        }
    }
    const double scale = std::fmin(double(maxWidth) / *width, double(maxHeight) / *height);
    *width = int(*width * scale) / 8 * 8;
    *height = int(*height * scale) / 8 * 8;
    return 1;
}

struct RuntimeOption {
    int mode; // matches xr_runtime: 0 = OpenXR default, 1 = SteamVR
    const wchar_t* label;
};

static const RuntimeOption kRuntimeOptions[] = {
    {1, L"OpenVR  (SteamVR)"},
    {0, L"OpenXR  (VDXR, PimaxOpenXR)"},
};

// --- The window ----------------------------------------------------------------------------------------------
//
// A dark window in the game's colours: near-black surfaces and the yellow of its UI as the one accent (the start
// button, a focused field, the entry under the pointer, the debug switch on). Every control is drawn here. The
// game's process runs without visual styles, so the stock combo boxes, check box and button came out in the
// classic look; and a theme would not take these colours anyway. The combo boxes stay real combo boxes (their
// keyboard, their list, the CB_ messages the launcher tests send): the closed field is painted by a subclass,
// the list's items are owner-drawn. Shapes go through GDI+ (anti-aliased corners), text through GDI (ClearType),
// every size through the window's DPI.

static const COLORREF kColBg         = RGB(14, 16, 21);
static const COLORREF kColSurface    = RGB(23, 26, 33);
static const COLORREF kColSurfaceHot = RGB(31, 35, 44);
static const COLORREF kColBorder     = RGB(43, 48, 59);
static const COLORREF kColBorderHot  = RGB(66, 73, 88);
static const COLORREF kColText       = RGB(236, 238, 242);
static const COLORREF kColMuted      = RGB(139, 147, 162);
static const COLORREF kColAccent     = RGB(252, 238, 10);
static const COLORREF kColAccentHot  = RGB(255, 246, 112);
static const COLORREF kColAccentDown = RGB(214, 201, 0);
static const COLORREF kColOnAccent   = RGB(16, 17, 20);

static UINT g_dpi = 96;
static ULONG_PTR g_gdiplus = 0;
static HFONT g_fontTitle  = nullptr;
static HFONT g_fontSub    = nullptr;
static HFONT g_fontLabel  = nullptr;
static HFONT g_fontBody   = nullptr;
static HFONT g_fontButton = nullptr;
static HFONT g_fontHint   = nullptr;
static HBRUSH g_brushBg      = nullptr;
static HBRUSH g_brushSurface = nullptr;
static HWND g_hRuntime = nullptr;
static HWND g_hHmd     = nullptr;
static HWND g_hRes     = nullptr;
static HWND g_hDebug   = nullptr;
static HWND g_hStart   = nullptr;
static HWND g_hot      = nullptr;   // the control under the pointer
static bool g_debugOn  = false;
static int g_clientW = 0, g_clientH = 0;
// Painted by the window itself: the header and the field labels.
static RECT g_rcTitle, g_rcSub, g_rcRule, g_rcLabelHmd, g_rcLabelRuntime, g_rcLabelRes, g_rcNote;

// THE RENDER SIZE, three ways: the headset's ladder (the combo box), a slider with - / + beside it, and the width,
// height and aspect typed. The slider runs over the pixel count (log scale) at the pick's aspect, the ladder's
// entries marked on it; - / + step the width by kResStepPx at that aspect; a typed width or height sets that side
// alone (the aspect follows), a typed aspect keeps the pixel count. A size that is not a ladder entry is the
// player's own: the combo box shows it as its last item ("Custom"), and it is saved with custom_res=1 so the
// startup snap leaves it alone. Engine stereo renders any size; without it the second eye is a VRCAM view of one of
// the ladder's sizes, so every control lands on the ladder entry nearest in cost.
static HWND g_hResDown = nullptr, g_hResUp = nullptr, g_hSlider = nullptr;
static HWND g_hEditW = nullptr, g_hEditH = nullptr, g_hEditAspect = nullptr;
static HWND g_hLobby = nullptr, g_hInterface = nullptr, g_hPhysics = nullptr;
static bool g_lobbyOn = true, g_interfaceOn = true, g_physicsOn = true;
static RECT g_rcFeatures;
static int g_resW = 0, g_resH = 0;         // the pick the controls show
static int g_refW = 0, g_refH = 0;         // the size asked for: the saved one, or the last one the player set
static bool g_refCustom = false;           // ... and whether it was the player's own
static double g_resAspect = 1.0;           // width / height the slider and the steps keep
static int g_sliderPos = 0;
static bool g_sliderDrag = false;
static double g_sliderLo = 1.0, g_sliderHi = 2.0;   // pixel counts at the slider's ends
static RECT g_rcCapW, g_rcCapH, g_rcCapAspect, g_rcFrameW, g_rcFrameH, g_rcFrameAspect;
static constexpr int kSliderSteps = 1000;
static constexpr int kResMinPx = 512, kResMaxPx = 8192, kResStepPx = 32;

static int Px(int v) { return MulDiv(v, int(g_dpi), 96); }

static HFONT MakeFont(int tenthsOfPoint, int weight) {
    return CreateFontW(-MulDiv(tenthsOfPoint, int(g_dpi), 720), 0, 0, 0, weight, FALSE, FALSE, FALSE,
                       DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                       DEFAULT_PITCH | FF_DONTCARE, L"Segoe UI");
}

static Gdiplus::Color ToColor(COLORREF c) { return Gdiplus::Color(255, GetRValue(c), GetGValue(c), GetBValue(c)); }

// A rounded rectangle, filled and outlined (no outline when border == fill); GDI's own when GDI+ did not start.
static void Rounded(HDC dc, const RECT& r, int radius, COLORREF fill, COLORREF border) {
    if (!g_gdiplus) {
        HBRUSH brush = CreateSolidBrush(fill);
        HPEN pen = CreatePen(PS_SOLID, 1, border);
        HGDIOBJ oldBrush = SelectObject(dc, brush), oldPen = SelectObject(dc, pen);
        RoundRect(dc, r.left, r.top, r.right, r.bottom, radius * 2, radius * 2);
        SelectObject(dc, oldBrush);
        SelectObject(dc, oldPen);
        DeleteObject(brush);
        DeleteObject(pen);
        return;
    }
    Gdiplus::Graphics g(dc);
    g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
    const float x = r.left + 0.5f, y = r.top + 0.5f;
    const float w = float(r.right - r.left) - 1.0f, h = float(r.bottom - r.top) - 1.0f;
    const float d = std::min({2.0f * radius, w, h});
    Gdiplus::GraphicsPath path;
    path.AddArc(x, y, d, d, 180, 90);
    path.AddArc(x + w - d, y, d, d, 270, 90);
    path.AddArc(x + w - d, y + h - d, d, d, 0, 90);
    path.AddArc(x, y + h - d, d, d, 90, 90);
    path.CloseFigure();
    Gdiplus::SolidBrush brush(ToColor(fill));
    g.FillPath(&brush, &path);
    if (border != fill) {
        Gdiplus::Pen pen(ToColor(border), 1.0f);
        g.DrawPath(&pen, &path);
    }
}

// The field's arrow: a chevron, pointing up while the list is open.
static void Chevron(HDC dc, int cx, int cy, bool up, COLORREF color) {
    const float s = float(Px(4)), t = up ? -1.0f : 1.0f;
    const Gdiplus::PointF p[3] = {{cx - s, cy - t * s / 2}, {float(cx), cy + t * s / 2}, {cx + s, cy - t * s / 2}};
    if (!g_gdiplus) {
        HPEN pen = CreatePen(PS_SOLID, Px(2), color);
        HGDIOBJ old = SelectObject(dc, pen);
        const POINT q[3] = {{LONG(p[0].X), LONG(p[0].Y)}, {LONG(p[1].X), LONG(p[1].Y)}, {LONG(p[2].X), LONG(p[2].Y)}};
        Polyline(dc, q, 3);
        SelectObject(dc, old);
        DeleteObject(pen);
        return;
    }
    Gdiplus::Graphics g(dc);
    g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
    Gdiplus::Pen pen(ToColor(color), 1.6f * g_dpi / 96.0f);
    pen.SetLineJoin(Gdiplus::LineJoinRound);
    pen.SetStartCap(Gdiplus::LineCapRound);
    pen.SetEndCap(Gdiplus::LineCapRound);
    g.DrawLines(&pen, p, 3);
}

static void Text(HDC dc, const wchar_t* text, RECT r, HFONT font, COLORREF color, UINT flags, int spacing = 0) {
    HGDIOBJ old = SelectObject(dc, font);
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, color);
    SetTextCharacterExtra(dc, spacing);
    DrawTextW(dc, text, -1, &r, flags | DT_NOPREFIX);
    SetTextCharacterExtra(dc, 0);
    SelectObject(dc, old);
}

// Paints `paint` into an off-screen copy of the window's client area, then onto `dc` in one go: no flicker.
template <class Paint>
static void Buffered(HWND hwnd, HDC dc, Paint paint) {
    RECT rc;
    GetClientRect(hwnd, &rc);
    HDC mem = CreateCompatibleDC(dc);
    HBITMAP bmp = CreateCompatibleBitmap(dc, std::max(1L, rc.right), std::max(1L, rc.bottom));
    HGDIOBJ old = SelectObject(mem, bmp);
    paint(mem, rc);
    BitBlt(dc, 0, 0, rc.right, rc.bottom, mem, 0, 0, SRCCOPY);
    SelectObject(mem, old);
    DeleteObject(bmp);
    DeleteDC(mem);
}

static void SetHot(HWND control) {
    if (g_hot == control) return;
    HWND previous = g_hot;
    g_hot = control;
    if (previous) InvalidateRect(previous, nullptr, FALSE);
    if (control) {
        TRACKMOUSEEVENT track{sizeof(track), TME_LEAVE, control, 0};
        TrackMouseEvent(&track);
        InvalidateRect(control, nullptr, FALSE);
    }
}

static std::wstring ComboItemText(HWND combo, int index) {
    const LRESULT length = index < 0 ? CB_ERR : SendMessageW(combo, CB_GETLBTEXTLEN, index, 0);
    if (length == CB_ERR) return {};
    std::wstring text(size_t(length) + 1, L'\0');
    SendMessageW(combo, CB_GETLBTEXT, index, reinterpret_cast<LPARAM>(text.data()));
    text.resize(size_t(length));
    return text;
}

// The closed combo box: a rounded field with the picked entry and a chevron. The accent outlines it while it has
// the focus or its list is open.
static void PaintField(HWND combo, HDC dc, const RECT& rc) {
    FillRect(dc, &rc, g_brushBg);
    const bool open = SendMessageW(combo, CB_GETDROPPEDSTATE, 0, 0) != 0;
    const bool focus = GetFocus() == combo;
    const bool hot = g_hot == combo;
    Rounded(dc, rc, Px(6), hot || open ? kColSurfaceHot : kColSurface,
            focus || open ? kColAccent : hot ? kColBorderHot : kColBorder);
    RECT text = rc;
    text.left += Px(14);
    text.right -= Px(40);
    const std::wstring label = ComboItemText(combo, int(SendMessageW(combo, CB_GETCURSEL, 0, 0)));
    Text(dc, label.c_str(), text, g_fontBody, kColText, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    Chevron(dc, rc.right - Px(20), (rc.top + rc.bottom) / 2, open, focus || open ? kColAccent : kColMuted);
}

static LRESULT CALLBACK FieldProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam, UINT_PTR, DWORD_PTR) {
    switch (msg) {
    case WM_PAINT: {
        PAINTSTRUCT ps;
        HDC dc = BeginPaint(hwnd, &ps);
        Buffered(hwnd, dc, [hwnd](HDC mem, const RECT& rc) { PaintField(hwnd, mem, rc); });
        EndPaint(hwnd, &ps);
        return 0;
    }
    case WM_PRINTCLIENT: {
        RECT rc;
        GetClientRect(hwnd, &rc);
        PaintField(hwnd, reinterpret_cast<HDC>(wParam), rc);
        return 0;
    }
    case WM_ERASEBKGND:
        return 1;
    case WM_MOUSEMOVE:
        SetHot(hwnd);
        break;
    case WM_MOUSELEAVE:
        if (g_hot == hwnd) SetHot(nullptr);
        break;
    case WM_SETFOCUS:
    case WM_KILLFOCUS: {
        const LRESULT result = DefSubclassProc(hwnd, msg, wParam, lParam);
        InvalidateRect(hwnd, nullptr, FALSE);
        return result;
    }
    case WM_NCDESTROY:
        RemoveWindowSubclass(hwnd, FieldProc, 1);
        break;
    }
    return DefSubclassProc(hwnd, msg, wParam, lParam);
}

// The open list: its one-pixel frame in the window's border colour instead of black.
static LRESULT CALLBACK ListProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam, UINT_PTR, DWORD_PTR) {
    if (msg == WM_NCPAINT) {
        const LRESULT result = DefSubclassProc(hwnd, msg, wParam, lParam);
        HDC dc = GetWindowDC(hwnd);
        RECT r;
        GetWindowRect(hwnd, &r);
        OffsetRect(&r, -r.left, -r.top);
        HBRUSH frame = CreateSolidBrush(kColBorderHot);
        FrameRect(dc, &r, frame);
        DeleteObject(frame);
        ReleaseDC(hwnd, dc);
        return result;
    }
    if (msg == WM_NCDESTROY) RemoveWindowSubclass(hwnd, ListProc, 1);
    return DefSubclassProc(hwnd, msg, wParam, lParam);
}

// The start button and the debug switch: pointer tracking, and the switch's own check state (an owner-drawn
// button keeps none) behind BM_GETCHECK / BM_SETCHECK.
static LRESULT CALLBACK ButtonProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam, UINT_PTR, DWORD_PTR) {
    switch (msg) {
    case BM_GETCHECK:
        if (hwnd == g_hDebug) return g_debugOn ? BST_CHECKED : BST_UNCHECKED;
        if (hwnd == g_hLobby) return g_lobbyOn ? BST_CHECKED : BST_UNCHECKED;
        if (hwnd == g_hInterface) return g_interfaceOn ? BST_CHECKED : BST_UNCHECKED;
        if (hwnd == g_hPhysics) return g_physicsOn ? BST_CHECKED : BST_UNCHECKED;
        break;
    case BM_SETCHECK:
        if (hwnd == g_hDebug || hwnd == g_hLobby || hwnd == g_hInterface || hwnd == g_hPhysics) {
            bool& checked = hwnd == g_hDebug ? g_debugOn : hwnd == g_hLobby ? g_lobbyOn :
                            hwnd == g_hInterface ? g_interfaceOn : g_physicsOn;
            checked = wParam == BST_CHECKED;
            InvalidateRect(hwnd, nullptr, FALSE);
            return 0;
        }
        break;
    case WM_MOUSEMOVE:
        SetHot(hwnd);
        break;
    case WM_MOUSELEAVE:
        if (g_hot == hwnd) SetHot(nullptr);
        break;
    case WM_NCDESTROY:
        RemoveWindowSubclass(hwnd, ButtonProc, 1);
        break;
    }
    return DefSubclassProc(hwnd, msg, wParam, lParam);
}

static HWND MakeCombo(HWND parent, int id, int x, int y, int w) {
    HWND combo = CreateWindowW(WC_COMBOBOXW, L"",
        CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS | WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL,
        x, y, w, Px(300), parent, (HMENU)(INT_PTR)id, nullptr, nullptr);
    SendMessageW(combo, WM_SETFONT, (WPARAM)g_fontBody, TRUE);
    SetWindowSubclass(combo, FieldProc, 1, 0);
    COMBOBOXINFO info{sizeof(info)};
    if (GetComboBoxInfo(combo, &info) && info.hwndList) SetWindowSubclass(info.hwndList, ListProc, 1, 0);
    return combo;
}

static HWND MakeButton(HWND parent, int id, const wchar_t* text, const RECT& r) {
    HWND button = CreateWindowW(L"BUTTON", text, WS_TABSTOP | WS_VISIBLE | WS_CHILD | BS_OWNERDRAW,
        r.left, r.top, r.right - r.left, r.bottom - r.top, parent, (HMENU)(INT_PTR)id, nullptr, nullptr);
    SendMessageW(button, WM_SETFONT, (WPARAM)g_fontBody, TRUE);
    SetWindowSubclass(button, ButtonProc, 1, 0);
    return button;
}

// The open list shows every entry it can (no scroll bar on a short ladder): a classic combo box takes the list's
// height from its window's, a themed one from CB_SETMINVISIBLE.
static void SizeList(HWND combo) {
    const int count = std::max(1, int(SendMessageW(combo, CB_GETCOUNT, 0, 0)));
    const int visible = std::min(count, 20);
    SendMessageW(combo, CB_SETMINVISIBLE, visible, 0);
    RECT r;
    GetWindowRect(combo, &r);
    const int item = int(SendMessageW(combo, CB_GETITEMHEIGHT, 0, 0));
    SetWindowPos(combo, nullptr, 0, 0, r.right - r.left, (r.bottom - r.top) + visible * item + 2,
                 SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
}

static const HmdPreset* SelectedHmd() {
    const int index = g_hHmd ? int(SendMessageW(g_hHmd, CB_GETCURSEL, 0, 0)) : -1;
    return index >= 0 && index < int(std::size(kHmdPresets)) ? &kHmdPresets[index] : nullptr;
}

// The ladder entry the resolution field shows (its item data: the entry's index in the headset's ladder).
static const ResolutionPreset* SelectedResolution() {
    const HmdPreset* hmd = SelectedHmd();
    const int item = g_hRes ? int(SendMessageW(g_hRes, CB_GETCURSEL, 0, 0)) : -1;
    if (!hmd || item < 0) return nullptr;
    const int entry = int(SendMessageW(g_hRes, CB_GETITEMDATA, item, 0));
    return entry >= 0 && entry < hmd->resolutionCount ? &hmd->resolutions[entry] : nullptr;
}

// --- The render size: the pick and the controls that show it ---------------------------------------------------

static bool OwnSizesAllowed() { return GetLauncherEngineStereo() != 0; }

static int RoundSide(double v) {
    const int r = int(std::lround(v / 8.0)) * 8;   // both sides multiples of 8, as every ladder entry
    return std::clamp(r, kResMinPx, kResMaxPx);
}

// The offered ladder entry of exactly w x h, or -1.
static int LadderEntryOf(const HmdPreset* hmd, int w, int h) {
    if (!hmd) return -1;
    for (int i = 0; i < hmd->resolutionCount; ++i)
        if (Offered(hmd->resolutions[i]) && hmd->resolutions[i].width == w && hmd->resolutions[i].height == h)
            return i;
    return -1;
}

static int ComboItemOfEntry(int entry) {
    const int count = int(SendMessageW(g_hRes, CB_GETCOUNT, 0, 0));
    for (int item = 0; item < count; ++item)
        if (int(SendMessageW(g_hRes, CB_GETITEMDATA, item, 0)) == entry) return item;
    return -1;
}

// The slider's ends: the ladder's cheapest and dearest offered entries, with room on both sides for own sizes when
// engine stereo takes them, and always the pick itself.
static void ResetSliderRange() {
    const HmdPreset* hmd = SelectedHmd();
    double lo = 0.0, hi = 0.0;
    if (hmd)
        for (int i = 0; i < hmd->resolutionCount; ++i) {
            if (!Offered(hmd->resolutions[i])) continue;
            const double cost = double(hmd->resolutions[i].width) * hmd->resolutions[i].height;
            lo = lo > 0.0 ? std::min(lo, cost) : cost;
            hi = std::max(hi, cost);
        }
    if (hi <= 0.0) { lo = 1024.0 * 1024.0; hi = 6144.0 * 6144.0; }
    if (OwnSizesAllowed()) { lo *= 0.5; hi *= 1.3; }
    const double pick = double(g_resW) * g_resH;
    if (pick > 0.0) { lo = std::min(lo, pick); hi = std::max(hi, pick); }
    if (hi <= lo * 1.01) hi = lo * 2.0;
    g_sliderLo = lo;
    g_sliderHi = hi;
}

static int SliderPosOf(double cost) {
    const double t = (std::log(cost) - std::log(g_sliderLo)) / (std::log(g_sliderHi) - std::log(g_sliderLo));
    return std::clamp(int(std::lround(t * kSliderSteps)), 0, kSliderSteps);
}

static double SliderCostAt(int pos) {
    return std::exp(std::log(g_sliderLo) + (std::log(g_sliderHi) - std::log(g_sliderLo)) * pos / kSliderSteps);
}

static void SetEditText(HWND edit, const wchar_t* text) {
    if (edit) SetWindowTextW(edit, text);
}

// The fields show the pick (the aspect to four places).
static void ShowPickInFields() {
    wchar_t text[32];
    swprintf_s(text, L"%d", g_resW);
    SetEditText(g_hEditW, text);
    swprintf_s(text, L"%d", g_resH);
    SetEditText(g_hEditH, text);
    swprintf_s(text, L"%.4f", g_resAspect);
    SetEditText(g_hEditAspect, text);
}

// The combo box shows the pick: its ladder entry, or a last item "Custom  W x H" (item data -1) for an own size.
static void ShowPickInCombo() {
    if (!g_hRes) return;
    for (int item = int(SendMessageW(g_hRes, CB_GETCOUNT, 0, 0)) - 1; item >= 0; --item)
        if (int(SendMessageW(g_hRes, CB_GETITEMDATA, item, 0)) < 0) SendMessageW(g_hRes, CB_DELETESTRING, item, 0);
    const int entry = LadderEntryOf(SelectedHmd(), g_resW, g_resH);
    int item = entry >= 0 ? ComboItemOfEntry(entry) : -1;
    if (item < 0) {
        wchar_t text[64];
        swprintf_s(text, L"Custom  %d x %d", g_resW, g_resH);
        item = int(SendMessageW(g_hRes, CB_ADDSTRING, 0, (LPARAM)text));
        SendMessageW(g_hRes, CB_SETITEMDATA, item, LPARAM(-1));
    }
    SendMessageW(g_hRes, CB_SETCURSEL, item, 0);
    SizeList(g_hRes);
    InvalidateRect(g_hRes, nullptr, FALSE);
}

static void RepaintResolutionText() {
    if (HWND parent = g_hRes ? GetParent(g_hRes) : nullptr) {
        InvalidateRect(parent, &g_rcLabelRes, FALSE);
        InvalidateRect(parent, &g_rcNote, FALSE);
    }
}

// Where a change to the pick came from: each source leaves its own control as the player set it.
enum class PickFrom { Ladder, Slider, Step, Side, Aspect };

// Makes w x h the pick. Without engine stereo it lands on the ladder entry nearest in cost.
static void SetPick(int w, int h, PickFrom from, double aspect = 0.0) {
    const HmdPreset* hmd = SelectedHmd();
    w = RoundSide(w);
    h = RoundSide(h);
    if (!OwnSizesAllowed() && hmd && LadderEntryOf(hmd, w, h) < 0) {
        const int nearest = NearestCostEntry(*hmd, w, h);
        if (nearest >= 0) { w = hmd->resolutions[nearest].width; h = hmd->resolutions[nearest].height; aspect = 0.0; }
    }
    g_resW = w;
    g_resH = h;
    if (aspect > 0.0) g_resAspect = aspect;
    else if (from != PickFrom::Slider && from != PickFrom::Step) g_resAspect = double(w) / h;
    g_refW = w;
    g_refH = h;
    g_refCustom = LadderEntryOf(hmd, w, h) < 0;
    if (from != PickFrom::Ladder) ShowPickInCombo();
    else if (g_hRes) {
        // a ladder entry picked: an own size listed before goes from the list
        for (int item = int(SendMessageW(g_hRes, CB_GETCOUNT, 0, 0)) - 1; item >= 0; --item)
            if (int(SendMessageW(g_hRes, CB_GETITEMDATA, item, 0)) < 0) SendMessageW(g_hRes, CB_DELETESTRING, item, 0);
        const int item = ComboItemOfEntry(LadderEntryOf(hmd, w, h));
        if (item >= 0) SendMessageW(g_hRes, CB_SETCURSEL, item, 0);
        SizeList(g_hRes);
    }
    if (from != PickFrom::Slider) {
        ResetSliderRange();
        g_sliderPos = SliderPosOf(double(w) * h);
    }
    if (g_hSlider) InvalidateRect(g_hSlider, nullptr, FALSE);
    ShowPickInFields();
    RepaintResolutionText();
}

// The slider moved to `pos`: that pixel count at the kept aspect. Within 2 % of a ladder entry of the same aspect
// it takes the entry itself (the marks hold the knob).
static void SliderMoved(int pos) {
    g_sliderPos = std::clamp(pos, 0, kSliderSteps);
    const double cost = SliderCostAt(g_sliderPos), aspect = g_resAspect;
    int w = RoundSide(std::sqrt(cost * aspect)), h = RoundSide(w / aspect);
    if (const HmdPreset* hmd = SelectedHmd())
        for (int i = 0; i < hmd->resolutionCount; ++i) {
            const ResolutionPreset& r = hmd->resolutions[i];
            if (!Offered(r)) continue;
            const double c = double(r.width) * r.height, a = double(r.width) / r.height;
            if (std::fabs(std::log(c / cost)) < 0.02 && std::fabs(a / aspect - 1.0) < 0.004) { w = r.width; h = r.height; }
        }
    if (!OwnSizesAllowed()) {
        SetPick(w, h, PickFrom::Ladder);   // the nearest entry; the knob jumps onto its mark
        g_sliderPos = SliderPosOf(double(g_resW) * g_resH);
        ShowPickInCombo();
        return;
    }
    SetPick(w, h, PickFrom::Slider);
}

// - / +: the width by kResStepPx at the kept aspect; without engine stereo the next ladder entry by cost.
static void StepPick(int direction) {
    const HmdPreset* hmd = SelectedHmd();
    if (!OwnSizesAllowed() && hmd) {
        const double now = double(g_resW) * g_resH;
        int best = -1;
        for (int i = 0; i < hmd->resolutionCount; ++i) {
            const ResolutionPreset& r = hmd->resolutions[i];
            const double c = double(r.width) * r.height;
            if (!Offered(r) || (direction > 0 ? c <= now : c >= now)) continue;
            const double bestCost = best < 0 ? 0.0 : double(hmd->resolutions[best].width) * hmd->resolutions[best].height;
            if (best < 0 || (direction > 0 ? c < bestCost : c > bestCost)) best = i;
        }
        if (best >= 0) SetPick(hmd->resolutions[best].width, hmd->resolutions[best].height, PickFrom::Ladder);
        ShowPickInCombo();
        return;
    }
    const int w = RoundSide(g_resW + direction * kResStepPx);
    SetPick(w, RoundSide(w / g_resAspect), PickFrom::Step);
}

// "16:9", "16/9", "16x9", "0.926" or "0,926" -> width / height; 0 when it is none of them or out of range.
static double ParseAspect(std::wstring text) {
    std::replace(text.begin(), text.end(), L',', L'.');
    const size_t split = text.find_first_of(L":/xX");
    double value = 0.0;
    if (split != std::wstring::npos) {
        const double a = _wtof(text.substr(0, split).c_str()), b = _wtof(text.substr(split + 1).c_str());
        value = b > 0.0 ? a / b : 0.0;
    } else {
        value = _wtof(text.c_str());
    }
    return value >= 0.25 && value <= 4.0 ? value : 0.0;
}

static std::wstring WindowText(HWND hwnd) {
    const int length = GetWindowTextLengthW(hwnd);
    std::wstring text(size_t(length) + 1, L'\0');
    GetWindowTextW(hwnd, text.data(), length + 1);
    text.resize(size_t(length));
    return text;
}

// A field's text taken (Enter or the field left): a width or a height sets that side, an aspect keeps the pixel
// count. Anything unreadable puts the pick back in the field.
static void CommitField(HWND edit) {
    const std::wstring text = WindowText(edit);
    if (edit == g_hEditAspect) {
        const double aspect = ParseAspect(text);
        if (aspect > 0.0 && std::fabs(aspect - g_resAspect) > 1e-6) {
            const double cost = double(g_resW) * g_resH;
            const int w = RoundSide(std::sqrt(cost * aspect));
            SetPick(w, RoundSide(w / aspect), PickFrom::Aspect, aspect);
            return;
        }
    } else {
        const int value = _wtoi(text.c_str());
        if (value > 0) {
            const int w = edit == g_hEditW ? value : g_resW, h = edit == g_hEditH ? value : g_resH;
            if (RoundSide(w) != g_resW || RoundSide(h) != g_resH) { SetPick(w, h, PickFrom::Side); return; }
        }
    }
    ShowPickInFields();
}

// The resolution field lists the headset's ladder, the entries this launch offers, and the pick shows in every
// resolution control. The size asked for (g_ref*) carries over a change of headset: kept when it is in the new
// ladder or the player's own (with engine stereo), else the entry nearest in cost, so the pick keeps its frame time.
static void PopulateResolutionCombo(int hmdIndex) {
    if (!g_hRes) return;
    SendMessageW(g_hRes, CB_RESETCONTENT, 0, 0);

    if (hmdIndex < 0 || hmdIndex >= static_cast<int>(std::size(kHmdPresets))) {
        hmdIndex = 0;
    }

    const HmdPreset& hmd = kHmdPresets[hmdIndex];
    for (int i = 0; i < hmd.resolutionCount; ++i) {
        if (!Offered(hmd.resolutions[i])) continue;
        const int item = int(SendMessageW(g_hRes, CB_ADDSTRING, 0, (LPARAM)hmd.resolutions[i].label));
        SendMessageW(g_hRes, CB_SETITEMDATA, item, i);
    }

    int w = g_refW, h = g_refH;
    const bool keepOwn = g_refCustom && OwnSizesAllowed() && w > 0 && h > 0;
    if (LadderEntryOf(&hmd, w, h) < 0 && !keepOwn && hmd.resolutionCount > 0) {
        // Not in this headset's ladder (another headset's pick, a size the ladders retired, or one only engine
        // stereo offers): the entry nearest in cost.
        const int nearest = w > 0 && h > 0 ? NearestCostEntry(hmd, w, h) : -1;
        const int entry = nearest >= 0 ? nearest : int(SendMessageW(g_hRes, CB_GETITEMDATA, 0, 0));
        if (entry >= 0) { w = hmd.resolutions[entry].width; h = hmd.resolutions[entry].height; }
    }
    if (w <= 0 || h <= 0) { w = 2048; h = 2048; }
    g_resW = w;
    g_resH = h;
    g_resAspect = double(w) / h;
    ShowPickInCombo();
    ResetSliderRange();
    g_sliderPos = SliderPosOf(double(w) * h);
    if (g_hSlider) InvalidateRect(g_hSlider, nullptr, FALSE);
    ShowPickInFields();
    RepaintResolutionText();
}

// --- The slider ---------------------------------------------------------------------------------------------------

static void SliderTrack(const RECT& rc, int& left, int& right, int& cy) {
    const int knob = Px(9);
    left = rc.left + knob + Px(1);
    right = rc.right - knob - Px(1);
    cy = (rc.top + rc.bottom) / 2;
}

static void PaintSlider(HWND hwnd, HDC dc, const RECT& rc) {
    FillRect(dc, &rc, g_brushBg);
    int left, right, cy;
    SliderTrack(rc, left, right, cy);
    const bool hot = g_hot == hwnd, focus = GetFocus() == hwnd;
    const RECT track{left, cy - Px(3), right, cy + Px(3)};
    Rounded(dc, track, Px(3), kColSurface, hot ? kColBorderHot : kColBorder);
    const int x = left + MulDiv(right - left, g_sliderPos, kSliderSteps);
    if (x > left + Px(2)) {
        const RECT fill{left, cy - Px(3), x, cy + Px(3)};
        Rounded(dc, fill, Px(3), kColAccent, kColAccent);
    }
    // the ladder's entries, marked under the track
    if (const HmdPreset* hmd = SelectedHmd()) {
        HBRUSH mark = CreateSolidBrush(kColBorderHot);
        for (int i = 0; i < hmd->resolutionCount; ++i) {
            const ResolutionPreset& r = hmd->resolutions[i];
            if (!Offered(r)) continue;
            const int tx = left + MulDiv(right - left, SliderPosOf(double(r.width) * r.height), kSliderSteps);
            const RECT tick{tx, cy + Px(7), tx + std::max(1, Px(1)), cy + Px(11)};
            FillRect(dc, &tick, mark);
        }
        DeleteObject(mark);
    }
    const int k = Px(9);
    const RECT knob{x - k, cy - k, x + k, cy + k};
    const COLORREF fill = g_sliderDrag || hot ? kColAccentHot : kColText;
    Rounded(dc, knob, k, fill, focus ? kColAccent : fill);
    if (focus) {
        RECT ring = knob;
        InflateRect(&ring, Px(3), Px(3));
        Gdiplus::Graphics g(dc);
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        Gdiplus::Pen pen(ToColor(kColAccent), 1.5f * g_dpi / 96.0f);
        if (g_gdiplus) g.DrawEllipse(&pen, float(ring.left), float(ring.top), float(ring.right - ring.left - 1),
                                     float(ring.bottom - ring.top - 1));
    }
}

static void SliderToPoint(HWND hwnd, int x) {
    RECT rc;
    GetClientRect(hwnd, &rc);
    int left, right, cy;
    SliderTrack(rc, left, right, cy);
    SliderMoved(right > left ? MulDiv(std::clamp(x, left, right) - left, kSliderSteps, right - left) : 0);
}

static LRESULT CALLBACK SliderProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam) {
    switch (msg) {
    case WM_GETDLGCODE:
        return DLGC_WANTARROWS;
    case WM_ERASEBKGND:
        return 1;
    case WM_PAINT: {
        PAINTSTRUCT ps;
        HDC dc = BeginPaint(hwnd, &ps);
        Buffered(hwnd, dc, [hwnd](HDC mem, const RECT& rc) { PaintSlider(hwnd, mem, rc); });
        EndPaint(hwnd, &ps);
        return 0;
    }
    case WM_LBUTTONDOWN:
        SetFocus(hwnd);
        SetCapture(hwnd);
        g_sliderDrag = true;
        SliderToPoint(hwnd, short(LOWORD(lParam)));
        return 0;
    case WM_MOUSEMOVE:
        SetHot(hwnd);
        if (g_sliderDrag) SliderToPoint(hwnd, short(LOWORD(lParam)));
        return 0;
    case WM_LBUTTONUP:
        if (g_sliderDrag) { g_sliderDrag = false; ReleaseCapture(); InvalidateRect(hwnd, nullptr, FALSE); }
        return 0;
    case WM_CAPTURECHANGED:
        g_sliderDrag = false;
        InvalidateRect(hwnd, nullptr, FALSE);
        return 0;
    case WM_MOUSELEAVE:
        if (g_hot == hwnd) SetHot(nullptr);
        return 0;
    case WM_KEYDOWN:
        // arrows: one step of the - / + buttons; Page Up / Down: a tenth of the slider; Home / End: its ends
        switch (wParam) {
        case VK_LEFT: case VK_DOWN: StepPick(-1); return 0;
        case VK_RIGHT: case VK_UP: StepPick(+1); return 0;
        case VK_PRIOR: SliderMoved(g_sliderPos + kSliderSteps / 10); return 0;
        case VK_NEXT: SliderMoved(g_sliderPos - kSliderSteps / 10); return 0;
        case VK_HOME: SliderMoved(0); return 0;
        case VK_END: SliderMoved(kSliderSteps); return 0;
        }
        break;
    case WM_SETFOCUS:
    case WM_KILLFOCUS:
        InvalidateRect(hwnd, nullptr, FALSE);
        return 0;
    }
    return DefWindowProcW(hwnd, msg, wParam, lParam);
}

// --- The size fields ----------------------------------------------------------------------------------------------

// Enter takes the field (not the dialog's Start), as does leaving it; focus outlines the field's frame.
static LRESULT CALLBACK EditProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam, UINT_PTR, DWORD_PTR) {
    switch (msg) {
    case WM_GETDLGCODE:
        if (lParam && reinterpret_cast<const MSG*>(lParam)->message == WM_KEYDOWN && wParam == VK_RETURN)
            return DLGC_WANTALLKEYS | DefSubclassProc(hwnd, msg, wParam, lParam);
        break;
    case WM_KEYDOWN:
        if (wParam == VK_RETURN) {
            CommitField(hwnd);
            SendMessageW(hwnd, EM_SETSEL, 0, -1);
            return 0;
        }
        break;
    case WM_CHAR:
        if (wParam == L'\r') return 0;
        break;
    case WM_SETFOCUS: {
        const LRESULT result = DefSubclassProc(hwnd, msg, wParam, lParam);
        PostMessageW(hwnd, EM_SETSEL, 0, -1);
        InvalidateRect(GetParent(hwnd), nullptr, FALSE);
        return result;
    }
    case WM_KILLFOCUS: {
        const LRESULT result = DefSubclassProc(hwnd, msg, wParam, lParam);
        CommitField(hwnd);
        InvalidateRect(GetParent(hwnd), nullptr, FALSE);
        return result;
    }
    case WM_NCDESTROY:
        RemoveWindowSubclass(hwnd, EditProc, 1);
        break;
    }
    return DefSubclassProc(hwnd, msg, wParam, lParam);
}

static HWND MakeEdit(HWND parent, int id, const RECT& frame) {
    const int textH = Px(22);
    const int top = (frame.top + frame.bottom - textH) / 2;
    HWND edit = CreateWindowExW(0, L"EDIT", L"", WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_CENTER | ES_AUTOHSCROLL,
        frame.left + Px(8), top, (frame.right - frame.left) - Px(16), textH, parent, (HMENU)(INT_PTR)id, nullptr,
        nullptr);
    SendMessageW(edit, WM_SETFONT, (WPARAM)g_fontBody, TRUE);
    SendMessageW(edit, EM_SETLIMITTEXT, 12, 0);
    SetWindowSubclass(edit, EditProc, 1, 0);
    return edit;
}

static void PaintWindow(HWND hwnd, HDC dc, const RECT& rc) {
    FillRect(dc, &rc, g_brushBg);
    Text(dc, L"cyberpunk-vr-port", g_rcTitle, g_fontTitle, kColText, DT_LEFT | DT_TOP | DT_SINGLELINE);
    Text(dc, L"Configure the headset, the runtime and the render size, then start the game.", g_rcSub, g_fontSub,
         kColMuted, DT_LEFT | DT_TOP | DT_WORDBREAK);
    HBRUSH rule = CreateSolidBrush(kColBorder);
    FillRect(dc, &g_rcRule, rule);
    DeleteObject(rule);

    const UINT label = DT_LEFT | DT_BOTTOM | DT_SINGLELINE;
    Text(dc, L"VR FEATURES", g_rcFeatures, g_fontLabel, kColMuted, label, Px(1));
    Text(dc, L"VR HEADSET", g_rcLabelHmd, g_fontLabel, kColMuted, label, Px(1));
    Text(dc, L"VR RUNTIME", g_rcLabelRuntime, g_fontLabel, kColMuted, label, Px(1));
    Text(dc, L"RENDER RESOLUTION", g_rcLabelRes, g_fontLabel, kColMuted, label, Px(1));
    if (g_resW > 0 && g_resH > 0) {
        wchar_t cost[48];
        swprintf_s(cost, L"%.1f MP per eye", double(g_resW) * g_resH / 1e6);
        Text(dc, cost, g_rcLabelRes, g_fontHint, kColMuted, DT_RIGHT | DT_BOTTOM | DT_SINGLELINE);
    }
    // the size fields: their captions and frames (the edits sit inside, borderless)
    Text(dc, L"WIDTH", g_rcCapW, g_fontLabel, kColMuted, label, Px(1));
    Text(dc, L"HEIGHT", g_rcCapH, g_fontLabel, kColMuted, label, Px(1));
    Text(dc, L"ASPECT  W / H", g_rcCapAspect, g_fontLabel, kColMuted, label, Px(1));
    const std::pair<const RECT*, HWND> frames[] = {{&g_rcFrameW, g_hEditW}, {&g_rcFrameH, g_hEditH},
                                                   {&g_rcFrameAspect, g_hEditAspect}};
    for (const auto& [frame, edit] : frames) {
        if (!edit || IsRectEmpty(frame)) continue;
        const bool focus = GetFocus() == edit;
        Rounded(dc, *frame, Px(6), kColSurface, focus ? kColAccent : kColBorder);
    }
    wchar_t note[160] = L"";
    const HmdPreset* hmd = SelectedHmd();
    const double lens = hmd && hmd->resolutionCount > 0
        ? double(hmd->resolutions[0].width) / hmd->resolutions[0].height : 0.0;
    if (!OwnSizesAllowed())
        swprintf_s(note, L"Engine stereo is off: only the listed sizes.");
    else if (lens > 0.0 && std::fabs(g_resAspect / lens - 1.0) > 0.005)
        swprintf_s(note, L"Headset aspect %.4f; another renders past the lenses.", lens);
    else
        swprintf_s(note, L"Slide, step with - / +, or type a size or an aspect (16:9).");
    Text(dc, note, g_rcNote, g_fontHint, kColMuted, DT_LEFT | DT_TOP | DT_SINGLELINE | DT_END_ELLIPSIS);
    (void)hwnd;
}

static void DrawStart(const DRAWITEMSTRUCT& d) {
    FillRect(d.hDC, &d.rcItem, g_brushBg);
    const bool down = (d.itemState & ODS_SELECTED) != 0;
    const bool hot = g_hot == d.hwndItem;
    const COLORREF fill = down ? kColAccentDown : hot ? kColAccentHot : kColAccent;
    Rounded(d.hDC, d.rcItem, Px(6), fill, fill);
    if (d.itemState & ODS_FOCUS) {
        RECT ring = d.rcItem;
        InflateRect(&ring, -Px(3), -Px(3));
        Rounded(d.hDC, ring, Px(4), fill, kColOnAccent);
    }
    Text(d.hDC, L"START GAME", d.rcItem, g_fontButton, kColOnAccent, DT_CENTER | DT_VCENTER | DT_SINGLELINE, Px(1));
}

// An on / off switch with its title and a line under it (the debug mode, the VR lobby).
static void DrawSwitch(const DRAWITEMSTRUCT& d, bool on, const wchar_t* titleText, const wchar_t* subText) {
    FillRect(d.hDC, &d.rcItem, g_brushBg);
    const bool hot = g_hot == d.hwndItem;
    const int h = Px(22), w = Px(40);
    const int top = (d.rcItem.top + d.rcItem.bottom - h) / 2;
    const RECT track{d.rcItem.left, top, d.rcItem.left + w, top + h};
    const COLORREF fill = on ? (hot ? kColAccentHot : kColAccent) : (hot ? kColSurfaceHot : kColSurface);
    Rounded(d.hDC, track, h / 2, fill, on ? fill : (d.itemState & ODS_FOCUS) ? kColAccent
                                                 : hot ? kColBorderHot : kColBorder);
    const int knob = h - Px(8);
    const int kx = on ? track.right - Px(4) - knob : track.left + Px(4);
    const RECT dot{kx, top + Px(4), kx + knob, top + Px(4) + knob};
    const COLORREF knobColor = on ? kColOnAccent : kColMuted;
    Rounded(d.hDC, dot, knob / 2, knobColor, knobColor);
    if ((d.itemState & ODS_FOCUS) && on) {
        RECT ring = track;
        InflateRect(&ring, Px(2), Px(2));
        HBRUSH frame = CreateSolidBrush(kColText);
        FrameRect(d.hDC, &ring, frame);
        DeleteObject(frame);
    }
    RECT title = d.rcItem;
    title.left = track.right + Px(14);
    title.bottom = (d.rcItem.top + d.rcItem.bottom) / 2 + Px(1);
    Text(d.hDC, titleText, title, g_fontBody, kColText, DT_LEFT | DT_BOTTOM | DT_SINGLELINE);
    RECT sub = d.rcItem;
    sub.left = title.left;
    sub.top = title.bottom + Px(1);
    Text(d.hDC, subText, sub, g_fontHint, kColMuted, DT_LEFT | DT_TOP | DT_SINGLELINE | DT_END_ELLIPSIS);
}

// - / + beside the slider: a small field-coloured button with its sign.
static void DrawStep(const DRAWITEMSTRUCT& d, bool up) {
    FillRect(d.hDC, &d.rcItem, g_brushBg);
    const bool hot = g_hot == d.hwndItem, down = (d.itemState & ODS_SELECTED) != 0;
    Rounded(d.hDC, d.rcItem, Px(6), down || hot ? kColSurfaceHot : kColSurface,
            (d.itemState & ODS_FOCUS) ? kColAccent : hot ? kColBorderHot : kColBorder);
    const int cx = (d.rcItem.left + d.rcItem.right) / 2, cy = (d.rcItem.top + d.rcItem.bottom) / 2, s = Px(6);
    HBRUSH sign = CreateSolidBrush(hot ? kColAccent : kColText);
    const int t = std::max(1, Px(2)) / 2;
    const RECT bar{cx - s, cy - t, cx + s, cy - t + std::max(2, Px(2))};
    FillRect(d.hDC, &bar, sign);
    if (up) {
        const RECT post{cx - t, cy - s, cx - t + std::max(2, Px(2)), cy + s};
        FillRect(d.hDC, &post, sign);
    }
    DeleteObject(sign);
}

static void DrawListItem(const DRAWITEMSTRUCT& d) {
    if (d.itemState & ODS_COMBOBOXEDIT) {
        // The combo box repaints its field straight to the screen on a pick or a focus change; the subclass
        // paints the whole field, so it just repaints.
        InvalidateRect(d.hwndItem, nullptr, FALSE);
        return;
    }
    if (d.itemID == UINT(-1)) return;
    const bool selected = (d.itemState & ODS_SELECTED) != 0;
    HBRUSH fill = CreateSolidBrush(selected ? kColSurfaceHot : kColSurface);
    FillRect(d.hDC, &d.rcItem, fill);
    DeleteObject(fill);
    if (selected) {
        const RECT bar{d.rcItem.left, d.rcItem.top + Px(6), d.rcItem.left + Px(3), d.rcItem.bottom - Px(6)};
        HBRUSH accent = CreateSolidBrush(kColAccent);
        FillRect(d.hDC, &bar, accent);
        DeleteObject(accent);
    }
    RECT text = d.rcItem;
    text.left += Px(14);
    text.right -= Px(8);
    const std::wstring label = ComboItemText(d.hwndItem, int(d.itemID));
    Text(d.hDC, label.c_str(), text, g_fontBody, selected ? kColText : RGB(205, 209, 217),
         DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
}

static void StartGame(HWND hwnd) {
    // a field still being typed in counts as taken
    const HWND focus = GetFocus();
    if (focus && (focus == g_hEditW || focus == g_hEditH || focus == g_hEditAspect)) CommitField(focus);
    SetLauncherDebugAndPersist(SendMessageW(g_hDebug, BM_GETCHECK, 0, 0) == BST_CHECKED ? 1 : 0);
    SetLauncherLobbyAndPersist(SendMessageW(g_hLobby, BM_GETCHECK, 0, 0) == BST_CHECKED ? 1 : 0);
    SetLauncherInterfaceAndPersist(SendMessageW(g_hInterface, BM_GETCHECK, 0, 0) == BST_CHECKED ? 1 : 0);
    SetLauncherHandPhysicsAndPersist(SendMessageW(g_hPhysics, BM_GETCHECK, 0, 0) == BST_CHECKED ? 1 : 0);
    if (const HmdPreset* hmd = SelectedHmd()) SetHmdTypeAndPersist(hmd->mhdType);
    const int rIdx = int(SendMessageW(g_hRuntime, CB_GETCURSEL, 0, 0));
    if (rIdx >= 0 && rIdx < static_cast<int>(std::size(kRuntimeOptions))) SetRuntimeModeAndPersist(kRuntimeOptions[rIdx].mode);
    if (const ResolutionPreset* res = SelectedResolution()) {
        SetLauncherCustomResolution(0);
        SetWindowResolutionAndPersist(res->width, res->height);
    } else if (g_resW > 0 && g_resH > 0) {
        // the combo box's "Custom" item: the player's own size
        SetLauncherCustomResolution(1);
        SetWindowResolutionAndPersist(g_resW, g_resH);
    }
    g_restoreGameFocus = GetForegroundWindow() == hwnd;
    DestroyWindow(hwnd);
}

LRESULT CALLBACK LauncherWndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam) {
    switch (msg) {
    case WM_CREATE: {
        g_dpi = GetDpiForWindow(hwnd);
        if (!g_dpi) g_dpi = 96;
        g_fontTitle  = MakeFont(200, FW_SEMIBOLD);
        g_fontSub    = MakeFont(95, FW_NORMAL);
        g_fontLabel  = MakeFont(80, FW_SEMIBOLD);
        g_fontBody   = MakeFont(105, FW_NORMAL);
        g_fontButton = MakeFont(105, FW_BOLD);
        g_fontHint   = MakeFont(90, FW_NORMAL);
        g_brushSurface = CreateSolidBrush(kColSurface);
        g_hot = nullptr;

        const int margin = Px(28);
        g_clientW = Px(820);
        const int right = Px(392), fieldW = right - margin;
        const int featureLeft = Px(440), featureRight = g_clientW - margin;
        int y = Px(26);
        g_rcTitle = {margin, y, right, y + Px(38)};
        g_rcSub = {margin, y + Px(44), featureRight, y + Px(84)};
        y += Px(98);
        g_rcRule = {margin, y, featureRight, y + 1};
        y += Px(22);

        // One field: its label, then the combo box (its closed height set by its owner-drawn field height).
        auto field = [&](RECT& labelRect, int id) {
            labelRect = {margin, y, right, y + Px(18)};
            y += Px(26);
            HWND combo = MakeCombo(hwnd, id, margin, y, fieldW);
            RECT r;
            GetWindowRect(combo, &r);
            y += (r.bottom - r.top) + Px(20);
            return combo;
        };
        g_hHmd = field(g_rcLabelHmd, ID_COMBO_HMD);
        const int currentHmd = GetCurrentHmdType();
        int hmdSel = 0;
        for (int i = 0; i < static_cast<int>(std::size(kHmdPresets)); ++i) {
            SendMessageW(g_hHmd, CB_ADDSTRING, 0, (LPARAM)kHmdPresets[i].label);
            if (kHmdPresets[i].mhdType == currentHmd) hmdSel = i;
        }
        SendMessageW(g_hHmd, CB_SETCURSEL, hmdSel, 0);
        SizeList(g_hHmd);

        g_hRuntime = field(g_rcLabelRuntime, ID_COMBO_RUNTIME);
        const int currentRuntime = GetXrRuntimeMode();
        int runtimeSel = 0;
        for (int i = 0; i < static_cast<int>(std::size(kRuntimeOptions)); ++i) {
            SendMessageW(g_hRuntime, CB_ADDSTRING, 0, (LPARAM)kRuntimeOptions[i].label);
            if (kRuntimeOptions[i].mode == currentRuntime) runtimeSel = i;
        }
        SendMessageW(g_hRuntime, CB_SETCURSEL, runtimeSel, 0);
        SizeList(g_hRuntime);

        g_hRes = field(g_rcLabelRes, ID_COMBO_RES);
        y -= Px(8);
        // - [slider] +
        const int stepW = Px(34), rowH = Px(30);
        g_hResDown = MakeButton(hwnd, ID_BUTTON_RES_DOWN, L"-", {margin, y, margin + stepW, y + rowH});
        g_hSlider = CreateWindowExW(0, L"CyberpunkVRPortLauncherSlider", L"", WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            margin + stepW + Px(10), y, fieldW - 2 * (stepW + Px(10)), rowH, hwnd, (HMENU)(INT_PTR)ID_SLIDER_RES,
            nullptr, nullptr);
        g_hResUp = MakeButton(hwnd, ID_BUTTON_RES_UP, L"+", {right - stepW, y, right, y + rowH});
        y += rowH + Px(12);
        // WIDTH   HEIGHT   ASPECT: captions over borderless edits in painted frames
        const int gap = Px(12), colW = (fieldW - 2 * gap) / 3, frameH = Px(34);
        const int colX[3] = {margin, margin + colW + gap, right - colW};
        g_rcCapW = {colX[0], y, colX[0] + colW, y + Px(16)};
        g_rcCapH = {colX[1], y, colX[1] + colW, y + Px(16)};
        g_rcCapAspect = {colX[2], y, colX[2] + colW, y + Px(16)};
        y += Px(22);
        g_rcFrameW = {colX[0], y, colX[0] + colW, y + frameH};
        g_rcFrameH = {colX[1], y, colX[1] + colW, y + frameH};
        g_rcFrameAspect = {colX[2], y, colX[2] + colW, y + frameH};
        g_hEditW = MakeEdit(hwnd, ID_EDIT_WIDTH, g_rcFrameW);
        g_hEditH = MakeEdit(hwnd, ID_EDIT_HEIGHT, g_rcFrameH);
        g_hEditAspect = MakeEdit(hwnd, ID_EDIT_ASPECT, g_rcFrameAspect);
        if (!OwnSizesAllowed()) SendMessageW(g_hEditAspect, EM_SETREADONLY, TRUE, 0);
        y += frameH + Px(8);
        g_rcNote = {margin, y, right, y + Px(18)};
        y += Px(18) + Px(18);

        // the pick: the saved size (its ladder entry, or the player's own when engine stereo takes it)
        g_refW = GetCurrentWindowWidth();
        g_refH = GetCurrentWindowHeight();
        g_refCustom = GetLauncherCustomResolution() != 0;
        PopulateResolutionCombo(hmdSel);

        // Independent feature switches retain each feature's detailed preferences.
        int fy = Px(146);
        g_rcFeatures = {featureLeft, fy, featureRight, fy + Px(18)};
        fy += Px(30);
        g_lobbyOn = GetLauncherLobby() != 0;
        g_interfaceOn = GetLauncherInterface() != 0;
        g_physicsOn = GetLauncherHandPhysics() != 0;
        g_hLobby = MakeButton(hwnd, ID_CHECK_LOBBY, L"VR Lobby", {featureLeft, fy, featureRight, fy + Px(48)});
        fy += Px(78);
        g_hInterface = MakeButton(hwnd, ID_CHECK_INTERFACE, L"VR Interface", {featureLeft, fy, featureRight, fy + Px(48)});
        fy += Px(78);
        g_hPhysics = MakeButton(hwnd, ID_CHECK_PHYSICS, L"Hand physics", {featureLeft, fy, featureRight, fy + Px(48)});

        // Master switch for every probe, census and dump in the mod (see debug_gate.cpp). Off is the normal way
        // to play: on, the log grows by tens of MB and the frame time with it, because probes like
        // SightAxisProbe take a mutex thousands of times a frame.
        g_debugOn = GetLauncherDebug() != 0;
        g_hDebug = MakeButton(hwnd, ID_CHECK_DEBUG, L"Debug mode", {margin, y, right, y + Px(42)});
        y += Px(42) + Px(24);

        g_hStart = MakeButton(hwnd, ID_BUTTON_START, L"Start Game", {margin, y, right, y + Px(48)});
        y += Px(48) + Px(28);
        g_clientH = y;
        break;
    }
    case WM_MEASUREITEM: {
        auto* m = reinterpret_cast<MEASUREITEMSTRUCT*>(lParam);
        if (m->CtlType == ODT_COMBOBOX) {
            // The closed field (item -1) sets the box's height; the classic frame adds 3 pixels a side.
            m->itemHeight = m->itemID == UINT(-1) ? UINT(Px(40) - 6) : UINT(Px(32));
            return TRUE;
        }
        break;
    }
    case WM_DRAWITEM: {
        const auto& d = *reinterpret_cast<const DRAWITEMSTRUCT*>(lParam);
        if (d.CtlType == ODT_COMBOBOX) DrawListItem(d);
        else if (d.CtlID == ID_BUTTON_START) DrawStart(d);
        else if (d.CtlID == ID_CHECK_DEBUG)
            DrawSwitch(d, g_debugOn, L"Debug mode", L"Probes and a verbose log. Slower; for bug reports.");
        else if (d.CtlID == ID_CHECK_LOBBY)
            DrawSwitch(d, g_lobbyOn, L"VR Lobby", L"The main menu in its own VR world.");
        else if (d.CtlID == ID_CHECK_INTERFACE)
            DrawSwitch(d, g_interfaceOn, L"VR Interface", L"SAO menu, shops and storage in VR.");
        else if (d.CtlID == ID_CHECK_PHYSICS)
            DrawSwitch(d, g_physicsOn, L"Hand physics", L"Collisions, object/NPC grabs and climbing.");
        else if (d.CtlID == ID_BUTTON_RES_DOWN || d.CtlID == ID_BUTTON_RES_UP)
            DrawStep(d, d.CtlID == ID_BUTTON_RES_UP);
        return TRUE;
    }
    case WM_CTLCOLORLISTBOX: {
        HDC dc = (HDC)wParam;
        SetTextColor(dc, kColText);
        SetBkColor(dc, kColSurface);
        return (LRESULT)g_brushSurface;
    }
    case WM_CTLCOLOREDIT:
    case WM_CTLCOLORSTATIC: {
        // the size fields (a read-only one sends the STATIC message): the frame's colour, muted when read-only
        HDC dc = (HDC)wParam;
        const HWND edit = reinterpret_cast<HWND>(lParam);
        const bool readOnly = (GetWindowLongPtrW(edit, GWL_STYLE) & ES_READONLY) != 0;
        SetTextColor(dc, readOnly ? kColMuted : kColText);
        SetBkColor(dc, kColSurface);
        return (LRESULT)g_brushSurface;
    }
    case WM_ERASEBKGND:
        return 1;
    case WM_PAINT: {
        PAINTSTRUCT ps;
        HDC dc = BeginPaint(hwnd, &ps);
        Buffered(hwnd, dc, [hwnd](HDC mem, const RECT& rc) { PaintWindow(hwnd, mem, rc); });
        EndPaint(hwnd, &ps);
        return 0;
    }
    case WM_MOUSEMOVE:
        SetHot(nullptr);
        break;
    case WM_COMMAND: {
        const int notif = HIWORD(wParam);
        const int id    = LOWORD(wParam);

        if (id == ID_COMBO_HMD && notif == CBN_SELCHANGE) {
            PopulateResolutionCombo((int)SendMessageW(g_hHmd, CB_GETCURSEL, 0, 0));
            InvalidateRect(hwnd, &g_rcLabelRes, FALSE);
            break;
        }
        if (id == ID_COMBO_RES && notif == CBN_SELCHANGE) {
            if (const ResolutionPreset* res = SelectedResolution())
                SetPick(res->width, res->height, PickFrom::Ladder);
            InvalidateRect(hwnd, &g_rcLabelRes, FALSE);
        }
        if ((notif == CBN_DROPDOWN || notif == CBN_CLOSEUP || notif == CBN_SELCHANGE) &&
            (id == ID_COMBO_HMD || id == ID_COMBO_RUNTIME || id == ID_COMBO_RES))
            InvalidateRect(reinterpret_cast<HWND>(lParam), nullptr, FALSE);
        if ((id == ID_BUTTON_RES_DOWN || id == ID_BUTTON_RES_UP) && notif == BN_CLICKED) {
            StepPick(id == ID_BUTTON_RES_UP ? +1 : -1);
        } else if (id == ID_CHECK_LOBBY && notif == BN_CLICKED) {
            g_lobbyOn = !g_lobbyOn;
            InvalidateRect(g_hLobby, nullptr, FALSE);
        } else if (id == ID_CHECK_INTERFACE && notif == BN_CLICKED) {
            g_interfaceOn = !g_interfaceOn;
            InvalidateRect(g_hInterface, nullptr, FALSE);
        } else if (id == ID_CHECK_PHYSICS && notif == BN_CLICKED) {
            g_physicsOn = !g_physicsOn;
            InvalidateRect(g_hPhysics, nullptr, FALSE);
        } else if (id == ID_CHECK_DEBUG && notif == BN_CLICKED) {
            g_debugOn = !g_debugOn;
            InvalidateRect(g_hDebug, nullptr, FALSE);
        } else if (id == ID_BUTTON_START || id == IDOK) {
            StartGame(hwnd);
        } else if (id == IDCANCEL) {
            g_restoreGameFocus = GetForegroundWindow() == hwnd;
            DestroyWindow(hwnd);
        }
        break;
    }
    case WM_CLOSE:
        g_restoreGameFocus=GetForegroundWindow()==hwnd;
        DestroyWindow(hwnd);
        return 0;
    case WM_DESTROY:
        for (HFONT* font : {&g_fontTitle, &g_fontSub, &g_fontLabel, &g_fontBody, &g_fontButton, &g_fontHint})
            if (*font) { DeleteObject(*font); *font = nullptr; }
        if (g_brushSurface) { DeleteObject(g_brushSurface); g_brushSurface = nullptr; }
        g_hHmd = g_hRuntime = g_hRes = g_hDebug = g_hStart = g_hot = nullptr;
        g_hResDown = g_hResUp = g_hSlider = g_hEditW = g_hEditH = g_hEditAspect = g_hLobby = g_hInterface = g_hPhysics = nullptr;
        g_sliderDrag = false;
        break;
    default:
        return DefWindowProcW(hwnd, msg, wParam, lParam);
    }
    return DefWindowProcW(hwnd, msg, wParam, lParam);
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
    Gdiplus::GdiplusStartupInput gdiplusInput;
    if (Gdiplus::GdiplusStartup(&g_gdiplus, &gdiplusInput, nullptr) != Gdiplus::Ok) g_gdiplus = 0;
    struct GdiplusEnd { ~GdiplusEnd() { if (g_gdiplus) { Gdiplus::GdiplusShutdown(g_gdiplus); g_gdiplus = 0; } } } gdiplusEnd;
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
    // the resolution slider's own class (painted and driven in SliderProc)
    WNDCLASSW sliderClass = {};
    sliderClass.lpfnWndProc = SliderProc;
    sliderClass.hInstance = wc.hInstance;
    sliderClass.lpszClassName = L"CyberpunkVRPortLauncherSlider";
    sliderClass.hCursor = LoadCursor(nullptr, IDC_HAND);
    const auto sliderRegistered = RegisterClassW(&sliderClass);
    struct SliderClassEnd {
        ATOM atom; HINSTANCE instance;
        ~SliderClassEnd() { if (atom) UnregisterClassW(L"CyberpunkVRPortLauncherSlider", instance); }
    } sliderClassEnd{sliderRegistered, wc.hInstance};

    const DWORD style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    const DWORD extended=WS_EX_TOPMOST|WS_EX_APPWINDOW;
    MONITORINFO monitor{sizeof(monitor)};
    RECT work{};
    if(GetMonitorInfoW(MonitorFromWindow(startup.game,MONITOR_DEFAULTTONEAREST),&monitor))work=monitor.rcWork;
    else SystemParametersInfoW(SPI_GETWORKAREA,0,&work,0);

    // Created on the game's monitor, then sized from the client area WM_CREATE laid out at that monitor's DPI.
    HWND hwnd = CreateWindowExW(
        extended,
        L"CyberpunkVRPortLauncherClass",
        L"cyberpunk-vr-port",
        style,
        work.left, work.top, 400, 600,
        nullptr, nullptr, wc.hInstance, nullptr
    );

    if (hwnd == nullptr) {
        Log("Launcher: CreateWindowExW failed (%lu)\n",GetLastError());
        if(registered)UnregisterClassW(wc.lpszClassName,wc.hInstance);
        if (g_brushBg) { DeleteObject(g_brushBg); g_brushBg = nullptr; }
        return;
    }

    RECT rc = {0, 0, g_clientW, g_clientH};
    AdjustWindowRectExForDpi(&rc, style, FALSE, extended, g_dpi);
    const int winW = rc.right - rc.left;
    const int winH = rc.bottom - rc.top;
    const int xPos=work.left+std::max(0L,(work.right-work.left-winW)/2);
    const int yPos=work.top+std::max(0L,(work.bottom-work.top-winH)/2);
    SetWindowPos(hwnd, nullptr, xPos, yPos, winW, winH, SWP_NOZORDER | SWP_NOACTIVATE);
    // A dark title bar (DWMWA_USE_IMMERSIVE_DARK_MODE: 20, 19 before Windows 10 20H1) in the window's colour
    // (DWMWA_CAPTION_COLOR / DWMWA_BORDER_COLOR, Windows 11). Older systems keep theirs.
    const BOOL dark = TRUE;
    if (FAILED(DwmSetWindowAttribute(hwnd, 20, &dark, sizeof(dark)))) DwmSetWindowAttribute(hwnd, 19, &dark, sizeof(dark));
    const COLORREF caption = kColBg, frame = kColBorder;
    DwmSetWindowAttribute(hwnd, 35, &caption, sizeof(caption));
    DwmSetWindowAttribute(hwnd, 34, &frame, sizeof(frame));

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
