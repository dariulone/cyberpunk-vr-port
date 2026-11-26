#include "Render/StereoMeshPlan.hpp"
#include "Render/NativePacketKey.hpp"
#include "Render/NativePacketPass.hpp"
#include <cstdio>
#include <random>
#include <source_location>
#include <string>
#include <stdexcept>
using namespace cvr::stereo;
static void Require(bool value,std::source_location at=std::source_location::current()){
    if(!value)throw std::runtime_error("Mesh plan assertion failed at line "+std::to_string(at.line()));
}
static size_t Reference(std::span<const uint64_t> a,std::span<const uint64_t> b) {
    std::vector<size_t> row(b.size()+1);
    for(auto id:a){size_t previous{};for(size_t j=0;j<b.size();++j){const auto old=row[j+1];
        row[j+1]=id && id==b[j]?previous+1:std::max(row[j],row[j+1]);previous=old;}}
    return row.back();
}
static void Validate(const std::vector<uint64_t>& a,const std::vector<uint64_t>& b) {
    MeshPlan plan;Require(BuildStereoMeshPlan(a,b,plan));
    uint32_t next[2]{};size_t common{};
    for(const auto& draw:plan.draws) {
        Require(draw.mask>=1 && draw.mask<=3);
        for(unsigned eye=0;eye<2;++eye) {
            if(draw.mask&(1u<<eye))Require(draw.source[eye]==next[eye]++);
            else Require(draw.source[eye]==UINT32_MAX);
        }
        if(draw.mask==3){Require(a[draw.source[0]] && a[draw.source[0]]==b[draw.source[1]]);++common;}
    }
    Require(next[0]==a.size() && next[1]==b.size() && common==plan.common && common==Reference(a,b));
    Require(plan.draws.size()==a.size()+b.size()-common);
}
int main() try {
    Validate({},{});Validate({1,1,1},{1});Validate({1,2,3},{3,2,1});Validate({0,1,0},{0,1,0});
    Validate({1,2,3,2,4},{1,3,2,4,5});Validate({1,2},{});Validate({},{1,2});
    std::mt19937 random(0x4d455348);
    for(unsigned i=0;i<5000;++i){std::vector<uint64_t> a(random()%70),b(random()%70);
        for(auto& v:a)v=random()%12;for(auto& v:b)v=random()%12;Validate(a,b);}
    MeshPlan preserved;preserved.common=77;preserved.draws.push_back({2,{UINT32_MAX,9}});
    const std::vector<uint64_t> duplicates(1024,1);
    Require(!BuildStereoMeshPlan(duplicates,duplicates,preserved));Require(preserved.common==77 && preserved.draws.size()==1);
    Require(!BuildStereoMeshPlan({duplicates.data(),3},{},preserved,{2,10}));
    using namespace packets;
    Inputs input{};input.valid=127;input.instances=1;input.source=1;input.instanceStride=48;input.instanceBytes=48;
    Packet packet{0x1234,(uint64_t(1)<<51)|(1<<18)};std::vector<uint8_t> blob(48,3);std::string original,changed;
    Require(MeshCandidateKey(packet,input,blob,original));
    for(unsigned byte=0;byte<64;++byte){auto copy=input;copy.draw[byte]^=0x11;
        const bool valid=MeshCandidateKey(packet,copy,blob,changed);
        const bool padding=(byte>=1 && byte<8)||byte>=45;
        Require(byte?valid:true);if(byte)Require((original==changed)==padding);else Require(!valid);}
    for(unsigned byte=0;byte<40;++byte){auto copy=input;copy.state[byte]^=0x33;
        Require(MeshCandidateKey(packet,copy,blob,changed));
        // 2.5's 32-byte record: +4..7, the program pointer +8..F, +14..17 and +1F are not keyed.
        Require((original==changed)==((byte>=0x04 && byte<0x10)||(byte>=0x14 && byte<0x18)||byte==0x1f));}
    auto copy=packet;copy.first^=0x1fff;Require(MeshCandidateKey(copy,input,blob,changed)&&changed==original);
    copy=packet;copy.first^=1ull<<13;Require(MeshCandidateKey(copy,input,blob,changed)&&changed!=original);
    copy=packet;copy.first^=0x3fffffffffull<<17;copy.second^=0x3ffff|(0x1ffffull<<33);
    Require(MeshCandidateKey(copy,input,blob,changed)&&changed==original);
    auto invalid=input;invalid.instanceOffset=UINT32_MAX;changed="unchanged";
    Require(!MeshCandidateKey(packet,invalid,blob,changed)&&changed=="unchanged");
    invalid=input;invalid.valid=111;Require(!MeshCandidateKey(packet,invalid,blob,changed));
    invalid=input;invalid.instances=2;Require(!MeshCandidateKey(packet,invalid,blob,changed));
    blob[0]^=1;Require(MeshCandidateKey(packet,input,blob,changed)&&changed!=original);
    blob.resize(68);const uint32_t programWords[5]{1,0,13,4,0x12345678};std::memcpy(blob.data()+48,programWords,20);
    auto programmed=input;programmed.programOffset=48;programmed.programBytes=programmed.commandProgramField=20;
    Require(MeshCandidateKey(packet,programmed,blob,original));
    programmed.state[StateProgram]^=0x44;Require(MeshCandidateKey(packet,programmed,blob,changed)&&original==changed);
    blob.back()^=1;Require(MeshCandidateKey(packet,programmed,blob,changed)&&original!=changed);
    programmed.programBytes=19;Require(!MeshCandidateKey(packet,programmed,blob,changed));
    programmed.programBytes=20;blob[48]=2;Require(!MeshCandidateKey(packet,programmed,blob,changed));
    std::array<Record,4> chunks{};
    for(uint32_t i=0;i<4;++i) {
        auto& r=chunks[i];r.frame=1;r.node=0x33DDA4;r.side=1;r.metadataFlags=7;r.beginQpc=1;r.endQpc=2;
        r.begin=0x1000+(7*i/4)*16;r.end=0x1000+(7*(i+1)/4)*16;r.count=uint32_t((r.end-r.begin)/16);r.offset=7*i/4;
        const uint32_t parts=4;std::memcpy(r.groupBytes.data()+40,&i,4);std::memcpy(r.groupBytes.data()+44,&parts,4);
    }
    std::vector<PacketRange> ranges;Require(AssemblePass(chunks,7,ranges));
    std::swap(chunks[0],chunks[3]);Require(AssemblePass(chunks,7,ranges));Require(ranges[0].offset==0 && ranges[3].offset==5);
    const auto good=chunks;
    chunks[0].groupBytes[40]=2;Require(!AssemblePass(chunks,7,ranges));chunks=good;
    chunks[0].offset=1;Require(!AssemblePass(chunks,7,ranges));chunks=good;
    chunks[0].begin+=16;Require(!AssemblePass(chunks,7,ranges));chunks=good;
    chunks[0].groupBytes[8]^=1;Require(!AssemblePass(chunks,7,ranges));chunks=good;
    chunks[0].argumentBytes[20]^=3;chunks[0].groupBytes[13]^=0x7f;chunks[0].groupBytes[33]^=0x22;
    Require(AssemblePass(chunks,7,ranges));chunks=good;
    Require(!AssemblePass(std::span(chunks).first(3),7,ranges));Require(!AssemblePass(chunks,6,ranges));
    std::puts("PASS order/multiplicity/provenance, 5000 optimal plans, bounded matching and native key semantics");return 0;
}catch(const std::exception& e){std::fprintf(stderr,"%s\n",e.what());return 1;}
