#include "Runtimes/HapticRouting.hpp"
#include <winsock2.h>
#include <windows.h>
#include <iphlpapi.h>
#include <vector>
#include <cwchar>

namespace cvr::haptics {
bool DetectToolkitBridge() {
    ULONG bytes = 0;
    if (GetExtendedUdpTable(nullptr, &bytes, FALSE, AF_INET, UDP_TABLE_OWNER_PID, 0) != ERROR_INSUFFICIENT_BUFFER)
        return false;
    // Races while processes bind/unbind are bounded and retried, never on the frame path.
    for (int attempt = 0; attempt < 3 && bytes > 0 && bytes < 4 * 1024 * 1024; ++attempt) {
        std::vector<unsigned char> storage(bytes);
        const DWORD result = GetExtendedUdpTable(storage.data(), &bytes, FALSE, AF_INET, UDP_TABLE_OWNER_PID, 0);
        if (result == ERROR_INSUFFICIENT_BUFFER) continue;
        if (result != NO_ERROR) return false;
        const auto* table = reinterpret_cast<const MIB_UDPTABLE_OWNER_PID*>(storage.data());
        DWORD ownSession = 0;
        if (!ProcessIdToSessionId(GetCurrentProcessId(), &ownSession)) return false;
        for (DWORD i = 0; i < table->dwNumEntries; ++i) {
            const auto& row = table->table[i];
            if (row.dwLocalAddr != htonl(INADDR_LOOPBACK) || row.dwLocalPort == 0) continue;
            DWORD session = 0;
            if (!ProcessIdToSessionId(row.dwOwningPid, &session) || session != ownSession) continue;
            HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, FALSE, row.dwOwningPid);
            if (!process) continue;
            wchar_t path[32768]{}; DWORD count = static_cast<DWORD>(std::size(path));
            bool matches = false;
            if (WaitForSingleObject(process, 0) == WAIT_TIMEOUT && QueryFullProcessImageNameW(process, 0, path, &count)) {
                const wchar_t* name = wcsrchr(path, L'\\');
                matches = _wcsicmp(name ? name + 1 : path, L"psvr2_toolkit_dsx_bridge_017.exe") == 0;
            }
            CloseHandle(process);
            if (matches) return true;
        }
        return false;
    }
    return false;
}
}
