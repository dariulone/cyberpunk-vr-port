#include "Render/NativeSourceCbvSnapshot.hpp"
#include <cstdio>
#include <stdexcept>

namespace snapshot=cvr::stereo::source_cbv_snapshot;
namespace policy=cvr::stereo::source_cbv_snapshot::detail;
static void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
static policy::Admission Tracked(){
    policy::Admission a;a.resource=9;a.gpu=0x100000;a.width=512;a.nativeGpu=a.gpu;a.nativeBytes=480;a.reflectedBytes=476;a.viewBytes=512;
    a.heap=D3D12_HEAP_TYPE_DEFAULT;a.nativeKind=3;a.handle=7;a.creationKind=2;a.creationHeap=a.allocationHeap=12;
    a.creationBytes=a.allocationBytes=65536;a.allocation=13;a.allocationResource=a.resource;a.allocationKind=1;
    a.allocatorPimpl=a.allocationPimpl=14;a.state={D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER,2,3};
    a.sourceCurrent=a.creationKnown=a.initialKnown=a.detailKnown=a.stateKnown=a.allocationKnown=a.allocatorKnown=a.direct=a.aliasFieldsEmpty=true;return a;
}
static policy::Admission Pool(){auto a=Tracked();a.width=a.allocationBytes=262144;a.nativeKind=1;a.nativeBytes=16;
    a.comOffset=512;a.nativeGpu=a.gpu+512;a.reflectedBytes=4;a.viewBytes=256;a.creationKind=1;a.creationHeap=a.allocationHeap=0;
    a.creationBytes=0;a.allocationKind=0;a.initialState=D3D12_RESOURCE_STATE_GENERIC_READ;a.state={D3D12_RESOURCE_STATE_GENERIC_READ,8,1};a.poolKnown=true;return a;}
static policy::Admission Upload(){auto a=Tracked();a.width=201326592;a.comOffset=4096;a.nativeGpu=a.nativeBytes=0;a.reflectedBytes=112;a.viewBytes=256;
    a.heap=D3D12_HEAP_TYPE_UPLOAD;a.handle=0;a.nativeKind=0;a.creator=0x3362A0;a.creationKind=1;a.creationHeap=0;
    a.initialState=D3D12_RESOURCE_STATE_GENERIC_READ;a.state={D3D12_RESOURCE_STATE_GENERIC_READ,1,0};return a;}
static void Admissions(){
    policy::Kind kind;auto a=Tracked();Require(policy::Admit(a,kind)==snapshot::Failure::None && kind==policy::Kind::Tracked,"Observed placed class3 was rejected");
    Require(a.reflectedBytes==476 && a.width==512 && a.allocationBytes==65536,"Allocator envelope replaced source payload bounds");
    a.reflectedBytes=512;Require(policy::Admit(a,kind)==snapshot::Failure::Range,"CBV padding outside native logical payload was admitted");
    a=Tracked();a.creationHeap++;Require(policy::Admit(a,kind)==snapshot::Failure::Allocation,"Foreign placement heap admitted");
    a=Tracked();a.allocationResource++;Require(policy::Admit(a,kind)==snapshot::Failure::Allocation,"Foreign MA resource admitted");
    a=Tracked();a.aliasFieldsEmpty=false;Require(policy::Admit(a,kind)==snapshot::Failure::Allocation,"Unobserved alias form admitted");
    a=Tracked();a.state.value=D3D12_RESOURCE_STATE_COPY_DEST;Require(policy::Admit(a,kind)==snapshot::Failure::State,"Writable source state admitted");
    a=Tracked();a.state.status=4;Require(policy::Admit(a,kind)==snapshot::Failure::State,"Pending source state admitted");
    a=Pool();Require(policy::Admit(a,kind)==snapshot::Failure::None && kind==policy::Kind::Pool,"Small class1 shader payload retained the292-byte limit");
    a.poolKnown=false;Require(policy::Admit(a,kind)==snapshot::Failure::Pool,"Unknown pool occupancy admitted");
    a=Pool();a.nativeBytes=513;Require(policy::Admit(a,kind)==snapshot::Failure::Pool,"Unobserved class1 pool unit admitted");
    a=Upload();Require(policy::Admit(a,kind)==snapshot::Failure::None && kind==policy::Kind::Upload,"Exact committed b5 upload admitted incorrectly");
    a.handle=7;a.nativeKind=4;a.creator=0x3346B1;a.nativeBytes=448;a.reflectedBytes=448;a.viewBytes=512;a.state.kind=4;
    Require(policy::Admit(a,kind)==snapshot::Failure::None && kind==policy::Kind::Upload,"Transient upload required fake B0 GPU/resource fields");
    a.heap=D3D12_HEAP_TYPE_GPU_UPLOAD;Require(policy::Admit(a,kind)==snapshot::Failure::Class,"Unobserved GPU_UPLOAD admitted");
    a=Upload();a.initialKnown=false;Require(policy::Admit(a,kind)==snapshot::Failure::Creation,"Unknown creation state admitted");
    a=Upload();a.creator=0x333C1A;Require(policy::Admit(a,kind)==snapshot::Failure::Class,"Unobserved standalone creator admitted");
}
static void RangesAndRecovery(){
    auto a=Tracked();a.comOffset=1;Require(!policy::Range(a),"Unaligned CBV slice admitted");
    a=Tracked();a.gpu=UINT64_MAX-255;Require(!policy::Range(a),"Backing GPU overflow admitted");
    std::vector<uint64_t> offsets;uint64_t end{};const std::array<uint32_t,3> views{256,512,1024};
    Require(policy::Reserve(256,2048,views,offsets,end) && offsets==std::vector<uint64_t>{256,512,1024} && end==2048,"Private padded slices were not256-aligned");
    Require(!policy::Reserve(256,1792,views,offsets,end) && offsets.empty(),"Partial arena reservation published offsets");
    const std::array<uint32_t,1> invalid{112};Require(!policy::Reserve(0,2048,invalid,offsets,end),"Reflected size mistaken for padded CBV extent");
    uint32_t required{};
    Require(policy::RecoveryMask(D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER,{D3D12_RESOURCE_STATE_COPY_SOURCE,2,3},required) &&
        required==(D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER|D3D12_RESOURCE_STATE_COPY_SOURCE),"Recovery dropped an original read permission");
    Require(!policy::RecoveryMask(1,{D3D12_RESOURCE_STATE_COPY_DEST,2,3},required),"Recovery guessed through writable state");
    Require(!policy::RecoveryMask(1,{0,5,3},required) && !policy::RecoveryMask(1,{1,4,3},required),"Recovery guessed unknown/split state");
}
static void Occurrences(){
    snapshot::ReadEpochCertificate epoch{11,12,13,false};Require(!policy::Epoch(epoch,11,12,13),"Tuple manufactured an epoch certificate");
    epoch.certified=true;Require(policy::Epoch(epoch,11,12,13) && !policy::Epoch(epoch,11,12,14),"Certified epoch matched a later occurrence");
    cvr::stereo::indirect_copy::detail::ScopeLedger ledger;const auto first=ledger.Enter(11,12,13,14);
    Require(ledger.Matches(first,11,12,13,14) && !ledger.Matches(first,11,12,13,15),"Foreign recording thread accepted");
    const auto nested=ledger.Enter(11,12,14,14);Require(!ledger.Matches(first,11,12,13,14),"Nested source did not invalidate prior occurrence");
    Require(ledger.Exit(nested) && !ledger.Matches(first,11,12,13,14),"Prior occurrence revived after nested source");
}
struct Token final:IUnknown {
    ULONG refs=1;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID,void**)override{return E_NOINTERFACE;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{return --refs;}
};
static void InlineUploadEpoch(){
    Token token,foreign;cvr::stereo::source_cbv::SourceCbvBacking source;
    source.creatorRva=0x3362A0;source.completedProducerRva=0x33626F;source.logicalBytes=112;
    source.descriptorBytes=source.reservedBytes=256;
    source.recording={&token,12,14,11,15};source.creationRecording=source.recording;
    snapshot::ReadEpochCertificate epoch{11,12,13,true};
    Require(policy::InlineDirectUpload(source,epoch,112),"Exact inline producer/original epoch was rejected");
    Require(!policy::InlineDirectUpload(source,epoch,113),"Padded/unwritten bytes acquired producer coverage");
    auto bad=source;bad.completedProducerRva=0;Require(!policy::InlineDirectUpload(bad,epoch,112),"Missing completed producer was accepted");
    bad=source;bad.creationRecording.token=&foreign;Require(!policy::InlineDirectUpload(bad,epoch,112),"Another native creation token was accepted");
    bad=source;++bad.creationRecording.generation;Require(!policy::InlineDirectUpload(bad,epoch,112),"Older recording supplied an upload epoch");
    bad=source;bad.creatorRva=0x3346B1;Require(!policy::InlineDirectUpload(bad,epoch,112),"Pre-write transient creator was accepted");
    bad=source;bad.reservedBytes=512;Require(!policy::InlineDirectUpload(bad,epoch,112),"Another reserved extent was accepted");
    epoch.certified=false;Require(!policy::InlineDirectUpload(source,epoch,112),"Producer facts manufactured a valid queued epoch");
    epoch.certified=true;source.creatorRva=0x3346B1;source.completedProducerRva=0x33435E;
    source.native.kind=4;source.native.handle=7;source.writerNonce=8;source.writerDestination=0x100000;
    source.writerBytes=source.logicalBytes=448;source.reservedBytes=source.descriptorBytes=512;source.writerRecording=source.recording;
    Require(policy::InlineTransientUpload(source,epoch,448),"Exact transient completion did not inherit its original queued epoch");
    Require(!policy::InlineTransientUpload(source,epoch,449),"Transient incomplete prefix acquired coverage");
    bad=source;bad.writerRecording.token=&foreign;Require(!policy::InlineTransientUpload(bad,epoch,448),"Foreign writer recording acquired an epoch");
    bad=source;bad.writerNonce=0;Require(!policy::InlineTransientUpload(bad,epoch,448),"Missing invocation supplied transient completion");
    bad=source;bad.completedProducerRva=0x22010D;Require(!policy::InlineTransientUpload(bad,epoch,448),"Unqualified pointer-returning writer acquired an epoch");
}
static void RecordedPrivateMetadata(){
    // Pure metadata fixtures do not instantiate the private native Receipt or
    // grant a successful MatchesRecordedPrivateCbv runtime authority.
    const policy::PrivateCbvIdentity saved{11,12,512,256,228,0x100100,19,0,228,256,256};
    snapshot::PreparedCopies proof;proof.list=11;proof.generation=12;proof.occurrence=13;proof.copiedBytes=228;proof.count=1;
    const snapshot::ReadEpochCertificate epoch{11,12,13,true};
    Require(policy::RecordedCopyFields(proof,epoch,228,1),"Exact stored receipt labels did not match");
    Require(!policy::RecordedCopyFields({},epoch,228,1),"Empty receipt labels accepted");
    for(UINT field=0;field<5;++field){auto changed=proof;
        switch(field){case 0:++changed.list;break;case 1:++changed.generation;break;case 2:++changed.occurrence;break;
        case 3:++changed.copiedBytes;break;case 4:++changed.count;break;}
        Require(!policy::RecordedCopyFields(changed,epoch,228,1),"Mutated receipt labels accepted");
    }
    Require(!policy::RecordedCopyFields(proof,{11,12,13,false},228,1),"Uncertified epoch supplied recorded metadata");
    const auto matches=[&](const policy::PrivateCbvIdentity& actual){
        return policy::RecordedPrivateIdentity(actual,saved,11,12,0x100000,512,1024);
    };
    Require(matches(saved),"Exact recorded CBV metadata rejected");
    for(UINT field=0;field<11;++field){auto changed=saved;
        switch(field){case 0:++changed.resource;break;case 1:++changed.owner;break;case 2:++changed.sourceOffset;break;
        case 3:changed.privateOffset+=256;break;case 4:++changed.copiedBytes;break;case 5:changed.gpu+=256;break;
        case 6:++changed.parameter;break;case 7:++changed.offset;break;case 8:++changed.reflectedBytes;break;
        case 9:changed.paddedBytes+=256;break;case 10:changed.viewBytes+=256;break;}
        Require(!matches(changed),"Changed private CBV retained recorded identity");
    }
    Require(!matches({}),"Empty private view matched a recorded slice");
    Require(!policy::RecordedPrivateIdentity(saved,saved,0,12,0x100000,512,1024),"Missing arena resource accepted");
    Require(!policy::RecordedPrivateIdentity(saved,saved,11,13,0x100000,512,1024),"Other arena owner accepted");
    Require(!policy::RecordedPrivateIdentity(saved,saved,11,12,0x100000,511,1024),"Unwritten private padding accepted");
    Require(!policy::RecordedPrivateIdentity(saved,saved,11,12,0x100000,512,256),"Arena cursor overflow accepted");
    Require(!policy::RecordedPrivateIdentity(saved,saved,11,12,UINT64_MAX-511,512,1024),"Arena GPU address overflow accepted");
    auto malformed=saved;malformed.copiedBytes=227;
    Require(!policy::RecordedPrivateIdentity(malformed,malformed,11,12,0x100000,512,1024),"Recorded copy extent did not cover reflected payload");
    malformed=saved;malformed.paddedBytes=malformed.viewBytes=228;
    Require(!policy::RecordedPrivateIdentity(malformed,malformed,11,12,0x100000,512,1024),"Unaligned recorded CBV admitted");
}
int main()try{Admissions();RangesAndRecovery();Occurrences();InlineUploadEpoch();RecordedPrivateMetadata();std::puts("PASS pure observed source CBV snapshot contracts");return 0;}
catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
