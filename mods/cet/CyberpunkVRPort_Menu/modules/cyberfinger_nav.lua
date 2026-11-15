-- Only called for the optional CyberFinger records returned by VRMenuPointer.
-- Button geometry and hit testing belong to the menu; no engine dependencies.
local N={}
function N.new() return {axes={{},{}},grabs={{},{}},menus={{},{}},rays={},focus=false} end
local function back(s,left,right)
  local down={left,right}
  if left and right then s.menuChord=true end
  if s.menuChord then
    for i=1,2 do s.menus[i].held=false;s.menus[i].armed=not down[i] end
    if not left and not right then s.menuChord=false end
    return false
  end
  local fire=false
  for i=1,2 do
    local m=s.menus[i]
    if not down[i] then fire=fire or m.held==true;m.armed=true;m.held=false
    elseif m.armed then m.held=true end
  end
  return fire
end
local function edge(s,down)
  if not down then s.armed=true;s.held=false;return false end
  local fire=s.armed and not s.held;s.held=true;return fire==true
end
local function axis(s,x,y,now,repeat_)
  if x~=x or y~=y then s.armed=false;s.dir=nil;return nil end
  local m=math.max(math.abs(x),math.abs(y))
  if m<.35 then s.armed=true;s.dir=nil;return nil end
  if not s.armed or m<.65 then return nil end
  local d=math.abs(x)>math.abs(y) and (x<0 and 3 or 4) or (y>0 and 1 or 2)
  if s.dir~=d then s.dir=d;s.next=now+.4;return d end
  if repeat_ and now>=s.next then s.next=now+.12;return d end
end
function N.centre(b)
  local y=b.y;if b.view then y=b.view.y+y-b.view.offset end
  return b.x+b.w/2,y+b.h/2
end
function N.visible(buttons,hitTest)
  local list={}
  for _,b in ipairs(buttons) do
    local x,y=N.centre(b)
    if (b.click or b.drag) and hitTest(x,y)==b then list[#list+1]=b end
  end
  return list
end
function N.neighbour(buttons,current,direction)
  if #buttons==0 then return nil end
  local found=false;for _,b in ipairs(buttons) do if b==current then found=true end end
  if not found then return buttons[1] end
  local x,y=N.centre(current);local best,score
  for _,b in ipairs(buttons) do if b~=current then
    local bx,by=N.centre(b);local dx,dy=bx-x,by-y
    local ahead,side
    if direction<3 then ahead=direction==1 and -dy or dy;side=math.abs(dx)
    else ahead=direction==3 and -dx or dx;side=math.abs(dy) end
    if ahead>1 then
      local cost=ahead+side*3+side*side/ahead
      if not score or cost<score then best,score=b,cost end
    end
  end end
  return best or current
end
function N.update(s,out,now,buttons,hitTest,cw,ch)
  if s.last and (now<s.last or now-s.last>.25) then
    local fresh=N.new();for k in pairs(s) do s[k]=nil end;for k,v in pairs(fresh) do s[k]=v end
  end
  local dt=s.last and math.max(0,math.min(.05,now-s.last)) or 0;s.last=now
  local axes,grabs=out[9],out[10]
  local l=edge(s.grabs[1],grabs.x>.5);local r=edge(s.grabs[2],grabs.y>.5)
  local meta=out[11] or {};local goBack=back(s,(meta.y or 0)>.5,(meta.z or 0)>.5)
  local d=axis(s.axes[1],axes.x,axes.y,now,true)
  local tab=axis(s.axes[2],math.abs(axes.z)>math.abs(axes.w) and axes.z or 0,0,now,false)
  local visible=N.visible(buttons,hitTest)
  local target;local targets={}
  for hand=1,2 do
    local ray=out[(hand-1)*4+2]
    if ray.w>.5 then
      local b=hitTest(ray.x*cw,ray.y*ch);target=target or b;targets[hand]=b
      local before=s.rays[hand]
      if not d and s.focus and before and ((ray.x-before.x)^2+(ray.y-before.y)^2)>.0016 then s.focus=false end
      if not s.focus or d or not before then s.rays[hand]={x=ray.x,y=ray.y} end
    end
  end
  if d then
    local previous=s.focus and s.button or target
    s.button=N.neighbour(visible,previous,d);s.focus=true
    if s.button==previous and previous and previous.view and d<3 then
      previous.view:scrollBy((d==1 and -1 or 1)*previous.h)
    end
  end
  local valid=false;for _,b in ipairs(visible) do if b==s.button then valid=true end end
  if s.focus and not valid then s.button=visible[1] end
  if not s.focus then s.button=target end
  local b=s.button
  if b and b.view and math.abs(axes.w)>.25 and math.abs(axes.w)>math.abs(axes.z) then
    b.view:scrollBy(-axes.w*dt*900)
  end
  local hand=l and 1 or r and 2 or nil
  return {back=goBack,secondary=hand,context=hand and (s.focus and b or targets[hand]) or nil,
    direction=d,tab=tab==3 and -1 or tab==4 and 1 or 0,button=s.focus and b or nil}
end
function N.tab(buttons,step)
  local tabs={};local selected=1
  for _,b in ipairs(buttons) do if b.cfTab then
    tabs[#tabs+1]=b;if b.cfSelected and b.cfSelected() then selected=#tabs end
  end end
  if #tabs>0 then return tabs[(selected-1+step)%#tabs+1] end
end
return N
