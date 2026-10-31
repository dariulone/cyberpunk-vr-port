#pragma once
#include <algorithm>
#include <cmath>
#include <cstdint>
namespace cvr::psvr2 {
// The v1 bridge polls one mailbox every 4 ms. Keep one pending impact per
// hand and publish at most every 8 ms so simultaneous hands cannot overwrite.
struct HapticPulseQueue {
    struct Event { int hand{}; float amplitude{}; int duration{}; uint64_t queued{}; uint64_t order{}; };
    Event pending[2]{};
    uint64_t order{}, sentAt{};
    bool sent{};
    void Push(int hand, float amplitude, int duration, uint64_t now) {
        if (hand < 0 || hand > 1 || !std::isfinite(amplitude) || amplitude <= 0 || amplitude > 1 || duration < 1 || duration > 1000) return;
        auto& p = pending[hand];
        if (!p.order || now < p.queued || now-p.queued > 100) p = {hand, amplitude, duration, now, ++order};
        else { p.amplitude=std::max(p.amplitude,amplitude); p.duration=std::max(p.duration,duration); p.queued=now; }
    }
    bool Pop(uint64_t now, Event& out) {
        for (auto& p : pending) if (p.order && (now < p.queued || now-p.queued > 100)) p = {};
        if (sent && now >= sentAt && now-sentAt < 8) return false;
        Event* p = nullptr;
        for (auto& candidate : pending) if (candidate.order && (!p || candidate.order < p->order)) p=&candidate;
        if (!p) return false;
        out=*p; *p={}; sent=true; sentAt=now; return true;
    }
};
}
