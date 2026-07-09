#include <windows.h>
#include <psapi.h>
#include <cstdint>
#include <vector>

void* FindPattern(const char* moduleName, const char* pattern, const char* mask) {
    HMODULE hModule = GetModuleHandleA(moduleName);
    if (!hModule) return nullptr;

    MODULEINFO moduleInfo;
    if (!GetModuleInformation(GetCurrentProcess(), hModule, &moduleInfo, sizeof(moduleInfo))) {
        return nullptr;
    }

    uint8_t* base = reinterpret_cast<uint8_t*>(moduleInfo.lpBaseOfDll);
    DWORD size = moduleInfo.SizeOfImage;
    size_t patternLen = strlen(mask);

    uint8_t* current = base;
    uint8_t* end = base + size - patternLen;

    while (current < end) {
        MEMORY_BASIC_INFORMATION mbi;
        if (!VirtualQuery(current, &mbi, sizeof(mbi))) break;

        if (mbi.State == MEM_COMMIT && (mbi.Protect & (PAGE_READONLY | PAGE_READWRITE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE))) {
            uint8_t* regionEnd = static_cast<uint8_t*>(mbi.BaseAddress) + mbi.RegionSize;
            if (regionEnd > end) regionEnd = end;

            for (uint8_t* p = current; p < regionEnd; ++p) {
                bool found = true;
                for (size_t j = 0; j < patternLen; ++j) {
                    if (mask[j] == 'x' && p[j] != static_cast<uint8_t>(pattern[j])) {
                        found = false;
                        break;
                    }
                }
                if (found) {
                    return p;
                }
            }
        }
        current = static_cast<uint8_t*>(mbi.BaseAddress) + mbi.RegionSize;
    }
    return nullptr;
}
