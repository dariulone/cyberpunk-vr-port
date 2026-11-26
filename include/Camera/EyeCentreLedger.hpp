#pragma once
#include <array>
#include <cstdint>
#include <mutex>
#include <algorithm>
#include <cmath>
#include <limits>
#if CVR_RUST_ROOM
#include "Rust/CvrRust.h"
#endif

namespace cvr::camera {
#if CVR_RUST_ROOM
// rust/crates/cvr-camera/src/eye_centre.rs, owned by this object (its own lock inside).
class EyeCentreLedger {
public:
    using Position = std::array<int32_t, 3>;
    using WorldPosition = std::array<float, 3>;
    EyeCentreLedger() : m_rust(cvr_eye_ledger_new()) {}
    ~EyeCentreLedger() { cvr_eye_ledger_free(m_rust); }
    EyeCentreLedger(const EyeCentreLedger&) = delete;
    EyeCentreLedger& operator=(const EyeCentreLedger&) = delete;
    void Publish(Position eye, Position centre) { Publish(eye, centre, centre); }
    void Publish(Position eye, Position centre, Position bodyBase) {
        cvr_eye_ledger_publish(m_rust, eye.data(), centre.data(), bodyBase.data());
    }
    bool Read(Position eye, Position& centre, Position* bodyBase = nullptr) {
        return cvr_eye_ledger_read(m_rust, eye.data(), centre.data(), bodyBase ? bodyBase->data() : nullptr);
    }
    bool ReadWorld(WorldPosition eyeOrCentre, WorldPosition& centre, WorldPosition* bodyBase = nullptr) {
        return cvr_eye_ledger_read_world(m_rust, eyeOrCentre.data(), centre.data(), bodyBase ? bodyBase->data() : nullptr);
    }
    static WorldPosition World(Position p) {
        return {float(p[0])/131072.0f, float(p[1])/131072.0f, float(p[2])/131072.0f};
    }
private:
    void* m_rust;
};
#else
class EyeCentreLedger {
public:
    using Position = std::array<int32_t, 3>;
    using WorldPosition = std::array<float, 3>;
    void Publish(Position eye, Position centre) {
        Publish(eye,centre,centre);
    }
    void Publish(Position eye, Position centre, Position bodyBase) {
        std::lock_guard lock(m_mutex);
        m_records[m_next++ % m_records.size()] = {eye, centre, bodyBase};
    }
    bool Read(Position eye, Position& centre, Position* bodyBase = nullptr) {
        std::lock_guard lock(m_mutex);
        const auto n = std::min<uint64_t>(m_next, m_records.size());
        for (uint64_t age = 1; age <= n; ++age) {
            const auto& record = m_records[(m_next-age) % m_records.size()];
            if (record.eye == eye) {
                centre = record.centre;
                if (bodyBase) *bodyBase = record.bodyBase;
                return true;
            }
        }
        return false;
    }
    bool ReadWorld(WorldPosition eyeOrCentre, WorldPosition& centre, WorldPosition* bodyBase = nullptr) {
        std::lock_guard lock(m_mutex);
        const auto n = std::min<uint64_t>(m_next, m_records.size());
        bool found = false;
        WorldPosition candidate{}, candidateBase{};
        Record candidateRecord{};
        for (uint64_t age = 1; age <= n; ++age) {
            const auto& record = m_records[(m_next-age) % m_records.size()];
            const auto eye = World(record.eye), head = World(record.centre);
            // Lua's Matrix:GetTranslation returns float metres, so match in that
            // representation. Never round a float back to an invented fixed-point
            // position or guess a nearest eye from a different animation frame.
            if (eyeOrCentre != eye && eyeOrCentre != head) continue;
            // A fixed-point eye can collapse to the same Lua float while its
            // centre lands on opposite sides of one float rounding boundary.
            // Accept only that representation uncertainty, with a0.25mm cap.
            if (found && !SameFloatCentre(candidate,head,eyeOrCentre)) return false;
            const auto base = World(record.bodyBase);
            // Equal float eye positions can alias several nearby fixed-point
            // translations. If their relative offset agrees within one fixed-
            // point unit (7.6 micrometres), the independently rounded native
            // transforms still describe the same relative frame. Rounding
            // each world coordinate independently can otherwise make the body
            // appear 1-2 float ULPs apart and starve the seated camera publisher.
            if (found && bodyBase && candidateBase != base && !SameBodyOffset(candidateRecord,record)) return false;
            // Keep the newest complete record; do not accumulate rounding
            // tolerances by replacing the candidate while walking older ones.
            if(!found) { candidate=head;candidateBase=base;candidateRecord=record;found=true; }
        }
        if (found) {
            centre = candidate;
            if (bodyBase) *bodyBase = candidateBase;
        }
        return found;
    }
    static WorldPosition World(Position p) {
        return {float(p[0])/131072.0f, float(p[1])/131072.0f, float(p[2])/131072.0f};
    }
private:
    struct Record { Position eye{}, centre{}, bodyBase{}; };
    static bool SameBodyOffset(const Record& a,const Record& b) {
        for(int k=0;k<3;++k)
            if(std::abs((int64_t(a.centre[k])-a.bodyBase[k])-(int64_t(b.centre[k])-b.bodyBase[k]))>1)return false;
        return true;
    }
    static bool SameFloatCentre(const WorldPosition& a,const WorldPosition& b,const WorldPosition& query) {
        for(int k=0;k<3;++k) {
            if(a[k]==b[k])continue;
            const float x=std::abs(query[k]);
            const float ulp=std::nextafter(x,std::numeric_limits<float>::infinity())-x;
            if(!std::isfinite(ulp) || std::abs(a[k]-b[k])>std::min(ulp,.00025f))return false;
        }
        return true;
    }
    std::array<Record, 32> m_records{};
    uint64_t m_next{};
    std::mutex m_mutex;
};
#endif
}
