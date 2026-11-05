#pragma once

// The overlay key's action: the host's overlay opens or closes (src/Lua/Host.cpp).
void ToggleOverlay();

// CET's bindings widget (src/overlay/widgets/Bindings.h) as far as VKBindings and ScriptStore ask: the bind that
// toggles the overlay, "cet.overlay_key" in config\lua_bindings.json (never CET's own key: src/Lua/Migration.hpp).
struct Bindings
{
    static const VKModBind& GetOverlayToggleModBind() noexcept
    {
        static const VKModBind s_bind{"cet", "overlay_key"};
        return s_bind;
    }

    static const VKBind& GetOverlayToggleBind() noexcept
    {
        static const VKBind s_bind{"overlay_key", "Overlay Key", "Opens the Lua mods' windows.", [] { ToggleOverlay(); }};
        return s_bind;
    }
};
