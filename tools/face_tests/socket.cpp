#include <winsock2.h>
#include "Anim/FaceTracking.hpp"
#include "Core/Paths.hpp"
#include <cstdio>
#include <cstdlib>
#include <string>
#include <vector>
using namespace cvr::face;
namespace cvr::paths { std::string FileA(Folder,const char*){return FACE_TEST_DIR "/face_tracking.ini";} }
namespace cvr::face {
bool InstallHooks(){return true;}
Status RigStatus(){Status result;result.hooks=true;return result;}
void ClearMirror(){}
}
void Check(bool pass,const char* message){if(!pass){std::fprintf(stderr,"FAIL %s\n",message);std::exit(1);}}
void String(std::vector<uint8_t>& b,std::string_view s){b.insert(b.end(),s.begin(),s.end());b.push_back(0);while(b.size()%4)b.push_back(0);}
int main() {
    WSADATA data{};Check(!WSAStartup(MAKEWORD(2,2),&data),"network startup");
    auto sender=socket(AF_INET,SOCK_DGRAM,IPPROTO_UDP);Check(sender!=INVALID_SOCKET,"sender socket");
    auto blocker=socket(AF_INET,SOCK_DGRAM,IPPROTO_UDP);Check(blocker!=INVALID_SOCKET,"blocker socket");
    int exclusive=1;
    setsockopt(blocker,SOL_SOCKET,SO_EXCLUSIVEADDRUSE,reinterpret_cast<const char*>(&exclusive),sizeof(exclusive));
    sockaddr_in address{};address.sin_family=AF_INET;address.sin_addr.s_addr=htonl(INADDR_LOOPBACK);
    Check(!bind(blocker,reinterpret_cast<const sockaddr*>(&address),sizeof(address)),"reserve unused loopback port");
    int size=sizeof(address);Check(!getsockname(blocker,reinterpret_cast<sockaddr*>(&address),&size),"read reserved port");
    Settings settings;settings.enabled=true;settings.port=ntohs(address.sin_port);SetSettings(settings);
    Tick();Check(!GetStatus().listening && GetStatus().socketError,"exclusive port conflict reported");
    closesocket(blocker);SetSettings(settings);Tick();
    Check(GetStatus().listening && !GetStatus().socketError && !GetStatus().saveError,"bind succeeds after conflicting listener exits");
    std::vector<uint8_t> packet;String(packet,Addresses[SmileL]);String(packet,",f");
    const uint32_t coefficient=htonl(std::bit_cast<uint32_t>(.75f));
    const auto* bytes=reinterpret_cast<const uint8_t*>(&coefficient);packet.insert(packet.end(),bytes,bytes+4);
    Check(sendto(sender,reinterpret_cast<const char*>(packet.data()),int(packet.size()),0,
        reinterpret_cast<const sockaddr*>(&address),sizeof(address))==int(packet.size()),"send OSC datagram");
    for(int i=0;i<20 && !GetInput().channels[SmileL].present;++i){Tick();Sleep(1);}
    Check(GetInput().channels[SmileL].value==.75f && GetStatus().packets==1,"native receiver publishes valid sample");
    packet.pop_back();sendto(sender,reinterpret_cast<const char*>(packet.data()),int(packet.size()),0,
        reinterpret_cast<const sockaddr*>(&address),sizeof(address));
    for(int i=0;i<20 && !GetStatus().malformed;++i){Tick();Sleep(1);}
    Check(GetStatus().malformed==1 && GetInput().channels[SmileL].value==.75f,"malformed input preserves prior sample");
    settings.enabled=false;SetSettings(settings);Tick();
    Check(!GetStatus().listening && !GetInput().channels[SmileL].present,"disable clears samples and closes socket");
    settings.enabled=true;SetSettings(settings);Tick();Check(GetStatus().listening,"enable reopens socket");
    Shutdown();Check(!GetStatus().listening,"shutdown closes socket");
    closesocket(sender);sender=socket(AF_INET,SOCK_DGRAM,IPPROTO_UDP);
    Check(!bind(sender,reinterpret_cast<const sockaddr*>(&address),sizeof(address)),"port is released for another listener");
    closesocket(sender);WSACleanup();
    std::puts("PASS exclusive localhost receiver, port conflict, data validation, disable and shutdown");
}
