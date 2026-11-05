#pragma once
#include <d3d12.h>
#include <wrl/client.h>
#include <array>
#include <atomic>
#include <cstdint>
#include <mutex>

namespace cvr::render {
// The original PSO owns its variants through COM private data: they die with it, and a new PSO at the
// same address starts without them. Lookups go through an address index (PipelineVariantIndex) that
// an owner leaves when the original's destruction releases it -- before that address can come back --
// so a lookup never calls into the pipeline. Draw hooks look up the pipeline last set on their thread,
// which can be one destroyed since (a list Reset with its initial state, a bundle): GetPrivateData on
// that object took its lock and AddRef'd whatever lay there, writing into freed memory (2026-10-23:
// three crashes right after a save loaded, at three engine sites, each a read of -1).
template<size_t Count>
class PipelineVariantIndex;

template<size_t Count>
class PipelineVariants final : public IUnknown {
public:
    std::array<Microsoft::WRL::ComPtr<ID3D12PipelineState>,Count> variants;
    Microsoft::WRL::ComPtr<ID3D12PipelineState> mask;

    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out) override {
        if(!out)return E_POINTER;
        *out=nullptr;
        if(id!=__uuidof(IUnknown))return E_NOINTERFACE;
        *out=static_cast<IUnknown*>(this);AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef() override {return ++references_;}
    ULONG STDMETHODCALLTYPE Release() override {
        const ULONG left=--references_;if(!left)delete this;return left;
    }
private:
    friend class PipelineVariantIndex<Count>;
    ~PipelineVariants();
    PipelineVariantIndex<Count>* index_=nullptr;
    uintptr_t original_=0;
    std::atomic<ULONG> references_{1};
};

// The live originals that own variants, by address. Find is lock-free and only reads the index; an
// entry it returns stays valid while the original is alive (the variants are the original's).
template<size_t Count>
class PipelineVariantIndex {
public:
    using Entry=PipelineVariants<Count>;
    explicit PipelineVariantIndex(const GUID& key):key_(key) {}

    const Entry* Find(const ID3D12PipelineState* original) const {
        if(!original || !live_.load(std::memory_order_acquire))return nullptr;
        const uintptr_t address=reinterpret_cast<uintptr_t>(original);
        for(size_t i=Home(address),n=0;n<Slots;++n,i=(i+1)&(Slots-1)) {
            const uintptr_t at=slots_[i].pso.load(std::memory_order_acquire);
            if(at==address)return slots_[i].entry.load(std::memory_order_acquire);
            if(at==Empty)return nullptr;   // a tombstone keeps the probe going
        }
        return nullptr;
    }
    uint32_t Live() const {return live_.load(std::memory_order_relaxed);}
    // Bumped by every Store and Remove: an answer Find gave for an address holds while it is unchanged (a per-thread
    // cache of the last pipeline asked about, for the draw hooks that ask on every draw).
    uint64_t Generation() const {return generation_.load(std::memory_order_acquire);}

    // Consumes every supplied creation reference, including on failure or duplicate registration.
    // S_FALSE: the original already has variants (a driver may hand back an existing immutable PSO);
    // they stay, for commands already recorded with them.
    HRESULT Store(ID3D12PipelineState* original,ID3D12PipelineState* const (&created)[Count],
                  ID3D12PipelineState* createdMask) {
        Microsoft::WRL::ComPtr<Entry> entry;entry.Attach(new Entry);
        for(size_t i=0;i<Count;++i)entry->variants[i].Attach(created[i]);
        entry->mask.Attach(createdMask);
        if(!original)return E_POINTER;
        const uintptr_t address=reinterpret_cast<uintptr_t>(original);
        std::lock_guard lock(mutex_);
        if(Find(original))return S_FALSE;
        size_t slot=Slots;
        for(size_t i=Home(address),n=0;n<Slots;++n,i=(i+1)&(Slots-1)) {
            const uintptr_t at=slots_[i].pso.load(std::memory_order_relaxed);
            if(at==Empty || at==Tombstone){slot=i;break;}
        }
        if(slot==Slots || (slots_[slot].pso.load(std::memory_order_relaxed)==Empty && used_>=Limit))return E_OUTOFMEMORY;
        const HRESULT hr=original->SetPrivateDataInterface(key_,entry.Get());
        if(FAILED(hr))return hr;
        entry->index_=this;entry->original_=address;
        if(slots_[slot].pso.load(std::memory_order_relaxed)==Empty)++used_;
        slots_[slot].entry.store(entry.Get(),std::memory_order_release);
        slots_[slot].pso.store(address,std::memory_order_release);
        live_.fetch_add(1,std::memory_order_release);
        generation_.fetch_add(1,std::memory_order_acq_rel);
        return S_OK;
    }

private:
    friend class PipelineVariants<Count>;
    static constexpr size_t Slots=4096,Limit=Slots*3/4;   // power of two; probes stay short
    static constexpr uintptr_t Empty=0,Tombstone=1;
    static size_t Home(uintptr_t v) {v^=v>>21;v*=0x9E3779B97F4A7C15ull;return size_t(v>>40)&(Slots-1);}
    // The original is being destroyed: its address leaves the index before it can be reused.
    void Remove(uintptr_t address,const Entry* entry) {
        std::lock_guard lock(mutex_);
        for(size_t i=Home(address),n=0;n<Slots;++n,i=(i+1)&(Slots-1)) {
            const uintptr_t at=slots_[i].pso.load(std::memory_order_relaxed);
            if(at==Empty)return;
            if(at!=address || slots_[i].entry.load(std::memory_order_relaxed)!=entry)continue;
            slots_[i].entry.store(nullptr,std::memory_order_release);
            slots_[i].pso.store(Tombstone,std::memory_order_release);
            live_.fetch_sub(1,std::memory_order_release);
            generation_.fetch_add(1,std::memory_order_acq_rel);
            return;
        }
    }
    struct Slot {std::atomic<uintptr_t> pso{Empty};std::atomic<const Entry*> entry{nullptr};};
    GUID key_;
    std::array<Slot,Slots> slots_{};
    std::atomic<uint32_t> live_{0};
    std::atomic<uint64_t> generation_{0};
    size_t used_=0;   // slots ever taken (live or tombstone), under mutex_
    // Recursive: SetPrivateDataInterface under the lock may release an entry, whose destructor removes it.
    std::recursive_mutex mutex_;
};

template<size_t Count>
PipelineVariants<Count>::~PipelineVariants() {if(index_)index_->Remove(original_,this);}
}
