#include "launcher_dialog.h"
#include <windows.h>
#include <commctrl.h>
#include <iterator>
#include <string>

extern "C" void SetWindowResolutionAndPersist(int width, int height);
extern "C" int GetCurrentWindowWidth();
extern "C" int GetCurrentWindowHeight();

#define ID_COMBOBOX 101
#define ID_BUTTON_START 102

struct ResolutionPreset {
    int width;
    int height;
    const wchar_t* label;
};

static const ResolutionPreset kResolutionPresets[] = {
    {2048, 2048, L"2048x2048"},
    {2560, 2560, L"2560x2560"},
    {3072, 3072, L"3072x3072"},
    {3584, 3584, L"3584x3584"},
    {4096, 4096, L"4096x4096"},
};

LRESULT CALLBACK LauncherWndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam) {
    static HWND hwndCombo;
    static HWND hwndButton;

    switch (msg) {
    case WM_CREATE: {
        CreateWindowW(L"STATIC", L"Select VR Resolution:",
            WS_VISIBLE | WS_CHILD | SS_LEFT,
            20, 20, 250, 20,
            hwnd, NULL, NULL, NULL);

        hwndCombo = CreateWindowW(WC_COMBOBOXW, L"",
            CBS_DROPDOWNLIST | CBS_HASSTRINGS | WS_CHILD | WS_OVERLAPPED | WS_VISIBLE,
            20, 50, 250, 150,
            hwnd, (HMENU)ID_COMBOBOX, NULL, NULL);

        int currentWidth = GetCurrentWindowWidth();
        int currentHeight = GetCurrentWindowHeight();
        int selectedIndex = 0;

        for (int i = 0; i < static_cast<int>(std::size(kResolutionPresets)); ++i) {
            SendMessageW(hwndCombo, CB_ADDSTRING, 0, (LPARAM)kResolutionPresets[i].label);
            if (kResolutionPresets[i].width == currentWidth &&
                kResolutionPresets[i].height == currentHeight) {
                selectedIndex = i;
            }
        }
        SendMessageW(hwndCombo, CB_SETCURSEL, selectedIndex, 0);

        hwndButton = CreateWindowW(L"BUTTON", L"Start Game",
            WS_TABSTOP | WS_VISIBLE | WS_CHILD | BS_DEFPUSHBUTTON,
            80, 100, 120, 30,
            hwnd, (HMENU)ID_BUTTON_START, NULL, NULL);

        break;
    }

    case WM_COMMAND: {
        if (LOWORD(wParam) == ID_BUTTON_START) {
            int idx = (int)SendMessageW(hwndCombo, CB_GETCURSEL, 0, 0);
            if (idx >= 0 && idx < static_cast<int>(std::size(kResolutionPresets))) {
                SetWindowResolutionAndPersist(kResolutionPresets[idx].width, kResolutionPresets[idx].height);
            }
            DestroyWindow(hwnd);
        }
        break;
    }

    case WM_DESTROY:
        PostQuitMessage(0);
        break;

    default:
        return DefWindowProc(hwnd, msg, wParam, lParam);
    }
    return 0;
}

void ShowLauncherDialog() {
    WNDCLASSW wc = {};
    wc.lpfnWndProc = LauncherWndProc;
    wc.hInstance = GetModuleHandle(NULL);
    wc.lpszClassName = L"CyberpunkVRPortLauncherClass";
    wc.hbrBackground = (HBRUSH)(COLOR_WINDOW + 1);

    RegisterClassW(&wc);

    HWND hwnd = CreateWindowExW(
        0,
        L"CyberpunkVRPortLauncherClass",
        L"CyberpunkVRPort Configuration",
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
        CW_USEDEFAULT, CW_USEDEFAULT, 310, 190,
        NULL, NULL, wc.hInstance, NULL
    );

    if (hwnd == NULL) {
        return;
    }

    // Center window
    RECT rc;
    GetWindowRect(hwnd, &rc);
    int xPos = (GetSystemMetrics(SM_CXSCREEN) - (rc.right - rc.left)) / 2;
    int yPos = (GetSystemMetrics(SM_CYSCREEN) - (rc.bottom - rc.top)) / 2;
    SetWindowPos(hwnd, 0, xPos, yPos, 0, 0, SWP_NOZORDER | SWP_NOSIZE);

    ShowWindow(hwnd, SW_SHOW);
    UpdateWindow(hwnd);

    MSG msg = {};
    while (GetMessage(&msg, NULL, 0, 0)) {
        TranslateMessage(&msg);
        DispatchMessage(&msg);
    }
}
