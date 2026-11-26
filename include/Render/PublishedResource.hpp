#pragma once
#include <atomic>
#include <mutex>
#include <wrl/client.h>

namespace cvr::gpu {
// Publication owns only the current resource. A consumer must acquire its own
// reference and keep it until its GPU work completes, not only until submission.
template<class T> class PublishedResource {
    mutable std::mutex mutex;
    Microsoft::WRL::ComPtr<T> current;
    std::atomic<T*> identity{nullptr};
public:
    bool Publish(T* resource) {
        Microsoft::WRL::ComPtr<T> next;
        {
            std::lock_guard lock(mutex);
            if(current.Get()==resource)return false;
            next=resource;
            current.Swap(next);
            identity.store(resource,std::memory_order_release);
        }
        // Release outside the publication lock: driver destruction can be slow.
        return true;
    }
    Microsoft::WRL::ComPtr<T> Acquire() const {
        std::lock_guard lock(mutex);
        return current;
    }
    // Equality/diagnostics only. This is not a lease and must not be dereferenced.
    T* Identity() const { return identity.load(std::memory_order_acquire); }
};
}
