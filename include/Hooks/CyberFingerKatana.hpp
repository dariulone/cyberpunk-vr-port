#pragma once
#include <atomic>
#include <cstdint>
#include "Runtimes/CyberFingerMenuPolicy.hpp"

namespace cvr::cyberfinger {
// Lua identifies the exact item in each stock/offhand slot or at a visible holster.
// The animation pass supplies proximity to the equipped blade's lower hilt.
class KatanaGrip {
    using Packet = uint64_t;
    static Packet stamp(uint32_t token, uint32_t now) { return (Packet(now) << 32) | token; }
    static uint32_t fresh(Packet p, uint32_t now) {
        return now - uint32_t(p >> 32) <= 150 ? uint32_t(p) : 0;
    }
    std::atomic<Packet> equipment{}, support{}, nearHilt{}, equipmentTap{}, supportTap{};
    ButtonEdge edge; // Only the input thread advances these fields.
    uint32_t lastInput{};
    bool claimed{}, maskGesture{};
    uint32_t latched{}; // Only the animation pass changes the latch.
public:
    bool Offer(uint32_t item, uint32_t otherHandBlade, uint32_t now) {
        equipment.store(stamp(item,now));
        support.store(stamp(otherHandBlade,now));
        const auto tap = fresh(equipmentTap.exchange(0), now);
        return item && tap == item;
    }
    bool BlocksGesture() const { return maskGesture; }
    struct Claim { bool button, gesture; };
    Claim Input(bool allowed, bool down, bool curled, uint32_t now) {
        if (!allowed || now-lastInput>150) edge.Reset();
        lastInput=now;
        if (!down) claimed=false;
        if (!down && !curled) maskGesture=false;
        if (!allowed) { equipmentTap.store(0); supportTap.store(0); }
        else if (edge.Step(down)) {
            const auto blade=fresh(support.load(),now);
            const auto item=fresh(equipment.load(),now);
            if (blade && fresh(nearHilt.load(),now)==blade) {
                supportTap.store(stamp(blade,now));
                claimed=true;
            } else if (item) {
                equipmentTap.store(stamp(item,now));
                claimed=true;
            }
            if (claimed) maskGesture=true;
        }
        return {claimed,maskGesture};
    }
    enum class Support { Gesture, Hold, Release };
    Support Pose(bool inRange, bool active, uint32_t now) {
        const auto blade=fresh(support.load(),now);
        const bool held=active && latched && latched==blade;
        nearHilt.store(stamp((inRange || active) ? blade : 0,now));
        const auto tap=fresh(supportTap.exchange(0),now);
        if (blade && tap==blade && (inRange || active)) {
            latched=active ? 0 : blade;
            return active ? Support::Release : Support::Hold;
        }
        const bool lost=active && latched && !held;
        if (!held) latched=0;
        return held ? Support::Hold : lost ? Support::Release : Support::Gesture;
    }
};
inline KatanaGrip katanaGrips[2];
}
