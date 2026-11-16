// FrameOrderProbe -- see include/Natives/FrameOrderProbe.hpp.
#include "Natives/FrameOrderProbe.hpp"
#include "Natives/NativeFunctions.hpp"
#include "Natives/PhysXStep.hpp"
#include <RED4ext/Scripting/Utils.hpp>
#include <windows.h>
#include <algorithm>
#include <atomic>
#include <cstdint>
#include <cstdio>
#include <cstring>

extern void Log(const char* fmt, ...);

namespace cvr::frameorder {
namespace {
constexpr int kMostEvents = 1 << 16, kMostFrames = 120;

struct Event { int64_t qpc; uint32_t thread; char what; };
Event events[kMostEvents];
std::atomic<int> eventCount{0};
std::atomic<int> framesLeft{0};

void Dump() {
    // Late notes from other threads may still land; a short wait lets them, and they are past the last submit anyway.
    Sleep(1);
    const int n = std::min(eventCount.load(), kMostEvents);
    std::sort(events, events + n, [](const Event& a, const Event& b) { return a.qpc < b.qpc; });
    LARGE_INTEGER f;
    QueryPerformanceFrequency(&f);
    const double toMs = 1000.0 / double(f.QuadPart);
    uint32_t threads[32]{};
    int nThreads = 0;
    uint32_t game = 0;
    for (int i = 0; i < n; ++i)
        if (events[i].what == 'U') { game = events[i].thread; break; }
    const auto label = [&](uint32_t t) -> char {
        if (t == game) return 'G';
        for (int k = 0; k < nThreads; ++k)
            if (threads[k] == t) return char('a' + k);
        if (nThreads < 26) { threads[nThreads] = t; return char('a' + nThreads++); }
        return '?';
    };
    struct Seen { double first, last; int count; char thread; };
    int64_t start = 0;
    int frame = 0;
    Seen seen[128]{};
    for (int i = 0; i < n; ++i) {
        const Event& e = events[i];
        if (!start) {
            if (e.what == 'U') start = e.qpc;
            continue;
        }
        const double t = double(e.qpc - start) * toMs;
        Seen& s = seen[uint8_t(e.what) & 127];
        if (!s.count) { s.first = t; s.thread = label(e.thread); }
        s.last = t;
        ++s.count;
        if (e.what != 'U') continue;
        char line[1024];
        int len = std::snprintf(line, sizeof(line), "[frameorder] frame %d, %.2f ms:", ++frame, t);
        char order[128];
        int m = 0;
        for (int c = 0; c < 128; ++c)
            if (seen[c].count) order[m++] = char(c);
        std::sort(order, order + m, [&](char a, char b) { return seen[uint8_t(a)].first < seen[uint8_t(b)].first; });
        for (int k = 0; k < m && len < int(sizeof(line)) - 48; ++k) {
            const Seen& s = seen[uint8_t(order[k])];
            len += s.count > 1
                ? std::snprintf(line + len, sizeof(line) - len, " %c %.2f..%.2f x%d (%c)", order[k], s.first, s.last, s.count, s.thread)
                : std::snprintf(line + len, sizeof(line) - len, " %c %.2f (%c)", order[k], s.first, s.thread);
        }
        Log("%s\n", line);
        std::memset(seen, 0, sizeof(seen));
        start = e.qpc;
    }
    char names[512];
    int len = std::snprintf(names, sizeof(names), "[frameorder] %d events; threads: G the submit's (%lu)", n, game);
    for (int k = 0; k < nThreads && len < int(sizeof(names)) - 24; ++k)
        len += std::snprintf(names + len, sizeof(names) - len, ", %c %lu", char('a' + k), threads[k]);
    Log("%s\n", names);
}
}  // namespace

void Note(char what) {
    if (framesLeft.load(std::memory_order_relaxed) <= 0) return;
    const int i = eventCount.fetch_add(1, std::memory_order_relaxed);
    if (i >= kMostEvents) return;
    LARGE_INTEGER q;
    QueryPerformanceCounter(&q);
    events[i] = Event{q.QuadPart, GetCurrentThreadId(), what};
}

void NoteSubmit() {
    if (framesLeft.load(std::memory_order_relaxed) <= 0) return;
    Note('U');
    if (framesLeft.fetch_sub(1) == 1) Dump();
}

bool Start(int frames) {
    if (!cvr::physxstep::Ensure()) return false;
    eventCount.store(0);
    // one more than asked: the first submit only opens the first frame
    framesLeft.store(std::clamp(frames, 1, kMostFrames) + 1);
    return true;
}
}  // namespace cvr::frameorder

// VRFrameOrderProbe(frames) -> Bool: records that many frames' order of events into the log.
void VRFrameOrderProbe(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    int32_t frames = 0;
    RED4ext::GetParameter(stack, &frames);
    stack->code++;
    const bool ok = cvr::frameorder::Start(frames);
    if (out) *out = ok;
}
