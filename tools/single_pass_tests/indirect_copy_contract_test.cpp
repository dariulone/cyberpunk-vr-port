#include "Render/NativeIndirectCopy.hpp"
#include <cstdio>
#include <stdexcept>

using namespace cvr::stereo;
using namespace cvr::stereo::indirect_copy;
static void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
static preparation::GraphParentRecord Parent(){
    preparation::GraphParentRecord value;value.context=0x1000;value.manager=0x2000;value.fingerprint=0x3000;
    value.object=0x4000;value.work=0x5000;value.name=0x6000;value.handle=1;value.ordinal=2;value.slot=3;value.viewIndex=1;
    return value;
}
static detail::Observation Source(){
    detail::Observation value;value.native={0x8000,0x100000,4096,0x200000,0x300000,4,0};
    value.list=0xA000;value.generation=8;value.position=9;value.resource=value.native.resource;
    value.backingGpu=value.native.gpu;value.backingBytes=value.native.bytes;value.offset=40;value.bytes=20;
    value.recordAddress=value.native.registry+0x5C0AF8+uint64_t(value.native.handle-1)*0xB0;
    value.caller=0x342B11;value.maxCount=value.signatureKind=1;value.stride=20;value.heap=D3D12_HEAP_TYPE_DEFAULT;
    value.flags=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;value.contextKind=1;value.lowType=0xB;
    value.state={D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT,2,0};
    value.fullBacking=value.parentCurrent=value.recordingCurrent=value.nativeOrigin=value.sameDevice=true;
    return value;
}
static void Admission(){
    const auto parent=Parent();const auto source=Source();
    Require(detail::Admit(source,parent)==Failure::None,"Exact original indexed source rejected");
    auto check=[&](detail::Observation value,Failure reason,const char* message){Require(detail::Admit(value,parent)==reason,message);};
    auto bad=source;bad.caller=0x624830;check(bad,Failure::Arguments,"Other indexed site accepted");
    bad=source;bad.caller=0x62462F;check(bad,Failure::Arguments,"Nonindexed source site accepted");
    bad=source;bad.maxCount=2;check(bad,Failure::Arguments,"Foreign command count accepted");
    bad=source;bad.stride=24;check(bad,Failure::Arguments,"Padded/foreign signature accepted");
    bad=source;bad.signatureKind=2;check(bad,Failure::Arguments,"Pure DRAW accepted");
    bad=source;bad.bytes=16;check(bad,Failure::Arguments,"Wrong snapshot interval accepted");
    bad=source;bad.offset=42;check(bad,Failure::Arguments,"Unaligned indirect offset accepted");
    bad=source;bad.lowType=6;check(bad,Failure::Arguments,"Material fixed pool accepted");
    bad=source;bad.resource++;check(bad,Failure::Arguments,"Native/API resource mismatch accepted");
    bad=source;bad.recordAddress++;check(bad,Failure::Arguments,"Foreign registry record accepted");
    bad=source;bad.native.registry=UINT64_MAX-8;check(bad,Failure::Arguments,"Registry arithmetic overflow accepted");
    bad=source;bad.native.handle=0;check(bad,Failure::Arguments,"Zero native handle accepted");
    bad=source;bad.position=0;check(bad,Failure::Arguments,"Missing exact source position accepted");
    bad=source;bad.parentCurrent=false;check(bad,Failure::Parent,"Historical graph parent accepted");
    auto foreign=parent;foreign.status=1;Require(detail::Admit(source,foreign)==Failure::Parent,"Unresolved graph parent accepted");
    foreign=parent;foreign.viewIndex=2;Require(detail::Admit(source,foreign)==Failure::Parent,"Non-eye parent accepted");
    foreign=parent;foreign.ordinal=220;Require(detail::Admit(source,foreign)==Failure::Parent,"Unbounded parent ordinal accepted");
    bad=source;bad.recordingCurrent=false;check(bad,Failure::Binding,"Foreign/reset recording accepted");
    bad=source;bad.nativeOrigin=false;check(bad,Failure::Binding,"Distant/foreign native caller accepted");
    bad=source;bad.sameDevice=false;check(bad,Failure::Binding,"Foreign device accepted");
    bad=source;bad.pending=1;check(bad,Failure::State,"Pending native barrier, including split begin, accepted");
    bad=source;bad.state.status=4;check(bad,Failure::State,"Unready/pending split state accepted");
    bad=source;bad.state.status=5;check(bad,Failure::State,"Absent native state accepted");
    bad=source;bad.state.value=0x80000000;check(bad,Failure::State,"Uninitialized native state accepted");
    bad=source;bad.state.value=D3D12_RESOURCE_STATE_COMMON;check(bad,Failure::State,"Guessed COMMON accepted");
    bad=source;bad.state.value|=D3D12_RESOURCE_STATE_UNORDERED_ACCESS;check(bad,Failure::State,"Read/write combined state accepted");
    bad=source;bad.state.value=D3D12_RESOURCE_STATE_COPY_SOURCE;check(bad,Failure::State,"Missing native indirect read accepted");
    for(const UINT kind:{1u,2u,4u,5u,6u,15u}){bad=source;bad.native.kind=bad.state.kind=kind;check(bad,Failure::State,"Unproved native buffer class accepted");}
    bad=source;bad.state.kind=3;check(bad,Failure::State,"State belongs to another native class");
    bad=source;bad.heap=D3D12_HEAP_TYPE_UPLOAD;check(bad,Failure::Backing,"Unadmitted upload path accepted");
    bad=source;bad.flags=0;check(bad,Failure::Backing,"Unproved type-B resource flags accepted");
    bad=source;bad.fullBacking=false;check(bad,Failure::Backing,"Shared backing transition accepted");
    bad=source;bad.native.gpu+=256;check(bad,Failure::Backing,"Logical GPU suballocation accepted");
    bad=source;bad.native.bytes-=256;check(bad,Failure::Backing,"Logical byte suballocation accepted");
    bad=source;bad.offset=bad.backingBytes-16;check(bad,Failure::Backing,"Out-of-bounds 20-byte read accepted");
    bad=source;bad.backingGpu=bad.native.gpu=UINT64_MAX-32;check(bad,Failure::Backing,"GPU-range overflow accepted");
    bad=source;bad.native.kind=bad.state.kind=3;check(bad,Failure::None,"Audited tracked default class 3 rejected");
    Require(!detail::ReadMask(source.state,0,true),"INDIRECT_ARGUMENT alone granted copy-read permission");
    auto ready=source.state;ready.value|=D3D12_RESOURCE_STATE_COPY_SOURCE;
    Require(detail::ReadMask(ready,0,true),"Native combined read mask rejected");
    ready.value|=D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;Require(detail::ReadMask(ready,0,true),"Other native read bits were discarded");
}
static void Occurrences(){
    detail::ScopeLedger ledger;const auto first=ledger.Enter(1,2,3,4);
    Require(ledger.Matches(first,1,2,3,4),"Live exact occurrence rejected");
    Require(!ledger.Matches(first,9,2,3,4) && !ledger.Matches(first,1,9,3,4) && !ledger.Matches(first,1,2,9,4),"Foreign list/generation/position accepted");
    Require(!ledger.Matches(first,1,2,3,9),"Foreign thread accepted");
    Require(ledger.Exit(first) && !ledger.Matches(first,1,2,3,4),"Scope-exit reuse accepted");
    const auto later=ledger.Enter(1,2,3,4);
    Require(later.nonce!=first.nonce && !ledger.Matches(first,1,2,3,4) && ledger.Matches(later,1,2,3,4),"Same operands licensed a later occurrence");
    const auto nested=ledger.Enter(1,2,3,4);
    Require(!ledger.Matches(later,1,2,3,4) && ledger.Matches(nested,1,2,3,4),"Nested source reused its parent's receipt");
    Require(ledger.Exit(nested) && !ledger.Matches(later,1,2,3,4),"Intervening nested source failed to invalidate the outer insertion");
    Require(ledger.Exit(later),"Outer scope failed to close");
    const auto next=ledger.Enter(1,2,4,4);Require(ledger.Matches(next,1,2,4,4),"Fresh occurrence did not recover after nesting");
    Require(ledger.Exit(next),"Fresh occurrence failed to close");
    Require(!ledger.Enter(0,2,3,4).nonce && !ledger.Enter(1,0,3,4).nonce && !ledger.Enter(1,2,0,4).nonce,"Invalid scope entered");
    detail::ScopeLedger broken;const auto outer=broken.Enter(1,2,3,4),inner=broken.Enter(1,2,3,4);
    Require(!broken.Exit(outer) && !broken.Matches(inner,1,2,3,4) && !broken.Enter(1,2,3,4).nonce,"Out-of-order scope destruction did not fail closed");
}
static void Owners(){
    detail::Allocation physical{0x1000,0x2000,0x3000,65536,0x4000};
    detail::AllocationBinding direct;direct.allocation=physical.address;
    Require(detail::AllocationForm(direct,physical,0x4000,4096),"Genuine committed allocation rejected");
    physical.kind=1;physical.heap=0x5000;Require(detail::AllocationForm(direct,physical,0x4000,4096),"Genuine placed allocation rejected");
    auto missingHeap=physical;missingHeap.heap=0;Require(!detail::AllocationForm(direct,missingHeap,0x4000,4096),"Placed allocation without a heap accepted");
    Require(!detail::AllocationForm(direct,physical,0,4096) && !detail::AllocationForm(direct,physical,0x4000,0),"Empty source ownership accepted");
    auto bad=physical;bad.resource=0x6000;Require(!detail::AllocationForm(direct,bad,0x4000,4096),"Different allocation resource accepted");
    bad=physical;bad.kind=2;Require(!detail::AllocationForm(direct,bad,0x4000,4096),"Heap reservation used as direct-resource owner");
    bad=physical;bad.kind=3;Require(!detail::AllocationForm(direct,bad,0x4000,4096),"Unknown allocation kind accepted");
    bad=physical;bad.bytes=2048;Require(!detail::AllocationForm(direct,bad,0x4000,4096),"Too-small physical allocation accepted");
    physical.resource=0;physical.kind=2;
    detail::AllocationBinding alias{physical.address,0x7000,0x8000,0xC0,0x10000,physical.bytes,true};
    Require(detail::AllocationForm(alias,physical,0x4000,4096),"Native shared heap allocation rejected");
    physical.kind=1;Require(detail::AllocationForm(alias,physical,0x4000,4096),"Native shared placed allocation rejected");
    auto wrong=alias;wrong.flags=0;Require(!detail::AllocationForm(wrong,physical,0x4000,4096),"Unproved alias flags accepted");
    wrong=alias;wrong.alignment=4096;Require(!detail::AllocationForm(wrong,physical,0x4000,4096),"Unproved alias alignment accepted");
    wrong=alias;wrong.bytes--;Require(!detail::AllocationForm(wrong,physical,0x4000,4096),"Alias holder/allocation size mismatch accepted");
    wrong=alias;wrong.control=0;Require(!detail::AllocationForm(wrong,physical,0x4000,4096),"Unowned alias holder accepted");
    bad=physical;bad.resource=0x4000;Require(!detail::AllocationForm(alias,bad,0x4000,4096),"Shared allocation masqueraded as a direct resource");
    bad=physical;bad.kind=0;Require(!detail::AllocationForm(alias,bad,0x4000,4096),"Committed allocation licensed placed aliasing");
    bad=physical;bad.heap=0;Require(!detail::AllocationForm(alias,bad,0x4000,4096),"Unknown alias heap accepted");
    wrong=direct;wrong.payload=0x9000;Require(!detail::AllocationForm(wrong,physical,0x4000,4096),"Mixed ownership forms accepted");
}
int main()try{
    Admission();Occurrences();Owners();std::puts("PASS pure indirect-copy admission, ownership forms and exact source occurrence contracts");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
