#if !CVR_RUST_VRIK   // the Rust build registers this file's hook from rust/crates/cvr-render/src/hook_table.rs
// Physical contacts on original world screens. Use the game's mesh/UV picker,
// including mesh-bound curved displays, rather than an assumed rectangle or gaze.
#include "Natives/NativeFunctions.hpp"
#include "Natives/VrMenu.hpp"
#include "Core/GameBuild.hpp"
#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Overlay/VrOverlay.hpp"
#include <RED4ext/Scripting/Natives/inkWidget.hpp>
#include <RED4ext/Scripting/Natives/entIComponent.hpp>
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <windows.h>
#include <cmath>
#include <cstring>

#if CVR_RUST_VRIK
// The call patch, the thunk's callback and VRWorldTouchPointer are rust/crates/cvr-rig/src/vrik/world_touch.rs (with
// CvrWorldTouchTriangleOriginal); the thunk (src/Hooks/WorldTouchTriangleThunk.asm) and the registration stay.
extern "C" bool cvr_hook_world_touch_install();
CVR_HOOK("WorldTouch",::cvr::hooks::Stage::Boot,88,cvr_hook_world_touch_install);
#else
extern "C" { void* CvrWorldTouchTriangleOriginal{}; void CvrWorldTouchTriangleThunk(); }

namespace {
using namespace RED4ext;
struct WorldHit {
    WeakHandle<ink::Widget> window;
    WeakHandle<ent::IComponent> component;
    Vector2 local, cursor, screen;
};
static_assert(sizeof(WorldHit)==56);
struct Ray { float origin[3], direction[3]; };
struct Contact { bool hit{}; Vector4 point{}; };
thread_local Contact* queryContact{};
using PickFn=bool(*)(void*,const Handle<IScriptable>*,const Ray*,WorldHit*,bool*);
PickFn meshPick{},planePick{};
bool ready{};

struct QueryScope {
    Contact* previous;
    explicit QueryScope(Contact& contact):previous(queryContact){queryContact=&contact;}
    ~QueryScope(){queryContact=previous;}
};
bool Install() {
    if(!cvr::game_build::Supported() || cvr::game_build::Overridden())return false;
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    // Verified in the 2.5 database matching the installed executable.
    const uint8_t tri[]={0x40,0x55,0x48,0x8b,0xec,0x48,0x83,0xec,0x40,0x48,0x8b,0x45,0x30};
    const uint8_t mesh[]={0x48,0x8b,0xc4,0x4c,0x89,0x48,0x20,0x4c,0x89,0x40,0x18};
    const uint8_t plane[]={0x48,0x8b,0xc4,0x48,0x89,0x58,0x18,0x48,0x89,0x70,0x20,0x48,0x89,0x48,0x08};
    if(memcmp(reinterpret_cast<void*>(base+0x20fb94),tri,sizeof(tri)) ||
       memcmp(reinterpret_cast<void*>(base+0x29ef804),mesh,sizeof(mesh)) ||
       memcmp(reinterpret_cast<void*>(base+0x29f006c),plane,sizeof(plane)))return false;
    // Patch ONLY the world-screen picker's call. The triangle leaf has a
    // stronger-than-Win64 register contract; the thunk preserves its full result.
    auto* site=reinterpret_cast<uint8_t*>(base+0x29efd49);
    int32_t rel{};std::memcpy(&rel,site+1,4);
    if(site[0]!=0xe8 || site+5+rel!=reinterpret_cast<uint8_t*>(base+0x20fb94))return false;
    SYSTEM_INFO info{};GetSystemInfo(&info);
    const uintptr_t gran=info.dwAllocationGranularity;
    uint8_t* relay{};
    for(uintptr_t distance=gran;!relay && distance<0x70000000ull;distance+=gran) {
        for(uintptr_t candidate:{reinterpret_cast<uintptr_t>(site)+distance,reinterpret_cast<uintptr_t>(site)-distance}) {
            relay=static_cast<uint8_t*>(VirtualAlloc(reinterpret_cast<void*>(candidate&~(gran-1)),gran,
                MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE));
            if(relay)break;
        }
    }
    if(!relay)return false;
    const intptr_t delta=relay-(site+5);
    if(delta<INT32_MIN || delta>INT32_MAX){VirtualFree(relay,0,MEM_RELEASE);return false;}
    uint8_t jump[14]={0xff,0x25,0,0,0,0};
    const auto destination=reinterpret_cast<uintptr_t>(&CvrWorldTouchTriangleThunk);
    std::memcpy(jump+6,&destination,8);std::memcpy(relay,jump,sizeof(jump));
    DWORD old{};
    if(!VirtualProtect(relay,gran,PAGE_EXECUTE_READ,&old)){VirtualFree(relay,0,MEM_RELEASE);return false;}
    FlushInstructionCache(GetCurrentProcess(),relay,sizeof(jump));
    if(!VirtualProtect(site,5,PAGE_EXECUTE_READWRITE,&old)){VirtualFree(relay,0,MEM_RELEASE);return false;}
    CvrWorldTouchTriangleOriginal=reinterpret_cast<void*>(base+0x20fb94);
    const int32_t displacement=static_cast<int32_t>(delta);
    std::memcpy(site+1,&displacement,4);
    FlushInstructionCache(GetCurrentProcess(),site,5);
    DWORD ignored{};VirtualProtect(site,5,old,&ignored);
    meshPick=reinterpret_cast<PickFn>(base+0x29ef804);
    planePick=reinterpret_cast<PickFn>(base+0x29f006c);
    ready=true;return true;
}
CVR_HOOK("WorldTouch",::cvr::hooks::Stage::Boot,88,Install);
bool Visible(Handle<ink::Widget> widget) {
    for(int i=0;widget && i<32;++i) {
        if(!widget->visible || widget->opacity<=0)return false;
        widget=widget->parentWidget.Lock();
    }
    return !widget;
}
}

extern "C" void CvrWorldTouchTriangleAfter(bool found,const RED4ext::Vector4* hit) {
    if(found && queryContact){queryContact->hit=true;queryContact->point=*hit;}
}

// component, world centre and outward unit normal (current component transform).
// Each hand: (native ink X,Y, signed metres in front of the REAL screen, valid),
// followed by its contact point in world space. No click/button input is generated.
void VRWorldTouchPointer(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,
                        RED4ext::DynArray<RED4ext::Vector4>* out,int64_t) {
    using namespace RED4ext;
    Handle<IScriptable> component;
    Vector4 centre{},normal{};
    GetParameter(frame,&component);GetParameter(frame,&centre);GetParameter(frame,&normal);++frame->code;
    if(!out)return;
    out->Clear();
    if(!ready || !component || g_menuModeValue!=0 || cvr::vrmenu::CapturesButtons() || cvr::vrui::CapturesInput() ||
       !component->GetType()->IsA(CRTTISystem::Get()->GetClass("WorldWidgetComponent")))return;
    const float n=std::sqrt(normal.X*normal.X+normal.Y*normal.Y+normal.Z*normal.Z);
    if(!std::isfinite(n) || n<.9f || n>1.1f)return;
    normal.X/=n;normal.Y/=n;normal.Z/=n;
    // Drop points have a recessed screen: their script uses a 10 cm approach
    // press and 13 cm rearm. Keep ordinary touch screens at the original reach.
    auto* const owner=static_cast<ent::IComponent*>(component.GetPtr())->owner;
    const auto dropPointType=CRTTISystem::Get()->GetClass("DropPoint");
    const float reach=owner && dropPointType && owner->GetType()->IsA(dropPointType) ? .18f : .075f;
    for(int side=0;side<2;++side) {
        Vector4 sample{-1,-1,0,0},point{};
        float finger[3];
        if(cvr::vrmenu::FingerWorld(side,finger)) {
            // Limit the search in front of the finger. A far hit cannot activate a button.
            Ray ray{{finger[0]+normal.X*.08f,finger[1]+normal.Y*.08f,finger[2]+normal.Z*.08f},
                    {-normal.X,-normal.Y,-normal.Z}};
            WorldHit hit{};
            Contact contact{};
            bool fromBehind=false,found=false;
            {
                QueryScope scope(contact);
                found=meshPick(nullptr,&component,&ray,&hit,&fromBehind);
            }
            if(found && !contact.hit)found=false;
            if(!found && planePick(nullptr,&component,&ray,&hit,&fromBehind)) {
                // Native planar fallback refuses components with a mesh hit surface.
                const float d=(finger[0]-centre.X)*normal.X+(finger[1]-centre.Y)*normal.Y+(finger[2]-centre.Z)*normal.Z;
                contact.point={finger[0]-normal.X*d,finger[1]-normal.Y*d,finger[2]-normal.Z*d,1};
                found=true;
            }
            const auto window=hit.window.Lock();
            if(found && window && Visible(window) && hit.component.instance==component.GetPtr()) {
                point=contact.point;
                const float d=(finger[0]-point.X)*normal.X+(finger[1]-point.Y)*normal.Y+(finger[2]-point.Z)*normal.Z;
                if(std::isfinite(hit.local.X+hit.local.Y+d) && d>=-.035f && d<=reach)
                    sample={hit.local.X,hit.local.Y,d,1};
            }
        }
        out->PushBack(sample);out->PushBack(point);
    }
}
#endif
#endif
