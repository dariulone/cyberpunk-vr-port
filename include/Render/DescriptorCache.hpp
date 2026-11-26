#pragma once
#include <cstdint>
#include <list>
#include <mutex>
#include <optional>
#include <span>
#include <unordered_map>
#include <unordered_set>

namespace cvr::render {
struct DescriptorTarget {uintptr_t resource{};uint32_t width{},height{};};
// Metadata only: no COM references and no GPU allocations. CPU descriptors can
// be recycled, so a replacement publishes resource and dimensions together.
// Capacity is the floor; Reserve raises it to the number of descriptor slots the engine's heaps
// actually hold. One slot is one handle, so a cache that large never has to evict a live one.
#if CVR_RUST_VRIK
// The cache: rust/crates/cvr-render/src/descriptor_cache.rs (tools/descriptor_cache_tests runs its cases against it in
// its Rust variant). This object owns a Rust cache made with its Capacity; the Rust cache keeps its own lock.
namespace descriptor_cache_detail {
struct RustCache;
extern "C" {
RustCache* cvr_dcache_new(size_t);
void cvr_dcache_free(RustCache*);
size_t cvr_dcache_count(const RustCache*,uint32_t,size_t);
void cvr_dcache_pin(const RustCache*,const uintptr_t*,size_t);
bool cvr_dcache_store(const RustCache*,uintptr_t,const DescriptorTarget*,bool*);
bool cvr_dcache_read(const RustCache*,uintptr_t,DescriptorTarget*);
}
}
static_assert(sizeof(DescriptorTarget)==16,"mirrored by descriptor_cache.rs");
template<size_t Capacity> class DescriptorCache {
    static_assert(Capacity>0);
    descriptor_cache_detail::RustCache* rust_=descriptor_cache_detail::cvr_dcache_new(Capacity);
public:
    DescriptorCache()=default;
    DescriptorCache(const DescriptorCache&)=delete;
    DescriptorCache& operator=(const DescriptorCache&)=delete;
    ~DescriptorCache(){descriptor_cache_detail::cvr_dcache_free(rust_);}
    void Reserve(size_t slots){descriptor_cache_detail::cvr_dcache_count(rust_,0,slots);}
    size_t Limit(){return descriptor_cache_detail::cvr_dcache_count(rust_,1,0);}
    void PinResources(std::span<const uintptr_t> resources){descriptor_cache_detail::cvr_dcache_pin(rust_,resources.data(),resources.size());}
    bool Store(uintptr_t handle,DescriptorTarget target,bool* evicted=nullptr){
        return descriptor_cache_detail::cvr_dcache_store(rust_,handle,&target,evicted);
    }
    std::optional<DescriptorTarget> Read(uintptr_t handle){
        DescriptorTarget target;if(!descriptor_cache_detail::cvr_dcache_read(rust_,handle,&target))return {};return target;
    }
    size_t Size(){return descriptor_cache_detail::cvr_dcache_count(rust_,2,0);}
};
#else
template<size_t Capacity> class DescriptorCache {
    static_assert(Capacity>0);
    struct Entry {DescriptorTarget target;typename std::list<uintptr_t>::iterator recent;};
    std::mutex mutex;
    std::list<uintptr_t> recent;
    std::unordered_map<uintptr_t,Entry> entries;
    std::unordered_set<uintptr_t> pinned;
    size_t capacity=Capacity;
public:
    void Reserve(size_t slots) {std::lock_guard lock(mutex);if(slots>capacity)capacity=slots;}
    size_t Limit(){std::lock_guard lock(mutex);return capacity;}
    void PinResources(std::span<const uintptr_t> resources) {
        std::lock_guard lock(mutex);pinned.clear();
        for(auto p:resources)if(p)pinned.insert(p);
    }
    bool Store(uintptr_t handle,DescriptorTarget target,bool* evicted=nullptr) {
        if(evicted)*evicted=false;
        if(!handle)return false;
        std::lock_guard lock(mutex);
        auto found=entries.find(handle);
        if(!target.resource) {
            if(found!=entries.end()){recent.erase(found->second.recent);entries.erase(found);}
            return true;
        }
        if(found!=entries.end()) {
            found->second.target=target;recent.splice(recent.begin(),recent,found->second.recent);return true;
        }
        if(entries.size()>=capacity) {
            auto victim=recent.end();
            for(auto it=recent.rbegin();it!=recent.rend();++it) {
                if(!pinned.contains(entries.at(*it).target.resource)){victim=std::prev(it.base());break;}
            }
            if(victim==recent.end())return false;
            entries.erase(*victim);recent.erase(victim);if(evicted)*evicted=true;
        }
        recent.push_front(handle);entries.emplace(handle,Entry{target,recent.begin()});return true;
    }
    std::optional<DescriptorTarget> Read(uintptr_t handle) {
        std::lock_guard lock(mutex);const auto it=entries.find(handle);
        if(it==entries.end())return {};
        recent.splice(recent.begin(),recent,it->second.recent);return it->second.target;
    }
    size_t Size(){std::lock_guard lock(mutex);return entries.size();}
};
#endif
}
