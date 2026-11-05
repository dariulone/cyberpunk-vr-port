#include "Anim/RigTrackMapping.hpp"
#include <cassert>
#include <limits>
int main() {
 float buf[]{99,88,1,0,77};
 volatile float read[4]{}, set[]{0,1,0,0}; volatile int on[]{1,1,0,0};
 const uint32_t map[]{0,2,1,3};
 assert(cvr::anim::ApplyRigTracks(map,2,2,buf,4,read,set,on,4));
 assert(buf[0]==99 && buf[1]==88 && buf[2]==0 && buf[3]==1 && buf[4]==77);
 assert(read[0]==1 && read[1]==0);
 const uint32_t bad[]{0,4,99,1};
 assert(!cvr::anim::ApplyRigTracks(bad,2,2,buf,4,read,set,on,4));
 assert(buf[4]==77 && buf[1]==88 && read[0]==-1);
 assert(!cvr::anim::ApplyRigTracks(nullptr,2,2,buf,4,read,set,on,4));
 assert(!cvr::anim::ApplyRigTracks(map,2,2,buf,0,read,set,on,4));
 const uint32_t single[]{1,3};set[1]=std::numeric_limits<float>::quiet_NaN();
 assert(cvr::anim::ApplyRigTracks(single,1,2,buf,4,read,set,on,4));
 assert(buf[3]==1 && read[0]==-1);
}
