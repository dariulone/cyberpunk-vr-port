// WheelGrab -- give the arm back to the driving animation, and steer with your hands.
//
// Ported from iPowerTech's fork (425d4262 "wip motioncontroller vehicle steering" and 51861118
// "Shoot while you drive, steering deadzone, steering sensitivity, VR Horn"), which was written
// against 0.1.1 -- before this tree was restructured and before the dxgi proxy was retired. The
// grab, animation handoff and horn originate there. Steering now calibrates at
// the grab position and integrates signed frame-to-frame rotation; see WheelSteering.hpp.
// Settings are read from g_liveControls and state stays inside this plugin.
//
// THE IDEA, and it is the whole reason this works everywhere: driving, the engine ALREADY animates
// both hands onto the wheel (or the handlebars), and that pose is sitting in the bone buffer on every
// fresh solve before we touch anything. So there is no steering wheel to find, no vehicle bone to
// resolve and no per-model offset table -- the reference pose IS the animation, which is why the same
// code works on a bike, in any car, and in vehicles this port has never seen.
//
// Per hand, independently: bring a hand to where the animation holds the wheel, click that grip,
// and the arm is handed back to the animation (fingers included -- VRIK never writes finger bones, so
// the native grip comes for free). Click again to release the arm to the controller, wherever that
// hand is. The other hand is unaffected, so you can keep one on the wheel and the other on a gun.
//
// Handing an arm back means three writes must stop TOGETHER, not just the IK solve:
//   * VRIK_SolveArm              -- the rotations
//   * VRIK_ScaleArmBonesFromRest -- the segment lengths scaled to the player's real arm; leave them
//                                  in and the hand lands next to the wheel, not on it
//   * the solve cache            -- the same-tick replay would re-apply the last solved locals 4-5x
//                                  per tick and glue the arm right back where it was
// and the two translation writes that are NOT reverted by the engine (the length scale and the
// shoulder protraction) have to be put back to the rig's rest values on the way out. That is what
// VRIK_RestoreArmRestTrans in src/Anim/CharacterRig.cpp does.

#include "Anim/WheelGrab.hpp"
#include "Anim/WheelSteering.hpp"
#include "Anim/TrackedWheelSteering.hpp"
#include "Runtimes/OpenXRManager.hpp"

#include "Anim/CharacterRig.hpp"
#include "Anim/VrikHook.hpp"
#include "Anim/VrikState.hpp"
#include "Core/LiveControls.hpp"
#include "Core/VrCoreShared.hpp"   // g_isDriving, g_isInVehicle, g_hasWeaponEquipped
#include "Camera/CameraState.hpp"
#include "Overlay/ImGuiOverlay.hpp"
#include "Utils/SharedSlots.hpp"

#include <cmath>
#include <cstdint>
#include <bit>
#include <mutex>
#include <windows.h>

extern float* g_pSharedHands;

namespace cvr::anim {

std::atomic<float> g_wheelBlendRight{0.0f};
std::atomic<float> g_wheelBlendLeft{0.0f};
std::atomic<float> g_wheelSteer{0.0f};
std::atomic<float> g_wheelSteerDeg{0.0f};
std::atomic<bool> g_wheelMountedCar{false};
void WheelSetMountedCar(bool car) {g_wheelMountedCar.store(car,std::memory_order_relaxed);}
std::atomic<int>   g_wheelHornMask{0};

namespace {

struct WheelHand {
    float blend       = 0.0f;   // 0 = arm IK drives the hand, 1 = animation does
    bool  engaged     = false;  // latched by a grip click that started at the wheel
    bool  atWheel     = false;  // controller is within the radius of the animated hand
    bool  atHub       = false;  // controller is on the wheel HUB -> horn
    bool  gripPrev    = false;
    bool  targetValid = false;
    float target[3]   = {};     // last solve's IK target (model space) -- the player's real hand
    float animPos[3]  = {};     // this solve's ANIMATED hand position (model space)
    float animRot[4]  = { 0.0f, 0.0f, 0.0f, 1.0f };
    bool  animValid   = false;
    uint64_t targetFrame = 0;
};
WheelHand g_wheel[2];           // [0] = right, [1] = left

// WHEEL CENTRE, model space, for the horn and nominal radius. The driving animation
// holds the wheel at 9 and 3, so their midpoint is the hub -- captured while nothing is grabbed and
// then FROZEN for the whole grab. Frozen, because with a weapon out the game switches to a one-handed
// driving pose, and a live midpoint would then wander off the hub and take the steering with it. A
// rotation about the hub does not move the hub, so freezing costs nothing while the pose stays
// two-handed either.
float g_wheelCenter[3] = {};
bool  g_wheelCenterValid = false;
// Distance between the two ANIMATED hands, captured with the centre: the wheel's diameter as the
// animation holds it. It is the reference LEVER for the steering measurement below.
float g_wheelSpan = 0.0f;
TrackedWheelSteering g_steering;
std::mutex g_steerMutex;
uint64_t g_sampledGrabEpoch=0;
uint64_t g_wheelFrame=0;
// Low two bits are ownership, upper bits identify a new grab even if an input
// poll did not observe the release between two grabs.
std::atomic<uint64_t> g_grabState{0};
std::atomic<float> g_physicalRadius{.19f};
std::atomic<uint64_t> g_wheelStampMs{0};
struct GamepadProfile {float inner,outer;};
std::atomic<uint64_t> g_padProfile{std::bit_cast<uint64_t>(GamepadProfile{.35f,.9f})};

// HORN. Fallback hub radius, and the range the setting is trusted in. The hub is the small pad in the
// middle of the wheel, not the wheel: 12 cm reaches it with a hand you cannot see while still sitting
// well inside the ~17 cm the rim is held at.
constexpr float kHornRadiusDefault = 0.12f;
constexpr float kHornRadiusMin     = 0.04f;
constexpr float kHornRadiusMax     = 0.30f;
// Leaves at a slightly wider radius than it enters. A hand held right on the boundary would otherwise
// chatter the horn on and off every solve -- an audible stutter, not a honk.
constexpr float kHornHysteresis    = 0.03f;

// Engage slower than release: reaching for the wheel is deliberate, letting go is a reaction.
constexpr float kEngageSec  = 0.16f;
constexpr float kReleaseSec = 0.11f;
// Above this the arm is handed over completely (nothing written at all). Below 1 the solve still
// runs, with the target blended toward the animated hand -- that is what makes it a movement instead
// of a snap.
constexpr float kFullBlend  = 0.995f;

// RAW slot read, for the two GRIPS only. Everything else this feature needs is a plugin global now,
// but the grips are a real channel: the XInput detour publishes them for the CET mods, and reading
// them back here is what every other high-slot consumer in the plugin does. SharedPose() covers only
// [0..126] -- the size of the seqlock snapshot -- and [155] is past the end of that array.
inline float WheelSlot(int i) {
    return (g_pSharedHands && i >= 0 && i < vrshared::kSlotCount) ? g_pSharedHands[i] : 0.0f;
}

float g_relaxFingerRot[2][32][4] = {};
bool  g_relaxFingerHave[2] = { false, false };

}  // namespace

void WheelCaptureAnim(int hand, int handIdx) {
    if (hand < 0 || hand > 1) return;
    WheelHand& w = g_wheel[hand];
    w.animValid = false;
    if (handIdx < 0 || handIdx >= VRIK_MAX_BONES) return;
    w.animPos[0] = g_fkPos[handIdx][0];
    w.animPos[1] = g_fkPos[handIdx][1];
    w.animPos[2] = g_fkPos[handIdx][2];
    w.animRot[0] = g_fkRot[handIdx][0];
    w.animRot[1] = g_fkRot[handIdx][1];
    w.animRot[2] = g_fkRot[handIdx][2];
    w.animRot[3] = g_fkRot[handIdx][3];
    w.animValid = true;
}

// One state update per FRESH solve. The proximity test uses the target the previous solve built (one
// tick old): the target for this solve is not computed until well inside the arm block, and a tick of
// lag on a 28 cm radius is not a thing a hand can outrun.
void WheelUpdate(float dtSec) {
    ++g_wheelFrame;
    const bool enabled = (g_liveControls.xrWheelGrab != 0);
    const bool driving = g_isDriving.load(std::memory_order_relaxed);
    float radius = g_liveControls.xrWheelRadius;
    if (!(radius > 0.05f) || radius > 1.0f) radius = 0.28f;
    if (dtSec < 0.0f) dtSec = 0.0f;
    if (dtSec > 0.10f) dtSec = 0.10f;

    // A DRAWN WEAPON OWNS THE RIGHT HAND. With a gun equipped the right hand shoots -- the input side
    // has already taken its trigger off the throttle and put it on the weapon -- so that hand cannot
    // also take hold of the wheel: no proximity, no grab, no arming (which leaves its grip as the
    // normal gameplay button the holster system expects). Holster the weapon and the right hand gets
    // the wheel back. The LEFT hand is never gated: driving one-handed with a gun out is the entire
    // point of the mode.
    const bool weaponOut = g_hasWeaponEquipped;

    int armedMask = 0;
    for (int h = 0; h < 2; ++h) {
        WheelHand& w = g_wheel[h];
        const bool tracked=SharedPose(h==0 ? 8:0)>0;
        const bool handBlocked = ((h == 0) && weaponOut) || !tracked;
        if(!tracked)w.targetValid=false;
        // [49] is the right grip, [155] the left -- both binary, both published every XInput poll,
        // neither inside the hands seqlock.
        const bool grip = (h == 0) ? (WheelSlot(49) > 0.5f)
                                   : (WheelSlot(vrshared::kLeftGripPressed) > 0.5f);
        w.atWheel = false;
        if (enabled && driving && !handBlocked && w.animValid && w.targetValid) {
            const float dx = w.target[0] - w.animPos[0];
            const float dy = w.target[1] - w.animPos[1];
            const float dz = w.target[2] - w.animPos[2];
            w.atWheel = (dx*dx + dy*dy + dz*dz) <= (radius * radius);
        }

        if (!enabled || !driving || handBlocked) {
            // In practice the right hand is already OFF the wheel when a weapon appears -- equipping
            // means reaching to the holster and squeezing there, which is the opposite end of the
            // gesture. This clears the grab anyway: a weapon equipped some other way (a script, the
            // radial, a keyboard) must not leave the hand welded to the wheel, and the blend below
            // then walks the arm back over the usual ~0.1 s instead of snapping.
            w.engaged = false;
        } else if (grip && !w.gripPrev) {
            if (w.engaged) {
                w.engaged = false;                  // second click releases, wherever the hand moved
            } else if (w.atWheel) {
                w.engaged = true;                   // first click must begin at the wheel/handlebar
            }
        }
        // Kept up to date even while blocked: a grip still held when the weapon is holstered is not a
        // fresh click, so the wheel is not re-grabbed behind the player's back.
        w.gripPrev = grip;

        const float step = dtSec / (w.engaged ? kEngageSec : kReleaseSec);
        if (w.engaged) { w.blend += step; if (w.blend > 1.0f) w.blend = 1.0f; }
        else           { w.blend -= step; if (w.blend < 0.0f) w.blend = 0.0f; }

        // ARMED = the grip is not a gameplay button right now. Raised on proximity alone, before any
        // press, so the CET side never leaks the first frame of the squeeze.
        if (w.atWheel || w.engaged)
            armedMask |= (h == 0) ? vrshared::kWheelArmedRightBit : vrshared::kWheelArmedLeftBit;
    }

    // WHEEL CENTRE. Track it while nothing is held; freeze it for the duration of a grab.
    if (!g_wheel[0].engaged && !g_wheel[1].engaged) {
        if (g_wheel[0].animValid && g_wheel[1].animValid) {
            const float sx = g_wheel[0].animPos[0] - g_wheel[1].animPos[0];
            const float sy = g_wheel[0].animPos[1] - g_wheel[1].animPos[1];
            const float sz = g_wheel[0].animPos[2] - g_wheel[1].animPos[2];
            const float span = std::sqrt(sx*sx + sy*sy + sz*sz);
            // A one-handed driving pose (weapon out) collapses the two animated hands together; that
            // midpoint is not the hub and that span is not the wheel. Keep the last good one.
            if (span > 0.15f) {
                g_wheelCenter[0] = (g_wheel[0].animPos[0] + g_wheel[1].animPos[0]) * 0.5f;
                g_wheelCenter[1] = (g_wheel[0].animPos[1] + g_wheel[1].animPos[1]) * 0.5f;
                g_wheelCenter[2] = (g_wheel[0].animPos[2] + g_wheel[1].animPos[2]) * 0.5f;
                g_wheelSpan = span;
                g_wheelCenterValid = true;
            }
        }
        // Nothing held -> nothing to steer with. Cleared here rather than in the steering pass so it
        // is also cleared on the solves where the arm blocks never run.
        g_grabState.fetch_and(~uint64_t(3),std::memory_order_release);
        g_wheelSteer.store(0,std::memory_order_relaxed);
        g_wheelSteerDeg.store(0,std::memory_order_relaxed);
    }

    // HORN. Laying a hand on the middle of the wheel is the gesture everyone already knows, so a
    // controller inside a small sphere at the (frozen) wheel centre holds the vehicle's horn button
    // down. Measured against the same centre the one-handed steering uses -- the midpoint of the two
    // ANIMATED hands, i.e. the hub -- so it needs no bone and no per-vehicle table.
    //
    // A hand that is GRABBING is excluded: the arm is the animation's, the controller is free to
    // wander, and working a wheel two-handed regularly takes a hand across the hub. Toggling the grab
    // off turns a hand at the hub back into a horn.
    int hornMask = 0;
    {
        const bool hornEnabled = (g_liveControls.xrWheelHorn != 0);
        float hornR = g_liveControls.xrWheelHornRadius;
        if (!(hornR >= kHornRadiusMin) || hornR > kHornRadiusMax) hornR = kHornRadiusDefault;
        const float rIn  = hornR;
        const float rOut = hornR + kHornHysteresis;

        for (int h = 0; h < 2; ++h) {
            WheelHand& w = g_wheel[h];
            bool at = false;
            // The gun hand does not honk: with a weapon drawn the right hand is held out in front of
            // the driver all the time, and the hub sphere sits exactly where it passes.
            const bool hornBlocked = (h == 0) && weaponOut;
            if (hornEnabled && driving && !hornBlocked
                && g_wheelCenterValid && w.targetValid && !w.engaged) {
                const float dx = w.target[0] - g_wheelCenter[0];
                const float dy = w.target[1] - g_wheelCenter[1];
                const float dz = w.target[2] - g_wheelCenter[2];
                const float d2 = dx*dx + dy*dy + dz*dz;
                const float r  = w.atHub ? rOut : rIn;
                at = (d2 <= r * r);
            }
            w.atHub = at;
            if (at) hornMask |= (h == 0) ? vrshared::kWheelArmedRightBit
                                         : vrshared::kWheelArmedLeftBit;
        }
    }

    g_wheelBlendRight.store(g_wheel[0].blend, std::memory_order_relaxed);
    g_wheelBlendLeft.store(g_wheel[1].blend, std::memory_order_relaxed);
    g_wheelHornMask.store(hornMask, std::memory_order_relaxed);
    // THE ONE THING THAT STILL CROSSES A BOUNDARY: the CET mods read the grips out of the shared
    // block ([49] and [155] feed holster equip, smoking poses, basketball grab and reload's magazine
    // hand), and a grip latched onto the wheel must not also mean any of those.
    if (g_pSharedHands) g_pSharedHands[vrshared::kWheelArmedMask] = static_cast<float>(armedMask);
}

// Animation decides proximity/ownership. Its moving model-space targets must
// never become steering measurements: a native wheel turn moves that frame.
void WheelPublishGrab() {
    uint8_t held=0;
    for(int h=0;h<2;++h)if(g_wheel[h].engaged && g_wheel[h].targetValid &&
                           g_wheel[h].targetFrame==g_wheelFrame)held|=uint8_t(1u<<h);
    const auto previous=g_grabState.load(std::memory_order_relaxed);
    const auto epoch=(previous>>2)+((previous&3)==0 && held ? 1:0);
    if(g_wheelSpan>.15f)g_physicalRadius.store(g_wheelSpan*.5f,std::memory_order_relaxed);
    g_wheelStampMs.store(GetTickCount64(),std::memory_order_relaxed);
    g_grabState.store((epoch<<2)|held,std::memory_order_release);
}

void WheelMaintainGrab() {
    // Camera availability does not determine whether a tracked, latched wheel grab remains active.
    // Without this heartbeat, a >250ms camera miss reset the physical pivot
    // mid-turn even though controllers were fresh. New grabs still require a
    // normal proximity solve; this path can only retain or release old grabs.
    const bool allowed=g_liveControls.xrWheelGrab && g_isDriving.load(std::memory_order_relaxed);
    for(int h=0;h<2;++h) {
        auto& w=g_wheel[h];
        const bool grip=WheelSlot(h==0 ? 49:vrshared::kLeftGripPressed)>.5f;
        const bool tracked=SharedPose(h==0 ? 8:0)>0;
        if(!allowed || !tracked || (h==0 && g_hasWeaponEquipped))w.engaged=false;
        else if(w.engaged && grip && !w.gripPrev)w.engaged=false; // release still works during a camera miss
        w.gripPrev=grip;
        if(!w.engaged)w.blend=0;
    }
    g_wheelBlendRight.store(g_wheel[0].blend,std::memory_order_relaxed);
    g_wheelBlendLeft.store(g_wheel[1].blend,std::memory_order_relaxed);
    WheelPublishGrab();
}

int WheelControlState() {
    if(!g_isDriving.load(std::memory_order_relaxed) || !g_liveControls.xrWheelGrab ||
       g_menuModeValue!=0 || OverlayIsVisible())return 0;
    const auto grab=g_grabState.load(std::memory_order_acquire);
    const auto stamp=g_wheelStampMs.load(std::memory_order_relaxed),now=GetTickCount64();
    return 4|((stamp && now>=stamp && now-stamp<=250) ? int(grab&3):0);
}
float WheelSteerInput() {
    const int state=WheelControlState();
    const auto grab=g_grabState.load(std::memory_order_acquire);
    OpenXRHeadPose head{},hands[2]{};uint64_t sequence{},stamp{};
    WheelTrackingFrame frame{};
    if((state&3) && OpenXRManager::Get().GetGestureHandFrame(&head,hands,&sequence,&stamp)) {
        frame.head={{head.oriX,head.oriY,head.oriZ,head.oriW},{head.posX,head.posY,head.posZ}};
        frame.sequence=sequence;frame.origin=head.originSerial;frame.stampUs=stamp;frame.headValid=head.valid;
        for(int i=0;i<2;++i) {
            const auto& hand=hands[1-i]; // tracking uses left/right, wheel uses right/left
            frame.hands[i]={hand.posX,hand.posY,hand.posZ};
            if(hand.valid)frame.validHands|=uint8_t(1u<<i);
        }
    }
    std::lock_guard lock(g_steerMutex);
    if(g_sampledGrabEpoch!=(grab>>2)){g_steering.Reset();g_sampledGrabEpoch=grab>>2;}
    const float out=g_steering.Update(uint8_t(state&grab&3),frame,XrDiagNowUs(),
        g_physicalRadius.load(std::memory_order_relaxed),
        WheelFullLockForVehicle(g_liveControls.xrWheelSteerMaxDeg,g_wheelMountedCar.load(std::memory_order_relaxed)),g_liveControls.xrWheelSteerDeadDeg,
        g_liveControls.xrWheelPrediction ? g_liveControls.xrWheelPredictionMs:0.f);
    g_wheelSteer.store(out,std::memory_order_relaxed);
    g_wheelSteerDeg.store(g_steering.Angle(),std::memory_order_relaxed);
    return out;
}
bool WheelSetGamepadProfile(float inner,float outer) {
    if(!std::isfinite(inner)||!std::isfinite(outer)||inner<0||inner>=.5f||outer>1||outer<=inner+.01f)return false;
    g_padProfile.store(std::bit_cast<uint64_t>(GamepadProfile{inner,outer}),std::memory_order_relaxed);
    return true;
}
float WheelGamepadSteer(float normalized) {
    const auto p=std::bit_cast<GamepadProfile>(g_padProfile.load(std::memory_order_relaxed));
    return WheelGamepadAxis(normalized,p.inner,p.outer);
}

// Blend the IK target toward the animated hand. At blend 0 this is a no-op; the caller skips the
// solve entirely once the blend is full, so this only ever runs on the way in and out.
void WheelBlendTarget(int hand, float* target, float* handRot) {
    if (hand < 0 || hand > 1 || !target || !handRot) return;
    const WheelHand& w = g_wheel[hand];
    if (w.blend <= 0.0f || !w.animValid) return;
    const float b = w.blend;
    target[0] += (w.animPos[0] - target[0]) * b;
    target[1] += (w.animPos[1] - target[1]) * b;
    target[2] += (w.animPos[2] - target[2]) * b;
    float from[4] = { handRot[0], handRot[1], handRot[2], handRot[3] };
    float to[4]   = { w.animRot[0], w.animRot[1], w.animRot[2], w.animRot[3] };
    if (from[0]*to[0] + from[1]*to[1] + from[2]*to[2] + from[3]*to[3] < 0.0f) {
        to[0] = -to[0]; to[1] = -to[1]; to[2] = -to[2]; to[3] = -to[3];
    }
    // nlerp: the two poses are close enough by construction (this only runs while the hand is
    // travelling the last few centimetres to the wheel) that slerp would buy nothing.
    handRot[0] = from[0] + (to[0] - from[0]) * b;
    handRot[1] = from[1] + (to[1] - from[1]) * b;
    handRot[2] = from[2] + (to[2] - from[2]) * b;
    handRot[3] = from[3] + (to[3] - from[3]) * b;
    VRIK_QuatNorm(handRot);
}

void WheelStoreTarget(int hand, const float* target) {
    if (hand < 0 || hand > 1 || !target) return;
    WheelHand& w = g_wheel[hand];
    w.target[0] = target[0]; w.target[1] = target[1]; w.target[2] = target[2];
    w.targetValid = true;
    w.targetFrame = g_wheelFrame;
}

// True once the arm is fully the animation's: no solve, no length scale, no cache entry.
bool WheelHandsOff(int hand) {
    return (hand >= 0 && hand <= 1) && g_wheel[hand].blend >= kFullBlend;
}

// ================================================================================================
// OPEN HANDS IN A CAR.
//
// VRIK owns the arm and the wrist and has never written a finger bone -- that is what makes the
// native grip on the wheel free when an arm is handed over. The other side of it is that the driving
// animation's CLOSED FISTS are on the avatar the whole time you are seated, wheel held or not: hands
// following your controllers around the cabin with the knuckles of someone still gripping a wheel
// that is not there.
//
// So the fingers need a pose of their own for the not-holding case, and the honest source for one is
// the player: latch the finger locals while ON FOOT with empty hands -- the game's own relaxed hand
// -- and replay them for whichever hand is not on the wheel. Nothing authored, nothing hardcoded per
// rig, and it tracks whatever the character's idle hand actually is.
// ================================================================================================
void WheelFingers(uint8_t* boneBuf) {
    if (!boneBuf) return;
    const bool inVehicle = g_isInVehicle;
    // A weapon is held with the fingers the weapon needs -- ours would open the hand around the grip,
    // and on foot it would be what we latched as "relaxed".
    const bool weapon = g_hasWeaponEquipped;

    for (int h = 0; h < 2; ++h) {
        const int   count = (h == 0) ? g_VRSmokeFingerCount : g_VRSmokeFingerCountL;
        const int*  idx   = (h == 0) ? g_VRSmokeFingerIdx   : g_VRSmokeFingerIdxL;
        if (count <= 0) continue;
        // Smoking owns this hand's fingers when it is holding something.
        const bool smoking = (h == 0) ? (g_VRSmokeFingerActive != 0) : (g_VRSmokeFingerActiveL != 0);
        const bool grip = (h == 0) ? (WheelSlot(49) > 0.5f)
                                   : (WheelSlot(vrshared::kLeftGripPressed) > 0.5f);

        if (!inVehicle) {
            // CAPTURE. Empty-handed and not squeezing -- a squeezed grip is usually a fist, and
            // latching that would put it back on in the car.
            if (!weapon && !smoking && !grip) {
                for (int k = 0; k < count && k < 32; ++k) {
                    const int bi = idx[k];
                    if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
                    const float* q =
                        reinterpret_cast<const float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
                    g_relaxFingerRot[h][k][0] = q[0]; g_relaxFingerRot[h][k][1] = q[1];
                    g_relaxFingerRot[h][k][2] = q[2]; g_relaxFingerRot[h][k][3] = q[3];
                }
                g_relaxFingerHave[h] = true;
            }
            continue;
        }

        // APPLY. Seated, this hand not (mostly) on the wheel, nothing else claiming the fingers. The
        // switch is at half the grab blend: past that the hand is close enough to the wheel that the
        // animation's grip is the pose you want to see closing around it.
        if (!weapon && !smoking && g_relaxFingerHave[h] && g_wheel[h].blend < 0.5f) {
            for (int k = 0; k < count && k < 32; ++k) {
                const int bi = idx[k];
                if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
                float* q = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
                q[0] = g_relaxFingerRot[h][k][0]; q[1] = g_relaxFingerRot[h][k][1];
                q[2] = g_relaxFingerRot[h][k][2]; q[3] = g_relaxFingerRot[h][k][3];
            }
        }
    }
}

void WheelReset() {
    std::lock_guard lock(g_steerMutex);
    g_steering.Reset();g_wheelFrame=0;
    const auto epoch=(g_grabState.load(std::memory_order_relaxed)>>2)+1;
    g_grabState.store(epoch<<2,std::memory_order_release);g_wheelStampMs.store(0,std::memory_order_relaxed);
    for (int h = 0; h < 2; ++h) {
        g_wheel[h].blend = 0.0f;
        g_wheel[h].engaged = false;
        g_wheel[h].atWheel = false;
        g_wheel[h].atHub = false;
        g_wheel[h].gripPrev = false;
        g_wheel[h].targetValid = false;
        g_wheel[h].animValid = false;
        g_wheel[h].targetFrame = 0;
    }
    g_wheelCenterValid = false;
    g_wheelSpan = 0.0f;
    g_wheelBlendRight.store(0.0f, std::memory_order_relaxed);
    g_wheelBlendLeft.store(0.0f, std::memory_order_relaxed);
    g_wheelSteer.store(0.0f, std::memory_order_relaxed);
    g_wheelSteerDeg.store(0.0f, std::memory_order_relaxed);
    g_wheelHornMask.store(0, std::memory_order_relaxed);   // or the horn sounds until the game exits
    if (g_pSharedHands) g_pSharedHands[vrshared::kWheelArmedMask] = 0.0f;
}

}  // namespace cvr::anim
