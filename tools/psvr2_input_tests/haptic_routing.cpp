#include "Runtimes/HapticRouting.hpp"
#include "Runtimes/LegacyHapticPolicy.hpp"
#include "Runtimes/HapticPulseQueue.hpp"
#include <cstdlib>
#include <iostream>
#include <limits>

static int checks = 0;
static void check(bool result, const char* message) {
    ++checks;
    if (!result) { std::cerr << "FAIL: " << message << '\n'; std::exit(1); }
}
int main() {
    using cvr::haptics::Routing;
    Routing r;
    check(r.Requested() == 2 && !r.Toolkit(), "new installs use auto with native fallback");
    r.BeginSession(true, false);
    check(!r.Toolkit() && !r.Pending(true), "PSVR2 native route is not suppressed");
    r.Request(1);
    check(r.Requested() == 1 && !r.Toolkit() && r.Pending(true), "request cannot take actuators mid-session");
    r.BeginSession(true, true);
    check(r.Toolkit() && !r.Pending(true), "PSVR2 explicit opt-in takes exclusive Toolkit route");
    r.Request(0);
    check(r.Toolkit() && r.Pending(true), "Toolkit remains owner until restart");
    r.BeginSession(true, true);
    check(!r.Toolkit() && !r.Pending(true), "restart restores native shots and output");
    r.Request(1);
    r.BeginSession(false, true);
    check(!r.Toolkit() && !r.Pending(false), "Quest and all non-PSVR2 HMDs ignore saved Toolkit preference");
    check(r.Requested() == 1, "switching headset does not erase PSVR2 preference");
    r.BeginSession(true, true);
    check(r.Toolkit(), "returning to PSVR2 applies saved preference");
    r.BeginSession(true, false);
    check(!r.Toolkit() && !r.Pending(true), "missing bridge falls back even for saved Toolkit selection");
    r.Request(2); r.BeginSession(true, true);
    check(r.Toolkit(), "auto selects a detected compatible bridge on PSVR2");
    r.BeginSession(false, true);
    check(!r.Toolkit(), "auto never redirects Quest to a running PSVR2 bridge");
    r.Request(0); r.BeginSession(true, true);
    check(!r.Toolkit(), "explicit OpenXR preference is retained");
    for (int invalid : {-1, 3, 99, std::numeric_limits<int>::max()}) {
        r.Request(invalid);
        check(r.Requested() == 0, "invalid preference fails to native");
        r.BeginSession(true, true);
        check(!r.Toolkit(), "invalid preference cannot claim Toolkit output");
    }
    using namespace cvr::haptics;
    PulseRequest p;
    p.Merge(.45f, 45, 100);
    p.Merge(1.0f, 90, 100);
    p.Merge(.25f, 20, 101);
    check(p.amplitude == 1 && p.durationMs == 90, "strongest/longest swing wins within a frame");
    check(p.Fresh(201) && !p.Fresh(202), "pulse expires instead of replaying after stall");
    p.Merge(.2f, 10, 202);
    check(p.amplitude == .2f && p.durationMs == 10, "stale pulse not merged into new event");
    p.Merge(std::numeric_limits<float>::quiet_NaN(), 100, 203);
    p.Merge(2, 100, 203); p.Merge(1, 1001, 203); p.Merge(1, -1, 203);
    check(p.amplitude == .2f && p.durationMs == 10, "invalid requests ignored");
    auto shot = LegacyShot(1);
    check(std::abs(shot.amplitude-.7f) < .001f && shot.durationMs == 43, "old recoil formula restored");
    check(LegacyShot(9).amplitude > shot.amplitude && LegacyShot(9).durationMs > shot.durationMs, "heavy weapon ladder retained");
    check(LegacyShot(std::numeric_limits<float>::quiet_NaN()).durationMs == 43, "non-finite kick guarded");
    check(SafeGain(-1) == 0 && SafeGain(5) == 2, "gain bounded");
    check(VehicleFresh(true, 200, 100) && !VehicleFresh(true, 201, 100), "vehicle envelope expires after 100 ms");
    check(!VehicleFresh(false, 105, 100) && !VehicleFresh(true, 100, 105), "no vehicle on foot or from future stamp");
    cvr::psvr2::HapticPulseQueue q;
    cvr::psvr2::HapticPulseQueue::Event event;
    q.Push(0,.5f,40,100); q.Push(1,.8f,70,100); q.Push(0,.9f,90,101);
    check(q.Pop(101,event) && event.hand==0 && event.amplitude==.9f && event.duration==90,"same-hand pulses merge without losing other hand");
    check(!q.Pop(108,event),"mailbox cannot be overwritten before bridge polling interval");
    check(q.Pop(109,event) && event.hand==1,"other hand delivered after eight ms");
    q.Push(0,1,90,200);check(!q.Pop(301,event),"bridge pulse expires rather than replaying after stall");
    q.Push(2,1,90,400);q.Push(0,std::numeric_limits<float>::quiet_NaN(),90,400);
    check(!q.Pop(400,event),"bridge queue rejects invalid hand and amplitude");
    std::cout << checks << " haptic routing checks passed\n";
}
