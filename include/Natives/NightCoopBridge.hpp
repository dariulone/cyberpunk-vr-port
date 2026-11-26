#pragma once
#include <RED4ext/RED4ext.hpp>

// NIGHTCOOP: the VR body as partners see it (src/Natives/NightCoopBridge.cpp).
//
// NightCoop (the co-op mod) takes a pose source through its SDK and lays it on our player's body on the partners'
// machines: the neck and head by the head, a two-bone IK of each arm to its hand, the finger curls, the barrel's line
// for the shots, a squat. This bridge is that source: the head, the hands as the solved rig has them, the fingers
// as drawn, the muzzle, the physical crouch -- every frame, in world space.
//
// NightCoop is optional: nothing links to it, NightCoop.dll is looked up from the main thread once a second until it
// is there, and without it (or with its SDK switched off) this does nothing.
namespace cvr::nightcoop {
// RED4ext Running state: the lazy lookup and registration. Never finishes (false).
bool Update(RED4ext::CGameApplication*);
// Plugin unload: the pose source is taken off and the mod unregistered while NightCoop is still there.
void Shutdown();
}
