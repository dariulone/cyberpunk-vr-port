#pragma once
#include <atomic>

namespace cvr::haptics {
// A saved preference, not hot actuator ownership. The external Toolkit process
// has independent EDS/audio writers; a game restart and bridge start/stop are
// required when changing routes. Detection is a startup snapshot, not a readiness
// handshake: the released bridge only reads our mapping and has no response channel.
class Routing {
    std::atomic<int> requested{2};
    std::atomic<int> activeRequest{2};
    std::atomic<bool> toolkit{false};
    std::atomic<bool> detected{false};
public:
    int Requested() const { return requested.load(std::memory_order_relaxed); }
    void Request(int mode) { requested.store(mode >= 0 && mode <= 2 ? mode : 0, std::memory_order_relaxed); }
    void BeginSession(bool psvr2, bool bridgeDetected) {
        const int mode = Requested();
        activeRequest.store(mode, std::memory_order_relaxed);
        detected.store(bridgeDetected, std::memory_order_relaxed);
        toolkit.store(psvr2 && mode != 0 && bridgeDetected, std::memory_order_relaxed);
    }
    bool Toolkit() const { return toolkit.load(std::memory_order_relaxed); }
    bool BridgeDetected() const { return detected.load(std::memory_order_relaxed); }
    bool Pending(bool psvr2) const { return psvr2 && Requested() != activeRequest.load(std::memory_order_relaxed); }
};
inline Routing& HapticRoute() { static Routing route; return route; }
// Checks a same-Windows-session _017 bridge owning a loopback UDP listener.
// A driver install, mapping created by the game, or another DSX client is not evidence.
bool DetectToolkitBridge();
}
