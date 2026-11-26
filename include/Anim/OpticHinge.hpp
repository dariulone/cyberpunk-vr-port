#pragma once
#include <array>
#include <cstdint>
#include <cstring>
#include <span>

namespace cvr::anim::optic_hinge {
inline constexpr uint32_t Bone=8, BoneCount=13;
// Native Osprey rig referencePoseLS[8], confirmed by osprey_hinge_audit:
// only this rotation becomes identity in idlepose_ironsight (180 degree fold).
inline constexpr std::array<float,4> HipRotation{0,0,1,0};
inline constexpr uint64_t BoneNameHash=[] {uint64_t h=14695981039346656037ull;for(char c:"scope_hinge"){if(!c)break;h=(h^uint8_t(c))*1099511628211ull;}return h;}();
struct Identity {
    uintptr_t player{},weapon{},component{},object{},descriptor{},rig{},metaRig{},track{};
    uint64_t metaRigId{};
    uintptr_t graphComponent{},controlBinding{};
    bool operator==(const Identity&) const = default;
    bool Valid() const {return player&&weapon&&component&&object&&descriptor&&rig&&metaRig&&track&&graphComponent;}
};
struct Target {Identity identity;uint64_t stamp{};bool enabled{};};
struct Pass {
    Identity current;
    uintptr_t object{},descriptor{},rig{},track{},player{},metaRig{};
    uint64_t now{};
    bool context{},weakAlive{},legacyConflict{};
};
inline bool Matches(const Target& target,const Pass& pass) {
    return target.enabled&&target.identity.Valid()&&target.stamp&&pass.context&&pass.weakAlive&&!pass.legacyConflict&&
        pass.now>=target.stamp&&pass.now-target.stamp<200&&target.identity==pass.current&&
        pass.player==target.identity.player&&pass.object==target.identity.object&&
        pass.descriptor==target.identity.descriptor&&pass.rig==target.identity.rig&&pass.track==target.identity.track&&pass.metaRig==target.identity.metaRig;
}
// This is called only after the native pose copy. Do not fall back to LOD0 for
// an omitted bone: then the native call did not establish a fresh local value.
// Disabling leaves the next native copied rotation untouched; no stale restore.
inline bool Apply(const Target& target,const Pass& pass,std::span<const uint32_t> pairs,
                  uint8_t* bones,uint32_t slots,std::span<const uint64_t> destinationNames) {
    if(!Matches(target,pass)||!bones||pairs.size()%2||pairs.size()>BoneCount*2)return false;
    uint32_t destination=UINT32_MAX;
    for(size_t i=0;i<pairs.size();i+=2)if(pairs[i]==Bone){if(destination!=UINT32_MAX)return false;destination=pairs[i+1];}
    if(destination>=slots||destination>=destinationNames.size()||destinationNames[destination]!=BoneNameHash)return false;
    std::memcpy(bones+size_t(destination)*48+16,HipRotation.data(),sizeof(HipRotation));
    return true;
}
}

namespace cvr::anim {
// Called after the existing rig-write lock is released. Owns its bounded lock;
// transient strong engine handles are released outside it, with native SEH contained.
void ApplyOpticHinge(void* object,void* objectDescriptor,void** a1,void** a2,unsigned lod,
                     void* poseDescriptor,uint8_t* bones,uintptr_t track);
}
