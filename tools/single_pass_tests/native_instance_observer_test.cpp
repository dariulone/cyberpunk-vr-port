#include "Render/NativeInstanceObserver.hpp"
#include <cstdio>
#include <stdexcept>
#include <vector>

namespace observer=cvr::stereo::instance_observer;
namespace plan=observer::detail;
namespace ring=cvr::stereo::instance_ring;
static void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
static ring::RingObservation Native(uint64_t head=128){return {0x1000,7,0x2000,0x10000000,0x20000000,0x3000,
    0x4000,0x5000,0x6000,0x600000,0x20000,head,0,17,48,5};}
static ring::Scope Scope(uint64_t occurrence){return {0x7000,9,occurrence,3,0x8000,0x3000,0x2401};}
int main()try{
    static_assert(sizeof(observer::QueryObservation)==256);
    static_assert(size_t(observer::QueryField::SourceList)==3 && size_t(observer::QueryField::CpuBytes)==25 &&
        size_t(observer::QueryField::LiveCpuBytes)==31);
    Require(observer::WriterRva==0x337144 && observer::ReserveRva==0x939ACC && observer::PublishRva==0x4F34FC,"exact native hook addresses");
    plan::ReservationRange reservation;
    Require(plan::ReservationResult(64,0x20000,128,64,192,reservation) && reservation.headBefore==64 &&
        reservation.absoluteBegin==64 && reservation.absoluteEnd==192,"exclusive genuine native allocator result");
    Require(!plan::ReservationResult(64,0x20000,128,64,193,reservation),"a later head cannot stand in for this original allocator return");
    Require(!plan::ReservationResult(64,0x20000,128,65,192,reservation),"returned physical first must match actual before/after reservation");
    Require(plan::ReservationResult(0x1FFC0,0x20000,128,0,0x20080,reservation) && reservation.absoluteBegin==0x20000,
        "native wrap padding retains absolute rather than guessed physical coordinates");
    Require(!plan::ReservationResult(UINT64_MAX-10,0x20000,128,0,117,reservation) && !reservation.absoluteBegin,"absolute wrap overflow rejected");
    Require(!plan::ReservationResult(0,100,1,0,1,reservation) && !plan::ReservationResult(0,0x8000,0,0,0,reservation) &&
        !plan::ReservationResult(0,0x8000,0x8001,0,0x8001,reservation),"invalid native capacities/counts rejected");
    Require(plan::ReservationResult(64,0x20000,128,64,192,reservation),"fresh reservation fixture");reservation.sequence=1;
    plan::WriterRange writer;
    Require(plan::ResolveWriter(0x9000,0x1000,7,3,0x20000,{0,0},{70,192},64,6,reservation,{},writer) &&
        writer.absoluteBegin==64 && writer.cache && writer.cache->next.first==70,"completed native writer joins exact new128-instance cache reservation");
    auto cache=writer.cache;
    Require(plan::ResolveWriter(0x9000,0x1000,7,3,0x20000,{70,192},{75,192},70,5,{},cache,writer) && writer.absoluteBegin==70 &&
        writer.reservation.sequence==1 && writer.cache->next.first==75,"fast cached writer retains exact original reservation provenance");
    Require(!plan::ResolveWriter(0x9000,0x1000,7,3,0x20000,{70,192},{75,192},70,5,{},{},writer),
        "cached cursor existing before observer activation has no invented reservation");
    auto changed=*cache;++changed.ringGeneration;
    Require(!plan::ResolveWriter(0x9000,0x1000,7,3,0x20000,{70,192},{75,192},70,5,{},changed,writer),"cached reservation cannot cross native lifecycle");
    changed=*cache;++changed.thread;
    Require(!plan::ResolveWriter(0x9000,0x1000,7,3,0x20000,{70,192},{75,192},70,5,{},changed,writer),"foreign cursor thread rejected");
    Require(!plan::ResolveWriter(0x9100,0x1000,7,3,0x20000,{70,192},{75,192},70,5,{},cache,writer),"same numeric cursor values do not establish cursor identity");
    Require(!plan::ResolveWriter(0x9000,0x1000,7,3,0x20000,{71,192},{76,192},71,5,{},cache,writer),"unobserved cursor advancement invalidates cache join");
    Require(!plan::ResolveWriter(0x9000,0x1000,7,3,0x20000,{70,192},{75,193},70,5,{},cache,writer),"changed cache end rejected");
    Require(plan::ReservationResult(192,0x20000,200,192,392,reservation),"direct noncached reservation");reservation.sequence=2;
    Require(plan::ResolveWriter(0x9000,0x1000,7,3,0x20000,{70,192},{70,192},192,200,reservation,cache,writer) &&
        writer.absoluteBegin==192 && !writer.cache,"native fallback allocation preserves unchanged old cursor without claiming a new cache");
    Require(!plan::ResolveWriter(0x9000,0x1000,7,3,0x20000,{70,192},{71,192},192,200,reservation,cache,writer),"fallback cannot alter native cursor");
    reservation.sequence=0;
    Require(!plan::ResolveWriter(0x9000,0x1000,7,3,0x20000,{70,192},{70,192},192,200,reservation,cache,writer),"unobserved reservation label rejected");
    {
        ring::Ledger ledger;auto native=Native();ring::WriterTicket old;
        Require(ledger.Begin(native,Scope(1),old)==ring::Status::Observed,"initial writer");ledger.Unknown(native.ring);
        ring::WriterTicket rejected;
        Require(ledger.Begin(native,Scope(2),rejected)==ring::Status::Changed,"numeric observation alone cannot recover unknown cached producer");
        Require(plan::ReservationResult(128,0x20000,128,128,256,reservation),"later exact original reservation restarts local observation");
        reservation.sequence=3;native.head=256;
        Require(ledger.RestartObservation(native)==ring::Status::Observed && native.generation==7,"local recovery does not fabricate native recreation");
        ring::WriterTicket fresh;Require(ledger.Begin(native,Scope(3),fresh)==ring::Status::Observed &&
            ledger.Reuse(native,reservation.absoluteBegin,128,&fresh)==ring::Status::Observed,"real reservation can invalidate old versions within its own live writer");
        const std::array<uint8_t,48> bytes{};
        Require(ledger.Complete(native,{fresh,128,128,1},bytes)==ring::Status::Completed,"fresh completed CPU writer after local recovery");
        ring::CpuCandidate cpu;
        Require(ledger.Query(native,{fresh,Scope(9),128,128,1},cpu)==ring::Status::Completed && cpu.Bytes().size()==48,"fresh CPU candidate survives local recovery");
        Require(ledger.Query(native,{old,Scope(9),0,0,1},cpu)==ring::Status::UnknownWriter && !cpu,"pre-recovery ticket never revives");
        auto foreign=native;++foreign.resource;
        Require(ledger.RestartObservation(foreign)==ring::Status::Changed,"local restart cannot replace genuine native backing identity");
    }
    {
        std::vector<cvr::stereo::shader_snapshot::Input> inputs{
            {"POSITION",0,DXGI_FORMAT_R32G32B32_FLOAT,7,0,D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,1},
            {"TRANSFORM",1,DXGI_FORMAT_R32G32B32_FLOAT,7,D3D12_APPEND_ALIGNED_ELEMENT,D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,1},
            {"TRANSFORM",2,DXGI_FORMAT_R32G32B32_FLOAT,7,D3D12_APPEND_ALIGNED_ELEMENT,D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,1},
            {"TRANSFORM",3,DXGI_FORMAT_R32G32B32_FLOAT,7,D3D12_APPEND_ALIGNED_ELEMENT,D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,1}};
        Require(plan::InstanceLayout(inputs,48),"actual step1 four-float3 instance span");
        inputs.push_back({"SKIN",0,DXGI_FORMAT_R32G32B32A32_UINT,7,48,D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,1});
        Require(plan::InstanceLayout(inputs,64) && !plan::InstanceLayout(inputs,48),"skin attributes must fit complete copied stride");
        inputs.back().step=2;Require(!plan::InstanceLayout(inputs,64),"step2 layout cannot claim first/count stride1 span");
        inputs.back().step=1;inputs.back().classification=D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA;
        Require(!plan::InstanceLayout(inputs,64),"slot7 per-vertex input cannot use instance span proof");
        inputs.back().classification=D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA;inputs.back().offset=52;
        Require(!plan::InstanceLayout(inputs,64),"attribute beyond final stride rejected");
    }
    std::puts("PASS exact exclusive native reservation results, witnessed cached/fallback CPU writers, local recovery without fake native generations, source layout and32Q ABI; no GPU permission");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"native_instance_observer_test: %s\n",error.what());return 1;}
