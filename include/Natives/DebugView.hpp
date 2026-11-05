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
// physics::ProxyType of a proxy, 0 if there is none.
int ProxyKind(uint32_t proxy);
int ProxyBodies(uint32_t proxy);
// Lifts PhysX's ceiling on how fast a body may turn, which this body was held to. False if it is not a body of the PhysX this was read from. `was` gets the old ceiling.
bool RaiseSpin(uint32_t proxy,uint32_t body,float most,float* was);
}
