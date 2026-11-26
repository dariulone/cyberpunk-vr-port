#include "Render/GpuResidencyLease.hpp"
#include "Render/NativeTextureRead.hpp"
#include <atomic>
#include <climits>
#include <new>
#include <utility>
#include <vector>

namespace cvr::gpu::residency {
using Microsoft::WRL::ComPtr;
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's leases are rust/crates/cvr-render/src/prepared_pair_native.rs' (its only user)
#if CVR_RUST_VRIK
// The lease's SDK residency -- its owners, transaction, queue and fence, the accounting: rust/crates/cvr-render/src/
// residency_lease.rs. The native authentication of material sources and borrowed targets, their records and the
// quarantined list stay here.
struct RustLeaseMember {const void* lease;void* storage;};
extern "C" {
HRESULT cvr_rl_begin(ID3D12Device*,size_t,bool,uint32_t,void**);
void cvr_rl_free(void*);
HRESULT cvr_rl_add(void*,ID3D12Pageable*);
HRESULT cvr_rl_acquire(void*);
size_t cvr_rl_objects(const void*);
uint32_t cvr_rl_status(const void*);
HRESULT cvr_rl_submit(ID3D12CommandQueue*,const RustLeaseMember*,size_t,bool(*)(void*,size_t),void*);
HRESULT cvr_rl_fence(void*,ID3D12CommandQueue*,ID3D12Fence*,uint64_t,HRESULT);
HRESULT cvr_rl_release(void*,uint32_t);
bool cvr_rl_quarantine(void*);
bool cvr_rl_healthy();
uint64_t cvr_rl_quarantined();
bool cvr_rl_same_owner(ID3D12Pageable*,ID3D12Pageable*);
}
namespace detail {
struct Storage {
    ComPtr<ID3D12Device> device;void* rust{};
    std::vector<stereo::texture_read::ImmutablePageable> sources;
    std::vector<stereo::target_lifetime::BorrowedPageable> targets;
    Storage* next{};
    explicit Storage(ID3D12Device* value):device(value){}
    Storage(const Storage&)=delete;Storage& operator=(const Storage&)=delete;
    ~Storage(){cvr_rl_free(rust);}
};
}
namespace {
std::atomic<detail::Storage*> quarantined{};
}
#else
namespace detail {
struct Storage {
    Transaction transaction;
    ComPtr<ID3D12Device> device;
    ComPtr<ID3D12CommandQueue> queue;
    ComPtr<ID3D12Fence> fence;
    std::vector<stereo::texture_read::ImmutablePageable> sources;
    std::vector<stereo::target_lifetime::BorrowedPageable> targets;
    std::vector<ComPtr<ID3D12Pageable>> owners;
    std::vector<ComPtr<IUnknown>> identities;
    std::vector<ID3D12Pageable*> raw;
    Storage* next{};
};
}
namespace {
std::atomic<bool> healthy{true};
std::atomic<detail::Storage*> quarantined{};
std::atomic<uint64_t> quarantineCount{};
bool SameDevice(ID3D12Device* expected,ID3D12DeviceChild* object){
    ComPtr<ID3D12Device> actual;
    return expected && object && SUCCEEDED(object->GetDevice(IID_PPV_ARGS(&actual))) && actual.Get()==expected;
}
HRESULT ReleaseOwner(detail::Storage& storage,size_t index){
    auto* owner=storage.owners[index].Get();HRESULT hr{};
    try{hr=storage.device->Evict(1,&owner);}catch(...){hr=E_FAIL;}
    if(FAILED(hr))healthy.store(false,std::memory_order_release);return hr;
}
HRESULT AddOwner(detail::Storage& storage,const ComPtr<ID3D12Pageable>& pageable){
    ComPtr<IUnknown> identity;
    if(!pageable || FAILED(pageable.As(&identity)) || !identity)return E_NOINTERFACE;
    for(const auto& saved:storage.identities)if(saved==identity)return S_OK;
    storage.owners.push_back(pageable);storage.identities.push_back(std::move(identity));storage.raw.push_back(pageable.Get());return S_OK;
}
bool SameOwner(const ComPtr<ID3D12Pageable>& a,const ComPtr<ID3D12Pageable>& b){
    ComPtr<IUnknown> first,second;return a && b && SUCCEEDED(a.As(&first)) && SUCCEEDED(b.As(&second)) && first && first==second;
}
}
#endif
Lease::Lease()=default;
Lease::~Lease(){Quarantine();}
Lease::Lease(Lease&& other)noexcept:storage_(std::move(other.storage_)),terminal_(other.terminal_),objects_(other.objects_),sources_(other.sources_){
    other.terminal_=State::Empty;other.objects_=other.sources_=0;
}
Lease& Lease::operator=(Lease&& other)noexcept{
    if(this!=&other){Quarantine();storage_=std::move(other.storage_);terminal_=other.terminal_;objects_=other.objects_;sources_=other.sources_;
        other.terminal_=State::Empty;other.objects_=other.sources_=0;}return *this;
}
#if CVR_RUST_VRIK
State Lease::Status()const noexcept{return storage_?State(cvr_rl_status(storage_->rust)):terminal_;}
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool Healthy()noexcept{return cvr_rl_healthy();}
#endif
uint64_t QuarantinedCount()noexcept{return cvr_rl_quarantined();}
HRESULT Lease::AcquireStorage(std::unique_ptr<detail::Storage> next,size_t sourceCount){
    const auto hr=cvr_rl_acquire(next->rust);
    if(FAILED(hr) && State(cvr_rl_status(next->rust))!=State::Quarantined)return hr;
    objects_=cvr_rl_objects(next->rust);sources_=sourceCount;storage_=std::move(next);
    if(FAILED(hr))Quarantine();return hr;
}
HRESULT Lease::Acquire(ID3D12Device* device,std::span<const stereo::texture_read::ImmutableLease> sources)try{
    auto next=std::make_unique<detail::Storage>(device);
    const auto admitted=cvr_rl_begin(device,sources.size(),bool(storage_),UINT(terminal_),&next->rust);if(FAILED(admitted))return admitted;
    next->sources.reserve(sources.size());
    for(const auto& source:sources){stereo::texture_read::ImmutablePageable physical;
        if(!stereo::texture_read::GetImmutablePageable(device,source,physical))return E_ACCESSDENIED;
        const auto added=cvr_rl_add(next->rust,physical.pageable.Get());if(FAILED(added))return added;
        next->sources.push_back(std::move(physical));
    }
    return AcquireStorage(std::move(next),sources.size());
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}catch(...){return E_FAIL;}
HRESULT Lease::AcquireBorrowedTargets(ID3D12Device* device,std::span<const BorrowedTarget> targets)try{
    auto next=std::make_unique<detail::Storage>(device);
    const auto admitted=cvr_rl_begin(device,targets.size(),bool(storage_),UINT(terminal_),&next->rust);if(FAILED(admitted))return admitted;
    next->targets.reserve(targets.size());
    for(const auto& target:targets){stereo::target_lifetime::BorrowedPageable physical;
        if(!stereo::target_lifetime::GetBorrowedPageable(device,target.identity,target.lifetime,physical))return E_ACCESSDENIED;
        const auto added=cvr_rl_add(next->rust,physical.pageable.Get());if(FAILED(added))return added;
        next->targets.push_back(std::move(physical));
    }
    return AcquireStorage(std::move(next),targets.size());
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}catch(...){return E_FAIL;}
#else
State Lease::Status()const noexcept{return storage_?storage_->transaction.Status():terminal_;}
bool Healthy()noexcept{return healthy.load(std::memory_order_acquire);}
uint64_t QuarantinedCount()noexcept{return quarantineCount.load(std::memory_order_acquire);}
HRESULT Lease::AcquireStorage(std::unique_ptr<detail::Storage> next,size_t sourceCount){
    if(!Healthy())return E_ACCESSDENIED;
    // The SDK transaction is all-or-nothing, including driver-internal batches:
    // https://microsoft.github.io/DirectX-Specs/d3d/ResourceHeaps.html#make-heaps--other-objects-resident
    const auto hr=next->transaction.Acquire(next->raw.size(),[&]{return next->device->MakeResident(UINT(next->raw.size()),next->raw.data());});
    if(FAILED(hr) && next->transaction.Status()!=State::Quarantined)return hr;
    objects_=next->owners.size();sources_=sourceCount;storage_=std::move(next);
    if(FAILED(hr))Quarantine();return hr;
}
HRESULT Lease::Acquire(ID3D12Device* device,std::span<const stereo::texture_read::ImmutableLease> sources)try{
    if(!Healthy())return E_ACCESSDENIED;
    if(storage_ || (terminal_!=State::Empty && terminal_!=State::Retired) || !device ||
        sources.empty() || sources.size()>UINT_MAX || device->GetNodeCount()!=1)return E_INVALIDARG;
    auto next=std::make_unique<detail::Storage>();next->device=device;
    next->sources.reserve(sources.size());next->owners.reserve(sources.size());next->identities.reserve(sources.size());next->raw.reserve(sources.size());
    for(const auto& source:sources){stereo::texture_read::ImmutablePageable physical;
        if(!stereo::texture_read::GetImmutablePageable(device,source,physical))return E_ACCESSDENIED;
        const auto added=AddOwner(*next,physical.pageable);if(FAILED(added))return added;
        next->sources.push_back(std::move(physical));
    }
    return AcquireStorage(std::move(next),sources.size());
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}catch(...){return E_FAIL;}
HRESULT Lease::AcquireBorrowedTargets(ID3D12Device* device,std::span<const BorrowedTarget> targets)try{
    if(!Healthy())return E_ACCESSDENIED;
    if(storage_ || (terminal_!=State::Empty && terminal_!=State::Retired) || !device ||
        targets.empty() || targets.size()>UINT_MAX || device->GetNodeCount()!=1)return E_INVALIDARG;
    auto next=std::make_unique<detail::Storage>();next->device=device;
    next->targets.reserve(targets.size());next->owners.reserve(targets.size());next->identities.reserve(targets.size());next->raw.reserve(targets.size());
    for(const auto& target:targets){stereo::target_lifetime::BorrowedPageable physical;
        if(!stereo::target_lifetime::GetBorrowedPageable(device,target.identity,target.lifetime,physical))return E_ACCESSDENIED;
        const auto added=AddOwner(*next,physical.pageable);if(FAILED(added))return added;
        next->targets.push_back(std::move(physical));
    }
    return AcquireStorage(std::move(next),targets.size());
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}catch(...){return E_FAIL;}
#endif
HRESULT Lease::MarkSubmitted(ID3D12CommandQueue* queue){
    std::array<Lease*,1> group{this};return MarkSubmittedTogether(queue,group);
}
#if CVR_RUST_VRIK
HRESULT Lease::MarkSubmittedTogether(ID3D12CommandQueue* queue,std::span<Lease*> group)try{
    std::vector<RustLeaseMember> members;members.reserve(group.size());
    for(auto* lease:group)members.push_back({lease,lease && lease->storage_?lease->storage_->rust:nullptr});
    // Borrowed target authority expires with the native invocation: each member's targets authenticate again.
    const auto current=[](void* context,size_t index)->bool{const auto& storage=*(*static_cast<std::span<Lease*>*>(context))[index]->storage_;
        for(const auto& target:storage.targets){stereo::target_lifetime::BorrowedPageable now;
            if(!stereo::target_lifetime::GetBorrowedPageable(storage.device.Get(),target.identity,target.lifetime,now) ||
                !cvr_rl_same_owner(now.pageable.Get(),target.pageable.Get()))return false;}
        return true;};
    return cvr_rl_submit(queue,members.data(),members.size(),current,&group);
}catch(...){return E_FAIL;}
HRESULT Lease::AssociateFence(ID3D12CommandQueue* queue,ID3D12Fence* fence,uint64_t value,HRESULT signalResult){
    if(!storage_ || State(cvr_rl_status(storage_->rust))!=State::Submitted)return E_UNEXPECTED;
    const auto hr=cvr_rl_fence(storage_->rust,queue,fence,value,signalResult);if(FAILED(hr))Quarantine();return hr;
}
HRESULT Lease::FinishRelease(HRESULT hr)noexcept{
    const auto state=State(cvr_rl_status(storage_->rust));
    if(state==State::Quarantined)Quarantine();else if(state==State::Retired){storage_.reset();terminal_=State::Retired;}
    return hr;
}
HRESULT Lease::Cancel(){return storage_?FinishRelease(cvr_rl_release(storage_->rust,0)):E_UNEXPECTED;}
HRESULT Lease::Retire(){return storage_?FinishRelease(cvr_rl_release(storage_->rust,1)):E_UNEXPECTED;}
void Lease::Quarantine()noexcept{
    if(!storage_)return;
    if(!cvr_rl_quarantine(storage_->rust)){storage_.reset();return;}
    auto* abandoned=storage_.release();abandoned->next=quarantined.load(std::memory_order_relaxed);
    while(!quarantined.compare_exchange_weak(abandoned->next,abandoned,std::memory_order_release,std::memory_order_relaxed)){}
    terminal_=State::Quarantined;
}
#else
HRESULT Lease::MarkSubmittedTogether(ID3D12CommandQueue* queue,std::span<Lease*> group)try{
    if(!queue || group.empty() || group.size()>UINT_MAX || queue->GetDesc().Type!=D3D12_COMMAND_LIST_TYPE_DIRECT)return E_INVALIDARG;
    for(size_t i=0;i<group.size();++i){
        if(!group[i])return E_INVALIDARG;
        for(size_t j=0;j<i;++j)if(group[i]==group[j])return E_INVALIDARG;
    }
    for(const auto* lease:group){
        if(!lease->storage_ || lease->storage_->transaction.Status()!=State::Acquired ||
            !SameDevice(lease->storage_->device.Get(),queue))return E_INVALIDARG;
        // Borrowed target authority expires with the native invocation. Check
        // every member before any member becomes potentially submitted.
        for(const auto& target:lease->storage_->targets){stereo::target_lifetime::BorrowedPageable current;
            if(!stereo::target_lifetime::GetBorrowedPageable(lease->storage_->device.Get(),target.identity,target.lifetime,current) ||
                !SameOwner(current.pageable,target.pageable))return E_ACCESSDENIED;
        }
    }
    // Retain queues before changing ledger state. The unique, acquired members
    // cannot change under caller serialization; these final commits cannot fail.
    for(auto* lease:group)lease->storage_->queue=queue;
    for(auto* lease:group)lease->storage_->transaction.MarkSubmitted();
    return S_OK;
}catch(...){return E_FAIL;}
HRESULT Lease::AssociateFence(ID3D12CommandQueue* queue,ID3D12Fence* fence,uint64_t value,HRESULT signalResult){
    if(!storage_ || storage_->transaction.Status()!=State::Submitted)return E_UNEXPECTED;
    const auto result=FAILED(signalResult)?signalResult:
        !queue || storage_->queue.Get()!=queue || !fence || !SameDevice(storage_->device.Get(),fence)?E_INVALIDARG:signalResult;
    const auto hr=storage_->transaction.AssociateFence(value,result);
    if(FAILED(hr))Quarantine();else storage_->fence=fence;return hr;
}
HRESULT Lease::FinishRelease(HRESULT hr)noexcept{
    if(storage_->transaction.Status()==State::Quarantined)Quarantine();
    else if(storage_->transaction.Status()==State::Retired){storage_.reset();terminal_=State::Retired;}
    return hr;
}
HRESULT Lease::Cancel(){
    if(!storage_)return E_UNEXPECTED;
    const auto hr=storage_->transaction.Cancel([&](size_t index){return ReleaseOwner(*storage_,index);});
    return FinishRelease(hr);
}
HRESULT Lease::Retire(){
    if(!storage_)return E_UNEXPECTED;
    if(storage_->transaction.Status()==State::Submitted){Quarantine();return E_UNEXPECTED;}
    if(storage_->transaction.Status()!=State::Fenced || !storage_->fence)return E_ACCESSDENIED;
    const auto completed=storage_->fence->GetCompletedValue();
    const auto hr=storage_->transaction.Retire(completed,[&](size_t index){return ReleaseOwner(*storage_,index);});
    return FinishRelease(hr);
}
void Lease::Quarantine()noexcept{
    if(!storage_)return;
    const auto state=storage_->transaction.Status();
    if(state==State::Empty || state==State::Retired){storage_.reset();return;}
    healthy.store(false,std::memory_order_release);storage_->transaction.Quarantine();
    auto* abandoned=storage_.release();abandoned->next=quarantined.load(std::memory_order_relaxed);
    while(!quarantined.compare_exchange_weak(abandoned->next,abandoned,std::memory_order_release,std::memory_order_relaxed)){}
    ++quarantineCount;terminal_=State::Quarantined;
}
#endif
#endif
}
