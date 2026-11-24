// launcher_preview OUT_PREFIX [hmd] [engine_stereo]: shows the launcher (src/Overlay/LauncherDialog.cpp) without the
// game and saves what it looks like: OUT_PREFIX.bmp (the window) and OUT_PREFIX_open.bmp (the resolution list
// dropped down), captured from the screen, then closes it. For looking at the dialog while changing it.
#include <windows.h>
#include <cstdio>
#include <string>
#include "Overlay/LauncherDialog.hpp"

HWND g_gameHwnd = nullptr;
void Log(const char*, ...) {}
int savedWidth = 2160, savedHeight = 2160, savedHmd = 3, savedRuntime = 1, savedDebug = 0, engineStereo = 1;
extern "C" void SetWindowResolutionAndPersist(int w, int h) { savedWidth = w; savedHeight = h; }
extern "C" int GetCurrentWindowWidth() { return savedWidth; }
extern "C" int GetCurrentWindowHeight() { return savedHeight; }
extern "C" void SetRuntimeModeAndPersist(int mode) { savedRuntime = mode; }
extern "C" int GetXrRuntimeMode() { return savedRuntime; }
extern "C" void SetHmdTypeAndPersist(int value) { savedHmd = value; }
extern "C" int GetCurrentHmdType() { return savedHmd; }
extern "C" void SetLauncherDebugAndPersist(int value) { savedDebug = value; }
extern "C" int GetLauncherDebug() { return savedDebug; }
extern "C" int GetLauncherEngineStereo() { return engineStereo; }
int savedCustom = 0, savedLobby = 1;
extern "C" int GetLauncherCustomResolution() { return savedCustom; }
extern "C" void SetLauncherCustomResolution(int on) { savedCustom = on; }
extern "C" int GetLauncherLobby() { return savedLobby; }
extern "C" void SetLauncherLobbyAndPersist(int on) { savedLobby = on; }
int savedInterface = 1, savedPhysics = 1;
extern "C" int GetLauncherInterface() { return savedInterface; }
extern "C" void SetLauncherInterfaceAndPersist(int on) { savedInterface = on; }
extern "C" int GetLauncherHandPhysics() { return savedPhysics; }
extern "C" void SetLauncherHandPhysicsAndPersist(int on) { savedPhysics = on; }

static std::string g_out;

static void SaveScreenRect(const RECT& r, const std::string& path) {
    const int w = r.right - r.left, h = r.bottom - r.top;
    HDC screen = GetDC(nullptr);
    HDC mem = CreateCompatibleDC(screen);
    BITMAPINFO bi{};
    bi.bmiHeader = {sizeof(BITMAPINFOHEADER), w, -h, 1, 32, BI_RGB};
    void* bits = nullptr;
    HBITMAP bmp = CreateDIBSection(screen, &bi, DIB_RGB_COLORS, &bits, nullptr, 0);
    HGDIOBJ old = SelectObject(mem, bmp);
    BitBlt(mem, 0, 0, w, h, screen, r.left, r.top, SRCCOPY | CAPTUREBLT);
    SelectObject(mem, old);
    BITMAPFILEHEADER fh{0x4D42, DWORD(sizeof(fh) + sizeof(BITMAPINFOHEADER) + DWORD(w) * h * 4), 0, 0,
                        DWORD(sizeof(fh) + sizeof(BITMAPINFOHEADER))};
    if (FILE* f = std::fopen(path.c_str(), "wb")) {
        std::fwrite(&fh, sizeof(fh), 1, f);
        std::fwrite(&bi.bmiHeader, sizeof(BITMAPINFOHEADER), 1, f);
        std::fwrite(bits, 4, size_t(w) * h, f);
        std::fclose(f);
    }
    DeleteObject(bmp);
    DeleteDC(mem);
    ReleaseDC(nullptr, screen);
}

static DWORD WINAPI Capture(void*) {
    HWND window = nullptr;
    for (int i = 0; i < 200 && !window; ++i) {
        Sleep(25);
        window = FindWindowW(L"CyberpunkVRPortLauncherClass", nullptr);
    }
    if (!window) return 1;
    Sleep(600);
    RECT r{};
    GetWindowRect(window, &r);
    SaveScreenRect(r, g_out + ".bmp");
    HWND res = GetDlgItem(window, 101);
    SendMessageW(res, CB_SHOWDROPDOWN, TRUE, 0);
    Sleep(500);
    COMBOBOXINFO info{sizeof(info)};
    GetComboBoxInfo(res, &info);
    RECT list{};
    GetWindowRect(info.hwndList, &list);
    RECT all{std::min(r.left, list.left), std::min(r.top, list.top), std::max(r.right, list.right),
             std::max(r.bottom, list.bottom)};
    SaveScreenRect(all, g_out + "_open.bmp");
    SendMessageW(res, CB_SHOWDROPDOWN, FALSE, 0);
    PostMessageW(window, WM_CLOSE, 0, 0);
    return 0;
}

int main(int argc, char** argv) {
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    g_out = argc > 1 ? argv[1] : "launcher";
    if (argc > 2) savedHmd = std::atoi(argv[2]);
    if (argc > 3) engineStereo = std::atoi(argv[3]);
    CloseHandle(CreateThread(nullptr, 0, Capture, nullptr, 0, nullptr));
    ShowLauncherDialog();
    return 0;
}
