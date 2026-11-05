// DebugView -- what the hands and the physics actually see, drawn in the world.
//
// THE SHAPES COME FROM PHYSX ITSELF. The game keeps them out of reach: its body interface answers dimensions and no shape, and static
// collision is compiled into sector data. But it links PhysX 3.4 from PhysX3_x64.dll, which exports PxGetPhysics, and from there everything
// is the public API: the scenes, an overlap query, and each hit's actor, shape and geometry, for the world as much as for loose props.
//
// BY TABLE SLOT, because the port carries no PhysX headers. The game's PhysX is older than the published 3.4 headers: its scene has six more
// virtuals before the queries (getActiveTransforms among them), so the slots below were placed in the DLL itself, from the error messages each
// function holds: simulate 56, fetchResults 61, setCCDMaxPasses 70, getRenderBuffer 79, flushQueryUpdates 83, fetchQueries 93, lockWrite 107,
// from the table's start at RVA 0x20CAF0. The shape has one virtual more than the headers too, before getGeometryType: its slots below were read from the
// functions themselves, each getter's own compare (getBox 3, getSphere 0, getCapsule 2, getConvex 4, getTriangles 5). COUNTED FROM THE HEADERS THEY WERE ONE OFF,
// and what was called as getLocalPose was setLocalPose: the view wrote a zero pose over live collision shapes, and the player fell through the world. So a slot that
// has a setter beside it is checked against the function's address in this very DLL before the first call, and any other DLL leaves the world's shapes off.
// All of this is off unless the view is on.
//
// PHYSX'S SPACE IS NOT THE WORLD'S. The game's raycast (sub_1403DCB80 in 2.5) passes PhysX the world position less a vector at +0x134 of its scene
// object, whose +0x18 is the PxScene, and its hit converter (sub_1403DDE38) adds that vector back. The scene's userData is that object. So
// every query here goes in less the offset, and every shape comes out plus it.
//
// A shape only appears if the game lets queries see it. Cloth and particles are not in a query at all.

#include "Natives/DebugView.hpp"
#include "Natives/NativeFunctions.hpp"
#include "Core/LiveControls.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/Generated/Vector4.hpp>
#include <Windows.h>
#include <atomic>
#include <cmath>
#include <cstring>
#include <mutex>

extern void Log(const char* fmt, ...);

namespace cvr::debugview {
namespace {
Segment back[kMaxSegments];
int backCount=0;
Segment front[kMaxSegments];
int frontCount=0;
uint64_t frontStamp=0;
std::mutex frontLock;

inline void Add(const float* a,const float* b,int color) {
    if(backCount>=kMaxSegments)return;
    Segment& s=back[backCount++];
    s.a[0]=a[0];s.a[1]=a[1];s.a[2]=a[2];s.b[0]=b[0];s.b[1]=b[1];s.b[2]=b[2];s.color=uint32_t(color);
}

int gunOutlineColor=-1;   // a body's outline: its lines in this colour
// ---- PhysX 3.4, by slot
struct Vec3 {float x,y,z;};
struct Quat {float x,y,z,w;};
struct Transform {Quat q;Vec3 p;};
struct OverlapHit {void* actor;void* shape;uint32_t faceIndex;uint32_t pad;};
struct SphereGeometry {int type;float radius;};
struct BoxGeometry {int type;Vec3 half;};
struct CapsuleGeometry {int type;float radius;float halfHeight;};
struct ConvexGeometry {int type;Vec3 scale;Quat scaleRot;void* mesh;uint8_t flags;uint8_t pad[3];};
struct TriangleGeometry {int type;Vec3 scale;Quat scaleRot;uint8_t flags;uint8_t pad[3];void* mesh;};
struct HullPolygon {float plane[4];uint16_t count;uint16_t base;};
struct FilterData {uint32_t word[4];uint16_t flags;uint8_t client;uint8_t pad;};
// PxHitCallback<PxOverlapHit>: the virtuals in this order, the members in this order.
struct OverlapCallback {
    OverlapHit block;bool hasBlock;OverlapHit* touches;uint32_t maxTouches;uint32_t touchCount;
    uint32_t reported=0;
    virtual bool processTouches(const OverlapHit*,uint32_t count){reported=count;return false;}
    virtual void finalizeQuery(){}
    virtual ~OverlapCallback()=default;
};
enum {GeomSphere,GeomPlane,GeomCapsule,GeomBox,GeomConvex,GeomTriangles,GeomHeightField};
constexpr int kPhysicsGetNbScenes=19,kPhysicsGetScenes=20;
constexpr int kSceneGetPhysics=6,kSceneGetGravity=67,kSceneOverlap=96,kSceneGetNbActors=18,kSceneGetActors=19,kSceneGetNbAggregates=26,kSceneGetAggregates=27;
constexpr int kAggregateGetActors=11;
constexpr int kBaseGetConcreteTypeName=1;
constexpr int kActorGetType=5,kActorGetWorldBounds=9,kActorGetGlobalPose=20,kActorGetShapes=26;
constexpr int kShapeGetGeometryType=7,kShapeGetBox=10,kShapeGetSphere=11,kShapeGetCapsule=12,kShapeGetConvex=14,kShapeGetTriangles=15,kShapeGetLocalPose=19;
constexpr uintptr_t kShapeGetLocalPoseRva=0x66910,kShapeGetGeometryTypeRva=0x66250;
constexpr int kConvexGetVertices=6,kConvexGetIndexBuffer=7,kConvexGetNbPolygons=8,kConvexGetPolygon=9;
constexpr int kTrianglesGetNbVertices=5,kTrianglesGetVertices=6,kTrianglesGetNbTriangles=9,kTrianglesGetTriangles=10,kTrianglesGetFlags=11;
template<class F> F Slot(void* object,int slot){return reinterpret_cast<F>((*reinterpret_cast<void***>(object))[slot]);}

constexpr int kMaxScenes=32,kMaxHits=256,kMaxTriangles=1500,kMaxActors=40000,kMaxActorShapes=16;
void* actorBuffer[kMaxActors];
void* aggregateBuffer[kMaxActors];
int geometryCount[8];
int shapeSamples=0;
int sceneSaid[kMaxScenes];
int sceneLast[kMaxScenes];
void* scenes[kMaxScenes];
void* sceneOwners[kMaxScenes];   // the game's object for each, when it is proved to be
int sceneCount=0;
int proved=0;      // 0 not yet, 1 yes, -1 no
OverlapHit hitBuffer[kMaxHits];
OverlapCallback callback;
bool shapeProved=false;

inline Vec3 Rotate(const Quat& q,const Vec3& v) {
    const float tx=2*(q.y*v.z-q.z*v.y),ty=2*(q.z*v.x-q.x*v.z),tz=2*(q.x*v.y-q.y*v.x);
    return {v.x+q.w*tx+q.y*tz-q.z*ty,v.y+q.w*ty+q.z*tx-q.x*tz,v.z+q.w*tz+q.x*ty-q.y*tx};
}
inline Vec3 Apply(const Transform& t,const Vec3& v) {
    const Vec3 r=Rotate(t.q,v);
    return {r.x+t.p.x,r.y+t.p.y,r.z+t.p.z};
}
Vec3 drawOffset{};   // the scene being drawn: PhysX's space to the world's
inline Vec3 ToWorld(const Transform& t,const Vec3& v) {
    const Vec3 r=Apply(t,v);
    return {r.x+drawOffset.x,r.y+drawOffset.y,r.z+drawOffset.z};
}
inline Quat Multiply(const Quat& a,const Quat& b) {
    return {a.w*b.x+a.x*b.w+a.y*b.z-a.z*b.y,a.w*b.y-a.x*b.z+a.y*b.w+a.z*b.x,a.w*b.z+a.x*b.y-a.y*b.x+a.z*b.w,a.w*b.w-a.x*b.x-a.y*b.y-a.z*b.z};
}
inline void Line(const Transform& t,const Vec3& a,const Vec3& b,int color) {
    const Vec3 wa=ToWorld(t,a),wb=ToWorld(t,b);
    Add(&wa.x,&wb.x,color);
}
void Ring(const Transform& t,const Vec3& center,const Vec3& u,const Vec3& v,float radius,int color,int parts,float from,float to) {
    Vec3 last{};
    for(int i=0;i<=parts;++i) {
        const float angle=from+(to-from)*float(i)/float(parts),c=std::cos(angle)*radius,s=std::sin(angle)*radius;
        const Vec3 p{center.x+u.x*c+v.x*s,center.y+u.y*c+v.y*s,center.z+u.z*c+v.z*s};
        if(i)Line(t,last,p,color);
        last=p;
    }
}
constexpr float kPi=3.14159265f;
void BoxLines(const Transform& t,const Vec3& h,int color) {
    const Vec3 c[8]={{-h.x,-h.y,-h.z},{h.x,-h.y,-h.z},{h.x,h.y,-h.z},{-h.x,h.y,-h.z},{-h.x,-h.y,h.z},{h.x,-h.y,h.z},{h.x,h.y,h.z},{-h.x,h.y,h.z}};
    for(int i=0;i<4;++i) {
        Line(t,c[i],c[(i+1)%4],color);Line(t,c[i+4],c[(i+1)%4+4],color);Line(t,c[i],c[i+4],color);
    }
}
void SphereLines(const Transform& t,float r,int color) {
    Ring(t,{0,0,0},{1,0,0},{0,1,0},r,color,12,0,2*kPi);
    Ring(t,{0,0,0},{0,1,0},{0,0,1},r,color,12,0,2*kPi);
    Ring(t,{0,0,0},{0,0,1},{1,0,0},r,color,12,0,2*kPi);
}
// Along local X, as PhysX has them.
void CapsuleLines(const Transform& t,float r,float h,int color) {
    const Vec3 y{0,1,0},z{0,0,1},x{1,0,0};
    const Vec3 a{-h,0,0},b{h,0,0};
    Ring(t,a,y,z,r,color,8,0,2*kPi);
    Ring(t,b,y,z,r,color,8,0,2*kPi);
    Line(t,{-h,r,0},{h,r,0},color);Line(t,{-h,-r,0},{h,-r,0},color);Line(t,{-h,0,r},{h,0,r},color);Line(t,{-h,0,-r},{h,0,-r},color);
    Ring(t,b,y,x,r,color,4,0,kPi);Ring(t,b,z,x,r,color,4,0,kPi);
    Ring(t,a,x,y,-r,color,4,-kPi/2,kPi/2);Ring(t,a,x,z,-r,color,4,-kPi/2,kPi/2);
}
void ConvexLines(const Transform& t,const ConvexGeometry& g,int color) {
    if(!g.mesh)return;
    const Vec3* verts=Slot<const Vec3*(*)(void*)>(g.mesh,kConvexGetVertices)(g.mesh);
    const uint8_t* indices=Slot<const uint8_t*(*)(void*)>(g.mesh,kConvexGetIndexBuffer)(g.mesh);
    const uint32_t polygons=Slot<uint32_t(*)(void*)>(g.mesh,kConvexGetNbPolygons)(g.mesh);
    const auto getPolygon=Slot<bool(*)(void*,uint32_t,HullPolygon*)>(g.mesh,kConvexGetPolygon);
    if(!verts || !indices)return;
    for(uint32_t p=0;p<polygons && p<256;++p) {
        HullPolygon h{};
        if(!getPolygon(g.mesh,p,&h) || h.count<2)continue;
        for(uint16_t i=0;i<h.count;++i) {
            const Vec3& a=verts[indices[h.base+i]];
            const Vec3& b=verts[indices[h.base+(i+1)%h.count]];
            Line(t,{a.x*g.scale.x,a.y*g.scale.y,a.z*g.scale.z},{b.x*g.scale.x,b.y*g.scale.y,b.z*g.scale.z},color);
        }
    }
}
void TriangleLines(const Transform& t,const TriangleGeometry& g,const Vec3& center,float radius,int color) {
    if(!g.mesh)return;
    const Vec3* verts=Slot<const Vec3*(*)(void*)>(g.mesh,kTrianglesGetVertices)(g.mesh);
    const uint32_t vertexCount=Slot<uint32_t(*)(void*)>(g.mesh,kTrianglesGetNbVertices)(g.mesh);
    const uint32_t triangles=Slot<uint32_t(*)(void*)>(g.mesh,kTrianglesGetNbTriangles)(g.mesh);
    const void* data=Slot<const void*(*)(void*)>(g.mesh,kTrianglesGetTriangles)(g.mesh);
    uint8_t flags=0;
    Slot<void*(*)(void*,uint8_t*)>(g.mesh,kTrianglesGetFlags)(g.mesh,&flags);
    if(!verts || !data)return;
    const bool shortIndices=(flags&2)!=0;   // PxTriangleMeshFlag::e16_BIT_INDICES
    int drawn=0;
    const float limit=radius*radius;
    for(uint32_t i=0;i<triangles && drawn<kMaxTriangles;++i) {
        uint32_t k[3];
        for(int c=0;c<3;++c)k[c]=shortIndices?static_cast<const uint16_t*>(data)[i*3+c]:static_cast<const uint32_t*>(data)[i*3+c];
        if(k[0]>=vertexCount || k[1]>=vertexCount || k[2]>=vertexCount)continue;
        Vec3 w[3];
        bool close=false;
        for(int c=0;c<3;++c) {
            const Vec3& v=verts[k[c]];
            w[c]=ToWorld(t,{v.x*g.scale.x,v.y*g.scale.y,v.z*g.scale.z});
            const float dx=w[c].x-center.x,dy=w[c].y-center.y,dz=w[c].z-center.z;
            if(dx*dx+dy*dy+dz*dz<limit)close=true;
        }
        if(!close)continue;
        Add(&w[0].x,&w[1].x,color);Add(&w[1].x,&w[2].x,color);Add(&w[2].x,&w[0].x,color);
        ++drawn;
    }
}

// No C++ objects in here: SEH.
void DrawHit(const OverlapHit& hit,const Vec3& center,float radius) {
    void* shape=hit.shape;void* actor=hit.actor;
    if(!shape || !actor)return;
    if(!shapeProved) {
        const uintptr_t dll=reinterpret_cast<uintptr_t>(GetModuleHandleA("PhysX3_x64.dll"));
        void** table=*reinterpret_cast<void***>(shape);
        const char* name=Slot<const char*(*)(void*)>(shape,kBaseGetConcreteTypeName)(shape);
        if(!dll || !name || std::strncmp(name,"PxShape",7)!=0 ||
           reinterpret_cast<uintptr_t>(table[kShapeGetLocalPose])-dll!=kShapeGetLocalPoseRva ||
           reinterpret_cast<uintptr_t>(table[kShapeGetGeometryType])-dll!=kShapeGetGeometryTypeRva) {
            proved=-1;
            Log("[debugview] the shape's table is not the one these slots were read from: world shapes off\n");
            return;
        }
        shapeProved=true;
    }
    Transform global{},local{};
    Slot<void*(*)(void*,Transform*)>(actor,kActorGetGlobalPose)(actor,&global);
    Slot<void*(*)(void*,Transform*)>(shape,kShapeGetLocalPose)(shape,&local);
    const Vec3 at=Apply(global,local.p);
    const Transform t{Multiply(global.q,local.q),at};
    const int actorType=Slot<int(*)(void*)>(actor,kActorGetType)(actor);
    const int color=gunOutlineColor>=0?gunOutlineColor:(actorType==0?WorldStatic:WorldDynamic);
    const int geometry=Slot<int(*)(void*)>(shape,kShapeGetGeometryType)(shape);
    if(geometry>=0 && geometry<8)++geometryCount[geometry];
    if(shapeSamples<12) {
        ++shapeSamples;
        const Vec3 w{at.x+drawOffset.x,at.y+drawOffset.y,at.z+drawOffset.z};
        Log("[debugview] a shape: geometry %d, actor type %d, in the world at (%.2f %.2f %.2f), %.2f m from the middle\n",geometry,actorType,w.x,w.y,w.z,
            std::sqrt((w.x-center.x)*(w.x-center.x)+(w.y-center.y)*(w.y-center.y)+(w.z-center.z)*(w.z-center.z)));
    }
    switch(geometry) {
    case GeomBox: {
        BoxGeometry g{};
        if(Slot<bool(*)(void*,BoxGeometry*)>(shape,kShapeGetBox)(shape,&g))BoxLines(t,g.half,color);
        break;
    }
    case GeomSphere: {
        SphereGeometry g{};
        if(Slot<bool(*)(void*,SphereGeometry*)>(shape,kShapeGetSphere)(shape,&g))SphereLines(t,g.radius,color);
        break;
    }
    case GeomCapsule: {
        CapsuleGeometry g{};
        if(Slot<bool(*)(void*,CapsuleGeometry*)>(shape,kShapeGetCapsule)(shape,&g))CapsuleLines(t,g.radius,g.halfHeight,color);
        break;
    }
    case GeomConvex: {
        ConvexGeometry g{};
        if(Slot<bool(*)(void*,ConvexGeometry*)>(shape,kShapeGetConvex)(shape,&g))ConvexLines(t,g,color);
        break;
    }
    case GeomTriangles: {
        TriangleGeometry g{};
        if(Slot<bool(*)(void*,TriangleGeometry*)>(shape,kShapeGetTriangles)(shape,&g))TriangleLines(t,g,center,radius,color);
        break;
    }
    default:break;
    }
}

// THE GAME MAKES AND DROPS SCENES, so the list is asked for each time. A pointer kept from an earlier call was a call through freed memory. Only once: that this table is the one counted.
bool ReadScenes() {
    HMODULE dll=GetModuleHandleA("PhysX3_x64.dll");
    auto getPhysics=dll?reinterpret_cast<void*(*)()>(GetProcAddress(dll,"PxGetPhysics")):nullptr;
    void* physics=getPhysics?getPhysics():nullptr;
    if(!physics){Log("[debugview] no PxGetPhysics\n");return false;}
    const uint32_t count=Slot<uint32_t(*)(void*)>(physics,kPhysicsGetNbScenes)(physics);
    if(count>64){Log("[debugview] %u scenes: not believed\n",count);return false;}
    void* found[kMaxScenes]{};
    const uint32_t got=count?Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(physics,kPhysicsGetScenes)(physics,found,kMaxScenes,0):0;
    sceneCount=0;
    for(uint32_t i=0;i<got && i<kMaxScenes;++i) {
        void* scene=found[i];
        if(!scene || Slot<void*(*)(void*)>(scene,kSceneGetPhysics)(scene)!=physics) {
            Log("[debugview] scene %u does not know its PhysX: the table is not what was counted\n",i);
            return false;
        }
        void* owner=*reinterpret_cast<void**>(static_cast<char*>(scene)+8);
        const bool isOwner=owner && *reinterpret_cast<void**>(static_cast<char*>(owner)+0x18)==scene;
        sceneOwners[sceneCount]=isOwner?owner:nullptr;
        scenes[sceneCount++]=scene;
    }
    return true;
}

// Every shape of an actor, if its bounds come near.
int ActorNear(void* actor,const Vec3& center,float radius) {
    struct {Vec3 lo,hi;} bounds{};
    Slot<void*(*)(void*,void*,float)>(actor,kActorGetWorldBounds)(actor,&bounds,1.01f);
    const Vec3 c{center.x-drawOffset.x,center.y-drawOffset.y,center.z-drawOffset.z};
    const float dx=c.x<bounds.lo.x?bounds.lo.x-c.x:(c.x>bounds.hi.x?c.x-bounds.hi.x:0.f);
    const float dy=c.y<bounds.lo.y?bounds.lo.y-c.y:(c.y>bounds.hi.y?c.y-bounds.hi.y:0.f);
    const float dz=c.z<bounds.lo.z?bounds.lo.z-c.z:(c.z>bounds.hi.z?c.z-bounds.hi.z:0.f);
    if(!(dx*dx+dy*dy+dz*dz<=radius*radius))return 0;
    void* shapes[kMaxActorShapes]{};
    const uint32_t n=Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(actor,kActorGetShapes)(actor,shapes,kMaxActorShapes,0);
    int drawn=0;
    for(uint32_t s=0;s<n && s<kMaxActorShapes;++s) {
        OverlapHit hit{actor,shapes[s],0,0};
        DrawHit(hit,center,radius);
        if(proved<0)return drawn;
        ++drawn;
    }
    return drawn;
}

// A query answered nothing: every actor of the scene, by its bounds. Slow, and only while the view is on.
int ActorsNear(void* scene,const Vec3& center,float radius,uint32_t* outActors) {
    // PxActorTypeFlags has a copy constructor: by address, in this ABI.
    const uint16_t types=1|2;
    const uint32_t count=Slot<uint32_t(*)(void*,const uint16_t*)>(scene,kSceneGetNbActors)(scene,&types);
    *outActors=count;
    if(!count || count>kMaxActors)return 0;
    const uint32_t got=Slot<uint32_t(*)(void*,const uint16_t*,void**,uint32_t,uint32_t)>(scene,kSceneGetActors)(scene,&types,actorBuffer,kMaxActors,0);
    int drawn=0;
    for(uint32_t i=0;i<got && proved>=0;++i)drawn+=ActorNear(actorBuffer[i],center,radius);
    return drawn;
}

// LOOSE PROPS ARE NOT AMONG THE SCENE'S ACTORS: they sit in aggregates (the proxy kind 10 of a ray's hit), which the scene lists apart.
int AggregatesNear(void* scene,const Vec3& center,float radius,uint32_t* outAggregates,uint32_t* outActors) {
    const uint32_t count=Slot<uint32_t(*)(void*)>(scene,kSceneGetNbAggregates)(scene);
    *outAggregates=count;
    if(!count || count>kMaxActors)return 0;
    const uint32_t got=Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(scene,kSceneGetAggregates)(scene,aggregateBuffer,kMaxActors,0);
    int drawn=0;
    for(uint32_t i=0;i<got && proved>=0;++i) {
        void* members[64]{};
        const uint32_t n=Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(aggregateBuffer[i],kAggregateGetActors)(aggregateBuffer[i],members,64,0);
        *outActors+=n;
        for(uint32_t a=0;a<n && a<64 && proved>=0;++a)drawn+=ActorNear(members[a],center,radius);
    }
    return drawn;
}

int WorldInner(const Vec3& center,float radius) {
    if(proved<0 || !ReadScenes()) {proved=-1;return -1;}
    proved=1;
    SphereGeometry sphere{GeomSphere,radius};
    FilterData filter{};
    filter.flags=1|2|32;   // static, dynamic, every hit a touch
    int total=0;
    for(int s=0;s<sceneCount;++s) {
        if(!sceneOwners[s])continue;   // not the game's world: five planes each
        drawOffset={0,0,0};
        if(sceneOwners[s]) {
            const float* o=reinterpret_cast<const float*>(static_cast<char*>(sceneOwners[s])+0x134);
            drawOffset={o[0],o[1],o[2]};
        }
        const Transform pose{{0,0,0,1},{center.x-drawOffset.x,center.y-drawOffset.y,center.z-drawOffset.z}};
        callback.hasBlock=false;callback.touches=hitBuffer;callback.maxTouches=kMaxHits;callback.touchCount=0;callback.reported=0;
        const auto overlap=Slot<bool(*)(void*,const SphereGeometry*,const Transform*,OverlapCallback*,const FilterData*,void*)>(scenes[s],kSceneOverlap);
        overlap(scenes[s],&sphere,&pose,&callback,&filter,nullptr);
        const int n=int(callback.touchCount>callback.reported?callback.touchCount:callback.reported);
        total+=n;
        for(int i=0;i<n && i<kMaxHits;++i) {
            DrawHit(hitBuffer[i],center,radius);
            if(proved<0)return -1;
        }
        uint32_t actors=0,aggregates=0,inAggregates=0;
        int byActors=0;
        if(n==0) {
            byActors=ActorsNear(scenes[s],center,radius,&actors);
            if(proved<0)return -1;
            total+=byActors;
        }
        const int byAggregates=n==0?AggregatesNear(scenes[s],center,radius,&aggregates,&inAggregates):0;
        if(proved<0)return -1;
        total+=byAggregates;
        // Once for each scene, and again each of the first few times it has something new to show.
        const int found=n+byActors+byAggregates;
        if(sceneSaid[s]<12 && found!=sceneLast[s]) {
            ++sceneSaid[s];
            sceneLast[s]=found;
            Log("[debugview] scene %d at (%.1f %.1f %.1f): query %d hits (touch %u reported %u block %d); %u actors, %d shapes; %u aggregates of %u actors, %d shapes; so far sphere %d plane %d capsule %d box %d convex %d mesh %d field %d\n",
                s,center.x,center.y,center.z,n,callback.touchCount,callback.reported,int(callback.hasBlock),actors,byActors,aggregates,inAggregates,byAggregates,
                geometryCount[0],geometryCount[1],geometryCount[2],geometryCount[3],geometryCount[4],geometryCount[5],geometryCount[6]);
        }
    }
    return total;
}

// ---- WHAT IS THIS BODY? For one test: does a prop in the hand belong to an entity? The proxy says who owns it, so this prints it, by the class of what it
// points at, from the executable's own RTTI. It only reads, and only memory it has first asked Windows about: a bad read must not happen at all, because
// __try does not reliably stop one in this process (a crash report came out of a guarded read, 2026-11-04).
bool Readable(uintptr_t at,size_t size) {
    // canonical user space only: anything else faults whatever VirtualQuery says
    if(at<0x10000 || at>=0x00007FFFFFFFFFFFull || size>0x1000)return false;
    MEMORY_BASIC_INFORMATION info{};
    if(!VirtualQuery(reinterpret_cast<const void*>(at),&info,sizeof(info)) || info.State!=MEM_COMMIT)return false;
    if(info.Protect&(PAGE_NOACCESS|PAGE_GUARD))return false;
    const uintptr_t end=reinterpret_cast<uintptr_t>(info.BaseAddress)+info.RegionSize;
    return at+size<=end;
}
void ProbeProxy(int32_t proxy) {
    // sub_14027D430(system at 0x143BFCDD0, out handle, proxy id), and sub_14027D374(handle) to let it go, in game 2.5. Byte +0x34 of what it hands back is the kind.
    constexpr uintptr_t kSystemRva=0x3BFCDD0,kLookupRva=0x27D430,kReleaseRva=0x27D374;
    static const uint8_t kPrologue[]={0x48,0x89,0x5C,0x24,0x08,0x48,0x89,0x74,0x24,0x20,0x55,0x57,0x41,0x56};
    const uintptr_t exe=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    if(std::memcmp(reinterpret_cast<const void*>(exe+kLookupRva),kPrologue,sizeof(kPrologue))!=0){Log("[proxy] the lookup is not where it was: another game version\n");return;}
    void* system=*reinterpret_cast<void**>(exe+kSystemRva);
    if(!system){Log("[proxy] no physics system\n");return;}
    uintptr_t handle[2]={0,0};
    reinterpret_cast<void*(*)(void*,uintptr_t*,uint32_t)>(exe+kLookupRva)(system,handle,uint32_t(proxy));
    const uintptr_t object=handle[0];
    if(object && Readable(object,0x40)) {
        // +0x34 the kind (physics::ProxyType). +0x35 read 6 for every prop so far, physics::PhysicalSystemOwner::InstancedDestructibleNode if that is what it is.
        static const char* const kOwners[]={"Unknown","BakedDestructionNode","ClothMeshNode","CollisionAreaNode","DecorationMeshNode","DynamicMeshNode","InstancedDestructibleNode",
            "PhysicalDestructionNode","PhysicalTriggerNode","StaticMeshNode","TerrainCollisionNode","WaterPatchNode","WorldCollisionNode","BakedDestructionComponent","ClothComponent",
            "ColliderComponent","PhysicalDestructionComponent","PhysicalMeshComponent","PhysicalSkinnedMeshComponent","PhysicalTriggerComponent","SimpleColliderComponent",
            "SkinnedClothComponent","StateMachineComponent","VehicleChassisComponent","PhysicalParticleSystem","PhotoModeSystem","RagdollBinder","FoliageDestruction","EntityProxy"};
        const unsigned owner=*reinterpret_cast<uint8_t*>(object+0x35);
        Log("[proxy] %d: kind %u, owner byte %u (%s)\n",proxy,unsigned(*reinterpret_cast<uint8_t*>(object+0x34)),owner,owner<29?kOwners[owner]:"?");
    }
    reinterpret_cast<void(*)(uintptr_t*)>(exe+kReleaseRva)(handle);
}
void ProbeGuarded(int32_t proxy) {
    __try {
        ProbeProxy(proxy);
    } __except(EXCEPTION_EXECUTE_HANDLER) {
        Log("[proxy] an exception looking at it\n");
    }
}

// The body's actor by what the game writes in its userData (the proxy at +0x10, the body at +0x14 of it): where PhysX has it, against where the game says.
void* FindActorIn(void** list,uint32_t count,int32_t proxy,int32_t body) {
    for(uint32_t i=0;i<count;++i) {
        const char* a=static_cast<const char*>(list[i]);
        if(*reinterpret_cast<const uint32_t*>(a+0x10)==uint32_t(proxy) && *reinterpret_cast<const uint16_t*>(a+0x14)==uint16_t(body))return list[i];
    }
    return nullptr;
}
void ProbeBodyInner(int32_t proxy,int32_t body,const Vec3& world) {
    if(proved<0 || !ReadScenes())return;
    for(int s=0;s<sceneCount;++s) {
        void* found=nullptr;
        const uint32_t aggregates=Slot<uint32_t(*)(void*)>(scenes[s],kSceneGetNbAggregates)(scenes[s]);
        const uint32_t gotA=aggregates<=kMaxActors?Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(scenes[s],kSceneGetAggregates)(scenes[s],aggregateBuffer,kMaxActors,0):0;
        for(uint32_t i=0;i<gotA && !found;++i) {
            void* members[64]{};
            const uint32_t n=Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(aggregateBuffer[i],kAggregateGetActors)(aggregateBuffer[i],members,64,0);
            found=FindActorIn(members,n<64?n:64,proxy,body);
        }
        const char* where="an aggregate";
        if(!found) {
            const uint16_t types=1|2;
            const uint32_t got=Slot<uint32_t(*)(void*,const uint16_t*,void**,uint32_t,uint32_t)>(scenes[s],kSceneGetActors)(scenes[s],&types,actorBuffer,kMaxActors,0);
            found=FindActorIn(actorBuffer,got,proxy,body);
            where="the scene's list";
        }
        if(found) {
            Transform pose{};
            Slot<void*(*)(void*,Transform*)>(found,kActorGetGlobalPose)(found,&pose);
            Log("[debugview] body %d/%d is an actor of scene %d in %s: PhysX has it at (%.2f %.2f %.2f), the game at (%.2f %.2f %.2f), so the origin is (%.2f %.2f %.2f)\n",
                proxy,body,s,where,pose.p.x,pose.p.y,pose.p.z,world.x,world.y,world.z,world.x-pose.p.x,world.y-pose.p.y,world.z-pose.p.z);
            return;
        }
    }
    Log("[debugview] body %d/%d is no actor of any scene\n",proxy,body);
}
bool RaiseSpinInner(int32_t proxy,int32_t body,float most,float* was) {
    // PxRigidDynamic's table and the two functions, read in this DLL (2026-11-04): the setter squares its argument, the getter takes the root.
    constexpr uintptr_t kDynamicTableRva=0x20B2E8,kSetRva=0x4BE90,kGetRva=0x4BF20;
    constexpr int kSetSlot=60,kGetSlot=61;
    if(proved<0 || !ReadScenes())return false;
    const uintptr_t dll=reinterpret_cast<uintptr_t>(GetModuleHandleA("PhysX3_x64.dll"));
    for(int s=0;s<sceneCount;++s) {
        void* found=nullptr;
        const uint32_t aggregates=Slot<uint32_t(*)(void*)>(scenes[s],kSceneGetNbAggregates)(scenes[s]);
        const uint32_t gotA=aggregates<=kMaxActors?Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(scenes[s],kSceneGetAggregates)(scenes[s],aggregateBuffer,kMaxActors,0):0;
        for(uint32_t i=0;i<gotA && !found;++i) {
            void* members[64]{};
            const uint32_t n=Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(aggregateBuffer[i],kAggregateGetActors)(aggregateBuffer[i],members,64,0);
            found=FindActorIn(members,n<64?n:64,proxy,body);
        }
        if(!found) {
            const uint16_t types=1|2;
            const uint32_t got=Slot<uint32_t(*)(void*,const uint16_t*,void**,uint32_t,uint32_t)>(scenes[s],kSceneGetActors)(scenes[s],&types,actorBuffer,kMaxActors,0);
            found=FindActorIn(actorBuffer,got,proxy,body);
        }
        if(!found)continue;
        void** table=*reinterpret_cast<void***>(found);
        if(reinterpret_cast<uintptr_t>(table)-dll!=kDynamicTableRva || reinterpret_cast<uintptr_t>(table[kSetSlot])-dll!=kSetRva || reinterpret_cast<uintptr_t>(table[kGetSlot])-dll!=kGetRva)return false;
        *was=reinterpret_cast<float(*)(void*)>(table[kGetSlot])(found);
        reinterpret_cast<void(*)(void*,float)>(table[kSetSlot])(found,most);
        return true;
    }
    return false;
}
// ---- A GUN AS BOXES OF ITS OWN SHAPE, against the world's own shapes. Rays from the wrist to a box's corners see nothing that stands between them; this asks PhysX how far each box is
// inside each shape near it, PxGeometryQuery::computePenetration, exported by PhysX3Common_x64.dll. No scene query is involved, so it does not matter that this game's overlap answers nothing.
//
// WALKING 15000 ACTORS A FRAME WOULD SHOW, so the near list is built a slice at a time, a cycle taking a few frames, and only statics: what is loose gets pushed, not stopped at. It holds copies of
// the geometry, whose mesh pointers live as long as their sector: so it is thrown away when the gun jumps or the calls have stopped, as they do over a loading screen.
constexpr int kGunNear=2048,kGunSlice=1500,kGunMostBoxes=12,kGunActorShapes=64,kGunMostTests=1024,kGunPasses=3;
constexpr uint32_t kGunActorMostShapes=8192;
uint32_t gunShapesMost=0,gunTethered=0,gunGaveUp=0;
Vec3 gunAnchor{};
bool gunAnchorOn=false;
constexpr uintptr_t kShapeGetSimFilterRva=0x66a80,kShapeGetQueryFilterRva=0x66ae0,kShapeGetFlagsRva=0x67200;
constexpr int kShapeGetHeightField=16,kShapeGetSimFilter=21,kShapeGetQueryFilter=23,kShapeGetFlags=34;
struct Bounds {Vec3 lo,hi;};
struct NearShape {alignas(8) uint8_t geometry[64];Transform pose;Bounds bounds;Vec3 origin;uint32_t query[4];uint32_t sim[4];uint8_t flags;int type;};
struct GunBox {Vec3 center,half;};
NearShape gunNear[kGunNear],gunNext[kGunNear];
int gunNearCount=0,gunNextCount=0;
bool gunReady=false,gunNextFull=false;
GunBox gunBoxes[kGunMostBoxes];
int gunBoxCount=0;
float gunRadius=.5f;
int gunScene=-1;
uint32_t gunStart=0;
uint64_t gunLastCall=0;
Vec3 gunLastCenter{};
int gunProved=0;
using PenetrationFn=bool(*)(Vec3*,float*,const void*,const void*,const void*,const void*);
using BoundsFn=Bounds(*)(const void*,const void*,float);
// (unit direction, distance, the gun's box and pose, the shape and its pose, the hit, flags by address (PxFlags has a copy constructor, read from the function), inflation)
using SweepFn=bool(*)(const Vec3*,float,const void*,const void*,const void*,const void*,void*,const uint16_t*,float);
// The hit's layout differs from the headers (the normal is not at +36, measured): the first answer is searched for three floats of length one, with the distance after them, and that place is kept.
struct SweepHit {uint8_t bytes[128];};
int gunHitNormalAt=-1;
SweepFn gunSweep=nullptr;
Vec3 gunPrev{};
bool gunPrevOn=false;
bool gunSweepBad=false;
PenetrationFn gunPenetration=nullptr;
BoundsFn gunBounds=nullptr;
int gunSaid=0;

void GunForget() {gunNearCount=gunNextCount=0;gunReady=false;gunNextFull=false;gunScene=-1;gunStart=0;gunPrevOn=false;}
bool GunProveShape(void* shape) {
    if(gunProved)return gunProved>0;
    const uintptr_t dll=reinterpret_cast<uintptr_t>(GetModuleHandleA("PhysX3_x64.dll"));
    void** table=*reinterpret_cast<void***>(shape);
    const char* name=Slot<const char*(*)(void*)>(shape,kBaseGetConcreteTypeName)(shape);
    const auto at=[&](int slot){return reinterpret_cast<uintptr_t>(table[slot])-dll;};
    HMODULE common=GetModuleHandleA("PhysX3Common_x64.dll");
    gunPenetration=common?reinterpret_cast<PenetrationFn>(GetProcAddress(common,"?computePenetration@PxGeometryQuery@physx@@SA_NAEAVPxVec3@2@AEAMAEBVPxGeometry@2@AEBVPxTransform@2@23@Z")):nullptr;
    gunBounds=common?reinterpret_cast<BoundsFn>(GetProcAddress(common,"?getWorldBounds@PxGeometryQuery@physx@@SA?AVPxBounds3@2@AEBVPxGeometry@2@AEBVPxTransform@2@M@Z")):nullptr;
    gunSweep=common?reinterpret_cast<SweepFn>(GetProcAddress(common,"?sweep@PxGeometryQuery@physx@@SA_NAEBVPxVec3@2@MAEBVPxGeometry@2@AEBVPxTransform@2@12AEAUPxSweepHit@2@V?$PxFlags@W4Enum@PxHitFlag@physx@@G@2@M@Z")):nullptr;
    if(!gunSweep)Log("[gunshape] no sweep: the gun is pushed out of what it is in\n");
    if(!dll || !name || std::strncmp(name,"PxShape",7)!=0 || at(kShapeGetLocalPose)!=kShapeGetLocalPoseRva || at(kShapeGetGeometryType)!=kShapeGetGeometryTypeRva ||
       at(kShapeGetSimFilter)!=kShapeGetSimFilterRva || at(kShapeGetQueryFilter)!=kShapeGetQueryFilterRva || at(kShapeGetFlags)!=kShapeGetFlagsRva || !gunPenetration || !gunBounds) {
        gunProved=-1;
        Log("[gunshape] this is not the PhysX these slots were read from (penetration %p, bounds %p): the gun stays on its rays\n",gunPenetration,gunBounds);
        return false;
    }
    gunProved=1;
    return true;
}
void GunAddActor(void* actor,const Vec3& center,float radius,const Vec3& origin) {
    Bounds whole{};
    Slot<void*(*)(void*,void*,float)>(actor,kActorGetWorldBounds)(actor,&whole,1.01f);
    const Vec3 c{center.x-origin.x,center.y-origin.y,center.z-origin.z};
    const float dx=c.x<whole.lo.x?whole.lo.x-c.x:(c.x>whole.hi.x?c.x-whole.hi.x:0.f);
    const float dy=c.y<whole.lo.y?whole.lo.y-c.y:(c.y>whole.hi.y?c.y-whole.hi.y:0.f);
    const float dz=c.z<whole.lo.z?whole.lo.z-c.z:(c.z>whole.hi.z?c.z-whole.hi.z:0.f);
    if(!(dx*dx+dy*dy+dz*dz<=radius*radius))return;
    Transform global{};
    Slot<void*(*)(void*,Transform*)>(actor,kActorGetGlobalPose)(actor,&global);
    // AN ACTOR CAN HOLD HUNDREDS OF SHAPES, a stretch of baked world for one, and only what lies near is kept. Asked for the first 64 alone, the gun went through the walls among the rest.
    for(uint32_t start=0;start<kGunActorMostShapes;) {
    void* shapes[kGunActorShapes]{};
    const uint32_t n=Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(actor,kActorGetShapes)(actor,shapes,kGunActorShapes,start);
    start+=n;
    if(start>gunShapesMost)gunShapesMost=start;
    for(uint32_t i=0;i<n && i<kGunActorShapes;++i) {
        void* shape=shapes[i];
        if(!GunProveShape(shape))return;
        uint8_t flags=0;
        Slot<void*(*)(void*,uint8_t*)>(shape,kShapeGetFlags)(shape,&flags);
        if((flags&4) || !(flags&3))continue;     // a trigger, or nothing at all
        if(gunNextCount>=kGunNear) {gunNextFull=true;return;}
        NearShape& s=gunNext[gunNextCount];
        s.type=Slot<int(*)(void*)>(shape,kShapeGetGeometryType)(shape);
        bool got=false;
        switch(s.type) {
        case GeomBox: got=Slot<bool(*)(void*,void*)>(shape,kShapeGetBox)(shape,s.geometry);break;
        case GeomSphere: got=Slot<bool(*)(void*,void*)>(shape,kShapeGetSphere)(shape,s.geometry);break;
        case GeomCapsule: got=Slot<bool(*)(void*,void*)>(shape,kShapeGetCapsule)(shape,s.geometry);break;
        case GeomConvex: got=Slot<bool(*)(void*,void*)>(shape,kShapeGetConvex)(shape,s.geometry);break;
        case GeomTriangles: got=Slot<bool(*)(void*,void*)>(shape,kShapeGetTriangles)(shape,s.geometry);break;
        case GeomHeightField: got=Slot<bool(*)(void*,void*)>(shape,kShapeGetHeightField)(shape,s.geometry);break;
        default:break;
        }
        if(!got)continue;
        Transform local{};
        Slot<void*(*)(void*,Transform*)>(shape,kShapeGetLocalPose)(shape,&local);
        s.pose=Transform{Multiply(global.q,local.q),Apply(global,local.p)};
        s.bounds=gunBounds(s.geometry,&s.pose,0.f);
        {
            const float sx=c.x<s.bounds.lo.x?s.bounds.lo.x-c.x:(c.x>s.bounds.hi.x?c.x-s.bounds.hi.x:0.f);
            const float sy=c.y<s.bounds.lo.y?s.bounds.lo.y-c.y:(c.y>s.bounds.hi.y?c.y-s.bounds.hi.y:0.f);
            const float sz=c.z<s.bounds.lo.z?s.bounds.lo.z-c.z:(c.z>s.bounds.hi.z?c.z-s.bounds.hi.z:0.f);
            if(!(sx*sx+sy*sy+sz*sz<=radius*radius))continue;
        }
        s.origin=origin;
        s.flags=flags;
        Slot<void*(*)(void*,uint32_t*)>(shape,kShapeGetQueryFilter)(shape,s.query);
        Slot<void*(*)(void*,uint32_t*)>(shape,kShapeGetSimFilter)(shape,s.sim);
        ++gunNextCount;
    }
    if(n<kGunActorShapes)break;
    }
}
// A slice of the near list. Scenes are read anew each time: the game drops them.
void GunScanSlice(const Vec3& center) {
    if(proved<0 || gunProved<0 || !ReadScenes())return;
    if(gunScene<0) {gunScene=0;gunStart=0;gunNextCount=0;gunNextFull=false;}
    const float radius=gunRadius+1.5f;
    while(gunScene<sceneCount && !sceneOwners[gunScene]) {++gunScene;gunStart=0;}
    if(gunScene<sceneCount) {
        void* scene=scenes[gunScene];
        const float* o=reinterpret_cast<const float*>(static_cast<char*>(sceneOwners[gunScene])+0x134);
        const Vec3 origin{o[0],o[1],o[2]};
        const uint16_t statics=1;
        const uint32_t got=Slot<uint32_t(*)(void*,const uint16_t*,void**,uint32_t,uint32_t)>(scene,kSceneGetActors)(scene,&statics,actorBuffer,kGunSlice,gunStart);
        for(uint32_t i=0;i<got;++i)GunAddActor(actorBuffer[i],center,radius,origin);
        gunStart+=got;
        if(got<kGunSlice) {++gunScene;gunStart=0;}
    }
    if(gunScene>=sceneCount) {
        std::memcpy(gunNear,gunNext,sizeof(NearShape)*size_t(gunNextCount));
        gunNearCount=gunNextCount;
        gunReady=gunProved>0;
        if(gunNextFull && gunSaid<3) {++gunSaid;Log("[gunshape] more than %d shapes near\n",kGunNear);}
        gunScene=-1;
    }
}
// How far the gun has to go to be out of everything near it, in the world.
bool GunQueryInner(const Vec3& at,const Quat& turn,Vec3* push) {
    *push={0,0,0};
    int tests=0;
    const Transform gun{turn,at};
    for(int pass=0;pass<kGunPasses;++pass) {
        bool moved=false;
        for(int b=0;b<gunBoxCount;++b) {
            const GunBox& box=gunBoxes[b];
            const Vec3 middle=Apply(gun,box.center);
            // the box's own world bounds
            const Vec3 ax=Rotate(turn,{box.half.x,0,0}),ay=Rotate(turn,{0,box.half.y,0}),az=Rotate(turn,{0,0,box.half.z});
            const Vec3 extent{std::fabs(ax.x)+std::fabs(ay.x)+std::fabs(az.x),std::fabs(ax.y)+std::fabs(ay.y)+std::fabs(az.y),std::fabs(ax.z)+std::fabs(ay.z)+std::fabs(az.z)};
            const BoxGeometry geometry{GeomBox,box.half};
            for(int i=0;i<gunNearCount;++i) {
                const NearShape& s=gunNear[i];
                const Vec3 p{middle.x+push->x-s.origin.x,middle.y+push->y-s.origin.y,middle.z+push->z-s.origin.z};
                if(p.x+extent.x<s.bounds.lo.x || p.x-extent.x>s.bounds.hi.x || p.y+extent.y<s.bounds.lo.y || p.y-extent.y>s.bounds.hi.y ||
                   p.z+extent.z<s.bounds.lo.z || p.z-extent.z>s.bounds.hi.z)continue;
                if(++tests>kGunMostTests)return true;
                const Transform pose{turn,p};
                Vec3 direction{};
                float depth=0;
                if(!gunPenetration(&direction,&depth,&geometry,&pose,s.geometry,&s.pose) || !(depth>0.f) || !std::isfinite(depth))continue;
                push->x+=direction.x*depth;push->y+=direction.y*depth;push->z+=direction.z*depth;
                moved=true;
                if(gunSaid<40) {
                    ++gunSaid;
                    Log("[gunshape] box %d in a shape, geometry %d, flags %u, %.3f m, query %08x %08x %08x %08x, sim %08x %08x %08x %08x\n",b,s.type,unsigned(s.flags),depth,
                        s.query[0],s.query[1],s.query[2],s.query[3],s.sim[0],s.sim[1],s.sim[2],s.sim[3]);
                }
            }
        }
        if(!moved)break;
    }
    return true;
}
// THE WRIST'S SIDE OF THE WALL. Each box's middle has nothing between it and the wrist that holds it, or it has got behind that: by a turn pushed out at the far side of a thin wall, or by the hand going too far from
// where the gun stopped. It comes back to the near side, and the next sweep starts from there.
void GunTether(Vec3& pos,const Quat& turn) {
    if(!gunAnchorOn || !gunSweep || gunHitNormalAt<0)return;
    constexpr float kThin=.003f,kBack=.012f;
    constexpr uint16_t kFlags=1|2|4|16|128|256;
    const BoxGeometry thin{GeomBox,{kThin,kThin,kThin}};
    const Quat none{0,0,0,1};
    int tests=0;
    for(int b=0;b<gunBoxCount && tests<kGunMostTests;++b) {
        const Vec3 middle=Apply(Transform{turn,pos},gunBoxes[b].center);
        const Vec3 way{middle.x-gunAnchor.x,middle.y-gunAnchor.y,middle.z-gunAnchor.z};
        const float length=std::sqrt(way.x*way.x+way.y*way.y+way.z*way.z);
        if(length<.03f)continue;
        const Vec3 dir{way.x/length,way.y/length,way.z/length};
        float best=length;
        bool hit=false;
        for(int i=0;i<gunNearCount && tests<kGunMostTests;++i) {
            const NearShape& s=gunNear[i];
            const Vec3 p{gunAnchor.x-s.origin.x,gunAnchor.y-s.origin.y,gunAnchor.z-s.origin.z};
            const Vec3 q{p.x+way.x,p.y+way.y,p.z+way.z};
            if((p.x<q.x?p.x:q.x)-kThin>s.bounds.hi.x || (p.x>q.x?p.x:q.x)+kThin<s.bounds.lo.x || (p.y<q.y?p.y:q.y)-kThin>s.bounds.hi.y || (p.y>q.y?p.y:q.y)+kThin<s.bounds.lo.y ||
               (p.z<q.z?p.z:q.z)-kThin>s.bounds.hi.z || (p.z>q.z?p.z:q.z)+kThin<s.bounds.lo.z)continue;
            ++tests;
            const Transform pose{none,p};
            SweepHit h{};
            if(!gunSweep(&dir,best,&thin,&pose,s.geometry,&s.pose,&h,&kFlags,0.f))continue;
            const float* f=reinterpret_cast<const float*>(h.bytes+gunHitNormalAt);
            if(f[3]>=0.f && f[3]<=best && std::isfinite(f[3])) {best=f[3];hit=true;}
        }
        if(hit) {
            const float back=length-best+kBack;
            pos={pos.x-dir.x*back,pos.y-dir.y*back,pos.z-dir.z*back};
            ++gunTethered;
        }
    }
}
// THE GUN IS MOVED, FROM WHERE IT WAS TO WHERE THE HAND WANTS IT, and stopped and slid along what is in the way. Asked only how to get out of a shape, as above, the answer is the nearest way out, and
// that is the far side of a crate once the hand has gone down far enough. This never crosses a surface, as a game's character controller does not. What it turned into this frame is still pushed out,
// which is a small thing; and when the hand has gone further from it than an arm should, the gun comes, on the wrist's side of what is there. At .6 m it came straight through: a rifle held out at a wall reaches that
// before the wrist has got to the wall, and then only the hand stopped.
bool GunMove(const Vec3& target,const Quat& turn,Vec3* push) {
    constexpr float kSkin=.002f,kGiveUp=1.f;
    constexpr uint16_t kFlags=1|2|4|16|128|256;   // position, normal, distance, assume no overlap at the start, both sides of a mesh, precise
    *push={0,0,0};
    if(!gunPrevOn || !gunSweep || gunSweepBad) {
        GunQueryInner(target,turn,push);
        gunPrev={target.x+push->x,target.y+push->y,target.z+push->z};
        GunTether(gunPrev,turn);
        *push={gunPrev.x-target.x,gunPrev.y-target.y,gunPrev.z-target.z};
        gunPrevOn=true;
        return true;
    }
    Vec3 pos=gunPrev;
    Vec3 fix{};
    GunQueryInner(pos,turn,&fix);
    pos={pos.x+fix.x,pos.y+fix.y,pos.z+fix.z};
    Vec3 left{target.x-pos.x,target.y-pos.y,target.z-pos.z};
    int tests=0;
    for(int slide=0;slide<3;++slide) {
        const float dist=std::sqrt(left.x*left.x+left.y*left.y+left.z*left.z);
        if(dist<1e-4f)break;
        const Vec3 dir{left.x/dist,left.y/dist,left.z/dist};
        float best=dist;
        Vec3 normal{};
        bool hit=false;
        const Transform gun{turn,pos};
        for(int b=0;b<gunBoxCount && tests<kGunMostTests;++b) {
            const GunBox& box=gunBoxes[b];
            const Vec3 middle=Apply(gun,box.center);
            const Vec3 ax=Rotate(turn,{box.half.x,0,0}),ay=Rotate(turn,{0,box.half.y,0}),az=Rotate(turn,{0,0,box.half.z});
            const Vec3 extent{std::fabs(ax.x)+std::fabs(ay.x)+std::fabs(az.x),std::fabs(ax.y)+std::fabs(ay.y)+std::fabs(az.y),std::fabs(ax.z)+std::fabs(ay.z)+std::fabs(az.z)};
            const BoxGeometry geometry{GeomBox,box.half};
            for(int i=0;i<gunNearCount && tests<kGunMostTests;++i) {
                const NearShape& s=gunNear[i];
                const Vec3 p{middle.x-s.origin.x,middle.y-s.origin.y,middle.z-s.origin.z};
                const Vec3 q{p.x+left.x,p.y+left.y,p.z+left.z};
                if((p.x<q.x?p.x:q.x)-extent.x>s.bounds.hi.x || (p.x>q.x?p.x:q.x)+extent.x<s.bounds.lo.x || (p.y<q.y?p.y:q.y)-extent.y>s.bounds.hi.y || (p.y>q.y?p.y:q.y)+extent.y<s.bounds.lo.y ||
                   (p.z<q.z?p.z:q.z)-extent.z>s.bounds.hi.z || (p.z>q.z?p.z:q.z)+extent.z<s.bounds.lo.z)continue;
                ++tests;
                const Transform pose{turn,p};
                SweepHit h{};
                if(!gunSweep(&dir,best,&geometry,&pose,s.geometry,&s.pose,&h,&kFlags,0.f))continue;
                if(gunHitNormalAt<0) {
                    for(int at=16;at<=64 && gunHitNormalAt<0;at+=4) {
                        const float* f=reinterpret_cast<const float*>(h.bytes+at);
                        const float size=f[0]*f[0]+f[1]*f[1]+f[2]*f[2];
                        if(size>.99f && size<1.01f && f[3]>=0.f && f[3]<=best+1e-3f)gunHitNormalAt=at;
                    }
                    Log("[gunshape] a sweep's normal is at +%d\n",gunHitNormalAt);
                    if(gunHitNormalAt<0) {
                        gunSweepBad=true;
                        return GunQueryInner(target,turn,push);
                    }
                }
                const float* f=reinterpret_cast<const float*>(h.bytes+gunHitNormalAt);
                const float size=f[0]*f[0]+f[1]*f[1]+f[2]*f[2];
                if(!(size>.9f && size<1.1f) || !(f[3]>=0.f) || !std::isfinite(f[3]))continue;
                if(f[3]<=best) {best=f[3];normal=Vec3{f[0],f[1],f[2]};hit=true;}
            }
        }
        if(!hit) {
            pos={pos.x+left.x,pos.y+left.y,pos.z+left.z};
            left={0,0,0};
            break;
        }
        const float go=best>kSkin?best-kSkin:0.f;
        pos={pos.x+dir.x*go,pos.y+dir.y*go,pos.z+dir.z*go};
        left={left.x-dir.x*go,left.y-dir.y*go,left.z-dir.z*go};
        const float into=left.x*normal.x+left.y*normal.y+left.z*normal.z;
        if(into<0.f)left={left.x-normal.x*into,left.y-normal.y*into,left.z-normal.z*into};
    }
    if((pos.x-target.x)*(pos.x-target.x)+(pos.y-target.y)*(pos.y-target.y)+(pos.z-target.z)*(pos.z-target.z)>kGiveUp*kGiveUp) {
        ++gunGaveUp;
        pos=target;
    }
    GunTether(pos,turn);
    *push={pos.x-target.x,pos.y-target.y,pos.z-target.z};
    gunPrev=pos;
    return true;
}
bool GunGuarded(const Vec3& at,const Quat& turn,const Vec3& center,Vec3* push) {
    __try {
        GunScanSlice(center);
        if(!gunReady)return false;
        return GunMove(at,turn,push);
    } __except(EXCEPTION_EXECUTE_HANDLER) {
        GunForget();
        return false;
    }
}
// WHAT THE GUN'S BOXES TOUCH OF WHAT IS LOOSE: asked of PhysX itself, every shape of every moving actor near, the same test as against the world. There are few of those (31 aggregates of 274 actors measured,
// against 16000 statics), so they are walked fresh each call and nothing is kept.
struct GunTouch {uint32_t proxy,body;Vec3 away;float depth;};
constexpr int kGunMostTouches=4;
GunTouch gunTouches[kGunMostTouches];
int gunTouchCount=0;
void GunTouchActor(void* actor,const Transform& gun,const Vec3& origin) {
    if(Slot<int(*)(void*)>(actor,kActorGetType)(actor)!=1)return;
    // MOST OF THEM ARE NOWHERE NEAR, and where an actor stands costs far less to ask than its bounds, which go through every shape. 4 m: what is further and still reaches the gun is no loose thing.
    {
        Transform place{};
        Slot<void*(*)(void*,Transform*)>(actor,kActorGetGlobalPose)(actor,&place);
        const float fx=place.p.x+origin.x-gun.p.x,fy=place.p.y+origin.y-gun.p.y,fz=place.p.z+origin.z-gun.p.z;
        if(fx*fx+fy*fy+fz*fz>16.f)return;
    }
    Bounds whole{};
    Slot<void*(*)(void*,void*,float)>(actor,kActorGetWorldBounds)(actor,&whole,1.01f);
    const Quat turn=gun.q;
    for(int b=0;b<gunBoxCount;++b) {
        const GunBox& box=gunBoxes[b];
        const Vec3 middle=Apply(gun,box.center);
        const Vec3 ax=Rotate(turn,{box.half.x,0,0}),ay=Rotate(turn,{0,box.half.y,0}),az=Rotate(turn,{0,0,box.half.z});
        const Vec3 extent{std::fabs(ax.x)+std::fabs(ay.x)+std::fabs(az.x),std::fabs(ax.y)+std::fabs(ay.y)+std::fabs(az.y),std::fabs(ax.z)+std::fabs(ay.z)+std::fabs(az.z)};
        const Vec3 p{middle.x-origin.x,middle.y-origin.y,middle.z-origin.z};
        if(p.x+extent.x<whole.lo.x || p.x-extent.x>whole.hi.x || p.y+extent.y<whole.lo.y || p.y-extent.y>whole.hi.y || p.z+extent.z<whole.lo.z || p.z-extent.z>whole.hi.z)continue;
        void* shapes[kGunActorShapes]{};
        const uint32_t n=Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(actor,kActorGetShapes)(actor,shapes,kGunActorShapes,0);
        Transform global{};
        Slot<void*(*)(void*,Transform*)>(actor,kActorGetGlobalPose)(actor,&global);
        const BoxGeometry geometry{GeomBox,box.half};
        const Transform pose{turn,p};
        for(uint32_t i=0;i<n && i<kGunActorShapes;++i) {
            void* shape=shapes[i];
            uint8_t flags=0;
            Slot<void*(*)(void*,uint8_t*)>(shape,kShapeGetFlags)(shape,&flags);
            if((flags&4) || !(flags&1))continue;     // a trigger, or what does not collide
            uint8_t stored[64]{};
            const int type=Slot<int(*)(void*)>(shape,kShapeGetGeometryType)(shape);
            bool got=false;
            switch(type) {
            case GeomBox: got=Slot<bool(*)(void*,void*)>(shape,kShapeGetBox)(shape,stored);break;
            case GeomSphere: got=Slot<bool(*)(void*,void*)>(shape,kShapeGetSphere)(shape,stored);break;
            case GeomCapsule: got=Slot<bool(*)(void*,void*)>(shape,kShapeGetCapsule)(shape,stored);break;
            case GeomConvex: got=Slot<bool(*)(void*,void*)>(shape,kShapeGetConvex)(shape,stored);break;
            default:break;
            }
            if(!got)continue;
            Transform local{};
            Slot<void*(*)(void*,Transform*)>(shape,kShapeGetLocalPose)(shape,&local);
            const Transform shapePose{Multiply(global.q,local.q),Apply(global,local.p)};
            Vec3 direction{};
            float depth=0;
            if(!gunPenetration(&direction,&depth,&geometry,&pose,stored,&shapePose) || !(depth>0.f) || !std::isfinite(depth))continue;
            const char* a=static_cast<const char*>(actor);
            const uint32_t proxy=*reinterpret_cast<const uint32_t*>(a+0x10),body=*reinterpret_cast<const uint16_t*>(a+0x14);
            for(int t=0;t<gunTouchCount;++t)if(gunTouches[t].proxy==proxy && gunTouches[t].body==body)return;
            if(gunTouchCount<kGunMostTouches)gunTouches[gunTouchCount++]=GunTouch{proxy,body,direction,depth};
            return;
        }
    }
}
int GunTouchInner(const Vec3& at,const Quat& turn) {
    gunTouchCount=0;
    if(gunBoxCount==0 || gunProved<=0 || !ReadScenes())return 0;
    const Transform gun{turn,at};
    for(int s=0;s<sceneCount;++s) {
        if(!sceneOwners[s])continue;
        const float* o=reinterpret_cast<const float*>(static_cast<char*>(sceneOwners[s])+0x134);
        const Vec3 origin{o[0],o[1],o[2]};
        const uint16_t types=2;
        const uint32_t got=Slot<uint32_t(*)(void*,const uint16_t*,void**,uint32_t,uint32_t)>(scenes[s],kSceneGetActors)(scenes[s],&types,actorBuffer,kMaxActors,0);
        for(uint32_t i=0;i<got;++i)GunTouchActor(actorBuffer[i],gun,origin);
        const uint32_t aggregates=Slot<uint32_t(*)(void*)>(scenes[s],kSceneGetNbAggregates)(scenes[s]);
        const uint32_t gotA=aggregates<=kMaxActors?Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(scenes[s],kSceneGetAggregates)(scenes[s],aggregateBuffer,kMaxActors,0):0;
        for(uint32_t i=0;i<gotA;++i) {
            void* members[64]{};
            const uint32_t n=Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(aggregateBuffer[i],kAggregateGetActors)(aggregateBuffer[i],members,64,0);
            for(uint32_t m=0;m<n && m<64;++m)GunTouchActor(members[m],gun,origin);
        }
    }
    return gunTouchCount;
}
// WHAT A HAND POINTS AT: of the moving actors, the one whose middle is nearest the line of either direction, by angle, within the cone. Not a thin ray: small things on a shelf could hardly be hit. The one marked
// last is kept unless another is clearly nearer the line, or two neighbours would flicker.
int PickConeInner(const Vec3& from,const Vec3& dirA,const Vec3& dirB,float cone,float range,uint32_t keepProxy,uint32_t keepBody,NearBody* out,int capacity) {
    if(proved<0 || !ReadScenes())return 0;
    int count=0;
    const float cosCone=std::cos(cone);
    const auto consider=[&](void* actor,const Vec3& origin) {
        if(Slot<int(*)(void*)>(actor,kActorGetType)(actor)!=1)return;
        {
            Transform place{};
            Slot<void*(*)(void*,Transform*)>(actor,kActorGetGlobalPose)(actor,&place);
            const float fx=place.p.x+origin.x-from.x,fy=place.p.y+origin.y-from.y,fz=place.p.z+origin.z-from.z;
            if(fx*fx+fy*fy+fz*fz>(range+2.f)*(range+2.f))return;
        }
        struct {Vec3 lo,hi;} bounds{};
        Slot<void*(*)(void*,void*,float)>(actor,kActorGetWorldBounds)(actor,&bounds,1.01f);
        const Vec3 size{bounds.hi.x-bounds.lo.x,bounds.hi.y-bounds.lo.y,bounds.hi.z-bounds.lo.z};
        if(size.x>2.f || size.y>2.f || size.z>2.f)return;    // no loose thing to pull
        const Vec3 m{(bounds.lo.x+bounds.hi.x)*.5f+origin.x-from.x,(bounds.lo.y+bounds.hi.y)*.5f+origin.y-from.y,(bounds.lo.z+bounds.hi.z)*.5f+origin.z-from.z};
        const float d=std::sqrt(m.x*m.x+m.y*m.y+m.z*m.z);
        if(d>range || d<.15f)return;
        const float c1=(m.x*dirA.x+m.y*dirA.y+m.z*dirA.z)/d,c2=(m.x*dirB.x+m.y*dirB.y+m.z*dirB.z)/d;
        const float c=c1>c2?c1:c2;
        if(c<cosCone)return;
        const char* a=static_cast<const char*>(actor);
        const uint32_t proxy=*reinterpret_cast<const uint32_t*>(a+0x10),body=*reinterpret_cast<const uint16_t*>(a+0x14);
        float angle=std::acos(c<1.f?c:1.f);
        if(proxy==keepProxy && body==keepBody)angle-=.05f;
        NearBody n{proxy,body,angle,d};
        int at=count<capacity?count++:capacity-1;
        if(count==capacity && out[at].distance<n.distance)return;
        while(at>0 && out[at-1].distance>n.distance) {out[at]=out[at-1];--at;}
        out[at]=n;
    };
    for(int s=0;s<sceneCount;++s) {
        if(!sceneOwners[s])continue;
        const float* o=reinterpret_cast<const float*>(static_cast<char*>(sceneOwners[s])+0x134);
        const Vec3 origin{o[0],o[1],o[2]};
        const uint16_t types=2;
        const uint32_t got=Slot<uint32_t(*)(void*,const uint16_t*,void**,uint32_t,uint32_t)>(scenes[s],kSceneGetActors)(scenes[s],&types,actorBuffer,kMaxActors,0);
        for(uint32_t i=0;i<got;++i)consider(actorBuffer[i],origin);
        const uint32_t aggregates=Slot<uint32_t(*)(void*)>(scenes[s],kSceneGetNbAggregates)(scenes[s]);
        const uint32_t gotA=aggregates<=kMaxActors?Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(scenes[s],kSceneGetAggregates)(scenes[s],aggregateBuffer,kMaxActors,0):0;
        for(uint32_t i=0;i<gotA;++i) {
            void* members[64]{};
            const uint32_t n=Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(aggregateBuffer[i],kAggregateGetActors)(aggregateBuffer[i],members,64,0);
            for(uint32_t m=0;m<n && m<64;++m)consider(members[m],origin);
        }
    }
    return count;
}
// A body's real shape in the view's lines, in a colour.
bool OutlineInner(uint32_t proxy,uint32_t body,int color) {
    if(proved<0 || !ReadScenes())return false;
    for(int s=0;s<sceneCount;++s) {
        if(!sceneOwners[s])continue;
        const float* o=reinterpret_cast<const float*>(static_cast<char*>(sceneOwners[s])+0x134);
        drawOffset={o[0],o[1],o[2]};
        void* found=nullptr;
        const uint16_t types=2;
        const uint32_t got=Slot<uint32_t(*)(void*,const uint16_t*,void**,uint32_t,uint32_t)>(scenes[s],kSceneGetActors)(scenes[s],&types,actorBuffer,kMaxActors,0);
        found=FindActorIn(actorBuffer,got,int32_t(proxy),int32_t(body));
        if(!found) {
            const uint32_t aggregates=Slot<uint32_t(*)(void*)>(scenes[s],kSceneGetNbAggregates)(scenes[s]);
            const uint32_t gotA=aggregates<=kMaxActors?Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(scenes[s],kSceneGetAggregates)(scenes[s],aggregateBuffer,kMaxActors,0):0;
            for(uint32_t i=0;i<gotA && !found;++i) {
                void* members[64]{};
                const uint32_t n=Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(aggregateBuffer[i],kAggregateGetActors)(aggregateBuffer[i],members,64,0);
                found=FindActorIn(members,n<64?n:64,int32_t(proxy),int32_t(body));
            }
        }
        if(!found)continue;
        void* shapes[kMaxActorShapes]{};
        const uint32_t n=Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(found,kActorGetShapes)(found,shapes,kMaxActorShapes,0);
        gunOutlineColor=color;
        for(uint32_t i=0;i<n && i<kMaxActorShapes;++i) {
            OverlapHit hit{found,shapes[i],0,0};
            Transform pose{};
            Slot<void*(*)(void*,Transform*)>(found,kActorGetGlobalPose)(found,&pose);
            DrawHit(hit,pose.p,5.f);
        }
        gunOutlineColor=-1;
        return true;
    }
    return false;
}
// How many bodies PhysX has of a proxy, as rigid bodies; a ragdoll made of articulation links would be none.
int ProxyBodiesInner(uint32_t proxy) {
    if(proved<0 || !ReadScenes())return 0;
    int count=0,typeSeen=-1;
    const auto consider=[&](void* actor) {
        if(*reinterpret_cast<const uint32_t*>(static_cast<const char*>(actor)+0x10)==proxy) {++count;typeSeen=Slot<int(*)(void*)>(actor,kActorGetType)(actor);}
    };
    for(int s=0;s<sceneCount;++s) {
        if(!sceneOwners[s])continue;
        const uint16_t types=2;
        const uint32_t got=Slot<uint32_t(*)(void*,const uint16_t*,void**,uint32_t,uint32_t)>(scenes[s],kSceneGetActors)(scenes[s],&types,actorBuffer,kMaxActors,0);
        for(uint32_t i=0;i<got;++i)consider(actorBuffer[i]);
        const uint32_t aggregates=Slot<uint32_t(*)(void*)>(scenes[s],kSceneGetNbAggregates)(scenes[s]);
        const uint32_t gotA=aggregates<=kMaxActors?Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(scenes[s],kSceneGetAggregates)(scenes[s],aggregateBuffer,kMaxActors,0):0;
        for(uint32_t i=0;i<gotA;++i) {
            void* members[64]{};
            const uint32_t n=Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(aggregateBuffer[i],kAggregateGetActors)(aggregateBuffer[i],members,64,0);
            for(uint32_t m=0;m<n && m<64;++m)consider(members[m]);
        }
    }
    Log("[proxy] %u has %d bodies, actor type %d\n",proxy,count,typeSeen);
    return count;
}
int ProxyBodiesGuarded(uint32_t proxy) {
    __try {return ProxyBodiesInner(proxy);}
    __except(EXCEPTION_EXECUTE_HANDLER) {return 0;}
}
int GunTouchGuarded(const Vec3& at,const Quat& turn) {
    __try {return GunTouchInner(at,turn);}
    __except(EXCEPTION_EXECUTE_HANDLER) {gunTouchCount=0;return 0;}
}
int PickConeGuarded(const Vec3& from,const Vec3& a,const Vec3& b,float cone,float range,uint32_t kp,uint32_t kb,NearBody* out,int capacity) {
    __try {return PickConeInner(from,a,b,cone,range,kp,kb,out,capacity);}
    __except(EXCEPTION_EXECUTE_HANDLER) {return 0;}
}
bool OutlineGuarded(uint32_t proxy,uint32_t body,int color) {
    __try {return OutlineInner(proxy,body,color);}
    __except(EXCEPTION_EXECUTE_HANDLER) {gunOutlineColor=-1;return false;}
}
void ProbeBodyGuarded(int32_t proxy,int32_t body,const Vec3& world) {
    __try {
        ProbeBodyInner(proxy,body,world);
    } __except(EXCEPTION_EXECUTE_HANDLER) {
        Log("[debugview] an exception finding a body's actor\n");
    }
}

int FindNearInner(const Vec3& center,float radius,NearBody* out,int capacity) {
    if(proved<0 || !ReadScenes())return 0;
    int count=0;
    const auto consider=[&](void* actor) {
        if(Slot<int(*)(void*)>(actor,kActorGetType)(actor)!=1)return;   // only what is dynamic to PhysX
        {
            Transform place{};
            Slot<void*(*)(void*,Transform*)>(actor,kActorGetGlobalPose)(actor,&place);
            const float fx=place.p.x+drawOffset.x-center.x,fy=place.p.y+drawOffset.y-center.y,fz=place.p.z+drawOffset.z-center.z;
            if(fx*fx+fy*fy+fz*fz>(radius+4.f)*(radius+4.f))return;
        }
        struct {Vec3 lo,hi;} bounds{};
        Slot<void*(*)(void*,void*,float)>(actor,kActorGetWorldBounds)(actor,&bounds,1.01f);
        const Vec3 c{center.x-drawOffset.x,center.y-drawOffset.y,center.z-drawOffset.z};
        const float dx=c.x<bounds.lo.x?bounds.lo.x-c.x:(c.x>bounds.hi.x?c.x-bounds.hi.x:0.f);
        const float dy=c.y<bounds.lo.y?bounds.lo.y-c.y:(c.y>bounds.hi.y?c.y-bounds.hi.y:0.f);
        const float dz=c.z<bounds.lo.z?bounds.lo.z-c.z:(c.z>bounds.hi.z?c.z-bounds.hi.z:0.f);
        const float d2=dx*dx+dy*dy+dz*dz;
        if(!(d2<=radius*radius))return;
        const char* a=static_cast<const char*>(actor);
        NearBody n{*reinterpret_cast<const uint32_t*>(a+0x10),*reinterpret_cast<const uint16_t*>(a+0x14),std::sqrt(d2),
            (bounds.hi.x-bounds.lo.x)*(bounds.hi.y-bounds.lo.y)*(bounds.hi.z-bounds.lo.z)};
        int at=count<capacity?count++:capacity-1;
        if(count==capacity && out[at].distance<n.distance)return;
        while(at>0 && (out[at-1].distance>n.distance || (out[at-1].distance==n.distance && out[at-1].volume>n.volume))) {out[at]=out[at-1];--at;}
        out[at]=n;
    };
    for(int s=0;s<sceneCount;++s) {
        if(!sceneOwners[s])continue;
        const float* o=reinterpret_cast<const float*>(static_cast<char*>(sceneOwners[s])+0x134);
        drawOffset={o[0],o[1],o[2]};
        const uint16_t types=2;
        const uint32_t got=Slot<uint32_t(*)(void*,const uint16_t*,void**,uint32_t,uint32_t)>(scenes[s],kSceneGetActors)(scenes[s],&types,actorBuffer,kMaxActors,0);
        for(uint32_t i=0;i<got;++i)consider(actorBuffer[i]);
        const uint32_t aggregates=Slot<uint32_t(*)(void*)>(scenes[s],kSceneGetNbAggregates)(scenes[s]);
        const uint32_t gotA=aggregates<=kMaxActors?Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(scenes[s],kSceneGetAggregates)(scenes[s],aggregateBuffer,kMaxActors,0):0;
        for(uint32_t i=0;i<gotA;++i) {
            void* members[64]{};
            const uint32_t n=Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(aggregateBuffer[i],kAggregateGetActors)(aggregateBuffer[i],members,64,0);
            for(uint32_t m=0;m<n && m<64;++m)consider(members[m]);
        }
    }
    return count;
}

bool ActorWorldInner(uint32_t proxy,uint32_t body,float* xyz) {
    if(proved<0 || !ReadScenes())return false;
    for(int s=0;s<sceneCount;++s) {
        if(!sceneOwners[s])continue;
        void* found=nullptr;
        const uint16_t types=2;
        const uint32_t got=Slot<uint32_t(*)(void*,const uint16_t*,void**,uint32_t,uint32_t)>(scenes[s],kSceneGetActors)(scenes[s],&types,actorBuffer,kMaxActors,0);
        found=FindActorIn(actorBuffer,got,int32_t(proxy),int32_t(body));
        if(!found) {
            const uint32_t aggregates=Slot<uint32_t(*)(void*)>(scenes[s],kSceneGetNbAggregates)(scenes[s]);
            const uint32_t gotA=aggregates<=kMaxActors?Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(scenes[s],kSceneGetAggregates)(scenes[s],aggregateBuffer,kMaxActors,0):0;
            for(uint32_t i=0;i<gotA && !found;++i) {
                void* members[64]{};
                const uint32_t n=Slot<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(aggregateBuffer[i],kAggregateGetActors)(aggregateBuffer[i],members,64,0);
                found=FindActorIn(members,n<64?n:64,int32_t(proxy),int32_t(body));
            }
        }
        if(found) {
            Transform pose{};
            Slot<void*(*)(void*,Transform*)>(found,kActorGetGlobalPose)(found,&pose);
            const float* o=reinterpret_cast<const float*>(static_cast<char*>(sceneOwners[s])+0x134);
            xyz[0]=pose.p.x+o[0];xyz[1]=pose.p.y+o[1];xyz[2]=pose.p.z+o[2];
            return true;
        }
    }
    return false;
}

int ProxyKindInner(uint32_t proxy) {
    constexpr uintptr_t kSystemRva=0x3BFCDD0,kLookupRva=0x27D430,kReleaseRva=0x27D374;
    static const uint8_t kPrologue[]={0x48,0x89,0x5C,0x24,0x08,0x48,0x89,0x74,0x24,0x20,0x55,0x57,0x41,0x56};
    const uintptr_t exe=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    if(std::memcmp(reinterpret_cast<const void*>(exe+kLookupRva),kPrologue,sizeof(kPrologue))!=0)return 0;
    void* system=*reinterpret_cast<void**>(exe+kSystemRva);
    if(!system)return 0;
    uintptr_t handle[2]={0,0};
    reinterpret_cast<void*(*)(void*,uintptr_t*,uint32_t)>(exe+kLookupRva)(system,handle,proxy);
    int kind=0;
    if(handle[0] && Readable(handle[0],0x40))kind=*reinterpret_cast<uint8_t*>(handle[0]+0x34);
    reinterpret_cast<void(*)(uintptr_t*)>(exe+kReleaseRva)(handle);
    return kind;
}

int WorldGuarded(const Vec3& center,float radius) {
    __try {
        return WorldInner(center,radius);
    } __except(EXCEPTION_EXECUTE_HANDLER) {
        proved=-1;
        Log("[debugview] an exception reading PhysX: world shapes off\n");
        return -1;
    }
}

// A ray is sent once in many frames, in turn with the others. Each stays up a moment, or the view flickers while nothing is wrong.
constexpr int kMaxHeld=1024;
constexpr uint64_t kHeldMs=150;
Segment held[kMaxHeld];
uint64_t heldAt[kMaxHeld];
int heldNext=0;

inline void Cross(const float* v3,float a[3],float b[3],float c[3]) {
    float len=std::sqrt(v3[0]*v3[0]+v3[1]*v3[1]+v3[2]*v3[2]);
    if(len<1e-5f){a[0]=0;a[1]=0;a[2]=1;}else{a[0]=v3[0]/len;a[1]=v3[1]/len;a[2]=v3[2]/len;}
    const float h[3]={std::fabs(a[2])<.9f?0.f:1.f,0.f,std::fabs(a[2])<.9f?1.f:0.f};
    b[0]=a[1]*h[2]-a[2]*h[1];b[1]=a[2]*h[0]-a[0]*h[2];b[2]=a[0]*h[1]-a[1]*h[0];
    len=std::sqrt(b[0]*b[0]+b[1]*b[1]+b[2]*b[2]);
    b[0]/=len;b[1]/=len;b[2]/=len;
    c[0]=a[1]*b[2]-a[2]*b[1];c[1]=a[2]*b[0]-a[0]*b[2];c[2]=a[0]*b[1]-a[1]*b[0];
}
void WorldRing(const float* center,const float* u,const float* v,float radius,int color,float from,float to,int parts) {
    float last[3]{};
    for(int i=0;i<=parts;++i) {
        const float angle=from+(to-from)*float(i)/float(parts),c=std::cos(angle)*radius,s=std::sin(angle)*radius;
        float p[3];
        for(int k=0;k<3;++k)p[k]=center[k]+u[k]*c+v[k]*s;
        if(i)Add(last,p,color);
        std::memcpy(last,p,sizeof(p));
    }
}
}

int FindNear(float x,float y,float z,float radius,NearBody* out,int capacity) {
    __try {
        return FindNearInner(Vec3{x,y,z},radius,out,capacity);
    } __except(EXCEPTION_EXECUTE_HANDLER) {
        return 0;
    }
}
bool ActorWorld(uint32_t proxy,uint32_t body,float* xyz) {
    __try {
        return ActorWorldInner(proxy,body,xyz);
    } __except(EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}
int PickCone(const float* from,const float* a,const float* b,float cone,float range,uint32_t keepProxy,uint32_t keepBody,NearBody* out,int capacity) {
    return PickConeGuarded(Vec3{from[0],from[1],from[2]},Vec3{a[0],a[1],a[2]},Vec3{b[0],b[1],b[2]},cone,range,keepProxy,keepBody,out,capacity);
}
bool RaiseSpin(uint32_t proxy,uint32_t body,float most,float* was) {
    __try {return RaiseSpinInner(int32_t(proxy),int32_t(body),most,was);}
    __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
int ProxyBodies(uint32_t proxy) {return ProxyBodiesGuarded(proxy);}
int ProxyKindAsked(uint32_t proxy) {
    __try {
        return ProxyKindInner(proxy);
    } __except(EXCEPTION_EXECUTE_HANDLER) {
        return 0;
    }
}
// Asked of the game's table, which is shared with what the physics does; what a proxy is does not change from one moment to the next.
int ProxyKind(uint32_t proxy) {
    struct Seen {uint32_t proxy;int kind;uint64_t at;};
    static thread_local Seen seen[64]{};
    Seen& s=seen[proxy%64];
    const uint64_t now=GetTickCount64();
    if(s.proxy==proxy && now-s.at<(s.kind?20000u:1000u))return s.kind;
    s=Seen{proxy,ProxyKindAsked(proxy),now};
    return s.kind;
}

int Copy(Segment* out,int capacity) {
    std::lock_guard<std::mutex> guard(frontLock);
    if(!frontCount || GetTickCount64()-frontStamp>250)return 0;
    const int n=frontCount<capacity?frontCount:capacity;
    std::memcpy(out,front,sizeof(Segment)*size_t(n));
    return n;
}
}

using namespace cvr::debugview;

void VRDebugOn(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    stack->code++;
    if (out) *out = g_liveControls.xrDebugView != 0;
}

void VRDebugLine(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    RED4ext::Vector4 a{}, b{};
    int32_t color = 0;
    RED4ext::GetParameter(stack, &a);
    RED4ext::GetParameter(stack, &b);
    RED4ext::GetParameter(stack, &color);
    stack->code++;
    Add(&a.X, &b.X, color);
    if (out) *out = true;
}

void VRDebugRay(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    RED4ext::Vector4 a{}, b{};
    int32_t color = 0;
    RED4ext::GetParameter(stack, &a);
    RED4ext::GetParameter(stack, &b);
    RED4ext::GetParameter(stack, &color);
    stack->code++;
    Segment& s = held[heldNext];
    s.a[0] = a.X; s.a[1] = a.Y; s.a[2] = a.Z; s.b[0] = b.X; s.b[1] = b.Y; s.b[2] = b.Z; s.color = uint32_t(color);
    heldAt[heldNext] = GetTickCount64();
    heldNext = (heldNext + 1) % kMaxHeld;
    if (out) *out = true;
}

void VRDebugCapsule(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    RED4ext::Vector4 a{}, b{};
    float radius = 0.0f;
    int32_t color = 0;
    RED4ext::GetParameter(stack, &a);
    RED4ext::GetParameter(stack, &b);
    RED4ext::GetParameter(stack, &radius);
    RED4ext::GetParameter(stack, &color);
    stack->code++;
    const float pa[3] = {a.X, a.Y, a.Z}, pb[3] = {b.X, b.Y, b.Z};
    const float d[3] = {pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]};
    float x[3], y[3], z[3];
    Cross(d, x, y, z);
    WorldRing(pa, y, z, radius, color, 0.0f, 2 * kPi, 8);
    WorldRing(pb, y, z, radius, color, 0.0f, 2 * kPi, 8);
    for (int side = 0; side < 4; ++side) {
        const float* axis = side < 2 ? y : z;
        const float sign = side % 2 ? -radius : radius;
        const float s[3] = {pa[0] + axis[0] * sign, pa[1] + axis[1] * sign, pa[2] + axis[2] * sign};
        const float e[3] = {pb[0] + axis[0] * sign, pb[1] + axis[1] * sign, pb[2] + axis[2] * sign};
        Add(s, e, color);
    }
    WorldRing(pb, y, x, radius, color, 0.0f, kPi, 4);
    WorldRing(pb, z, x, radius, color, 0.0f, kPi, 4);
    WorldRing(pa, y, x, -radius, color, 0.0f, kPi, 4);
    WorldRing(pa, z, x, -radius, color, 0.0f, kPi, 4);
    if (out) *out = true;
}

void VRDebugPoint(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    RED4ext::Vector4 p{};
    float size = 0.01f;
    int32_t color = 0;
    RED4ext::GetParameter(stack, &p);
    RED4ext::GetParameter(stack, &size);
    RED4ext::GetParameter(stack, &color);
    stack->code++;
    for (int axis = 0; axis < 3; ++axis) {
        float s[3] = {p.X, p.Y, p.Z}, e[3] = {p.X, p.Y, p.Z};
        s[axis] -= size;
        e[axis] += size;
        Add(s, e, color);
    }
    if (out) *out = true;
}

// The real collision shapes within radius of center, in the game's world. Returns how many, or -1 when they are out of reach.
void VRDebugWorld(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, int32_t* out, int64_t) {
    RED4ext::Vector4 center{};
    float radius = 2.0f;
    RED4ext::GetParameter(stack, &center);
    RED4ext::GetParameter(stack, &radius);
    stack->code++;
    const int n = WorldGuarded(Vec3{center.X, center.Y, center.Z}, radius);
    if (out) *out = n;
}

// What a body in the hand is, for the log. A few times a session.
void VRDebugProbeBody(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    int32_t proxy = 0, body = 0;
    RED4ext::Vector4 world{};
    RED4ext::GetParameter(stack, &proxy);
    RED4ext::GetParameter(stack, &body);
    RED4ext::GetParameter(stack, &world);
    stack->code++;
    static int times = 0;
    if (times < 6) {
        ++times;
        ProbeGuarded(proxy);
        ProbeBodyGuarded(proxy, body, Vec3{world.X, world.Y, world.Z});
    }
    if (out) *out = true;
}

// Where PhysX has a body, in the world, w 1; w 0 if it has none. For measuring how far behind the game's own answer is.
void VRBodyPhysXPos(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, RED4ext::Vector4* out, int64_t) {
    int32_t proxy = 0, body = 0;
    RED4ext::GetParameter(stack, &proxy);
    RED4ext::GetParameter(stack, &body);
    stack->code++;
    if (!out) return;
    float xyz[3] = {0, 0, 0};
    const bool ok = ActorWorld(uint32_t(proxy), uint32_t(body), xyz);
    *out = RED4ext::Vector4{xyz[0], xyz[1], xyz[2], ok ? 1.0f : 0.0f};
}

// THE GUN'S SHAPE, boxes in its own frame, and how far it must go to be out of the world's. W 1 with an answer, 0 while there is none yet (the near list takes a few frames).
void VRGunBoxClear(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    stack->code++;
    gunBoxCount = 0;
    gunRadius = .3f;
    gunPrevOn = false;
    gunTouchCount = 0;
    if (out) *out = true;
}
void VRGunBoxAdd(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    RED4ext::Vector4 lo{}, hi{};
    RED4ext::GetParameter(stack, &lo);
    RED4ext::GetParameter(stack, &hi);
    stack->code++;
    bool added = false;
    if (gunBoxCount < kGunMostBoxes && hi.X > lo.X && hi.Y > lo.Y && hi.Z > lo.Z) {
        GunBox& b = gunBoxes[gunBoxCount++];
        b.center = {(lo.X + hi.X) * .5f, (lo.Y + hi.Y) * .5f, (lo.Z + hi.Z) * .5f};
        b.half = {(hi.X - lo.X) * .5f, (hi.Y - lo.Y) * .5f, (hi.Z - lo.Z) * .5f};
        const float reach = std::sqrt(b.center.x * b.center.x + b.center.y * b.center.y + b.center.z * b.center.z) +
            std::sqrt(b.half.x * b.half.x + b.half.y * b.half.y + b.half.z * b.half.z);
        if (reach > gunRadius) gunRadius = reach;
        added = true;
    }
    if (out) *out = added;
}
// VRGunAnchor(the wrist that holds it, world): what the gun must be on this side of.
void VRGunAnchor(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    RED4ext::Vector4 at{};
    RED4ext::GetParameter(stack, &at);
    stack->code++;
    gunAnchor = Vec3{at.X, at.Y, at.Z};
    gunAnchorOn = std::isfinite(at.X + at.Y + at.Z);
    if (out) *out = true;
}
static double gunTouchTotal = 0;
static uint32_t gunTouchCalls = 0;
void VRGunPush(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, RED4ext::Vector4* out, int64_t) {
    RED4ext::Vector4 at{}, turn{};
    RED4ext::GetParameter(stack, &at);
    RED4ext::GetParameter(stack, &turn);
    stack->code++;
    if (!out) return;
    *out = RED4ext::Vector4{0, 0, 0, 0};
    if (gunBoxCount == 0 || proved < 0 || gunProved < 0) return;
    LARGE_INTEGER t0, t1, f;
    QueryPerformanceCounter(&t0);
    const Vec3 center{at.X, at.Y, at.Z};
    const uint64_t now = GetTickCount64();
    const float dx = center.x - gunLastCenter.x, dy = center.y - gunLastCenter.y, dz = center.z - gunLastCenter.z;
    if (!gunLastCall || now - gunLastCall > 500 || dx * dx + dy * dy + dz * dz > 1.f) GunForget();
    gunLastCall = now;
    gunLastCenter = center;
    Vec3 push{};
    const bool ok = GunGuarded(center, Quat{turn.X, turn.Y, turn.Z, turn.W}, center, &push);
    // w 2: moved by sweeps, so it cannot have gone through anything and no ray is needed
    if (ok) *out = RED4ext::Vector4{push.x, push.y, push.z, gunPrevOn && gunSweep && !gunSweepBad ? 2.f : 1.f};
    QueryPerformanceCounter(&t1);
    QueryPerformanceFrequency(&f);
    static double total = 0, most = 0;
    static uint32_t calls = 0;
    static uint64_t saidAt = 0;
    const double ms = double(t1.QuadPart - t0.QuadPart) * 1000.0 / double(f.QuadPart);
    total += ms;
    most = ms > most ? ms : most;
    ++calls;
    if (now - saidAt > 10000) {
        Log("[gunshape] %d boxes, %d shapes near, %u calls, %.3f ms mean, %.3f most; loose things %.3f ms mean; brought back to the wrist's side %u, the hand too far from it %u\n", gunBoxCount, gunNearCount, calls, total / calls, most, gunTouchCalls ? gunTouchTotal / gunTouchCalls : 0.0, gunTethered, gunGaveUp);
        gunTethered = gunGaveUp = 0;
        gunTouchTotal = 0;
        gunTouchCalls = 0;
        saidAt = now;
        total = most = 0;
        calls = 0;
    }
    if (g_liveControls.xrDebugView) {
        // in the view: the boxes as they are after the push
        const Quat q{turn.X, turn.Y, turn.Z, turn.W};
        const Vec3 saved = drawOffset;
        drawOffset = {0, 0, 0};
        const Transform gun{q, {center.x + push.x, center.y + push.y, center.z + push.z}};
        for (int b = 0; b < gunBoxCount; ++b)
            BoxLines(Transform{q, Apply(gun, gunBoxes[b].center)}, gunBoxes[b].half, (push.x != 0 || push.y != 0 || push.z != 0) ? Red : Orange);
        drawOffset = saved;
    }
}

// VRGunTouches(at, turn) -> how many loose bodies the gun's boxes are in now; VRGunTouchOf(i) proxy, body; VRGunTouchWay(i) the way out of it, world, and how deep.
void VRGunTouches(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, int32_t* out, int64_t) {
    RED4ext::Vector4 at{}, turn{};
    RED4ext::GetParameter(stack, &at);
    RED4ext::GetParameter(stack, &turn);
    stack->code++;
    LARGE_INTEGER t0, t1, f;
    QueryPerformanceCounter(&t0);
    const int n = GunTouchGuarded(Vec3{at.X, at.Y, at.Z}, Quat{turn.X, turn.Y, turn.Z, turn.W});
    QueryPerformanceCounter(&t1);
    QueryPerformanceFrequency(&f);
    gunTouchTotal += double(t1.QuadPart - t0.QuadPart) * 1000.0 / double(f.QuadPart);
    ++gunTouchCalls;
    if (out) *out = n;
}
void VRGunTouchOf(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, RED4ext::Vector4* out, int64_t) {
    int32_t i = 0;
    RED4ext::GetParameter(stack, &i);
    stack->code++;
    if (!out) return;
    *out = RED4ext::Vector4{0, 0, 0, 0};
    if (i >= 0 && i < gunTouchCount) *out = RED4ext::Vector4{float(gunTouches[i].proxy), float(gunTouches[i].body), 0, 1};
}
void VRGunTouchWay(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, RED4ext::Vector4* out, int64_t) {
    int32_t i = 0;
    RED4ext::GetParameter(stack, &i);
    stack->code++;
    if (!out) return;
    *out = RED4ext::Vector4{0, 0, 0, 0};
    if (i >= 0 && i < gunTouchCount) *out = RED4ext::Vector4{gunTouches[i].away.x, gunTouches[i].away.y, gunTouches[i].away.z, gunTouches[i].depth};
}
void VRProxyBodies(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, int32_t* out, int64_t) {
    int32_t proxy = 0;
    RED4ext::GetParameter(stack, &proxy);
    stack->code++;
    const int n = ProxyBodiesGuarded(uint32_t(proxy));
    if (out) *out = n;
}
// VRThingOutline(proxy, body, colour): the body's real shape, in the view's lines. The caller flushes, whether the view is on or not.
void VRThingOutline(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    int32_t proxy = 0, body = 0, color = 0;
    RED4ext::GetParameter(stack, &proxy);
    RED4ext::GetParameter(stack, &body);
    RED4ext::GetParameter(stack, &color);
    stack->code++;
    const bool ok = OutlineGuarded(uint32_t(proxy), uint32_t(body), color);
    if (out) *out = ok;
}

void VRDebugFlush(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    stack->code++;
    const uint64_t now = GetTickCount64();
    for (int i = 0; i < kMaxHeld; ++i)
        if (heldAt[i] && now - heldAt[i] <= kHeldMs) Add(held[i].a, held[i].b, int(held[i].color));
    {
        std::lock_guard<std::mutex> guard(frontLock);
        std::memcpy(front, back, sizeof(Segment) * size_t(backCount));
        frontCount = backCount;
        frontStamp = GetTickCount64();
    }
    backCount = 0;
    if (out) *out = true;
}
