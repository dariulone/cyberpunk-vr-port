#pragma once
#include <cstdint>
#include <RED4ext/Scripting/Natives/Generated/GameplayTier.hpp>

namespace cvr::anim {
inline bool IsControllerVrikEnabled(int bindingMode,bool headAimActive) {
    return bindingMode==4 && !headAimActive;
}

// Keep the stored UI values: -1/0 disable, 1..4 select Tier2..Tier5.
// The engine enum is one-based and must not be compared to the UI index.
inline bool ShouldSuspendVrik(int engineTier,int configuredTier) {
    if(configuredTier<1 || configuredTier>4)return false;
    const int minimum=static_cast<int>(RED4ext::GameplayTier::Tier1_FullGameplay)+configuredTier;
    return engineTier>=minimum && engineTier<=static_cast<int>(RED4ext::GameplayTier::Tier5_Cinematic);
}
inline bool ShouldSuspendVrik(int engineTier,bool inVehicle,int onFootTier,int vehicleTier) {
    return ShouldSuspendVrik(engineTier,inVehicle ? vehicleTier : onFootTier);
}
// A POSE THE GAME HOLDS THE PLAYER IN -- sitting, lying, leaning: a workspot (locomotion 9, measured) in a staged scene
// (Tier2 and up). Its entry plays above the suspend threshold, and when it settles the tier drops back below it with the
// player still held, so VRIK and the physical body came back and drew the body beside the seat. With the setting at
// "Never" the player has asked for VRIK always, and gets it.
inline bool ShouldSuspendVrikForWorkspot(int engineTier,int locomotionState,int configuredTier) {
    return configuredTier>=1 && configuredTier<=4 && locomotionState==9 &&
        engineTier>static_cast<int>(RED4ext::GameplayTier::Tier1_FullGameplay) &&
        engineTier<=static_cast<int>(RED4ext::GameplayTier::Tier5_Cinematic);
}
// A GRAPPLE OR TAKEDOWN IS A PAIRED ANIMATION: the victim is placed against the body the animation
// authors, so arms solved to the controllers let go of it. gamePSMTakedown 1..4 (EnteringGrapple,
// Grapple, Leap, Takedown) hand the body to the animation; 0 and unknown (-1) do not.
inline bool ShouldSuspendVrikForTakedown(int takedownState,int enabled) {
    return enabled!=0 && takedownState>=1 && takedownState<=4;
}
// A CARRIED BODY. Lifting it, dropping it, throwing it and putting it away (gamePSMBodyCarrying 1 PickUp,
// 4 Drop, 6 Throw, 3 Dispose) are paired animations of both arms, handed over like a grapple. Putting it
// into a container is a workspot during which BodyCarrying already reads 0 (measured): it is told by
// BodyDisposalDetailed 1 Dispose, 2 Lethal, 3 NonLethal instead, and only while the player is still in that
// workspot (locomotion 9, measured) -- the trunk clears the flag itself, the container's clear is not in
// script, so a flag left behind cannot hold VRIK off afterwards. Carrying it (2 Carry, 5 Aim) keeps the
// right hand free for a one-handed weapon, so only the LEFT arm, the one under the body, goes to the
// animation.
inline bool ShouldSuspendVrikForBodyCarry(int bodyCarryingState,int bodyDisposalState,int locomotionState,int enabled) {
    return enabled!=0 && (bodyCarryingState==1 || bodyCarryingState==3 || bodyCarryingState==4 || bodyCarryingState==6 ||
        (bodyDisposalState>=1 && bodyDisposalState<=3 && locomotionState==9));
}
// A FINISHER with its paired animation is a workspot tagged FinisherWorkspot: the victim is
// held against the body the animation authors, as in a grapple. Fast finishers are an ordinary melee hit
// and never get here. A finisher lasts a few seconds; a flag older than 20 s is one whose end never
// arrived (a load in the middle of it) and must not keep VRIK off.
inline bool ShouldSuspendVrikForFinisher(int finisherActive,uint64_t ageMs,int enabled) {
    return enabled!=0 && finisherActive>0 && ageMs<20000;
}
inline bool ShouldHandLeftArmToBodyCarry(int bodyCarryingState,int enabled) {
    return enabled!=0 && (bodyCarryingState==2 || bodyCarryingState==5);
}
// ...and the right arm too while the carry animation holds the body with both hands
// (AnimFeature_Carry.useBothHands: every carry except the strong one with a firearm out and the wounded
// soldier). Unknown (-1, the redscript missing) keeps the right arm on its controller.
inline bool ShouldHandRightArmToBodyCarry(int bodyCarryingState,int bothHands,int enabled) {
    return ShouldHandLeftArmToBodyCarry(bodyCarryingState,enabled) && bothHands>0;
}
inline int ResolveVehicleVrikSuspendTier(int onFootTier,int vehicleTier,bool explicitlyConfigured) {
    const int tier=explicitlyConfigured ? vehicleTier : onFootTier;
    return tier < -1 ? -1 : (tier > 4 ? 4 : tier);
}
}
