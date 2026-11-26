#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Utils/DebugGate.hpp"
// Inject displacement at the already-buffered native CCT boundary. The original
// move owns contacts, steps, gravity, backend velocity and entity propagation.
//
// THE MOVE IS RUST'S with CVR_RUST_VRIK (rust/crates/cvr-rig/src/vrik/roomscale_move.rs): this file then keeps only the
// registration (so the install order holds), and cvr::roomscale::* forwards from
// src/Hooks/RoomscaleRust.cpp. The capsule tweak is src/Hooks/RoomscaleTweaks.cpp in both builds. The C++ below reaches
// the rest of the plugin through src/Hooks/HookServices.cpp's C forms where tools/vrik_tests must stand in for it.
#include "Hooks/RoomscaleMove.hpp"
#include "Hooks/BodyHeading.hpp"
#include "Hooks/RoomscaleSites.hpp"
#include "Hooks/RoomscaleEligibility.hpp"
#include "Hooks/SwimmingInput.hpp"
#include "Hooks/LadderInput.hpp"
#include "Runtimes/SwimmingMovement.hpp"
#include "Hooks/Hook.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Camera/CameraState.hpp"
#include "Camera/CameraLink.hpp"
#include "Core/LiveControls.hpp"
#include "Core/VrCoreShared.hpp"
#include "Anim/VrikState.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/HeadAimWeapon.hpp"
#include "Fbt/Game/Body.hpp"
#include "Lobby/MenuLobby.hpp"
#include <MinHook.h>
#include <intrin.h>
#include <atomic>
#include <limits>
#include <mutex>
#include <cstdio>
#include "Fbt/Game/Calibration.hpp"
#include <cstring>

#if !CVR_RUST_VRIK
extern void Log(const char* fmt, ...);
#endif

// The exports are src/Hooks/BodyHooksState.cpp's (both builds).
#include "Hooks/RoomscaleDebug.hpp"
extern "C" {
extern int CyberpunkVR_HeadLeadsCapsule,CyberpunkVR_BodyStrictUnderHead,CyberpunkVR_RoomscaleHooksReady;
#if !CVR_RUST_VRIK
extern RoomscaleDebug CyberpunkVR_RoomscaleDebug;
extern std::atomic<uint32_t> CyberpunkVR_RoomscaleDebugSeq;
#endif
extern uint64_t CyberpunkVR_SwimmingFeedbackReads[2],CyberpunkVR_SwimmingPropertyFeedbackReads;
#if !CVR_RUST_VRIK
extern float CyberpunkVR_SwimmingRedirect[3];
#endif
}
#if !CVR_RUST_VRIK
extern "C" int Get3DofMovement();   // Core/VrCore.cpp: the ledger walks the eyes, not the neck, when set
#endif

#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK   // registered from rust/crates/cvr-render/src/hook_table.rs
CVR_HOOK("RoomscaleMovement", ::cvr::hooks::Stage::Boot, 77, cvr_hook_roomscale_install);
#endif
#else
// src/Hooks/HookServices.cpp, src/Hooks/RoomscaleTweaks.cpp, src/Camera/CameraServices.cpp
extern "C" {
uint64_t cvr_cpp_xr_tracking_origin_serial();
bool cvr_cpp_xr_roomscale_sample(cvr::roomscale::Sample* out,OpenXRHeadPose* frameHead,XrTime* frameTime);
void cvr_cpp_xr_flush_hands_to_shared(const OpenXRHeadPose* frameHead,XrTime frameTime,const cvr::roomscale::Vec2* consumed);
float cvr_cpp_tweak_capsule_radius();
void cvr_cpp_camera_player_changed();
}
// The move, the velocity getter given its caller, the property reader, and the binding of the originals: C linkage, so
// tools/vrik_tests calls the same names in both builds.
extern "C" uintptr_t cvr_roomscale_move(void* backend,float* displacement,float dt);
extern "C" float* cvr_roomscale_velocity_from(void* provider,float* output,uintptr_t caller);
extern "C" uintptr_t cvr_roomscale_property(void* registry,uint32_t handle,uint32_t flags,uint32_t mode,
                                           uint32_t property,void* output,uint32_t bytes);

namespace cvr::roomscale {
namespace {
std::atomic<uintptr_t> s_player{}, s_movement{};
uintptr_t s_base{};
std::mutex s_mutex;
Movement s_motion;
std::atomic<bool> s_bodyFollowing{};
bool s_waterMode{};
PhysicalWalkGate s_walkGate;
bool s_strict{};          // the last move was strict: the readers get s_reported in place of s_feedback
Vec2 s_reported{};
Vec2 s_feedback{};
float s_feedbackZ{};
uintptr_t s_feedbackBackend{};
uintptr_t s_feedbackProvider{};
uint32_t s_feedbackHandle{};
uint64_t s_feedbackUs{};
using MoveFn = uintptr_t(__fastcall*)(void*, float*, float);
using VelocityFn = float*(__fastcall*)(void*, float*);
MoveFn s_move{};
VelocityFn s_velocity{};
using PropertyFn=uintptr_t(__fastcall*)(void*,uint32_t,uint32_t,uint32_t,uint32_t,void*,uint32_t);
PropertyFn s_property{};

// XrDiagNowUs, or the tests' counter (g_VRIKClockQpc, 1 MHz).
uint64_t NowUs() { return g_VRIKClockQpc ? static_cast<uint64_t>(g_VRIKClockQpc()) : XrDiagNowUs(); }

template<class T> T Read(const void* p, size_t offset) {
    T value;
    std::memcpy(&value, static_cast<const uint8_t*>(p) + offset, sizeof(value));
    return value;
}

struct NativeState {
    uintptr_t movement{}, provider{}, backend{};
    uint32_t handle{};
    float yaw{}, position[3]{}, velocity[3]{};
    bool enabled{};
};

// SEH is limited to foreign-memory reads. Never catch an exception raised by the
// engine move itself, and never hold our mutex while calling engine/OpenXR code.
bool ReadNative(void* backend, NativeState* out) {
    __try {
        const auto player = s_player.load(std::memory_order_acquire);
        const auto state = s_movement.load(std::memory_order_acquire);
        if (!player || !state || Read<uintptr_t>(backend, 8) != player ||
            Read<uintptr_t>(backend, 0) != s_base + sites::CctBackendVtable) return false;
        const auto s = reinterpret_cast<const void*>(state);
        if (Read<uintptr_t>(s, 0x90) != player) return false;
        const auto provider = Read<uintptr_t>(s, 0xA8);
        const auto adapter = Read<uintptr_t>(s, 0x160);
        if (!provider || !adapter) return false;
        const auto p = reinterpret_cast<const void*>(provider);
        const auto a = reinterpret_cast<const void*>(adapter);
        if (Read<uintptr_t>(p, 0) != s_base + sites::PhysicalProviderVtable ||
            Read<uintptr_t>(p, 0x10) != state ||
            Read<uintptr_t>(a, 0) != s_base + sites::PhysicalAdapterVtable ||
            Read<uintptr_t>(a, 0x18) != state) return false;
        const auto handle = Read<uint32_t>(backend, 0x30);
        if (handle != Read<uint32_t>(p, 0xC4) || handle != Read<uint32_t>(a, 0x40)) return false;
        out->movement = state;
        out->provider = provider;
        out->backend = reinterpret_cast<uintptr_t>(backend);
        out->handle = handle;
        out->enabled = Read<uint8_t>(backend, 0x185) && Read<uint8_t>(s, 0x8B);
        const float z = Read<float>(s, 0x1D8), w = Read<float>(s, 0x1DC);
        if (!std::isfinite(z) || !std::isfinite(w) || z*z+w*w < 0.5f) return false;
        out->yaw = 2.0f * std::atan2(z, w);
        for (int i = 0; i < 3; ++i) {
            out->position[i] = Read<float>(backend, 0x108 + 4*i);
            out->velocity[i] = Read<float>(backend, 0x114 + 4*i);
            if (!std::isfinite(out->position[i]) || !std::isfinite(out->velocity[i])) return false;
        }
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}

// How far from the capsule's axis the eye may lead it at an obstacle. Within the
// capsule's own footprint the controller keeps space free, so the view cannot enter
// geometry; the clearance and the controller's contact offset (7 cm measured: the
// axis stops 0.47 m from a wall) keep the face and the near plane 12 cm off the
// surface the capsule touches. (The capsule itself: src/Hooks/RoomscaleTweaks.cpp.)
constexpr float kLeadClearance = 0.05f;
constexpr float kDefaultCapsuleRadius = 0.25f;

Movement::LeadLimit HeadLeadLimit() {
    static std::atomic<float> radius{0};
    static std::atomic<int> attempts{0};
    float r = radius.load(std::memory_order_relaxed);
    if (r <= 0) {
        r = attempts.fetch_add(1, std::memory_order_relaxed) < 600 ? cvr_cpp_tweak_capsule_radius() : 0;
        if (r > 0) { radius.store(r, std::memory_order_relaxed); Log("[roomscale] capsule radius %.3f\n", r); }
        else r = kDefaultCapsuleRadius;
    }
    Movement::LeadLimit limit{};
    limit.radius = std::max(r - kLeadClearance, 0.0f);
    // The view's offset from the entity before any tracking offset: the camera's
    // calibration, as the view composes it (ComposeViewTranslation with no residual).
    cvr::camera::AnchorRecipe recipe{};
    if (cvr::camera::AnchorRecipeRead(&recipe)) {
        const auto base = cvr::camera::ComposeViewTranslation({}, recipe, recipe.trackingYaw, recipe.modelYaw);
        if (std::isfinite(base.x) && std::isfinite(base.y)) limit.eyeBase = {base.x, base.y};
    }
    return limit;
}

bool IsBackendPlayer(void* backend) {
    __try {
        const auto player = s_player.load(std::memory_order_acquire);
        return player && Read<uintptr_t>(backend, 8) == player;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}
} // namespace
} // namespace cvr::roomscale

uintptr_t cvr_roomscale_move(void* backend, float* displacement, float dt) {
    using namespace cvr::roomscale;
    if (!IsBackendPlayer(backend)) return s_move(backend, displacement, dt);
    const auto player = s_player.load(std::memory_order_acquire);
    Sample sample{};
    OpenXRHeadPose frameHead{};
    XrTime frameTime{};
    cvr_cpp_xr_roomscale_sample(&sample,&frameHead,&frameTime);
    NativeState before{};
    const bool nativeOk = ReadNative(backend, &before);
    const bool allowed = nativeOk && before.enabled && GameplayAllowed();
    const float yaw = BodyTrackingYaw(before.yaw);
    Step step{};
    const bool waterMode=cvr::swimming::Active();
    // On land, and not under full-body tracking (the trackers place the body there).
    const bool strict = CyberpunkVR_BodyStrictUnderHead != 0 && !waterMode && !cvr::fbt::game::BodyActive();
    {
        std::lock_guard lock(s_mutex);
        // Removing/reinstating the land hinge component is a posture-mode
        // change, not a physical step. Keep the consumed camera ledger.
        if(waterMode!=s_waterMode) { s_motion.Suspend();s_waterMode=waterMode; }
        step = s_motion.Begin(sample, NowUs(), dt, GetWorldScale(), yaw, allowed,
                              g_liveControls.xrBodyMoveRadius, strict);
        // The bend tracker's walking mode: in strict mode a walk is the gate's walk, not every move of the head.
        s_bodyFollowing.store(strict ? s_walkGate.Walking() : s_motion.Following(), std::memory_order_relaxed);
    }
    const float native[3]{displacement[0], displacement[1], displacement[2]};
    XrVector3f swimDirection{};int swimState{};
    const bool steer=nativeOk && before.enabled && cvr::swimming::ReadSteering(&swimDirection,&swimState);
    const cvr::swimming::Vector original{native[0],native[1],native[2]};
    const auto redirected=steer ? cvr::swimming::Redirect(original,swimDirection,yaw,swimState==2):original;
    const auto added=redirected-original+cvr::swimming::Vector{step.world.x,step.world.y,0};
    float combined[3]{native[0]+added.x,native[1]+added.y,native[2]+added.z};
    if(cvr::RuntimeDiagnosticsEnabled()) {
    CyberpunkVR_SwimmingRedirect[0]=redirected.x-original.x;
    CyberpunkVR_SwimmingRedirect[1]=redirected.y-original.y;
    CyberpunkVR_SwimmingRedirect[2]=redirected.z-original.z;
    }
    const uintptr_t result = s_move(backend, combined, dt);
    // Not under full-body tracking (Fbt/Game/Body.hpp): there the trackers place the body, and a lead estimated from the
    // capsule's velocity kept growing on open floor too -- the body trailed the head and slid after it.
    const bool headLeads = CyberpunkVR_HeadLeadsCapsule != 0 && !cvr::fbt::game::BodyActive();
    auto leadLimit = headLeads ? HeadLeadLimit() : Movement::LeadLimit{};
    // The ledger walks the neck (EyeFromNeck); the limit is the eye's, so the lever goes back on.
    if (headLeads && Finite(sample.eyeFromNeck))
        leadLimit.eyeBase = leadLimit.eyeBase + Rotate(sample.eyeFromNeck * GetWorldScale(), yaw);
    NativeState after{};
    const bool resultOk = nativeOk && ReadNative(backend, &after);
    Vec2 completedConsumed{};
    {
        std::lock_guard lock(s_mutex);
        if (s_player.load(std::memory_order_acquire) != player) return result;
        // What the controller did with the headset's part of the move; the stick's
        // and the swim redirect's share is the rest of the request.
        const Vec2 accepted = resultOk && headLeads
            ? AcceptedDisplacement({redirected.x,redirected.y},step.world,
                                   {after.velocity[0]*dt,after.velocity[1]*dt})
            : step.world;
        s_motion.Complete(step,accepted,leadLimit);
        completedConsumed=s_motion.Consumed(sample.origin);
        s_feedback = resultOk ? PhysicalVelocity({native[0],native[1]},{added.x,added.y},
            {after.velocity[0],after.velocity[1]},dt) : Vec2{};
        // Pitch and buoyancy are already part of the native vertical solve.
        // Neither heading correction nor roomscale adds vertical velocity.
        s_feedbackZ=0;
        if (strict && resultOk) s_walkGate.Update(s_feedback, dt);
        else s_walkGate.Reset();
        s_strict = strict && resultOk;
        s_reported = s_walkGate.Reported();
        s_feedbackBackend = resultOk ? before.backend : 0;
        s_feedbackProvider = resultOk ? before.provider : 0;
        s_feedbackHandle = resultOk ? before.handle : 0;
        s_feedbackUs = NowUs();
        if(cvr::RuntimeDiagnosticsEnabled()) {
        CyberpunkVR_RoomscaleDebugSeq.fetch_add(1, std::memory_order_acq_rel);
        auto& d = CyberpunkVR_RoomscaleDebug;
        ++d.calls;
        if (Dot(step.world, step.world) > 0) ++d.injected;
        if (step.reason == Reason::Baseline || step.reason == Reason::Discontinuity) ++d.resets;
        d.player = s_player.load(std::memory_order_relaxed);
        d.movement = before.movement; d.backend = before.backend;
        d.poseSequence = sample.sequence; d.origin = sample.origin;
        d.reason = static_cast<uint32_t>(step.reason); d.dt = dt; d.yaw = yaw;
        d.gates = (nativeOk ? 1u : 0u) | (before.enabled ? 2u : 0u) |
                  (GameplayAllowed() ? 4u : 0u) | (sample.valid ? 8u : 0u);
        for (int i = 0; i < 3; ++i) {
            d.native[i] = native[i]; d.before[i] = before.position[i];
            d.after[i] = after.position[i]; d.velocity[i] = after.velocity[i];
        }
        d.requested[0] = step.world.x; d.requested[1] = step.world.y; d.requested[2] = 0;
        const auto consumed = s_motion.Consumed(sample.origin);
        d.consumed[0] = consumed.x; d.consumed[1] = consumed.y;
        d.feedback[0] = s_feedback.x; d.feedback[1] = s_feedback.y;
        d.reported[0] = s_reported.x; d.reported[1] = s_reported.y;
        d.strict = s_strict ? 1u : 0u; d.walking = s_walkGate.Walking() ? 1u : 0u;
        const auto lead = s_motion.Pending() * GetWorldScale();
        d.accepted[0] = accepted.x; d.accepted[1] = accepted.y;
        d.lead[0] = lead.x; d.lead[1] = lead.y; d.leadRadius = leadLimit.radius;
        d.eyeBase[0] = leadLimit.eyeBase.x; d.eyeBase[1] = leadLimit.eyeBase.y;
        CyberpunkVR_RoomscaleDebugSeq.fetch_add(1, std::memory_order_release);
        static uint64_t lastLogUs = 0;
        if (s_feedbackUs - lastLogUs >= 1000000) {
            lastLogUs = s_feedbackUs;
            Log("[roomscale] ticks=%llu moves=%llu reason=%u gates=%X seq=%llu origin=%llu dt=%.4f "
                "request=(%.5f,%.5f) consumed=(%.3f,%.3f) velocity=(%.3f,%.3f) feedback=(%.3f,%.3f)\n",
                d.calls, d.injected, d.reason, d.gates, d.poseSequence, d.origin, dt,
                step.world.x, step.world.y, consumed.x, consumed.y,
                after.velocity[0], after.velocity[1], s_feedback.x, s_feedback.y);
        }
        }
    }
    // Publish after Complete: the head and the consumed displacement must name
    // the same native move. Camera/IK must never subtract a later move's ledger.
    if (nativeOk && before.enabled && PoseFrameAllowed() && sample.valid && frameHead.valid && frameTime>0)
        cvr_cpp_xr_flush_hands_to_shared(&frameHead,frameTime,&completedConsumed);
    return result;
}

float* cvr_roomscale_velocity_from(void* provider, float* output, uintptr_t caller) {
    using namespace cvr::roomscale;
    float* result = s_velocity(provider, output);
    // Water queries share the physics-property reader below; do not subtract
    // twice when a provider wrapper happens to use that same path.
    if(cvr::swimming::Active())return result;
    // The animation call (4C6143 -> return 4C6148) keeps the FULL CCT velocity.
    // Land and both swimming solves must not integrate physical motion again.
    // Their direct call sites are checked against the installed EXE at startup.
    const bool swimDiving=caller==s_base+sites::SwimmingDivingVelocityReturn;
    const bool swimSurface=caller==s_base+sites::SwimmingSurfaceVelocityReturn;
    const bool solver=caller==s_base+sites::SolverVelocityReturn || swimDiving || swimSurface;
    // Every other reader -- the locomotion animation among them -- sees the full velocity in the follow mode. In
    // strict mode it sees the physical part only as a walk (PhysicalWalkGate): the head's every move is a capsule move
    // there, and read raw each one would start and stop the walking animation.
    if(!solver) {
        if(CyberpunkVR_BodyStrictUnderHead==0 || !IsPlayerProvider(provider))return result;
        const auto now = NowUs();
        std::lock_guard lock(s_mutex);
        if (!s_strict || !s_feedbackBackend || reinterpret_cast<uintptr_t>(provider) != s_feedbackProvider ||
            now < s_feedbackUs || now - s_feedbackUs > Movement::MaxAgeUs) return result;
        result[0] += s_reported.x - s_feedback.x;
        result[1] += s_reported.y - s_feedback.y;
        return result;
    }
    if(!IsPlayerProvider(provider))return result;
    const auto now = NowUs();
    std::lock_guard lock(s_mutex);
    if (!s_feedbackBackend || reinterpret_cast<uintptr_t>(provider) != s_feedbackProvider ||
        now < s_feedbackUs || now - s_feedbackUs > Movement::MaxAgeUs) return result;
    result[0] -= s_feedback.x;
    result[1] -= s_feedback.y;
    if(swimDiving || swimSurface) {
        result[2]-=s_feedbackZ;
        CVR_DIAGNOSTIC(++CyberpunkVR_SwimmingFeedbackReads[swimSurface ? 1:0]);
    }
    if(cvr::RuntimeDiagnosticsEnabled()) {
    CyberpunkVR_RoomscaleDebugSeq.fetch_add(1, std::memory_order_acq_rel);
    ++CyberpunkVR_RoomscaleDebug.feedbackReads;
    CyberpunkVR_RoomscaleDebugSeq.fetch_add(1, std::memory_order_release);
    }
    return result;
}

uintptr_t cvr_roomscale_property(void* registry,uint32_t handle,uint32_t flags,uint32_t mode,
                                 uint32_t property,void* output,uint32_t bytes) {
    using namespace cvr::roomscale;
    const auto result=s_property(registry,handle,flags,mode,property,output,bytes);
    // Property4 is the resolved linear velocity (12 bytes), shared by native
    // swimming and script state-vector readers. Filtering a few provider
    // callsites missed the water solver's direct/cached property reads.
    if(!(result&0xff) || property!=4 || bytes!=12 || !output || !cvr::swimming::Active())return result;
    const auto now=NowUs();std::lock_guard lock(s_mutex);
    if(!s_feedbackBackend || handle!=s_feedbackHandle || now<s_feedbackUs || now-s_feedbackUs>Movement::MaxAgeUs)return result;
    auto* velocity=static_cast<float*>(output);
    if(!std::isfinite(velocity[0]) || !std::isfinite(velocity[1]) || !std::isfinite(velocity[2]))return result;
    velocity[0]-=s_feedback.x;velocity[1]-=s_feedback.y;velocity[2]-=s_feedbackZ;
    CVR_DIAGNOSTIC(++CyberpunkVR_SwimmingPropertyFeedbackReads);
    return result;
}

// The image base and the three originals: the install's, or the tests' fakes.
extern "C" void cvr_roomscale_bind(uintptr_t base,void* move,void* velocity,void* property) {
    using namespace cvr::roomscale;
    s_base=base;
    s_move=reinterpret_cast<MoveFn>(move);s_velocity=reinterpret_cast<VelocityFn>(velocity);
    s_property=reinterpret_cast<PropertyFn>(property);
}

namespace cvr::roomscale {
namespace {
// The getter's return address names its caller.
float* __fastcall Velocity(void* provider, float* output) {
    return cvr_roomscale_velocity_from(provider,output,reinterpret_cast<uintptr_t>(_ReturnAddress()));
}

bool Matches(const sites::Site& site) {
    const auto bytes = reinterpret_cast<const uint8_t*>(s_base + site.rva);
    for (size_t i = 0; site.bytes[i*2]; ++i) {
        unsigned expected{};
        if (std::sscanf(site.bytes + i*2, "%2x", &expected) != 1 || bytes[i] != expected) return false;
    }
    return true;
}

bool InstallRoomscale() {
    s_base = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    if (!s_base || !Matches(sites::CctMove) || !Matches(sites::Velocity) || !Matches(sites::SolverCall) ||
        !Matches(sites::PhysicsProperty)) {
        Log("Roomscale: native ABI signature mismatch; hooks not installed.\n");
        return false;
    }
    const auto init = MH_Initialize();
    if (init != MH_OK && init != MH_ERROR_ALREADY_INITIALIZED) return false;
    void* move = reinterpret_cast<void*>(s_base + sites::CctMove.rva);
    void* velocity = reinterpret_cast<void*>(s_base + sites::Velocity.rva);
    void* property = reinterpret_cast<void*>(s_base + sites::PhysicsProperty.rva);
    if (MH_CreateHook(move, reinterpret_cast<void*>(&cvr_roomscale_move), reinterpret_cast<void**>(&s_move)) != MH_OK) return false;
    if (MH_CreateHook(velocity, reinterpret_cast<void*>(&Velocity), reinterpret_cast<void**>(&s_velocity)) != MH_OK) {
        MH_RemoveHook(move); return false;
    }
    if(MH_CreateHook(property,reinterpret_cast<void*>(&cvr_roomscale_property),reinterpret_cast<void**>(&s_property))!=MH_OK) {
        MH_RemoveHook(move);MH_RemoveHook(velocity);return false;
    }
    if (MH_EnableHook(velocity) != MH_OK || MH_EnableHook(move) != MH_OK || MH_EnableHook(property)!=MH_OK) {
        MH_DisableHook(move); MH_DisableHook(velocity);MH_DisableHook(property);
        MH_RemoveHook(move); MH_RemoveHook(velocity);MH_RemoveHook(property);
        return false;
    }
    CyberpunkVR_RoomscaleHooksReady = 1;
    Log("Roomscale: native CCT displacement + solver-only velocity feedback installed (2.31).\n");
    return true;
}
} // namespace

void SetPlayer(uintptr_t player) {
    if (s_player.exchange(player, std::memory_order_acq_rel) == player) return;
    // A previous puppet can keep animating after a player replacement. Its
    // camera counters still advance, so the worker's idle recovery never fires.
    cvr_cpp_camera_player_changed();
    s_movement.store(0, std::memory_order_release);
    std::lock_guard lock(s_mutex);
    s_motion.Reset(); s_feedback = {};s_feedbackZ=0; s_feedbackBackend = 0; s_feedbackProvider = 0; s_feedbackUs = 0;
    CyberpunkVR_EngineBodyYawValid = 0;
    CyberpunkVR_PlayerEntityValid = 0;
    BodyYawFollowRelease();
    BodyHeadingReset();
    Log("Roomscale: player identity -> %p, tracking baseline reset.\n", reinterpret_cast<void*>(player));
}

bool IsPlayerMovement(const void* state) {
    __try {
        const auto player = s_player.load(std::memory_order_acquire);
        return player && state && Read<uintptr_t>(state, 0x90) == player;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}

uintptr_t PlayerIdentity() { return s_player.load(std::memory_order_acquire); }

bool IsPlayerProvider(const void* provider) {
    __try {
        if (!provider || Read<uintptr_t>(provider, 0) != s_base + sites::PhysicalProviderVtable) return false;
        const auto state = reinterpret_cast<const void*>(Read<uintptr_t>(provider, 0x10));
        return IsPlayerMovement(state) && Read<uintptr_t>(state, 0xA8) == reinterpret_cast<uintptr_t>(provider);
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}

void ObserveMovement(void* state) {
    if (IsPlayerMovement(state)) s_movement.store(reinterpret_cast<uintptr_t>(state), std::memory_order_release);
}

bool GameplayAllowed() {
    const int loco=g_VRLocomotionState;
    // Not while full-body tracking calibrates: V stays where it stands and the player walks into it.
    return g_liveControls.xrRoomscaleMovement != 0 && PhysicalBodyAllowed() && !cvr::fbt::game::Calibrating() &&
        loco!=6 && loco!=9 && loco!=10 && loco!=11;
}

static bool OnFootViewAllowed() {
    // The lobby is a place, as for PhysicalBodyAllowed: its body walks and turns through the native move, so its view
    // takes that move's frame too. Left to the menu's sources there, the camera and the hands read the head from
    // other instants than the move the body was drawn with -- the body shook against the view on a turn.
    return (g_menuModeValue == 0 || cvr::lobby::WorldShown()) && !DeviceCamActive() &&
        !g_bdActive.load(std::memory_order_relaxed);
}

static bool PhysicalBodyScopeEnabled() {
    return !cvr::ladder::Active() && g_pSharedHands &&
        !DeviceCamActive() && !g_bdActive.load(std::memory_order_relaxed) &&
        IsVrikBodyMovementAllowed(g_VRBind,
        cvr::anim::IsHeadAimWeaponActive(),g_isInVehicle,
        g_sceneTier.load(std::memory_order_relaxed),g_liveControls.xrCutsceneSuspendTier);
}

bool PhysicalBodyEnabled() {
    return PhysicalBodyScopeEnabled() && g_VRBoneCount>0 && g_VRHeadBoneIdx>=0;
}

bool PhysicalBodyHeadingOwned() {
    // Do not observe the half-built metadata from ArmPlayerLocked. The camera
    // retains cancellation until that binding completes (or fails), while
    // PhysicalBodyAllowed still refuses new motion without a ready skeleton.
    return PhysicalBodyScopeEnabled() && g_PlayerPoseHeadingReady.load(std::memory_order_acquire);
}

bool PhysicalBodyAllowed() {
    // The main menu is a menu (Hooks/MenuMode.cpp keeps it one under the lobby), but the lobby is a place: the
    // player stands in its world, so the body walks with the room there as it does in the game.
    return (g_menuModeValue==0 || cvr::lobby::WorldShown()) && PhysicalBodyEnabled();
}

bool PoseFrameAllowed() {
    // Native provider/adapter validation additionally excludes climb/animation
    // controllers. Script locomotion flags supplement it; they do not identify an owner.
    // Keep publishing the camera's coherent pose when VRIK is switched off;
    // only physical body motion depends on the solver mode, not the view.
    return OnFootViewAllowed() && !g_isInVehicle && IsVrikPoseTierAllowed(
        g_sceneTier.load(std::memory_order_relaxed),VRIK_EffectiveCutsceneSuspendTier());
}

Vec2 CameraConsumed(uint64_t origin) {
    std::lock_guard lock(s_mutex);
    if (!GameplayAllowed()) s_motion.Suspend();
    // Keep the offset when movement is disabled. Dropping it would apply all
    // distance already travelled a second time to the camera.
    return s_motion.Consumed(origin);
}

Vec2 CameraConsumed() {
    return CameraConsumed(cvr_cpp_xr_tracking_origin_serial());
}

bool CameraRestNeck(uint64_t origin, Vec2* out) {
    if (Get3DofMovement() != 0) return false;
    std::lock_guard lock(s_mutex);
    return s_motion.RestNeck(origin, out);
}

bool CameraRestNeck(Vec2* out) {
    return CameraRestNeck(cvr_cpp_xr_tracking_origin_serial(), out);
}

bool BodyFollowing() {
    return s_bodyFollowing.load(std::memory_order_relaxed);
}

CVR_HOOK("RoomscaleMovement", ::cvr::hooks::Stage::Boot, 77, InstallRoomscale);
} // namespace cvr::roomscale
#endif
#endif
