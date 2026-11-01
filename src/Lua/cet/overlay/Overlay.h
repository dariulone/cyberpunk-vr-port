#pragma once

// CET's overlay (src/overlay/Overlay.h) as far as the scripting core asks: whether it is open. Here it is the Lua
// mods' windows in the plugin's ImGui (src/Lua/Host.cpp): its key toggles it, opening and closing fire
// onOverlayOpen / onOverlayClose, and while it is open the windows are drawn and take the mouse and keyboard.
#include "widgets/Bindings.h"

struct Overlay
{
    [[nodiscard]] bool IsEnabled() const noexcept { return m_enabled; }
    void Toggle(); // src/Lua/Host.cpp

private:
    friend void SetOverlayEnabled(Overlay& aOverlay, bool aEnabled) noexcept;
    bool m_enabled{false};
};

inline void SetOverlayEnabled(Overlay& aOverlay, bool aEnabled) noexcept
{
    aOverlay.m_enabled = aEnabled;
}
