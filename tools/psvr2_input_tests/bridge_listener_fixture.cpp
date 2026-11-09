// Isolated socket-only fixture: never loads Toolkit or drives controllers.
#include <winsock2.h>
#include <windows.h>
#include <string>
int wmain(int argc, wchar_t** argv) {
    if (argc != 2) return 1;
    const std::wstring readyName = argv[1], stopName = readyName + L"-stop";
    HANDLE ready = OpenEventW(EVENT_MODIFY_STATE, FALSE, readyName.c_str());
    HANDLE stop = OpenEventW(SYNCHRONIZE, FALSE, stopName.c_str());
    if (!ready || !stop) return 2;
    WSADATA data{}; if (WSAStartup(MAKEWORD(2,2), &data)) return 3;
    SOCKET socket = ::socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
    sockaddr_in address{}; address.sin_family = AF_INET; address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (socket == INVALID_SOCKET || bind(socket, reinterpret_cast<sockaddr*>(&address), sizeof(address))) return 4;
    SetEvent(ready);
    WaitForSingleObject(stop, 15000);
    closesocket(socket); WSACleanup(); CloseHandle(stop); CloseHandle(ready);
    return 0;
}
