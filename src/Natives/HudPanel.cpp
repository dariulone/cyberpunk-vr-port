// Verified against CP2077 SHA256 a7de82945c03e041... with IDA headless.
// Only native separate-window slots are moved; direct/projected HUD widgets stay native.
#include "Natives/NativeFunctions.hpp"
#include "Camera/SurveillanceFollow.hpp"
#include "Render/HudPanel.hpp"
#include "Render/OpticsProbe.hpp"
#include <RED4ext/Scripting/Natives/Generated/ent/RenderToTextureCameraComponent.hpp>
#include "Runtimes/HudLayout.hpp"
#include "Runtimes/HudCrop.hpp"
#include "Camera/GazeCenter.hpp"
#include "Anim/HeadAimWeapon.hpp"
#include "Camera/CameraState.hpp"
#include "Core/VrCoreShared.hpp"
#include "Core/LiveControls.hpp"
#include "Core/GameBuild.hpp"
#include "Stereo/EngineStereo.hpp"
#include "Hooks/Hook.hpp"
#include "Utils/DebugGate.hpp"
#include <MinHook.h>
#include <atomic>
#include <RED4ext/Scripting/Natives/inkLayer.hpp>
#include <RED4ext/Scripting/Natives/inkHUDLayer.hpp>
#include <RED4ext/Scripting/Natives/inkHudWidgetSpawnEntry.hpp>
#include <RED4ext/Scripting/Natives/inkWidget.hpp>
#include <RED4ext/Scripting/Natives/inkLayerProxy.hpp>
#include <RED4ext/Scripting/Natives/Generated/DynamicTexture.hpp>
#include <RED4ext/Scripting/Natives/Generated/ink/ImageWidget.hpp>
#include <RED4ext/Scripting/Natives/Generated/ink/CompoundWidget.hpp>
#include <RED4ext/Scripting/Natives/Generated/ink/HudEntryInfo.hpp>
#include <RED4ext/Scripting/Functions.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <shared_mutex>
#include <unordered_map>
#include <algorithm>
#include <cmath>

extern "C" {
// The HUD root's size in its pixels, as VRHudPanelUpdate last saw it; 0 until then (HudDesignScale).
__declspec(dllexport) float CyberpunkVR_HudRootWidth = 0, CyberpunkVR_HudRootHeight = 0;
// The HUD's ink layer and its virtual window as VRHudPanelUpdate last saw them (diagnostics).
__declspec(dllexport) uintptr_t CyberpunkVR_HudLayerAddress[2] = {};
// 1 (default): a HUD window with nothing visible under its root (HasVisibleContent) is not drawn,
// and a channel with only such windows builds no canvas and submits no layer. 0 draws everything.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_HudSkipEmpty{1};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_HudEmptySprites{0};
// VRHudPanelUpdate cost, QPC ticks: [0] calls, [1] whole call, [2] Geometry, [3] Texture, [4] HasVisibleContent,
// [5] separate-window entries walked, [6] masks and publish after the walk, [7] entries that became sprites.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_HudPanelTiming[8]{};
// 1 (default): the per-frame upkeep of the HUD roots modules/hud_panel.lua finds once a second (crop bounds,
// window size, repaint, the loot test) runs here inside VRHudPanelUpdate; the script only hands the roots
// over. 0: the script does it, as before (A/B; the script switches at its next scan).
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_HudRootsNative{1};
// [0] frames with upkeep, [1] roots walked, [2] of them active, [3] crop size writes, [4] crop offset writes,
// [5] window SetSize calls, [6] SetSize calls that failed, [7] subtitle crop writes.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_HudRootsStats[8]{};
// 1 (default): the upkeep's repaints run at the frame submit (see REPAINTS AT THE FRAME SUBMIT). 0: at once.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_HudRepaintAtSubmit{1};
// [0] repaints queued, [1] run at the frame submit, [2] run late in the next update (no frame submit between).
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_HudRepaintStats[3]{};
}
namespace {
using namespace RED4ext;
const uintptr_t base = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
using ChildRectFn = Vector2* (*)(ink::Widget*, Vector2*, const Handle<ink::Widget>*);
using VisibleFn = uintptr_t (*)(ink::Widget*, bool);
using InvalidateFn = void (*)(ink::Widget*);
using CopyEntriesFn = uintptr_t (*)(uintptr_t, DynArray<ink::HudWidgetSpawnEntry>*);
VisibleFn originalVisible = nullptr;
CopyEntriesFn originalCopyEntries = nullptr;
using FrameSubmitFn = void (*)(void*, void*, char);
FrameSubmitFn originalFrameSubmit = nullptr;
void FlushRepaintsAtSubmit();
std::atomic<bool> hooksReady{false};
bool Verified() {
    static const bool ok = [] {
        if (!cvr::game_build::Supported()) return false;   // the natives stay callable on any build
        const uint8_t layout[] = {0x40,0x53,0x48,0x83,0xec,0x70,0x48,0x8b,0x89,0xd0,0x01,0,0};
        const uint8_t release[] = {0x48,0x89,0x5c,0x24,0x08,0x57,0x48,0x83,0xec,0x20,0x48,0x8b,0x15,0x4f,0xaa,0x7e,0x03,0x48,0x8b,0xf9,0x80,0x7a};
        return !memcmp(reinterpret_cast<void*>(base+0x15b19e4), layout, sizeof(layout)) &&
               !memcmp(reinterpret_cast<void*>(base+0x15b1990), layout, sizeof(layout)) &&
               !memcmp(reinterpret_cast<void*>(base+0x3e3248), release, sizeof(release));
    }();
    return ok;
}
struct Mask {
    Handle<ink::Widget> widget;
    std::atomic<bool> wantedVisible;
    bool originalAffectsLayout;
    std::atomic<cvr::hud::Channel> channel;
    explicit Mask(const Handle<ink::Widget>& value,cvr::hud::Channel group)
        : widget(value), wantedVisible(value->visible), originalAffectsLayout(value->affectsLayoutWhenHidden),channel(group) {}
};
using MaskSet = std::vector<std::shared_ptr<Mask>>;
std::unordered_map<ink::Widget*, std::shared_ptr<Mask>> masks;
std::atomic<std::shared_ptr<const MaskSet>> publishedMasks;
uint64_t generation = 0;
ink::Widget* lastRoot = nullptr;
std::array<bool,cvr::hud::ChannelCount> lastMasked{};
std::unordered_map<uint64_t,cvr::hud::LayoutLatch> layoutCache;
uint32_t layoutWidth=0,layoutHeight=0;

void HideSlot(ink::Widget* widget) {
    widget->affectsLayoutWhenHidden=true;
    if (widget->visible) {
        // ImageWidget::OnVisibilityChanged(false), RVA4FC34C, unloads the image
        // and invalidates its layout even with affectsLayoutWhenHidden=true.
        // Mask only the final draw and visually invalidate the ancestors instead.
        // The separate window/texture stays live and its layout keeps its size.
        widget->visible=false;
        reinterpret_cast<InvalidateFn>(base+0x2805fc)(widget);
    }
}

// The game updates slot visibility after CET onUpdate too. Intercept its request,
// retain the intended visibility, and keep only our final slot image from drawing.
// Source windows and their controllers are not hidden. Immutable target snapshots
// keep the handles alive while an engine UI job is inside the detour.
uintptr_t Visible(ink::Widget* widget, bool visible) {
    const auto targets=publishedMasks.load(std::memory_order_acquire);
    if (targets) for (const auto& mask:*targets) if (mask->widget.GetPtr()==widget) {
        mask->wantedVisible.store(visible,std::memory_order_relaxed);
        const auto channel=mask->channel.load(std::memory_order_acquire);
        if (g_liveControls.xrHudPanel && !g_menuModeValue && !g_liveControls.xrMenuRect &&
            (channel!=cvr::hud::Channel::Interaction || g_liveControls.xrInteractionPanel) && cvr::hud::ConsumerReady(channel)) {
            HideSlot(widget);return 0;
        }
        break;
    }
    return originalVisible(widget,visible);
}

uintptr_t CopyEntries(uintptr_t resource, DynArray<ink::HudWidgetSpawnEntry>* entries) {
    const auto result=originalCopyEntries(resource,entries);
    // These HUD entries are authored without separate windows. Move them
    // through the engine's own separate-window creation path BEFORE spawn.
    // Never convert projected widgets (crosshair, mappins, scanner borders, etc.).
    for (auto& e:*entries) if (cvr::hud::NeedsSeparateWindow(e.hudEntryName.hash)) {
        e.useSeparateWindow=true;
        Log("[hud-panel] separate window enabled for %s before spawn\n",e.hudEntryName.ToString());
    }
    return result;
}

bool InstallHudPanelHooks() {
    const uint8_t visibility[]={0x48,0x89,0x5c,0x24,0x08,0x57,0x48,0x83,0xec,0x20,0x40,0x8a,0xfa,0x48,0x8b,0xd9};
    const uint8_t entries[]={0x48,0x89,0x5c,0x24,0x08,0x48,0x89,0x6c,0x24,0x10,0x48,0x89,0x74,0x24,0x18,0x57,0x48,0x83,0xec,0x20};
    const uint8_t invalidate[]={0x48,0x89,0x5c,0x24,0x18,0x55,0x48,0x8b,0xec,0x48,0x83,0xec,0x60,0x80,0x89,0xfd,0x01,0x00};
    const uint8_t invalidateLayout[]={0x48,0x89,0x5c,0x24,0x08,0x48,0x89,0x7c,0x24,0x10,0x55,0x48,0x8d,0x6c,0x24,0xa9,0x48,0x81,0xec,0xa0,0,0,0};
    if (!Verified() || memcmp(reinterpret_cast<void*>(base+0x3c9e0c),visibility,sizeof(visibility)) ||
        memcmp(reinterpret_cast<void*>(base+0x482a90),entries,sizeof(entries)) ||
        memcmp(reinterpret_cast<void*>(base+0x2805fc),invalidate,sizeof(invalidate)) ||
        memcmp(reinterpret_cast<void*>(base+0x2804e4),invalidateLayout,sizeof(invalidateLayout))) return false;
    auto vis=reinterpret_cast<void*>(base+0x3c9e0c), copy=reinterpret_cast<void*>(base+0x482a90);
    if (MH_CreateHook(vis,reinterpret_cast<void*>(&Visible),reinterpret_cast<void**>(&originalVisible))!=MH_OK) return false;
    if (MH_CreateHook(copy,reinterpret_cast<void*>(&CopyEntries),reinterpret_cast<void**>(&originalCopyEntries))!=MH_OK) {
        MH_RemoveHook(vis);return false;
    }
    if (MH_EnableHook(vis)!=MH_OK || MH_EnableHook(copy)!=MH_OK) {
        MH_DisableHook(vis);MH_DisableHook(copy);return false;
    }
    // The frame submit is optional: without it the repaints run in the update as before.
    const uint8_t submit[]={0x48,0x89,0x5c,0x24,0x10,0x55,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57,0x48,0x8b,0xec};
    auto frameSubmit=reinterpret_cast<void*>(base+0x3d6d84);
    if (!memcmp(frameSubmit,submit,sizeof(submit)) &&
        MH_CreateHook(frameSubmit,reinterpret_cast<void*>(+[](void* scene,void* views,char flag){
            FlushRepaintsAtSubmit();originalFrameSubmit(scene,views,flag);}),reinterpret_cast<void**>(&originalFrameSubmit))==MH_OK &&
        MH_EnableHook(frameSubmit)!=MH_OK) originalFrameSubmit=nullptr;
    hooksReady.store(true,std::memory_order_release);return true;
}
CVR_HOOK("HudPanel",::cvr::hooks::Stage::Boot,82,InstallHudPanelHooks);

void PublishMasks() {
    auto next=std::make_shared<MaskSet>();next->reserve(masks.size());
    for (auto& [p,m]:masks) next->push_back(m);
    publishedMasks.store(std::move(next),std::memory_order_release);
}
void RestoreMask(Mask& mask) {
    // Calling the original bypasses our interception, so it cannot mistake our
    // own hide/restore for a new game-side visibility request.
    auto* widget=mask.widget.GetPtr();
    const bool wanted=mask.wantedVisible.load(std::memory_order_relaxed);
    const bool layoutChanged=widget->affectsLayoutWhenHidden!=mask.originalAffectsLayout;
    widget->affectsLayoutWhenHidden=mask.originalAffectsLayout;
    // Our hidden bit is only a draw mask. Deliver the real visibility transition
    // now, including ImageWidget's resource/layout callback when the game hid it.
    widget->visible=!wanted;
    originalVisible(widget,wanted);
    if(layoutChanged) reinterpret_cast<InvalidateFn>(base+0x2804e4)(widget);
}
void Restore() {
    publishedMasks.store({},std::memory_order_release);
    for (auto& [p,m]:masks) RestoreMask(*m);
    masks.clear();
}

// Whether anything under a separate window's root can reach its texture: a path of visible,
// non-transparent widgets ending in something that is not a container (image, text, shape --
// counted as content without looking further). Containers draw nothing themselves, so one whose
// visible children all lead nowhere is empty. An empty window renders a fully transparent texture
// (the interaction window measured 0 non-zero pixels of 6.5 million), so skipping it is exact.
bool HasVisibleContent(ink::Widget* widget, unsigned depth = 0) {
    if (!widget) return false;
    if (depth > 64) return true;
    if (!widget->visible || !(widget->opacity > 0) || !(widget->tintColor.Alpha > 0)) return false;
    static CClass* const compound = CRTTISystem::Get()->GetClass("inkCompoundWidget");
    if (!compound || !widget->GetType() || !widget->GetType()->IsA(compound)) return true;
    auto* children = static_cast<ink::CompoundWidget*>(widget)->children.GetPtr();
    if (!children) return false;
    // inkMultiChildren::children (DynArray<Handle<inkWidget>>) at 0x38, per the SDK's layout.
    const auto& list = *reinterpret_cast<const DynArray<Handle<ink::Widget>>*>(reinterpret_cast<uintptr_t>(children) + 0x38);
    for (const auto& child : list)
        if (HasVisibleContent(child.GetPtr(), depth + 1)) return true;
    return false;
}

// ---- HUD root upkeep (was the per-frame half of modules/hud_panel.lua updatePhoneBounds) ----
// The script still finds the roots once a second -- which controllers, which windows, the inkHudEntryInfo it
// creates when a root has none -- and hands them over through VRHudPanelRootsBegin / VRHudPanelRoot /
// VRHudPanelSubtitle. What it did every frame for each of them happens here, in the same order relative
// to the slot walk below: about 40 script calls a frame through CET for six roots, 0.15 ms, measured.
enum : uint32_t { RootScanner = 1, RootWindowOrigin = 2, RootInteraction = 4, RootGazeDot = 8 };
struct UpkeepRoot {
    Handle<ink::Widget> root, window;
    Handle<ink::HudEntryInfo> info;
    float padding = 0;
    uint32_t flags = 0;
    int8_t wasActive = -1;   // unknown after every scan, as the script's fresh tables were
};
struct UpkeepSubtitle {
    Handle<ink::Widget> root, window, panel;
    Handle<ink::HudEntryInfo> info;
    float x = 0, y = 0, width = 0, height = 0;   // the authored crop the root had when first seen
    int8_t wasActive = -1;
};
std::vector<UpkeepRoot> upkeepRoots, pendingRoots;
std::vector<UpkeepSubtitle> upkeepSubtitles, pendingSubtitles;
bool pendingScan = false;

void Invalidate(ink::Widget* widget) { reinterpret_cast<InvalidateFn>(base+0x2805fc)(widget); }   // FlagForVisualInvalidation

// REPAINTS AT THE FRAME SUBMIT. The upkeep runs in CET's onUpdate, at the start of the game thread's frame,
// while the previous frame's render may still be in its UI composition node (sub_14033D0AC), and it repaints
// the HUD roots and their separate windows every frame. With the render running further beside the game
// thread (every async compute pass on the graphics queue), pausing crashed in that node: a composition
// element's texture object had been freed and its memory reused (2026-10-17, docs/gpu-not-full-20261016.md).
// With the upkeep off the same pause stress passed. So the repaints are queued here and run on the game
// thread at the frame submit (sub_1403D6D84), where the previous frame's render has finished and the next
// one has not started; they reach the HUD a frame later, which a repaint every frame does not show. Without
// a frame submit since the last update (menus, loading), the update runs them itself.
std::vector<Handle<ink::Widget>> repaints;
DWORD repaintThread = 0;
void FlushRepaints(size_t stat) {
    for (auto& widget : repaints) if (widget) Invalidate(widget.GetPtr());
    if (!repaints.empty()) CyberpunkVR_HudRepaintStats[stat].fetch_add(repaints.size(), std::memory_order_relaxed);
    repaints.clear();
}
void Repaint(const Handle<ink::Widget>& widget) {
    if (!widget) return;
    if (!CyberpunkVR_HudRepaintAtSubmit.load(std::memory_order_relaxed) || !originalFrameSubmit) { Invalidate(widget.GetPtr()); return; }
    repaints.push_back(widget);
    CyberpunkVR_HudRepaintStats[0].fetch_add(1, std::memory_order_relaxed);
}
void FlushRepaintsAtSubmit() {
    if (!repaintThread || GetCurrentThreadId() != repaintThread) return;
    FlushRepaints(1);
}
bool Active(const ink::Widget* widget) { return widget->visible && widget->opacity > 0; }
bool Within(float v) { return std::abs(v) < 16384.0f; }
// Root pixels per design unit: the HUD layer fits its 3840x2160 design into the root (0.6896 at 2648x2864,
// bound by the width; the scanner's 3084-unit crop made a 2126-pixel texture). 0 until the root is seen.
float HudDesignScale() {
    const float width = CyberpunkVR_HudRootWidth, height = CyberpunkVR_HudRootHeight;
    if (!(width > 0) || !(height > 0)) return 0;
    return std::min(width / 3840.0f, height / 2160.0f);
}
Vector2 ChildPosition(ink::Widget* parent, const Handle<ink::Widget>& child) {
    Vector2 out{}; reinterpret_cast<ChildRectFn>(base+0x15b19e4)(parent, &out, &child); return out;
}
Vector2 ChildSize(ink::Widget* parent, const Handle<ink::Widget>& child) {
    Vector2 out{}; reinterpret_cast<ChildRectFn>(base+0x15b1990)(parent, &out, &child); return out;
}
// The direct child with exactly this name: what the script's GetWidgetByPathName plus its name check gave.
Handle<ink::Widget> ChildNamed(ink::Widget* widget, CName name) {
    static CClass* const compound = CRTTISystem::Get()->GetClass("inkCompoundWidget");
    if (!widget || !compound || !widget->GetType() || !widget->GetType()->IsA(compound)) return {};
    auto* children = static_cast<ink::CompoundWidget*>(widget)->children.GetPtr();
    if (!children) return {};
    const auto& list = *reinterpret_cast<const DynArray<Handle<ink::Widget>>*>(reinterpret_cast<uintptr_t>(children) + 0x38);
    for (const auto& child : list) if (child && child->name == name) return child;
    return {};
}
// The widget's own SetSize, through RTTI: it relayouts, which a field write would not.
bool SetWidgetSize(ink::Widget* widget, Vector2 size) {
    auto* fn = widget->GetType() ? widget->GetType()->GetFunction("SetSize") : nullptr;
    if (!fn) return false;
    float x = size.X, y = size.Y;
    StackArgs_t args;
    if (fn->params.Size() == 2) { args.emplace_back(nullptr, &x); args.emplace_back(nullptr, &y); }
    else if (fn->params.Size() == 1 && fn->params[0]->type->GetName() == CName("Vector2")) args.emplace_back(nullptr, &size);
    else return false;
    return ExecuteFunction(widget, fn, nullptr, args);
}

void CommitScan() {
    if (!pendingScan) return;
    pendingScan = false;
    // A subtitle root seen in the previous scan keeps its first authored crop, as the script's did.
    for (auto& next : pendingSubtitles)
        for (const auto& old : upkeepSubtitles)
            if (old.root.GetPtr() == next.root.GetPtr()) { next.x = old.x; next.y = old.y; next.width = old.width; next.height = old.height; break; }
    upkeepRoots.swap(pendingRoots); pendingRoots.clear();
    upkeepSubtitles.swap(pendingSubtitles); pendingSubtitles.clear();
}

// Returns whether the interaction hub's loot plate is up.
bool UpkeepRoots(ink::HUDLayer* hud) {
    static const CName topName("topWidgets"), lootName("looting"), borderName("border"), overlayName("scanner_overlay");
    static const CName scannerName("scanner"),detailsName("scanner_details");
    struct Allocation {ink::Widget* root{};Vector2 size{};};
    std::array<Allocation,2> scannerTextures{};
    if(hud) {
        std::shared_lock lock(hud->spawningLock);
        for(const auto& entry:hud->entries) {
            const int index=entry.hudEntryName==scannerName?0:entry.hudEntryName==detailsName?1:-1;
            if(index>=0 && entry.rootWidget && entry.slotTexture)
                scannerTextures[index]={entry.rootWidget.GetPtr(),{float(entry.slotTexture->width),float(entry.slotTexture->height)}};
        }
    }
    CyberpunkVR_HudRootsStats[0].fetch_add(1, std::memory_order_relaxed);
    bool lootVisible = false;
    for (auto& r : upkeepRoots) {
        auto* root = r.root.GetPtr(); auto* window = r.window.GetPtr(); auto* info = r.info.GetPtr();
        if (!root || !window) continue;
        if(r.flags & RootGazeDot) {
            // The persistent dot and the braindance HUD: full-screen roots laid out about the screen's middle, which
            // under the asymmetric projection is not where the head looks. Moved onto the gaze line like the
            // scanner's crop; the braindance one sat a gaze offset above the scanner (2026-11-01).
            const float y=g_menuModeValue==0 ? cvr::gaze::CanvasOffset(cvr::gaze::Read(),CyberpunkVR_HudRootHeight,HudDesignScale()) : 0.f;
            if(std::abs(root->renderTransform.translation.Y-y)>.01f) {
                auto* fn=root->GetType()->GetFunction("SetTranslation");
                if(fn) {
                    Vector2 position{0,y};float x=0,dy=y;StackArgs_t args;
                    if(fn->params.Size()==1 && fn->params[0]->type->GetName()==CName("Vector2"))args.emplace_back(nullptr,&position);
                    else if(fn->params.Size()==2){args.emplace_back(nullptr,&x);args.emplace_back(nullptr,&dy);}
                    else continue;
                    ExecuteFunction(root,fn,nullptr,args);
                }
            }
            continue;
        }
        if(!info)continue;
        CyberpunkVR_HudRootsStats[1].fetch_add(1, std::memory_order_relaxed);
        // Hidden panels need no crop work or redraw; the last visible image is cleared once on hide.
        const bool active = Active(root);
        if (!active && r.wasActive != 0) { Repaint(r.root); Repaint(r.window); }
        r.wasActive = active ? 1 : 0;
        if (!active) continue;
        CyberpunkVR_HudRootsStats[2].fetch_add(1, std::memory_order_relaxed);
        if (r.flags & RootInteraction) {
            // The loot plate and tooltip stay in their native window; only the XR follow policy changes.
            const auto top = ChildNamed(root, topName);
            const auto loot = top ? ChildNamed(top.GetPtr(), lootName) : Handle<ink::Widget>{};
            if (loot && Active(loot.GetPtr()) && Active(top.GetPtr())) lootVisible = true;
        }
        const Vector2 size = root->size;
        if (!(size.X > 0 && size.Y > 0 && size.X <= 16384 && size.Y <= 16384)) continue;
        const Vector2 bounds = info->size;
        const float padding = r.padding;
        float x = -padding, y = -padding, width = size.X + padding * 2, height = size.Y + padding * 2;
        if (r.flags & RootWindowOrigin) {
            // Centered roots with anchorPoint (0,0): the crop starts at the root's offset within its window.
            const Vector2 origin = ChildPosition(window, r.root);
            if (Within(origin.X) && Within(origin.Y)) { x = origin.X - padding; y = origin.Y - padding; }
            else { x = info->offset.X; y = info->offset.Y; }
        }
        if (r.flags & RootScanner) {
            // The scanner's 1920x1080 root holds a 3840x2160 overlay at (-960,-540). The crop is CENTRED ON
            // THE OVERLAY'S MIDDLE, the reticle: the engine centres the slot on the HUD and draws the window
            // texture into it 1:1, so the crop's middle is what lands on the gaze (the Surveillance panel's
            // centre). The engine's own rules for that texture (sub_14028AA24): the crop times the HUD
            // scale, clamped to the HUD root, and re-created only when that GROWS, the slot taking the
            // texture's size. A crop wider than the root (3840 + 2 x 32 margin) was cut on the right and
            // its middle sat 22 units right of the slot's; a crop smaller than the texture is drawn into
            // its top-left corner, which raised the whole scanner after a larger one (2026-10-18). So the
            // requested crop is clamped to the root and cannot shrink below its previous size.
            // The retained texture may still be larger: its actual extent is handled below.
            const float scale = HudDesignScale();
            const auto border = ChildNamed(root, borderName);
            const auto panel = border ? ChildNamed(border.GetPtr(), overlayName) : Handle<ink::Widget>{};
            bool placed = false;
            if (panel) {
                const Vector2 a = ChildPosition(root, border), b = ChildPosition(border.GetPtr(), panel);
                const Vector2 s = ChildSize(border.GetPtr(), panel), origin = ChildPosition(window, r.root);
                const float px = origin.X + a.X + b.X, py = origin.Y + a.Y + b.Y;
                if (s.X > 0 && s.Y > 0 && s.X < 16384 && s.Y < 16384 && Within(px) && Within(py)) {
                    const float cx = std::floor(px + s.X * 0.5f), cy = std::floor(py + s.Y * 0.5f);
                    float halfW = std::max({s.X * 0.5f + 32, cx - origin.X, origin.X + size.X - cx});
                    float halfH = std::max({s.Y * 0.5f + 32, cy - origin.Y, origin.Y + size.Y - cy});
                    if (bounds.X > 0 && bounds.Y > 0 && bounds.X < 16384 && bounds.Y < 16384) {
                        halfW = std::max(halfW, bounds.X * 0.5f); halfH = std::max(halfH, bounds.Y * 0.5f);
                    }
                    if (scale > 0) {
                        halfW = std::min(halfW, std::round(CyberpunkVR_HudRootWidth / scale) * 0.5f);
                        halfH = std::min(halfH, std::round(CyberpunkVR_HudRootHeight / scale) * 0.5f);
                    }
                    halfW = std::ceil(halfW); halfH = std::ceil(halfH);
                    x = cx - halfW; y = cy - halfH; width = 2 * halfW; height = 2 * halfH;
                    placed = true;
                }
            }
            if (!placed && bounds.X > 0 && bounds.Y > 0) { x = info->offset.X; y = info->offset.Y; width = bounds.X; height = bounds.Y; }
        }
        for(const auto& texture:scannerTextures)if(texture.root==root) {
            const auto crop=cvr::hud::KeepCropCentered({x,y,width,height},texture.size.X,texture.size.Y,HudDesignScale());
            x=crop.x;y=crop.y;width=crop.width;height=crop.height;
            break;
        }
        if (bounds.X != width || bounds.Y != height) {
            info->size = {width, height}; Repaint(r.root);
            CyberpunkVR_HudRootsStats[3].fetch_add(1, std::memory_order_relaxed);
        }
        if ((padding > 0 || (r.flags & (RootScanner | RootWindowOrigin))) && (info->offset.X != x || info->offset.Y != y)) {
            info->offset = {x, y}; Repaint(r.root);
            CyberpunkVR_HudRootsStats[4].fetch_add(1, std::memory_order_relaxed);
        }
        if (window->size.X != size.X || window->size.Y != size.Y) {
            CyberpunkVR_HudRootsStats[5].fetch_add(1, std::memory_order_relaxed);
            if (!SetWidgetSize(window, size)) CyberpunkVR_HudRootsStats[6].fetch_add(1, std::memory_order_relaxed);
        }
        // Fullscreen controllers cache their draw data; without a repaint every frame quest-icon animation
        // periodically leaves an empty offscreen image though the window stays visible.
        Repaint(r.root); Repaint(r.window);
    }
    for (auto& sub : upkeepSubtitles) {
        auto* root = sub.root.GetPtr(); auto* window = sub.window.GetPtr(); auto* info = sub.info.GetPtr();
        if (!root || !window || !info || !sub.panel) continue;
        const bool active = Active(root);
        if (!active && sub.wasActive != 0) { Repaint(sub.root); Repaint(sub.window); }
        sub.wasActive = active ? 1 : 0;
        if (!active) continue;
        // The game puts mainPanel above the 100px root (Y=-255); the authored crop starts at Y=0 and would
        // clip the whole line once the subtitles are a window of their own.
        const Vector2 p = ChildPosition(root, sub.panel), s = ChildSize(root, sub.panel);
        if (!(s.X > 0 && s.Y > 0 && s.X < 16384 && s.Y < 16384 && Within(p.X) && Within(p.Y))) continue;
        const float x = std::min(sub.x, std::floor(p.X - 32)), y = std::min(sub.y, std::floor(p.Y - 32));
        const float width = std::max(sub.width, std::ceil(p.X + s.X + 32 - x));
        const float height = std::max(sub.height, std::ceil(p.Y + s.Y + 32 - y));
        if (info->offset.X != x || info->offset.Y != y || info->size.X != width || info->size.Y != height) {
            info->offset = {x, y}; info->size = {width, height};
            CyberpunkVR_HudRootsStats[7].fetch_add(1, std::memory_order_relaxed);
        }
        Repaint(sub.root); Repaint(sub.window);
    }
    return lootVisible;
}

bool Geometry(Handle<ink::Widget> child, ink::Widget* root, cvr::hud::Sprite& s, bool& dirty) {
    Vector2 position{}, size{};
    bool first = true;
    for (unsigned depth = 0; child && depth < 24; ++depth) {
        dirty |= (child->flags & 0x06)!=0;
        auto mask = masks.find(child.GetPtr());
        const bool visible = mask==masks.end() ? child->visible : mask->second->wantedVisible.load(std::memory_order_relaxed);
        if (!visible || child->opacity <= 0) return false;
        const auto& t = child->tintColor;
        const float alpha = std::clamp(child->opacity * t.Alpha, 0.0f, 1.0f);
        s.tint[0] *= t.Red * alpha; s.tint[1] *= t.Green * alpha;
        s.tint[2] *= t.Blue * alpha; s.tint[3] *= alpha;
        if (child.GetPtr() == root)
            return !first && std::isfinite(s.x+s.y+s.width+s.height) && s.width > 0 && s.height > 0;
        auto parent = child->parentWidget.Lock();
        if (!parent) return false;
        reinterpret_cast<ChildRectFn>(base+0x15b19e4)(parent.GetPtr(), &position, &child);
        if (first) {
            reinterpret_cast<ChildRectFn>(base+0x15b1990)(parent.GetPtr(), &size, &child);
            s.width = size.X; s.height = size.Y; first = false;
        }
        s.x += position.X; s.y += position.Y;
        child = std::move(parent);
    }
    return false;
}

// THE ENGINE'S STEREO LAYS THE HUD OUT TWO EYES WIDE (Stereo/EngineStereo.hpp HudDesignRatio): a 5296 x 2864
// root on the Quest 3 profile, its design fitted by the height, every window 1.92x the size it has on one eye's
// 2648 x 2864 root and the corner ones out at the far ends -- the minimap and the health bar each landed in one
// eye only. The panel is built on the one-eye root instead. The HUD's windows hang from its 3840 x 2160 design
// area, which the layer centres in either root (one eye's: 1490 tall, 687 from the top), so the move is one
// scale about the roots' centres. Keeping each window's distance from the root's own edges instead put the
// health bar, the quest text and the phone 690 pixels too high.
cvr::hud::Rect ToEyeRoot(cvr::hud::Rect r, float rootWidth, float rootHeight, float eyeWidth, float ratio) {
    return {eyeWidth * 0.5f + (r.x - rootWidth * 0.5f) * ratio, rootHeight * 0.5f + (r.y - rootHeight * 0.5f) * ratio,
            r.width * ratio, r.height * ratio};
}

std::shared_ptr<cvr::hud::TextureLease> Texture(DynamicTexture* texture, bool idle = false) {
    if (!texture) return {};
    const uintptr_t backing = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uintptr_t>(texture)+0x70);
    if (!backing || *reinterpret_cast<uintptr_t*>(backing) != base+0x30646a8) return {};
    const uint32_t id = *reinterpret_cast<uint32_t*>(backing+0x18);
    if (!id || id > 131072) return {};
    const uintptr_t pool = *reinterpret_cast<uintptr_t*>(base+0x3bcdca8);
    if (!pool) return {};
    const uintptr_t entry = pool + 0xb0ull*(id-1);
    auto ref = reinterpret_cast<volatile LONG*>(entry+0x2f1d0);
    LONG count = *ref;
    while (count > 0) {
        LONG observed = InterlockedCompareExchange(ref, count+1, count);
        if (observed == count) break;
        count = observed;
    }
    if (count <= 0) return {};
    auto result = std::make_shared<cvr::hud::TextureLease>();
    result->handle = id; // Engine handle lease also keeps the placed-resource heap alive.
    auto resource = *reinterpret_cast<ID3D12Resource**>(entry+0x2f1d8);
    if (!resource) return {};
    auto desc = resource->GetDesc();
    if (desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.SampleDesc.Count != 1 ||
        (desc.Format != DXGI_FORMAT_R8G8B8A8_TYPELESS && desc.Format != DXGI_FORMAT_R8G8B8A8_UNORM &&
         desc.Format != DXGI_FORMAT_R8G8B8A8_UNORM_SRGB &&
         desc.Format != DXGI_FORMAT_R16G16B16A16_FLOAT &&
         desc.Format != DXGI_FORMAT_R16G16B16A16_TYPELESS) ||
        desc.Width != texture->width || desc.Height != texture->height) return {};
    // This is the engine's read state (IDA sub_1401F9BBC / sub_140337264).
    // Do not guess barriers for a texture with a different native state.
    const uint32_t state = *reinterpret_cast<uint32_t*>(entry+0x2f220);
    if (state != 0xc0 && !(idle && state == 0)) return {};
    result->resource = resource;
    return result;
}
}

std::shared_ptr<cvr::hud::TextureLease> cvr::hud::Lease(RED4ext::DynamicTexture* texture, bool idle) {
    return Texture(texture, idle);
}

cvr::hud::TextureLease::~TextureLease() {
    if (handle) reinterpret_cast<void (*)(uint32_t*)>(base+0x3e3248)(&handle);
}

void VRPhysicalOpticsProbe(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,bool* out,int64_t) {
    RED4ext::Handle<RED4ext::ent::IComponent> component;
    RED4ext::GetParameter(frame,&component);++frame->code;if(out)*out=false;
    if(!Verified() || !component)return;
    // Diagnostics may inspect the original scope UI as well as our dedicated
    // panels, so a captured artifact can be attributed to the correct layer.
    const auto widgetType=RED4ext::CRTTISystem::Get()->GetClass("WorldWidgetComponent");
    if(widgetType && component->GetType()->IsA(widgetType)) {
        const auto texture=*reinterpret_cast<RED4ext::DynamicTexture**>(reinterpret_cast<uintptr_t>(component.GetPtr())+0x230);
        if(!texture || *reinterpret_cast<const uintptr_t*>(texture)!=base+0x3064700)return;
        RED4ext::Handle<RED4ext::DynamicTexture> source(texture);
        if(out)*out=cvr::optics::probe::Request(Texture(source.GetPtr()));
        return;
    }
    if(component->name!=RED4ext::CName("vr_optics_camera"))return;
    const auto type=RED4ext::CRTTISystem::Get()->GetClass("entRenderToTextureCameraComponent");
    if(!type || !component->GetType()->IsA(type))return;
    const auto camera=static_cast<RED4ext::ent::RenderToTextureCameraComponent*>(component.GetPtr());
    const auto& source=*reinterpret_cast<const RED4ext::Handle<RED4ext::DynamicTexture>*>(
        reinterpret_cast<uintptr_t>(camera)+0x1e8);
    if(source && out)*out=cvr::optics::probe::Request(Texture(source.GetPtr()));
}

namespace {
int64_t HudNow() { LARGE_INTEGER t; QueryPerformanceCounter(&t); return t.QuadPart; }
void HudAdd(size_t slot, int64_t ticks) { CyberpunkVR_HudPanelTiming[slot].fetch_add(static_cast<uint64_t>(ticks), std::memory_order_relaxed); }
struct HudCallTimer {
    int64_t t0 = HudNow();
    ~HudCallTimer() { HudAdd(0, 1); HudAdd(1, HudNow() - t0); }
};
}

void VRHudPanelUpdate(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, int32_t* out, int64_t) {
    using namespace RED4ext;
    using cvr::hud::Channel;
    const HudCallTimer callTimer;
    Handle<ink::Widget> root;
    bool lootVisible=false;
    GetParameter(frame, &root); GetParameter(frame, &lootVisible); ++frame->code;
    if (out) *out = 0;
    if (!Verified() || !hooksReady.load(std::memory_order_acquire)) return;
    repaintThread = GetCurrentThreadId();
    FlushRepaints(2);   // still queued: no frame submit since the last update
    // Before anything below can return: the script's upkeep ran whatever the HUD panel's state.
    CommitScan();
    // Dedicated camera/turret weapons retain their own optical aiming axis.
    cvr::gaze::headLook.store(g_menuModeValue==0 && !DeviceCamActive() && !cvr::anim::IsHeadAimWeaponActive(),std::memory_order_relaxed);
    auto proxy = root ? root->layerProxy : Handle<ink::LayerProxy>{};
    auto layer = proxy ? proxy->layer.Lock() : Handle<ink::Layer>{};
    const bool hudLayer = layer && layer->GetType()->GetName() == CName("inkHUDLayer");
    if (hudLayer && root->size.X > 0 && root->size.Y > 0 && root->size.X <= 16384 && root->size.Y <= 16384) {
        CyberpunkVR_HudRootWidth = root->size.X; CyberpunkVR_HudRootHeight = root->size.Y;
    }
    if (hudLayer) {
        CyberpunkVR_HudLayerAddress[0] = reinterpret_cast<uintptr_t>(layer.GetPtr());
        CyberpunkVR_HudLayerAddress[1] = reinterpret_cast<uintptr_t>(layer->window.GetPtr());
    }
    if (CyberpunkVR_HudRootsNative.load(std::memory_order_relaxed) &&
        UpkeepRoots(hudLayer ? static_cast<ink::HUDLayer*>(layer.GetPtr()) : nullptr)) lootVisible = true;
    std::array<std::shared_ptr<cvr::hud::Snapshot>,cvr::hud::ChannelCount> snapshots;
    for(auto& snapshot:snapshots){snapshot=std::make_shared<cvr::hud::Snapshot>();snapshot->stamp=GetTickCount64();}
    snapshots[cvr::hud::Index(Channel::Interaction)]->lootVisible=lootVisible;
    auto publish=[&]{for(size_t i=0;i<snapshots.size();++i)cvr::hud::Publish(snapshots[i],static_cast<Channel>(i));};
    if (!g_liveControls.xrHudPanel || g_menuModeValue != 0 || g_liveControls.xrMenuRect || !hudLayer) {
        Restore(); lastRoot = nullptr; lastMasked = {}; ++generation;publish();return;
    }
    if (root.GetPtr() != lastRoot) { Restore(); layoutCache.clear();lastRoot = root.GetPtr(); ++generation; }
    auto hud = static_cast<ink::HUDLayer*>(layer.GetPtr());
    const uint32_t rootWidth=static_cast<uint32_t>(root->size.X),height=static_cast<uint32_t>(root->size.Y);
    if(!rootWidth || !height || rootWidth>8192 || height>8192){Restore();publish();return;}
    // The canvas: the root, or one eye's root under the engine's stereo (ToEyeRoot).
    const float designRatio=cvr::engine_stereo::HudDesignRatio(float(rootWidth),float(height));
    const uint32_t width=designRatio!=1.0f && g_launcherHeight>0
        ? static_cast<uint32_t>(std::lround(double(height)*g_launcherWidth/g_launcherHeight)) : rootWidth;
    for(auto& snapshot:snapshots){snapshot->width=width;snapshot->height=height;}
    if(layoutWidth!=rootWidth || layoutHeight!=height){layoutCache.clear();layoutWidth=rootWidth;layoutHeight=height;}
    std::array<std::vector<Handle<ink::Widget>>,cvr::hud::ChannelCount> slots;
    std::array<bool,cvr::hud::ChannelCount> complete;complete.fill(true);
    const auto settings=cvr::hud::GetLayoutSettings();
    const bool sniperNest=cvr::camera::SniperHeadFollowActive();
    cvr::hud::LayoutStatus status{};
    {
        std::shared_lock lock(hud->spawningLock);
        for(const auto& e:hud->entries){
            if(!e.useSeparateWindow || !e.slotWidget || !e.slotTexture || !e.window)continue;
            const auto channel=cvr::hud::EntryChannel(e.hudEntryName.hash,sniperNest);
            if(channel==Channel::Interaction && !g_liveControls.xrInteractionPanel)continue;
            const auto group=cvr::hud::Index(channel);auto& snapshot=*snapshots[group];
            Handle<ink::Widget> slot(e.slotWidget);
            cvr::hud::Sprite sprite;bool dirty=false;
            HudAdd(5,1);
            const int64_t tGeometry=HudNow();
            const bool placed=Geometry(slot,root.GetPtr(),sprite,dirty);
            const int64_t tTexture=HudNow();HudAdd(2,tTexture-tGeometry);
            if(!placed)continue;
            sprite.texture=Texture(e.slotTexture.GetPtr());
            HudAdd(3,HudNow()-tTexture);
            if(!sprite.texture || snapshot.sprites.size()>=cvr::hud::MaxSprites){complete[group]=false;continue;}
            sprite.name=e.hudEntryName.hash;
            auto rect=layoutCache[sprite.name].Update({sprite.x,sprite.y,sprite.width,sprite.height},dirty);
            if(width!=rootWidth)rect=ToEyeRoot(rect,float(rootWidth),float(height),float(width),designRatio);
            const int index=cvr::hud::ElementIndex(sprite.name);
            if(index>=0){
                status[index]={true,rect.x,rect.y,rect.width,rect.height};
                rect=cvr::hud::ApplyElement(rect,settings[index],float(width),float(height));
                const float opacity=settings[index].visible?settings[index].opacity:0;
                for(float& component:sprite.tint)component*=opacity;
            }
            sprite.x=rect.x;sprite.y=rect.y;sprite.width=rect.width;sprite.height=rect.height;
            // A window with nothing visible contributes only zeros: keep its slot (masked), skip its draw.
            const int64_t tContent=HudNow();
            const bool content=CyberpunkVR_HudSkipEmpty.load(std::memory_order_relaxed)==0 || HasVisibleContent(e.rootWidget.GetPtr());
            HudAdd(4,HudNow()-tContent);
            if(content){snapshot.sprites.push_back(std::move(sprite));HudAdd(7,1);}
            else CyberpunkVR_HudEmptySprites.fetch_add(1,std::memory_order_relaxed);
            slots[group].push_back(std::move(slot));
        }
    }
    const int64_t tTail=HudNow();
    struct TailTimer { int64_t t0; ~TailTimer() { HudAdd(6, HudNow() - t0); } } tailTimer{tTail};
    cvr::hud::SetLayoutStatus(status);
    // Each XR consumer acknowledges only its own group. A missing dialogue
    // texture must not hide ordinary HUD, or mask a native prompt without a copy.
    std::unordered_map<ink::Widget*,Channel> desired;
    int total=0;bool allMasked=true;
    for(size_t i=0;i<snapshots.size();++i){
        auto& snapshot=*snapshots[i];const auto channel=static_cast<Channel>(i);
        snapshot.masked=complete[i] && !slots[i].empty() && cvr::hud::ConsumerReady(channel);
        if(snapshot.masked)for(auto& slot:slots[i])desired.emplace(slot.GetPtr(),channel);
        if(snapshot.masked!=lastMasked[i]){++generation;lastMasked[i]=snapshot.masked;}
        snapshot.generation=generation;
        if(!complete[i])snapshot.sprites.clear();
        snapshot.emptyContent=complete[i] && snapshot.sprites.empty() && !slots[i].empty();
        total+=int(snapshot.sprites.size());if(!snapshot.sprites.empty() && !snapshot.masked)allMasked=false;
    }
    std::vector<std::shared_ptr<Mask>> retired;bool changed=false;
    for(auto it=masks.begin();it!=masks.end();){
        if(!desired.contains(it->first)){retired.push_back(it->second);it=masks.erase(it);changed=true;}
        else {it->second->channel.store(desired.at(it->first),std::memory_order_release);++it;}
    }
    for(size_t i=0;i<slots.size();++i)if(snapshots[i]->masked)for(auto& slot:slots[i]){
        if(!masks.contains(slot.GetPtr())){masks.emplace(slot.GetPtr(),std::make_shared<Mask>(slot,static_cast<Channel>(i)));changed=true;}
    }
    if(changed)PublishMasks();
    for(auto& old:retired)RestoreMask(*old);
    for(auto& [widget,channel]:desired)HideSlot(widget);
    if(out)*out=total*(allMasked?1:-1);
    if(cvr::RuntimeDiagnosticsEnabled()){
        static int previous=-1000;const int current=total*(allMasked?1:-1);
        if(current!=previous){previous=current;Log("[hud-panel] main=%u interactions=%u masked=%d/%d canvas=%ux%u\n",
            unsigned(snapshots[0]->sprites.size()),unsigned(snapshots[1]->sprites.size()),snapshots[0]->masked,snapshots[1]->masked,width,height);}
    }
    publish();
}

// Whether VRHudPanelUpdate does the per-frame root upkeep. False when its hooks could not be verified,
// so the script keeps doing it.
void VRHudPanelRootsNative(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    ++frame->code;
    if (out) *out = Verified() && hooksReady.load(std::memory_order_acquire) && CyberpunkVR_HudRootsNative.load(std::memory_order_relaxed);
}

// A new scan: the roots that follow replace the current ones at the next VRHudPanelUpdate.
void VRHudPanelRootsBegin(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, void*, int64_t) {
    ++frame->code;
    pendingRoots.clear(); pendingSubtitles.clear(); pendingScan = true;
}

namespace {
// GetUserData hands the script an inkUserData reference; the crop is only ever an inkHudEntryInfo.
RED4ext::Handle<RED4ext::ink::HudEntryInfo> EntryInfo(const RED4ext::Handle<RED4ext::ink::UserData>& data) {
    static RED4ext::CClass* const type = RED4ext::CRTTISystem::Get()->GetClass("inkHudEntryInfo");
    if (!data || !type || !data->GetType() || !data->GetType()->IsA(type)) return {};
    return *reinterpret_cast<const RED4ext::Handle<RED4ext::ink::HudEntryInfo>*>(&data);
}
void RejectedOnce(const char* what, bool root, bool window, bool data, bool info) {
    static std::atomic<bool> logged{false};
    if (!logged.exchange(true))
        Log("[hud-panel] %s could not be taken (root=%d window=%d userData=%d inkHudEntryInfo=%d); the script keeps the upkeep\n",
            what, root ? 1 : 0, window ? 1 : 0, data ? 1 : 0, info ? 1 : 0);
}
}

// False when a root cannot be taken (the script then keeps doing its upkeep itself).
void VRHudPanelRoot(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    RED4ext::Handle<RED4ext::ink::Widget> root, window;
    RED4ext::Handle<RED4ext::ink::UserData> data;
    int32_t flags = 0; float padding = 0;
    RED4ext::GetParameter(frame, &root); RED4ext::GetParameter(frame, &window); RED4ext::GetParameter(frame, &data);
    RED4ext::GetParameter(frame, &flags); RED4ext::GetParameter(frame, &padding);
    ++frame->code;
    if (out) *out = false;
    auto info = EntryInfo(data);
    if (!root || !window || (!info && !(flags & RootGazeDot))) { RejectedOnce("a root", bool(root), bool(window), bool(data), bool(info)); return; }
    UpkeepRoot r; r.root = root; r.window = window; r.info = std::move(info); r.flags = static_cast<uint32_t>(flags); r.padding = padding;
    pendingRoots.push_back(std::move(r));
    if (out) *out = true;
}

void VRHudPanelSubtitle(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    RED4ext::Handle<RED4ext::ink::Widget> root, window, panel;
    RED4ext::Handle<RED4ext::ink::UserData> data;
    RED4ext::GetParameter(frame, &root); RED4ext::GetParameter(frame, &window); RED4ext::GetParameter(frame, &data);
    RED4ext::GetParameter(frame, &panel);
    ++frame->code;
    if (out) *out = false;
    auto info = EntryInfo(data);
    if (!root || !window || !info || !panel) { RejectedOnce("a subtitle root", bool(root), bool(window), bool(data), bool(info)); return; }
    UpkeepSubtitle sub; sub.root = root; sub.window = window; sub.panel = panel;
    sub.x = info->offset.X; sub.y = info->offset.Y; sub.width = info->size.X; sub.height = info->size.Y;
    sub.info = std::move(info);
    pendingSubtitles.push_back(std::move(sub));
    if (out) *out = true;
}
