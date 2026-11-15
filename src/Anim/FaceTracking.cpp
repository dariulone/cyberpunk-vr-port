#include <winsock2.h>
#include <ws2tcpip.h>
#include "Anim/FaceTracking.hpp"
#include "Core/Paths.hpp"
#include <algorithm>
#include <atomic>
#include <cstdio>
#include <mutex>
#include <string>
#pragma comment(lib,"ws2_32.lib")

// Versioned, read-only diagnostics: counters and freshness, never face recordings.
// [0] sequence, [1] version, [2] enabled/listening/hooks/bound flags, [3] port,
// [4] socket error, [5] fresh channels, [6] mapped controls, [7] packets,
// [8] malformed, [9] owner passes, [10] facial passes, [11] writes,
// [12] faults, [13] rejected layouts, [14] sample tick, [15] save error.
extern "C" { __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_FaceTracking[16]{}; }

namespace cvr::face {
namespace {
std::mutex mutex;
Settings settings;
Input input;
Status status;
bool loaded{},networkStarted{};
SOCKET socketHandle=INVALID_SOCKET;
int activePort{};
uint64_t retryAt{};
std::string Path(){return paths::FileA(paths::Folder::Config,"face_tracking.ini");}
void Load() {
    if(loaded)return;
    loaded=true;
    FILE* file=nullptr;
    if(fopen_s(&file,Path().c_str(),"r") || !file)return;
    char key[48],line[128];int value;
    while(fgets(line,sizeof(line),file))if(sscanf_s(line,"%47[^=]=%d",key,unsigned(sizeof(key)),&value)==2) {
        const std::string name=key;
        if(name=="enabled")settings.enabled=value==1;
        else if(name=="mouth")settings.mouth=value==1;
        else if(name=="blink")settings.blink=value==1;
        else if(name=="eyes")settings.eyes=value==1;
        else if(name=="brows")settings.brows=value==1;
        else if(name=="port" && value>=1024 && value<=65535)settings.port=value;
    }
    fclose(file);
}
void Close() {
    if(socketHandle!=INVALID_SOCKET){closesocket(socketHandle);socketHandle=INVALID_SOCKET;}
    if(networkStarted){WSACleanup();networkStarted=false;}
    status.listening=false;input={};activePort=0;
}
void Publish() {
    const auto config=GetSettings();const auto s=GetStatus();
    const uint64_t flags=(config.enabled?1ull:0ull)|(s.listening?2ull:0ull)|(s.hooks?4ull:0ull)|(s.bound?8ull:0ull);
    const uint64_t values[]{1,flags,uint64_t(config.port),uint64_t(s.socketError),s.fresh,s.mapped,
        s.packets,s.malformed,s.ownerPasses,s.facialPasses,s.writes,s.faults,s.rejected,GetTickCount64(),uint64_t(s.saveError)};
    CyberpunkVR_FaceTracking[0].fetch_add(1);
    for(unsigned i=0;i<std::size(values);++i)CyberpunkVR_FaceTracking[i+1].store(values[i],std::memory_order_relaxed);
    CyberpunkVR_FaceTracking[0].fetch_add(1);
}
}
Settings GetSettings(){std::lock_guard lock(mutex);Load();return settings;}
void SetSettings(Settings value) {
    value.port=std::clamp(value.port,1024,65535);
    std::lock_guard lock(mutex);Load();settings=value;retryAt=0;status.saveError=0;
    FILE* file=nullptr;
    const auto error=fopen_s(&file,Path().c_str(),"w");
    if(!error && file) {
        const auto written=std::fprintf(file,"; Quest Pro / Steam Link facial tracking in interactive mirrors.\n"
            "enabled=%d\nport=%d\nmouth=%d\nblink=%d\neyes=%d\nbrows=%d\n",
            int(value.enabled),value.port,int(value.mouth),int(value.blink),int(value.eyes),int(value.brows));
        const auto closed=fclose(file);
        if(written<0 || closed)status.saveError=EIO;
    } else status.saveError=error?error:EIO;
}
Input GetInput(){std::lock_guard lock(mutex);return settings.enabled?input:Input{};}
Status GetStatus() {
    auto result=RigStatus();
    std::lock_guard lock(mutex);
    result.listening=status.listening;result.socketError=status.socketError;result.saveError=status.saveError;
    result.packets=status.packets;result.malformed=status.malformed;
    const auto now=GetTickCount64();
    for(unsigned i=0;i<ChannelCount;++i)if(input.Fresh(Channel(i),now))++result.fresh;
    return result;
}
void Tick() {
    // Publish after the socket mutex is released, including early exits.
    struct Report { ~Report(){Publish();} } report;
    const auto config=GetSettings();
    if(config.enabled)InstallHooks();
    else ClearMirror();
    std::lock_guard lock(mutex);
    const auto now=GetTickCount64();
    if(!settings.enabled || (activePort && activePort!=settings.port))Close();
    if(!settings.enabled){status.socketError=0;return;}
    if(socketHandle==INVALID_SOCKET && now>=retryAt) {
        retryAt=now+2000;
        WSADATA data{};
        const int startup=WSAStartup(MAKEWORD(2,2),&data);
        if(startup){status.socketError=startup;return;}
        networkStarted=true;
        socketHandle=socket(AF_INET,SOCK_DGRAM,IPPROTO_UDP);
        int exclusive=1;
        sockaddr_in address{};address.sin_family=AF_INET;
        address.sin_addr.s_addr=htonl(INADDR_LOOPBACK);address.sin_port=htons(u_short(settings.port));
        u_long nonblocking=1;
        if(socketHandle==INVALID_SOCKET ||
           setsockopt(socketHandle,SOL_SOCKET,SO_EXCLUSIVEADDRUSE,reinterpret_cast<const char*>(&exclusive),sizeof(exclusive))==SOCKET_ERROR ||
           bind(socketHandle,reinterpret_cast<const sockaddr*>(&address),sizeof(address))==SOCKET_ERROR ||
           ioctlsocket(socketHandle,FIONBIO,&nonblocking)==SOCKET_ERROR) {
            const auto error=WSAGetLastError();Close();status.socketError=error;return;
        }
        activePort=settings.port;status.socketError=0;status.listening=true;
    }
    if(socketHandle==INVALID_SOCKET)return;
    std::array<uint8_t,65536> bytes;
    // Bound work even if another local process floods this socket.
    for(unsigned i=0;i<128;++i) {
        const int count=recv(socketHandle,reinterpret_cast<char*>(bytes.data()),int(bytes.size()),0);
        if(count==SOCKET_ERROR) {
            const auto error=WSAGetLastError();
            if(error!=WSAEWOULDBLOCK){Close();status.socketError=error;retryAt=now+2000;}
            break;
        }
        ++status.packets;
        if(!Receive(std::span(bytes.data(),size_t(count)),input,now))++status.malformed;
    }
}
void Shutdown() {
    ClearMirror();
    std::lock_guard lock(mutex);settings.enabled=false;Close();
}
}
