#include "launcher_dialog.h"
#include <windows.h>
#include <commctrl.h>
#include <iterator>
#include <string>

extern "C" void SetWindowResolutionAndPersist(int width, int height);
extern "C" int GetCurrentWindowWidth();
extern "C" int GetCurrentWindowHeight();
extern "C" void SetRuntimeModeAndPersist(int mode);
extern "C" int GetXrRuntimeMode();

#define ID_COMBO_RES     101
#define ID_BUTTON_START  102
#define ID_COMBO_RUNTIME 103

// --- Visual constants -------------------------------------------------------
static const COLORREF kColBg     = RGB(248, 249, 251); // window background
static const COLORREF kColHeader = RGB(0, 120, 215);   // Windows accent blue
static const COLORREF kColSub    = RGB(120, 124, 130);  // muted gray
static const COLORREF kColLabel  = RGB(45, 48, 54);     // near-black label

static const int kMargin  = 26;
static const int kClientW = 372;
static const int kClientH = 336;
static const int kFieldW  = kClientW - 2 * kMargin;

struct ResolutionPreset {
    int width;
    int height;
    const wchar_t* label;
};

static const ResolutionPreset kResolutionPresets[] = {
    {1280, 1280, L"1280 x 1280"},
    {1440, 1440, L"1440 x 1440"},
    {2048, 2048, L"2048 x 2048"},
    {2160, 2160, L"2160 x 2160"},
    {2560, 2560, L"2560 x 2560"},
    {3072, 3072, L"3072 x 3072"},
    {3584, 3584, L"3584 x 3584"},
    {4096, 4096, L"4096 x 4096"},
};

struct RuntimeOption {
    int mode; // matches xr_runtime: 0 = OpenXR default, 1 = SteamVR
    const wchar_t* label;
};

static const RuntimeOption kRuntimeOptions[] = {
    {1, L"SteamVR  (OpenVR)"},
    {0, L"OpenXR  (Virtual Desktop)"},
};

static HFONT g_fontHeader = nullptr;
static HFONT g_fontSub    = nullptr;
static HFONT g_fontBody   = nullptr;
static HBRUSH g_brushBg   = nullptr;

static HWND g_hHeader  = nullptr;
static HWND g_hSub     = nullptr;
static HWND g_hRuntime = nullptr;
static HWND g_hRes     = nullptr;

static HFONT MakeFont(int pointSize, int weight) {
    HDC hdc = GetDC(nullptr);
    const int height = -MulDiv(pointSize, GetDeviceCaps(hdc, LOGPIXELSY), 72);
    ReleaseDC(nullptr, hdc);
    return CreateFontW(height, 0, 0, 0, weight, FALSE, FALSE, FALSE,
        DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY, DEFAULT_PITCH | FF_DONTCARE, L"Segoe UI");
}

static HWND MakeLabel(HWND parent, const wchar_t* text, int x, int y, int w, int h, HFONT font) {
    HWND label = CreateWindowW(L"STATIC", text,
        WS_VISIBLE | WS_CHILD | SS_LEFT,
        x, y, w, h, parent, nullptr, nullptr, nullptr);
    SendMessageW(label, WM_SETFONT, (WPARAM)font, TRUE);
    return label;
}

static HWND MakeCombo(HWND parent, int id, int x, int y, int w) {
    HWND combo = CreateWindowW(WC_COMBOBOXW, L"",
        CBS_DROPDOWNLIST | CBS_HASSTRINGS | WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL,
        x, y, w, 260, parent, (HMENU)(INT_PTR)id, nullptr, nullptr);
    SendMessageW(combo, WM_SETFONT, (WPARAM)g_fontBody, TRUE);
    return combo;
}

LRESULT CALLBACK LauncherWndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam) {
    static HWND hwndButton;

    switch (msg) {
    case WM_CREATE: {
        g_fontHeader = MakeFont(20, FW_SEMIBOLD);
        g_fontSub    = MakeFont(10, FW_NORMAL);
        g_fontBody   = MakeFont(11, FW_NORMAL);

        int y = kMargin;

        g_hHeader = MakeLabel(hwnd, L"CyberpunkVRPort", kMargin, y, kFieldW, 44, g_fontHeader);
        y += 46;
        g_hSub = MakeLabel(hwnd, L"VR Configuration", kMargin, y, kFieldW, 22, g_fontSub);
        y += 38;

        // --- VR Runtime ---
        MakeLabel(hwnd, L"VR Runtime", kMargin, y, kFieldW, 24, g_fontBody);
        y += 26;
        g_hRuntime = MakeCombo(hwnd, ID_COMBO_RUNTIME, kMargin, y, kFieldW);

        const int currentRuntime = GetXrRuntimeMode();
        int runtimeSel = 0;
        for (int i = 0; i < static_cast<int>(std::size(kRuntimeOptions)); ++i) {
            SendMessageW(g_hRuntime, CB_ADDSTRING, 0, (LPARAM)kRuntimeOptions[i].label);
            if (kRuntimeOptions[i].mode == currentRuntime) {
                runtimeSel = i;
            }
        }
        SendMessageW(g_hRuntime, CB_SETCURSEL, runtimeSel, 0);
        y += 50;

        // --- Render Resolution ---
        MakeLabel(hwnd, L"Render Resolution (per eye)", kMargin, y, kFieldW, 24, g_fontBody);
        y += 26;
        g_hRes = MakeCombo(hwnd, ID_COMBO_RES, kMargin, y, kFieldW);

        const int currentWidth = GetCurrentWindowWidth();
        const int currentHeight = GetCurrentWindowHeight();
        int resSel = 0;
        for (int i = 0; i < static_cast<int>(std::size(kResolutionPresets)); ++i) {
            SendMessageW(g_hRes, CB_ADDSTRING, 0, (LPARAM)kResolutionPresets[i].label);
            if (kResolutionPresets[i].width == currentWidth &&
                kResolutionPresets[i].height == currentHeight) {
                resSel = i;
            }
        }
        SendMessageW(g_hRes, CB_SETCURSEL, resSel, 0);
        y += 56;

        // --- Start button ---
        hwndButton = CreateWindowW(L"BUTTON", L"Start Game",
            WS_TABSTOP | WS_VISIBLE | WS_CHILD | BS_DEFPUSHBUTTON,
            kMargin, y, kFieldW, 38,
            hwnd, (HMENU)ID_BUTTON_START, nullptr, nullptr);
        SendMessageW(hwndButton, WM_SETFONT, (WPARAM)g_fontBody, TRUE);

        break;
    }

    case WM_CTLCOLORSTATIC: {
        HDC hdc = (HDC)wParam;
        HWND ctrl = (HWND)lParam;
        SetBkMode(hdc, TRANSPARENT);
        if (ctrl == g_hHeader) {
            SetTextColor(hdc, kColHeader);
        } else if (ctrl == g_hSub) {
            SetTextColor(hdc, kColSub);
        } else {
            SetTextColor(hdc, kColLabel);
        }
        return (LRESULT)g_brushBg;
    }

    case WM_COMMAND: {
        if (LOWORD(wParam) == ID_BUTTON_START) {
            int rIdx = (int)SendMessageW(g_hRuntime, CB_GETCURSEL, 0, 0);
            if (rIdx >= 0 && rIdx < static_cast<int>(std::size(kRuntimeOptions))) {
                SetRuntimeModeAndPersist(kRuntimeOptions[rIdx].mode);
            }
            int idx = (int)SendMessageW(g_hRes, CB_GETCURSEL, 0, 0);
            if (idx >= 0 && idx < static_cast<int>(std::size(kResolutionPresets))) {
                SetWindowResolutionAndPersist(kResolutionPresets[idx].width, kResolutionPresets[idx].height);
            }
            DestroyWindow(hwnd);
        }
        break;
    }

    case WM_DESTROY:
        if (g_fontHeader) { DeleteObject(g_fontHeader); g_fontHeader = nullptr; }
        if (g_fontSub)    { DeleteObject(g_fontSub);    g_fontSub = nullptr; }
        if (g_fontBody)   { DeleteObject(g_fontBody);   g_fontBody = nullptr; }
        PostQuitMessage(0);
        break;

    default:
        return DefWindowProc(hwnd, msg, wParam, lParam);
    }
    return 0;
}

void ShowLauncherDialog() {
    g_brushBg = CreateSolidBrush(kColBg);

    WNDCLASSW wc = {};
    wc.lpfnWndProc = LauncherWndProc;
    wc.hInstance = GetModuleHandle(nullptr);
    wc.lpszClassName = L"CyberpunkVRPortLauncherClass";
    wc.hbrBackground = g_brushBg;
    wc.hCursor = LoadCursor(nullptr, IDC_ARROW);

    RegisterClassW(&wc);

    const DWORD style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    RECT rc = {0, 0, kClientW, kClientH};
    AdjustWindowRect(&rc, style, FALSE);
    const int winW = rc.right - rc.left;
    const int winH = rc.bottom - rc.top;

    const int xPos = (GetSystemMetrics(SM_CXSCREEN) - winW) / 2;
    const int yPos = (GetSystemMetrics(SM_CYSCREEN) - winH) / 2;

    HWND hwnd = CreateWindowExW(
        0,
        L"CyberpunkVRPortLauncherClass",
        L"CyberpunkVRPort Configuration",
        style,
        xPos, yPos, winW, winH,
        nullptr, nullptr, wc.hInstance, nullptr
    );

    if (hwnd == nullptr) {
        if (g_brushBg) { DeleteObject(g_brushBg); g_brushBg = nullptr; }
        return;
    }

    ShowWindow(hwnd, SW_SHOW);
    UpdateWindow(hwnd);

    MSG msg = {};
    while (GetMessage(&msg, nullptr, 0, 0)) {
        if (IsDialogMessage(hwnd, &msg)) {
            continue;
        }
        TranslateMessage(&msg);
        DispatchMessage(&msg);
    }

    if (g_brushBg) { DeleteObject(g_brushBg); g_brushBg = nullptr; }
}
