#pragma once
// ENGINE STEREO -- the game's own two-eye frame as the headset's two eyes (docs/engine-stereo-plan.md).
//
// Game 2.5 can render both eyes of the Samsung Odyssey 3D monitor in ONE frame: one scene traversal, shared
// shadows and probes, prepass / G-buffer / decals / transparents / UI drawn per eye, the eyes side by side in
// one target (render feature 98). In this mode the port asks for that frame instead of its second view:
//
//   * the window, swapchain and MAIN target are two eyes wide (Core/VrCore.cpp);
//   * MAIN sits at the head centre (no eye offset in the port's pose chain) and the engine places the eyes
//     +-sep/2 along the camera's right vector -- sep is the IPD, convergence is parallel (src/Stereo/EngineStereo.cpp);
//   * MAIN gets the union of the two lens windows (what culling and the cascades see) and each eye camera its
//     own lens window when the engine builds it (Stereo/AsymProjection.cpp, src/Stereo/EngineStereo.cpp);
//   * VRCAM is never requested (Stereo/VrcamConfig.hpp);
//   * the presented backbuffer is split: each half goes to its eye's swapchain (Runtimes/OpenXRCapture.cpp);
//   * port features drawn for one eye per target (hidden-area mask, fixed VRS, wide merge) are off; DLSS, foveated
//     DLSS and framegen's inputs go per eye (Stereo/EngineStereoDlss.cpp, Render/FoveatedDlss.cpp,
//     Framegen/Inputs.cpp).
//
// The engine renders the frame when its Odyssey 3D SDK answers "available" and the [Odyssey3D] cvars ask for
// stereo. Both are the plugin's (src/Stereo/EngineStereoSdk.cpp): no Samsung SDK, registry key or cvar ini. Stereo
// starts when the main menu is first shown; before it -- splash screens, intro videos, the first loading -- the
// engine renders mono and the headset shows the window as one picture.
#include <cstdint>

extern "C" __declspec(dllexport) int32_t CyberpunkVR_EngineStereo;   // src/Stereo/EngineStereo.cpp
// 1 = each eye camera renders its own lens window (default); 0 = both render the union, live.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_EngineStereoEyeWindows;
// The mono mode (cvr::engine_stereo::ApplyMonoMode): 1 = the switch is on (xr_mono_mode) ...
extern "C" __declspec(dllexport) int32_t CyberpunkVR_MonoMode;
// ... and 1 = this launch shows one picture for both eyes.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_MonoView;

namespace cvr::engine_stereo {

// xr_engine_stereo in vrport.ini, read ONCE: the window, the swapchain and the eye split are sized from it,
// so it is not a live switch. Default 1 (an ini without the key, as a fresh install writes it): the game 2.5 path.
// Off for the launch while the mono mode is on.
bool Enabled();
// xr_engine_stereo as the ini asks for it, whatever the mono mode made of it this launch: what is written back.
bool Requested();

// THE MONO MODE (xr_mono_mode): one picture, drawn once from the head centre, for both eyes -- no depth, the most
// frames. Read with xr_engine_stereo at start: a launch in mono has neither engine stereo nor the port's second view
// (SecondViewWanted) and stays mono until the next start. Turned on during a launch in the port's own stereo it
// applies at once: VRCAM off, MAIN at the head centre, one window both eyes share (Runtimes/AsymmetricProjection.hpp).
// Whether the launch started in the mono mode.
bool MonoLaunch();
// The switch, from the live controls when the key is read or changed. With engine stereo running, or turned off in
// a launch that started in mono, it only records the choice (RestartPending).
void ApplyMonoMode(bool mono);
// The mono picture is what the headset gets now.
inline bool MonoView() { return CyberpunkVR_MonoView != 0; }
// The running launch does not match the switch: engine stereo, and the way out of a mono launch, need a restart.
inline bool RestartPending() {
    return ((Requested() && CyberpunkVR_MonoMode == 0) != Enabled()) || (MonoLaunch() && CyberpunkVR_MonoMode == 0);
}

// THE PORT'S SECOND VIEW EXISTS ONLY OUTSIDE ENGINE STEREO. What was built for the MAIN + VRCAM pair -- MAIN's work
// lent to VRCAM (HUD composition, light block list, fog history), VRCAM's culling reuse and replay, the mono
// cascades, the single-pass instrumentation around the pair -- has no second view to serve here, and some of it
// still acted on MAIN alone (the mono cascades re-cleared every cascade they waited a snapshot for). Its detours are
// not installed and its features stay off when this is false. Lends that also serve the scope camera's view
// (vr_optics_feed: distant shadows, GI, sky, wind, clouds, the graph cache salt) are not under this switch.
// Nor is there one in a launch that started in the mono mode.
inline bool SecondViewWanted() { return !Enabled() && !MonoLaunch(); }

// Both eyes render one window (the eye windows are off): Runtimes/AsymmetricProjection.hpp then makes the two
// lens windows the same, their union.
inline bool SharedWindow() { return Enabled() && CyberpunkVR_EngineStereoEyeWindows == 0; }

// Whether MAIN's latest prepared frame had the engine's stereo (render feature 98) on.
bool Rendering();

// The main menu's controller (mods/redscript/CyberpunkVRPort_EngineStereo, native VREngineStereoMenuShown) says it
// was shown; track its lifetime. The first closure of the last frontend arms stereo.
void NoteMainMenu();
// The main menu's controller was uninitialised (the save loads, or the game quits to the desktop).
void NoteMainMenuClosed();
// Whether the frontend has closed in this launch: the engine is asked for stereo from then on.
bool Armed();
// Whether the main menu is up now (between its controller's initialisation and uninitialisation).
bool MainMenuUp();

// THE WINDOW IS ONE PICTURE: the engine rendered the frame mono (before the main menu, or any frame without feature
// 98), or the loading screen is up. The capture does not split it into the eyes: both get one picture on the menu
// panel, as they get a menu (Runtimes/OpenXRCapture.cpp, Runtimes/OpenXRFrameLoop.cpp).
bool OnePicture();

// The view producer's detour (Stereo/AsymProjection.cpp) calls this after the engine prepared MAIN.
void NoteMainPrepared(const uint8_t* view);

// What each eye camera is given at its build, in the engine's terms (Runtimes/AsymmetricProjection.hpp
// ToEngine): aspect and NDC shift; the vertical fov is the centre's, which MAIN was given. Published by the view
// producer's detour before the engine builds the eyes; `valid` false leaves the eyes on the centre's window.
struct EyeProjection { float aspect = 0.0f, shiftX = 0.0f, shiftY = 0.0f; };
void PublishEyes(const EyeProjection eyes[2], bool valid);
// The window eye 0 (left) / 1 (right) is built with: false when the eyes render the centre's window instead
// (eye windows off, or nothing published yet).
bool EyeWindow(int eye, EyeProjection* out);
// The separation the engine places the eyes at (m): the IPD.
float EyeSeparation();
// The size the eye cameras turn their TAA jitter (pixels) into NDC with -- the view's, both eyes wide -- as the
// last eye build had it. False before an eye was built.
bool EyeJitterSize(uint32_t* width, uint32_t* height);
// One eye's half of the view in pixels (half the width the view prepare hands the centre, its height), as the last
// centre build had it: what an eye's jitter in its own pixels is relative to. False before one was built.
bool EyeSize(uint32_t* width, uint32_t* height);

// A rendering plane with its own projection (the first-person plane's override) is rebuilt from it after its
// eyes were cloned, and the rebuild moves both eye clones to the centre's position (src/Stereo/EngineStereo.cpp).
// The plane detour (Stereo/AsymProjection.cpp) keeps the eyes' places before the rebuild and puts them back after.
struct EyePlaces { uint8_t* eye[2]{}; int32_t position[2][3]{}; float world[2][4]{}; bool valid = false; };
EyePlaces KeepEyePlaces(uint8_t* descriptor);
void RestoreEyePlaces(const EyePlaces& places);

// THE HUD IS LAID OUT TWO EYES WIDE. Ink sizes its layers to the window, so the HUD root here is 5296 x 2864 on
// the Quest 3 profile, and the HUD layer fits its 3840 x 2160 design into that root bound by the HEIGHT (1.326
// root pixels per unit) where one eye's 2648 x 2864 root is bound by the width (0.690). Every element is 1.92x
// the size it has in the port's other modes. This is the factor that brings a root of this size back to the
// one-eye design scale; 1 outside the mode or for a root of another size.
float HudDesignRatio(float width, float height);

// Whether ink lays the UI out in one eye's window (src/Stereo/EngineStereo.cpp THE UI IS LAID OUT FOR ONE EYE): the
// layers each eye's UI pass draws (the HUD, the notifications) and their canvases are one eye's size, and each eye's
// pass draws its canvas into its half. Menus keep the whole window.
bool UiOneEye();

// The factor the UI view `view` narrows its camera's NDC across by: the window's width over the layer's for the view
// a one-eye layer's controllers project through (THE CAMERA THEY PLACE WORLD POINTS WITH IS ONE EYE'S), 1 for any
// other. A source projection read through that view takes it on its P00 and P20.
float UiViewNarrowing(const void* view);

// Whether the loading screen is up (src/Stereo/EngineStereo.cpp THE LOADING SCREEN IS ONE PICTURE): it is shown as
// one picture on the menu panel, as a menu is, also once the game's menu state has gone back to gameplay while the
// screen waits for the button.
bool LoadingShown();
// The loading screen is up over a frame the engine rendered both eyes of: each eye's UI pass drew the whole screen
// into its own half, squeezed to half its width (the 3D monitor's side by side). One half then already is the
// picture the capture's squeeze makes of a window drawn once, and is taken as it is. Not under the main menu: the
// loading layer stays active there, and the menu is drawn once over the window.
bool LoadingSideBySide();

}  // namespace cvr::engine_stereo
