#pragma once

// HANDS ON THE STEERING WHEEL. See src/Anim/WheelGrab.cpp for the whole mechanism and why it needs
// no per-vehicle data at all.
//
// Signatures are copied from the definitions, never retyped.

#include <atomic>
#include <cstdint>
#include "Runtimes/VehicleMinimap.hpp"

namespace cvr::anim {

// ---- what the rest of the plugin reads ----------------------------------------------------------
//
// PLAIN GLOBALS, not shared slots. The upstream version (iPowerTech, wip motioncontroller vehicle
// steering) published thirteen of these into the shared block because it had to cross from the hands
// plugin to the dxgi proxy; that proxy is gone and both ends are this one DLL now, so the only thing
// still crossing a boundary is the armed mask the CET mods read (vrshared::kWheelArmedMask).
//
// Atomic because the producer and the consumers are different threads: the pose hook writes them on
// the engine's animation thread, the XInput detour reads them on the input thread and the overlay on
// the present thread.
extern std::atomic<float> g_wheelBlendRight;   // 0 = arm IK drives the hand, 1 = the animation does
extern std::atomic<float> g_wheelBlendLeft;
extern std::atomic<float> g_wheelSteer;        // -1 full left .. +1 full right, immediate controller input
extern std::atomic<float> g_wheelSteerDeg;     // the raw angle, for the overlay read-out
extern std::atomic<int>   g_wheelHornMask;     // a hand is on the hub: vrshared::kWheelArmed*Bit
// Functional script bridge: bit 2 = wheel feature available in driver seat;
// bits 0/1 = fresh right/left grip ownership. Gamepad settings are read, not changed.
int WheelControlState();
void WheelSetMountedCar(bool car);
void WheelSetMountedBike(bool bike); // the map over the bars; steering pushes and pulls them
bool WheelMountedBike();              // what the last refresh saw: the player is on a motorcycle
// The car's own cabin wheel (its steering_wheel slot) in the player's model space, from script each frame while a
// hand holds the rim: a hand on it rides it exactly (WheelBlendTarget). False for a non-finite pose.
bool WheelSetCabinSlot(const float pos[3], const float rot[4]);
// Fresh cabin- or bars-locked HUD anchor (a car or a bike); false for transitions, stale poses or recenter.
bool WheelReadMinimapAnchor(uint64_t origin,cvr::hud::VehicleMinimapAnchor& out);
bool WheelSetGamepadProfile(float inner,float outer);
float WheelGamepadSteer(float normalized);
float WheelSteerInput(); // latest coherent raw tracking, sampled by XInput

// Called from the pose hook, in this order, once per FRESH solve:
//
//   WheelCaptureAnim   right after the first VRIK_ComputeFK, where the buffer still holds the pure
//                      ANIMATED pose -- that FK hand IS the hand the driving animation puts on the
//                      wheel, and three lines later the segment lengths are rescaled and it stops
//                      being the animation's answer.
//   WheelUpdate        proximity, grab state and the blends.
//   WheelHandsOff      true once an arm is entirely the animation's: no solve, no length scale, no
//                      clavicle write, no cache entry.
//   WheelStoreTarget   the controller target this solve built, which is what the NEXT solve measures
//                      against the animated hand. Must keep being recorded while the arm is handed
//                      over, or letting go could never re-arm. Before hand-stop constraints.
//   WheelBlendTarget   blends the IK target toward the animated hand. A no-op at blend 0.
//   WheelPublishGrab   after both arm blocks: fresh ownership/radius only.
//                      Steering itself samples raw tracking at input-poll time.
void WheelCaptureAnim(int hand, int handIdx);
void WheelUpdate(float dtSec);
void WheelPublishGrab();
void WheelMaintainGrab(); // retain existing grip ownership during a camera-pair miss
void WheelBlendTarget(int hand, float* target, float* handRot);
void WheelStoreTarget(int hand, const float* target);
bool WheelHandsOff(int hand);

// Replays a relaxed finger pose onto whichever hand is not on the wheel. Called on EVERY player pass
// (the 4-5 passes per tick must leave the finger bones identical or they flicker), before the smoke
// grip so smoking still wins.
void WheelFingers(uint8_t* boneBuf);

// Drops the whole thing, published state included. WheelUpdate only runs inside a fresh mode-4
// solve, so switching tracking off with a hand at the wheel would otherwise leave the armed mask
// raised -- and a grip permanently out of its gameplay meaning.
void WheelReset();

}  // namespace cvr::anim
