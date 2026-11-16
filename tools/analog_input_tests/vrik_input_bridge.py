"""Exercise the real VRIK update with either mouse-Y setting, without the game."""
from pathlib import Path
from lupa import LuaRuntime

source = (Path(__file__).resolve().parents[2] /
          "mods/cet/CyberpunkVRPort_VRIK/init.lua").read_text(encoding="utf-8")

for native in (False, True):
    lua = LuaRuntime(unpack_returned_tuples=True)
    lua.globals().native_bridge = native
    lua.execute("""
        events = {}; pumps = 0; fileReads = 0; disabled = false
        function registerForEvent(name, fn) events[name] = fn end
        function registerHotkey() end
        function Observe() end
        function UpdateVRIKAnimInputs() pumps = pumps + 1 end
        function GetVRMouseYDisabled() return disabled end
        if not native_bridge then GetVRMouseYDisabled = nil end
        io.open = function(name, mode)
            assert(name == 'vrik_settings.ini' and mode == 'r')
            fileReads = fileReads + 1
            return {
                read = function() return 'disable_mouse_y=' .. (disabled and '1' or '0') .. '\\n' end,
                close = function() end
            }
        end
        -- Stop after the native input/pose pump, before unrelated gizmo math.
        cam = { GetLocalToWorld = function() return nil end }
        player = { GetFPPCameraComponent = function() return cam end }
        Game = { GetPlayer = function() return player end }
        AnimationControllerComponent = { SetInputFloat = function() end }
    """)
    lua.execute(source)
    lua.execute("""
        events.onInit()
        for _, value in ipairs({false, true, false}) do
            disabled = value
            local before = pumps
            events.onUpdate(.25)
            assert(pumps == before + 1, 'mouse-Y flag gated the VRIK input pump')
            assert(cam.pitchMin == (value and 0 or -80))
            assert(cam.pitchMax == (value and 0 or 80))
        end
        assert(fileReads == (native_bridge and 0 or 3), 'native/fallback bridge precedence changed')
    """)
    print(f"PASS VRIK mouse-Y 0/1/0: {'native' if native else 'file fallback'} bridge; input pump always runs")
