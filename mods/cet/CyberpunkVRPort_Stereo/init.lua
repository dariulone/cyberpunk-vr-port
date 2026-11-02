-- CyberpunkVRPort_Stereo — script-side VRCAM activation and HUD source publication.
--
-- Game objects are accessed only after CET onInit. Binding registration belongs
-- to module loading, when CET exposes registerHotkey.
-- VRCAM activation switches an entity component on.
-- entRenderToTextureCameraComponent.isEnabled is only reachable through the game's RTTI, which is
-- script-side, so the plugin asks and this mod does it. Everything else about the second view --
-- the view key, the render graph, the camera, the submit -- is native.
--
-- Files:
--   vrcam.json            which component to enable, and the full authored catalogue.
--                         Written by the launcher, read by both sides.
--   bridge/vrcam_enable.txt   native -> here: turn VRCAM on or off.
--   bridge/vrcam_active.txt   here -> native: the virtualCameraName actually enabled.
--
-- What used to live here and is gone: the testbed entity spawner, the file-IPC command channel to
-- the old dxgi.dll proxy, the RTT steer/capture harness and the ImGui overlay. The proxy they
-- talked to no longer exists, and the in-game overlay is drawn by the plugin now.

local VrcamSel = require("modules/vrcam_select")
local HudPanel = require("modules/hud_panel")
local VrOverlay = require("modules/vr_overlay")
local KeypadInput = require("modules/keypad_input")
local StoryAttention = require("modules/story_attention")
local WorldTouch = require("modules/world_touch")
local InteractionDepth = require("modules/interaction_depth")
local HudClearance = require("modules/hud_clearance")

local Stereo = { ready = false, VrcamSel = VrcamSel, HudPanel = HudPanel, VrOverlay = VrOverlay, KeypadInput = KeypadInput, StoryAttention = StoryAttention, WorldTouch = WorldTouch }

-- Bind in CET > Bindings if vrcam.json was edited by hand.
registerHotkey("vrcam_reload_selection", "VRCAM: re-read vrcam.json", function()
    VrcamSel.reload()
    print("[Stereo.VRCAM] " .. VrcamSel.status())
end)

-- RT reflections are traced at full resolution. The engine traces them at half the render resolution, enough on a
-- monitor; an eye spreads its pixels over a much wider field, so half-resolution reflections look soft next to the
-- SSR ones (rainy street, 2026-10-16: road detail 11.5 -> 11.9, about 1.5 of 35 FPS, only with RT reflections on).
-- DLSS Ray Reconstruction sharpens them far more (11 -> 16) and works per eye. false keeps the engine's half resolution.
Stereo.rtFullResReflections = true

registerForEvent("onInit", function()
    VrcamSel.init()
    Stereo.ready = true
    print("[Stereo] " .. VrcamSel.status())
    if Stereo.rtFullResReflections then
        GameOptions.SetInt("RayTracing/Reflection", "EnableHalfResolutionTracing", 0)
    end
end)

-- CET'S GARBAGE COLLECTOR IS RUN FROM HERE, for every mod at once: they all share one Lua state.
--
-- LuaJIT's own trigger is switched off, because it was the periodic hitch. The finalize phase
-- lowers LuaJIT's live-heap estimate by a fixed cost per finalized object, and CET finalizes a
-- userdata for every game object a mod touched, so the next threshold (twice the estimate) landed
-- below the heap: a whole new cycle started with a 1.5-2.6 MB debt and swept 2-5 MB inside one
-- frame -- a 13-15 ms GameThread stall about once a second. Instead every frame takes a step as large
-- as what the mods allocated since the last one (at least 16 KB), so a cycle is always spread over
-- frames and the collector keeps pace with the allocation rather than running ahead of it. In the
-- city the mods allocate ~40 KB per frame; a fixed 64 KB step cost 1.01 ms of this onUpdate against
-- 0.83 ms for 16 KB, i.e. the GC runs ~10 us per KB stepped (2026-10-17).
--
-- No rest between cycles by default. The atomic phase is not incremental and walks every userdata
-- made since the last cycle, so a rest grows it: measured in the city, rest 0 -> atomic ~3.3 ms,
-- 1.05 -> 4.4, 1.2 -> 7.9, 1.35 -> 11.5, a 200-frame rest -> 17 ms. Frame rate was the same within
-- noise for all of them; rest 0 gave the flattest frame times. restGrowth > 1 rests until the heap
-- has grown by that factor -- the knob for a scene where the game thread, not the GPU, is the limit.
-- Stereo.gc is this mod's table, so it can be tuned live: GetMod('CyberpunkVRPort_Stereo').gc.
--
-- IN THE MENUS THE STEP IS LARGER (menuStepKB): there the GC is not paced to the allocation but runs well ahead
-- of it. A cycle paced to the allocation leaves as much garbage behind per cycle whatever the rate, and the main
-- menu's is userdata (component references, names, vectors): ~800k of them per cycle on a ~150 MB heap, which
-- the atomic phase walks -- a 100-140 ms freeze every 20-30 s, the menu's periodic freeze (2026-10-28, the
-- lobby: stepKB 16 -> atomic 83-138 ms; 96 -> 27-30 ms every 7 s; 192 -> 12-15 ms every 3.5 s, frame median
-- 11.13 -> 11.25 ms at 90 Hz). The menu is held at the headset's rate, not the game thread's, so the extra
-- collector time costs no frames there; in the game the city is CPU-bound and keeps the paced step.
--
-- `stop` holds only until the next step re-arms the trigger, so it is set again after each one.
-- onUpdate runs in the menus and on loading screens too, and a mod reload builds a new Lua state.
Stereo.gc = { stepKB = 16, stepMaxKB = 1024, restGrowth = 0, menuStepKB = 192 }
local gcResumeAt, gcLastKB = nil, nil

local function paceGc()
    -- IN THE GAME THE PLUGIN'S LUA HOST PACES IT (src/Lua/HostGc.cpp): its steps run while the game thread waits
    -- for the world tick, off the frame's path, and only the finalizers run before onUpdate. The menus keep the
    -- pacing below (VRLuaHostGc(false)).
    if type(VRLuaHostGc) == "function" and VRLuaHostGc(not VrcamSel.preGame) then
        gcLastKB, gcResumeAt = nil, nil
        return
    end
    local cfg = Stereo.gc
    local heap = collectgarbage("count")
    local grown = gcLastKB and heap - gcLastKB or 0
    gcLastKB = heap
    if gcResumeAt then
        if heap < gcResumeAt then collectgarbage("stop") return end
        gcResumeAt = nil
    end
    -- The menu as the VRCAM selector saw it last frame (it asks the game every frame).
    local step = VrcamSel.preGame and cfg.menuStepKB or cfg.stepKB
    if grown > step then step = math.min(grown, cfg.stepMaxKB) end
    local finished = collectgarbage("step", step)
    collectgarbage("stop")
    if finished and cfg.restGrowth > 1.0 then
        gcLastKB = collectgarbage("count")
        gcResumeAt = gcLastKB * cfg.restGrowth
    end
end

registerForEvent("onUpdate", function(dt)
    paceGc()
    if not Stereo.ready then return end
    VrcamSel.tick(dt)
    HudPanel.tick()
    VrOverlay.tick()
    KeypadInput.tick(dt, VrOverlay.active)
    WorldTouch.tick(dt, VrOverlay.active)
    StoryAttention.tick(dt, VrOverlay.active)
    InteractionDepth.tick(dt, VrOverlay.active)
    HudClearance.tick(dt, VrOverlay.active)
end)

registerForEvent("onShutdown", function() WorldTouch.shutdown();StoryAttention.shutdown();KeypadInput.shutdown();VrOverlay.shutdown();HudPanel.shutdown() end)

return Stereo
