#pragma once
// A carried thing in the hand: held by its own body (HoldBody), or its picture drawn on the hand while its body follows (Hold).
#include <cstdint>

namespace cvr::heldbody {
// A carried body, as the script knows it: its proxy and body index, the bone of the hand, and where the thing sits in that hand's frame.
// False if this kind of thing cannot be drawn this way (or the setting is off): then it stays as it was, its picture on its body.
// With the setting off it is still measured (View, the let-go line in the log), and nothing about it is changed.
// Blocked: the body has run into something, and the picture goes back to it. Reach: how far from its body the picture may be, m, for a wall ahead of it.
bool Hold(uint32_t proxy,uint32_t body,int bone,const float offset[3],const float turn[4],bool blocked,float reach);
// The grip has opened: from now on the picture goes with the body, closing what is between them.
void LetGo(uint32_t proxy,uint32_t body);
// On the game thread at the frame submit, the last moment before this frame's proxies are flushed.
void AtFrameSubmit();
// Held by its own body: its PhysX actor, dynamic, given at each PhysX step the velocities that bring it to where the hand has
// it (offset and turn as for Hold), as far as a hand's force can. False if it has no such actor.
bool HoldBody(uint32_t proxy,uint32_t body,int bone,const float offset[3],const float turn[4]);
// Lets it go at the next step, with this throw when `thrown` (m/s, rad/s, the game's world), else as it was going.
void LetGoBody(uint32_t proxy,uint32_t body,const float velocity[3],const float spin[3],bool thrown);
// Just before PxScene::simulate, on the thread that steps that scene (Natives/PhysXStep.hpp).
void BeforeSimulate(void* scene,float dt);
// What a held body's shapes add to their simulation filter while held (word 2 0xFFFFFFFF: their own group).
void SetHeldFilter(uint32_t word0,uint32_t word2);
// A body held by HoldBody in one hand alone (on) or not: seated for its grasp, the fingers closed on it. False if not held so.
bool GraspBody(uint32_t proxy,uint32_t body,bool on);
// A carried thing as it will be drawn: its body as the physics system has it now, its picture as the next submit will send it, and
// where the hand has it (position x y z, then rotation x y z w; handKnown false until the hand has been posed), with the picture's
// weight (0 on the body, 1 on the hand). With the picture setting off it is measured all the same and its picture is its body.
// False if it is not held here.
bool View(uint32_t proxy,uint32_t body,float bodyPose[7],float picture[7],float hand[7],bool* handKnown,float* weight);
// The hands steering a body of their own now: bit 0 the left, bit 1 the right. Any thread.
int HoldingMask();
// A body's queries (the player's movement and rays see it) off or back on, for a body with a PhysX actor of its own: set
// at the next PhysX step on that actor if it is still the body's, never through a released one. 1 if taken here, 0 if
// the body has no such actor (or none was set off here): then the game's SetIsQueryable is the way.
int QueryWanted(uint32_t proxy,uint32_t body,bool on);
}
