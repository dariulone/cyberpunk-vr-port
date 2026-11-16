"""Isolated LuaJIT regressions for item tooltips and ripperdoc selection (#188).

Run with python -I; requires lupa.luajit21. --source-dir selects older module files.
No game, rendering process, or native controller is started by this fixture.
"""
import argparse
from pathlib import Path
import sys
import unittest
from lupa.luajit21 import LuaRuntime

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "mods/cet/CyberpunkVRPort_Menu/modules"

SETUP = r'''
clock = 0
os.clock = function() return clock end
function print(...) end
widgets = {}
function widget()
    local w = {visible=true, opacity=1, size={X=0,Y=0}, children={}}
    function w:SetVisible(v) self.visible=v end
    function w:SetAffectsLayoutWhenHidden(v) self.hiddenLayout=v end
    function w:SetOpacity(v) self.opacity=v end
    function w:SetScale(v) self.scale=v end
    function w:SetMargin(v) self.margin=v end
    function w:SetAnchor(...) end
    function w:SetAnchorPoint(...) end
    function w:SetRenderTransformPivot(...) end
    function w:SetTintColor(...) end
    function w:GetDesiredSize() return self.size end
    function w:RemoveAllChildren() self.children={} end
    function w:RemoveChild(c)
        for i,v in ipairs(self.children) do if v==c then table.remove(self.children,i);break end end
    end
    function w:GetController() return self.controller end
    widgets[#widgets+1]=w
    return w
end
K={BEAM={len=1},ATLAS={phone="phone"},CYAN={},YELLOW={},BLACK={},GREY={},NAVY={},RED={},TEXT={}}
function K.canvas(parent,...) local w=widget();if parent then parent.children[#parent.children+1]=w end;return w end
K.roundFill=K.canvas;K.text=K.canvas;K.rect=K.canvas;K.image=K.canvas
function K.cutFrame(...) return {} end
K.roundEdge=K.cutFrame
function K.tint(...) end
function K.color(c) return c end
function K.clip(text,...) return text end
function K.move(w,x,y) w.x=x;w.y=y end
Vector2={new=function(v) return v end}
inkMargin={new=function(v) return v end}
CName={new=function(v) return v end}
ResRef={FromString=function(v) return v end}
inkEAnchor={TopLeft=0}
gamedataItemType={Prt_Program=1}
gamedataEquipmentArea={FrontalCortexCW=1,EyesCW=2}
ItemDisplayContext={Vendor=1,GearPanel=2}
Game={GetPlayer=function() return {} end}
ItemDisplayContextData={Make=function(_,context,compare) return {context=context,compare=compare,AddTag=function() end} end}
-- Actual clean 2.5 RTTI names (DumpType), not the decompiler's m_-prefixed script property spelling.
UIInventoryItemTooltipWrapper={Make=function(ui,context) return {data=ui,displayContext=context,overridePrice=-1} end}
UIInventoryItemComparisonManager={Make=function(ui,compare) return {item=ui,compare=compare} end}
function item(key,area,deck)
    local raw={HasTag=function(_,tag) return deck and tag=="Cyberdeck" end}
    local ui={GetName=function() return key end, GetQualityText=function() return "Tier 3" end,
        GetDescription=function() return "Description" end, IsWeapon=function() return false end,
        GetItemData=function() return raw end,GetItemType=function() return 0 end}
    return {key=key,ui=ui,raw=raw,quality="Common",area=area}
end
nativeSpawns=0
controller={SpawnFromExternal=function(_,parent,path,library)
    nativeSpawns=nativeSpawns+1
    local w=K.canvas(parent);w.path=path;w.library=library
    w.controller={SetData=function(_,data) w.data=data end,Show=function() w.visible=true end}
    lastWidget=w
    return w
end}
C={state=function() return state end,redraw=function() redraws=(redraws or 0)+1 end}
U={line=function() end,picture=function() end,title=function() end,
    action=function() return {} end,headAction=function() return {} end}
function C.list(...) return {enableMask=function() end} end
left=item("left",1);right=item("right",2)
snapshot={groups={
    {area="FrontalCortexCW",name="Frontal cortex",slots={{item=left,index=0}},items={}},
    {area="EyesCW",name="Eyes",slots={{item=right,index=0}},items={}}
},byKey={left=left,right=right},vendor=false,
stats={max=100,used=10,armorMax=100,armor=10,base=100,extra=0}}
D={open=function() return {} end,snapshot=function() return snapshot end,signature=function() return "same" end,
    session=function() return nil end,updatePurchase=function() return false end}
state={}
buttons={}
kit={S={time=0,views={}},P1X=100,P1Y=50,glass=function() end,menuController=function() return controller end,
    at=function(x,y) return {x=x,y=y} end,button=function(group,origin,x,y,w,h,click,paint)
        local b={group=group,x=origin.x+x,y=origin.y+y,w=w,h=h,click=click,paint=paint}
        buttons[#buttons+1]=b;return b
    end}
function button(key) for _,b in ipairs(buttons) do if b.key==key then return b end end;error(key) end
function overlaps(a,b)
    return a.x<b.x+b.w and b.x<a.x+a.w and a.y<b.y+b.h and b.y<a.y+a.h
end
modules={
    ["modules/ink"]=K,["modules/locale"]={tr=function(_,en) return en end},
    ["modules/cyberware_data"]=D,["modules/inventory_data"]={invalidate=function() end},
    ["modules/gameplay_data"]={},["modules/cyberware_anatomy"]={new=function() return {} end,focus=function() end,update=function() end}
}
function require(name) return assert(modules[name],name) end
'''


class TooltipTests(unittest.TestCase):
    def setUp(self):
        self.lua = LuaRuntime(unpack_returned_tuples=True)
        self.lua.execute(SETUP)
        self.tooltip = self.lua.execute((SOURCE / "item_tooltip.lua").read_text(encoding="utf-8"))
        self.lua.globals().modules["modules/item_tooltip"] = self.tooltip
        self.lua.globals().Tooltip = self.tooltip

    def native_bridge(self):
        self.lua.execute('''
prepareStatus=0;prepareCalls=0;bridgeSpawns=0
local stock=controller.SpawnFromExternal
controller.SpawnFromExternal=function() error("stock path cannot see undeclared tooltip library") end
function VRMenuTooltipPrepare(path) prepareCalls=prepareCalls+1;return prepareStatus end
function VRMenuTooltipSpawn(ctrl,parent,path,library)
    assert(ctrl==controller);bridgeSpawns=bridgeSpawns+1
    return stock(ctrl,parent,path,library)
end
''')

    def test_pending_library_spawns_once_when_ready(self):
        self.native_bridge()
        self.lua.execute('''
tip=Tooltip.spawn(kit,widget(),left.ui,1,0,0,500,900,right.ui,125,true)
assert(tip.pending and not tip.widget)
for _=1,20 do Tooltip.fit({tip}) end
assert(bridgeSpawns==0 and #tip.parent.children==0,"loading leaked a black backdrop")
prepareStatus=1;Tooltip.fit({tip})
assert(not tip.pending and tip.widget and bridgeSpawns==1)
assert(tip.widget.data.overridePrice==125)
assert(tip.widget.data.comparisonData.compare==right.ui)
Tooltip.fit({tip});assert(bridgeSpawns==1)
''')

    def test_slow_library_replaces_fallback_after_load(self):
        self.native_bridge()
        self.lua.execute('''
tip=Tooltip.spawn(kit,widget(),left.ui,1,0,0,500,900)
clock=1;Tooltip.fit({tip});assert(tip.pending and tip.fell and tip.card)
prepareStatus=1;Tooltip.fit({tip})
assert(tip.widget and not tip.pending and not tip.fell and not tip.card)
tip.widget.size={X=500,Y=700};Tooltip.fit({tip});assert(tip.laid)
''')

    def test_failed_library_stops_retry_and_shows_fallback(self):
        self.native_bridge()
        self.lua.execute('''
tip=Tooltip.spawn(kit,widget(),left.ui,1,0,0,500,900)
prepareStatus=-1;Tooltip.fit({tip})
assert(not tip.pending and tip.fell and tip.card and not tip.widget)
local calls=prepareCalls;Tooltip.fit({tip});assert(prepareCalls==calls and bridgeSpawns==0)
''')

    def test_closed_panel_does_not_spawn_from_old_pending_request(self):
        self.native_bridge()
        self.lua.execute('''
local tips={Tooltip.spawn(kit,widget(),left.ui,1,0,0,500,900)}
tips={};prepareStatus=1;Tooltip.fit(tips)
assert(bridgeSpawns==0)
''')

    def test_upgrade_snapshot_waits_for_native_library(self):
        self.native_bridge()
        self.lua.execute('''
local data={snapshot=true}
tip=Tooltip.spawnData(kit,widget(),data,0,0,500,900)
assert(tip.pending and tip.noCard)
clock=1;Tooltip.fit({tip});assert(not tip.card)
prepareStatus=1;Tooltip.fit({tip})
assert(tip.widget and tip.widget.data==data and not tip.pending)
''')

    def test_cyberware_deferred_spawn_uses_native_bridge(self):
        self.native_bridge()
        self.load_cyberware()
        self.lua.execute('''
view.draw(kit,widget(),widget());button("cyberArea:FrontalCortexCW:0").paint(true)
kit.S.time=.2;view.update(kit)
local tip=state.cyber.tips[1];assert(tip.pending and bridgeSpawns==0)
prepareStatus=1;kit.S.time=.3;view.update(kit)
assert(tip.widget and not tip.pending and bridgeSpawns==1)
''')

    def test_comparison_and_price_reach_native_wrapper_fields(self):
        self.lua.execute('''
tip=Tooltip.spawn(kit,widget(),left.ui,1,0,0,500,900,right.ui,125)
assert(tip.widget.data.comparisonData and tip.widget.data.comparisonData.item==left.ui)
assert(tip.widget.data.comparisonData.compare==right.ui)
assert(tip.widget.data.overridePrice==125)
''')

    def test_zero_price_override_is_not_lost(self):
        self.lua.execute('''
tip=Tooltip.spawn(kit,widget(),left.ui,1,0,0,500,900,nil,0)
assert(tip.widget.data.overridePrice==0)
''')

    def test_late_native_layout_replaces_temporary_fallback(self):
        self.lua.execute('''
tip=Tooltip.spawn(kit,widget(),left.ui,1,0,0,500,900)
clock=1;Tooltip.fit({tip})
assert(tip.fell and tip.card and not tip.widget.visible and tip.widget.hiddenLayout)
local card=tip.card
tip.widget.size={X=500,Y=700};Tooltip.fit({tip})
assert(tip.laid and not tip.fell,"late native layout stayed permanently on the description-only card")
assert(tip.widget.visible and tip.widget.opacity==1)
assert(not card.visible or tip.card==nil,"fallback overlaps recovered native stats")
''')

    def test_missing_widget_keeps_safe_fallback(self):
        self.lua.execute('''
controller.SpawnFromExternal=function() return nil end
tip=Tooltip.spawn(kit,widget(),left.ui,1,0,0,500,900)
clock=5;Tooltip.fit({tip});Tooltip.fit({tip})
assert(tip.fell and tip.card and not tip.widget)
''')

    def test_native_tooltip_that_lays_out_does_not_use_fallback(self):
        self.lua.execute('''
tip=Tooltip.spawn(kit,widget(),left.ui,1,20,30,500,900)
tip.widget.size={X=500,Y=700};Tooltip.fit({tip})
assert(tip.laid and not tip.fell and not tip.card)
assert(tip.widget.opacity==1)
''')

    def load_cyberware(self):
        factory = self.lua.execute((SOURCE / "cyberware_view.lua").read_text(encoding="utf-8"))
        self.lua.globals().view = factory(self.lua.globals().C, self.lua.globals().U)

    def test_overview_tooltip_never_blocks_its_own_card(self):
        self.load_cyberware()
        self.lua.execute('''
view.draw(kit,widget(),widget())
for _,key in ipairs({"cyberArea:FrontalCortexCW:0","cyberArea:EyesCW:0"}) do
    local b=button(key);b.paint(true);kit.S.time=kit.S.time+.2;view.update(kit)
    assert(state.cyber.tipBlock.visible)
    assert(not overlaps(b,state.cyber.tipBlock),"tooltip blocker covers its hovered card: "..key)
    assert(state.cyber.tipClose.x>=state.cyber.tipBlock.x)
    assert(state.cyber.tipClose.x+state.cyber.tipClose.w<=state.cyber.tipBlock.x+state.cyber.tipBlock.w)
    b.paint(false);kit.S.time=kit.S.time+.2;view.update(kit)
end
''')

    def test_right_overview_card_stays_hovered_and_clickable(self):
        self.load_cyberware()
        self.lua.execute('''
view.draw(kit,widget(),widget())
local b=button("cyberArea:EyesCW:0")
local hovered=false
for _=1,90 do
    local blocked=state.cyber.tipBlock.visible and overlaps(b,state.cyber.tipBlock)
    local on=not blocked
    if on~=hovered then b.paint(on);hovered=on end
    kit.S.time=kit.S.time+1/90;view.update(kit)
end
assert(hovered and state.cyber.hover=="right","hover oscillates against its own tooltip")
assert(nativeSpawns==1,"stationary pointer repeatedly respawns the tooltip")
b.click();assert(state.cyber.area=="EyesCW")
''')

    def test_cyberware_uses_same_late_layout_recovery(self):
        self.load_cyberware()
        self.lua.execute('''
view.draw(kit,widget(),widget());button("cyberArea:FrontalCortexCW:0").paint(true)
kit.S.time=.2;view.update(kit)
local tip=state.cyber.tips[1]
clock=1;kit.S.time=.3;view.update(kit);assert(tip.fell)
tip.widget.size={X=500,Y=700};kit.S.time=.4;view.update(kit)
assert(tip.laid and not tip.fell and tip.widget.visible)
''')

    def test_upgrade_preserves_native_base_to_option_comparison(self):
        self.load_cyberware()
        self.lua.execute('''
D.item=function(_,raw,_,ui) return {ui=ui,raw=raw} end
right.ui.GetItemData=function() return right.raw end
D.upgrade=function() return {allowed=true} end
state.cyber={ctx={},upgrade={base=left,data={option1InventoryItem=right.ui},up={have=10,cost={materialCount=1}}}}
view.draw(kit,widget(),widget())
local data=state.cyber.tips[2].widget.data
assert(data.comparisonData and data.comparisonData.item==left.ui)
assert(data.comparisonData.compare==right.ui)
''')


if __name__ == "__main__":
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--source-dir", type=Path)
    args, remaining = parser.parse_known_args()
    if args.source_dir:
        SOURCE = args.source_dir.resolve()
    unittest.main(argv=[sys.argv[0], *remaining])
