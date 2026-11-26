#pragma once
#include "Render/NativeGeometryLeases.hpp"
#include "Render/NativeShaderSnapshot.hpp"
#include "Render/NativeSourceCbvSnapshot.hpp"
#include "Render/NativeStereoFamilies.hpp"
#include "Render/NativeTextureRead.hpp"
#include "Render/NativeMaterialBufferSnapshot.hpp"
#include "Render/StereoBindingBank.hpp"
#include "Render/StructuredSnapshotView.hpp"
#include "Render/StereoVrsSnapshot.hpp"
#include "Render/SourceBindingAssembly.hpp"
#include "Render/GeometryTargetCheckpoint.hpp"
#include "Render/NativeTargetAccess.hpp"
#include "Render/NativeTargetLifetime.hpp"
#include <cstring>
#include <span>

namespace cvr::stereo::prepared_pair {
struct Material {descriptors::SourceResourceBinding binding;texture_read::ImmutableLease immutable;std::optional<material_buffer_snapshot::Copy> buffer;};
using ShaderInputCheck=source_binding_assembly::ShaderInputCheck;
struct NativeExitPlane {
    UINT slot{},plane{},rows{},stride{},component{},bytes{};
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT layout{};
};
// The retained source recording owns these already-recorded copies. The owned
// consumer's same-queue fence must complete before any native readback is mapped.
struct NativeExit {
    Microsoft::WRL::ComPtr<ID3D12Resource> readback;
    UINT width{},height{},eye{},sourceSide{},frame{};
    uintptr_t list{};uint64_t generation{};
    std::vector<NativeExitPlane> planes;
};
namespace native_exit_detail {
inline constexpr uint64_t MaxReadbackBytes=512ull*1024*1024;
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/native_exit.rs (tools/single_pass_tests/native_exit_compare_test runs its cases
// against them in its Rust variant). A plane crosses with its readback bytes borrowed for the call.
struct Comparison;
struct PlaneData {const NativeExitPlane* row{};const uint8_t* data{};size_t bytes{};};
extern "C" {
UINT cvr_nx_plane_index(UINT,UINT);
bool cvr_nx_readback_shape(const D3D12_RESOURCE_DESC*);
bool cvr_nx_plane_bounds(const NativeExitPlane*,UINT,UINT,uint64_t,uint64_t*);
bool cvr_nx_validate_plane(const NativeExitPlane*,UINT,UINT,uint64_t,const D3D12_SUBRESOURCE_FOOTPRINT*,UINT,uint64_t);
bool cvr_nx_complete_planes(const NativeExitPlane*,size_t);
bool cvr_nx_compare_plane(UINT,UINT,const std::array<PlaneData,3>*,Comparison*);
void cvr_nx_accumulate(uint64_t*,UINT,const NativeExitPlane*,UINT,const Comparison*);
bool cvr_nx_rate_value(uint8_t);
bool cvr_nx_always_finer(const uint8_t*,size_t,const uint8_t*,size_t);
bool cvr_nx_rate_map_shape(UINT,UINT,UINT,UINT,UINT,size_t);
bool cvr_nx_attribute_color(UINT,UINT,const std::array<PlaneData,2>*,UINT,UINT,UINT,const uint8_t*,size_t,const uint8_t*,size_t,
    std::array<uint64_t,2>*);
void cvr_nx_acceptance(bool,const uint64_t*,const uint64_t*,const uint64_t*,const uint64_t*,std::array<uint64_t,8>*);
}
inline UINT PlaneIndex(UINT slot,UINT plane){return cvr_nx_plane_index(slot,plane);}
inline bool ReadbackShape(const D3D12_RESOURCE_DESC& desc){return cvr_nx_readback_shape(&desc);}
inline bool PlaneBounds(const NativeExitPlane& row,UINT width,UINT height,uint64_t bufferBytes,uint64_t* end=nullptr){
    return cvr_nx_plane_bounds(&row,width,height,bufferBytes,end);
}
inline bool ValidatePlane(const NativeExitPlane& row,UINT width,UINT height,uint64_t bufferBytes,
    const D3D12_SUBRESOURCE_FOOTPRINT& expected,UINT expectedRows,uint64_t expectedRowBytes){
    return cvr_nx_validate_plane(&row,width,height,bufferBytes,&expected,expectedRows,expectedRowBytes);
}
inline bool CompletePlanes(std::span<const NativeExitPlane> planes){return cvr_nx_complete_planes(planes.data(),planes.size());}
#else
inline UINT PlaneIndex(UINT slot,UINT plane){return slot<3 && !plane?slot:slot==4 && plane<2?3+plane:UINT_MAX;}
inline bool ReadbackShape(const D3D12_RESOURCE_DESC& desc){
    return desc.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER && desc.Width && desc.Width<=MaxReadbackBytes &&
        desc.Height==1 && desc.DepthOrArraySize==1 && desc.MipLevels==1 && desc.Format==DXGI_FORMAT_UNKNOWN &&
        desc.SampleDesc.Count==1 && !desc.SampleDesc.Quality && desc.Layout==D3D12_TEXTURE_LAYOUT_ROW_MAJOR && !desc.Flags;
}
inline bool PlaneBounds(const NativeExitPlane& row,UINT width,UINT height,uint64_t bufferBytes,uint64_t* end=nullptr){
    if(!width || width>4096 || !height || height>4096 || !bufferBytes || bufferBytes>MaxReadbackBytes ||
        PlaneIndex(row.slot,row.plane)==UINT_MAX || row.rows!=height)return false;
    const auto& footprint=row.layout.Footprint;
    const bool stencil=row.slot==4 && row.plane;
    if(row.bytes!=(stencil?1u:4u) || row.component!=(stencil && row.stride==8?4u:0u) ||
        (row.slot<3?row.stride!=4:row.stride!=4 && row.stride!=8 && !(stencil && row.stride==1)) ||
        footprint.Format==DXGI_FORMAT_UNKNOWN || footprint.Width!=width || footprint.Height!=height || footprint.Depth!=1 ||
        row.layout.Offset%D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT || !footprint.RowPitch ||
        footprint.RowPitch%D3D12_TEXTURE_DATA_PITCH_ALIGNMENT || row.layout.Offset>bufferBytes)return false;
    const auto rowBytes=uint64_t(width)*row.stride;
    if(rowBytes>footprint.RowPitch || row.component+row.bytes>row.stride)return false;
    const auto payload=uint64_t(row.rows-1)*footprint.RowPitch+rowBytes;
    if(payload>bufferBytes-row.layout.Offset)return false;
    if(end)*end=row.layout.Offset+payload;return true;
}
inline bool ValidatePlane(const NativeExitPlane& row,UINT width,UINT height,uint64_t bufferBytes,
    const D3D12_SUBRESOURCE_FOOTPRINT& expected,UINT expectedRows,uint64_t expectedRowBytes){
    const auto& actual=row.layout.Footprint;
    return PlaneBounds(row,width,height,bufferBytes) && expectedRows==row.rows && expectedRowBytes==uint64_t(width)*row.stride &&
        actual.Format==expected.Format && actual.Width==expected.Width && actual.Height==expected.Height &&
        actual.Depth==expected.Depth && actual.RowPitch==expected.RowPitch;
}
inline bool CompletePlanes(std::span<const NativeExitPlane> planes){
    if(planes.size()!=5)return false;UINT mask{};
    for(const auto& row:planes){const auto index=PlaneIndex(row.slot,row.plane);
        if(index==UINT_MAX || (mask&(1u<<index)))return false;mask|=1u<<index;}
    return mask==31;
}
#endif
struct Difference {
    uint64_t count{},value{},nativeValue{};UINT x=UINT_MAX,y=UINT_MAX;
};
struct Comparison {Difference reference,optimized;};
#if CVR_RUST_VRIK
inline bool ComparePlane(UINT width,UINT height,const NativeExitPlane& reference,std::span<const uint8_t> referenceData,
    const NativeExitPlane& optimized,std::span<const uint8_t> optimizedData,const NativeExitPlane& native,
    std::span<const uint8_t> nativeData,Comparison& result){
    const std::array<PlaneData,3> planes{{{&reference,referenceData.data(),referenceData.size()},
        {&optimized,optimizedData.data(),optimizedData.size()},{&native,nativeData.data(),nativeData.size()}}};
    return cvr_nx_compare_plane(width,height,&planes,&result);
}
inline void Accumulate(std::span<uint64_t,24> stats,UINT eye,const NativeExitPlane& plane,UINT width,const Comparison& comparison){
    cvr_nx_accumulate(stats.data(),eye,&plane,width,&comparison);
}
inline bool RateValue(uint8_t value){return cvr_nx_rate_value(value);}
inline bool AlwaysFiner(std::span<const uint8_t> common,std::span<const uint8_t> original){
    return cvr_nx_always_finer(common.data(),common.size(),original.data(),original.size());
}
inline bool RateMapShape(UINT width,UINT height,UINT tile,UINT mapWidth,UINT mapHeight,size_t bytes){
    return cvr_nx_rate_map_shape(width,height,tile,mapWidth,mapHeight,bytes);
}
inline bool AttributeColor(UINT width,UINT height,const NativeExitPlane& reference,std::span<const uint8_t> referenceData,
    const NativeExitPlane& native,std::span<const uint8_t> nativeData,UINT tile,UINT mapWidth,UINT mapHeight,
    std::span<const uint8_t> common,std::span<const uint8_t> original,uint64_t& inside,uint64_t& outside){
    const std::array<PlaneData,2> planes{{{&reference,referenceData.data(),referenceData.size()},{&native,nativeData.data(),nativeData.size()}}};
    std::array<uint64_t,2> counts{};
    const bool result=cvr_nx_attribute_color(width,height,&planes,tile,mapWidth,mapHeight,common.data(),common.size(),
        original.data(),original.size(),&counts);
    inside=counts[0];outside=counts[1];return result;
}
inline std::array<uint64_t,8> Acceptance(bool allowVrsRefinement,std::span<const uint64_t,24> native,
    std::span<const uint64_t,16> nativeRate,std::span<const uint64_t,12> attribution,std::span<const uint64_t,2> pairDifferent){
    std::array<uint64_t,8> result{};
    cvr_nx_acceptance(allowVrsRefinement,native.data(),nativeRate.data(),attribution.data(),pairDifferent.data(),&result);
    return result;
}
static_assert(sizeof(NativeExitPlane)==56 && offsetof(NativeExitPlane,layout)==24 && sizeof(Difference)==32 &&
    sizeof(Comparison)==64 && sizeof(PlaneData)==24,"mirrored by native_exit.rs");
#else
inline bool ComparePlane(UINT width,UINT height,const NativeExitPlane& reference,std::span<const uint8_t> referenceData,
    const NativeExitPlane& optimized,std::span<const uint8_t> optimizedData,const NativeExitPlane& native,
    std::span<const uint8_t> nativeData,Comparison& result){
    result={};
    for(const auto* row:{&reference,&optimized,&native})
        if(row->slot!=native.slot || row->plane!=native.plane || row->bytes!=native.bytes)return false;
    if(!PlaneBounds(reference,width,height,referenceData.size()) || !PlaneBounds(optimized,width,height,optimizedData.size()) ||
        !PlaneBounds(native,width,height,nativeData.size()))return false;
    for(UINT y=0;y<height;++y){
        const auto* expected=nativeData.data()+native.layout.Offset+uint64_t(y)*native.layout.Footprint.RowPitch+native.component;
        const auto* a=referenceData.data()+reference.layout.Offset+uint64_t(y)*reference.layout.Footprint.RowPitch+reference.component;
        const auto* b=optimizedData.data()+optimized.layout.Offset+uint64_t(y)*optimized.layout.Footprint.RowPitch+optimized.component;
        for(UINT x=0;x<width;++x){
            const auto record=[&](const uint8_t* value,Difference& difference){
                const auto* target=expected+size_t(x)*native.stride;
                if(!std::memcmp(value,target,native.bytes))return;
                if(!difference.count){difference.x=x;difference.y=y;std::memcpy(&difference.value,value,native.bytes);
                    std::memcpy(&difference.nativeValue,target,native.bytes);}
                ++difference.count;
            };
            record(a+size_t(x)*reference.stride,result.reference);record(b+size_t(x)*optimized.stride,result.optimized);
        }
    }
    return true;
}
inline void Accumulate(std::span<uint64_t,24> stats,UINT eye,const NativeExitPlane& plane,UINT width,const Comparison& comparison){
    const UINT delta=plane.slot<3?4:plane.plane?12:8;
    ++stats[2];stats[3]+=uint64_t(width)*plane.rows*plane.bytes;
    stats[delta+eye]+=comparison.reference.count;stats[delta+2+eye]+=comparison.optimized.count;
    stats[16+eye]+=comparison.reference.count;stats[18+eye]+=comparison.optimized.count;
}
inline bool RateValue(uint8_t value){return value==0 || value==1 || value==4 || value==5 || value==6 || value==9 || value==10;}
inline bool AlwaysFiner(std::span<const uint8_t> common,std::span<const uint8_t> original){
    if(common.empty() || common.size()!=original.size())return false;
    for(size_t i=0;i<common.size();++i)if(!RateValue(common[i]) || !RateValue(original[i]) ||
        (common[i]>>2)>(original[i]>>2) || (common[i]&3)>(original[i]&3))return false;
    return true;
}
inline bool RateMapShape(UINT width,UINT height,UINT tile,UINT mapWidth,UINT mapHeight,size_t bytes){
    return width && width<=4096 && height && height<=4096 && tile && tile<=4096 && mapWidth && mapWidth<=512 && mapHeight && mapHeight<=512 &&
        mapWidth>=(width+tile-1)/tile && mapHeight>=(height+tile-1)/tile && bytes==size_t(mapWidth)*mapHeight;
}
inline bool AttributeColor(UINT width,UINT height,const NativeExitPlane& reference,std::span<const uint8_t> referenceData,
    const NativeExitPlane& native,std::span<const uint8_t> nativeData,UINT tile,UINT mapWidth,UINT mapHeight,
    std::span<const uint8_t> common,std::span<const uint8_t> original,uint64_t& inside,uint64_t& outside){
    inside=outside=0;
    if(reference.slot>=3 || reference.slot!=native.slot || reference.plane || native.plane ||
        !PlaneBounds(reference,width,height,referenceData.size()) || !PlaneBounds(native,width,height,nativeData.size()) ||
        !RateMapShape(width,height,tile,mapWidth,mapHeight,common.size()) || !AlwaysFiner(common,original))return false;
    for(UINT y=0;y<height;++y){
        const auto* a=referenceData.data()+reference.layout.Offset+uint64_t(y)*reference.layout.Footprint.RowPitch;
        const auto* b=nativeData.data()+native.layout.Offset+uint64_t(y)*native.layout.Footprint.RowPitch;
        for(UINT x=0;x<width;++x)if(std::memcmp(a+size_t(x)*reference.stride,b+size_t(x)*native.stride,native.bytes)){
            const auto cell=size_t(y/tile)*mapWidth+x/tile;
            if(common[cell]!=original[cell])++inside;else ++outside;
        }
    }
    return true;
}
inline std::array<uint64_t,8> Acceptance(bool allowVrsRefinement,std::span<const uint64_t,24> native,
    std::span<const uint64_t,16> nativeRate,std::span<const uint64_t,12> attribution,std::span<const uint64_t,2> pairDifferent){
    std::array<uint64_t,8> result{};result[0]=allowVrsRefinement;
    result[1]=nativeRate[0]==1 && nativeRate[1]==2 && nativeRate[3]==10 && nativeRate[13]==1 && nativeRate[14]==1 &&
        !nativeRate[11] && !nativeRate[12] && !nativeRate[15];
    result[2]=attribution[3]==1 && attribution[4]==1 && attribution[9]==1 && attribution[10]==1 && attribution[11]==1;
    for(UINT eye=0;eye<2;++eye){
        uint64_t unexplained=attribution[7+eye];
        for(UINT index:{8u,10u,12u,14u})unexplained=native[index+eye]>UINT64_MAX-unexplained?UINT64_MAX:unexplained+native[index+eye];
        result[3+eye]=unexplained;result[5+eye]=pairDifferent[eye];
    }
    const bool compared=native[0]==1 && native[1]==2 && native[2]==10 && native[20]==1;
    const bool strict=native[21]==1 && !native[16] && !native[17] && !native[18] && !native[19];
    const bool approved=allowVrsRefinement && result[1] && result[2] && !result[3] && !result[4];
    result[7]=compared && !native[22] && !nativeRate[15] && !result[5] && !result[6] && (strict || approved);
    return result;
}
#endif
}
struct Source {
    struct FullT5 {
        structured_buffer_snapshot::PreparedCopy copy;
        structured_buffer_snapshot::PrivateRange range;
        D3D12_SHADER_RESOURCE_VIEW_DESC view{};
    };
    uint64_t ordinal{};
    native_recording::Lease sourceRecording;
    Microsoft::WRL::ComPtr<ID3D12CommandQueue> sourceQueue;
    geometry::DrawResources original,owned;
    Microsoft::WRL::ComPtr<ID3D12RootSignature> root;
    std::shared_ptr<const shader_snapshot::Snapshot> shader;
    std::vector<source_cbv_snapshot::PrivateCbv> cbvs;
    source_cbv_snapshot::PreparedCopies cbvReceipt;
    std::vector<SharedBankTable> tables;
    std::vector<SharedBankTable> referenceTables;
    std::vector<Material> materials;
    // Original selector predictions, checked against this source occurrence's
    // finalized GENERIC_READ GPU copies before any speculative material
    // descriptor may be consumed. Predicate failure cannot repair source state.
    std::vector<ShaderInputCheck> inputChecks;
    std::optional<source_binding_assembly::GuardedT5View> t5;
    // Complete original-index T5 data, independently captured at this source
    // occurrence. Dynamic texture-table access still needs its own contract.
    std::optional<FullT5> fullT5;
    Microsoft::WRL::ComPtr<ID3D12Resource> rateImage;
    // Full private pass-entry checkpoint, before any selected geometry. Both
    // comparison outputs inherit its colors, depth and stencil without clears.
    std::shared_ptr<GeometryTargetCheckpoint> passBaseline;
    // Optional direct comparison against the actual exit of this native pass.
    // Every source in an eye must share the same exit and baseline checkpoint.
    std::shared_ptr<const NativeExit> nativeExit;
    // Explicit caller opt-in. All sources must agree, and native exit evidence
    // must prove that every difference is explained by a finer common VRS map.
    bool allowVrsRefinement{};
    // Optional compatibility alias; when supplied it must equal baseline depth.
    Microsoft::WRL::ComPtr<ID3D12Resource> passDepth;
    std::shared_ptr<StereoVrsSnapshot> rateCapture;
    std::vector<Microsoft::WRL::ComPtr<IUnknown>> owners;
};
// Elements are left/side1, right/side0. Caller supplies completed, owned source
// copies; GPU predicates select paired/single geometry in the consuming recording.
bool CompatibleSources(const Source&,const Source&);
uint32_t CompatibilityFailure(const Source&,const Source&);
bool Start(ID3D12Device*,std::array<Source,2>,std::shared_ptr<const families::Family>);
bool StartSequence(ID3D12Device*,std::vector<Source>);
// Records an owned DIRECT consumer from finalized, submission-certified source
// copies before CPU readback. No source completion or queue ordering is inferred.
bool StartCurrentSequence(ID3D12Device*,std::vector<Source>,ID3D12CommandQueue*,UINT frame);
// Trusted caller only, after both original source submissions on this same queue
// and frame. Separate Execute calls establish the source-to-consumer ordering.
bool SubmitCurrent(ID3D12CommandQueue*,UINT frame);
// Optional diagnostic native output. All original draws remain enabled. The
// access receipt and all eight borrowed lifetimes must belong to this frame.
bool WantsNativeExport(UINT frame);
bool PrepareNativeExport(ID3D12CommandQueue*,UINT frame,const target_access::Receipt&,
                         const std::array<target_lifetime::BorrowedScope,8>&);
bool CurrentActive(UINT frame);
// Discards only an unsubmitted owned list; potential execution is quarantined.
void CancelCurrent(UINT frame);
bool Active();
bool NeedsNativeBindings();
void Poll();
void Draw(ID3D12GraphicsCommandList*,const native_probe::DrawRecord&);
void BeforeSubmit(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
void Submitted(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
void Reset(ID3D12GraphicsCommandList*);
}
extern "C" {
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_PreparedActualMembershipEnable;
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedActualMembershipStats[24],CyberpunkVR_PreparedActualMembershipRows[32*512];
__declspec(dllexport) extern uint32_t CyberpunkVR_PreparedActualMembershipCount,CyberpunkVR_PreparedActualMembershipRowBytes;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_PreparedNativeExportEnable;
// requested,prepared,submitted,completed,planes,bytes,differentLeft,differentRight,
// compared,preservedOnInputReject,inputRejected,error,sourceFrame,exportList,
// targetHeaps,residencyAcquired,residencySubmitted,residencyFenced,residencyRetired,
// residencyHRESULT,accessRevalidated,lifetimesRevalidated,CPUwaits0,nativeSuppressed0.
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedNativeExport[24];
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_PreparedPairRequest,CyberpunkVR_PreparedPairState;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_PreparedPairSelection;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_PreparedPairSelectionCount;
// Diagnostic only: flip one expected selector byte, never an actual shader input.
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_PreparedPairInputFault;
// Request1 selects one pair; request2 compares the whole admitted source sequence.
// State3 waits draw recording,4 draw fence,5 pass,6 fail. Values1/2 are reserved.
// Stats: sourceLeft,sourceRight,meshBytes,meshEqual,referenceDraws,pairedDraws,
// differenceLeft,differenceRight,referenceColorDeltaLeft,referenceColorDeltaRight,error,stage,
// destinationList,generation,registeredAssets,nativeSuppressed(always0).
// Owned mode reports its private list/serial, never a native recording token.
// pairedDraws is the executed GPU path count, including singles and mismatches.
// meshEqual means at least one verified shared branch and no disabled pair;
// individual mismatches use fallback and are listed in MeshResults below.
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedPairStats[16];
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedPairCandidates[16];
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedPairStateMismatch[24];
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedPairVrs[8];
// unique source images, GPU dispatches, diagnostic readback bytes, differing rate cells.
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedPairVrsGpu[4];
// Three draws (left reference, right reference, paired): 11 pipeline counters + occlusion.
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedPairGpu[36];
// Absolute nonzero color occupancy, reference then paired, eye-major, three
// targets per eye. Baseline-relative changes are reported separately below.
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedPairColor[12];
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedPairDepthState[16];
// pairs,ranges,comparedBytes,dispatches,equal,fallback,disabled,resultReadbackBytes,
// meshReadbackBytes(always0),preDrawCpuFenceWaits(always0),pairedRecorded,fallbackRecorded.
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedPairMeshStats[12];
// sources,ranges,comparedBytes,dispatches,expectedUploadBytes,accepted,rejected,resultReadbackBytes.
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedPairInputStats[8];
// source leases, physical pageables, lifecycle state, marked submitted,
// associated fence, retired, last HRESULT, process quarantine count.
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedPairResidency[8];
// mode(0 native callback,1 owned), sourceFrame, recordedFrame, submittedFrame,
// recorded, closed, submitted, completed, sourceCount, nativeCallbacksUsed,
// CPUCompletionWaits(always0), nativeSuppressed(always0), primaryError, assetLeases,
// reserved0, reserved1.
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedCurrent[16];
// baselineEyes,colors,seededOutputs,comparedPlanes,baselinePayloadBytes,
// baselineNonzeroColorPixels,refColorDeltaL/R,optColorDeltaL/R,refDepthDeltaL/R,
// optDepthDeltaL/R,refStencilDeltaL/R,optStencilDeltaL/R,rejectedUnchanged,
// rejectedExecutionZero,positiveExecution,fullBaselineCompared,inputRejected,queriesRead.
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedBaseline[24];
// mode,eyes,planes,bytes,refColorDiffL/R,optColorDiffL/R,refDepthDiffL/R,
// optDepthDiffL/R,refStencilDiffL/R,optStencilDiffL/R,refTotalL/R,optTotalL/R,
// compared,nativeEquivalent,inputRejected,abiVersion(1).
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedNativeExit[24];
// Ten plane rows of16: eye,slot,plane,refDiff,optDiff,refFirstX,refFirstY,
// refValue,nativeAtRef,optFirstX,optFirstY,optValue,nativeAtOpt,stride,component,
// bytes. Coordinates are UINT64_MAX when that version has no difference.
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedNativeExitSamples[160];
// Diagnostic original-PSO reference with each source's original VRS image:
// mode,eyes,recordedDraws,comparedPlanes,payloadBytes,diffColorL/R,diffDepthL/R,
// diffStencilL/R,totalL/R,compared,equivalent,inputRejected.
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedNativeRate[16];
// tileSize,mapWidth,mapHeight,eyeMapsUniformL/R,refColorDiffInsideL/R,
// refColorDiffOutsideL/R,originalRateCompared,commonAlwaysFiner,valid.
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedVrsAttribution[12];
// policy(0 strict,1 approved VRS refinement),nativeRateEquivalent,
// attributionValidAndFiner,unexplainedL/R,pairDifferentL/R,accepted.
// Unexplained includes outside-tile reference color plus both variants' native
// depth/stencil differences. Raw native-equivalence diagnostics stay unchanged.
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedNativeAcceptance[8];
// Two uint64 predicates per planned pair: equal, fallback. At most256 pairs.
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedPairMeshResults[512];
// leftSources,rightSources,candidatePairs,sharedDraws,leftOnly,rightOnly,
// plannedDraws,referenceRecorded,optimizedRecorded,meshFallbacks,bankRejected,
// orderVerified,targetInitializations,imageReadbacks,nativeSuppressed,sequenceMode.
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedBatchStats[16];
// Source rows: ordinal, eye index. Plan rows: mask, left ordinal, right ordinal,
// paired geometry index (UINT64_MAX for source-only emission).
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedBatchSources[1024];
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedBatchPlan[4096];
__declspec(dllexport) extern uint32_t CyberpunkVR_PreparedBatchSourceCount,CyberpunkVR_PreparedBatchPlanCount;
// reference descriptors, shared-pass descriptors, shared draws, extra SRV snapshots.
__declspec(dllexport) extern uint64_t CyberpunkVR_PreparedBatchDescriptors[4];
}
