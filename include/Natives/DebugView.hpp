#pragma once
// The collision view: lines the script and PhysX put in the world, and the overlay draws for both eyes.
#include <cstdint>

namespace cvr::debugview {
struct Segment {float a[3];float b[3];uint32_t color;};
constexpr int kMaxSegments=12000;
// Colours by index, as the script passes them.
enum Color:int {White,Green,Red,Blue,Yellow,Cyan,Magenta,Orange,WorldStatic,WorldDynamic,ColorCount};
// The last finished frame's lines, or 0 once the script has stopped sending them.
int Copy(Segment* out,int capacity);
// What moves near a point, from PhysX itself: for a hand closing where no ray happened to go. Nearest first, by its bounds.
struct NearBody {uint32_t proxy;uint32_t body;float distance;float volume;};
int FindNear(float x,float y,float z,float radius,NearBody* out,int capacity);
// What a hand points at, best first, `distance` being the angle: within a cone round either direction, in the game's world.
int PickCone(const float* from,const float* a,const float* b,float cone,float range,uint32_t keepProxy,uint32_t keepBody,NearBody* out,int capacity);
// Where PhysX has a body, in the game's world, or false if it has none.
bool ActorWorld(uint32_t proxy,uint32_t body,float* xyz);
// The body of `proxy` whose own shapes are nearest `world` (the game's world), within `radius`: its index, its actor (a
// PxRigidDynamic), that actor's scene and the scene's origin, and how far (0 inside). False when none is. Script thread.
bool NearestBodyOf(uint32_t proxy,const float* world,float radius,uint32_t* body,void** actor,void** scene,float* origin,float* distance);
// PhysX's actor of a body (a test tool's look at its shapes), or null if it has none.
void* ActorOf(uint32_t proxy,uint32_t body);
// physics::ProxyType of a proxy, 0 if there is none.
int ProxyKind(uint32_t proxy);
int ProxyBodies(uint32_t proxy);
// Lifts PhysX's ceiling on how fast a body may turn, which this body was held to. False if it is not a body of the PhysX this was read from. `was` gets the old ceiling.
bool RaiseSpin(uint32_t proxy,uint32_t body,float most,float* was);
// The PhysX actor of a body, when it is a PxRigidDynamic of the PhysX this was read from: with its scene, and where that scene's origin is in the game's world
// (PhysX's poses are relative to it). Null if not. Read on the script's thread, like the rest of this file.
void* RigidDynamicOf(uint32_t proxy,uint32_t body,void** scene,float* origin);
// What decides a body's contacts, to the log: flags, dominance, aggregate, its shapes' filter data.
void FilterInfo(uint32_t proxy,uint32_t body);
// True once a body of a destructible thing has broken and its actor is gone: nothing may call its body interface after
// (a call reaches the freed actor). Any other owner's body is never gone. Any thread.
bool BodyGone(uint32_t proxy,uint32_t body);
// Told by whoever saw its actor die (Natives/HeldBody.cpp).
void NoteGone(uint32_t proxy,uint32_t body);
// A body whose actor is known (Natives/HeldBody.cpp, as it takes hold): BodyGone checks that actor on every call from now
// on, whatever the body's owner. Any thread.
void NoteActor(uint32_t proxy,uint32_t body,void* actor);
}
