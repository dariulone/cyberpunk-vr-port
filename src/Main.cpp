// CyberpunkVRPort as a RED4ext plugin -- the replacement for the dxgi.dll proxy.
//
// WHY THIS EXISTS
//
// As dxgi.dll we stood in front of DXGI and owned what we assumed was the process's only
// swapchain. Every path grew around that assumption. The moment a second one appeared -- the
// VRCAM mirror's window -- it broke in a new way on each attempt: our own IAT hooks recursing
// through our module, the overlay drawing into the mirror's backbuffer, a shared surface
// coming back DXGI_ERROR_ACCESS_DENIED. None of those are mirror bugs; they are consequences
// of being the proxy. The build that actually ran the mirror was a red4ext plugin with no
// proxy at all, so this puts us back in that shape.
//
// WHAT CHANGES, AND WHAT DOES NOT
//
// Almost nothing moves. The engine hooks, OpenXR submit, capture, overlay and stereo module
// are the same translation units; they never needed to be a proxy, they only needed a device,
// a queue and Present. Only the way those three are acquired changes:
//
//   device + queue : sync_stereo hooks d3d12!D3D12CreateDevice before the game creates its
//                    device and keeps both. The plugin waits for them instead of being handed
//                    them by CreateSwapChain.
//   Present        : the swapchain vtable is shared process-wide, so a throwaway swapchain
//                    exposes the same table the game presents through.
//   game window    : learned from the first Present that is not one of ours.
//
// DEPLOYMENT: this DLL goes in bin\x64\plugins\cyberpunkvrport\, and bin\x64\dxgi.dll must be
// REMOVED. Running both means two copies of every hook fighting for the same addresses.

#include <windows.h>
#include "Core/Paths.hpp"
#include "Core/InputConfig.hpp"
#include "Core/TweakRecords.hpp"
#include "Core/ArchiveExtensions.hpp"
#include "Lobby/Notice.hpp"
#include "Core/ScriptCompiler.hpp"
#include "Lua/Host.hpp"
#include "Natives/NativeFunctions.hpp"
#include <memory>
#include <d3d12.h>
#include <atomic>
#include <cstring>

#include <RED4ext/RED4ext.hpp>

#include "Core/GameBuild.hpp"
#include "Hooks/RenderDocBridge.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Overlay/ImGuiOverlay.hpp"
#include "Natives/VendorRoute.hpp"
#include "Hooks/PromptButtons.hpp"

extern void Log(const char* fmt, ...);

// Runtime paths (log file, vrport.ini, CET mod folders). Was called from DllMain.
extern void InitRuntimePaths();
// One-shot on a fresh install: copy the shipped UserSettings.json over the game's own. Reads and
// sets first_launch in vrport.ini, so it happens exactly once and never touches a player's tuning
// afterwards. As early as we get -- the game may still have read its settings first, in which case
// they take on the next launch.
extern "C" void ApplyFirstLaunchGameSettings();
// Every launch: the game's window starts borderless (/video/display WindowMode), whatever it was left at.
extern "C" void ApplyLaunchWindowMode();
// The game-hook pass: camera, FOV, LoD, DLSS resolution, XInput. Sleeps ~8 s first, then
// pattern-scans. Unchanged -- none of it depended on being the proxy.
extern DWORD WINAPI WorkerThread(LPVOID);
// Device/queue/Present/overlay wiring, formerly done at CreateSwapChain time.
extern "C" __declspec(dllexport) void CyberpunkVRPort_PluginBootstrap();
// Boots sync_stereo (which installs the D3D12CreateDevice hook that captures device+queue).
extern "C" void CyberpunkVRPort_InitStereo();
extern "C" void CyberpunkVR_CloseOwnedExecution();
extern "C" ID3D12Device*       CyberpunkVR_GetGameDevice();
extern "C" ID3D12CommandQueue* CyberpunkVR_GetGameQueue();
// The script-callable natives (VRBodyBonePos, VRPalmModelPos, the lot). They used to be a SECOND
// RED4ext plugin, CyberpunkVR_Hands.dll; they are now translation units of this one and only their
// entry points had to give -- see the note at the definition.
extern void CyberpunkVR_RegisterHandsNatives();

namespace {

std::atomic<bool> g_started{false};
std::atomic<bool> g_disabled{false};

// Said once, off the loader's thread: a message box on the thread that loads plugins would hold the
// game's start until it is answered. The player is usually in the headset, so the log line comes
// first and the box only adds the desktop explanation.
DWORD WINAPI UnsupportedBuildMessage(LPVOID) {
    wchar_t text[640];
    // Same strings = the build is known and CVR_PRETEND_UNKNOWN_BUILD=1 made it a stranger: say so, instead of
    // "made for X, this game is X".
    if (std::strcmp(cvr::game_build::Expected(), cvr::game_build::Running()) == 0)
        swprintf_s(text,
                   L"CVR_PRETEND_UNKNOWN_BUILD=1 is set: this launch of Cyberpunk 2077 %hs is treated as a build "
                   L"the port does not know.\n\n"
                   L"VR is OFF for this launch: the port hooked nothing, the game runs as normal.\n"
                   L"Unset the variable to play in VR.",
                   cvr::game_build::Running());
    else
        swprintf_s(text,
                   L"CyberpunkVRPort was made for Cyberpunk 2077 %hs, and this game is %hs.\n\n"
                   L"VR is OFF for this launch: the port hooked nothing, the game runs as normal.\n"
                   L"Install a CyberpunkVRPort release made for this game version.",
                   cvr::game_build::Expected(), cvr::game_build::Running());
    MessageBoxW(nullptr, text, L"CyberpunkVRPort", MB_OK | MB_ICONWARNING | MB_SETFOREGROUND | MB_TOPMOST);
    return 0;
}

// Two copies of the port in red4ext\plugins: said once, off the loader's thread, like the build message.
DWORD WINAPI OtherCopyMessage(LPVOID parameter) {
    std::unique_ptr<std::wstring> other(static_cast<std::wstring*>(parameter));
    const std::wstring root = cvr::paths::Dir(cvr::paths::Folder::Root);
    wchar_t text[1024];
    if (other->rfind(root, 0) == 0)
        swprintf_s(text,
                   L"An older copy of CyberpunkVRPort was left beside this one:\n\n%ls\n\n"
                   L"It started first, so it runs this launch and this one stays off. It has been renamed to "
                   L"CyberpunkVR_Stereo.dll.old (the plugin is cyberpunkvrport.dll now): start the game again.",
                   other->c_str());
    else
        swprintf_s(text,
                   L"Two copies of CyberpunkVRPort are installed:\n\n%ls\n%ls\n\n"
                   L"Both would hook the game, so this one stays off for this launch. Delete the folder of the one "
                   L"you do not want (an earlier release's red4ext\\plugins\\CyberpunkVR_Stereo is replaced by "
                   L"red4ext\\plugins\\cyberpunkvrport) and start the game again.",
                   root.c_str(), other->c_str());
    MessageBoxW(nullptr, text, L"CyberpunkVRPort", MB_OK | MB_ICONWARNING | MB_SETFOREGROUND | MB_TOPMOST);
    return 0;
}

// The game hooks (camera, FOV, LoD, DLSS resolution, XInput) on their own thread, exactly as
// DllMain used to start them. WorkerThread does its own wait before pattern-scanning.
void StartWorkerThread() {
    if (HANDLE h = CreateThread(nullptr, 0, WorkerThread, nullptr, 0, nullptr)) CloseHandle(h);
}

} // namespace

RED4EXT_C_EXPORT bool RED4EXT_CALL Main(RED4ext::v1::PluginHandle handle, RED4ext::v1::EMainReason reason,
                                        const RED4ext::v1::Sdk* sdk) {
    switch (reason) {
    case RED4ext::v1::EMainReason::Load: {
        if (g_started.exchange(true)) break;
        // TWO COPIES HOOK EVERYTHING TWICE, natives included. An earlier release's folder left beside this one
        // is the usual way to get there, and that release does not look for us: so this copy stays out, before
        // it registers or hooks anything, and says why.
        // The same with an older copy beside this one, under the plugin's old file name: renamed out of the way,
        // and when it already runs this launch this copy stays out of it.
        std::wstring other = cvr::paths::RetireOldName();
        if (other.empty()) other = cvr::paths::OtherCopy();
        if (!other.empty()) {
            g_disabled = true;
            Log("[paths] another copy of the port is installed (%ls): this one (%ls) stays off\n", other.c_str(),
                cvr::paths::Dir(cvr::paths::Folder::Root).c_str());
            if (HANDLE h = CreateThread(nullptr, 0, OtherCopyMessage, new std::wstring(std::move(other)), 0, nullptr))
                CloseHandle(h);
            break;
        }
        // Everything lives in the plugin's own folder now; what an earlier release kept elsewhere moves in first.
        cvr::paths::MigrateLegacyLayout();
        InitRuntimePaths();
        Log("=== CyberpunkVRPort red4ext plugin loaded (no dxgi proxy) ===\n");
        // FIRST, and deliberately: registering the RTTI callbacks is cheap and has a window. The
        // graphics init below waits on the game's device and can take seconds; doing it first is
        // how the natives would come back nil in Lua.
        CyberpunkVR_RegisterHandsNatives();
        // Native signatures whose types RTTI made only after registration (Natives/Registration.cpp), before any Lua.
        RED4ext::v1::GameState signatures{&cvr::natives::CompleteSignatures,nullptr,nullptr};
        sdk->gameStates->Add(handle,RED4ext::EGameStateType::Initialization,&signatures);
        RED4ext::v1::GameState signaturesLate{&cvr::natives::CompleteSignatures,nullptr,nullptr};
        sdk->gameStates->Add(handle,RED4ext::EGameStateType::Running,&signaturesLate);
        // The port's spawn system joins the game's systems once there is a game instance (Natives/SpawnSystem.cpp).
        RED4ext::v1::GameState spawnSystem{&cvr::spawn::AddToGame,nullptr,nullptr};
        sdk->gameStates->Add(handle,RED4ext::EGameStateType::Running,&spawnSystem);
        RED4ext::v1::GameState vendorWatchdog{nullptr,&cvr::vendor_route::Update,&cvr::vendor_route::Reset};
        const bool vendorWatchdogReady=sdk->gameStates->Add(handle,RED4ext::EGameStateType::Running,&vendorWatchdog);
        cvr::vendor_route::WatchdogRegistered(vendorWatchdogReady);
        Log("[vendor-route] main-thread watchdog registered=%d\n",vendorWatchdogReady);
        // THE BUILD GATE, after the natives (the scripts declaring them must still load) and before
        // anything that touches an engine address. See Core/GameBuild.hpp.
        if (!cvr::game_build::Supported()) {
            g_disabled = true;
            Log("[build] game %s, port made for %s: NOT the same build -- VR off, nothing installed.\n",
                cvr::game_build::Running(), cvr::game_build::Expected());
            if (HANDLE h = CreateThread(nullptr, 0, UnsupportedBuildMessage, nullptr, 0, nullptr)) CloseHandle(h);
            // The one thing kept: it touches no engine address, only a resident capture layer. Without it a
            // RenderDoc launch of a build we do not know loses NVAPI and with it ray tracing.
            cvr::RenderDocAllowNvApi();
            break;
        }
        if (cvr::game_build::Overridden())
            Log("[build] game %s, port made for %s: CVR_ALLOW_UNKNOWN_BUILD=1 -- every old address trusted.\n",
                cvr::game_build::Running(), cvr::game_build::Expected());
        else
            Log("[build] game %s matches the port's build.\n", cvr::game_build::Running());
        // The port's input bindings, before the engine reads its input configuration (Core/InputConfig.hpp).
        cvr::input_config::Apply();
        // The port's TweakDB records, applied as the engine loads its TweakDB (Core/TweakRecords.hpp).
        cvr::tweak_records::Install(handle, sdk);
        // The boot's notice in the game's language, set as the game's ink settings load (Lobby/Notice.hpp).
        cvr::lobby::notice::Install();
        // The port's archive, mounted from the plugin's folder, and its manifest (Core/ArchiveExtensions.hpp).
        cvr::archive::Install(handle, sdk);
        // The port's scripts, compiled with the game's -- by redscript's compiler, the plugin's own when redscript is
        // not installed (Core/ScriptCompiler.hpp).
        cvr::scripts::Install(handle, sdk);
        // The plugin's Lua host (src/Lua): the runtime checked, then the host made -- it hooks the engine's script
        // load, TweakDB load, player spawn and main thread, as CET does.
        if (cvr::lua::SelfTest()) cvr::lua::Install();
        // The key icons on screen follow the button layout (left-handed, a seat's mirrors): looked up again on the
        // main thread when it changes.
        RED4ext::v1::GameState promptIcons{nullptr,&cvr::input::UpdatePromptIcons,nullptr};
        Log("[prompt-icon] main-thread refresh registered=%d\n",
            sdk->gameStates->Add(handle,RED4ext::EGameStateType::Running,&promptIcons));
        ApplyFirstLaunchGameSettings();
        ApplyLaunchWindowMode();
        // Must run before the game's D3D12CreateDevice: this is what captures the device and
        // queue, and what patches the descriptor-heap size the second view needs.
        CyberpunkVRPort_InitStereo();
        // Install the DXGI factory hook NOW, not after waiting for a device: it has to be in
        // place before the game creates its swapchain, because that single call is where the
        // resolution override is applied and where the device, queue, overlay and OpenXR all
        // get wired. Waiting for the device first is what missed it -- the game reached the
        // loading screen with no override and an XR path that was never initialised, and sat
        // there.
        CyberpunkVRPort_PluginBootstrap();
        StartWorkerThread();
        break;
    }
    case RED4ext::v1::EMainReason::Unload:
        if (g_disabled) break;
        cvr::vendor_route::Reset(nullptr);
        CyberpunkVR_CloseOwnedExecution();
        // RED4ext unloads the plugin BEFORE the process finishes tearing down the graphics runtime,
        // and nothing here used to take that window (dabinn, TofuExpress fbe336fa). Stopping OpenXR
        // and dropping our overlay's device references while D3D is still alive is the difference
        // between an orderly exit and the runtime calling through objects D3D shutdown has already
        // poisoned -- which is the crash-on-quit.
        OverlaySetDeviceAndQueue(nullptr, nullptr);
        OpenXRManager::Get().Shutdown();
        Log("=== CyberpunkVRPort red4ext plugin unloaded ===\n");
        break;
    }
    return true;
}

RED4EXT_C_EXPORT void RED4EXT_CALL Query(RED4ext::v1::PluginInfo* info) {
    info->name = L"CyberpunkVRPort";
    info->author = L"CyberpunkVRPort";
    info->version = RED4EXT_V1_SEMVER(CVR_VERSION_MAJOR, CVR_VERSION_MINOR, CVR_VERSION_PATCH);
    // Matches the loader actually installed here (runtime 1.29.1), which predates API v1 and
    // skips plugins that declare it outright -- the testbed plugin had to do the same.
    info->runtime = RED4EXT_V1_RUNTIME_VERSION_INDEPENDENT;
    info->sdk = RED4EXT_V1_SDK_VERSION_1_0_0_COMPAT_0_5_0;
}

RED4EXT_C_EXPORT uint32_t RED4EXT_CALL Supports() { return RED4EXT_API_VERSION_1_COMPAT_0; }
