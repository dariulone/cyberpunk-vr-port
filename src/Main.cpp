// THE C++ HALF'S PART OF THE PLUGIN'S START AND STOP.
//
// The entry points RED4ext calls -- Main, Query, Supports -- are Rust's since stage 2 of docs/rust-migration-plan.md
// (rust/crates/cvr-ffi/src/plugin.rs, exported through /EXPORT in CMakeLists.txt). So are the checks that come
// first: another copy of the port, the legacy layout, the log, the build gate, the script compiler.
//
// What is left here are seams: one extern "C" function per C++ subsystem the start calls, in the order this file
// used to call them itself (plugin.rs states the order and why). Each seam disappears with its subsystem's move
// to Rust.

#include <windows.h>
#include "Core/InputConfig.hpp"
#include "Core/TweakRecords.hpp"
#include "Core/ArchiveExtensions.hpp"
#include "Lobby/Notice.hpp"
#include "Lua/Host.hpp"
#include "Natives/NativeFunctions.hpp"
#include <d3d12.h>

#include <RED4ext/RED4ext.hpp>

#include "Hooks/RenderDocBridge.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Overlay/ImGuiOverlay.hpp"
#include "Natives/VendorRoute.hpp"
#include "Hooks/PromptButtons.hpp"

// Runtime paths (the settings files' locations in the config folder). Was called from DllMain.
#if !CVR_RUST_VRIK
extern void InitRuntimePaths();
#endif
// The game-hook pass: camera, FOV, LoD, DLSS resolution, XInput. Sleeps ~8 s first, then pattern-scans.
#if !CVR_RUST_VRIK
extern DWORD WINAPI WorkerThread(LPVOID);
#endif
// The script-callable natives (VRBodyBonePos, VRPalmModelPos, the lot): their RTTI callbacks.
#if !CVR_RUST_VRIK
extern void CyberpunkVR_RegisterHandsNatives();
#endif

using App = RED4ext::CGameApplication;

extern "C" {

#if !CVR_RUST_VRIK   // the Rust build's is rust/crates/cvr-render/src/cpp_services.rs
void cvr_cpp_init_runtime_paths() { InitRuntimePaths(); }
#endif

#if !CVR_RUST_VRIK
// FIRST, and deliberately: registering the RTTI callbacks is cheap and has a window. (The Rust build's table:
// rust/crates/cvr-ffi/src/natives.rs.)
void cvr_cpp_register_natives() { CyberpunkVR_RegisterHandsNatives(); }

// Native signatures whose types RTTI made only after registration (Natives/Registration.cpp).
bool cvr_cpp_complete_signatures(App* app) { return cvr::natives::CompleteSignatures(app); }
#endif

// The port's spawn system joins the game's systems once there is a game instance (Natives/SpawnSystem.cpp).
bool cvr_cpp_spawn_add_to_game(App* app) { return cvr::spawn::AddToGame(app); }

#if !CVR_RUST_VRIK
// The Rust build's are rust/crates/cvr-rig/src/vrik/vendor_route.rs.
bool cvr_cpp_vendor_route_update(App* app) { return cvr::vendor_route::Update(app); }
bool cvr_cpp_vendor_route_reset(App* app) { return cvr::vendor_route::Reset(app); }
void cvr_cpp_vendor_route_watchdog_registered(bool registered) { cvr::vendor_route::WatchdogRegistered(registered); }
#endif

// On a build the gate turns VR off: it touches no engine address, only a resident capture layer.
#if !CVR_RUST_VRIK   // the Rust build's is rust/crates/cvr-render/src/cpp_services.rs
void cvr_cpp_renderdoc_allow_nvapi() { cvr::RenderDocAllowNvApi(); }
#endif

// The port's input bindings, before the engine reads its input configuration (Core/InputConfig.hpp).
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
void cvr_cpp_input_config_apply() { cvr::input_config::Apply(); }
#endif

// The port's TweakDB records, applied as the engine loads its TweakDB (Core/TweakRecords.hpp).
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
void cvr_cpp_tweak_records_install(RED4ext::v1::PluginHandle handle, const RED4ext::v1::Sdk* sdk) {
    cvr::tweak_records::Install(handle, sdk);
}
#endif

// The boot's notice in the game's language, set as the game's ink settings load (Lobby/Notice.hpp).
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
void cvr_cpp_lobby_notice_install() { cvr::lobby::notice::Install(); }
#endif

// The port's archive, mounted from the plugin's folder, and its manifest (Core/ArchiveExtensions.hpp).
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
void cvr_cpp_archive_install(RED4ext::v1::PluginHandle handle, const RED4ext::v1::Sdk* sdk) {
    cvr::archive::Install(handle, sdk);
}
#endif

// The plugin's Lua host (src/Lua): the runtime checked, then the host made.
bool cvr_cpp_lua_self_test() { return cvr::lua::SelfTest(); }
void cvr_cpp_lua_install() { cvr::lua::Install(); }

// The key icons on screen follow the button layout: looked up again on the main thread when it changes.
#if !CVR_RUST_VRIK
bool cvr_cpp_update_prompt_icons(App* app) { return cvr::input::UpdatePromptIcons(app); }
#endif

// The game hooks on their own thread, exactly as DllMain used to start them.
#if !CVR_RUST_VRIK   // unused by the Rust build
void cvr_cpp_start_worker_thread() {
    if (HANDLE h = CreateThread(nullptr, 0, WorkerThread, nullptr, 0, nullptr)) CloseHandle(h);
}
#endif

// RED4ext unloads the plugin BEFORE the process finishes tearing down the graphics runtime. Stopping OpenXR and
// dropping the overlay's device references while D3D is still alive is the difference between an orderly exit and
// the runtime calling through objects D3D shutdown has already poisoned.
#if !CVR_RUST_VRIK   // unused by the Rust build
void cvr_cpp_shutdown_overlay_and_xr() {
    OverlaySetDeviceAndQueue(nullptr, nullptr);
    OpenXRManager::Get().Shutdown();
}
#endif

}  // extern "C"
