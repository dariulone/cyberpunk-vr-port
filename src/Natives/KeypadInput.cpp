#include "Natives/NativeFunctions.hpp"
#include "Hooks/KeypadInput.hpp"
#include "Core/VrCoreShared.hpp"
#include "Core/LiveControls.hpp"
#include "Overlay/VrOverlay.hpp"
#include "Hooks/Hook.hpp"
#include "Utils/DebugGate.hpp"
#include <MinHook.h>
#include <RED4ext/Scripting/Natives/inkWidget.hpp>
#include <RED4ext/Scripting/Natives/inkLayer.hpp>
#include <RED4ext/Scripting/Natives/inkLayerProxy.hpp>
#include <RED4ext/Scripting/Natives/entIComponent.hpp>
#include <atomic>
#include <bit>
#include <chrono>
#include <memory>
#include <mutex>

extern "C" {
__declspec(dllexport) uint64_t CyberpunkVR_KeypadRoot{};
__declspec(dllexport) uint64_t CyberpunkVR_KeypadLayer{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_KeypadHeadHits{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_KeypadHeldHits{};
__declspec(dllexport) float CyberpunkVR_KeypadCursor[2]{};
}

namespace {
using namespace RED4ext;
using Clock = std::chrono::steady_clock;
std::atomic<uint64_t> activeStamp{}, stickStamp{}, stickBits{};
std::atomic<bool> hookReady{};
struct Scope {
    WeakHandle<ink::Widget> root, window;
    WeakHandle<ink::Layer> layer;
};
std::atomic<std::shared_ptr<const Scope>> scope;

// CP2077 2.31's world-widget hit record (RVA 8BB5B0 / 2446050 / 24468B8).
// These are WEAK handles, including the game's temporary output. Copying them
// with the SDK preserves the same native weak-reference ownership.
struct WorldHit {
    WeakHandle<ink::Widget> window;
    WeakHandle<ent::IComponent> component;
    Vector2 local, cursor, screen;
};
static_assert(sizeof(WorldHit) == 56);
static_assert(offsetof(WorldHit, local) == 32);
static_assert(offsetof(WorldHit, cursor) == 40);
static_assert(offsetof(WorldHit, screen) == 48);
using QueryFn = WorldHit* (*)(uintptr_t, WorldHit*, const Vector2*);
QueryFn originalQuery{};

struct CursorState {
    std::shared_ptr<const Scope> owner;
    WorldHit hit{};
    cvr::input::KeypadCursor motion;
    Clock::time_point time{};
    uintptr_t tester{};
    uint64_t device{};
};
std::mutex cursorMutex;
// Never destroyed: released at process exit it would call into the engine after it is gone (crash on quit, 2026-11-23).
CursorState& cursorState = *new CursorState();

bool Visible(Handle<ink::Widget> widget) {
    for (int i=0; widget && i<24; ++i) {
        if (!widget->visible || widget->opacity <= 0) return false;
        widget = widget->parentWidget.Lock();
    }
    return !widget;
}

WorldHit* Query(uintptr_t tester, WorldHit* out, const Vector2* screen) {
    auto* result = originalQuery(tester, out, screen);
    const auto current = scope.load(std::memory_order_acquire);
    std::lock_guard lock(cursorMutex);
    auto& state = cursorState;
    if (!current || !cvr::input::KeypadInputActive()) {
        state = {}; return result;
    }
    const auto root = current->root.Lock();
    const auto window = current->window.Lock();
    const auto layer = current->layer.Lock();
    // The native tester belongs to this world layer and already has the exact
    // DeviceZoom entity selected. Do not keep a panel after native zoom exits.
    const auto device = *reinterpret_cast<const uint64_t*>(tester + 0xA8);
    if (!root || !window || !layer || !Visible(root) || !device ||
        *reinterpret_cast<const uintptr_t*>(reinterpret_cast<uintptr_t>(layer.GetPtr()) + 0x1A8) != tester) {
        state = {}; return result;
    }
    if (state.owner != current || state.tester != tester || state.device != device) {
        state = {}; state.owner = current; state.tester = tester; state.device = device;
    }
    const bool headHit = out->window.instance == window.GetPtr() && bool(out->component.Lock());
    if (headHit) state.hit = *out;
    // CET may identify the keypad a frame after the player looks away. Use the
    // tester's last genuine hit to arm it, but only for this exact window.
    if (!state.hit.window.instance) {
        const auto& previous = *reinterpret_cast<const WorldHit*>(tester + 0x40);
        if (previous.window.instance == window.GetPtr() && previous.component.Lock())
            state.hit = previous;
    }
    if (state.hit.window.instance != window.GetPtr() || !state.hit.component.Lock()) {
        state = {}; return result;
    }
    const auto now = Clock::now();
    const float dt = state.time.time_since_epoch().count() ? std::chrono::duration<float>(now-state.time).count() : 0.f;
    state.time = now;
    cvr::input::KeypadPoint stick{};
    const auto stamp = stickStamp.load(std::memory_order_acquire);
    if (stamp && GetTickCount64() - stamp <= 100)
        stick = std::bit_cast<cvr::input::KeypadPoint>(stickBits.load(std::memory_order_relaxed));

    // Native hit mapping: local = authored cursor * window render scale.
    // The native picker and cursor renderer keep handling hover/click events.
    const auto scale = window->renderTransform.scale;
    if (!std::isfinite(scale.X) || !std::isfinite(scale.Y) || scale.X <= 0 || scale.Y <= 0) {
        state = {}; return result;
    }
    const cvr::input::KeypadPoint size{window->size.X, window->size.Y};
    if (!state.motion.initialized) {
        state.motion.Update(true, {state.hit.cursor.X, state.hit.cursor.Y}, {}, size, 0);
        state.motion.hadHead = headHit;
    }
    if (!state.motion.Update(headHit, {out->cursor.X, out->cursor.Y}, stick, size, dt)) return result;
    *out = state.hit;
    out->cursor = {state.motion.position.x, state.motion.position.y};
    out->local = {out->cursor.X * scale.X, out->cursor.Y * scale.Y};
    out->screen = *screen;
    CVR_DIAGNOSTIC(
        (headHit ? CyberpunkVR_KeypadHeadHits : CyberpunkVR_KeypadHeldHits).fetch_add(1, std::memory_order_relaxed);
        CyberpunkVR_KeypadCursor[0] = out->cursor.X;
        CyberpunkVR_KeypadCursor[1] = out->cursor.Y;
    );
    return result;
}

bool InstallKeypadCursor() {
    // Verified complete prologue, not a broad pointer or widget patch. A game
    // version mismatch leaves the native input path untouched.
    const uint8_t bytes[]={0x48,0x8b,0xc4,0x48,0x89,0x58,0x10,0x55,0x56,0x57,0x41,0x56,0x41,0x57,
        0x48,0x8d,0xa8,0xf8,0xfd,0xff,0xff,0x48,0x81,0xec,0xe0,0x02,0,0};
    auto* address = reinterpret_cast<uint8_t*>(GetModuleHandleW(nullptr)) + 0x750AD4;
    if (memcmp(address, bytes, sizeof(bytes))) return false;
    if (MH_CreateHook(address, reinterpret_cast<void*>(&Query), reinterpret_cast<void**>(&originalQuery)) != MH_OK) return false;
    if (MH_EnableHook(address) != MH_OK) { MH_RemoveHook(address); return false; }
    hookReady.store(true, std::memory_order_release);
    return true;
}
CVR_HOOK("KeypadCursor", ::cvr::hooks::Stage::Boot, 87, InstallKeypadCursor);
} // namespace

bool cvr::input::KeypadInputActive() {
    return hookReady.load(std::memory_order_acquire) && g_menuModeValue == 0 &&
        g_liveControls.xrXInputHook && !cvr::vrui::CapturesInput() &&
        KeypadInputFresh(activeStamp.load(std::memory_order_acquire), GetTickCount64());
}

void cvr::input::PublishKeypadStick(float x, float y, float deadzone) {
    stickBits.store(std::bit_cast<uint64_t>(KeypadPoint{KeypadCursorAxis(x,deadzone),KeypadCursorAxis(y,deadzone)}), std::memory_order_relaxed);
    stickStamp.store(GetTickCount64(), std::memory_order_release);
}

void VRKeypadUpdate(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, int32_t* out, int64_t) {
    using namespace RED4ext;
    Handle<ink::Widget> root;
    GetParameter(frame, &root); ++frame->code;
    Handle<ink::Layer> layer;
    Handle<ink::Widget> window;
    auto current = root;
    for (int i=0; current && i<24; ++i) {
        if (current->GetType()->GetName() == CName("inkVirtualWindow")) window = current;
        const auto proxy = current->layerProxy;
        if (proxy) layer = proxy->layer.Lock();
        current = current->parentWidget.Lock();
    }
    const bool active = root && window && Visible(root) && layer &&
        layer->GetType()->GetName() == CName("inkWorldLayer") && g_menuModeValue == 0 &&
        hookReady.load(std::memory_order_acquire) && !cvr::vrui::CapturesInput();
    const auto previous = scope.load(std::memory_order_acquire);
    if (!active) {
        activeStamp.store(0, std::memory_order_release);
        stickStamp.store(0, std::memory_order_release);
        scope.store({}, std::memory_order_release);
    } else if (!previous || previous->root.instance != root.GetPtr() || previous->window.instance != window.GetPtr()) {
        auto next = std::make_shared<Scope>();
        next->root = root; next->window = window; next->layer = layer;
        stickStamp.store(0, std::memory_order_release);
        scope.store(std::move(next), std::memory_order_release);
    }
    activeStamp.store(active ? GetTickCount64() : 0, std::memory_order_release);
    CVR_DIAGNOSTIC(
        CyberpunkVR_KeypadRoot = reinterpret_cast<uintptr_t>(root.GetPtr());
        CyberpunkVR_KeypadLayer = reinterpret_cast<uintptr_t>(layer.GetPtr());
    );
    if (out) *out = active ? 1 : 0;
}
