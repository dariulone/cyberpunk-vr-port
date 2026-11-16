-- One gate for direct opens and native vendor/storage interception.
local A={}
function A.enabled()
  if type(VRInterfaceEnabled)~="function" then return true end
  local ok,on=pcall(VRInterfaceEnabled)
  return ok and on==true
end
return A
