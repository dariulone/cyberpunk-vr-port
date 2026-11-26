"""Save-locked simulator probe. The driver only moves the controller; it does not dispatch an attack."""
import json
import struct
import time


def run(out, sim, pad, call, ex, equip, tag):
    equip('Items.Preset_Katana_Default')
    sim.hands_follow_head(True)
    sim.head(0,-7)
    base=(.15,-.51,-.47)
    def move(offset):
        sim.hand('right',pos=(base[0],base[1]-offset,base[2]),ypr=(0,-65,-10),mode='body')
    def gesture():
        for displacement in (.023,.046,.069,.092,.115):
            move(displacement)
            time.sleep(.018)
        time.sleep(.32)
    def state():
        row = call("""(function()
            local p=Game.GetPlayer();local w=p:GetActiveWeapon();local d=GetMod('CyberpunkVRPort_Weapon').CyberwareStatus()
            local n=StrokeTestNpc;local h=n and Game.GetStatPoolsSystem():GetStatPoolValue(n:GetEntityID(),gamedataStatPoolType.Health,false)
            return {shortQueries=d.strokeQueries or 0,shortDispatch=d.strokeDispatches or 0,full=d.fullSwingStarts or 0,
                sounds=d.whooshes or 0,health=h,hits=p.vrMeleeConfirmedHits,serial=w:VRBladeConfirmedSerial(),
                follow=w:VRBladeConfirmedFollowThrough(),mark=n and n:VRBladeMarkDiagnostic() or nil}
        end)()""")
        counts = struct.unpack('<8i', pad.read('CyberpunkVR_HapticStats', 32))
        row['haptic'] = dict(zip(('accepted','left','right','xrResult','profile','dropped','shots','events'), counts))
        return row
    move(0);time.sleep(1.3)
    result={'pid':sim.pid,'clip':'short','equipment':'Items.Preset_Katana_Default','input':'OpenXR hand position only'}
    path=out/(tag+'-short-stroke.json')
    try:
        result['airBefore']=state();gesture();result['airAfter']=state()
        move(0);time.sleep(1.0)
        # Place an isolated temporary test actor on the measured virtual prop segment.
        ex("""local p=Game.GetPlayer();local w=p:GetActiveWeapon();local a=w:GetWorldPosition();local u=w:VRMeleeDirection();local b=p:GetWorldPosition();local s=DynamicEntitySpec.new();s.recordID=TweakDBID.new('Character.maelstrom_grunt1_melee1_knife_ma');s.position=Vector4.new(a.x+.57*u.x,a.y+.57*u.y,b.z+.02,1);s.orientation=p:GetWorldOrientation();s.persistState=false;s.persistSpawn=false;s.alwaysSpawned=true;s.tags={CName.new('%s')};Game.GetDynamicEntitySystem():CreateEntity(s)"""%tag)
        time.sleep(1.4)
        ex("""StrokeTestNpc=Game.GetDynamicEntitySystem():GetTagged(CName.new('%s'))[1];assert(StrokeTestNpc);local p=Game.GetPlayer();StrokeTestNpc:GetAttitudeAgent():SetAttitudeTowards(p:GetAttitudeAgent(),EAIAttitude.AIA_Hostile);StrokeTestNpc:SetIndividualTimeDilation(CName.new('cpvr_stroke_test'),.005,15,CName.new('None'),CName.new('None'))"""%tag)
        time.sleep(.50);result['touchBefore']=state();gesture();result['touchAfter']=state()
        time.sleep(.5);result['still']=state()
        ab,aa,tb,ta,still=[result[k] for k in ('airBefore','airAfter','touchBefore','touchAfter','still')]
        result['pass']=bool(aa['shortQueries']>ab['shortQueries'] and aa['shortDispatch']==ab['shortDispatch'] and aa['hits']==ab['hits'] and aa['serial']==ab['serial'] and aa['full']==ab['full'] and ta['full']==tb['full'] and ta['sounds']==tb['sounds'] and ta['shortDispatch']==tb['shortDispatch']+1 and ta['serial']==tb['serial']+1 and ta['hits']==tb['hits']+1 and ta['health']<tb['health'] and not ta['follow'] and still['serial']==ta['serial'] and still['health']==ta['health'])
        # No synthetic haptic here. Compare counters around exactly the same input,
        # then require the actual damage event's profile on the weapon hand only.
        result['hapticsEnabled'] = pad.i32('CyberpunkVR_Haptics')
        ha,hb,ht,hh,hs=(row['haptic'] for row in (ab,aa,tb,ta,still))
        fixed=('accepted','left','right','events','dropped','shots')
        result['hapticPass']=bool(result['hapticsEnabled'] and all(ha[k]==hb[k] for k in fixed)
            and hh['accepted']==ht['accepted']+1 and hh['events']==ht['events']+1
            and hh['right']>ht['right'] and hh['left']==ht['left'] and hh['xrResult']==0
            and hh['profile']==14 and hh['dropped']==ht['dropped'] and hh['shots']==ht['shots']
            and all(hh[k]==hs[k] for k in fixed))
        print('evidence',json.dumps(result),flush=True)
        if result['pass']:
            try:sim.shot(out/(tag+'-short-stroke'), eyes='left',eye_size=960,timeout=3)
            except Exception as e:print('capture ack:',e,flush=True)
        assert result['pass'] and result['hapticPass'], 'The gesture, damage or real haptic control did not meet acceptance.'
    finally:
        path.write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
        ex("Game.GetDynamicEntitySystem():DeleteTagged(CName.new('%s'));StrokeTestNpc=nil"%tag)
