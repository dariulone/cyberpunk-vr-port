#include "launcher_dialog.h"
#include <windows.h>
#include <commctrl.h>
#include <string>

extern "C" void SetWindowResolutionAndPersist(int size);
extern "C" int GetCurrentWindowWidth();

#define ID_COMBOBOX 101
#define ID_BUTTON_START 102

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

        const int resolutions[] = { 2048, 2560, 3072, 3584, 4096 };
        int currentRes = GetCurrentWindowWidth();
        int selectedIndex = 0;

        for (int i = 0; i < 5; ++i) {
            std::wstring resStr = std::to_wstring(resolutions[i]) + L"x" + std::to_wstring(resolutions[i]);
            SendMessageW(hwndCombo, CB_ADDSTRING, 0, (LPARAM)resStr.c_str());
            if (resolutions[i] == currentRes) {
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
            const int resolutions[] = { 2048, 2560, 3072, 3584, 4096 };
            if (idx >= 0 && idx < 5) {
                SetWindowResolutionAndPersist(resolutions[idx]);
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
