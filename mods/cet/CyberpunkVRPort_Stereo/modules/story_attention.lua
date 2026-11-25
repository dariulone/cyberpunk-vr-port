-- Narrow story fixes adapted from Ajson44 V39; no quest facts are written.
-- Native leases expire if CET stops updating. No game object survives a tick.
local M = { mode=0, error=nil }
local elapsed=0
local cachedDefs=nil

local function context(overlayOpen)
    if overlayOpen then return nil end
    local player=Game.GetPlayer()
    if not player then return nil end
    cachedDefs=cachedDefs or GetAllBlackboardDefs()
    local defs=cachedDefs
    local boards=Game.GetBlackboardSystem()
    local menu=boards:Get(defs.UI_System)
    if not menu or menu:GetBool(defs.UI_System.IsInMenu) then return nil end
    local psm=boards:GetLocalInstanced(player:GetEntityID(),defs.PlayerStateMachine)
    if not psm then return nil end
    local locomotion=psm:GetInt(defs.PlayerStateMachine.Locomotion)
    local tier=psm:GetInt(defs.PlayerStateMachine.SceneTier)
    local vehicle=psm:GetInt(defs.PlayerStateMachine.Vehicle)
    local workspot=false
    if locomotion==9 then
        local system=Game.GetWorkspotSystem()
        workspot=system and system:IsActorInWorkspot(player) or false
    end
    local choices=false
    if vehicle==3 or vehicle==4 or vehicle==7 then
        -- Missing passenger-hub data is not permission to replace its ray.
        choices=true
        local ui=boards:Get(defs.UIInteractions)
        if ui then
            local hub=FromVariant(ui:GetVariant(defs.UIInteractions.InteractionChoiceHub))
            choices=hub and hub.choices and #hub.choices>0 or false
        end
    end
    local quests=Game.GetQuestsSystem()
    local started,done=0,1
    if quests then
        started=quests:GetFactStr('q110_02_camera_scan_start') or 0
        done=quests:GetFactStr('q110_02_scanning_done') or 0
    end
    return player,started,done,locomotion,tier,workspot and 1 or 0,vehicle,choices and 1 or 0
end

local function clue(player)
    if not player then return nil end
    local targeting=Game.GetTargetingSystem()
    local object=targeting and targeting:GetLookAtObject(player,false,false)
    if not object then return nil end
    -- The REDscript method checks the specific mesh AND the current scan state.
    -- Query/write errors fail closed; no cached true value across save loads.
    local index=object:CyberpunkVRPortManualFocusClueIndex()
    if index and index>=0 then return object,index end
    return nil
end

function M.tick(dt,overlayOpen)
    elapsed=elapsed+math.max(tonumber(dt) or 0,0)
    if elapsed<.05 then return end
    elapsed=0
    if type(VRStoryAttentionUpdate)~='function' or type(VRManualClueUpdate)~='function' then return end
    local ok,player,started,done,loco,tier,workspot,vehicle,choices=pcall(context,overlayOpen)
    M.error=not ok and tostring(player) or nil
    if not ok then player=nil end
    local published,mode=pcall(VRStoryAttentionUpdate,player,started or 0,done or 1,loco or -1,
        tier or 0,workspot or 0,vehicle or 0,choices or 0,player and 0 or 1)
    M.mode=published and tonumber(mode) or 0
    if not published then M.error=tostring(mode) end
    local found,object,index=pcall(clue,player)
    if not found then M.error=tostring(object);object=nil end
    local pressed,request=pcall(VRManualClueUpdate,player,object)
    if pressed and tonumber(request)==1 and object then
        -- Explicit X edge only; revalidate scan and identity inside the method.
        local inspected,result=pcall(function() return object:CyberpunkVRPortInspectManualFocusClue(index) end)
        if not inspected then M.error=tostring(result) end
    end
end
function M.shutdown()
    elapsed=0;M.mode=0
    if type(VRStoryAttentionUpdate)=='function' then pcall(VRStoryAttentionUpdate,nil,0,1,-1,0,0,0,0,1) end
    if type(VRManualClueUpdate)=='function' then pcall(VRManualClueUpdate,nil,nil) end
end
return M
