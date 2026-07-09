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
extern "C" void SetHmdTypeAndPersist(int hmdType);
extern "C" int GetCurrentHmdType();

#define ID_COMBO_RES     101
#define ID_BUTTON_START  102
#define ID_COMBO_RUNTIME 103
#define ID_COMBO_HMD     104

// --- Visual constants -------------------------------------------------------
static const COLORREF kColBg     = RGB(248, 249, 251); // window background
static const COLORREF kColHeader = RGB(0, 120, 215);   // Windows accent blue
static const COLORREF kColSub    = RGB(120, 124, 130);  // muted gray
static const COLORREF kColLabel  = RGB(45, 48, 54);     // near-black label
static const int kMargin  = 26;
static const int kClientW = 372;
static const int kClientH = 412;   // <-- Aumentato da 336 per ospitare combo HMD
static const int kFieldW  = kClientW - 2 * kMargin;

struct ResolutionPreset {
    int width;
    int height;
    const wchar_t* label;
};

struct HmdPreset {
    int mhdType;
    const wchar_t* hmd_type;
    const wchar_t* label;
    const ResolutionPreset* resolutions;
    int resolutionCount;
};

// --- Per-HMD resolution lists ---
// Risoluzioni NATIVE per ogni visore (per occhio)
// --- Per-HMD resolution lists ---
static const ResolutionPreset kQuest2Resolutions[] = {
    {1832, 1920, L"1832 x 1920 (Native)"},
    {1680, 1760, L"1680 x 1760"},
    {2048, 2138, L"2048 x 2138"},
    {2560, 2672, L"2560 x 2672"},
};

static const ResolutionPreset kQuest3SResolutions[] = {
    {1920, 1880, L"1920 x 1880 (Native)"},
    {1680, 1646, L"1680 x 1646"},
    {2048, 2006, L"2048 x 2006"},
    {2560, 2507, L"2560 x 2507"},
};

static const ResolutionPreset kQuest3Resolutions[] = {
    {2064, 2208, L"2064 x 2208 (Native)"},
    {1832, 1957, L"1832 x 1957"},
    {2048, 2189, L"2048 x 2189"},
    {2400, 2564, L"2400 x 2564"},
    {2560, 2735, L"2560 x 2735"},
};

static const ResolutionPreset kPico4Resolutions[] = {
    {2160, 2160, L"2160 x 2160 (Native)"},
    {1920, 1920, L"1920 x 1920"},
    {2048, 2048, L"2048 x 2048"},
    {2560, 2560, L"2560 x 2560"},
    {3072, 3072, L"3072 x 3072"},
};

static const ResolutionPreset kPico4UltraResolutions[] = {
    {2160, 2160, L"2160 x 2160 (Native)"},
    {1920, 1920, L"1920 x 1920"},
    {2048, 2048, L"2048 x 2048"},
    {2560, 2560, L"2560 x 2560"},
    {3072, 3072, L"3072 x 3072"},
};

static const ResolutionPreset kCrystalOGResolutions[] = {
    {2464, 2448, L"2464 x 2448 (Native)"},
    {2160, 2145, L"2160 x 2145"},
    {2048, 2034, L"2048 x 2034"},
    {2560, 2542, L"2560 x 2542"},
    {3072, 3051, L"3072 x 3051"},
};

static const ResolutionPreset kCrystalLightResolutions[] = {
    {2464, 2448, L"2464 x 2448 (Native)"},
    {2160, 2145, L"2160 x 2145"},
    {2048, 2034, L"2048 x 2034"},
    {2560, 2542, L"2560 x 2542"},
    {3072, 3051, L"3072 x 3051"},
};

static const ResolutionPreset kCrystalSuperResolutions[] = {
    {2464, 2448, L"2464 x 2448 (Native)"},
    {2160, 2145, L"2160 x 2145"},
    {2048, 2034, L"2048 x 2034"},
    {2560, 2542, L"2560 x 2542"},
    {3072, 3051, L"3072 x 3051"},
};

// Crystal Super Ultra Wide: risoluzioni quadrate confermate (Crystal aspect ~1.0)
static const ResolutionPreset kCrystalWFResolutions[] = {
    {2464, 2448, L"2464 x 2448 (Native)"},
    {2160, 2145, L"2160 x 2145"},
    {2048, 2034, L"2048 x 2034"},
    {2560, 2542, L"2560 x 2542"},
    {3072, 3051, L"3072 x 3051"},
};

// Valve Index: aspect ratio nativo 0.9 (1440x1600)
static const ResolutionPreset kValveIndexResolutions[] = {
    {1440, 1600, L"1440 x 1600 (Native)"},
    {1260, 1400, L"1260 x 1400"},
    {1080, 1200, L"1080 x 1200"},
    {1800, 2000, L"1800 x 2000"},
};

static const HmdPreset kHmdPresets[] = {
    {0, L"Q2",    L"Meta Quest2",     kQuest2Resolutions,       _countof(kQuest2Resolutions)},
    {1, L"Q3S",   L"Meta Quest3s",    kQuest3SResolutions,      _countof(kQuest3SResolutions)},
    {2, L"Q3",    L"Meta Quest3",     kQuest3Resolutions,       _countof(kQuest3Resolutions)},
    {3, L"P4",    L"Pico4",           kPico4Resolutions,        _countof(kPico4Resolutions)},
    {4, L"P4U",   L"Pico4 Ultra",     kPico4UltraResolutions,   _countof(kPico4UltraResolutions)},
    {5, L"CRYOG", L"Crystal OG",      kCrystalOGResolutions,    _countof(kCrystalOGResolutions)},
    {6, L"GRYLI", L"Crystal Light",   kCrystalLightResolutions, _countof(kCrystalLightResolutions)},
    {7, L"CRYSU", L"Crystal Super",   kCrystalSuperResolutions, _countof(kCrystalSuperResolutions)},
    {8, L"CRYWF", L"Crystal Utra Wide", kCrystalWFResolutions,    _countof(kCrystalWFResolutions)},
    {9, L"VINDEX",L"Valve Index",     kValveIndexResolutions,   _countof(kValveIndexResolutions)},
};

struct RuntimeOption {
    int mode; // matches xr_runtime: 0 = OpenXR default, 1 = SteamVR
    const wchar_t* label;
};

static const RuntimeOption kRuntimeOptions[] = {
    {1, L"OpenVR  (SteamVR)"},
    {0, L"OpenXR  (VDXR, PimaxOpenXR)"},
};

static HFONT g_fontHeader = nullptr;
static HFONT g_fontSub    = nullptr;
static HFONT g_fontBody   = nullptr;
static HBRUSH g_brushBg   = nullptr;
static HWND g_hHeader  = nullptr;
static HWND g_hSub     = nullptr;
static HWND g_hRuntime = nullptr;
static HWND g_hHmd     = nullptr;   // <-- NUOVO
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

// --- NUOVA FUNZIONE: popola la combo risoluzioni in base all'HMD selezionato ---
static void PopulateResolutionCombo(int hmdIndex) {
    if (!g_hRes) return;
    SendMessageW(g_hRes, CB_RESETCONTENT, 0, 0);

    if (hmdIndex < 0 || hmdIndex >= static_cast<int>(std::size(kHmdPresets))) {
        hmdIndex = 0;
    }

    const HmdPreset& hmd = kHmdPresets[hmdIndex];
    const int currentWidth = GetCurrentWindowWidth();
    const int currentHeight = GetCurrentWindowHeight();
    int resSel = 0;
    bool found = false;

    for (int i = 0; i < hmd.resolutionCount; ++i) {
        SendMessageW(g_hRes, CB_ADDSTRING, 0, (LPARAM)hmd.resolutions[i].label);
        if (!found &&
            hmd.resolutions[i].width == currentWidth &&
            hmd.resolutions[i].height == currentHeight) {
            resSel = i;
            found = true;
        }
    }

    // Se la risoluzione corrente non è nella lista del nuovo HMD,
    // seleziona la prima (quella nativa).
    if (!found && hmd.resolutionCount > 0) {
        resSel = 0;
    }

    SendMessageW(g_hRes, CB_SETCURSEL, resSel, 0);
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

        // --- VR Headset (NUOVO) ---
        MakeLabel(hwnd, L"VR Headset", kMargin, y, kFieldW, 24, g_fontBody);
        y += 26;
        g_hHmd = MakeCombo(hwnd, ID_COMBO_HMD, kMargin, y, kFieldW);
        const int currentHmd = GetCurrentHmdType();
        int hmdSel = 0;
        for (int i = 0; i < static_cast<int>(std::size(kHmdPresets)); ++i) {
            SendMessageW(g_hHmd, CB_ADDSTRING, 0, (LPARAM)kHmdPresets[i].label);
            if (kHmdPresets[i].mhdType == currentHmd) {
                hmdSel = i;
            }
        }
        SendMessageW(g_hHmd, CB_SETCURSEL, hmdSel, 0);
        y += 50;

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

        // -- Render Resolution ---
        MakeLabel(hwnd, L"Render Resolution (per eye)", kMargin, y, kFieldW, 24, g_fontBody);
        y += 26;
        g_hRes = MakeCombo(hwnd, ID_COMBO_RES, kMargin, y, kFieldW);
        PopulateResolutionCombo(hmdSel);   // <-- Popola in base all'HMD selezionato
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
        const int notif = HIWORD(wParam);
        const int id    = LOWORD(wParam);

        // --- Cambio selezione HMD: aggiorna la lista risoluzioni ---
        if (id == ID_COMBO_HMD && notif == CBN_SELCHANGE) {
            const int hmdIdx = (int)SendMessageW(g_hHmd, CB_GETCURSEL, 0, 0);
            PopulateResolutionCombo(hmdIdx);
            break;
        }

        if (id == ID_BUTTON_START) {
            // Salva la selezione HMD
            int hmdIdx = (int)SendMessageW(g_hHmd, CB_GETCURSEL, 0, 0);
            if (hmdIdx >= 0 && hmdIdx < static_cast<int>(std::size(kHmdPresets))) {
                SetHmdTypeAndPersist(kHmdPresets[hmdIdx].mhdType);
            }

            int rIdx = (int)SendMessageW(g_hRuntime, CB_GETCURSEL, 0, 0);
            if (rIdx >= 0 && rIdx < static_cast<int>(std::size(kRuntimeOptions))) {
                SetRuntimeModeAndPersist(kRuntimeOptions[rIdx].mode);
            }

            int idx = (int)SendMessageW(g_hRes, CB_GETCURSEL, 0, 0);
            if (idx >= 0) {
                // Recupera l'HMD corrente per leggere la sua lista risoluzioni
                int curHmdIdx = (int)SendMessageW(g_hHmd, CB_GETCURSEL, 0, 0);
                if (curHmdIdx >= 0 && curHmdIdx < static_cast<int>(std::size(kHmdPresets))) {
                    const HmdPreset& hmd = kHmdPresets[curHmdIdx];
                    if (idx < hmd.resolutionCount) {
                        SetWindowResolutionAndPersist(
                            hmd.resolutions[idx].width,
                            hmd.resolutions[idx].height);
                    }
                }
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