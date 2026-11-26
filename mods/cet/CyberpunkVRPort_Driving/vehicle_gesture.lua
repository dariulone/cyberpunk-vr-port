-- Contact + a deliberate stroke, once per grip. All points are vehicle-local.
local M={}
function M.new() return {blocked=true} end
function M.step(s,now,grip,contact,point)
    if not grip then
        s.blocked=false;s.was=false;s.target=nil;s.done=false;s.start=nil
        return false,false
    end
    if s.blocked then return false,false end
    if not s.was then s.pressed=now;s.was=true end
    if not s.target and contact and now-s.pressed<=.6 then
        s.target=contact;s.start=point;s.at=now
    end
    if not s.target then return false,false end
    if s.done then return false,true end
    if not point or now-s.at>2 then s.done=true;return false,true end
    local d2=(point.x-s.start.x)^2+(point.y-s.start.y)^2+(point.z-s.start.z)^2
    -- A teleport/tracking discontinuity spends this press without an action.
    if d2>.35^2 then s.done=true;return false,true end
    if now-s.at>=.09 and d2>=.045^2 then s.done=true;return true,true end
    return false,true
end
return M
