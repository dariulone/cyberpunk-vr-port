#pragma once
class OpenXRManager {
public:
    static OpenXRManager& Get() { static OpenXRManager manager; return manager; }
    bool IsSessionRunning() const { return true; }
    int ApplyHaptic(int hand, float amplitude, float durationSec);
};
