#include <windows.h>
#include <commctrl.h>
#include <cstdint>
#include <iostream>
#include <stdexcept>
#include <string>
#include <vector>
#include "Overlay/LauncherDialog.hpp"
// Exercise the actual IAT handlers with fake cursor functions. The window test
// runs on an unshown desktop and never clips or moves the real pointer.
#include "../../src/Hooks/OsInput.cpp"

volatile int g_verboseLog=0;
bool g_cursorClipped=false;
void Log(const char*,...) {}
bool OverlayIsVisible(){return false;}
extern "C" UINT GetForcedDisplayModeWidth(){return 7654;}
extern "C" UINT GetForcedDisplayModeHeight(){return 4321;}
extern "C" UINT GetForcedSwapchainWidth(){return 7654;}
extern "C" UINT GetForcedSwapchainHeight(){return 4321;}
extern "C" UINT GetForcedWindowWidth(){return 640;}
extern "C" UINT GetForcedWindowHeight(){return 480;}
extern "C" int GetMenuMode(){return 0;}
// What OsInput.cpp asks of the desktop mirror and the swap chain since the desktop crop: there is no game
// swap chain or monitor-placed window here, so nothing maps and nothing is placed.
bool ComputeDesktopCoverTransform(UINT,UINT,UINT,UINT,DesktopCoverTransform& out){out={};return false;}
bool GetDesktopPhysicalClientRect(HWND,RECT& rect,POINT& origin){rect={};origin={};return false;}
bool GetDesktopMonitorWindowRect(HWND,RECT& rect,RECT*,RECT*){rect={};return false;}
void FitRenderSizeInsideBounds(UINT w,UINT h,UINT,UINT,UINT& outW,UINT& outH){outW=w;outH=h;}
extern "C" __declspec(dllexport) int CyberpunkVR_RuntimeDiagnostics=0;
int savedWidth=2048,savedHeight=2048,savedHmd=0,savedRuntime=0,savedDebug=0,saves=0;
extern "C" void SetWindowResolutionAndPersist(int w,int h){savedWidth=w;savedHeight=h;++saves;}
extern "C" int GetCurrentWindowWidth(){return savedWidth;}
extern "C" int GetCurrentWindowHeight(){return savedHeight;}
extern "C" void SetRuntimeModeAndPersist(int mode){savedRuntime=mode;}
extern "C" int GetXrRuntimeMode(){return savedRuntime;}
extern "C" void SetHmdTypeAndPersist(int value){savedHmd=value;}
extern "C" int GetCurrentHmdType(){return savedHmd;}
extern "C" void SetLauncherDebugAndPersist(int value){savedDebug=value;}
extern "C" int GetLauncherDebug(){return savedDebug;}
void Check(bool ok,const char* why){if(!ok)throw std::runtime_error(why);}
unsigned clips{},warps{};bool clipped{};
BOOL WINAPI FakeClip(const RECT* rect){++clips;clipped=rect!=nullptr;return TRUE;}
BOOL WINAPI FakeWarp(int,int){++warps;return TRUE;}
BOOL WINAPI FakePos(POINT* p){*p={47,29};return TRUE;}
BOOL WINAPI FakeInfo(CURSORINFO* info){info->ptScreenPos={47,29};return TRUE;}
DWORD WINAPI FakeMessagePos(){return MAKELONG(47,29);}
int mode{};UINT_PTR timer{};ULONGLONG deadline{};std::string failure;
RECT originalGameRect{};
std::wstring ComboText(HWND combo,int index) {
    const auto length=SendMessageW(combo,CB_GETLBTEXTLEN,index,0);
    Check(length!=CB_ERR,"missing combo item");
    std::wstring text(static_cast<size_t>(length)+1,L'\0');
    SendMessageW(combo,CB_GETLBTEXT,index,reinterpret_cast<LPARAM>(text.data()));
    text.resize(static_cast<size_t>(length));return text;
}
void SelectHmd(HWND window,int index) {
    SendMessageW(GetDlgItem(window,104),CB_SETCURSEL,index,0);
    SendMessageW(window,WM_COMMAND,MAKEWPARAM(104,CBN_SELCHANGE),reinterpret_cast<LPARAM>(GetDlgItem(window,104)));
}
std::vector<std::wstring> Resolutions(HWND window,int headset) {
    SelectHmd(window,headset);HWND combo=GetDlgItem(window,101);
    std::vector<std::wstring> result;
    const auto count=SendMessageW(combo,CB_GETCOUNT,0,0);
    for(int i=0;i<count;++i)result.push_back(ComboText(combo,i));
    return result;
}
VOID CALLBACK Inspect(HWND,UINT,UINT_PTR,DWORD){
    HWND window=FindWindowW(L"CyberpunkVRPortLauncherClass",nullptr);
    if(!window && GetTickCount64()<deadline)return;
    KillTimer(nullptr,timer);
    try {
        Check(window && IsLauncherOpen(),"launcher never became available");
        wchar_t title[100]{};GetWindowTextW(window,title,100);
        Check(std::wstring(title)==L"CyberpunkVRPort Configuration","Unicode title was truncated");
        Check(IsWindowVisible(window) && (GetWindowLongPtrW(window,GWL_EXSTYLE)&WS_EX_TOPMOST),"launcher is not visible/topmost");
        Check(!IsWindowVisible(g_gameHwnd) && !IsIconic(g_gameHwnd),"fullscreen owner was not hidden without minimizing");
        RECT gameRect{};GetWindowRect(g_gameHwnd,&gameRect);
        Check(EqualRect(&gameRect,&originalGameRect),"launcher changed or virtualized game window dimensions");
        RECT clip{0,0,1000,1000};HookedClipCursor(&clip);Check(!clipped,"launcher retained full-size cursor confinement");
        clip={1,1,2,2};HookedClipCursor(&clip);Check(!clipped,"launcher retained one-pixel cursor confinement");
        const auto before=warps;HookedSetCursorPos(800,600);Check(warps==before,"game can recenter launcher mouse");
        POINT position{};HookedGetCursorPos(&position);Check(position.x==47 && position.y==29,"launcher cursor coordinates were scaled");
        CURSORINFO info{sizeof(info)};HookedGetCursorInfo(&info);Check(info.ptScreenPos.x==47 && info.ptScreenPos.y==29,"launcher cursor info was scaled");
        Check(HookedGetMessagePos()==MAKELONG(47,29),"launcher message coordinates were scaled");
        ShowLauncherDialog();Check(IsWindow(window),"reentrant request destroyed the active launcher");
        if(mode==0) {
            Check(GetDlgItem(window,102) && GetDlgItem(window,104),"launcher controls missing");
            const auto headset=GetDlgItem(window,104);
            Check(SendMessageW(headset,CB_GETCOUNT,0,0)==17,"headset count changed unexpectedly");
            Check(ComboText(headset,3)==L"Pico4" && ComboText(headset,12)==L"Bigscreen Beyond 2/2e","existing headset IDs moved");
            Check(ComboText(headset,13)==L"Steam Frame" && ComboText(headset,14)==L"HP Reverb G2","new headset entries missing");
            Check(ComboText(headset,15)==L"MeganeX superlight 8K","MeganeX superlight 8K entry missing");
            Check(ComboText(headset,16)==L"Samsung Galaxy XR","Samsung Galaxy XR entry missing");
            const auto galaxy=Resolutions(window,16);
            Check(galaxy.size()==3 && galaxy.front()==L"4080 x 4768 (VD Rampage, native)" && galaxy.back()==L"3552 x 4144 (VD Monster)","Samsung Galaxy XR resolution ladder incorrect");
            // Steam Frame is selected last: the nearest-cost check below reads its ladder.
            const auto meganex=Resolutions(window,15),quest3=Resolutions(window,2);
            Check(meganex.size()==8 && meganex.front()==L"3824 x 3568 (cost of the 3552 x 3840 panel)" && meganex.back()==L"6216 x 5792","MeganeX superlight 8K resolution ladder incorrect");
            Check(quest3.size()==10 && quest3[4]==L"2648 x 2864","Quest 3 resolution ladder incorrect");
            const auto pico=Resolutions(window,3),reverb=Resolutions(window,14),frame=Resolutions(window,13);
            Check(pico.size()==8 && pico.front()==L"2160 x 2160 (Native)","Pico 4 resolution ladder incorrect");
            Check(reverb.size()==8 && reverb.front()==L"2192 x 2136 (cost of the 2160 x 2160 panel)" && reverb.back()==L"6080 x 5928","Reverb G2 resolution ladder incorrect");
            Check(frame.size()==8 && frame.front()==L"2152 x 2168 (cost of the 2160 x 2160 panel)" && frame.back()==L"5984 x 6024","Steam Frame resolution ladder incorrect");
            // a saved size no ladder of this headset offers selects the entry nearest in cost
            Check(SendMessageW(GetDlgItem(window,101),CB_GETCURSEL,0,0)==2,"retired 2048x2048 did not select the Steam Frame entry nearest in cost");
            int w=2464,h=2448;Check(SnapLauncherResolution(5,&w,&h)==1 && w==2256 && h==2672,"retired Crystal pick was not moved to the nearest cost");
            Check(SnapLauncherResolution(5,&w,&h)==0 && w==2256 && h==2672,"a ladder entry was moved");
            w=2648;h=2864;Check(SnapLauncherResolution(99,&w,&h)==0 && w==2648,"an unknown headset moved the pick");
            SelectHmd(window,1);
            SendMessageW(GetDlgItem(window,105),BM_SETCHECK,BST_CHECKED,0);
            SendMessageW(GetDlgItem(window,102),BM_CLICK,0,0);
        } else if(mode==1)PostMessageW(window,WM_CLOSE,0,0);
        else if(mode==2)PostQuitMessage(19);
        else if(mode==3) {
            SelectHmd(window,13);
            SendMessageW(GetDlgItem(window,101),CB_SETCURSEL,4,0);
            SendMessageW(GetDlgItem(window,102),BM_CLICK,0,0);
            Check(savedHmd==13 && savedWidth==3064 && savedHeight==3080,"Steam Frame selection was not saved");
        } else if(mode==4) {
            Check(SendMessageW(GetDlgItem(window,104),CB_GETCURSEL,0,0)==13 && SendMessageW(GetDlgItem(window,101),CB_GETCURSEL,0,0)==4,"Steam Frame selection did not reload");
            SelectHmd(window,14);
            SendMessageW(GetDlgItem(window,101),CB_SETCURSEL,7,0);
            SendMessageW(GetDlgItem(window,102),BM_CLICK,0,0);
            Check(savedHmd==14 && savedWidth==6080 && savedHeight==5928,"Reverb G2 selection was not saved");
        } else {
            Check(SendMessageW(GetDlgItem(window,104),CB_GETCURSEL,0,0)==14 && SendMessageW(GetDlgItem(window,101),CB_GETCURSEL,0,0)==7,"Reverb G2 selection did not reload");
            PostMessageW(window,WM_CLOSE,0,0);
        }
    }catch(const std::exception& e){failure=e.what();if(window)PostMessageW(window,WM_CLOSE,0,0);else PostQuitMessage(20);}
}
LRESULT CALLBACK RejectWindow(HWND hwnd,UINT message,WPARAM w,LPARAM l){return message==WM_NCCREATE?FALSE:DefWindowProcW(hwnd,message,w,l);}
int main()try {
    const auto originalDesktop=GetThreadDesktop(GetCurrentThreadId());
    const auto name=L"CVRLauncherTests-"+std::to_wstring(GetCurrentProcessId());
    HDESK desktop=CreateDesktopW(name.c_str(),nullptr,nullptr,0,GENERIC_ALL,nullptr);
    Check(desktop && SetThreadDesktop(desktop),"cannot create isolated test desktop");
    WNDCLASSW gameClass{};gameClass.lpfnWndProc=DefWindowProcW;gameClass.hInstance=GetModuleHandleW(nullptr);gameClass.lpszClassName=L"CVRLauncherTestGame";
    Check(RegisterClassW(&gameClass)!=0,"test game class unavailable");
    g_gameHwnd=CreateWindowExW(WS_EX_TOPMOST,gameClass.lpszClassName,L"Test Game",WS_POPUP|WS_VISIBLE,20,20,640,480,nullptr,nullptr,gameClass.hInstance,nullptr);
    Check(g_gameHwnd!=nullptr,"test owner unavailable");
    GetWindowRect(g_gameHwnd,&originalGameRect);
    InstallOSHooks();
    g_origClipCursor=FakeClip;g_origSetCursorPos=FakeWarp;g_origGetCursorPos=FakePos;
    g_origGetCursorInfo=FakeInfo;g_origGetMessagePos=FakeMessagePos;
    for(mode=0;mode<6;++mode){
        deadline=GetTickCount64()+3000;timer=SetTimer(nullptr,0,20,Inspect);Check(timer!=0,"timer unavailable");
        ShowLauncherDialog();KillTimer(nullptr,timer);
        Check(failure.empty(),failure.c_str());
        Check(!IsLauncherOpen() && IsWindowVisible(g_gameHwnd),"close did not restore the game and input mode");
        Check(!FindWindowW(L"CyberpunkVRPortLauncherClass",nullptr),"launcher window leaked");
        MSG message{};
        if(mode==2)Check(PeekMessageW(&message,nullptr,WM_QUIT,WM_QUIT,PM_REMOVE) && message.wParam==19,"application WM_QUIT was swallowed");
        else Check(!PeekMessageW(&message,nullptr,WM_QUIT,WM_QUIT,PM_REMOVE),"closing launcher posted application WM_QUIT");
        RECT client{};HookedGetClientRect(g_gameHwnd,&client);Check(client.right==7654 && client.bottom==4321,"game virtual input did not resume");
    }
    Check(saves==3 && savedDebug==1,"Start did not save once per selection");
    WNDCLASSW failed{};failed.lpfnWndProc=RejectWindow;failed.hInstance=gameClass.hInstance;failed.lpszClassName=L"CyberpunkVRPortLauncherClass";
    Check(RegisterClassW(&failed)!=0,"failure class unavailable");ShowLauncherDialog();
    Check(!IsLauncherOpen() && IsWindowVisible(g_gameHwnd),"creation failure stranded game/input");
    UnregisterClassW(failed.lpszClassName,failed.hInstance);
    DestroyWindow(g_gameHwnd);g_gameHwnd=nullptr;UnregisterClassW(gameClass.lpszClassName,gameClass.hInstance);
    Check(SetThreadDesktop(originalDesktop),"cannot restore test thread desktop");CloseDesktop(desktop);
    std::cout<<"PASS window-shaped Steam Frame/Reverb G2/MeganeX ladders, nearest-cost snap of retired picks, saved IDs, Unicode title, topmost, hidden fullscreen owner, input passthrough, start/close/quit, reentry and creation failure\n";
    return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
