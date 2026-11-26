-- One-run probe: the settings pages as the VR menu reads them (VRSettingsPage, Overlay/UiRec.hpp), one page at a time:
-- requested, then read back two seconds later (the record pass runs on the next present), printed with the
-- separators shown. Then the pages behind tabs and headers, opened through the menu's own edits (VRSettingsEdit),
-- and put back.
local waited = 0
local step = 1
local requestedAt = nil
local function shown(s)
    s = s:gsub('\30', ' || '):gsub('\31', ' | '):gsub('\29', ' ; '):gsub('\r', '\\r'):gsub('\n', '\\n')
    return s
end
-- {label, page, edit id or nil, edit value}
local steps = {
    {'0', 0}, {'1', 1}, {'2', 2}, {'3', 3}, {'4', 4}, {'5', 5}, {'6', 6},
    {'3-driving', 3, '3/controls-pages', 1},
    {'3-bindings', 3, '3/controls-pages', 2},
    {'3-back', 3, '3/controls-pages', 0},
    {'3-lefthanded', 3, '3/Left-handed controls', 1},
    {'3-righthanded', 3, '3/Left-handed controls', 0},
    {'4-osr', 4, "4/OSR (the port's other-eye reconstruction)", 1},
    {'4-osr-off', 4, "4/OSR (the port's other-eye reconstruction)", 0},
    {'4-pacing-off', 4, '4/Stable pacing with no overlay graphics', 0},
    {'4-pacing-on', 4, '4/Stable pacing with no overlay graphics', 1},
    {'6-center', 6, '6/CENTER PANEL', 1},
    {'6-reset', 6, '6/RESET', 1},
    {'2-health', 2, '2/Element', 0},
    {'2-quest', 2, '2/Element', 14},
    {'2-back', 2, '2/Element', 10},
    {'3-bindings-end', 3, '3/controls-pages', 2},
}
local menuAt = nil
registerForEvent('onUpdate', function(delta)
    if not Game.GetPlayer() then return end
    if step > #steps then
        -- Then the VR menu itself for 40 seconds: its draw data is dumped by the plugin (CyberpunkVR_ImGuiDump).
        waited = waited + delta
        if not menuAt then menuAt = waited; pcall(function() Game.VROverlayToggle() end); print('[page-probe] menu open')
        elseif menuAt > 0 and waited - menuAt >= 40 then menuAt = -1; pcall(function() Game.VROverlayToggle() end); print('[page-probe] menu closed') end
        return
    end
    waited = waited + delta
    if waited < 12 then return end
    local s = steps[step]
    if not requestedAt then
        if s[3] then pcall(function() Game.VRSettingsEdit(s[2], s[3], s[4]) end) end
        pcall(function() Game.VRSettingsPage(s[2]) end)
        requestedAt = waited
    elseif waited - requestedAt >= 2 then
        local ok, text = pcall(function() return Game.VRSettingsPage(s[2]) end)
        print(string.format('[page-probe] %s %s %s', s[1], tostring(ok), ok and shown(tostring(text)) or tostring(text)))
        step = step + 1
        requestedAt = nil
        if step > #steps then print('[page-probe] done') end
    end
end)
