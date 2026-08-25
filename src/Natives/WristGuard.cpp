// WristGuard -- keeps the forearm readout from sitting there as a black rectangle.
//
// WHAT THE FAULT IS, measured rather than reasoned (the full record is in the project memory note
// `worldui-on-fpp-arm-plane`; the decompilation is in engine_re/dumps/O_uitex_binder.md):
//
// The readout is a WorldWidgetComponent (`vrp_latd_ui`) rendering onto a skinned surface on the left
// forearm. The surface carries renderingPlaneAnimationParam = "renderPlane", the same value the game's
// own arm sticker carries, so the animation graph moves it into the WEAPON rendering plane whenever a
// weapon is drawn -- which is the only way the readout is not covered by the arms.
//
// That transition destroys the widget's UI render target. Found by taking two 944-byte snapshots of the
// live component in the SAME state (to learn which bytes are per-frame noise -- the two transforms) and
// only then comparing against the other state. Exactly one field survives that filter:
//
//     component + 0x220   the object    |    component + 0x228   its refcount block
//
// While the readout works, +0x220 points at a target object. After a weapon is drawn that object has
// been freed -- its memory reads as allocator poison -- and the handle points at a different object
// (a target wrapper of another class, alive, owning its own resource memory).
//
// And the reason that shows as BLACK rather than as nothing: `UIRenderTexture` is written into the
// material by the engine's binder only while it can lock that handle; otherwise it returns having
// written nothing, and the material samples its own default. The band's material,
// parallaxscreen_transparent_ui.mt, blends premultiplied (FAC_One + FAC_InvSrcAlpha), so a default
// sample is an opaque black rectangle. The mesh draws perfectly the whole time -- it is simply never
// handed a texture.
//
// WHAT THIS DOES. One 8-byte read per frame. When the pointer at +0x220 changes, the target has been
// swapped, so the component is toggled off and on once -- measured to be the only thing that gets a
// fresh target (RefreshAppearance alone does not, and neither does rewriting meshTargetBinding or
// sceneWidgetProperties). Then it goes quiet again.
//
// WHY THE PLUGIN AND NOT THE LUA MOD. The mod could only guess: it fired the repair a fixed delay after
// the weapon slot changed, because the plane transition lands somewhere inside the draw animation. Too
// early and nothing is fixed, too late and the black is visible for that long, and either way it
// sometimes missed entirely. This watches the thing that actually breaks.
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Functions.hpp>
#include <RED4ext/Scripting/Natives/ScriptGameInstance.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/Entity.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/IComponent.hpp>

#include <cstdint>

#include "Core/VrCoreShared.hpp"   // g_hasWeaponEquipped -- already maintained by the camera hook
#include "Natives/NativeFunctions.hpp"
// NativeHelpers declares helpers over anim and world types, so those headers have to be in scope first
// -- every other translation unit that includes it happens to pull them in through its own list.
#include <RED4ext/Containers/StaticArray.hpp>
#include <RED4ext/Scripting/Natives/animAnimatedObject.hpp>
#include <RED4ext/Scripting/Natives/Generated/world/AnimationSystem.hpp>
#include "Natives/NativeHelpers.hpp"

extern void Log(const char* fmt, ...);

namespace
{
// The component the readout lives on, and the one whose class layout the offset below belongs to.
constexpr const char* kWidgetComponentName = "vrp_latd_ui";
constexpr const char* kWidgetClassName = "WorldWidgetComponent";

// The handle to the UI render target. Located by diffing the live object, not by reading a header:
// the class registers with an instance size of 928 bytes, so 0x220+8 is safely inside it. The class
// name is checked before the offset is used, so this can never be applied to a different layout.
constexpr size_t kTargetHandleOffset = 0x220;

// Frames to stay quiet after a repair. The transition swaps the pointer once, so one repair settles it;
// the cooldown is only there so that a wrong assumption cannot turn this into a per-frame strobe.
constexpr int kCooldownFrames = 20;

// entIComponent identity fields, the same three LiveProjectile.cpp validates a cached pointer with.
constexpr size_t kNameOffset = 0x40;   // name, a CName (8-byte hash)
constexpr size_t kIdOffset   = 0x60;   // id, a CRUID -- nonzero once constructed

void* g_lastTarget = nullptr;      // the pointer seen on the previous tick
void* g_component = nullptr;       // which component that pointer was read from
void* g_componentVtbl = nullptr;   // its vptr, so a recycled allocation cannot pass validation
int   g_cooldown = 0;
int32_t g_repairs = 0;             // how many repairs this session
int32_t g_lastResult = 0;
int32_t g_walks = 0;               // how many times the expensive lookup actually ran
bool g_frozen = false;             // the one freeze-and-re-arm has been done for this player object
// WHICH object it was done for. Without this the flag outlived the entity: after a death or a save load
// the player is a NEW entity with NEW components, the asset gives its surface renderingPlaneAnimationParam
// = "renderPlane" again, and the freeze is owed again -- but the flag said done, so the recipe never ran
// and the target died on every single equip, with the watcher re-arming each time. Reported exactly that
// way: "после смерти указатель умирает каждый раз. и реармится на каждый equip".
void* g_frozenFor = nullptr;
// How many consecutive calls a weapon must already have been in hand, with the render target standing
// still, before the freeze is performed without ever having seen a transition. Not one frame: the
// animation graph has to have reached the weapon-out state, or the surface would be frozen in the wrong
// plane and stay black. Counted in calls, and the mod polls this per frame until the freeze.
constexpr int kSettleFrames = 45;
int g_settle = 0;
void* g_surface = nullptr;         // the skinned surface, cached the same way as the widget
void* g_surfaceVtbl = nullptr;

// How often the cached pointers are re-fetched through the entity's component array. That path is the
// safe one (the engine owns the array), so it is what catches a replaced component -- and at once every
// 120 calls it costs nothing measurable.
constexpr int kRewalkPeriod = 120;
int g_sinceRewalk = 0;

// THE CACHED ONE, CHECKED CHEAPLY. Three reads and no script-VM entry: the vptr must be the one this
// component had when it was first resolved, the name CName must still be ours, and the id must be
// nonzero. Anything else means the object was replaced (a load, a respawn) and the slow path runs.
// NO IsReadable ON THIS PATH, and that is a measurement: IsReadable is VirtualQuery, and in this process
// -- 5051 memory regions, 14 GB committed, streaming running -- one call costs about 13.5 ms. Timed from
// Lua: mode 2 (no lookup) 0.00 ms, mode 4 (two IsReadable) 28.65 ms, mode 5 (three) 40.40 ms per call.
// That was the 17 fps, and it had nothing to do with the cache, which never missed.
//
// What guards the reads instead: the pointer came from the engine's own component array, and it is
// re-fetched through that array every kRewalkPeriod calls, so a replaced object is picked up. Between
// those, three plain reads at fixed offsets are enough to notice it is no longer ours.
bool CachedComponentStillValid()
{
    if (!g_component || !g_componentVtbl)
        return false;
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_component);
    if (*reinterpret_cast<void**>(base) != g_componentVtbl)
        return false;
    static const uint64_t wantedName = RED4ext::CName(kWidgetComponentName).hash;
    if (*reinterpret_cast<uint64_t*>(base + kNameOffset) != wantedName)
        return false;
    return *reinterpret_cast<uint64_t*>(base + kIdOffset) != 0;
}

// The expensive path: a script-VM call for the player plus a walk of its components. This is what must
// not run per frame -- with it on every tick the frame rate went from 60 to 35.
RED4ext::ent::IComponent* WalkForWidgetComponent()
{
    auto* entity = FindPlayerEntity();
    if (!entity)
        return nullptr;

    ++g_walks;
    const RED4ext::CName wanted(kWidgetClassName);
    for (auto& handle : entity->components)
    {
        auto* component = handle.instance;
        if (!component)
            continue;
        const char* name = component->name.ToString();
        if (!name || !EqualsInsensitive(name, kWidgetComponentName))
            continue;
        RED4ext::CClass* type = component->GetType();
        if (!type || !ClassIsA(type, wanted))
            return nullptr;          // the name is ours but the layout is not: refuse the offset
        return component;
    }
    return nullptr;
}

RED4ext::ent::IComponent* FindWidgetComponent()
{
    if (++g_sinceRewalk >= kRewalkPeriod)
    {
        g_sinceRewalk = 0;
        g_component = nullptr;      // force one trip through the entity's array
        g_surface = nullptr;
    }
    if (CachedComponentStillValid())
        return reinterpret_cast<RED4ext::ent::IComponent*>(g_component);

    auto* component = WalkForWidgetComponent();
    if (!component)
    {
        g_component = nullptr;
        g_componentVtbl = nullptr;
        return nullptr;
    }
    g_component = component;
    g_componentVtbl = *reinterpret_cast<void**>(component);
    return component;
}

// ---- the surface, cached and validated the same way --------------------------------------------
constexpr const char* kSurfaceComponentName = "vrp_latd_screen";

bool CachedSurfaceStillValid()
{
    if (!g_surface || !g_surfaceVtbl)
        return false;
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_surface);
    if (*reinterpret_cast<void**>(base) != g_surfaceVtbl)
        return false;
    static const uint64_t wantedName = RED4ext::CName(kSurfaceComponentName).hash;
    if (*reinterpret_cast<uint64_t*>(base + kNameOffset) != wantedName)
        return false;
    return *reinterpret_cast<uint64_t*>(base + kIdOffset) != 0;
}

RED4ext::ent::IComponent* FindSurfaceComponent()
{
    if (CachedSurfaceStillValid())
        return reinterpret_cast<RED4ext::ent::IComponent*>(g_surface);

    auto* entity = FindPlayerEntity();
    if (!entity)
        return nullptr;
    ++g_walks;
    for (auto& handle : entity->components)
    {
        auto* component = handle.instance;
        if (!component)
            continue;
        const char* name = component->name.ToString();
        if (!name || !EqualsInsensitive(name, kSurfaceComponentName))
            continue;
        g_surface = component;
        g_surfaceVtbl = *reinterpret_cast<void**>(component);
        return component;
    }
    g_surface = nullptr;
    g_surfaceVtbl = nullptr;
    return nullptr;
}

// Clearing the plane param is what freezes the surface where the draw event left it -- the weapon plane.
// Written through the RTTI property rather than at a guessed offset, the way src/Core/VrCore.cpp resolves
// the player's own fields.
bool SetPlaneParam(RED4ext::ent::IComponent* aSurface, const char* aValue)
{
    RED4ext::CClass* type = aSurface->GetType();
    auto* prop = type ? type->GetProperty("renderingPlaneAnimationParam") : nullptr;
    if (!prop)
        return false;
    prop->SetValue<RED4ext::CName>(aSurface, RED4ext::CName(aValue));
    return true;
}

// The one repair that works, and it is the component's own scripted Toggle -- writing isEnabled would
// only change the field, not run the attach path that acquires a target.
bool ToggleComponent(RED4ext::ent::IComponent* aComponent)
{
    RED4ext::CClass* type = aComponent->GetType();
    auto* func = type ? type->GetFunction("Toggle") : nullptr;
    if (!func)
        return false;

    bool off = false;
    bool on = true;
    RED4ext::StackArgs_t argsOff;
    argsOff.emplace_back(nullptr, &off);
    RED4ext::StackArgs_t argsOn;
    argsOn.emplace_back(nullptr, &on);

    // Both calls in the same frame, so the disabled state is never rendered.
    const bool a = RED4ext::ExecuteFunction(aComponent, func, nullptr, argsOff);
    const bool b = RED4ext::ExecuteFunction(aComponent, func, nullptr, argsOn);
    return a && b;
}
} // namespace

// mode 0 -- tick: look, and repair if the target was swapped
// mode 1 -- forget what was seen, adopt whatever is there now (after a load, or to re-arm by hand)
// mode 2 -- report only: how many repairs so far, no side effects
// mode 3 -- report only: how many times the expensive component walk ran
// mode 4 -- detect only: 1 if the render-target pointer changed since the last mode-4 call
// mode 5 -- the whole recipe: freeze the surface in the weapon plane and re-arm, once; after that
//           it keeps watching and re-arms again if the target ever dies (returns 3 when it did)
// mode 6 -- forget that, so mode 5 runs again on the next draw
//
// returns  0 nothing to do,  1 repaired,  2 adopted the first sample,  3 in cooldown,
//          negative: -1 no player, -2 no component (or the wrong class), -3 the target field is
//          unreadable, -4 Toggle is not on the class
void VRWristGuard(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;

    if (mode == 2)
    {
        if (aOut) *aOut = g_repairs;
        return;
    }
    if (mode == 3)
    {
        // how many times the expensive lookup ran; it should stay in the single digits per session
        if (aOut) *aOut = g_walks;
        return;
    }

    // mode 5 -- THE WHOLE RECIPE, once per session, and no Lua involvement beyond calling it.
    //
    // Uses g_hasWeaponEquipped, which the plugin already maintains in src/Hooks/LocateCamera.cpp -- the
    // Lua version asked the transaction system for the weapon slot every frame instead, and that window
    // ran at 24 fps.
    //
    // returns 0 waiting, 1 just done, 2 already done, negative as elsewhere
    if (mode == 5)
    {
        // ALREADY FROZEN, BUT STILL WATCHING. After the freeze there are no plane transitions, so the
        // known killer of the render target is gone -- but "no known killer" is not "nothing can happen":
        // photo mode, a cutscene, a vehicle, or anything else that re-registers the component would drop
        // the target again, and with the mod no longer calling this there would be nothing to notice.
        // So the check keeps running (it is three reads per component now) and re-arms the widget if the
        // pointer ever changes again. The cooldown is there so a mistaken premise cannot become a strobe.
        auto* wCur = FindWidgetComponent();
        if (g_frozen && wCur && wCur != g_frozenFor)
        {
            // a different component object: a new player entity, so the freeze is owed again
            g_frozen = false;
            g_frozenFor = nullptr;
            g_lastTarget = nullptr;
            g_settle = 0;
            g_cooldown = 0;
            Log("[wrist] the player entity was replaced -> the freeze is owed again");
        }
        if (g_frozen)
        {
            if (g_cooldown > 0)
            {
                --g_cooldown;
                if (aOut) *aOut = 2;
                return;
            }
            auto* w2 = wCur;
            if (!w2)
            {
                if (aOut) *aOut = 2;
                return;
            }
            void* t2 = *reinterpret_cast<void**>(reinterpret_cast<uintptr_t>(w2) + kTargetHandleOffset);
            if (g_lastTarget && t2 != g_lastTarget)
            {
                ToggleComponent(w2);
                ++g_repairs;
                g_cooldown = kCooldownFrames;
                g_lastTarget = *reinterpret_cast<void**>(reinterpret_cast<uintptr_t>(w2) + kTargetHandleOffset);
                Log("[wrist] the target died again after the freeze -> re-armed (%d)", g_repairs);
                if (aOut) *aOut = 3;
                return;
            }
            g_lastTarget = t2;
            if (aOut) *aOut = 2;
            return;
        }
        auto* widget = wCur;
        auto* surface = FindSurfaceComponent();
        if (!widget || !surface)
        {
            g_lastTarget = nullptr;
            if (aOut) *aOut = -2;
            return;
        }
        const uintptr_t s5 = reinterpret_cast<uintptr_t>(widget) + kTargetHandleOffset;
        // the handle sits inside a 928-byte component whose vptr was just checked; no OS query needed
        void* t5 = *reinterpret_cast<void**>(s5);
        const bool first = (g_lastTarget == nullptr);
        const bool moved = (!first && t5 != g_lastTarget);
        g_lastTarget = t5;

        // THE DRAW EVENT IS WHAT CARRIES THE SURFACE INTO THE WEAPON PLANE, and the target pointer
        // changing is that event -- WHEN WE ARE THERE TO SEE IT.
        //
        // Spawn with the weapon already out and it happened before this native ever ran: the first call
        // only records the pointer, so `moved` is false by construction, and afterwards the pointer stands
        // still. The freeze was then never performed and every later draw moved the surface again -- black
        // on each weapon switch, until a holster and a draw finally produced a change. Reported exactly
        // that way, and it is also why it shows up after dying: the respawn owes the freeze again and the
        // player usually comes back holding something.
        //
        // So a weapon that is ALREADY in hand counts as the transition, once the picture has settled.
        // Never on the first frame: the graph has to have reached the weapon-out state, or this would
        // freeze the surface in the wrong plane and it would be black for good.
        bool ready = moved && g_hasWeaponEquipped;
        const char* how = "caught the plane transition";
        if (!ready)
        {
            if (g_hasWeaponEquipped && !moved && t5 != nullptr && ++g_settle >= kSettleFrames)
            {
                ready = true;
                how = "the weapon was already in hand";
            }
            else if (!g_hasWeaponEquipped || t5 == nullptr)
            {
                g_settle = 0;
            }
        }
        if (!ready)
        {
            if (aOut) *aOut = 0;
            return;
        }
        if (!SetPlaneParam(surface, "None"))
        {
            if (aOut) *aOut = -5;
            return;
        }
        if (!ToggleComponent(widget))
        {
            if (aOut) *aOut = -4;
            return;
        }
        g_frozen = true;
        g_frozenFor = widget;
        g_settle = 0;
        g_lastTarget = *reinterpret_cast<void**>(s5);
        Log("[wrist] %s -> frozen in the weapon plane and re-armed", how);
        if (aOut) *aOut = 1;
        return;
    }

    // mode 6 -- forget that it was done, so the recipe runs again on the next draw
    if (mode == 6)
    {
        g_frozen = false;
        g_frozenFor = nullptr;
        g_lastTarget = nullptr;
        g_settle = 0;
        if (aOut) *aOut = 0;
        return;
    }

    // mode 4 -- DETECT ONLY, no repair. This is what removes the visible black: the caller wants to know
    // the exact frame the surface changed rendering plane, and the render-target pointer being swapped IS
    // that frame. Waiting a fixed delay after the weapon slot fills meant the readout sat black for that
    // whole delay; asked this way it is one frame.
    if (mode == 4)
    {
        auto* c = FindWidgetComponent();
        if (!c)
        {
            g_lastTarget = nullptr;
            if (aOut) *aOut = -2;
            return;
        }
        const uintptr_t s4 = reinterpret_cast<uintptr_t>(c) + kTargetHandleOffset;
        // the handle sits inside a 928-byte component whose vptr was just checked; no OS query needed
        void* t4 = *reinterpret_cast<void**>(s4);
        const bool first = (g_lastTarget == nullptr);
        const bool changed = (!first && t4 != g_lastTarget);
        g_lastTarget = t4;
        if (aOut) *aOut = changed ? 1 : 0;
        return;
    }

    auto* component = FindWidgetComponent();
    if (!component)
    {
        // A load screen or a respawn: the component will be a different object, so nothing carries over.
        g_lastTarget = nullptr;
        g_component = nullptr;
        g_componentVtbl = nullptr;
        g_surface = nullptr;
        g_surfaceVtbl = nullptr;
        g_frozen = false;
        g_frozenFor = nullptr;
        g_cooldown = 0;
        g_lastResult = -1;
        if (aOut) *aOut = FindPlayerEntity() ? -2 : -1;
        return;
    }

    const uintptr_t slot = reinterpret_cast<uintptr_t>(component) + kTargetHandleOffset;
    void* target = *reinterpret_cast<void**>(slot);

    if (mode == 1 || component != g_component || g_lastTarget == nullptr)
    {
        g_component = component;
        g_lastTarget = target;
        g_cooldown = 0;
        g_lastResult = 2;
        if (aOut) *aOut = 2;
        return;
    }

    if (g_cooldown > 0)
    {
        --g_cooldown;
        // adopt whatever settled during the cooldown, so the next change is measured from here
        g_lastTarget = target;
        g_lastResult = 3;
        if (aOut) *aOut = 3;
        return;
    }

    if (target == g_lastTarget)
    {
        g_lastResult = 0;
        if (aOut) *aOut = 0;
        return;
    }

    // The target was swapped -- this is the frame the readout went black.
    if (!ToggleComponent(component))
    {
        g_lastTarget = target;
        g_lastResult = -4;
        if (aOut) *aOut = -4;
        return;
    }

    ++g_repairs;
    g_cooldown = kCooldownFrames;
    g_lastTarget = *reinterpret_cast<void**>(slot);
    g_lastResult = 1;
    Log("[wrist] render target swapped -> repaired (%d this session)", g_repairs);
    if (aOut) *aOut = 1;
}
