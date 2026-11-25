-- Sample when a physical swing starts; this never initiates an attack.
local M={}
local function held(side)
    return Game.GetVRSharedSlot(side==0 and 154 or 160)>.5
end
function M.strong(side)
    if type(VRCyberFingerEnabled)=='function' and VRCyberFingerEnabled() then
        return held(0) or held(1)
    end
    -- Other controllers retain the attacking hand's trigger modifier.
    return held(side)
end
return M
