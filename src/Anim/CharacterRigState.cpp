#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// The globals of the player's solve that the rest of the plugin shares -- the FK scratch, the hand stop and wrist
// lock, the solve cache, the body probes and the live controls. They were defined in src/Anim/CharacterRig.cpp; they
// live here so that both implementations of the solve use the same objects: CharacterRig.cpp in the C++ build, the
// Rust port (rust/crates/cvr-rig/src/vrik, CVR_RUST_VRIK) otherwise. All have C linkage (Anim/CharacterRig.hpp,
// Anim/VrikState.hpp, Anim/Cyberware.hpp); the comments on what they mean are at their declarations and in the solve.
#if !CVR_RUST_VRIK  // the Rust build's are rust/crates/cvr-rig/src/vrik/shared_globals.rs
#include "Anim/CharacterRig.hpp"
#include "Anim/Cyberware.hpp"
#include "Anim/WheelGrab.hpp"
#include "Anim/Recoil.hpp"
#include "Anim/GraspPose.hpp"
#include "Anim/TwoHandGrip.hpp"

#include <atomic>
#include <cmath>
#include <cstdint>

extern "C" {
bool g_VRUpperOwned[VRIK_MAX_BONES]{};
int g_VRShadowSource[VRIK_MAX_BONES]{};
bool g_VRHeadReferenceValid=false;
bool g_VRIKVehicleUpperActive=false;
float g_VRHeadReferenceModelRot[4]={0,0,0,1};
bool g_solveCacheModel=false;
float g_VRIKBodyBendAngle=0;
float g_VRIKBodyBendPelvis[3]{};
float g_VRIKBodyEyeTarget[3]{},g_VRIKBodyEyeSolved[3]{};
bool g_VRIKBodyEyeBound=false;
float g_VRIKGirdleReach[2][4]{}; // weight, clavicle degrees, joint advance(m), scapula travel(m)

// GRIP REACH (see VRIK_ReachGirdle and VRIK_GripTorso in the solve).
__declspec(dllexport) int   CyberpunkVR_GripReach = 1;
// A leg solved onto the animation's foot (squat, crouch, bend): its knee over the toes (1), in the animation's own leg
// plane (0, which a crouch splays 30-45 degrees out), or between the two (2).
__declspec(dllexport) int   CyberpunkVR_SquatKneeMode = 1;
// 1: a squat's feet stand about OUR hips as the animation's stand about its hips; 0: where the animation has them.
__declspec(dllexport) int   CyberpunkVR_SquatFeetUnderHips = 1;
// 1: a squat stands symmetric about the hips; 0: the animation's own stance.
__declspec(dllexport) int   CyberpunkVR_SquatSymmetric = 1;
// Debug: the animation's feet moving against its hips (m/s, the stance gate) and the stance's weight.
__declspec(dllexport) float CyberpunkVR_DebugSquatStance[2] = {};
// Debug: the toes (model space), [0] right, [1] left, as VRIK_PublishBodyBones left them.
__declspec(dllexport) float CyberpunkVR_DebugToes[2][3] = {};
__declspec(dllexport) float CyberpunkVR_GripProtract = 0.085f;      // m of protraction at full reach
__declspec(dllexport) float CyberpunkVR_GripTorsoMaxDeg = 20.0f;     // the thorax turn's limit
// R, L: how far the wrist fell short of its target after the caps, mm (negative = slack); the turn, degrees
__declspec(dllexport) float CyberpunkVR_DebugGripShortMm[2] = {0.0f, 0.0f};
__declspec(dllexport) float CyberpunkVR_DebugGripTorsoDeg = 0.0f;
// the left side: how far the turn carried the shoulder's anchor (mm); the girdle's weight, clavicle degrees and
// joint advance (mm); the reach as a fraction of the arm; 1 while that girdle ran in its grip profile
__declspec(dllexport) float CyberpunkVR_DebugGripReach[6] = {0,0,0,0,0,0};
// THE BODY OF A SLIDE, A JUMP, A FALL AND A LANDING FROM THEIR CLIP. 1 = through those moves the hips and spine are the
// clip's own; 0 = the standing body throughout.
__declspec(dllexport) int   CyberpunkVR_ClipBodyMoves = 1;
// the view (body base), the clip's camera bone and hips (model space), the clip's weight, the detailed locomotion state,
// and 1 when the torso came from a buffer that was our own output
__declspec(dllexport) float CyberpunkVR_DebugClipBody[12] = {};
__declspec(dllexport) int CyberpunkVR_NativeCyberwarePasses=1;

float g_VRIKHandPin[2][3]{};      // R, L: where the solve put the wrist, model space
float g_VRIKHandAim[2][8]{};      // R, L: the target the controller asks for (position, rotation, valid), model space
bool  g_VRIKTorsoOn=false;
int g_VRCyberwareFamily=0;

// Model-space FK scratch (recomputed each matched frame).
float g_fkPos[VRIK_MAX_BONES][3];
float g_fkRot[VRIK_MAX_BONES][4];
// FK SNAPSHOT -- the measurement channel for capsule geometry (filled only while diag capture is on; count last).
volatile float g_VRFKSnapPos[VRIK_MAX_BONES][3];
volatile float g_VRFKSnapRot[VRIK_MAX_BONES][4];
volatile int   g_VRFKSnapCount = 0;

// HAND STOP, WRIST LOCK and the raw tracked hand (VRIK_ApplyHandStop). Index 0 is LEFT, 1 is RIGHT.
volatile float g_VRHandStopModel[2][3] = { {0.0f,0.0f,0.0f}, {0.0f,0.0f,0.0f} };
volatile int   g_VRHandStopValid[2]    = { 0, 0 };
volatile float g_VRHandStopDeadband = 0.02f;
volatile float g_VRHandRotLock[2][4] = { {0.0f,0.0f,0.0f,1.0f}, {0.0f,0.0f,0.0f,1.0f} };
volatile int   g_VRHandRotLockOn[2]  = { 0, 0 };
volatile float g_VRHandRawModel[2][3] = { {0.0f,0.0f,0.0f}, {0.0f,0.0f,0.0f} };
volatile int   g_VRHandRawValid[2]    = { 0, 0 };
volatile float g_VRHandRawRot[2][4]   = { {0.0f,0.0f,0.0f,1.0f}, {0.0f,0.0f,0.0f,1.0f} };

// The clock the bend limit fades by (microseconds); tests set it, null = the steady clock.
int64_t (*g_VRBendLimitClockUs)()=nullptr;

// Solve cache (one solve per frame + replay), keyed by the DLL's own frame epoch.
uint32_t g_solveCacheTick = 0xFFFFFFFFu;
std::atomic<uint32_t> g_VrikFrameEpoch{0};
int   g_VrikOverwritePassHist[8] = {0,0,0,0,0,0,0,0};
int   g_VrikBatchGapNew = 0;
int   g_VrikBatchGapSame = 0;
float g_VrikBatchGapNewMin = 0.0f;
float g_VrikBatchGapSameMax = 0.0f;
float g_VrikEngineOverwriteMm = 0.0f;
int   g_VrikEngineOverwriteBone = -1;
int   g_VrikEngineOverwriteHits = 0;
int   g_VrikEngineOverwritePasses = 0;
float g_VrikShakePeakMm[2][4] = {{0.0f,0.0f,0.0f,0.0f},{0.0f,0.0f,0.0f,0.0f}};   // any motion
float g_VrikShakeSlowMm[2][4] = {{0.0f,0.0f,0.0f,0.0f},{0.0f,0.0f,0.0f,0.0f}};   // slow motion only
int   g_VrikShakeSlowN[2][4]  = {{0,0,0,0},{0,0,0,0}};
int   g_solveCacheN = 0;
int   g_solveCacheIdx[VRIK_MAX_BONES];
float g_solveCacheVal[VRIK_MAX_BONES][7];
float g_solveCacheYaw = 0.0f;      // heading the cached solve was built with
float g_solveCacheSnapCtr = -1.0f; // snap event counter [147] the cached solve consumed

// The shared hand block's latched frame and the view packet (src/Anim/HandSnapshot.cpp or its Rust port).
uint32_t g_handsStableSeq   = 0;
bool     g_handsStableValid = false;
float g_viewPkt[17] = { 0,0,0,1, 0,0,0, 0, 0, 0, 0,0,0, 0,0,0,1 }; // +[13..16]=head ori the view was built with
bool  g_viewPktValid = false;
float g_vrikViewScaleUsed = 0.0f;   // diag: 0=rejected/fallback, 2=delta-v2, 1/0.5=legacy abs
// The BODY's smoothed squat (deadzone+EMA), published by VRIK_PlaceBodyUnderHMD and reused by the arm's anchor.
float s_vrSharedSquatDrop = 0.0f;

// The finger layers' switches and probes (src/Anim/ReloadPose.cpp or its Rust port): the resting left and right hands,
// the reload's fingers, the right hand's carried grip. Their meaning is documented where they are used.
__declspec(dllexport) int CyberpunkVR_DebugRestFingerHave = 0;
__declspec(dllexport) unsigned long long CyberpunkVR_DebugRestFingerCaps = 0;
__declspec(dllexport) int CyberpunkVR_DebugRestFingerRefused = 0;
__declspec(dllexport) int CyberpunkVR_DebugRestFingerSaved = 0;
__declspec(dllexport) int CyberpunkVR_DebugRestFingerLoadTried = 0;
__declspec(dllexport) int CyberpunkVR_DebugRestFingerLoaded = 0;
__declspec(dllexport) int CyberpunkVR_RestFingerCaptureReq = 0;
__declspec(dllexport) int CyberpunkVR_RestFingerSaveReq = 0;
__declspec(dllexport) unsigned long long CyberpunkVR_DebugRestApplyCalls = 0;
__declspec(dllexport) int CyberpunkVR_DebugRestBones = 0;
__declspec(dllexport) int CyberpunkVR_DebugRestGate = 0;
__declspec(dllexport) int CyberpunkVR_RestFingerForce = 0;
__declspec(dllexport) int CyberpunkVR_RestFingerCaptureReqR = 0;
__declspec(dllexport) int CyberpunkVR_RestFingerSaveReqR = 0;
__declspec(dllexport) int CyberpunkVR_RestFingerApplyR = 1;
__declspec(dllexport) int CyberpunkVR_RestFingerForceR = 0;
__declspec(dllexport) int CyberpunkVR_DebugRestFingerHaveR = 0;
__declspec(dllexport) int CyberpunkVR_DebugRestFingerSavedR = 0;
__declspec(dllexport) int CyberpunkVR_DebugRestFingerRefusedR = 0;
__declspec(dllexport) int CyberpunkVR_DebugRestFingerCapsR = 0;
__declspec(dllexport) int CyberpunkVR_DebugRestBonesR = 0;
__declspec(dllexport) int CyberpunkVR_DebugRestGateR = 0;
__declspec(dllexport) int CyberpunkVR_DebugRestFingerLoadTriedR = 0;
__declspec(dllexport) int CyberpunkVR_CarryGripApply = 1;
__declspec(dllexport) int CyberpunkVR_DebugCarryGripHave = 0;
__declspec(dllexport) int CyberpunkVR_DebugCarryGripSnaps = 0;
__declspec(dllexport) int CyberpunkVR_DebugCarryGripBones = 0;

// The finger gestures (src/Anim/HandGestures.cpp or its Rust port): each hand's gesture and digits, and the gloves.
__declspec(dllexport) int CyberpunkVR_GestureMode[2]{};
__declspec(dllexport) float CyberpunkVR_GestureClosed[2][5]{};
__declspec(dllexport) float CyberpunkVR_GestureStraight[2][5]{};
// 1 = finger tracking (xr_finger_bones): each of V's fingers follows its own from OpenXR hand tracking. Hand tracking
// itself is enabled when the game starts with it on (Runtimes/FingerGrip.cpp); turned off, the fingers go back live.
__declspec(dllexport) int CyberpunkVR_FingerBones=0;
// Finger spread from hand tracking (xr_finger_spread): 0 = off (fingers only curl), 1 = default, up to 2.
__declspec(dllexport) float CyberpunkVR_FingerSpread=1.0f;
// 1 = the fingers also spread sideways (xr_finger_spread_enabled); 0 = they only bend, from the curl.
__declspec(dllexport) int CyberpunkVR_FingerSpreadOn=1;

// The weapon hand's layers (src/Anim/LeftHanded.cpp and AdsMuzzleStabilizer.cpp, or their Rust port): the mirror's
// twins and worst residual, each hand's mean finger curl ([0..1] as animated, [2..3] as drawn), and the non-VRIK ADS
// muzzle stabilizer's switch and counters.
__declspec(dllexport) int   CyberpunkVR_DebugLeftHandedTwins = 0;
__declspec(dllexport) float CyberpunkVR_DebugLeftHandedResidual = 0.0f;
__declspec(dllexport) float CyberpunkVR_DebugFingerFlex[4] = {};
__declspec(dllexport) int32_t  CyberpunkVR_NonVrikAdsStabilizer = 1;
__declspec(dllexport) uint64_t CyberpunkVR_DebugAdsStabApplies = 0;
__declspec(dllexport) uint64_t CyberpunkVR_DebugAdsStabHipCaptures = 0;

// The native reload played by both arms (src/Anim/ReloadHandOff.cpp or its Rust port).
// 1 = both arms follow the native reload animation; 0 = the controllers keep them.
__declspec(dllexport) int   CyberpunkVR_VrikReloadHandOff = 1;
// Seconds to reach the animation, and to come back to the controllers.
__declspec(dllexport) float CyberpunkVR_ReloadHandOffInSec  = 0.20f;
__declspec(dllexport) float CyberpunkVR_ReloadHandOffOutSec = 0.30f;
// How long before the reload can end the hands start back.
__declspec(dllexport) float CyberpunkVR_ReloadHandOffLeadSec = 0.15f;
// [0] weight, [1] wanted (0/1), [2] seconds in the reload, [3] seconds it was expected to take (-1 unknown),
// [4..6] the animated right hand relative to the chest (m), [7] reloads entered.
__declspec(dllexport) float CyberpunkVR_ReloadHandOffStats[8] = {};

// The hands' layers' counter (VRIK_ClockQpc): null = QueryPerformanceCounter.
int64_t (*g_VRIKClockQpc)()=nullptr;

// The hands' layers' clock (VRIK_ClockMs): null = GetTickCount64.
uint64_t (*g_VRIKClockMs)()=nullptr;

// The steering wheel's clock (XrDiagNowUs) and tracking sample (GetGestureHandFrame): null = the real ones.
uint64_t (*g_VRIKClockUs)()=nullptr;
bool (*g_VRIKWheelFrame)(cvr::anim::WheelTrackingFrame*)=nullptr;
}

// Hands on the wheel (src/Anim/WheelGrab.cpp, or rust/crates/cvr-rig/src/vrik/wheel_grab.rs): what the XInput detour
// and the overlay read.
namespace cvr::anim {
extern "C" {
std::atomic<float> g_wheelBlendRight{0.0f};
std::atomic<float> g_wheelBlendLeft{0.0f};
std::atomic<float> g_wheelSteer{0.0f};
std::atomic<float> g_wheelSteerDeg{0.0f};
std::atomic<int>   g_wheelHornMask{0};
}
}  // namespace cvr::anim

// ================================================================================================
// HAND RECOIL (src/Anim/Recoil.cpp, or rust/crates/cvr-rig/src/vrik/recoil.rs): its controls and read-outs.
// ================================================================================================
// ---- controls -----------------------------------------------------------------------------------
//
// Four numbers, and each one is a thing you can feel: how far the hand is thrown back, how far the
// muzzle rises, how long it takes to come home, and how much of it the second hand takes.
extern "C" __declspec(dllexport) int   CyberpunkVR_HandRecoil          = 1;
// ZERO BY DEFAULT, and that is the correction that matters.
//
// Throwing the wrist backwards is what a free-floating gun does, not what a held one does: the
// player's hand is on a controller and does not move, so pulling the IK target off it breaks the one
// rule this whole rig is built on -- the hand IS the controller. Reported from the headset as "it
// jerks the arm somehow, and that is wrong, my hand is steady", which is exactly what a translated
// wrist looks like from the inside.
//
// What a held weapon actually does with the impulse is ROTATE: the muzzle flips up about the wrist
// while the hand stays where it is being held. That is the whole of it below, and the weapon follows
// because it is parented to the hand bone. The travel stays as a slider for anyone who wants a looser
// grip, and it is the first thing to try if the rotation alone reads as too light.
//
// AND THE KICK GOES INTO THE SHOULDER, the hand jerked slightly back. So the
// travel is on now, deliberately small. The direction needs no new geometry -- minus the hand's own
// forward axis, on an arm holding a gun out, points back INTO the shoulder, which is where a real
// recoil goes. The old warning above stands: this pulls the IK target off the controller, so the
// number is a couple of centimetres rather than the several a free-floating gun would take, and it is
// scaled per weapon and damped by a second hand exactly like the rise is.
// 3 cm, up from 2 now that the travel is purely backwards: a backward slide runs along the line of
// sight and foreshortens, so it reads weaker than a dip of the same size. An ini key
// (xr_recoil_back_cm) with its ceiling, because it is the number this feature is judged by.
extern "C" __declspec(dllexport) float CyberpunkVR_HandRecoilBackCm    = 3.0f;   // backwards, in the body
// The ceiling for that travel, on the same argument as the angle's: a heavy weapon should shove the
// hand further than a light one, but not far enough to look like the arm was yanked.
extern "C" __declspec(dllexport) float CyberpunkVR_HandRecoilBackMaxCm = 4.0f;
// THE PEAK ANGLE OF ONE SHOT, and it is computed rather than chosen -- 22 deg is what a 9 mm pistol
// held at the wrist actually produces:
//
//     J  = m_b*v_b + m_powder*v_gas = 0.008*360 + 0.00033*540      = 3.06 N s
//     L  = J * h,  h = 0.07 m from the bore axis down to the wrist = 0.214 N m s
//     I  = 1.0*0.08^2 + 0.004 + 0.4*0.05^2                         = 0.0114 kg m^2
//     w0 = L / I                                                   = 18.8 rad/s
//     peak = 0.523 * w0 / w_spring,  w_spring = 2*pi/0.25 s        = 0.39 rad = 22 deg
//
// AN EARLIER VERSION OF THIS COMMENT SAID 7.8 deg AND WAS WRONG: it used h = 0.03 m, which is the
// bore-to-top-of-hand distance, not the distance to the joint the flip actually turns about. The
// error made the recoil invisible in the headset, and the fix for that was briefly a taste number
// (15 deg) -- which is exactly the kind of thing this file is not supposed to contain. The moment arm
// is the physical quantity; the angle follows from it.
//
// This is also the number the per-weapon table will write: J and I are the only things that change
// between a Kenshin and an Overture, so each weapon gets its own peak from the same formula.
// 15.4 deg: the computed 22 taken down by 30% after firing every weapon in the
// set. The physics above still holds -- it says what a 9 mm does to a free wrist -- and a hand that
// is braced for the shot, which a player's is, gives less than a free one.
// TUNED DOWN 20% IN THE HEADSET, in two steps and this is the second one. The derivation above is
// left exactly as it was written -- it says where 15.4 came from and it is still right. Half (7.7)
// was tried first and read as too short a throw, so the reduction settled at a fifth.
extern "C" __declspec(dllexport) float CyberpunkVR_HandRecoilRiseDeg   = 12.3f;  // 9 mm reference, -20%
// 180 ms, DOWN FROM 250: a hand holding a gun snaps back, it does not sink. The settle is a property
// of the wrist -- a spring of roughly fixed stiffness -- while the cartridge decides how FAR the muzzle
// goes, which is the per-weapon angle above. An ini key (xr_recoil_return_ms), because this and the
// exponent below are the two numbers that decide how the recoil feels.
extern "C" __declspec(dllexport) float CyberpunkVR_HandRecoilReturnMs  = 180.0f; // full settle
// THE OFF HAND'S SHARE IS GONE, and its absence is the answer rather than an oversight. It said how much
// of the kick a second hand takes when it is also on the weapon -- but a hand ON the weapon now rides the
// weapon: the support point is built from the already-kicked right hand, so the left one inherits exactly
// the motion the gun makes, about the gun's axis rather than its own. Sampling the spring for it again
// was the double kick that tore the hand off the grip. And a hand that is NOT on the weapon feels nothing
// at all. That leaves no case for a fraction, so there is no fraction.

// THE EQUIPPED WEAPON'S OWN KICK, in the game's degrees, published on each draw by the weapon module
// (SetVRWeaponKick). Zero means "unknown weapon" and the reference angle is used unscaled.
//
// The reference is the Lexington at 1.0, because 22 deg on it is the number that was tried in the
// headset and kept. Everything else follows its own ratio: an Overture at 4.0 flips four times as far,
// a Kappa at 0.24 barely moves -- which is the impulse ratio, not a taste ladder.
extern "C" __declspec(dllexport) float CyberpunkVR_WeaponKickDeg = 0.0f;
// THE PER-WEAPON KICK, SCALED. Tuned in the headset alongside the spring above: 0.7 with a halved
// spring was too little kick and too short a throw, so both settled at a fifth off. Kept as a
// separate multiplier rather than folded into the per-weapon table so the ORDERING that table encodes
// -- a .44 kicking four times a 9 mm -- stays visible and stays measured from the game's own
// RecoilKickMax.
// BACK TO 1.0. The 20% came off twice: once here and once in the reference angle
// above (15.4 -> 12.3), so a pistol was flipping at 0.64 of the number that had been tuned in the
// headset and read as weak. The angle keeps its reduction; this multiplier gives back the other one.
extern "C" __declspec(dllexport) float CyberpunkVR_WeaponKickScale = 1.0f;
// WHICH WEAPON IS THIS, as the record itself says: 0 unknown, 1 handgun/revolver, 2 rifle-ish
// (SMG, assault, MG), 3 shotgun, 4 sniper/precision rifle. Published per draw by the weapon module,
// which already reads the kick out of the same record.
extern "C" __declspec(dllexport) int   CyberpunkVR_WeaponClass = 0;
// THE LEFT GUN'S, while the game reports a gun in each hand (SetVRLeftWeaponKick, published by the weapon
// module the same way). Zero = not published, and the left hand then kicks like the right one's weapon.
extern "C" __declspec(dllexport) float CyberpunkVR_WeaponKickDegLeft = 0.0f;
extern "C" __declspec(dllexport) int   CyberpunkVR_WeaponClassLeft = 0;
// HOW THE IMPULSE IS SPLIT, per class -- and the split IS the realism, not the size. One shot puts the
// same momentum into the shooter either way; where it goes depends on how the thing is held. A rifle is
// against a shoulder with a hand well out on the handguard: the muzzle barely lifts and the shove goes
// straight back. A pistol is out on an arm with a 7 cm moment arm: it flips. A shotgun does both, hard.
//
// Multipliers rather than three tables, so the per-weapon ladder read out of the game's own
// RecoilKickMax still orders everything inside a class.
// WHICH WAY THE HAND IS THROWN -- and it is not purely backwards, which is the correction here.
//
// The travel was minus the hand's forward axis, straight back along the pointing direction, and what it
// reads as in the headset is DOWN. The physical picture says the observation is the right one: the
// muzzle flips UP about the wrist, so the GRIP -- below the bore, inside the hand -- dips. A pistol's
// visible answer to a shot is the butt dropping in the palm, not the fist sliding backwards. A
// shouldered weapon is the other way round: the shove goes into the shoulder and the grip barely moves.
//
// 0 = all backwards, 1 = all down. Per class, and ini keys, because this is a feel number.
//
// ALL THREE ARE ZERO, tried in the headset and rejected: a kick that only
// jerks downwards is wrong, it has to go back. The dip stays as a knob because the argument above is still true of a real
// pistol; what it is not is what this port should be doing to the hand. Back is what a shot reads as.
extern "C" __declspec(dllexport) float CyberpunkVR_RecoilDownPistol  = 0.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_RecoilDownRifle   = 0.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_RecoilDownShotgun = 0.0f;
// A PISTOL'S THROW IS THE SMALLEST OF THE FOUR. Its energy goes into the flip -- which is at full
// strength -- and not into pushing the hand back: there is no shoulder behind it and no mass to drive.
// A minimal jerk back. Rise is untouched: the reference IS the pistol.
extern "C" __declspec(dllexport) float CyberpunkVR_PistolBackMul   = 0.30f;
extern "C" __declspec(dllexport) float CyberpunkVR_RifleRiseMul    = 0.35f;
extern "C" __declspec(dllexport) float CyberpunkVR_RifleBackMul    = 2.00f;
extern "C" __declspec(dllexport) float CyberpunkVR_ShotgunRiseMul  = 1.00f;
extern "C" __declspec(dllexport) float CyberpunkVR_ShotgunBackMul  = 2.00f;
// SNIPERS ARE NOT RIFLES, as far as the hand is concerned: the one shouldered weapon whose impulse is
// big enough to move the shooter, so it takes MORE flip than the pistol reference rather than less, and
// by far the longest throw. Both numbers were set by feel.
extern "C" __declspec(dllexport) float CyberpunkVR_SniperRiseMul   = 2.00f;
extern "C" __declspec(dllexport) float CyberpunkVR_SniperBackMul   = 5.00f;
// ALL OF IT, 30% LESS (2026-10-16): one factor over the finished angle and throw of every weapon,
// applied after the ceilings so the heavy weapons that sit on the cap lose the same share as the rest.
// The per-weapon and per-class ladders above are untouched. ini: xr_recoil_scale.
extern "C" __declspec(dllexport) float CyberpunkVR_RecoilScale     = 0.70f;
// WHAT A SECOND HAND LEAVES OF THE BACKWARD THROW -- and it is deliberately NOT the angle's factor.
//
// Sharing that factor (0.286) is what this session already got wrong: it cut a rifle's shoulder shove
// to a fifth on exactly the weapon that is always held with two hands, and the shove is most of what a
// rifle should feel like. Two hands and a braced arm do reduce how far the shooter is moved -- a second
// arm's mass is real -- but they reduce the FLIP far more, because that is leverage against a moment
// arm and this is only mass against an impulse. 0.6 by feel, and an ini key
// (xr_recoil_twohand_back), because the previous round proved one number cannot serve both.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandBackMul = 0.60f;
extern "C" __declspec(dllexport) float CyberpunkVR_HandRecoilRefKick   = 1.0f;   // the Lexington
extern "C" __declspec(dllexport) float CyberpunkVR_HandRecoilMaxDeg    = 28.0f;  // a hand has limits
// HOW LONG THE HEAVY ONES TAKE TO COME BACK. The settle time of a hand holding a weapon is the period
// of its own spring, 2*pi/sqrt(k/I) -- it grows with the inertia, so a magnum has to come home slower
// than a smart pistol. The weapon's kick stands in for that inertia, mildly (a power of 0.35), which
// spreads a 250 ms reference across roughly 180..400 ms.
//
// THE GAME'S OWN RecoilRecoveryTime WAS READ FIRST AND REJECTED, which is worth writing down: it is a
// gameplay pacing number, not a physical one, and it runs the wrong way -- Kenshin 0.08 s, Kappa 0.30,
// Overture 0.15, Liberty up to 0.80. Taking it would have made the heaviest revolver settle faster
// than the lightest smart pistol.
// 0.15 -- NEARLY FLAT, and that is the correction that matters here. At 0.60 an Overture computed
// 575 ms and sat on the ceiling, so the heaviest weapons were the SLOWEST to come home: a wallow, when
// what a .50 does to a wrist is a crack. Seen against Bodycam (heavy pistols such as a Deagle
// kick very sharply).
//
// A trace of dependence is kept rather than none, because a heavier weapon really does stretch the
// hand's spring a little: Kappa 0.24 -> 145 ms, Lexington 1.0 -> 180, Overture 4.0 -> 221. The WEIGHT
// of the shot lives in the angle, which is already per-weapon and knee-compressed near the cap.
extern "C" __declspec(dllexport) float CyberpunkVR_HandRecoilReturnPow  = 0.15f;
extern "C" __declspec(dllexport) float CyberpunkVR_HandRecoilReturnMinMs = 80.0f;
// MUZZLE CLIMB ACROSS A BURST -- the piece a per-shot spring cannot express.
//
// One spring that returns to zero makes a burst N identical kicks around one point of aim. A real burst
// WALKS the muzzle up and comes back only part of the way, slowly: the shooter is fighting an
// accumulation, not a sequence of taps. So each shot adds a fraction of its own peak angle to a climb
// term that decays with a long time constant, on top of the fast spring.
//
// First order, not a spring: a climb settles, it does not bounce. Capped, because a held trigger must
// not be able to bend the wrist past what the model says a wrist does.
// OFF BY DEFAULT, AND THE REASON IS VR RATHER THAN BALLISTICS. Tried in the headset and rejected at
// once: the slow settle at the end looks wrong and unrealistic.
//
// In a flat shooter the slow walk back down is the sight picture recovering, and it is the game's job.
// Here the weapon is the player's HANDS: they never left, they are steady, and a drift that eases the
// muzzle down over most of a second is the game moving them -- the one thing this whole rig refuses to
// do everywhere else. Accumulation across a burst is still there and still physical, but it lives where
// it belongs: the fast spring takes a velocity impulse per round, so held fire stacks by construction
// and stops the instant the shooting does.
//
// The knob stays, with a short time constant, for anyone who wants a trace of walk-up: at 250 ms it is
// part of the kick rather than a separate drift.
extern "C" __declspec(dllexport) float CyberpunkVR_RecoilClimbFrac   = 0.0f;    // of one shot's peak
extern "C" __declspec(dllexport) float CyberpunkVR_RecoilClimbMaxDeg = 8.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_RecoilClimbMs     = 250.0f;  // time constant of the walk-back
extern "C" __declspec(dllexport) float CyberpunkVR_DebugRecoilClimbDeg = 0.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_HandRecoilReturnMaxMs = 420.0f;
// 40, set from the headset: the ratio keeps a revolver clearly heavier than a 9 mm, and past this
// the wrist reads as broken rather than kicked. It bites on the top four -- Silverhand, Liberty,
// Unity, Omaha, Nue and the Overture all land here, so those six are told apart by their recovery
// rather than by their peak.
// Readable live: how many shots the pose path has answered, and the current displacement.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugRecoilShots = 0;
extern "C" __declspec(dllexport) float    CyberpunkVR_DebugRecoilCm    = 0.0f;
// Advances every fresh solve. Zero here means the pose path never reached the spring at all.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugRecoilTicks = 0;
// What the SOLVE actually applied to the right hand this frame, degrees of muzzle rise. The spring
// can be perfect and still reach nothing -- everything between the sample and the arm solve is
// downstream, and this is the only number that sees it.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugRecoilAppliedDeg = 0.0f;
// Max-hold of the same number; write 0 to it to re-arm.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugRecoilPeakDeg = 0.0f;
// ...and the left hand's, which only ever moves while two guns are in hand.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugRecoilPeakDegLeft = 0.0f;

// The grasp's read-outs (src/Anim/GraspPose.cpp, or rust/crates/cvr-rig/src/vrik/grasp.rs).
extern "C" {
// Solves, passes that wrote a grasp, palms measured, segments that touched in the last solve.
__declspec(dllexport) volatile uint64_t CyberpunkVR_DebugGrasp[4] = {};
// Per side, the last solve: its kind (0 flat, 1 wrap, 2 ball, 3 pinch, 4 edge), the thumb's base lift and swing and its two
// joints, then index..pinky's three joints each, in degrees.
__declspec(dllexport) volatile float CyberpunkVR_DebugGraspAngles[2][17] = {};
// Per side, the palm in the slot's frame: skin centre, normal (out of the palm), along the fingers, toward the thumb.
__declspec(dllexport) volatile float CyberpunkVR_DebugGraspPalm[2][12] = {};
// Per side: 1 when the side the fingers close to and the hand's handedness disagree about the palm's normal.
__declspec(dllexport) volatile int CyberpunkVR_DebugGraspNormalFlip[2] = {};
// Per side, the thumb's search: its cost, the overlap (mm), the pad's gap to the thing (mm), the pad from its goal (mm).
__declspec(dllexport) volatile float CyberpunkVR_DebugGraspThumb[2][4] = {};
}

// ================================================================================================
// THE SUPPORT HAND ON THE WEAPON (src/Anim/TwoHandGrip.cpp, or rust/crates/cvr-rig/src/vrik/two_hand.rs): its switches,
// its live state, and what the natives and the weapon module set or read.
// ================================================================================================
// ---- the switches, and every one of them is a fact rather than a taste ----
extern "C" __declspec(dllexport) int   CyberpunkVR_TwoHandGrip    = 1;      // the feature
// 1 = the support-hand weld is OFF. Raised by the weapon module while the weapon is carried in the
// LEFT hand: the gun is parented to that hand then, so there is no support hand to weld -- and, more
// to the point, the weld has to be gone BEFORE the carry measures the bone it attaches to, or the
// wrist unbends afterwards and takes the weapon with it.
// A MASK, bit 0 being that. Bit k+1 takes away only the weapon's k-th hold (see g_cand): the reload module
// raises it for a hold on a magazine that is not in the gun -- with no magazine there is
// nothing to hold on to -- while the weapon's other holds stay on offer.
extern "C" __declspec(dllexport) int   CyberpunkVR_TwoHandSuppress = 0;
// Which of the weapon's holds is the one in use (index into g_cand), -1 = none; for tests and the log.
extern "C" __declspec(dllexport) int   CyberpunkVR_DebugTwoHandHoldIdx = -1;
// 1 = the weapon is being carried in the LEFT hand (the weapon module says so, on the frame it moves
// the item into the left slot).
extern "C" __declspec(dllexport) int   CyberpunkVR_CarryLeft      = 0;
// How long the wrist takes to come out of the welded hold and back to the controller, milliseconds.
// The weapon is attached to that wrist, so this is also how long the gun takes to settle -- 200 ms is
// the same order as the recoil settle and the finger-preview ramp.
extern "C" __declspec(dllexport) float CyberpunkVR_CarryBlendMs   = 320.0f;
// The damping of that blend. Under 1 it overshoots slightly before settling, and that overshoot is
// what makes a heavy thing feel heavy -- the same argument and the same shape as the hand recoil's
// spring, which runs at 0.55. A little stiffer here: this is a hand-over, not a kick.
extern "C" __declspec(dllexport) float CyberpunkVR_CarryBlendZeta = 0.65f;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugCarryBlend = 0.0f;
// How near the support point the hand has to be for the fingers to offer the hold. It shipped at 12 cm
// on the argument that that is the reach of a hand which means it -- and in play that was too wide: the
// offer appeared for a hand merely passing the weapon, which is the failure the same note predicted for
// a larger value. 6 cm by feel. It is an ini key now (xr_two_hand_radius) because this is a
// number to settle by feel, and settling it by feel must not cost a rebuild per attempt.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandRadius  = 0.06f;
// What a second hand takes off the kick. Two hands roughly triple the effective mass resisting the same
// impulse and add a second lever against the muzzle rise, so a fifth of the one-handed flip is what the
// physics of it says.
// 0.286, not 0.2, and the change is arithmetic rather than taste: the one-handed angle came down by
// 30% and the two-handed result was to stay where it was, so the fraction goes up by the same 30%.
// 0.2 / 0.7 = 0.286, and a two-handed Lexington still peaks at the 4.4 deg it did before.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandRecoil  = 0.286f;
// How fast the finger preview fades in and out, seconds. Matches the reload module's own ramp so the two
// previews feel like one system.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandFadeS   = 0.15f;
// HOW LONG THE HOLD TAKES TO ARRIVE AND TO LEAVE, milliseconds, and how it settles. The hand and the
// aim used to switch on the frame the grip closed; a hand does not arrive instantly and neither does
// the authority it has over the weapon. Same spring shape as the recoil and the carry blend, so the
// three feel like one system.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandEngageMs = 220.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandEngageZeta = 0.75f;
// The live weight: 0 = the hand is not on the weapon at all, 1 = it fully is. Published so the ini
// tuner and the overlay can see it.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandWeight  = 0.0f;
// HOW MUCH LEVERAGE THE SUPPORT HAND HAS, 0..1: the captured hold's own offset length over
// TwoHandLeverFull. A pistol's second hand sits 7 cm from the wrist and a rifle's is out at 35, and
// that ratio is the whole difference between steadying a weapon and merely touching it. The aim
// correction below has always used it; the recoil reads it too, so a hand under a pistol grip stops
// damping the flip as hard as a hand on a handguard.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandLever   = 0.0f;
// LETTING GO IS QUICKER THAN TAKING HOLD, and not for symmetry's sake: a hand closing on a rifle
// settles onto it, while a hand opening simply stops steadying it. A release should be minimal,
// so the release gets its own, much shorter span.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandReleaseMs = 110.0f;
// The weapon coming back into the right hand: how long it takes to settle out of the left hand's hold,
// and how that settle damps. See the note above CarryReturnRight for what is being faded.
// 0 = the plugin does NOT move the right wrist on the way back. The weapon now travels by its own
// slot offset, eased in the right bone's frame by the weapon module, so the hand must not be displaced
// as well -- two springs on one hand-over fight each other. The derivation above and the code below are
// kept because they are the only record of how the pose relation cancels out, and because the moment
// the carry ever goes back to moving the item, this is the way to make the return smooth again.
extern "C" __declspec(dllexport) int   CyberpunkVR_CarryReturnHand = 1;
extern "C" __declspec(dllexport) float CyberpunkVR_CarryReturnMs   = 260.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_CarryReturnZeta = 0.70f;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugCarryReturn = 0.0f;
// REACHING BACK FOR THE CARRIED WEAPON. Inside this radius the right hand is offered the weapon: its
// fingers close into the captured grip pose and the grip button takes the gun back. Same number for
// both, deliberately -- a preview that promises a take the button will not perform is a lie the player
// finds at the edge. The Lua gate reads it through GetVRCarryReach().
// 9 cm, halved from 18 by feel. An ini key (xr_carry_radius), because it is a number to
// settle by feel and that must not cost a rebuild per attempt.
extern "C" __declspec(dllexport) float CyberpunkVR_CarryGripRadius = 0.09f;
// ...and how the offer arrives: the same spring as everything else in this file, because a hand closing
// on a grip settles onto it. The pose preview blends in on that spring too.
extern "C" __declspec(dllexport) float CyberpunkVR_CarryGripMs     = 200.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_CarryGripZeta   = 0.80f;
extern "C" __declspec(dllexport) float CyberpunkVR_CarryGripBlend  = 0.0f;   // 0..1, read by the finger pass
extern "C" __declspec(dllexport) float CyberpunkVR_DebugCarryReach = -1.0f;  // metres, -1 = not carrying
// HOW MUCH OF THE AIM THE SUPPORT HAND OWNS. A third of the error, capped at 15 degrees, with the first
// two degrees ignored: enough that bringing the left hand up visibly settles and steers the weapon, far
// too little for it to take the weapon over and lay it on its side. The right hand stays the one holding
// the gun -- which is what it is doing.
// HOW MUCH OF THE AIM THE SUPPORT HAND OWNS -- AND IT IS DECIDED BY LEVERAGE, not by preference.
//
// The two ways this is done in VR are well known: most games snap the off hand to the weapon and let it
// affect nothing, while games with long guns (Pavlov and its like) aim the weapon along the line from the
// rear hand to the front one. Both are right, for different weapons, and the thing that separates them is
// the BASELINE -- how far apart the hands are.
//
// On a pistol both hands are on the same grip: measured here, 74 mm apart. A centimetre of tracking noise
// across 74 mm is 8 degrees, so aiming along that line is aiming along the noise -- which is exactly what
// "the pistol teleports sideways when I move my palm" was. On a rifle the support hand is out at the
// handguard, 350-450 mm away, where the same centimetre is under two degrees and the hand really does
// steer the weapon, because it really does have the leverage.
//
// So the gain SCALES with the captured baseline, in proportion to it: a pistol hold at 74 mm keeps
// about a fifth of the authority a rifle hold at 350 mm gets -- the first school and the second out of
// one rule, with no weapon list and no switch to set. The recoil the second hand absorbs is not gated on any of this -- holding a
// pistol with both hands steadies it whether or not it steers it.
// 1.0 AT FULL LEVERAGE: on a rifle the left hand controls the front and the right hand the back,
// as in life and in native VR games. At a rifle's baseline that is
// the whole error, i.e. the barrel really does follow the line between the hands -- the second school
// named above, in full, rather than six tenths of it. A pistol is untouched by this: its own leverage
// (74 mm of 350) still leaves it about a fifth, which is the first school.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandAimGain   = 1.00f;   // at full leverage
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandLeverFull = 0.35f;   // m, at this: all of it
// How quickly the push is allowed to become a correction, seconds. This is what makes a jump
// impossible: the aim can only ramp toward the hand, never step to it.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandAimTau    = 0.12f;
// THE CEILING AND THE DEADZONE ARE SCALED BY THE SAME LEVERAGE, and that is the point of them.
// Both existed to protect a PISTOL: 7 cm between the hands turns a centimetre of tracking noise into
// several degrees, so a hard 10 deg ceiling and a 4 deg deadzone were what kept it from shaking. On a
// rifle the same centimetre is under two degrees, the noise argument is gone, and those two numbers are
// simply a wall in front of the aim. So the ceiling is multiplied by the leverage and the deadzone is
// faded out by it: a pistol keeps 12.7 deg and 3.4, a rifle gets 60 deg and 1.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandAimMaxDeg = 60.0f;  // x leverage
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandAimDeadDeg = 4.0f;  // faded out by leverage
// HOW THE SUPPORT HAND STEERS THE BARREL. 0 = not at all: the weapon is the right controller's, one to
// one, and the support hand only closes on it and takes the kick. 1 = RELATIVE (the default): the barrel
// follows the left hand by how far it has moved AGAINST the right one since the grip closed. 2 = ABSOLUTE,
// the old behaviour: the barrel is laid on the line to the left hand from the first frame.
//
// 2 is what was seen: releasing the left grip made the weapon jump out onto the controllers'
// real pose. Laid on the line from the start, the weapon is turned by however far the player's hands
// sit from the captured animation's -- a constant the player never asked for -- so the drawn right hand is
// off the real one for the whole hold and the weapon travels back by all of it on release. 0 cured that
// and took the steering with it (the left hand could no longer correct the weapon). 1 keeps both: the squeeze
// steers nothing, hands moved together steer nothing, and what the weapon gives back on release is exactly
// what the left hand steered. ini: xr_two_hand_aim.
extern "C" __declspec(dllexport) int   CyberpunkVR_TwoHandAim = 1;
// How long the steer takes to leave once the hand has, milliseconds -- critically damped, so the weapon
// settles onto the controller rather than swinging past it. Longer than the hand's own release
// (TwoHandReleaseMs): the hand simply lets go, while the weapon has a deliberate steer to give back.
// 400, doubled from 200 (the weapon came back too slowly). An ini key
// (xr_two_hand_aim_release_ms), because it is a number to settle by feel.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandAimReleaseMs = 400.0f;
// The steer last applied, degrees -- for the overlay and the tuner.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugTwoHandAimDeg = 0.0f;
// A HELD GRIP NEEDS AN END. The support point now follows the hand only by direction, so a squeeze held on
// a hand pulled half a metre away would leave the drawn one welded to the handguard. Beyond this distance
// from the support point the hand comes off -- held that far for BreakMs, so a recoil throw (a sniper's
// reaches ~14 cm for a moment) never counts. Still squeezing and back inside the grip radius, it takes
// hold again by itself. 0 = never. Not applied to mode 2, which keeps its old semantics. ini:
// xr_two_hand_break.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandBreakRadius = 0.60f;
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandBreakMs     = 250.0f;

// ---- live state, published so it can be read from outside ----
extern "C" __declspec(dllexport) int   CyberpunkVR_TwoHandActive  = 0;      // 1 = the hand is on the gun
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandBlend   = 0.0f;   // finger preview ramp 0..1
extern "C" __declspec(dllexport) float CyberpunkVR_DebugTwoHandDist = -1.0f;
extern "C" __declspec(dllexport) int   CyberpunkVR_DebugTwoHandHave = 0;
extern "C" __declspec(dllexport) int   CyberpunkVR_TwoHandCaptureReq = 0;
extern "C" __declspec(dllexport) int   CyberpunkVR_DebugTwoHandRefused = 0;
extern "C" __declspec(dllexport) int   CyberpunkVR_DebugTwoHandSaved = 0;
// How many captured holds were found at startup -- one per weapon anyone has recorded.
extern "C" __declspec(dllexport) int   CyberpunkVR_DebugTwoHandLoaded = 0;
// The baseline the last capture saw, millimetres -- the number that tells a hold from a hand at rest.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugTwoHandBaseMm = -1.0f;

extern "C" {
// A PUMP'S FORE-END RUN BACK UNDER THE SUPPORT HAND: a MODEL-space offset of the support point, set by the
// reload mod (SetVRTwoHandSlide) while the player racks the gun with the hand still closed on it. Racking a
// pump IS the support hand's job, so the hold stays shut through the stroke and its drawn wrist rides the
// fore-end. The steer and the reach are measured with the stroke taken back out of the hand, so working the
// action neither turns the barrel nor reads as letting go. A deadline rather than a flag: a mod that stops
// refreshing it cannot leave the hand displaced.
float    g_VRTwoHandSlide[3]   = {0.0f, 0.0f, 0.0f};
uint64_t g_VRTwoHandSlideUntil = 0;
// ...and THE LEFT CONTROLLER ITSELF, model space, as this hold read it. While the hold is closed every published
// left wrist is the weld -- it rides the gun -- so a stroke measured from it against the gun is always zero. The
// pump measures the real hand from this instead (GetVRTwoHandLeftCtrl).
float g_VRTwoHandCtrlL[3]   = {0.0f, 0.0f, 0.0f};
int   g_VRTwoHandCtrlLValid = 0;
}

// ================================================================================================
// THE POSE HOOK'S SWITCHES (src/Hooks/AnimPoseBody.cpp, or rust/crates/cvr-rig/src/vrik/pose_hook.rs).
// ================================================================================================
// The body's physical turns pivot about the player's neck as the roomscale ledger has it (1), or about the eye's lever
// straight ahead (0) -- Camera/AnchorTranslation.hpp BodyOrbitNeckLever.
extern "C" __declspec(dllexport) int   CyberpunkVR_BodyOrbitRestNeck = 1;
// Debug: that neck (tracking metres from the calibrated eye: right, forward); NaN when there is none.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugRestNeck[2] = {NAN,NAN};
// 1 = a grapple or takedown (PSM Takedown 1..4, g_VRTakedownState) hands everything VRIK writes back to
// its paired animation; 0 = VRIK keeps solving through it.
extern "C" __declspec(dllexport) int CyberpunkVR_VrikTakedownSuspend = 1;
// 1 = a carried body takes the left arm (carry, aim) or the whole body (pick up, drop, throw, putting it
// away) for its animation (g_VRBodyCarryingState, g_VRBodyDisposalState); 0 = VRIK keeps solving through it.
extern "C" __declspec(dllexport) int CyberpunkVR_VrikBodyCarryHandOff = 1;
// 1 = a finisher's paired animation (workspot tagged FinisherWorkspot, g_VRFinisherActive) hands everything
// VRIK writes back to it, as a grapple does; 0 = VRIK keeps solving through it.
extern "C" __declspec(dllexport) int CyberpunkVR_VrikFinisherSuspend = 1;
#endif
#endif