#pragma once
#include "Render/CommandResources.hpp"
#include <atomic>
#include <cstdint>
#include <memory>
#include <new>

namespace cvr::gpu {
// The caller serializes lifecycle operations. Only completion markers run on
// other threads. A recording retains its marker even after completion so replay
// cannot reference a freed feature; Reset/destruction releases that ownership.
class IdleDlssContext {
    struct Flight { std::atomic<uint32_t> pending{}; };
    class Marker final : public IUnknown {
        std::atomic<ULONG> refs{1};
        std::shared_ptr<Flight> flight;
    public:
        explicit Marker(std::shared_ptr<Flight> f):flight(std::move(f)){++flight->pending;}
        ~Marker(){--flight->pending;}
        HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out) override {
            if(!out)return E_POINTER; *out=nullptr;
            if(id!=__uuidof(IUnknown))return E_NOINTERFACE;
            *out=static_cast<IUnknown*>(this);AddRef();return S_OK;
        }
        ULONG STDMETHODCALLTYPE AddRef() override{return ++refs;}
        ULONG STDMETHODCALLTYPE Release() override{auto n=--refs;if(!n)delete this;return n;}
    };
    std::shared_ptr<Flight> flight=std::make_shared<Flight>();
    uint64_t lastUse{};
    unsigned splitSuccesses{};
    bool resident{},unsafe{};
public:
    void Used(ID3D12GraphicsCommandList* list,uint64_t now) {
        resident=true;lastUse=now;splitSuccesses=0;
        auto* marker=new(std::nothrow) Marker(flight);
        if(!marker){unsafe=true;return;}
        if(!KeepCommandResources(list,{marker}))unsafe=true;
        marker->Release();
    }
    void SplitSucceeded(){if(splitSuccesses<2)++splitSuccesses;}
    bool Ready(uint64_t now) const {
        return resident && !unsafe && splitSuccesses>=2 && now>=lastUse && now-lastUse>=2000 && flight->pending.load()==0;
    }
    void Released(){resident=false;splitSuccesses=0;}
    void Disable(){unsafe=true;}
    uint32_t Pending() const {return flight->pending.load();}
};
}
