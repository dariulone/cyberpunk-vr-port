#pragma once
// THE FINGER FILTERS for hand-tracked curls (Runtimes/FingerGrip.cpp). Pure arithmetic on one finger's curl.
#include <windows.h>
#include <cmath>

namespace cvr::fingergrip {

using LogFn = void (*)(const char*, ...);

// SPIKE FILTER. A glove can report one finger far from where it is for a moment (often exactly 0 between normal
// values), and V's finger would snap there. Gloves update about 30 times a second, so a real fast flick moves a finger
// at most ~0.35 per update. A jump bigger than kJump from the shown value is held back: the finger keeps its
// place until the new reading has stayed (within kAgree of itself) for kConfirmMs, then it is taken. A spike that
// comes back sooner is dropped. Per finger it counts what it dropped and took, for the log.
constexpr float kJump = 0.40f, kAgree = 0.15f;
constexpr ULONGLONG kConfirmMs = 60;
// ZERO DROPOUTS. In a fist a glove can report a squeezed finger as exactly 0 for 0.1 s up to over 10 s, probably a
// sensor read past "fully bent". A really open finger does not read exactly 0 (about 0.07-0.17), so an exact 0 after a
// bent finger is no reading: the finger keeps its place, for at most kZeroHoldMs.
constexpr float kZero = 0.005f, kBent = 0.30f;
constexpr ULONGLONG kZeroHoldMs = 15000;

struct Finger {
    float shown = -1, candidate = 0;
    ULONGLONG since = 0;   // 0 = nothing held back
    float from = 0;        // value before the jump
    int dropped = 0, taken = 0;
    ULONGLONG longestDropped = 0;
    float low = 1, high = 0;   // range of the shown value, for calibration
    ULONGLONG zeroSince = 0;   // 0 = not in a zero dropout
    int zeros = 0;
    ULONGLONG longestZero = 0;
};

// One filter per hand: five fingers (thumb..little), with the counts the log reports.
class FingerFilter {
public:
    static constexpr const char* kName[5] = {"thumb", "index", "middle", "ring", "little"};
    explicit FingerFilter(LogFn log = nullptr) : log_(log) {}
    void SetLog(LogFn log) { log_ = log; }
    float Filter(Finger& f, float raw, ULONGLONG now, int hand, int finger) {
        if (f.shown < 0) f.shown = raw;
        if (raw < kZero && (f.zeroSince || f.shown > kBent)) {
            if (!f.zeroSince) f.zeroSince = now;
            if (now - f.zeroSince < kZeroHoldMs) return f.shown;
        } else if (f.zeroSince) {
            const ULONGLONG held = now - f.zeroSince;
            ++f.zeros;
            if (held > f.longestZero) f.longestZero = held;
            if (log_ && eventLines_++ < 300)
                log_("%s %s: zero dropout held %llu ms at %.2f, back at %.2f", hand ? "right" : "left", kName[finger], held, f.shown, raw);
            f.zeroSince = 0;
        }
        if (std::fabs(raw - f.shown) <= kJump) {
            if (f.since) {
                const ULONGLONG held = now - f.since;
                ++f.dropped;
                if (held > f.longestDropped) f.longestDropped = held;
                if (log_ && eventLines_++ < 300)
                    log_("%s %s: spike %.2f -> %.2f dropped after %llu ms", hand ? "right" : "left", kName[finger], f.from, f.candidate, held);
                f.since = 0;
            }
            f.shown = raw;
        } else if (!f.since) {
            f.since = now; f.from = f.shown; f.candidate = raw;
        } else if (std::fabs(raw - f.candidate) > kAgree) {
            f.candidate = raw; f.since = now;   // still moving: the new reading has to stay put from now
        } else if (now - f.since >= kConfirmMs) {
            ++f.taken;
            if (log_ && eventLines_++ < 300)
                log_("%s %s: jump %.2f -> %.2f taken after %llu ms", hand ? "right" : "left", kName[finger], f.from, raw, now - f.since);
            f.shown = raw; f.since = 0;
        }
        if (f.shown < f.low) f.low = f.shown;
        if (f.shown > f.high) f.high = f.shown;
        return f.shown;
    }
    Finger fingers_[2][5];
private:
    LogFn log_{};
    int eventLines_ = 0;
};
}
