"""World mirror appearance route: no native menu, real-player equipment untouched."""
from pathlib import Path
import unittest
from lupa.luajit21 import LuaRuntime
ROOT=Path(__file__).resolve().parents[2]
MOD=ROOT/'mods/cet/CyberpunkVRPort_Menu/modules'
SETUP=r'''
CName={new=function(v)return v end};TweakDBID={new=function(v)return v end};NameToString=function(v)return v end
TDBID={ToStringDEBUG=function(v)return v end};ItemID={FromTDBID=function(v)return v end,GetTDBID=function(v)return v end}
IsDefined=function(v)return v~=nil end;GetLocalizedTextByKey=function(v)return v end
TargetSearchQuery={new=function()return {}end};TargetingSet={Complete=1};gameuiCharacterCustomizationEditTag={HairDresser=1}
Vector4={Distance=function(a,b)return math.sqrt((a.x-b.x)^2+(a.y-b.y)^2+(a.z-b.z)^2)end}
function entity(id,record)
 return {slots={},items={},pos={x=0,y=0,z=0},GetEntityID=function()return {hash=id}end,
 GetRecordID=function()return record end,IsA=function()return true end,GetResolvedGenderName=function()return 'Male'end,
 GetWorldPosition=function(self)return self.pos end}
end
player=entity('player','Player');puppet=entity('mirror','Character.q110_tpp_male_v');johnny=0;enabled=true
function equip(e,s,i)e.slots[s]={GetItemID=function()return i end};e.items[i]=true end
equip(player,'AttachmentSlots.Head','real-hat');equip(puppet,'AttachmentSlots.Head','hat');equip(puppet,'AttachmentSlots.Eyes','mask')
equip(puppet,'AttachmentSlots.TppHead','original-head');equip(puppet,'AttachmentSlots.RightArm','original-arm')
EquipmentSystem={GetPlacementSlot=function(i)return i:find('Head') and 'AttachmentSlots.TppHead' or 'AttachmentSlots.RightArm'end}
changes={};reapplied={};removed={};handed={};target=nil;restoredTarget=0;accepted=0;cancelled=0;resource=true;acceptResult=true;initialized=0
options={}
for i=1,12 do options[i]={isEditable=true,isActive=true,isCensored=false,currIndex=0,
 info={name='o'..i,localizedName='Option'..i,GetClassName=function()return 'gameuiAppearanceInfo'end,
 definitions={{localizedName='a'},{localizedName='b'},{localizedName='c'}}}}end
resets={};inits={}
sys={GetUnitedOptions=function()return options end,
 ResetCustomizationOptionsResource=function()resets[#resets+1]={target=target,headBack=puppet.slots['AttachmentSlots.Head']~=nil,handed=#handed}end,
 InitializeOptionsFromFinalizedState=function()initialized=initialized+1
  inits[#inits+1]={target=target,tpp=puppet.slots['AttachmentSlots.TppHead']:GetItemID(),reapplied=#reapplied}end,ApplyEditTag=function()end,
 ApplyChangeToOption=function(_,o,i,now)lastNow=now;if o.currIndex==i then return end
  if o.currIndex==4294967295 then reapplied[#reapplied+1]={o=o,i=i,now=now,target=target} else changes[#changes+1]=i end
  o.currIndex=i end,ReFinalizeState=function()accepted=accepted+1;return acceptResult end,
 CancelFinalizedStateUpdate=function()cancelled=cancelled+1;return true end}
ts={GetItemInSlot=function(_,e,s)return e.slots[s]end,HasItem=function(_,e,i)return e.items[i]end,
 RemoveItemFromSlot=function(_,e,s,destroy)assert(e~=player and destroy==true);removed[#removed+1]=s;e.slots[s]=nil end,
 GiveItem=function(_,e,i)assert(e~=player);e.items[i]=true end,
 AddItemToSlot=function(_,e,s,i)assert(e~=player);equip(e,s,i);return true end,
 RemoveItem=function(_,e,i)assert(e~=player);e.items[i]=nil end}
VRMirrorAppearanceOptions=function(e,male)assert(e==puppet and male);return resource end
VRCreationTarget=function(e)target=e;if e==nil then restoredTarget=restoredTarget+1 end;return true end
finished={};VRMirrorAppearanceFinish=function(p)finished[#finished+1]={panel=p,headBack=puppet.slots['AttachmentSlots.Head']~=nil};return true end
handed={};nativeResult=true;nativeReset=true
VRMirrorAppearanceNative=function(p)handed[#handed+1]={panel=p,headBack=puppet.slots['AttachmentSlots.Head']~=nil,target=target,cancelled=cancelled,accepted=accepted};return nativeResult end
VRCustomizationReset=function()if not nativeReset then return false end
 resets[#resets+1]={target=target,headBack=puppet.slots['AttachmentSlots.Head']~=nil,handed=#handed,native=true};return true end
Game={GetPlayer=function()return player end,GetTransactionSystem=function()return ts end,
 GetCharacterCustomizationSystem=function()return sys end,GetQuestsSystem=function()return {GetFactStr=function()return johnny end}end,
 GetTargetingSystem=function()return {GetTargetParts=function()return true,{{GetComponent=function()return {GetEntity=function()return puppet end}end}} end}end}
shared={};kit={S={menu={placed=false}},rebuild=function()end}
function start()assert(A.route({})==nil);assert(not A.poll(.4));assert(A.current().phase=='ready');assert(A.route({})==true);assert(A.poll(.1))end
function activate()start();kit.S.menu.placed=true;A.present(kit);assert(A.state().phase=='active')end
function done()A.poll(1);A.poll(1)end
'''
class Tests(unittest.TestCase):
 def setUp(self):
  self.L=LuaRuntime(unpack_returned_tuples=True);self.L.execute(SETUP)
  self.L.globals().A=self.L.execute((MOD/'appearance.lua').read_text(encoding='utf-8'))
  self.L.execute('A.bind(shared,function()return enabled end)')
 def check(self,s):self.L.execute(s)
 def test_disabled_falls_back(self):self.check('enabled=false;assert(A.route({})==false and A.current()==nil)')
 def test_johnny_untouched(self):self.check('johnny=1;assert(A.route({})==false and #removed==0)')
 def test_real_player_never_preview(self):self.check('puppet=player;assert(A.route({})==false and #removed==0)')
 def test_only_mirror_record(self):self.check("puppet.GetRecordID=function()return 'Character.Judy'end;assert(A.route({})==false)")
 def test_readiness(self):self.check("assert(A.route({})==nil);assert(not A.poll(.1));assert(initialized==0);assert(A.route({})==nil);A.poll(.3);assert(initialized==1 and A.route({})==true)")
 def test_no_native_menu_dependency(self):self.check("activate();assert(A.state().world and target==puppet and player.slots['AttachmentSlots.Head'])")
 def test_current_values_reapplied(self):self.check("options[1].currIndex=2;options[2].isActive=false;options[3].isEditable=false;"
   "start();assert(#reapplied==11 and #changes==0);for _,r in ipairs(reapplied)do assert(r.now==true and r.target==puppet and r.o.currIndex==r.i)end;"
   "assert(reapplied[1].o==options[1] and reapplied[1].i==2 and options[2].currIndex==0)")
 def test_filter(self):self.check('options[1].isEditable=false;options[2].isActive=false;options[3].isCensored=true;assert(#A.rows()==9)')
 def test_options_wrap_and_apply_at_once(self):self.check("activate();local r=A.rows()[1];A.change(r,-1);assert(changes[1]==2 and lastNow==true);A.change(r,1);assert(changes[2]==0)")
 def test_cancel_restores_headwear_then_ends(self):self.check("activate();assert(not puppet.slots['AttachmentSlots.Head']);assert(A.cancel());"
   "assert(puppet.slots['AttachmentSlots.Head']:GetItemID()=='hat' and puppet.slots['AttachmentSlots.TppHead']:GetItemID()=='original-head');"
   "assert(A.current().phase=='restoring' and target==puppet and cancelled==1);done();"
   "assert(A.current()==nil and target==nil and cancelled==1 and not A.state().accepted)")
 def test_close_rebuilds_double_on_its_own_head(self):self.check("activate();assert(A.cancel());assert(initialized==1);A.poll(1);"
   "assert(initialized==2 and inits[2].tpp=='original-head' and inits[2].target==puppet);assert(#reapplied==24 and accepted==1);"
   "for i=13,24 do assert(reapplied[i].target==puppet) end;assert(A.current() and #finished==0);A.poll(1);assert(A.current()==nil and #finished==1)")
 def test_accept_then_rebuild(self):self.check('activate();assert(A.action(true));assert(accepted==1 and cancelled==0);done();assert(accepted==2 and cancelled==0 and A.state().accepted)')
 def test_failed_accept_stays_open(self):self.check("activate();acceptResult=false;assert(not A.action(true));assert(A.current().phase=='active' and cancelled==0)")
 def test_accept_only_when_active(self):self.check("assert(A.route({})==nil);assert(not A.action(true) and accepted==0)")
 def test_cancel_reverts_preview(self):self.check("activate();local r=A.rows()[1];A.change(r,1);assert(changes[1]==1);assert(A.cancel());assert(#changes==2 and changes[2]==0 and r.option.currIndex==0 and cancelled==1)")
 def test_accept_keeps_preview(self):self.check("activate();local r=A.rows()[1];A.change(r,1);assert(A.action(true));assert(#changes==1 and r.option.currIndex==1)")
 def test_close_steps_ignore_more_closes(self):self.check("activate();assert(A.cancel());assert(not A.cancel() and not A.action(true) and not A.native() and cancelled==1)")
 def test_walk_away_cancels(self):self.check("activate();player.pos={x=10,y=0,z=0};A.poll(.1);assert(A.current().phase=='restoring' and cancelled==1);done();assert(A.current()==nil)")
 def test_scene_completed_after_restore(self):self.check("local p={};assert(A.route(p)==nil);A.poll(.4);assert(A.route(p)==true);A.poll(.1);kit.S.menu.placed=true;A.present(kit);assert(A.cancel());done();assert(#finished==1 and finished[1].panel==p and finished[1].headBack);assert(A.state().sceneCompleted)")
 def test_resource_failure_fallback(self):self.check("resource=false;assert(A.route({})==nil);A.poll(.4);assert(A.route({})==false and A.current()==nil);assert(puppet.slots['AttachmentSlots.Head'])")
 def test_partial_rows_fallback(self):self.check("options={};assert(A.route({})==nil);A.poll(.4);assert(A.route({})==false and cancelled==1 and target==nil)")
 def test_fallback_still_reports_panel(self):self.check("resource=false;local p={};assert(A.route(p)==nil);A.poll(.4);assert(A.route(p)==false);assert(#finished==1 and finished[1].panel==p)")
 def test_fallback_leaves_system_clean(self):self.check("resource=false;assert(A.route({})==nil);A.poll(.4);assert(A.route({})==false);assert(#resets==2 and resets[2].target==nil)")
 def test_native_editor_handoff(self):self.check("local p={};assert(A.route(p)==nil);A.poll(.4);assert(A.route(p)==true);A.poll(.1);kit.S.menu.placed=true;A.present(kit);"
   "local r=A.rows()[1];A.change(r,1);assert(A.native());assert(changes[2]==0 and cancelled==1 and #handed==0);"
   "A.poll(1);assert(#handed==0 and initialized==2);A.poll(1);"
   "assert(#handed==1 and handed[1].panel==p and handed[1].headBack and handed[1].target==nil);"
   "assert(#resets==2 and resets[2].handed==0 and resets[2].headBack);"
   "assert(#finished==0 and A.current()==nil);local l=A.state();assert(l.native and not l.sceneCompleted and l.reason=='native')")
 def test_native_editor_refused_completes_scene(self):self.check("nativeResult=false;local p={};assert(A.route(p)==nil);A.poll(.4);assert(A.route(p)==true);A.poll(.1);kit.S.menu.placed=true;A.present(kit);"
   "assert(A.native());done();assert(#handed==1 and #finished==1 and finished[1].panel==p);local l=A.state();assert(not l.native and l.sceneCompleted)")
 def test_native_editor_only_when_active(self):self.check("assert(A.route({})==nil);assert(not A.native());assert(#handed==0 and A.current())")
 def test_reset_falls_back_to_script(self):self.check("nativeReset=false;activate();assert(#resets==1 and not resets[1].native)")
 def test_system_reset_after_session_settles(self):self.check("activate();assert(#resets==1);assert(A.cancel());done();assert(#resets==1);A.poll(1);assert(#resets==1);"
   "A.poll(1.1);assert(#resets==2 and resets[2].target==nil and resets[2].headBack);A.poll(5);assert(#resets==2)")
 def test_new_request_flushes_pending_reset(self):self.check("activate();assert(A.action(true));done();assert(#resets==1);A.route({});assert(#resets==2)")
 def test_no_reset_when_system_untouched(self):self.check("johnny=1;assert(A.route({})==false);assert(#resets==0)")
 def test_release(self):self.check('activate();A.release();assert(A.current()==nil and cancelled==1 and target==nil and #resets==2)')
 def test_release_while_restoring(self):self.check('activate();assert(A.cancel());A.release();assert(A.current()==nil and cancelled==1 and target==nil and #resets==2)')
if __name__=='__main__':unittest.main()
