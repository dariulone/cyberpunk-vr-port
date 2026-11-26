#pragma once
#include "Render/NativePacketInputs.hpp"
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <functional>

namespace cvr::stereo::packets {
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/eye_membership.rs (tools/single_pass_tests/eye_membership_test runs its cases
// against them in its Rust variant).
struct RustEyeMembership;struct RustBytes;
extern "C" {
bool cvr_em_key(const std::array<uint8_t,64>*,const uint8_t*,size_t,uint8_t*,size_t*);
bool cvr_em_identities(uint64_t,uint64_t,uint32_t,uint32_t,uint8_t*);
RustEyeMembership* cvr_em_new();
void cvr_em_free(RustEyeMembership*);
bool cvr_em_add(RustEyeMembership*,uint32_t,uint64_t,const Inputs*,const uint8_t*,size_t,uint32_t*);
bool cvr_em_plan(RustEyeMembership*,const std::array<uint8_t,64>*,const uint8_t*,size_t,uint32_t,uint8_t*,size_t,uint32_t*);
uint32_t cvr_em_known_extras(const RustEyeMembership*);
RustBytes* cvr_em_extras(RustEyeMembership*,const std::array<uint8_t,64>*,uint32_t);
const uint8_t* cvr_em_bytes(const RustBytes*,size_t*);
void cvr_em_bytes_free(RustBytes*);
}
#endif
// Address identities are a prospective matching domain, never instance bytes.
// Their users must compare owned GPU snapshots before accepting the peer result.
#if CVR_RUST_VRIK
inline bool GpuInstanceIdentities(uint64_t resource,uint64_t address,uint32_t count,uint32_t stride,std::vector<uint8_t>& output){
    if(!cvr_em_identities(resource,address,count,stride,nullptr))return false;
    std::vector<uint8_t> result(size_t(count)*stride);cvr_em_identities(resource,address,count,stride,result.data());
    output=std::move(result);return true;
}
#else
inline bool GpuInstanceIdentities(uint64_t resource,uint64_t address,uint32_t count,uint32_t stride,std::vector<uint8_t>& output){
    if(!resource || !address || stride!=48 || !count || count>32767 || address>UINT64_MAX-uint64_t(count)*stride)return false;
    std::vector<uint8_t> result(size_t(count)*stride);
    for(uint32_t i=0;i<count;++i){const uint64_t identity[]{resource,address+uint64_t(i)*stride};
        std::memcpy(result.data()+size_t(i)*stride,identity,sizeof(identity));}
    output=std::move(result);return true;
}
#endif
// Prospective geometry membership only. Materials, native IA bindings and camera
// constants must still match at the actual peer draw before accepting this plan.
#if CVR_RUST_VRIK
inline bool GeometryInstanceKey(std::array<uint8_t,64> draw,std::span<const uint8_t> instance,std::string& key){
    static_assert(sizeof(Inputs)==256,"mirrored by eye_membership.rs");
    std::array<uint8_t,128> bytes{};size_t length{};
    if(!cvr_em_key(&draw,instance.data(),instance.size(),bytes.data(),&length))return false;
    key.assign(reinterpret_cast<const char*>(bytes.data()),length);return true;
}
#else
inline bool GeometryInstanceKey(std::array<uint8_t,64> draw,std::span<const uint8_t> instance,std::string& key){
    if(draw[0] || (instance.size()!=48 && instance.size()!=64))return false;
    std::fill(draw.begin()+1,draw.begin()+8,uint8_t{});std::fill(draw.begin()+0x2d,draw.end(),uint8_t{});
    uint32_t indexOverride{};std::memcpy(&indexOverride,draw.data()+0x10,4);
    // 1F2B68 replaces ESI with descriptor+78 at 1F2D7E when the index override
    // handle is invalid. The unused draw+1C offset can contain stack bytes.
    if(!indexOverride || indexOverride>0x7fffffff)std::fill(draw.begin()+0x1c,draw.begin()+0x20,uint8_t{});
    key.assign(reinterpret_cast<const char*>(draw.data()),draw.size());
    key.append(reinterpret_cast<const char*>(instance.data()),instance.size());return true;
}
#endif
#if CVR_RUST_VRIK
class EyeMembership {
public:
    EyeMembership():rust_(cvr_em_new()){}
    ~EyeMembership(){cvr_em_free(rust_);}
    EyeMembership(const EyeMembership&)=delete;
    EyeMembership& operator=(const EyeMembership&)=delete;
    // A reset (membership={}) swaps in a fresh Rust membership; the temporary frees the old one.
    EyeMembership(EyeMembership&& other)noexcept:totals(other.totals),unresolved(other.unresolved),rust_(other.rust_){other.rust_=nullptr;}
    EyeMembership& operator=(EyeMembership&& other)noexcept{
        if(this!=&other){std::swap(rust_,other.rust_);totals=other.totals;unresolved=other.unresolved;}return *this;
    }
    bool Add(unsigned side,Packet packet,const Inputs& input,std::span<const uint8_t> blob){
        std::array<uint32_t,4> state{};
        const bool added=cvr_em_add(rust_,side,packet.second,&input,blob.data(),blob.size(),state.data());
        totals={state[0],state[1]};unresolved={state[2],state[3]};return added;
    }
    bool Plan(const std::array<uint8_t,64>& draw,std::span<const uint8_t> instances,uint32_t stride,std::vector<uint8_t>& masks,uint32_t* reason=nullptr){
        uint32_t why=1;
        std::vector<uint8_t> result((stride==48 || stride==64) && instances.size()<=16384?instances.size()/stride:0);
        const bool planned=cvr_em_plan(rust_,&draw,instances.data(),instances.size(),stride,result.data(),result.size(),&why);
        if(reason)*reason=why;
        if(planned)masks=std::move(result);
        return planned;
    }
    // Refreshed from the Rust membership after every Add.
    std::array<uint32_t,2> totals{},unresolved{};
    uint32_t KnownExtras()const{return cvr_em_known_extras(rust_);}
    bool Extras(const std::array<uint8_t,64>& draw,uint32_t stride,std::vector<uint8_t>& output){
        const auto bytes=cvr_em_extras(rust_,&draw,stride);if(!bytes)return false;
        struct Release{RustBytes* bytes;~Release(){cvr_em_bytes_free(bytes);}}release{bytes};
        size_t length{};const auto data=cvr_em_bytes(bytes,&length);output.assign(data,data+length);return true;
    }
private:
    RustEyeMembership* rust_;
};
#else
class EyeMembership {
public:
    bool Add(unsigned side,Packet packet,const Inputs& input,std::span<const uint8_t> blob){
        if(side>1 || !(input.valid&1))return false;
        if(input.draw[0] || !(packet.second&(uint64_t(1)<<51)))return true;
        if(!input.instances || input.instances>32767 || input.instances!=((packet.second>>18)&32767) ||
            input.instanceStride!=((packet.second&(uint64_t(1)<<50))?64u:48u))return false;
        if(!(input.valid&16)){
            std::array<uint8_t,48> empty{};std::string key;
            if(!GeometryInstanceKey(input.draw,empty,key))return false;
            unknown[side].insert(key.substr(0,64));++unresolved[side];return true;
        }
        if(uint64_t(input.instances)*input.instanceStride!=input.instanceBytes || input.instanceOffset>blob.size() ||
            input.instanceBytes>blob.size()-input.instanceOffset || totals[side]>262144-input.instances)return false;
        std::string key;
        for(uint32_t i=0;i<input.instances;++i){
            if(!GeometryInstanceKey(input.draw,blob.subspan(input.instanceOffset+size_t(i)*input.instanceStride,input.instanceStride),key))return false;
            ++counts[key][side];
            auto& original=initial[key];
            if(side==1 && !original[1])peerGeometry[key.substr(0,64)].push_back(key);
            ++original[side];
        }
        totals[side]+=input.instances;return true;
    }
    bool Plan(const std::array<uint8_t,64>& draw,std::span<const uint8_t> instances,uint32_t stride,std::vector<uint8_t>& masks,uint32_t* reason=nullptr){
        if(reason)*reason=1;
        if((stride!=48 && stride!=64) || instances.empty() || instances.size()%stride || instances.size()>16384)return false;
        // Table entries remain stable throughout planning. Group their pointers
        // to count duplicates without allocating a temporary hash node per instance.
        struct Match {std::array<uint32_t,2>* count;size_t index;};
        std::vector<Match> matches;matches.reserve(instances.size()/stride);
        std::vector<uint8_t> result(instances.size()/stride);std::string key;bool peerUnknown{};
        for(size_t i=0;i<instances.size();i+=stride){
            if(!GeometryInstanceKey(draw,instances.subspan(i,stride),key))return false;
            if(!i)peerUnknown=unknown[1].contains(key.substr(0,64));
            const auto it=counts.find(key);
            if(it==counts.end()){if(reason)*reason=3;return false;}
            matches.push_back({&it->second,i/stride});
        }
        std::sort(matches.begin(),matches.end(),[](const auto& a,const auto& b){
            return a.count==b.count?a.index<b.index:std::less<std::array<uint32_t,2>*>{}(a.count,b.count);});
        for(size_t begin=0;begin<matches.size();){
            size_t end=begin+1;while(end<matches.size() && matches[end].count==matches[begin].count)++end;
            const auto& count=*matches[begin].count;const auto n=uint32_t(end-begin),common=std::min(count[1],n);
            if(count[0]<n){if(reason)*reason=3;return false;}
            if(peerUnknown && common<n){if(reason)*reason=2;return false;}
            for(size_t i=begin;i<end;++i)result[matches[i].index]=i-begin<common?3:2;
            begin=end;
        }
        for(size_t begin=0;begin<matches.size();){
            size_t end=begin+1;while(end<matches.size() && matches[end].count==matches[begin].count)++end;
            auto& count=*matches[begin].count;const auto n=uint32_t(end-begin);count[0]-=n;count[1]-=std::min(count[1],n);begin=end;
        }
        masks=std::move(result);if(reason)*reason=0;return true;
    }
    std::array<uint32_t,2> totals{},unresolved{};
    uint32_t KnownExtras()const{
        uint32_t total{};for(const auto& [key,count]:initial)if(!unknown[0].contains(key.substr(0,64)) && count[1]>count[0])total+=count[1]-count[0];
        return total;
    }
    bool Extras(const std::array<uint8_t,64>& draw,uint32_t stride,std::vector<uint8_t>& output){
        if(stride!=48 && stride!=64)return false;
        std::array<uint8_t,64> empty{};std::string key;
        if(!GeometryInstanceKey(draw,{empty.data(),stride},key))return false;
        const auto geometry=key.substr(0,64);std::vector<uint8_t> bytes;std::vector<std::string> selected;
        const auto group=peerGeometry.find(geometry);
        if(group==peerGeometry.end()){output.clear();return true;}
        for(const auto& candidate:group->second){
            if(candidate.size()!=64+stride || claimed.contains(candidate))continue;
            const auto& count=initial.at(candidate);if(count[1]<=count[0])continue;
            if(unknown[0].contains(geometry))return false;
            const auto n=count[1]-count[0];if(uint64_t(n)*stride>16384-bytes.size())return false;
            for(uint32_t i=0;i<n;++i)bytes.insert(bytes.end(),candidate.begin()+64,candidate.end());
            selected.push_back(candidate);
        }
        for(const auto& candidate:selected)claimed.insert(candidate);
        output=std::move(bytes);return true;
    }
private:
    std::unordered_map<std::string,std::array<uint32_t,2>> counts,initial;
    std::array<std::unordered_set<std::string>,2> unknown;
    std::unordered_map<std::string,std::vector<std::string>> peerGeometry;
    std::unordered_set<std::string> claimed;
};
#endif
}
