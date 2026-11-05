#pragma once
// AFW -- ALTERNATE FRAME WARPING, PureDark's method, through his plugin (Render/AfwPlugin.hpp): the engine draws ONE
// eye per frame, the eyes taking turns, and the plugin makes the other eye for the same moment out of the drawn
// eye's colour, depth and motion and the other eye's previous frame. The cost of a frame is about one eye's, as in
// the mono mode, and the headset still gets two eyes. Engine stereo stays the default; this is its own switch.
//
// THE LAUNCH (xr_afw = 1 or xr_osr = 1, read once at start with xr_engine_stereo): no engine stereo and no second
// view -- a
// launch of the mono mode's kind, MAIN drawing through the union of the two lens windows -- but MAIN is put at an
// eye, not the head centre: the left eye in one frame, the right in the next (Hooks/PatchCamera.cpp, the eye from
// the renderer's frame clock, FrameEye). The eye a frame was drawn for travels with its picture in the image's pose
// label (OpenXRHeadPose::afwEye), because the frame is captured on the Present thread while the game thread is
// already placing the camera for the next one.
//
// THE CAPTURE (Runtimes/OpenXRCapture.cpp): the drawn eye goes to its eye as MAIN's picture does; Warp makes the
// other eye into the second eye's slot, labelled as the second view's picture would be, so the submit pairs the
// two exactly as it pairs MAIN and the second view. Menus, loading screens and a frame without its depth and motion
// fall back to one picture for both eyes.
//
// WHO MAKES THE OTHER EYE: two switches of their own. xr_osr: the port's own OSR (Render/Osr.hpp). xr_afw: PureDark's
// plugin, not shipped with the port: PDAFWPlugin.dll from PureDark's UEVR AFW release goes into the port's plugin
// folder (red4ext/plugins/cyberpunkvrport); without it OSR makes the eye. Both on: OSR (the menu keeps one).
#include <cstdint>
#include <d3d12.h>
#include <openxr/openxr.h>

namespace cvr::framegen { struct Inputs; }

namespace cvr::afw {

// The eyes take turns this launch: xr_afw or xr_osr as it read them (Stereo/EngineStereo.cpp ReadSwitch hands it over).
void SetLaunch(bool on);
bool Launch();
// The launch asked for AFW and the plugin is loaded (it is loaded on the first capture that needs it): the frames
// take turns between the eyes. False once loading failed -- the launch then stays one picture for both eyes.
bool Active();
// The eye MAIN draws this frame: it turns over with every move of the renderer's frame clock (the clock the frame's
// motion vectors and its framegen inputs are stamped with), so every write of the camera in one frame picks the same
// eye. Game thread only.
int FrameEye();

struct WarpRequest {
    ID3D12Device* device = nullptr;
    ID3D12CommandQueue* queue = nullptr;          // the game's queue the capture list goes to
    ID3D12GraphicsCommandList* list = nullptr;    // the capture list, recording
    ID3D12Resource* drawn = nullptr;              // the presented picture (the drawn eye), in PRESENT state
    const cvr::framegen::Inputs* inputs = nullptr;  // its depth and motion, and the camera it was drawn with
    uint32_t eye = 0;                             // the drawn eye
    XrFovf fov[2]{};                              // each eye's window, as submitted
    float ipd = 0.064f;                           // the separation the two eye positions are apart (m)
    ID3D12Resource* out = nullptr;                // the second eye's slot, in COPY_SOURCE state
};
// Records the other eye into `out`. False (nothing recorded) when the plugin is not there or the inputs do not fit.
bool Warp(const WarpRequest& request);

// The switches as one value (CyberpunkVR_Afw): 0 off, kModeAfw PureDark's plugin, kModeOsr the port's OSR (also
// whenever the plugin is not there). Within a launch that takes turns, a change between the two applies at once;
// on or off waits for the next launch (Stereo/EngineStereo.hpp RestartPending).
constexpr int kModeAfw = 1, kModeOsr = 2;
inline int ModeOf(bool afw, bool osr) { return osr ? kModeOsr : afw ? kModeAfw : 0; }
void SetMode(int mode);
// The other eye comes from OSR now: the OSR mode, or AFW without its plugin.
bool UsesOsr();

// A frame the capture could not hand to Warp: counted with Warp's own fallbacks.
void Fallback(const char* why);

// Diagnostics: frames warped, frames that fell back, the last failure.
struct Stats { uint64_t warped = 0, fallbacks = 0; const char* last = ""; };
Stats Statistics();

}  // namespace cvr::afw
