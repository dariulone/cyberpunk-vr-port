local N=dofile(arg[1])
local b={}
for y=0,1 do for x=0,1 do b[#b+1]={x=x*100,y=y*100,w=80,h=80,click=function()end} end end
local function hit(x,y)
  for _,v in ipairs(b) do if x>=v.x and x<=v.x+v.w and y>=v.y and y<=v.y+v.h then return v end end
end
local out={};for i=1,11 do out[i]={x=0,y=0,z=0,w=0} end
out[2]={x=.2,y=.2,z=1,w=1}
local s=N.new()
local function step(t) return N.update(s,out,t,b,hit,200,200) end
out[9].x=1;out[9].z=1;out[10].x=1
local r=step(0);assert(not r.back and not r.direction and r.tab==0,"held entry activated")
out[9].x=0;out[9].z=0;out[10].x=0;step(.01)
out[9].x=1;r=step(.02);assert(r.button==b[2],"right navigation missed neighbour")
out[9].x=0;step(.03);out[9].y=-1
r=step(.04);assert(r.button==b[4],"down navigation missed neighbour")
r=step(.05);assert(r.button==b[4],"stationary laser stole focus")
out[2].x=.6;r=step(.06);assert(not r.button,"deliberate pointing did not reclaim")
out[6]={x=.2,y=.2,z=1,w=1}
out[10].y=1;r=step(.07);assert(not r.back and r.secondary==2 and r.context==b[1],"right Grip did not target its own ray context")
r=step(.08);assert(not r.secondary and not r.back,"held context request repeated")
out[9].z=1;r=step(.09);assert(r.tab==1,"right stick did not switch tab")
for i=1,10 do r=step(.09+i*.1);assert(r.tab==0,"held tab repeated") end
out[9].z=0;step(1.1);out[9].z=-1;r=step(1.11);assert(r.tab==-1,"left tab missing")
b[1].cfTab=true;b[2].cfTab=true;b[2].cfSelected=function()return true end
assert(N.tab(b,1)==b[1] and N.tab(b,-1)==b[1],"tab wrap")
assert(N.neighbour({b[1],b[3]},b[1],2)==b[3],"filtered hidden items")
-- A pause/reconnect while controls stay held must not reactivate them.
r=step(2);assert(not r.back and not r.secondary and not r.direction and r.tab==0,"stale controls replayed")
out[11].y=1;r=step(2.01);assert(not r.back,"Menu fired before release")
out[11].y=0;r=step(2.02);assert(r.back,"left Menu did not go Back on release")
out[11].z=1;step(2.03);out[11].z=0;r=step(2.04);assert(r.back,"right Menu did not go Back")
out[11].y=1;step(2.05);out[11].z=1;r=step(2.06);assert(not r.back,"chord generated Back")
out[11].y=0;r=step(2.07);assert(not r.back,"partial chord release generated Back")
out[11].z=0;r=step(2.08);assert(not r.back,"chord release generated Back")
out[10].y=0;step(2.09);out[10].x=1;r=step(2.10)
assert(r.secondary==1 and r.context==b[2] and not r.back,"left Grip used other hand's target")
print("CyberFinger world-menu navigation passed")
