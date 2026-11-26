#pragma once
#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <mutex>
#include <utility>
#include <wrl/client.h>

namespace cvr::gpu {
// Optional diagnostic publication. Reset/disable retires only the publication's
// owners. Consumers acquire their own snapshot and keep submitted sources until
// their existing queue fence completes. Pointer comparison never dereferences a
// borrowed source, and duplicate observations do not AddRef each frame.
template<class T,size_t Capacity> class ResourceCaptureSet {
public:
    struct Entry {
        Microsoft::WRL::ComPtr<T> resource;
        uint32_t state{},format{},tag{};
    };
    struct Snapshot {
        std::array<Entry,Capacity> entries{};
        size_t count{};
    };
private:
    mutable std::mutex mutex;
    Snapshot current;
    std::atomic<bool> enabled{false};
    std::atomic<size_t> count{0};
public:
    bool SetEnabled(bool next) {
        if(enabled.load(std::memory_order_acquire)==next)return next;
        Snapshot retired;
        {
            std::lock_guard lock(mutex);
            enabled.store(next,std::memory_order_release);
            if(!next){retired=std::move(current);current={};count.store(0,std::memory_order_release);}
        }
        return next; // retire driver objects outside the publication lock
    }
    void Observe(T* resource,uint32_t state,uint32_t format,uint32_t tag) {
        if(!resource || !enabled.load(std::memory_order_acquire))return;
        std::lock_guard lock(mutex);
        if(!enabled.load(std::memory_order_relaxed))return;
        for(size_t i=0;i<current.count;++i)if(current.entries[i].resource.Get()==resource){current.entries[i].state=state;return;}
        if(current.count==Capacity)return;
        current.entries[current.count++]={resource,state,format,tag};
        count.store(current.count,std::memory_order_release);
    }
    void Update(T* resource,uint32_t state) {
        if(!resource || !count.load(std::memory_order_acquire))return;
        std::lock_guard lock(mutex);
        for(size_t i=0;i<current.count;++i)if(current.entries[i].resource.Get()==resource){current.entries[i].state=state;return;}
    }
    Snapshot Acquire() const {std::lock_guard lock(mutex);return current;}
    bool Empty() const {return count.load(std::memory_order_acquire)==0;}
    void Reset() {
        Snapshot retired;
        {
            std::lock_guard lock(mutex);
            retired=std::move(current);current={};count.store(0,std::memory_order_release);
        }
    }
};
}
