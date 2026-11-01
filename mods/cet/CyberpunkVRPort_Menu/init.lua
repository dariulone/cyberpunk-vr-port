-- CyberpunkVRPort_Menu -- the Sword Art Online drop-down menu, in Night City's colours.
-- Holographic theme: transparent dark glass, cut-corner cyan frames and amber focus rings.
-- Matte world backplates also support the icon column. Finger markers pulse on accepted clicks;
-- primary icon segments sweep briefly and the game's HUD fades out/in with the world menu.
--
-- SUMMON: the right hand, grip held, swept straight down. The same swipe
-- closes it. The column of round icons drops in where the hand went, at arm's reach:
--
--   x         close
--   person    tabs: equipment (default), attributes/skills/perks, cyberware; Trade during a shop visit
--             Stash during an interaction with a real storage container
--             opens the status panel on the left and the selected tab on the right
--   map       closes this menu and opens the game's original world map
--   !         tabs: quests (default) and tarot
--   database  discovered codex entries and illustrations
--   SHARDS    from Journal / Database: acquired chips, native read flags and encrypted-item actions
--   phone     contacts  -> a contact -> CALL / MESSAGES
--   car       vehicles  -> a vehicle -> summoned
--   radio     stations, on / off, volume
--   clock     WAIT: circular 24-hour dial, day/night preview and a native two-eye blink
--   gamepad   the pause menu's entries
--             Load: native save metadata/thumbnails grouped by playthrough, with confirmation
--             and a modded-save warning before the engine loads the freshly validated slot
--   gear      the port's VR settings: the ImGui panel's pages, read out by the plugin (modules/settings.lua)
--
-- Long lists scroll: dragged with the finger, or with the pointer on a held trigger.
-- Hold the right thumbstick button (R3) and move the hand to grab a list without clicking items.
-- Releasing R3 leaves scroll inertia; L3 + R3 still closes the menu.
--
-- TOUCH IT OR POINT AT IT. Either index fingertip presses what it pushes into (crossing the screen from
-- the front), and either hand's pointer clicks on a trigger pull. The beam shows only while a hand points
-- at the menu and is not already close enough to touch.
-- The icon column drops into the centre, then slides left when a submenu opens. Cursor and beam
-- appear only over visible controls (round icons use their actual circles). Only clicks vibrate.
-- Money and carried/max weight stay beside the close ring in every section.
--
-- WHAT IT IS. A plane in the world with a game ink screen on it. The native MenuSurface allocator owns
-- its full-resolution ink texture independently of the shared world-widget atlas. The
-- archive holds vrport\ui\vr_menu.ent (the engine's ad plane, the wrist's additive screen material and a
-- WorldWidgetComponent drawing an empty 3200 x 1600 canvas, 1.00 x 0.50 m) and vr_menu_beam.ent (the same
-- at 4 mm x 1.5 m). Everything on them is built here from ink widgets. The plane stays where it opened --
-- in the car's frame while mounted -- and closes when the player walks away from it.
--
-- WHAT THE PLUGIN DOES. Script cannot see the hands, so the natives (src/Natives/VrMenu.cpp) report the
-- swipe from the tracking history and, every frame, where each rendered fingertip and pointer meet the
-- plane. VRMenuCapture holds the triggers, grips and face buttons back from the game while the menu is
-- open; sticks keep walking and physical controls still animate the fingers. VRMenuSurfaceReady
-- reveals the disabled display only after a post-build ink write has finished on its GPU queue.
--
-- ORIENTATION, measured: the component's own rotation is ignored, so the entity carries it. With the
-- entity's local X = canvas LEFT, local Y = canvas DOWN and local Z = toward the viewer, the canvas reads
-- upright and unmirrored (quatFromBasis below; at a zero yaw that is qx(90) * qz(180)).
--
-- THE FILES. This one only wires CET's events; modules/menu.lua is the menu, modules/ink.lua the drawing kit,
-- modules/data.lua the phone/vehicle/radio calls, modules/gameplay.lua and gameplay_data.lua the
-- character/journal/codex/time panels, modules/scroll.lua the scrolling list, modules/settings.lua the settings.
-- reference_data.lua / reference_view.lua adapt the native attribute trees, Bink previews, skill
-- rewards, quest distances and tarot records. The summary appears only with the person group.
-- perk_layout.lua stores original ink slot positions; journal_view.lua keeps quest links beside their
-- quest, library_data.lua uses JournalManager for chip reads, summary_view.lua owns the opaque backing.
-- inventory_data.lua queues native loadout/part requests without drawing a weapon; inventory_view.lua
-- renders the item grids and native availability/new/comparison indicators. inventory_preview.lua
-- defaults to static male/female art, and leases the game's live paperdoll only when enabled.
-- Both previews focus on the chosen clothing area; preferences.lua persists the toggle separately.
-- cyberware_data.lua uses native requirements, shop purchases and seeded upgrade choices.
-- vendor_visit.lua intercepts the real vendor request before fullscreen widgets are created, opening
-- Cyberware at a ripperdoc and Trade at other sellers. The lease ends on close/leaving the shop;
-- tutorial visits retain the game's scripted workflow. trade_data/view.lua use native prices,
-- quantity limits, protected-item rules, buyback and comparison tooltips without changing saves directly.
-- VendorRoute.cpp retains native scene requests at the UI queue before menu mode starts. Lua
-- validates/accepts the real panel data; rejected or unacknowledged requests return to the engine.
-- World visits publish the native UI_Vendor open/close signals so quest dialogue is rearmed.
-- Trade snapshots and sorting advance in bounded steps while the loading panel stays responsive.
-- The hidden-hub path below remains a compatibility fallback when that native hook is unavailable.
-- Scene-driven shops initialize a transparent native hub and close its own menu instance through
-- OnVendorClose. The whole native menu layer is suppressed before entry, including its separate
-- background/cursor, then restored after teardown (also on cancellation, timeout and reload).
-- The world menu opens only after hub teardown and exit from the engine's menu mode.
-- item_sort.lua provides the game's sorting choices for inventory, trade and stash grids.
-- stash_visit/data/view.lua route native storage interactions, with independent filters/sorting,
-- stack quantities and equipped-item confirmation; VendorDataManager transfers original items.
-- cyberware_anatomy.lua aligns the original male/female organ layers and animates the focus.
-- modules/wheel.lua is the weapon wheel on a held Y (the game's radial wheel without the pause); it shares
-- the menu's screen asset and drawing kit, and stands down while the menu is open.
-- For development, GetMod("CyberpunkVRPort_Menu").hot() reads modules/
-- again off disk without a restart (the Reload mod's loader: CET hides `package`, so each file is compiled
-- with its own `require` in front of it).

local Menu = require("modules/menu")
local Wheel = require("modules/wheel")
local shared = { menuController = nil }

local function hotLoad()
  local cache = {}
  local function req(name)
    local v = cache[name]
    if v ~= nil then return v end
    local f = io.open(name .. ".lua", "r")
    if not f then error("hot reload: cannot open " .. name .. ".lua") end
    local src = f:read("*a")
    f:close()
    local chunk, err = loadstring("local require = ...; " .. src, "@" .. name .. ".lua")
    if not chunk then error(err) end
    v = chunk(req)
    if v == nil then v = true end
    cache[name] = v
    return v
  end
  return req("modules/menu"), req("modules/wheel")
end

registerForEvent("onInit", function()
  -- The engine glitches any world screen whose owner is neither a Device nor a Vehicle (the wrist
  -- readout's finding); the menu's entity is neither.
  pcall(function()
    Override("IWorldWidgetComponent", "StartGlitching", function(self, intensity, lifetime, wrapped) end)
  end)
  -- The hub opens through the in-game menu controller, which is made with each session.
  pcall(function()
    ObserveAfter("gameuiInGameMenuGameController", "OnInitialize", function(self) shared.menuController = self end)
  end)
  Menu.init(shared)
  print("[VRMenu] ready")
end)

registerForEvent("onUpdate", function(dt)
  Menu.update(dt)
  Wheel.update(dt, Menu.isOpen())
end)

registerForEvent("onShutdown", function()
  Wheel.release()
  Menu.release()
end)

-- GetMod("CyberpunkVRPort_Menu"): the menu's own calls (open, close, state, section, click), wheel() for the
-- weapon wheel's state, and hot().
return setmetatable({
  hot = function()
    local ok, new, newWheel = pcall(hotLoad)
    if not ok then return "hot reload failed: " .. tostring(new) end
    pcall(Wheel.release)
    pcall(Menu.release)
    Menu, Wheel = new, newWheel
    Menu.init(shared)
    return "ok"
  end,
  wheel = function() return Wheel.state() end,
}, { __index = function(_, k) return Menu[k] end })
