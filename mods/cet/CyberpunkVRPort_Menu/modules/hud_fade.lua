-- Fade only the HUD's ink window; preserve the game's visibility rules and original opacity.
local H={}
function H.restore()
  if H.root then pcall(function() H.root:SetOpacity(H.base or 1) end) end
  H.root,H.base,H.hidden,H.anim=nil,nil,nil,nil
end
function H.update(hidden,dt)
  if not H.root then
    if not hidden then return end
    local layer=Game.VRInkLayer(CName.new("inkHUDLayer"))
    local root=layer and layer:GetVirtualWindow()
    if not root then return end
    H.root=root;H.base=root:GetOpacity();H.hidden=false
  end
  if hidden~=H.hidden then
    H.hidden=hidden
    H.anim={from=H.root:GetOpacity(),to=hidden and 0 or H.base,time=0,duration=hidden and .14 or .18}
  end
  local a=H.anim
  if a then
    a.time=math.min(a.duration,a.time+math.max(0,math.min(dt or 0,.1)))
    local t=a.time/a.duration;local e=t*t*(3-2*t)
    H.root:SetOpacity(a.from+(a.to-a.from)*e)
    if t>=1 then
      H.anim=nil
      if not hidden then H.root,H.base,H.hidden=nil,nil,nil end
    end
  end
end
return H
