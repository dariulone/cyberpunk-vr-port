#pragma once
#include <windows.h>
#include <cmath>
#include <mutex>
#include "Runtimes/HapticPulseQueue.hpp"

namespace cvr::psvr2 {
// Same payload and heartbeat contract as the user's Toolkit slot-guard fork,
// in a DIFFERENT named mapping. Never alias upstream's live input slots.
inline constexpr char HapticMapping[] = "Local\\CyberpunkVR_PSVR2_Haptics_017_v1";
class HapticChannel {
    HANDLE handle{};
    float* data{};
    std::mutex mutex;
    HapticPulseQueue pulses;
public:
    HapticChannel() {
        handle = CreateFileMappingA(INVALID_HANDLE_VALUE, nullptr, PAGE_READWRITE, 0, 1024, HapticMapping);
        if (handle) data = static_cast<float*>(MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, 1024));
        if (data) { ZeroMemory(data, 1024); data[168] = 18512.0f; data[169] = 1.0f; }
    }
    ~HapticChannel() { if (data) UnmapViewOfFile(data); if (handle) CloseHandle(handle); }
    void Heartbeat() {
        std::lock_guard lock(mutex);
        if (!data) return;
        data[170] = data[170] >= 1000000.0f ? 1.0f : data[170] + 1.0f;
        HapticPulseQueue::Event pulse;
        if (pulses.Pop(GetTickCount64(), pulse)) {
            data[158] = static_cast<float>(pulse.hand); data[159] = pulse.amplitude;
            data[160] = static_cast<float>(pulse.duration);
            MemoryBarrier();
            data[157] = data[157] >= 1000000.0f ? 1.0f : data[157] + 1.0f;
        }
    }
    void Pulse(int hand, float amplitude, int duration) {
        std::lock_guard lock(mutex);
        if (data) pulses.Push(hand, amplitude, duration, GetTickCount64());
    }
};
inline HapticChannel& Haptics() { static HapticChannel channel; return channel; }
}
