#pragma once
#include "Render/StereoPairedRoot.hpp"
#include <wrl/client.h>
#include <climits>
#include <memory>

namespace cvr::stereo {
namespace source_binding_assembly {struct OwnedSrvCoverage;}
struct BankValue {
    // CBV: exact CPU bytes, copied during Create. SRV: strong source reference,
    // explicit valid native view and uniform all-subresource state at Capture's
    // GPU position. Null SRVs require an explicit valid typed view as well.
    std::span<const uint8_t> constants;
    Microsoft::WRL::ComPtr<ID3D12Resource> source;
    D3D12_SHADER_RESOURCE_VIEW_DESC srv{};
    D3D12_RESOURCE_STATES state=D3D12_RESOURCE_STATE_COMMON;
    // Alternative CBV: an immutable owned GPU version, with no CPU upload.
    // Caller proves its contents/read state through the consuming recording.
    D3D12_CONSTANT_BUFFER_VIEW_DESC gpuCbv{};
    Microsoft::WRL::ComPtr<IUnknown> owner;
    // Alternative SRV: use the already-owned version directly, without a copy.
    // Owner retention does not itself certify a destination read epoch.
    bool borrowedSrv{},implicitSrv{};
};
struct BankPair {PairedBinding binding;std::array<BankValue,2> eyes;};
struct SharedBankTable {
    // SourcePass: exact finite-table extent, or caller-admitted finite extent
    // for a final unbounded range. Covers every accessed descriptor, all finite
    // prefix ranges and at least the unbounded range's first descriptor.
    UINT parameter{},offset{},count{};
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> cpuHeap;
    // CPU descriptors already represent the intended version and remain stable
    // during creation. Caller certifies their complete resource set; paired
    // shared tables additionally require read-only resources through the pass.
    // Descriptor equality does not prove resource contents or allocation life.
    std::vector<Microsoft::WRL::ComPtr<IUnknown>> leases;
    // Source-pass deduplication only: both labels must be nonzero. One capture
    // identity fixes its version, heap, range and type. Both zero copy each use;
    // an incomplete identity/version pair is rejected by CreateSourcePass.
    uint64_t identity{},version{};
    // Alternative explicit SourcePass storage contract, not source admission.
    // Both labels nonzero: scope fixes the original root/parameter domain and
    // this held CPU heap's [offset, offset+storageExtent) immutable owned union.
    // count remains this source's admitted extent; count<=storageExtent. Extra
    // descriptors are unused by this source's exact access certificate. All
    // union backing owners and every current draw lease must still be supplied.
    // The complete storage extent is copied once per actual range/domain, even
    // when sources have different counts. Conflicting scope labels are rejected.
    // Both zero keep legacy identity/version behavior; the two modes cannot mix.
    uint64_t storageScope{};UINT storageExtent{};
    // Issued only by source-binding assembly for the exact immutable CPU table.
    // Copied numeric labels or another heap cannot create/reuse this receipt.
    std::shared_ptr<const source_binding_assembly::OwnedSrvCoverage> srvCoverage;
};
struct BankDraw {
    ID3D12RootSignature* root{};
    const PairedRootPlan* plan{};
    std::span<const BankPair> pairs;
    std::span<const SharedBankTable> shared;
    ID3D12RootSignature* sourceRoot{}; // optional original-PSO reference, finite CBV tables
};
struct SourceBankDraw {
    // D3D12 roots do not expose their serialized bytes. Caller certifies this
    // exact captured root/blob pair, every shader-used table and access coverage
    // within its admitted count. Root layout/table masks alone are not proof.
    Microsoft::WRL::ComPtr<ID3D12RootSignature> root;
    std::span<const uint8_t> serializedRoot;
    std::span<const SharedBankTable> tables;
};
enum class SourcePassFailureStage:UINT {
    None,Arguments,Input,RootBytes,RootLayout,Table,Storage,Lease,Capacity,ResourceHeap,SamplerHeap,Copy,Exception
};
enum class SourcePassFailureReason:uint64_t {
    AlreadyInitialized=1ull<<0,MissingDevice=1ull<<1,EmptyInputs=1ull<<2,InputLimit=1ull<<3,
    RootDevice=1ull<<4,TableLimit=1ull<<5,BlobExtent=1ull<<6,BlobBudget=1ull<<7,BlobMismatch=1ull<<8,
    DecoderCreate=1ull<<9,DecoderVersion=1ull<<10,RootParameters=1ull<<11,RootFlags=1ull<<12,
    RootParameterKind=1ull<<13,RootRanges=1ull<<14,RangeType=1ull<<15,RangeCount=1ull<<16,
    RangeOrder=1ull<<17,RangeAlias=1ull<<18,RangeExtent=1ull<<19,RangeRegisters=1ull<<20,
    Parameter=1ull<<21,DuplicateParameter=1ull<<22,CpuHeap=1ull<<23,CpuHeapDevice=1ull<<24,
    LegacyLabels=1ull<<25,StorageLabels=1ull<<26,MixedLabels=1ull<<27,HeapType=1ull<<28,
    HeapFlags=1ull<<29,SourceExtent=1ull<<30,SourceOffset=1ull<<31,StorageExtent=1ull<<32,
    MissingLeases=1ull<<33,NullLease=1ull<<34,StorageConflict=1ull<<35,LegacyConflict=1ull<<36,
    DescriptorCapacity=1ull<<37,HeapCreate=1ull<<38,Handle=1ull<<39,CopyAddress=1ull<<40,AllocationException=1ull<<41,
    SamplerCapacity=1ull<<42,ResourceCapacity=1ull<<43,Stride=1ull<<44
};
struct SourcePassFailure {
    UINT stage{},input=UINT_MAX,parameter=UINT_MAX;
    uint64_t bits{};
};
class StereoBindingBank {
public:
    static constexpr UINT MaxArenaDraws=1024;
    StereoBindingBank();
    ~StereoBindingBank();
    StereoBindingBank(const StereoBindingBank&)=delete;
    StereoBindingBank& operator=(const StereoBindingBank&)=delete;
    // Transactional, single initialization. Original parameter indices stay
    // intact. Shared CPU tables are copied once and identical ranges deduplicated.
    HRESULT Create(ID3D12Device*,ID3D12RootSignature*,const PairedRootPlan&,
                   std::span<const BankPair>,std::span<const SharedBankTable> shared={});
    // Multiple materials share one resource/sampler heap. Identical constants,
    // shared descriptor ranges and per-eye source snapshots are stored once.
    HRESULT CreatePass(ID3D12Device*,std::span<const BankDraw>);
    // Original descriptor-table roots, including parameters 32..63. An unbounded
    // table supports one final tail range with explicit finite admitted count;
    // finite tables retain their exact extent. The original root is not rewritten.
    // CPU tables already hold immutable intended versions during this call.
    // Resource leases include bound CBV/SRV/UAV resources and UAV counters; no contents,
    // allocation, state, native admission or GPU position is certified here.
    // Failure leaves the bank uninitialized. Unused tables may be omitted.
    HRESULT CreateSourcePass(ID3D12Device*,std::span<const SourceBankDraw>);
    // Read after the last CreateSourcePass call returns. Records its actual
    // rejection only; success clears it. Does not change storage or admission.
    SourcePassFailure LastSourcePassFailure()const noexcept{return sourcePassFailure_;}
    // Append-only CBV arena for one immutable paired root. Existing entries are
    // never rewritten while recording. Owner proves GPU completion before reset.
    HRESULT CreateArena(ID3D12Device*,ID3D12RootSignature*,const PairedRootPlan&,UINT maxDraws,UINT64 bytes);
    HRESULT AppendConstants(const PairedRootPlan&,std::span<const BankPair>,UINT& draw);
    bool RestartArena();
    // The Rust build's test facades: the bank's storage handed over (a reference) and taken (one reference adopted).
    void* DetachStorage();
    void AttachStorage(void* storage);
    bool CapturedFor(ID3D12GraphicsCommandList*)const;
    // Record after native SRV producers, before their first overwrite/alias.
    // No CPU wait. All captures and users must execute on this one recording,
    // with the normal command-list single-recording-thread discipline. The bank
    // must be rebuilt after Reset; cached raw command-list pointers are not leases.
    // Capture separately: native eyes may reuse the SAME resource/allocation.
    bool CaptureEye(ID3D12GraphicsCommandList*,UINT eye);
    bool Bind(ID3D12GraphicsCommandList*,UINT draw=0) const;
    bool BindSourceEye(ID3D12GraphicsCommandList*,UINT eye,UINT draw=0) const;
    // Reusable immutable descriptors on caller-certified DIRECT recordings.
    // Keeps the bank through recording/submission/GPU completion before binds.
    bool BindSource(ID3D12GraphicsCommandList*,UINT draw=0) const;
    UINT DrawCount() const;
    UINT DescriptorCount() const;
    UINT SnapshotCount() const;
    UINT64 ConstantBytes() const;
private:
    struct Storage;
    Microsoft::WRL::ComPtr<Storage> storage_;
    SourcePassFailure sourcePassFailure_;
};
}
