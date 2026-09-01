-- Enable exactly ONE authored VRCAM component, chosen by render resolution.
--
-- The player entity carries one entRenderToTextureCameraComponent per resolution
-- (vrcam_<W>x<H>, virtualCameraName vrcam_feed_<W>x<H>), ALL authored with isEnabled=0.
-- The launcher writes the picked one into vrcam.json; this module switches it on through the
-- game's own component API and switches every sibling off.
--
-- Selection is by NAME only. Nothing here inspects aspect ratio or resolution numbers, so
-- adding a resolution means adding a component plus a catalogue entry -- no code change.
--
-- The native plugin reads the same vrcam.json to derive the view key (CName hash of the
-- virtualCameraName). Both sides must agree, which is why there is a single file and it lives
-- here: CET sandboxes Lua file IO to the mod folder, so this side cannot reach out, while the
-- native side can and does.

local VrcamSelect = {
    selected     = nil,     -- component name from vrcam.json
    want         = true,    -- false once the overlay asks for VRCAM off
    appliedName  = nil,     -- what we last actually applied (nil = nothing applied yet)
    appliedWant  = nil,
    appliedId    = nil,     -- ...and to WHICH entity, because the player can be replaced under us
    lastError    = nil,
    ticksToRetry = 0,
    enabledCount = 0,
    seenCount    = 0,
    prevPlayer   = nil,     -- the entity we last applied to, so it can be let go of
    prevId       = nil,
    onBdReplacer = false,   -- the live player is the braindance replacer
}

local RETRY_TICKS = 30      -- ~150 ms @ 200 fps; the player is not there during load
local NAME_PREFIX = "vrcam_"
-- THE BRAINDANCE REPLACER'S OWN SET. A braindance swaps the player for braindance_replacer.ent, so the
-- same components are authored there under this prefix -- same virtualCameraName, same dtex, so nothing
-- downstream can tell which of the two is live, which is the point.
local BD_PREFIX = "vrcam_braindance_"

local function safeCall(obj, methodName, ...)
    if obj == nil then return false, "nil obj" end
    local m = obj[methodName]
    if m == nil then return false, "method missing" end
    local ok, ret = pcall(m, obj, ...)
    if not ok then return false, "throw: " .. tostring(ret) end
    return true, ret
end

-- CName reads back differently across CET builds (userdata with .value, or tostring-able).
local function cnameToString(v)
    if v == nil then return nil end
    local ok, s = pcall(function() return v.value end)
    if ok and type(s) == "string" and s ~= "" then return s end
    local ok2, s2 = pcall(function() return tostring(v) end)
    if ok2 and type(s2) == "string" and s2 ~= "" then return s2 end
    return nil
end

-- Read "component" out of the flat JSON. A pattern rather than a JSON library: the file is two
-- fields, and the "_comment" block is an array so it cannot match a "key": "value" pair.
local function readSelection()
    local f = io.open("vrcam.json", "r")
    if not f then return nil, "vrcam.json not found in the mod folder" end
    local body = f:read("*a")
    f:close()
    if not body or body == "" then return nil, "vrcam.json is empty" end
    local name = body:match('"component"%s*:%s*"([^"]+)"')
    if not name then return nil, 'vrcam.json has no "component" field' end
    return name, nil
end

-- The overlay's Disable/Enable VRCAM button writes this. Component isEnabled is only reachable
-- through the game's RTTI, i.e. from here, so native asks and this side does it.
local function readEnableRequest()
    local f = io.open("bridge/vrcam_enable.txt", "r")
    if not f then return nil end
    local body = f:read("*a")
    f:close()
    if not body then return nil end
    if body:find("0", 1, true) then return false end
    if body:find("1", 1, true) then return true end
    return nil
end

function VrcamSelect.reload()
    local name, err = readSelection()
    VrcamSelect.selected    = name
    VrcamSelect.lastError   = err
    VrcamSelect.appliedName = nil     -- force one re-apply after a manual reload
    VrcamSelect.appliedWant = nil
    if err then
        print("[Stereo.VRCAM] " .. err .. " -- no component will be enabled")
    else
        print("[Stereo.VRCAM] selection: " .. name)
    end
    return name
end

-- Flip one component. Toggle() is the engine's own enable/disable entry point; the direct field
-- write is a fallback for builds where it is not exposed to scripts.
local function setEnabled(comp, want)
    local ok = safeCall(comp, "Toggle", want)
    if ok then return true, "Toggle" end
    local okSet = pcall(function() comp.isEnabled = want end)
    if okSet then return true, "isEnabled" end
    return false, "no way to toggle"
end

-- Turn every vrcam_* off on an entity we are walking away from. Two enabled render-to-texture
-- cameras share one feed, and that is the flicker: the parked entity's transform lands in the same
-- place ours does, every other frame.
local function releaseEntity(ent)
    if ent == nil then return 0 end
    local okL, comps = safeCall(ent, "GetComponents")
    if not okL or not comps then return 0 end
    local n = 0
    pcall(function() n = #comps end)
    local off = 0
    for i = 1, n do
        local c = comps[i]
        local nm = c and cnameToString(c.name) or nil
        if nm and nm:sub(1, #NAME_PREFIX) == NAME_PREFIX then
            local ok = safeCall(c, "Toggle", false)
            if not ok then pcall(function() c.isEnabled = false end) end
            off = off + 1
        end
    end
    return off
end

local function apply(player, wanted, want_on)
    if not wanted then return false end

    local okL, comps = safeCall(player, "GetComponents")
    if not okL or not comps then
        VrcamSelect.lastError = "player:GetComponents() unavailable"
        return false
    end
    local n = 0
    pcall(function() n = #comps end)
    if n == 0 then
        VrcamSelect.lastError = "player has no components yet"
        return false
    end

    -- WHICH OF THE TWO SETS THIS ENTITY CARRIES. The replacer's components differ only in name, so the
    -- resolution suffix is what is kept and the prefix is what is chosen.
    local suffix = wanted:sub(#NAME_PREFIX + 1)
    local wantedBd = BD_PREFIX .. suffix
    local hasBd = false
    for i = 1, n do
        local c = comps[i]
        local nm = c and cnameToString(c.name) or nil
        if nm == wantedBd then hasBd = true break end
    end
    if hasBd then wanted = wantedBd end
    VrcamSelect.onBdReplacer = hasBd

    -- ...AND LET GO OF THE ENTITY WE CAME FROM, once, on the frame the player changes.
    local id = nil
    pcall(function() id = tostring(player:GetEntityID().hash) end)
    if id ~= nil and VrcamSelect.prevId ~= nil and id ~= VrcamSelect.prevId then
        local off = releaseEntity(VrcamSelect.prevPlayer)
        print(string.format("[Stereo.VRCAM] player changed (%s -> %s): %d component(s) released",
                            tostring(VrcamSelect.prevId), tostring(id), off))
    end
    VrcamSelect.prevPlayer, VrcamSelect.prevId = player, id

    local seen, enabled, disabled, found, how = 0, 0, 0, false, nil
    local enabledComp = nil
    for i = 1, n do
        local c = comps[i]
        local nm = c and cnameToString(c.name) or nil
        if nm and nm:sub(1, #NAME_PREFIX) == NAME_PREFIX then
            seen = seen + 1
            local want = want_on and (nm == wanted)
            local ok, via = setEnabled(c, want)
            if ok then
                if want then found, enabled, how, enabledComp = true, enabled + 1, via, c
                else disabled = disabled + 1 end
            else
                VrcamSelect.lastError = "cannot toggle " .. nm .. ": " .. tostring(via)
            end
        end
    end

    VrcamSelect.seenCount    = seen
    VrcamSelect.enabledCount = enabled

    if seen == 0 then
        -- Do not retry forever on an entity that simply has no VRCAM components authored.
        VrcamSelect.lastError = "no " .. NAME_PREFIX .. "* components on the player entity"
        return false
    end
    if want_on and not found then
        VrcamSelect.lastError = string.format(
            "%s not found among %d %s* component(s) -- all left disabled", wanted, seen, NAME_PREFIX)
        print("[Stereo.VRCAM] " .. VrcamSelect.lastError)
        return true      -- the entity was read fine; retrying will not change the outcome
    end

    VrcamSelect.lastError = nil
    -- Hand the native side the ACTUAL virtualCameraName of the component we just enabled.
    -- The render path identifies the VRCAM view by the CName hash of that name, and deriving
    -- it from the component name assumes the asset follows vrcam_feed_<suffix>. When it does
    -- not (a component renamed in WolvenKit while its camera field kept the old value) the
    -- key matches no view and everything downstream goes silent -- no stereo, no mirror --
    -- while every log line still names the right component. Reading it back off the live
    -- component removes that failure mode: the asset itself is the source of truth.
    local activeCam = ""
    if want_on and enabledComp then
        activeCam = cnameToString(enabledComp.virtualCameraName) or ""
    end
    local wf = io.open("bridge/vrcam_active.txt", "w")
    if wf then wf:write(activeCam); wf:close() end
    VrcamSelect.activeCamera = activeCam

    if want_on then
        print(string.format("[Stereo.VRCAM] enabled %s (camera %s) via %s (1 of %d, %d disabled)",
                            wanted, activeCam ~= "" and activeCam or "?", tostring(how),
                            seen, disabled))
        local expect = "vrcam_feed_" .. wanted:sub(#NAME_PREFIX + 1)
        if activeCam ~= "" and activeCam ~= expect then
            print(string.format(
                "[Stereo.VRCAM] %s has virtualCameraName=%s (expected %s) -- native will use the real one",
                wanted, activeCam, expect))
        end
    else
        print(string.format("[Stereo.VRCAM] VRCAM off (%d component(s) disabled)", disabled))
    end
    return true
end

function VrcamSelect.init()
    VrcamSelect.reload()
end

function VrcamSelect.tick(dt)
    local player = Game.GetPlayer and Game.GetPlayer() or nil
    if not player then
        -- Menu / loading: re-arm so the next session applies again.
        VrcamSelect.appliedName = nil
        VrcamSelect.appliedWant = nil
        return
    end
    if VrcamSelect.ticksToRetry > 0 then
        VrcamSelect.ticksToRetry = VrcamSelect.ticksToRetry - 1
        return
    end
    VrcamSelect.ticksToRetry = RETRY_TICKS

    local req = readEnableRequest()
    if req ~= nil then VrcamSelect.want = req end

    -- Re-apply only when the DESIRED state changed. The old guard compared tostring(player),
    -- but CET hands out a fresh wrapper on every Game.GetPlayer() call, so that string differed
    -- every time and the module re-applied (and logged) a few times a second.
    --
    -- ...AND WHEN THE PLAYER ITSELF CHANGES, which is the case this guard used to miss entirely. A
    -- braindance swaps the player for braindance_replacer.ent, a whole entity of its own carrying its
    -- own set of these components. The desired NAME does not change across that swap, so keying on the
    -- name alone left the replacer's cameras disabled and the parked original's enabled -- measured: one
    -- eye in the recording, the other looking from 2.8 km away. The entity id is what changes, so it is
    -- part of the key. It is read off the game object rather than the wrapper, so it is stable.
    local id = nil
    pcall(function() id = tostring(player:GetEntityID().hash) end)
    if VrcamSelect.appliedName == VrcamSelect.selected and
       VrcamSelect.appliedWant == VrcamSelect.want and
       VrcamSelect.appliedId == id then
        return
    end

    if apply(player, VrcamSelect.selected, VrcamSelect.want) then
        VrcamSelect.appliedName = VrcamSelect.selected
        VrcamSelect.appliedWant = VrcamSelect.want
        VrcamSelect.appliedId   = id
    end
end

function VrcamSelect.status()
    return string.format("selected=%s want=%s applied=%s vrcamComps=%d enabled=%d%s",
        tostring(VrcamSelect.selected), tostring(VrcamSelect.want),
        tostring(VrcamSelect.appliedName), VrcamSelect.seenCount, VrcamSelect.enabledCount,
        VrcamSelect.lastError and (" err=" .. VrcamSelect.lastError) or "")
end

return VrcamSelect
