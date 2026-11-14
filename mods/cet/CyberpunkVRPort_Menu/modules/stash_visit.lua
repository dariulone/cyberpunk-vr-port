-- The game's storage blackboard carries the actual container (apartment, vehicle or scene stash).
-- Intercept its menu request, after the device has applied its native stash retrofixes.
local V={}
local Availability=require("modules/availability")
function V.begin(shared,data,closeNative)
  if not Availability.enabled() then return false end
  local p=Game.GetPlayer();local object=data and data.storageObject
  if not p or not object then return false end
  local manager=VendorDataManager.new();manager:Initialize(p,object:GetEntityID())
  shared.stashSession={object=object,manager=manager,data=data,pending=true,closeNative=closeNative==true}
  return true
end
function V.bind(shared)
  shared.stashRouting=V
  if shared.stashRoutingBound then return end
  shared.stashRoutingBound=true
  Override("gameuiInGameMenuGameController","OnOpenStorage",function(self,value,wrapped)
    shared.menuController=self
    if shared.stashRouting.begin(shared,FromVariant(value),false) then return true end
    return wrapped(value)
  end)
end
function V.current(shared)
  local visit=shared.stashSession;local p=Game.GetPlayer()
  if not visit or not visit.active or not visit.object or not p then return nil end
  local a,b=p:GetWorldPosition(),visit.object:GetWorldPosition()
  if (a.x-b.x)^2+(a.y-b.y)^2+(a.z-b.z)^2>64 then return nil end
  return visit
end
return V
