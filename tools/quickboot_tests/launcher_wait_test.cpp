#include "Overlay/LauncherDialog.hpp"
#include "Render/MonitorOutput.hpp"
namespace cvr {
MonitorOutput GetMonitorOutput() { return MonitorOutput::Default; }
void SetMonitorOutput(MonitorOutput) {}
}
#include <windows.h>
#include <cstdio>
#include <stdexcept>
HWND g_gameHwnd{};
void Log(const char*,...){}
extern "C" {
void SetWindowResolutionAndPersist(int,int){}int GetCurrentWindowWidth(){return 2560;}int GetCurrentWindowHeight(){return 2560;}
void SetRuntimeModeAndPersist(int){}int GetXrRuntimeMode(){return 0;}
void SetHmdTypeAndPersist(int){}int GetCurrentHmdType(){return 3;}
void SetLauncherDebugAndPersist(int){}int GetLauncherDebug(){return 0;}
int GetLauncherEngineStereo(){return 1;}
void SetLauncherCustomResolution(int){}int GetLauncherCustomResolution(){return 0;}
void SetLauncherLobbyAndPersist(int){}int GetLauncherLobby(){return 1;}
void SetLauncherInterfaceAndPersist(int){}int GetLauncherInterface(){return 1;}
void SetLauncherHandPhysicsAndPersist(int){}int GetLauncherHandPhysics(){return 1;}
void SetLauncherModes(int,int){}int GetLauncherVrEnabled(){return 1;}int GetLauncherWeaponPlane(){return 0;}
}
static unsigned messages{};
static LRESULT CALLBACK WindowProc(HWND hwnd,UINT message,WPARAM w,LPARAM l){if(message==WM_APP+1){++messages;return 0;}return DefWindowProcW(hwnd,message,w,l);}
static void Require(bool v,const char* text){if(!v)throw std::runtime_error(text);}
int main() try {
    WNDCLASSW wc{};wc.hInstance=GetModuleHandleW(nullptr);wc.lpfnWndProc=WindowProc;wc.lpszClassName=L"CvrLauncherWaitTest";
    Require(RegisterClassW(&wc)!=0,"class registration");HWND window=CreateWindowExW(0,wc.lpszClassName,L"",0,0,0,1,1,nullptr,nullptr,wc.hInstance,nullptr);
    Require(window!=nullptr,"hidden test window");PostMessageW(window,WM_APP+1,0,0);WaitForLauncherStartup(0);Require(messages==0,"disabled wait handled messages");
    const auto start=GetTickCount64();WaitForLauncherStartup(50);Require(messages==1,"startup messages were not dispatched");Require(GetTickCount64()-start>=40,"wait ended too early");
    PostQuitMessage(42);const auto quitStart=GetTickCount64();WaitForLauncherStartup(5000);Require(GetTickCount64()-quitStart<500,"quit did not interrupt wait");
    MSG message{};Require(PeekMessageW(&message,nullptr,WM_QUIT,WM_QUIT,PM_REMOVE) && message.wParam==42,"quit message was lost");DestroyWindow(window);
    std::puts("PASS zero-delay, startup message dispatch, bounded wait and preserved quit");return 0;
}catch(const std::exception& e){std::fprintf(stderr,"%s\n",e.what());return 1;}
