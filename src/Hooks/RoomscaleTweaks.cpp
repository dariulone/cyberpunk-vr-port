#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// THE CAPSULE A HEADSET STANDS IN (formerly part of src/Hooks/RoomscaleMove.cpp). The C++ build's; with CVR_RUST_VRIK
// it is rust/crates/cvr-ffi/src/native_tweaks.rs (a native tweak of the Rust records, and cvr_tweak_capsule_radius for
// rust/crates/cvr-rig/src/vrik/roomscale_move.rs). The C++ move reads the radius through cvr_cpp_tweak_capsule_radius.
//
// Every on-foot locomotion state gives the player a 0.40 m radius, and the controller keeps another 7 cm of contact
// offset: the body's axis stops 0.47 m from a wall, the shoulders with it, and a hand on an arm measured from those
// shoulders could not reach a button on that wall. 0.25 m brings the axis to 0.32 m, where the arm reaches past the
// surface, and the view's lead over a blocked capsule shrinks with it. The states on foot only; swimming, grapples and
// takedowns, braindances and scene tiers keep the game's radius.
//
// NOT ON A LADDER. The ladder's exit over the top carries the capsule forward by what a 0.40 m one needs to stand on
// the floor beyond; a 0.25 m capsule was left over the gap between the ladder and that floor and fell -- the whole
// height of a long ladder (players, 2026-11-05, a 9.1 m ladder with exitStepTop -1: V reached the floor level, moved
// 12 cm and dropped 9 m; with the ladder states at 0.40 it walked off onto the roof). Nothing on a ladder needs the
// shoulders nearer a wall, so all five ladder states keep the game's radius.
#if !CVR_RUST_VRIK
#include "Core/TweakRecords.hpp"
#include <RED4ext/RTTISystem.hpp>
#include <RED4ext/TweakDB.hpp>
#include <cmath>
#include <initializer_list>
#include <shared_mutex>
#include <string>

extern void Log(const char* fmt, ...);

namespace {
constexpr float kVrCapsuleRadius = 0.25f;
struct StateGroup { const char* package; std::initializer_list<const char*> states; };
const StateGroup kOnFootStates[] = {
    {"playerStateMachineBaseLocomotion", {"baseLocomotionState", "locomotionAir", "locomotionDash", "locomotionGround"}},
    {"playerStateMachineBaseLocomotionLowGravity",
     {"baseLocomotionStateLowGravity", "locomotionAirLowGravity", "locomotionGroundLowGravity"}},
    {"playerStateMachineLocomotion",
     {"abstractStatusEffect", "aimWalk", "airHover", "airThrusters", "bodySlamJump", "chargeJump", "climb",
      "coolExitJump", "crouch", "crouchSprint", "deathLand", "dodge", "dodgeAir", "doubleJump", "failedLandingAbstract",
      "fall", "felled", "forceFreeze", "forceIdle", "forceWalk", "forcedKnockdown", "hardLand", "hoverJump", "jump",
      "knockdown", "landingAbstract",
      "regularLand", "slide", "slideAfterFall", "slideFall", "sprint", "stand", "stunned", "superheroFall",
      "superheroLand", "superheroLandRecovery", "unsecureFootingFall", "vault", "vehicleKnockdown", "veryHardLand",
      "workspot"}},
    {"playerStateMachineLocomotionLowGravity",
     {"climbLowGravity", "crouchLowGravity", "dodgeAirLowGravity", "dodgeCrouchLowGravity", "dodgeLowGravity",
      "fallLowGravity", "jumpLowGravity", "preCrouchLowGravity", "regularLandLowGravity", "slideLowGravity",
      "sprintJumpLowGravity", "sprintLowGravity", "sprintWindupLowGravity", "standLowGravity"}},
    {"playerStateMachineMelee", {"meleeGroundSlamAttack"}},
};

void ShrinkOnFootCapsules(RED4ext::TweakDB* db) {
    auto* const floatType = RED4ext::CRTTISystem::Get()->GetType("Float");
    cvr::tweak_records::FlatWrites writes(db);
    int changed = 0, missing = 0;
    for (const auto& group : kOnFootStates)
        for (const char* state : group.states) {
            const RED4ext::TweakDBID flat(std::string(group.package) + "." + state + ".capsuleRadius");
            const RED4ext::CStackType value = writes.Get(flat);
            if (value.type != floatType || !value.value) { ++missing; continue; }
            if (!(*static_cast<const float*>(value.value) > kVrCapsuleRadius)) continue;
            float radius = kVrCapsuleRadius;
            if (writes.Set(flat, floatType, &radius)) ++changed;
        }
    Log("[tweaks] on-foot capsule radius %.2f m: %d states set, %d not found\n", kVrCapsuleRadius, changed, missing);
}
const bool kCapsuleTweak = (cvr::tweak_records::AddNativeTweak(&ShrinkOnFootCapsules), true);
}

// The player's capsule radius as the locomotion states set it (stand, crouch, sprint, slide, jump, fall... all one),
// or 0 while TweakDB is not there or the value is implausible.
extern "C" float cvr_cpp_tweak_capsule_radius() {
    auto* db = RED4ext::TweakDB::Get();
    auto* rtti = RED4ext::CRTTISystem::Get();
    if (!db || !rtti) return 0;
    static auto* const floatType = rtti->GetType("Float");
    std::shared_lock<RED4ext::SharedSpinLock> guard(db->mutex00);
    auto* flat = db->GetFlatValue(RED4ext::TweakDBID("playerStateMachineLocomotion.stand.capsuleRadius"));
    if (!flat) return 0;
    const RED4ext::CStackType value = flat->GetValue();
    if (value.type != floatType || !value.value) return 0;
    const float radius = *static_cast<const float*>(value.value);
    return std::isfinite(radius) && radius > 0.1f && radius < 2.0f ? radius : 0;
}
#endif
#endif
