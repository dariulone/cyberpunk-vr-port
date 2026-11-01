#pragma once

// In CyberEngineTweaks this is the program: the singleton that owns its paths, options, key bindings, D3D12 hooks,
// overlay and Lua VM. Here the plugin's Lua host owns them (src/Lua/Host.cpp) and answers CET::Get() for the ported
// scripting core.
#include "Options.h"
#include "Paths.h"
#include "VKBindings.h"
#include "d3d12/D3D12.h"
#include "overlay/Overlay.h"
#include "scripting/LuaVM.h"

struct CET
{
    static CET& Get();

    const Paths& GetPaths() const noexcept;
    const Options& GetOptions() const noexcept;
    D3D12& GetD3D12() noexcept;
    VKBindings& GetBindings() noexcept;
    Overlay& GetOverlay() noexcept;
    LuaVM& GetVM() noexcept;

    static bool IsRunning() noexcept;
};
