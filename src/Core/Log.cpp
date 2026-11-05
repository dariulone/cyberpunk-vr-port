// Log -- the plugin's log, and the guarded dumps that read engine memory.
//
// One log for every module in this DLL. Cheap on the hot path (a formatted line and a write) and never
// conditional on a debugger being present, because the interesting failures happen on other people's
// machines.
//
// THE DUMP HELPERS GUARD EVERY READ. LogVec4At, LogU32FloatAt, LogPtrPayloadVec4At and LogStackWindowAt
// all take an ADDRESS from somewhere in the engine and print what is there, which is exactly the
// operation that turns a wrong guess into a crash. IsReadableAddressRange is the check, and it is why a
// mistyped offset in a diagnostic produces a line saying so instead of taking the game down.

#include <windows.h>
#include "Core/Paths.hpp"
#include <psapi.h>
#include <xinput.h>
#include "Utils/SharedSlots.hpp"   // CyberpunkVR_Hands_Shared slot map (single source of truth)
#include <cstdint>
#include <cstddef>
#include <cstdio>
#include <cstdarg>
#include <cstring>
#include <cmath>
#include <share.h>
#include "Utils/AobScanner.hpp"
#include "Overlay/LiveControlsUi.hpp"
#include "Overlay/LauncherDialog.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Runtimes/RuntimeFovCorrection.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/ScriptGameInstance.hpp>
#include <iostream>
#include <MinHook.h>
#include "Hooks/SwapChain.hpp"
#include "Utils/LogThrottle.hpp"
#include "Hooks/Trampoline.hpp"
#include "Utils/MemorySafe.hpp"
#include "Core/Telemetry.hpp"
#include "Core/LiveControls.hpp"
#include "Core/VrCoreShared.hpp"
#include "Core/CoreInternal.hpp"
#include "Camera/CameraLink.hpp"
#include "Hooks/Hook.hpp"

void Log(const char* fmt, ...) {
    if (!g_logFile) {
        const std::wstring logPath = cvr::paths::File(cvr::paths::Folder::Log, L"cyberpunkvrport.log");
        g_logFile = _wfsopen(logPath.c_str(), L"w", _SH_DENYNO);
    }
    if (!g_logFile) return;

    va_list args;
    va_start(args, fmt);
    vfprintf(g_logFile, fmt, args);
    va_end(args);

    // FLUSH PER LINE IS THE POINT -- until it is the problem.
    //
    // Every crash in this project has been diagnosed from the last few lines of this file, so
    // the flush stays: without it a crash loses whatever the CRT still had buffered. But a
    // per-line flush is a syscall AND it serialises on the FILE lock, and this function is
    // called from engine job threads. One bad diagnostic ("% 600" on a counter that reaches
    // 196M) put 370 flushes per second inside the transform-update jobs -- measured, 418976
    // lines in one session -- and that alone is enough to make head tracking stutter.
    //
    // So: flush normally, and under a burst keep writing but let the CRT buffer absorb it. The
    // content survives either way; only the per-line durability is traded, and only while
    // something is already logging far too much to be durable about.
    static std::atomic<uint32_t> s_windowCount{0};
    static std::atomic<uint64_t> s_windowStart{0};
    const uint64_t now = GetTickCount64();
    uint64_t start = s_windowStart.load(std::memory_order_relaxed);
    if (now - start >= 1000) {
        const uint32_t burst = s_windowCount.exchange(0, std::memory_order_relaxed);
        s_windowStart.store(now, std::memory_order_relaxed);
        if (burst > 300) {
            fprintf(g_logFile, "Log: %u lines in the previous second -- flush coalesced. "
                               "Something is logging from a hot path.\n", burst);
        }
    }
    if (s_windowCount.fetch_add(1, std::memory_order_relaxed) < 300) {
        fflush(g_logFile);
    }
}

// The trampoline arena and its instruction emitters now live in Hooks/Trampoline.cpp -- ONE
// arena for the whole plugin, because a second one fragments the +/-2 GB window and the
// hooks that miss out fail silently. See Hooks/Trampoline.hpp.



bool ReadFloatArraySafe(const float* src, float* out, size_t count) {
    if (!src || !out) return false;
    for (size_t i = 0; i < count; ++i) {
        if (!ReadFloatSafe(reinterpret_cast<uintptr_t>(src + i), &out[i])) {
            return false;
        }
    }
    return true;
}

bool WriteFloatArraySafe(float* dst, const float* values, size_t count) {
    if (!dst || !values) return false;
    for (size_t i = 0; i < count; ++i) {
        if (!WriteFloatSafe(reinterpret_cast<uintptr_t>(dst + i), values[i])) {
            return false;
        }
    }
    return true;
}

bool LooksProjectionLike(const float* values, size_t count) {
    if (!values || count < 16) return false;

    const float m00 = values[0];
    const float m11 = values[5];
    const float m03 = values[3];
    const float m13 = values[7];
    const float m23 = values[11];
    const float m33 = values[15];

    if (!(m00 > 0.2f && m00 < 8.0f && m11 > 0.2f && m11 < 8.0f)) return false;
    if (fabsf(m03) > 0.1f || fabsf(m13) > 0.1f) return false;
    if (!(fabsf(m23) > 0.1f || fabsf(m33) < 0.1f || fabsf(m33 - 1.0f) < 0.1f)) return false;
    return true;
}

void LogMatrix4x4(const char* prefix, const float* values) {
    if (!prefix || !values) return;
    Log("%s\n", prefix);
    Log("  [%.6f %.6f %.6f %.6f]\n", values[0], values[1], values[2], values[3]);
    Log("  [%.6f %.6f %.6f %.6f]\n", values[4], values[5], values[6], values[7]);
    Log("  [%.6f %.6f %.6f %.6f]\n", values[8], values[9], values[10], values[11]);
    Log("  [%.6f %.6f %.6f %.6f]\n", values[12], values[13], values[14], values[15]);
}


// Same guard, 64-bit: used to read the camera component's CName at obj+0x40, where the object
// may be anything the engine happens to pass through the hook site.


bool WriteU32Safe(uintptr_t addr, uint32_t value) {
    __try {
        *reinterpret_cast<uint32_t*>(addr) = value;
        return true;
    }
    __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}


void InitGameModuleInfo() {
    if (g_gameModuleBase != 0 && g_gameModuleSize != 0) return;

    HMODULE gameModule = GetModuleHandleA("Cyberpunk2077.exe");
    if (!gameModule) return;

    MODULEINFO moduleInfo = {};
    if (!GetModuleInformation(GetCurrentProcess(), gameModule, &moduleInfo, sizeof(moduleInfo))) {
        return;
    }

    g_gameModuleBase = reinterpret_cast<uintptr_t>(moduleInfo.lpBaseOfDll);
    g_gameModuleSize = static_cast<size_t>(moduleInfo.SizeOfImage);
}

static bool IsInGameModule(uintptr_t addr) {
    if (g_gameModuleBase == 0 || g_gameModuleSize == 0) return false;
    return addr >= g_gameModuleBase && addr < (g_gameModuleBase + g_gameModuleSize);
}

static bool IsReadableAddressRange(uintptr_t addr, size_t size) {
    if (!addr || size == 0) return false;
    if (addr < 0x10000000000ULL) return false; // reject small/tagged values like 0x0000000100000000

    MEMORY_BASIC_INFORMATION mbi = {};
    if (VirtualQuery(reinterpret_cast<const void*>(addr), &mbi, sizeof(mbi)) != sizeof(mbi)) {
        return false;
    }
    if (mbi.State != MEM_COMMIT) return false;
    if ((mbi.Protect & PAGE_GUARD) != 0 || (mbi.Protect & PAGE_NOACCESS) != 0) return false;

    const DWORD readableMask = PAGE_READONLY | PAGE_READWRITE | PAGE_WRITECOPY |
        PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY;
    if ((mbi.Protect & readableMask) == 0) return false;

    uintptr_t regionEnd = reinterpret_cast<uintptr_t>(mbi.BaseAddress) + mbi.RegionSize;
    if (regionEnd < addr) return false;
    return addr + size <= regionEnd;
}

void LogVec4At(const char* label, uintptr_t addr) {
    if (!addr) {
        Log("  %s: null\n", label);
        return;
    }

    float v[4] = {};
    for (int i = 0; i < 4; ++i) {
        if (!ReadFloatSafe(addr + i * sizeof(float), &v[i])) {
            Log("  %s @%p unreadable\n", label, reinterpret_cast<void*>(addr));
            return;
        }
    }

    Log("  %s @%p = (%.6f, %.6f, %.6f, %.6f)\n",
        label, reinterpret_cast<void*>(addr), v[0], v[1], v[2], v[3]);
}

void LogFloatAt(const char* label, uintptr_t addr) {
    float value = 0.0f;
    if (!addr) {
        Log("  %s: null\n", label);
        return;
    }
    if (!ReadFloatSafe(addr, &value)) {
        Log("  %s @%p unreadable\n", label, reinterpret_cast<void*>(addr));
        return;
    }
    Log("  %s @%p = %.6f\n", label, reinterpret_cast<void*>(addr), value);
}

static void LogU32FloatAt(const char* label, uintptr_t addr) {
    uint32_t u32Value = 0;
    float f32Value = 0.0f;
    if (!addr) {
        Log("  %s: null\n", label);
        return;
    }
    if (!ReadU32Safe(addr, &u32Value) || !ReadFloatSafe(addr, &f32Value)) {
        Log("  %s @%p unreadable\n", label, reinterpret_cast<void*>(addr));
        return;
    }
    Log("  %s @%p = u32=%u (0x%08X) f32=%.6f\n",
        label,
        reinterpret_cast<void*>(addr),
        u32Value,
        u32Value,
        f32Value);
}

void LogU8At(const char* label, uintptr_t addr) {
    uint8_t value = 0;
    if (!addr) {
        Log("  %s: null\n", label);
        return;
    }
    if (!ReadU8Safe(addr, &value)) {
        Log("  %s @%p unreadable\n", label, reinterpret_cast<void*>(addr));
        return;
    }
    Log("  %s @%p = 0x%02X (%u)\n", label, reinterpret_cast<void*>(addr), value, value);
}

void LogPtrAt(const char* label, uintptr_t addr) {
    uintptr_t value = 0;
    if (!addr) {
        Log("  %s: null\n", label);
        return;
    }
    if (!ReadPtrSafe(addr, &value)) {
        Log("  %s @%p unreadable\n", label, reinterpret_cast<void*>(addr));
        return;
    }
    Log("  %s @%p = %p\n", label, reinterpret_cast<void*>(addr), reinterpret_cast<void*>(value));
}

void LogPtrPayloadVec4At(const char* label, uintptr_t addr) {
    uintptr_t value = 0;
    if (!addr) {
        Log("  %s: null\n", label);
        return;
    }
    if (!ReadPtrSafe(addr, &value)) {
        Log("  %s @%p unreadable\n", label, reinterpret_cast<void*>(addr));
        return;
    }
    Log("  %s @%p = %p\n", label, reinterpret_cast<void*>(addr), reinterpret_cast<void*>(value));
    if (value != 0 && IsReadableAddressRange(value, sizeof(float) * 4)) {
        char payloadLabel[96];
        sprintf_s(payloadLabel, "%s payload", label);
        LogVec4At(payloadLabel, value);
    } else if (value != 0) {
        Log("  %s payload @%p skipped (not a plausible readable pointer)\n",
            label, reinterpret_cast<void*>(value));
    }
}

void LogStackWindowAt(const char* label, uintptr_t rsp, int slots) {
    if (!rsp) {
        Log("  %s: null\n", label);
        return;
    }

    Log("  %s @%p\n", label, reinterpret_cast<void*>(rsp));
    for (int i = 0; i < slots; ++i) {
        uintptr_t entryAddr = rsp + static_cast<uintptr_t>(i) * sizeof(uintptr_t);
        uintptr_t value = 0;
        if (!ReadPtrSafe(entryAddr, &value)) {
            Log("    [%02d] @%p unreadable\n", i, reinterpret_cast<void*>(entryAddr));
            return;
        }

        if (IsInGameModule(value)) {
            Log("    [%02d] @%p = %p (game+rva 0x%llX)\n",
                i,
                reinterpret_cast<void*>(entryAddr),
                reinterpret_cast<void*>(value),
                static_cast<unsigned long long>(value - g_gameModuleBase));
        } else {
            Log("    [%02d] @%p = %p\n",
                i,
                reinterpret_cast<void*>(entryAddr),
                reinterpret_cast<void*>(value));
        }
    }
}

// One line, for code that cannot name Log: the Lua host (src/Lua), where CET's logging namespace is called Log.
extern "C" void CvrLogText(const char* text) { Log("%s", text); }
