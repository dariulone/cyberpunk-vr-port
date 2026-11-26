// OpenXRManager::Get() for the bench: storage for an object nobody constructs (its constructor lives in the plugin's
// OpenXRManager.cpp), zeroed, with only m_sharedHandsPtr set -- to testXrShared -- so the class's inline GetSharedSlot and
// SetSharedSlot read and write a buffer the traces can see. The private member is reached the only way a test can.
#define _ALLOW_KEYWORD_MACROS 1
#include <windows.h>
#include <atomic>
#include <mutex>
#include <memory>
#include <string>
#include <vector>
#include <array>
#include <functional>
#include <thread>
#include <condition_variable>
#include <deque>
#include <optional>
#include <shared_mutex>
#define private public
#define protected public
#include "Runtimes/OpenXRManager.hpp"
#undef private
#undef protected
#include <cstddef>
#include <cstring>

extern float testXrShared[256];
OpenXRManager& OpenXRManager::Get() {
    alignas(64) static unsigned char storage[sizeof(OpenXRManager)]{};
    static bool once = [] {
        float* p = testXrShared;
        std::memcpy(storage + offsetof(OpenXRManager, m_sharedHandsPtr), &p, sizeof(p));
        return true;
    }();
    (void)once;
    return *reinterpret_cast<OpenXRManager*>(storage);
}