#pragma once
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <mutex>
#include <type_traits>

namespace cvr::texture_origins {
inline constexpr uint32_t Capacity=1024, StackDepth=32;
struct Handle {uintptr_t object{},control{};};
struct Descriptor {
    uint32_t width{},height{};
    uint8_t mips{},samples{},format{},flags{};
    uint32_t reserved{};
    uintptr_t backing{};
};
struct Record {
    uint64_t ordinal{},qpc{},object{},control{},objectSerial{},caller{};
    Descriptor descriptor{};
    uint32_t thread{},stackCount{},flags{},reserved{};
    uint64_t stack[StackDepth]{};
};
static_assert(sizeof(void*)==8 && sizeof(Handle)==16 && sizeof(Descriptor)==24);
static_assert(sizeof(Record)==344 && offsetof(Record,descriptor)==48 && offsetof(Record,stackCount)==76 && offsetof(Record,flags)==80 && offsetof(Record,stack)==88);
static_assert(std::is_trivially_copyable_v<Record>);

// Multi-producer publication: writers serialize only the fixed-size copy, never
// the native factory/stack capture. External RPM readers use the exported even
// sequence before/after copying. There are no resource owners in this ring.
class Writer {
    std::mutex mutex;
    Record* rows;
    const uint32_t capacity;
    std::atomic<uint64_t>& sequence;
    std::atomic<uint64_t>& count;
    std::atomic<uint64_t>& droppedBusy;
    std::atomic<uint64_t>& overwritten;
public:
    Writer(Record* r,uint32_t n,std::atomic<uint64_t>& seq,std::atomic<uint64_t>& total,
           std::atomic<uint64_t>& dropped,std::atomic<uint64_t>& old)
        :rows(r),capacity(n),sequence(seq),count(total),droppedBusy(dropped),overwritten(old){}
    bool Publish(Record record) {
        std::unique_lock lock(mutex,std::try_to_lock);
        if(!lock.owns_lock()){droppedBusy.fetch_add(1,std::memory_order_relaxed);return false;}
        sequence.fetch_add(1,std::memory_order_acq_rel);
        const auto ordinal=count.load(std::memory_order_relaxed)+1;
        record.ordinal=ordinal;rows[(ordinal-1)%capacity]=record;
        if(ordinal>capacity)overwritten.fetch_add(1,std::memory_order_relaxed);
        count.store(ordinal,std::memory_order_relaxed);
        sequence.fetch_add(1,std::memory_order_release);
        return true;
    }
};
}

#if defined(CVR_DYNAMIC_TEXTURE_ORIGINS_EXPORTS)
extern "C" {
extern __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_DynamicTextureOriginsEnable;
// 0 not installed, 1 ready, 2 unsupported build/override, 3 signature mismatch,
// 4 CreateHook failure, 5 EnableHook failure. Optional failures affect this observer only.
extern __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_DynamicTextureOriginsStatus;
extern __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_DynamicTextureOriginsSequence;
extern __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_DynamicTextureOriginsCount;
extern __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_DynamicTextureOriginsObserved;
extern __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_DynamicTextureOriginsInFlight;
extern __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_DynamicTextureOriginsDroppedReentrant;
extern __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_DynamicTextureOriginsDroppedBusy;
extern __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_DynamicTextureOriginsOverwritten;
extern __declspec(dllexport) uint64_t CyberpunkVR_DynamicTextureOriginsQpcFrequency;
extern __declspec(dllexport) uint32_t CyberpunkVR_DynamicTextureOriginsLayout[8];
extern __declspec(dllexport) cvr::texture_origins::Record CyberpunkVR_DynamicTextureOriginsRows[cvr::texture_origins::Capacity];
}
#endif
