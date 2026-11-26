#pragma once
#include <array>
#include <algorithm>
#include <cstdint>
#include <atomic>
#include <cmath>
#include <mutex>
#include <shared_mutex>
#include <span>
#include <unordered_map>

namespace cvr::camera {
// Fingerprints validate a KNOWN object. They are never used to search for a
// similar pose: object address plus publication generation establish ancestry.
struct PoseFingerprint {
    std::array<int32_t,3> position{};
    std::array<uint32_t,4> rotation{};
    bool operator==(const PoseFingerprint&) const = default;
};

template<class Payload, size_t Capacity=8192>
class PoseAddressLedger {
    static_assert(Capacity>0);
public:
    struct Receipt {
        uintptr_t source{};
        uint64_t generation{};
        PoseFingerprint fingerprint{};
        Payload payload{};
        explicit operator bool() const { return generation!=0; }
    };
    struct BlendInput { Receipt source;PoseFingerprint current;float weight{}; };
    void Publish(uintptr_t address,const PoseFingerprint& fingerprint,const Payload& payload) {
        if(!address)return;
        std::lock_guard lock(m_mutex);
        Store(address,fingerprint,payload);
    }
    void Invalidate(uintptr_t address) {
        std::lock_guard lock(m_mutex);
        m_records.erase(address);
    }
    // Every node dispatch of both eyes reads its camera here, from all render job threads at once.
    // A shared lock lets those reads run side by side; with the exclusive one they queued and slept
    // (city stack samples, 2026-10-09). The use stamp is the only thing a read writes: one past the
    // latest store, so a camera read since then still outranks every record stored before it.
    Receipt Capture(uintptr_t address,const PoseFingerprint& fingerprint) const {
        std::shared_lock lock(m_mutex);
        const auto found=m_records.find(address);
        if(found==m_records.end() || found->second.fingerprint!=fingerprint)return {};
        auto& r=found->second;
        const uint64_t use=m_clock.load(std::memory_order_relaxed)+1;
        if(r.lastUse.value.load(std::memory_order_relaxed)!=use)r.lastUse.value.store(use,std::memory_order_relaxed);
        return {address,r.generation,r.fingerprint,r.payload};
    }
    // Capture before the engine copy; commit only after it returned. Reusing
    // the source buffer, even for identical coordinates, revokes the receipt.
    bool Transfer(const Receipt& receipt,const PoseFingerprint& sourceNow,
                  uintptr_t destination,const PoseFingerprint& destinationNow) {
        std::lock_guard lock(m_mutex);
        const auto source=m_records.find(receipt.source);
        if(!receipt || source==m_records.end() ||
           source->second.generation!=receipt.generation ||
           source->second.fingerprint!=sourceNow) {
            m_records.erase(destination);return false;
        }
        Store(destination,destinationNow,receipt.payload);
        return true;
    }
    // CameraDirector blends serialized setups without calling the copy hook.
    // Right-multiplication by a common head rotation commutes with its nlerp;
    // only inputs carrying exactly the same head sample can keep that label.
    template<class SameSample>
    bool TransferBlend(std::span<const BlendInput> inputs,uintptr_t destination,
                       const PoseFingerprint& output,SameSample sameSample) {
        std::lock_guard lock(m_mutex);
        const Payload* selected=nullptr;
        double totalWeight=0;
        for(const auto& input:inputs) {
            if(!std::isfinite(input.weight) || input.weight<0) {
                m_records.erase(destination);return false;
            }
            if(input.weight==0)continue;
            totalWeight+=input.weight;
            const auto& receipt=input.source;
            const auto source=m_records.find(receipt.source);
            if(!receipt || source==m_records.end() || source->second.generation!=receipt.generation ||
               source->second.fingerprint!=input.current ||
               (selected && !sameSample(*selected,receipt.payload))) {
                m_records.erase(destination);return false;
            }
            selected=&receipt.payload;
        }
        if(!selected || !destination || (inputs.size()>1 && std::abs(totalWeight-1.0)>1.0e-6)) {
            m_records.erase(destination);return false;
        }
        Store(destination,output,*selected);
        return true;
    }
private:
    // Written by concurrent readers under the shared lock, so atomic; copied only under the exclusive one.
    struct UseStamp {
        std::atomic<uint64_t> value;
        UseStamp(uint64_t v=0):value(v) {}
        UseStamp(const UseStamp& other):value(other.value.load(std::memory_order_relaxed)) {}
        UseStamp& operator=(const UseStamp& other) { value.store(other.value.load(std::memory_order_relaxed),std::memory_order_relaxed);return *this; }
    };
    struct Record { uint64_t generation;UseStamp lastUse;PoseFingerprint fingerprint;Payload payload; };
    void Store(uintptr_t address,const PoseFingerprint& fingerprint,const Payload& payload) {
        // A cached VRCAM descriptor can remain alive while thousands of MAIN
        // copies update a few other objects. Bound distinct objects, not the
        // number of publications: activity elsewhere must not expire its ID. Records read in the same interval share
        // a use stamp; the older publication goes first, so the choice never depends on the map's iteration order.
        if(!m_records.contains(address) && m_records.size()>=Capacity) {
            const auto oldest=std::min_element(m_records.begin(),m_records.end(),[](const auto& a,const auto& b) {
                const auto useA=a.second.lastUse.value.load(std::memory_order_relaxed);
                const auto useB=b.second.lastUse.value.load(std::memory_order_relaxed);
                return useA<useB || (useA==useB && a.second.generation<b.second.generation);
            });
            m_records.erase(oldest);
        }
        // Stores step the clock by two, so the reads in between (clock + 1) rank above this store
        // and below the next one.
        const uint64_t stamp=m_clock.load(std::memory_order_relaxed)+2;
        m_clock.store(stamp,std::memory_order_relaxed);
        m_records.insert_or_assign(address,Record{++m_generation,UseStamp{stamp},fingerprint,payload});
    }
    mutable std::shared_mutex m_mutex;
    mutable std::unordered_map<uintptr_t,Record> m_records;
    uint64_t m_generation{};
    std::atomic<uint64_t> m_clock{};
};
}
