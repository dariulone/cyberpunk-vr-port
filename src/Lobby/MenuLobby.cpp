// The main menu as a place (Lobby/MenuLobby.hpp). Verified against game 2.5 (Core/GameBuild.hpp) with IDA headless and a
// RenderDoc capture of the main menu (composition shader a4dc07ba..., the dispatch before the final copy).
#include "Lobby/MenuLobby.hpp"
#include "Stereo/EngineStereo.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/DetourRegistry.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/GameEngine.hpp>
#include <RED4ext/Scripting/Natives/inkLayer.hpp>
#include <d3d12.h>
#include <windows.h>
#include <algorithm>
#include <atomic>
#include <cstring>

extern void Log(const char*, ...);
// The ink system's layer of a native type; null when it has none. Main thread (Natives/EngineObjects.cpp).
RED4ext::ink::Layer* FindInkLayer(RED4ext::CName type);

struct LobbyInput {uint32_t logical, physical, format, width, height, frame;};
extern "C" {
// xr_lobby (Core/LiveControlsPoll.cpp).
__declspec(dllexport) int32_t CyberpunkVR_Lobby = 1;
// 0 = the composition block is left as the game wrote it (the backdrop stays), the world still renders behind it.
__declspec(dllexport) int32_t CyberpunkVR_LobbyCompose = 1;
// xr_lobby_ambient (Core/LiveControlsPoll.cpp): the lobby's ambience plays (mods/cet/CyberpunkVRPort_Lobby
// modules/sound.lua asks, its menu's button switches it).
__declspec(dllexport) int32_t CyberpunkVR_LobbyAmbient = 1;
// 1 (default) = a loading screen after the first main menu is hung in the dark before the player (LoadingInSpace);
// 0 = the eyes are shut while it is up.
__declspec(dllexport) int32_t CyberpunkVR_LoadingInSpace = 1;
// [0] composition blocks given the world, [1] render-behind-menu switch set, [2] switch handed back,
// [3] ticks with no runtime UI system to find, [4] composition inputs recorded, [5] UI system lookups,
// [6] times the world layer was switched on for the lobby, [7] world layer ticks seen,
// [8] times the player's gameplay input was switched on, [9] times the menu's UI input context was set aside,
// [10] input handed back, [11] ticks with no player input object to find.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_LobbyStats[12]{};
// The node the composition block is uploaded under (RVA of its work function), and the resources that node resolved
// in the last frame it ran: the menu panel's UI is one of them.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_LobbyCompositeNode{0};
__declspec(dllexport) LobbyInput CyberpunkVR_LobbyInputs[16]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_LobbyInputCount{0};
}

namespace cvr::lobby {
namespace {
constexpr uintptr_t kRuntimeUiVtable = 0x361FC10;  // worlduiRuntimeSystemUI
constexpr size_t kRuntimeUi = 0x48;                // gameuiGameSystemUI -> the world's worlduiRuntimeSystemUI
constexpr size_t kRenderBehindMenus = 0x264;       // written by SetRenderGameInBackground (sub_142E06DC0)
// The frame graph's resource table (Framegen/NativeInputs.cpp Describe): physical handle -> ID3D12Resource.
constexpr uintptr_t kResourcePool = 0x3BCDCA8;
constexpr size_t kPoolResources = 0x2F1D8, kPoolStride = 0xB0;
// inkWorldLayer (its vtable) and inkLayer::active (the flag Stereo/EngineStereo.cpp reads on the loading layer).
constexpr uintptr_t kWorldLayerVtable = 0x3165E28;
constexpr size_t kLayerActive = 0xB0;
// The local player's input (sub_140732BE4): gameIPlayerManager's local player (its virtual at +0x200) holds it at
// +0x10. Its virtuals, each taking the object's own lock: +0xD0 gameplay input on, +0xD8 switch it, +0x98 the UI
// input context on top (a CName, written to the out argument), +0xE8 replace that context.
constexpr uintptr_t kPlayerInputVtable = 0x3077280;
constexpr size_t kLocalPlayer = 0x200, kPlayerInput = 0x10;
constexpr size_t kGameInputOn = 0xD0, kSetGameInputOn = 0xD8, kUiContext = 0x98, kSetUiContext = 0xE8;
// r6\config\inputContexts.xml: the main menu's UI context, and the empty one the pre-game menu leaves behind when it
// closes (sub_142D89F04).
constexpr uint64_t kUIMenu = 0xB59929853224746Cull, kReleaseAll = 0xBD3B8A9300D02CE6ull;

std::atomic<uint8_t*> g_systemUi{nullptr};
std::atomic<uint8_t*> g_playerManager{nullptr};
std::atomic<uint64_t> g_composedTick{0};
bool g_forced = false;
uint8_t* g_forcedRuntime = nullptr;  // the runtime UI the byte was written to
std::atomic<uint32_t> g_frame{0};
constexpr ULONGLONG kCurtainUnanswered = 20000;
std::atomic<float> g_curtain{1.0f};
std::atomic<bool> g_curtainAnswered{false};
std::atomic<ULONGLONG> g_menuUpTick{0};
constexpr ULONGLONG kLeavingMax = 5000;
std::atomic<ULONGLONG> g_leavingTick{0};

uintptr_t Base() { return reinterpret_cast<uintptr_t>(cvr::detail::g_exe_base); }

// The world's runtime UI system, read through the game system each time: the main menu world and the game world each
// have their own, and the game system points at whichever is current.
uint8_t* RuntimeUi() {
    auto* system = g_systemUi.load(std::memory_order_acquire);
    if (!system || !Base()) return nullptr;
    __try {
        auto* runtime = *reinterpret_cast<uint8_t**>(system + kRuntimeUi);
        if (runtime && *reinterpret_cast<uintptr_t*>(runtime) == Base() + kRuntimeUiVtable) return runtime;
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    return nullptr;
}

bool Describe(uint32_t physical, LobbyInput& row) {
    if (!physical || physical > 0x100000) return false;
    __try {
        const auto pool = *reinterpret_cast<const uintptr_t*>(Base() + kResourcePool);
        if (!pool) return false;
        auto* resource = *reinterpret_cast<ID3D12Resource**>(pool + kPoolResources + uintptr_t(physical - 1) * kPoolStride);
        if (!resource) return false;
        const auto d = resource->GetDesc();
        row.format = d.Format;
        row.width = uint32_t(d.Width);
        row.height = d.Height;
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    return false;
}
}  // namespace

namespace {
std::atomic<bool> g_suspended{false};
}

bool Enabled() { return CyberpunkVR_Lobby != 0; }
void SetEnabled(bool on) { CyberpunkVR_Lobby = on ? 1 : 0; }
bool Ambient() { return CyberpunkVR_LobbyAmbient != 0; }
void SetAmbient(bool on) { CyberpunkVR_LobbyAmbient = on ? 1 : 0; }
bool Active() {
    return Enabled() && !g_suspended.load(std::memory_order_relaxed) && engine_stereo::Enabled() &&
           engine_stereo::MainMenuUp();
}
void Suspend(bool on) {
    if (g_suspended.exchange(on, std::memory_order_relaxed) != on)
        Log("[lobby] %s\n", on ? "steps aside for the game's menu" : "back");
}
bool Suspended() { return g_suspended.load(std::memory_order_relaxed); }

namespace {
// THE WORLD'S SCREENS IN THE MAIN MENU. Every in-world screen -- the VR lobby's among them -- is a WorldWidgetComponent
// the world layer spawns and draws (sub_140607414 ticks it). The main menu leaves that layer inactive: it ticks, its
// components are registered and asked to spawn, and none ever gets its widget (2026-10-27, live: the lobby screen's
// root stayed null until +0xB0 was set, then it spawned within a frame and drew). While the lobby is up the layer is
// switched on. Handed back only while the main menu is still up (the lobby switched off); leaving the main menu for
// the game, the game switches the layer itself.
uint8_t* g_worldLayerHeld = nullptr;
using WorldLayerTickFn = int64_t(__fastcall*)(void*, void*, void*);
WorldLayerTickFn g_origWorldLayerTick = nullptr;

void HoldWorldLayer(uint8_t* layer, bool active) {
    __try {
        if (*reinterpret_cast<const uintptr_t*>(layer) != Base() + kWorldLayerVtable) return;
        if (active) {
            if (!layer[kLayerActive]) {
                layer[kLayerActive] = 1;
                CyberpunkVR_LobbyStats[6].fetch_add(1, std::memory_order_relaxed);
            }
            g_worldLayerHeld = layer;
        } else if (g_worldLayerHeld == layer) {
            if (engine_stereo::MainMenuUp()) layer[kLayerActive] = 0;
            g_worldLayerHeld = nullptr;
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
}

// THE PLAYER WALKS IN THE LOBBY ON THE GAME'S OWN INPUT. The main menu world has a whole player -- puppet, state
// machine, controller -- and the pad reaches it already; two things stand in the way (2026-10-27, live):
//  - the player's gameplay input is off: actions are not fed to the state machine, so MoveX/MoveY stay 0. Switched
//    on, the stick walked the puppet through the game's own locomotion (10.5 m in 3 s) and the right stick turned
//    it, the view following;
//  - the UI input context on top is UIMenu, the main menu's: A would press the menu's focused button as well as
//    jump. It is replaced by RELEASE_ALL, the context the pre-game menu itself leaves when it closes.
// Both go through the object's own virtuals. Handed back only while the main menu is still up (the lobby switched
// off): leaving the main menu for the game, the game is already where it wants to be -- its UI context is the one
// the menu would have left, and its gameplay input has to be on.
struct InputHold {
    uint8_t* input = nullptr;
    bool gameWas = false;
    bool uiSet = false;
};
InputHold g_inputHold;

template <typename Fn> Fn Virtual(uint8_t* object, size_t slot) {
    return *reinterpret_cast<Fn*>(*reinterpret_cast<uintptr_t*>(object) + slot);
}

uint8_t* PlayerInput() {
    auto* manager = g_playerManager.load(std::memory_order_acquire);
    if (!manager) return nullptr;
    __try {
        auto* player = Virtual<uint8_t*(__fastcall*)(void*)>(manager, kLocalPlayer)(manager);
        auto* input = player ? *reinterpret_cast<uint8_t**>(player + kPlayerInput) : nullptr;
        if (input && *reinterpret_cast<uintptr_t*>(input) == Base() + kPlayerInputVtable) return input;
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    return nullptr;
}

bool GameInputOn(uint8_t* input) { return Virtual<bool(__fastcall*)(void*)>(input, kGameInputOn)(input); }
void SetGameInputOn(uint8_t* input, bool on) { Virtual<void(__fastcall*)(void*, bool)>(input, kSetGameInputOn)(input, on); }
uint64_t UiContext(uint8_t* input) {
    uint64_t name = 0;
    Virtual<uint64_t*(__fastcall*)(void*, uint64_t*)>(input, kUiContext)(input, &name);
    return name;
}
void SetUiContext(uint8_t* input, uint64_t name) { Virtual<bool(__fastcall*)(void*, uint64_t)>(input, kSetUiContext)(input, name); }

void HoldInput(bool active) {
    __try {
        if (active) {
            uint8_t* input = PlayerInput();
            if (!input) {
                CyberpunkVR_LobbyStats[11].fetch_add(1, std::memory_order_relaxed);
                return;
            }
            if (input != g_inputHold.input) g_inputHold = {input, GameInputOn(input), false};
            if (!GameInputOn(input)) {
                SetGameInputOn(input, true);
                CyberpunkVR_LobbyStats[8].fetch_add(1, std::memory_order_relaxed);
            }
            if (UiContext(input) == kUIMenu) {
                SetUiContext(input, kReleaseAll);
                g_inputHold.uiSet = true;
                CyberpunkVR_LobbyStats[9].fetch_add(1, std::memory_order_relaxed);
            }
        } else if (g_inputHold.input) {
            uint8_t* input = g_inputHold.input;
            if (engine_stereo::MainMenuUp() && *reinterpret_cast<uintptr_t*>(input) == Base() + kPlayerInputVtable) {
                SetGameInputOn(input, g_inputHold.gameWas);
                if (g_inputHold.uiSet && UiContext(input) == kReleaseAll) SetUiContext(input, kUIMenu);
            }
            g_inputHold = {};
            CyberpunkVR_LobbyStats[10].fetch_add(1, std::memory_order_relaxed);
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        g_inputHold = {};
    }
}

// THE MAIN MENU TAKES NO INPUT UNDER THE LOBBY. The lobby drives the main menu's controller (Continue, Load, New
// Game ...), never its screen: the menu's window is hidden while the lobby is up (lobby.lua). Hidden, its layer still
// took the pointer: moving the mouse played the menu's hover and focus sounds over buttons nobody saw. The layer itself
// takes no input while the lobby is up (inkLayer::isInteractive, +0xB7, cleared) and is given its own value back when
// the lobby steps aside or ends. Found again every frame: the layer is the ink system's, not the lobby's to keep.
constexpr size_t kLayerInteractive = 0xB7;
struct MenuLayerHold {
    uint8_t* layer = nullptr;
    uint8_t was = 0;
};
MenuLayerHold g_menuLayerHold;

void HoldMenuLayer(bool active) {
    static const RED4ext::CName kMenuLayer("inkMenuLayer");
    __try {
        auto* layer = reinterpret_cast<uint8_t*>(FindInkLayer(kMenuLayer));
        if (active) {
            if (!layer) return;
            if (layer != g_menuLayerHold.layer) {
                g_menuLayerHold = {layer, layer[kLayerInteractive]};
                Log("[lobby] main menu layer: active %u interactive %u game input %u -> takes no input\n",
                    layer[kLayerActive], layer[kLayerInteractive], layer[kLayerInteractive + 1]);
            }
            layer[kLayerInteractive] = 0;
        } else if (g_menuLayerHold.layer) {
            if (layer == g_menuLayerHold.layer) layer[kLayerInteractive] = g_menuLayerHold.was;
            g_menuLayerHold = {};
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        g_menuLayerHold = {};
    }
}

// THE LOBBY HAS ITS OWN SOUND, NOT THE MAIN MENU'S. Entering the main menu the game starts its music and its
// ambience at once, from its menu music object (sub_14155D3F8, once per menu, the flag +0x91:
// mus_game_menus_00_title_screen_START, mus_game_menus_01_main_menu, ui_main_menu_loop_start on its "default"
// emitter) -- while the headset is still dark, then under the intro's legal notice, as if the loading screen had a
// sound (2026-10-30: the game's audio session silent through the boot's loading screen, at -25 dB peak from the main
// menu's first frame on, the endpoint's session meters read every 100 ms beside a debugger log of
// AK::SoundEngine::PostEvent). While the lobby can be the main menu (xr_lobby, engine stereo) that start is held here:
// the intro and the lobby play their own (mods/cet/CyberpunkVRPort_Lobby modules/sound.lua, Audio/PortSound.hpp). It
// is handed to the game when the main menu is the game's again -- the lobby switched off while the menu is up -- or
// when no script answers the curtain within 20 s of it (a lobby that never runs does not leave the menu silent). The
// menu closing drops the hold: the game stops its menu music itself then.
using MenuMusicFn = void(__fastcall*)(void*);
MenuMusicFn g_origMenuMusic = nullptr;
std::atomic<void*> g_heldMusic{nullptr};
std::atomic<ULONGLONG> g_heldMusicTick{0};
std::atomic<ULONGLONG> g_curtainTick{0};

void __fastcall Detour_MenuMusic(void* music) {
    if (music && Enabled() && engine_stereo::Enabled()) {
        g_heldMusicTick.store(GetTickCount64(), std::memory_order_relaxed);
        if (!g_heldMusic.exchange(music, std::memory_order_acq_rel))
            Log("[lobby] the main menu's music is held: the lobby has its own\n");
        return;
    }
    g_origMenuMusic(music);
}

void ReleaseMenuMusic(const char* why) {
    if (void* music = g_heldMusic.exchange(nullptr, std::memory_order_acq_rel)) {
        g_origMenuMusic(music);
        Log("[lobby] the main menu's music starts: %s\n", why);
    }
}

// The world layer ticks on the main thread every frame, the main menu's included: the lobby's per-frame holds.
int64_t __fastcall Detour_WorldLayerTick(void* layer, void* tick, void* jobs) {
    if (layer && Base()) {
        CyberpunkVR_LobbyStats[7].fetch_add(1, std::memory_order_relaxed);
        const bool active = Active();
        HoldWorldLayer(static_cast<uint8_t*>(layer), active);
        HoldInput(active);
        HoldMenuLayer(active);
        if (g_heldMusic.load(std::memory_order_relaxed)) {
            const ULONGLONG held = g_heldMusicTick.load(std::memory_order_relaxed);
            if (!Enabled() && engine_stereo::MainMenuUp())
                ReleaseMenuMusic("the lobby is switched off");
            else if (GetTickCount64() - held > kCurtainUnanswered && g_curtainTick.load(std::memory_order_relaxed) < held)
                ReleaseMenuMusic("no word from the lobby");
        }
    }
    return g_origWorldLayerTick(layer, tick, jobs);
}
}  // namespace
CVR_DETOUR("[lobby] world layer tick sub_140607414", 0x607414, Detour_WorldLayerTick, g_origWorldLayerTick)
CVR_DETOUR("[lobby] main menu music sub_14155D3F8", 0x155D3F8, Detour_MenuMusic, g_origMenuMusic)

bool HoldsRender(const void* runtimeUi) {
    return g_forced && runtimeUi && Active() && runtimeUi == RuntimeUi();
}

bool WorldShown() {
    return g_forced && Active() && GetTickCount64() - g_composedTick.load(std::memory_order_relaxed) < 500;
}

void NoteMainMenu() {
    // The eyes shut till the lobby is up -- not for one held up while it is (the new game's screens walked).
    if (engine_stereo::MainMenusUp() <= 1) {
        g_curtain.store(1.0f, std::memory_order_relaxed);
        g_curtainAnswered.store(false, std::memory_order_relaxed);
    }
    g_menuUpTick.store(GetTickCount64(), std::memory_order_relaxed);
    auto* engine = RED4ext::CGameEngine::Get();
    auto* framework = engine ? engine->framework : nullptr;
    auto* game = framework ? framework->gameInstance : nullptr;
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* type = rtti ? rtti->GetClass("gameuiGameSystemUI") : nullptr;
    auto* system = game && type ? game->GetSystem(type) : nullptr;
    g_systemUi.store(reinterpret_cast<uint8_t*>(system), std::memory_order_release);
    auto* managerType = rtti ? rtti->GetClass("gameIPlayerManager") : nullptr;
    auto* manager = game && managerType ? game->GetSystem(managerType) : nullptr;
    g_playerManager.store(reinterpret_cast<uint8_t*>(manager), std::memory_order_release);
    CyberpunkVR_LobbyStats[5].fetch_add(1, std::memory_order_relaxed);
    if (!system) Log("[lobby] gameuiGameSystemUI not found: the main menu world will pause behind the menu\n");
    if (!manager) Log("[lobby] gameIPlayerManager not found: the player cannot walk in the lobby\n");
}

bool LoadingInSpace() {
    return CyberpunkVR_LoadingInSpace && g_menuUpTick.load(std::memory_order_relaxed) &&
           engine_stereo::LoadingShown() && !engine_stereo::MainMenuUp();
}

bool LoadingDark() {
    if (engine_stereo::LoadingShown() && !engine_stereo::MainMenuUp()) {
        g_leavingTick.store(0, std::memory_order_relaxed);
        return !LoadingInSpace();
    }
    const ULONGLONG since = g_leavingTick.load(std::memory_order_relaxed);
    return since && GetTickCount64() - since < kLeavingMax;
}

void NoteMainMenuClosed() {
    // The last one: the game's own closing under the one the lobby holds up for the new game's screens leaves the
    // lobby where it is (the eyes were shut for 5 s each time NEW GAME was pressed).
    if (Active() && engine_stereo::MainMenusUp() <= 1) g_leavingTick.store(GetTickCount64(), std::memory_order_relaxed);
    if (g_heldMusic.exchange(nullptr, std::memory_order_acq_rel)) Log("[lobby] the main menu closed before its music started\n");
}

float Curtain() {
    if (!Active()) return 0.0f;
    if (!g_curtainAnswered.load(std::memory_order_relaxed) &&
        GetTickCount64() - g_menuUpTick.load(std::memory_order_relaxed) > kCurtainUnanswered)
        return 0.0f;
    return g_curtain.load(std::memory_order_relaxed);
}

void SetCurtain(float closure) {
    g_curtain.store(std::clamp(closure, 0.0f, 1.0f), std::memory_order_relaxed);
    g_curtainAnswered.store(true, std::memory_order_relaxed);
    g_curtainTick.store(GetTickCount64(), std::memory_order_relaxed);
}

void Tick() {
    g_frame.fetch_add(1, std::memory_order_relaxed);
    if (!engine_stereo::MainMenuUp()) g_suspended.store(false, std::memory_order_relaxed);
    const bool active = Active();
    if (!active && !g_forced) return;
    uint8_t* runtime = RuntimeUi();
    // THE BYTE IS TAKEN BACK WHEN THE RUNTIME UI IS THERE AGAIN. Leaving the main menu for a load, there is none for a
    // moment; the forced state used to be dropped right then, and the byte stayed set in the game: every menu kept
    // the world rendering behind it, the game never raised its world-stopped flag, and the port never saw a menu
    // (Hooks/MenuMode.cpp) -- the pause menu and the map were split between the eyes, each showing the left half of
    // the window (2026-10-31: LobbyStats set 1, cleared 0, runtime missing 1).
    if (!runtime) {
        CyberpunkVR_LobbyStats[3].fetch_add(1, std::memory_order_relaxed);
        return;
    }
    __try {
        if (active) {
            if (!runtime[kRenderBehindMenus]) {
                runtime[kRenderBehindMenus] = 1;
                CyberpunkVR_LobbyStats[1].fetch_add(1, std::memory_order_relaxed);
                if (!g_forced) Log("[lobby] the main menu world renders behind the menu\n");
            }
            g_forced = true;
            g_forcedRuntime = runtime;
        } else {
            // Only where it was written: another runtime UI's byte is the game's own.
            if (runtime == g_forcedRuntime) {
                runtime[kRenderBehindMenus] = 0;
                CyberpunkVR_LobbyStats[2].fetch_add(1, std::memory_order_relaxed);
                Log("[lobby] the world no longer renders behind menus\n");
            }
            g_forced = false;
            g_forcedRuntime = nullptr;
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
}

// fxCompositionShaderParams, 432 bytes (main menu, menuRender.rdc event 7261, its b6):
//   row 1 = uiFactor, uiPassthroughFactor, mainRenderFactor, blurredRenderFactor; row 2.x = backgroundTextureFactor;
//   rows 4-8 = the grading the HUD composite also carries (Stereo/Grading.cpp), repeated in rows 22-26;
//   rows 10, 15, 17 and 21 hold integers; row 16.zw = the target's width and height.
// Other 432-byte blocks go through the same upload. A weight read from an integer is a denormal and passes a plain
// 0..1 test, and 1.0 written over an index sends a shader to a descriptor that does not exist (three GPU page faults
// in three different shaders under the main menu, 2026-10-27). So every word the test reads has to be what this
// block holds: real weights, a whole-pixel target and the repeated grading.
static bool CompositionBlock(const float* b) {
    const auto weight = [](float v) { return v == 0.0f || (v >= 1e-3f && v <= 1.0f); };
    for (const int i : {1 * 4 + 0, 1 * 4 + 1, 1 * 4 + 2, 1 * 4 + 3, 2 * 4 + 0})
        if (!weight(b[i])) return false;
    const float w = b[16 * 4 + 2], h = b[16 * 4 + 3];
    if (!(w >= 64.0f && w <= 16384.0f && h >= 64.0f && h <= 16384.0f) || w != float(int(w)) || h != float(int(h)))
        return false;
    if (std::memcmp(b + 4 * 4, b + 22 * 4, 5 * 16) != 0) return false;
    // The grading is real: saturation set, aberration tiny.
    const float sat = b[6 * 4 + 2], ab = b[6 * 4 + 3];
    return sat >= 1e-3f && sat <= 4.0f && ab >= 0.0f && ab < 0.01f;
}

bool ComposeWorld(float* block) {
    if (!block || !CyberpunkVR_LobbyCompose || !Active() || !CompositionBlock(block)) return false;
    block[1 * 4 + 2] = 1.0f;
    block[1 * 4 + 3] = 0.0f;
    block[2 * 4 + 0] = 0.0f;
    if (Base() && cvr::detail::t_current_node_work > Base())
        CyberpunkVR_LobbyCompositeNode.store(uint32_t(cvr::detail::t_current_node_work - Base()), std::memory_order_relaxed);
    g_composedTick.store(GetTickCount64(), std::memory_order_relaxed);
    CyberpunkVR_LobbyStats[0].fetch_add(1, std::memory_order_relaxed);
    return true;
}

void ObserveResolve(uint32_t logical, uint32_t physical) {
    const uint32_t node = CyberpunkVR_LobbyCompositeNode.load(std::memory_order_relaxed);
    if (!node || !Base() || cvr::detail::t_current_node_work != Base() + node || !Active()) return;
    static uint32_t s_frame = 0;
    const uint32_t frame = g_frame.load(std::memory_order_relaxed);
    if (frame != s_frame) {
        s_frame = frame;
        CyberpunkVR_LobbyInputCount.store(0, std::memory_order_relaxed);
    }
    const uint32_t n = CyberpunkVR_LobbyInputCount.load(std::memory_order_relaxed);
    if (n >= 16) return;
    LobbyInput row{logical, physical, 0, 0, 0, frame};
    Describe(physical, row);
    CyberpunkVR_LobbyInputs[n] = row;
    CyberpunkVR_LobbyInputCount.store(n + 1, std::memory_order_relaxed);
    CyberpunkVR_LobbyStats[4].fetch_add(1, std::memory_order_relaxed);
}
}  // namespace cvr::lobby
