#pragma once
// THE GAME'S WINDOW AS AN ORDINARY WINDOW (xr_desktop_window=1 in vrport.ini; 0, the default, keeps the window the
// port fits to its monitor -- Render/DesktopMirror.cpp).
//
// The game draws into a virtual client of the render's size (GetClientRect is hooked, Hooks/OsInput.cpp), and the
// desktop shows the centre crop of it at whatever shape the real window has (DesktopMirrorRender), with the cursor
// mapped through the same crop. So nothing needs the window at any one size, and in this mode the port stops
// placing it: the game starts in its own Windowed mode (Core/FirstLaunch.cpp), the window gets a resizable frame,
// the player moves, resizes and maximises it, and its placement is remembered in vrport.ini
// (xr_desktop_window_rect, xr_desktop_maximized). Alt+Enter switches it to borderless fullscreen on its monitor and
// back (xr_desktop_fullscreen, remembered too). The game's own requests to move or size it are ignored.

#include <windows.h>

namespace cvr::desktop_window {

// xr_desktop_window=1.
bool Free();

// The free window, not fullscreen: the cursor is never confined to it -- neither by the port (UpdateCursorCapture)
// nor by the game's own ClipCursor -- so its frame can be grabbed and other windows reached. Fullscreen confines it
// as the fitted window does.
bool ReleasesCursor();

// From the present path, with the swapchain's window: the first time a window is seen, asks its own thread to set it
// up (frame, remembered placement, fullscreen) -- a posted message, so no window call crosses threads.
void OnPresent(HWND hwnd);

// From the game window's WndProc (the overlay's subclass, Overlay/ImGuiOverlay.cpp), before anything else. True when
// the message was handled here and must go no further, with *result its answer.
bool OnMessage(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam, LRESULT* result);

} // namespace cvr::desktop_window
