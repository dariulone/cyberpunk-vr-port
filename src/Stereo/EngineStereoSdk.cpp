// THE ODYSSEY 3D SDK, IN THE PLUGIN -- engine stereo without Samsung's SDK, its registry key or a cvar ini.
//
// 2.5 renders the two-eye frame (render feature 98) only through its Samsung Odyssey 3D wrapper (platform+0x4680,
// built by sub_142282E60 for every launch): the wrapper loads Odyssey3DSDK.dll from the path under
// HKCU\SOFTWARE\Samsung\Odyssey3DHub, makes the SDK's factory and, from it, the interface the frame gate asks every
// frame (sub_14227E5D0: [Odyssey3D] cvar && wrapper && interface Available()). The port used to need a stand-in DLL
// with that key (tools/odyssey3d_fake_sdk) and an engine/config ini setting the cvars. Here the wrapper's constructor
// is followed and, in engine stereo, handed the plugin's own factory and interface -- the contract the stand-in
// implements (tools/odyssey3d_fake_sdk/Odyssey3DSDK.cpp) -- and the three cvars are set in memory.
//
// Startup remains mono until the last frontend controller closes. Creating a menu controller
// can precede completion of the splash/initial loading screen; enabling SBS at creation
// transformed those remaining screens while capture still treated them as full-window UI.
// Once armed, stereo stays on for gameplay and subsequent menu transitions.

#include "Stereo/EngineStereo.hpp"
#include "Stereo/DetourRegistry.hpp"
#include "Stereo/StereoInternal.hpp"

#include <windows.h>
#include <array>
#include <atomic>
#include <cstdint>
#include <cstring>
#include <utility>

extern void Log(const char* fmt, ...);
extern int g_launcherWidth;    // one eye's render size (Core/VrCore.cpp)
extern int g_launcherHeight;

extern "C" {
// Calls into the plugin's SDK: [0] factory, [1] interface, by slot (8 bytes each). A slot outside the contract
// below is logged once and answered 0.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_DebugEngineStereoSdkCalls[2][16] = {};
// 1 once the main menu has been shown (Armed).
__declspec(dllexport) std::atomic<int32_t> CyberpunkVR_EngineStereoArmed{0};
// Main menu controllers initialised and not yet uninitialised (an instance can be made before the last one goes).
__declspec(dllexport) std::atomic<int32_t> CyberpunkVR_EngineStereoMainMenus{0};
}

namespace cvr::engine_stereo {
namespace {

constexpr uintptr_t WRAPPER_CTOR_RVA = 0x2282E60;
// The [Odyssey3D] cvars' values (ConfigVar<bool>, value byte at +0x30 of each 0x38-byte var in .data). Read by the
// frame gate, the camera pose builder sub_1402A59B8, the startup window path and sub_142083400.
constexpr uintptr_t OVERRIDE_ENABLED_RVA = 0x3929C98;   // use StereoEnabled instead of the user setting
constexpr uintptr_t STEREO_ENABLED_RVA = 0x3929CD0;
constexpr uintptr_t SBS_DEBUG_RVA = 0x3929D08;          // MapStereo3D (sub_142284580) keeps the raw side-by-side
// The wrapper (sub_142282E60): +0 loaded, +8 factory, +24 create_odyssey3d_factory_instance,
// +32 destroy_odyssey3d_factory_instance; sub_142284C50 then makes the interface (+16) from the factory.
constexpr size_t kLoaded = 0, kFactory = 8, kCreate = 24, kDestroy = 32;

std::atomic<bool> g_armed{false};

void Count(int table, int slot) {
    if (slot >= 0 && slot < 16) CyberpunkVR_DebugEngineStereoSdkCalls[table][slot].fetch_add(1, std::memory_order_relaxed);
}

void WriteCvars(bool stereo) {
    if (!detail::g_exe_base) return;
    auto* base = reinterpret_cast<volatile uint8_t*>(detail::g_exe_base);
    base[OVERRIDE_ENABLED_RVA] = 1;
    base[STEREO_ENABLED_RVA] = stereo ? 1 : 0;
    base[SBS_DEBUG_RVA] = 1;
}

struct Object { void* const* vtable; };
constexpr int kSlots = 16;

template <int Table, int Slot>
uint64_t Unknown(void*, void*, void*, void*) {
    if (CyberpunkVR_DebugEngineStereoSdkCalls[Table][Slot].fetch_add(1, std::memory_order_relaxed) == 0)
        Log("[engine-stereo-sdk] %s slot +%d called -- not in the contract, answered 0\n", Table ? "interface" : "factory",
            Slot * 8);
    return 0;
}
template <int Table, size_t... I>
std::array<void*, kSlots> UnknownTable(std::index_sequence<I...>) {
    return {reinterpret_cast<void*>(&Unknown<Table, static_cast<int>(I)>)...};
}

// ---- interface (MSVC returns the structs through a hidden pointer: each takes `ret` and returns it) ----------

// The monitor's head-tracked viewer, millimetres (sub_142284AA0 scales by 0.001). The separation and convergence
// the eyes are built with are the port's (EngineStereo.cpp getters); these are the stand-in's values.
float* EyePosition(Object*, float* ret, bool right) {
    Count(1, 1);
    ret[0] = right ? 32.0f : -32.0f;
    ret[1] = 0.0f;
    ret[2] = 600.0f;
    return ret;
}
// The display: the window the port made, both eyes wide.
float* Resolution(Object*, float* ret) {
    Count(1, 2);
    ret[0] = float(2 * g_launcherWidth);
    ret[1] = float(g_launcherHeight);
    return ret;
}
// A 27" 16:9 panel, the Odyssey 3D's size, in centimetres (the wrapper keeps x * 0.01).
float* PhysicalSize(Object*, float* ret) {
    Count(1, 3);
    ret[0] = 59.77f;
    ret[1] = 33.62f;
    return ret;
}
// Compared by the user-settings path only (sub_14226F850): zero never matches a monitor, no mode switch.
int64_t* MonitorRect(Object*, int64_t* ret) {
    Count(1, 6);
    std::memset(ret, 0, sizeof(int64_t) * 4);
    return ret;
}
// Once a frame (sub_1422854F0), result unused.
uint64_t FrameTick(Object*) {
    Count(1, 9);
    return 0;
}
// The gate: stereo from the main menu on.
bool Available(Object*) {
    Count(1, 10);
    return g_armed.load(std::memory_order_relaxed);
}
void* DeleteInterface(Object* self, unsigned flags) {
    Count(1, 12);
    if (flags & 1) delete self;
    return self;
}

struct InterfaceVtable {
    std::array<void*, kSlots> slots = UnknownTable<1>(std::make_index_sequence<kSlots>{});
    InterfaceVtable() {
        slots[1] = reinterpret_cast<void*>(&EyePosition);
        slots[2] = reinterpret_cast<void*>(&Resolution);
        slots[3] = reinterpret_cast<void*>(&PhysicalSize);
        slots[6] = reinterpret_cast<void*>(&MonitorRect);
        slots[9] = reinterpret_cast<void*>(&FrameTick);
        slots[10] = reinterpret_cast<void*>(&Available);
        slots[12] = reinterpret_cast<void*>(&DeleteInterface);
    }
};
const InterfaceVtable g_interfaceVtable;

// ---- factory -------------------------------------------------------------------------------------------------

Object** CreateInterface(Object*, Object** ret) {
    Count(0, 0);
    *ret = new Object{g_interfaceVtable.slots.data()};
    return ret;
}
// MapStereo3D asks for Samsung's weaver only without SBSDebug: an empty pair, which the engine checks before use.
void** CreateWeaver(Object*, void** ret, void*, void*, void*, void*, void*, void*, void*) {
    Count(0, 2);
    ret[0] = nullptr;
    ret[1] = nullptr;
    return ret;
}

struct FactoryVtable {
    std::array<void*, kSlots> slots = UnknownTable<0>(std::make_index_sequence<kSlots>{});
    FactoryVtable() {
        slots[0] = reinterpret_cast<void*>(&CreateInterface);
        slots[2] = reinterpret_cast<void*>(&CreateWeaver);
    }
};
const FactoryVtable g_factoryVtable;

void* CreateFactory() { return new Object{g_factoryVtable.slots.data()}; }
void DestroyFactory(void* factory) { delete static_cast<Object*>(factory); }

using DestroyFn = void (*)(void*);
using WrapperCtorFn = uint8_t*(__fastcall*)(uint8_t*);
WrapperCtorFn g_orig_wrapper_ctor = nullptr;

uint8_t* __fastcall Detour_WrapperCtor(uint8_t* wrapper) {
    uint8_t* result = g_orig_wrapper_ctor(wrapper);
    if (!wrapper) return result;
    // An SDK found through the registry (Samsung's, or the old stand-in) is let go: the headset's stereo is the
    // port's, never a monitor's.
    if (wrapper[kLoaded]) {
        auto destroy = *reinterpret_cast<DestroyFn*>(wrapper + kDestroy);
        if (destroy) destroy(*reinterpret_cast<void**>(wrapper + kFactory));
        Log("[engine-stereo-sdk] an Odyssey 3D SDK from the registry was loaded; replaced by the plugin's\n");
    }
    *reinterpret_cast<void**>(wrapper + kCreate) = reinterpret_cast<void*>(&CreateFactory);
    *reinterpret_cast<void**>(wrapper + kDestroy) = reinterpret_cast<void*>(&DestroyFactory);
    *reinterpret_cast<void**>(wrapper + kFactory) = CreateFactory();
    wrapper[kLoaded] = 1;
    WriteCvars(g_armed.load(std::memory_order_relaxed));
    Log("[engine-stereo-sdk] Odyssey 3D wrapper %p given the plugin's SDK; stereo from the main menu on\n",
        static_cast<void*>(wrapper));
    return result;
}

}  // namespace

void NoteMainMenu() {
    if (!Enabled()) return;
    CyberpunkVR_EngineStereoMainMenus.fetch_add(1, std::memory_order_relaxed);
    // Controller creation precedes the end of the splash. Keep startup mono.
}

void NoteMainMenuClosed() {
    int32_t open = CyberpunkVR_EngineStereoMainMenus.load(std::memory_order_relaxed);
    while (open > 0 && !CyberpunkVR_EngineStereoMainMenus.compare_exchange_weak(open, open - 1, std::memory_order_relaxed)) {
    }
    // Arm only after the last frontend controller closes, never on its creation.
    // Once armed, gameplay retains stereo (including return-to-menu transitions).
    if (Enabled() && open == 1 && !g_armed.exchange(true, std::memory_order_relaxed)) {
        CyberpunkVR_EngineStereoArmed.store(1, std::memory_order_relaxed);
        WriteCvars(true);
        Log("[engine-stereo-sdk] frontend closed: engine stereo enabled\n");
    }
}

bool Armed() { return g_armed.load(std::memory_order_relaxed); }

bool MainMenuUp() { return CyberpunkVR_EngineStereoMainMenus.load(std::memory_order_relaxed) > 0; }

CVR_DETOUR_IF("[engine-stereo] Odyssey 3D SDK in process sub_142282E60", WRAPPER_CTOR_RVA, Detour_WrapperCtor,
              g_orig_wrapper_ctor, Enabled)

}  // namespace cvr::engine_stereo
