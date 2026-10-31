#include "Runtimes/HapticRouting.hpp"
#include <windows.h>
#include <string>
#include <iostream>
int wmain(int argc, wchar_t** argv) {
    using cvr::haptics::DetectToolkitBridge;
    if (argc != 2) return 1;
    if (DetectToolkitBridge()) { std::cerr << "Close real bridge before running detection fixture.\n"; return 1; }
    const std::wstring name = L"Local\\CPVR-HapticDetectionTest-" + std::to_wstring(GetCurrentProcessId());
    HANDLE ready = CreateEventW(nullptr, TRUE, FALSE, name.c_str());
    HANDLE stop = CreateEventW(nullptr, TRUE, FALSE, (name+L"-stop").c_str());
    std::wstring command = L"\"" + std::wstring(argv[1]) + L"\" \"" + name + L"\"";
    STARTUPINFOW startup{}; startup.cb = sizeof(startup); PROCESS_INFORMATION child{};
    if (!CreateProcessW(nullptr, command.data(), nullptr, nullptr, FALSE, CREATE_NO_WINDOW, nullptr, nullptr, &startup, &child)) return 1;
    const bool childReady = WaitForSingleObject(ready, 5000) == WAIT_OBJECT_0;
    const bool detected = childReady && DetectToolkitBridge();
    SetEvent(stop);
    const bool exited = WaitForSingleObject(child.hProcess, 5000) == WAIT_OBJECT_0;
    const bool cleared = exited && !DetectToolkitBridge();
    CloseHandle(child.hThread); CloseHandle(child.hProcess); CloseHandle(ready); CloseHandle(stop);
    if (!detected || !cleared) { std::cerr << "Bridge detection fixture failed\n"; return 1; }
    std::cout << "Bridge absent / live loopback listener / exited listener checks passed\n";
    return 0;
}
