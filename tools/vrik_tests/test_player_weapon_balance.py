"""Offline production LuaJIT routing and compiled REDscript call-chain regression.

Pass --records <gamedataWeaponItem_Record.jsonl> for the optional WolvenKit census.
No writes to the game, no game launch, no CET connection.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import re
import shutil
import subprocess
from tempfile import TemporaryDirectory

from lupa.luajit21 import LuaRuntime

ROOT = Path(__file__).resolve().parents[2]
MOD = ROOT / 'mods/cet/CyberpunkVRPort_Weapon'
TRIGGER = ROOT / 'mods/redscript/CyberpunkVRPort_NoAnims/vrport_weapon_trigger.reds'
BUNDLE = Path('C:/Program Files (x86)/Steam/steamapps/common/Cyberpunk 2077/r6/cache/final.redscripts')
CLI = ROOT / 'build/redscript-cli-0532/release/redscript-cli.exe'


def hand_test(records=None, manifest=None):
    lua = LuaRuntime(unpack_returned_tuples=True)
    g = lua.globals()
    g.profile = lua.execute((MOD / 'hand_kick.lua').read_text(encoding='utf-8'))
    g.attachments = lua.execute((MOD / 'attachment_recoil.lua').read_text(encoding='utf-8'))
    lua.execute('''
        local noop = function() end
        local right, left, props, callbacks = nil, nil, {}, {}
        local active, serial = true, 0
        local pl = {GetActiveWeapon=function() return right or left end}
        local ts = {GetItemData=function(_,p,id) return id.data end, GetItemInSlot=function(_,p,s)
            assert(p==pl and s=='AttachmentSlots.WeaponLeft'); return left end}
        local stats = {AddModifier=function(_,id,mod) assert(id.hash and mod==0); zeros=zeros+1 end,
                       RemoveModifier=noop}
        zeros=0
        function gun(key, family, kind, iconic, friendly, mode, secondary)
            serial=serial+1; local id=serial
            props[key..'.audioName']={value=family}; props[key..'.itemType']=kind
            props[key..'.friendlyName']=friendly; props[key..'.primaryTriggerMode']=mode or 'TriggerMode.SemiAuto'
            props[key..'.secondaryTriggerMode']=secondary
            return {GetEntityID=function() return {hash=id} end, GetItemID=function() return key end,
                    GetIsIconic=function() return iconic or false end, GetMuzzleSlotWorldTransform=noop}
        end
        function frame(r,l,dt) right=r; left=l; callbacks.onUpdate(dt or 0) end
        held=false; GetVRTwoHandActive=function() return held end
        function near(a,b) assert(type(a)=='number' and math.abs(a-b)<1e-6,tostring(a)..' ~= '..tostring(b)) end
        Game={GetPlayer=function() if active then return pl end end,GetTransactionSystem=function() return ts end,
              GetStatsSystem=function() return stats end,VRCyberwareSelect=noop}
        function setPlayer(on) active=on end
        require=function(name) if name=='hand_kick' then return profile end
            if name=='attachment_recoil' then return attachments end
            return {update=noop,clear=noop,reset=noop,kind=function() return 0 end,inputKind=function() return 0 end,
                    handEquipment=noop,state={}} end
        registerForEvent=function(n,f) callbacks[n]=f end; spdlog={info=noop}; io={open=noop}
        observers={}; Observe=function(cls,n,f) assert(cls=='PlayerPuppet'); observers[n]=f end
        function start() callbacks.onInit() end
        TweakDB={GetFlat=function(_,k) return props[k] end}; TDBID={ToStringDEBUG=function(x) return x end}
        TweakDBID={new=function(s) return s end}; ItemID={GetTDBID=function(x) return x end}
        WeaponObject={IsMelee=function() return false end}; gameStatModifierType={Multiplier=1}
        gamedataStatType={RecoilKickMin=1,RecoilKickMax=2,RecoilKickMinADS=3,RecoilKickMaxADS=4,RecoilAngle=5,RecoilAngleADS=6}
        RPGManager={CreateStatModifier=function(_,op,v) assert(op==1 and v==0); return v end}
        function SetVRWeaponKick(k) primary=k end
        function SetVRLeftWeaponKick(k,c) partner=k end
    ''')
    lua.execute((MOD / 'init.lua').read_text(encoding='utf-8'))
    lua.execute('''
        local n=gun('Items.Preset_Nue_Default','wea_set_nue','gamedataItemType.Wea_Handgun')
        local k=gun('Items.Preset_Kappa_Default','wea_set_kappa','ItemType.Wea_Handgun',false,'w_handgun_arasaka_yukimura','TriggerMode.FullAuto')
        frame(n,k); near(primary,1.32); near(partner,.702); near(n.vrTapCycleScale,.205/.350)
        near(k.vrTapCycleScale,.90); assert(zeros==12)
        local ky=gun('Items.Preset_Kyubi_Default','wea_set_kyubi','ItemType.Wea_Rifle <TweakDBID>',false,'w_rifle_assault_militech_ajax')
        frame(nil,ky); near(primary,1); near(partner,1); near(ky.vrTapCycleScale,.85)
        local t=gun('Items.Preset_Nue_Arasaka_2020_Left_Hand1','wea_set_nue','Wea_Handgun')
        frame(t,nil); near(primary,1.32); near(t.vrTapCycleScale,.205/.400); assert(partner==0)
        local i=gun('Items.Preset_Nue_Jackie','wea_set_nue','ItemType.Wea_Handgun',true)
        frame(i,n); near(primary,1.32); near(partner,1.32); near(i.vrTapCycleScale,.205/.350)
        local unity=gun('Items.Preset_Unity_Default','wea_set_unity','ItemType.Wea_Handgun')
        frame(nil,unity); near(primary,1); near(partner,1); near(unity.vrTapCycleScale,.60)
        local z=zeros; local supported=math.sqrt(17/34)*1.2/1.4
        held=true; frame(nil,unity,.03); near(primary,1 + (supported-1)*(1-math.exp(-1)))
        frame(nil,unity,.3); near(primary,supported); near(unity.vrTapCycleScale,.60)
        held=false; frame(nil,unity,.3); near(primary,1); assert(zeros==z)
        local u=gun('Items.Unknown','unrecognized','ItemType.Wea_Handgun')
        frame(u,nil); assert(primary==1 and partner==0 and u.vrTapCycleScale==0)
        local old=zeros; setPlayer(false); frame(n,k); assert(zeros==old and primary==1 and partner==0)
        -- Exact type/audio matching, mode-specific cadence and documented exceptions.
        for _,case in ipairs({{'ticon','HANDGUN',1.06,.75,'Burst'},{'grit','Handgun',.9,.8},
            {'liberty','Handgun',1.2,.185/.275},{'omaha','Handgun',.9,.155/.1777},
            {'nue','Handgun',1.32,.205/.350,'Burst'},{'achilles','PrecisionRifle',.72,.875,'Burst'},
            {'sor22','PrecisionRifle',.85,.5},{'kolac','PrecisionRifle',.95,.53},
            {'achilles','PrecisionRifle',.72,.875},{'rasetsu','SniperRifle',.85,.720/1.20},
            {'grad','SniperRifle',.95,1.15/2.70},{'ashura','SniperRifle',.7,.410/.600},
            {'nekomata','SniperRifle',.92,.450/.600,'Charge'},{'silverhand','Handgun',1.704,.220/.300},
            {'overture','Revolver',1.584,.270/.400}, {'grad_panam','SniperRifle',.95,1.15/2.70},
            {'unity_dex','Handgun',1,.60},{'liberty_dex','Handgun',1.2,.185/.275},
            {'kyubi','Revolver',false},{'kyubi','RifleModded',false},{'kyubi','SniperRifle',false},
            {'lexington_special','Handgun',false},{'special_plasma_cutter','Rifle',.84,.82}}) do
            local typ='gamedataItemType.Wea_'..case[2]
            local obj=gun('Items.X','wea_set_'..case[1],typ,false,nil,'TriggerMode.'..(case[5] or 'SemiAuto'))
            local solo,supported,scale=profile(obj,'Items.X',typ)
            if case[3] then near(solo,case[3]); near(scale or 0,case[4]); assert(not supported or supported<=solo+1e-9)
            else assert(not solo and not scale,case[1]) end
        end
    ''')
    print('PASS LuaJIT: actual draw, left-only and duals, support transition, iconic/alternate modes and camera isolation')
    attachment_test(lua)
    if records:
        audit_records(g, records, manifest)


def attachment_test(lua):
    lua.execute('''
        setPlayer(true); start()
        local kick, quality = gamedataStatType.RecoilKickMax, 77
        gamedataStatType.Quality=quality
        local function mod(id, stat, op, v, perQuality)
            return {GetID=function() return id end, StatType=function() return {StatType=function() return stat end} end,
                IsA=function(_,name) return name==(perQuality and 'gamedataCombinedStatModifier_Record'
                                                            or 'gamedataConstantStatModifier_Record') end,
                RefObject=function() return {value='Self'} end, OpSymbol=function() return {value='*'} end,
                RefStat=function() return {StatType=function() return quality end} end,
                Value=function() return v end, ModifierType=function() return {value=op} end}
        end
        local function rec(name, rows, groups)
            return {GetID=function() return name end, TagsContains=function() return false end, StatModifiers=function() return rows end,
                    StatModifierGroups=function() return groups or {} end}
        end
        -- Installed game's w_muzzle_brake_01 rows, including both alternate representations.
        local brake=rec('Items.w_muzzle_brake_01',{
            mod('min','RecoilKickMin','AdditiveMultiplier',-.075),
            mod('max',kick,'AdditiveMultiplier',-.075), mod('tier',kick,'AdditiveMultiplier',-.025,true),
            mod('display','RecoilKickReduction','Additive',7.5), mod('displayTier','RecoilKickReduction','Additive',2.5,true)})
        local fixed=rec('Items.w_muzzle_brake_01_legendary',{mod('legend',kick,'AdditiveMultiplier',-.20)})
        local function group(limit,rows)
            return {StatModsLimit=function() return limit end, StatModsLimitModifier=function() end,
                    StatModifiers=function() return rows end}
        end
        local percent=mod('shared',kick,'AdditiveMultiplier',-.10)
        local grouped=rec('Items.TestGrouped',{percent},{group(-1,{percent}),
            group(1,{mod('candidate',kick,'Multiplier',0)})})
        local spread=rec('Items.w_att_power_street_12602_mzl',{mod('spread','Spread','Multiplier',.5)})
        local absolute=rec('Items.SuzieWeaponMod',{mod('absolute',kick,'Additive',-.5)})
        local scaled=rec('Items.NashWeaponMod',{mod('relative',kick,'Multiplier',.6)})
        local base=rec('weapon',{mod('base',kick,'Additive',2.5)})
        TweakDBInterface={GetItemRecord=function() return base end}
        local muzzle, modslot='AttachmentSlots.PowerModule','AttachmentSlots.TestMod'
        InventoryUtils={IsPart=function(slot) return slot==muzzle end}
        RPGManager.IsWeaponMod=function(id) return id and id.weaponMod end
        InnerItemData={GetItemID=function(p) return p.id end, GetStaticData=function(p) return p.record end,
            GetStatValueByType=function(p,s) assert(s==quality); return p.q end}
        ItemID.IsValid=function(id) return id~=nil end
        ItemID.GetTDBID=function(id) return id.key end
        WeaponObject.IsRanged=function(id) return id.key~=nil end
        local function installed(record,q,weaponMod)
            return {record=record,q=q or 0,id={weaponMod=weaponMod}}
        end
        local function copy()
            local key='Items.Preset_Nue_Default'
            local w=gun(key,'wea_set_nue','Wea_Handgun')
            local inv={slots={},parts={}, GetUsedSlotsOnItem=function(self) return self.slots end,
                GetItemPart=function(self,s) return self.parts[s] end, GetStatValueByType=function() return 0 end}
            local id={key=key,data=inv}
            w.GetItemID=function() return id end
            return w,inv,id
        end
        local first,a,aid=copy(); local second,b=copy()
        local scale={}
        SetVRAttachmentRecoil=function(side,f) scale[side]=f end
        local function expect(p,l)
            near(scale[1],p); near(scale[0],l)
            local status=attachments.status()
            assert(not status.primaryError and not status.partnerError,status.primaryError or status.partnerError)
        end
        -- Right and left are distinct instances of the same weapon record.
        frame(first,second); expect(1,1)
        a.slots={muzzle,muzzle}; a.parts[muzzle]=installed(brake,3)
        b.slots={muzzle}; b.parts[muzzle]=installed(brake,1)
        observers.OnPartAddedToSlotEvent(); frame(first,second); expect(.85,.90)
        local z=zeros
        held=true; frame(first,second,.1); expect(.85,.90)
        assert(zeros==z); held=false
        a.parts[muzzle]=installed(fixed)
        observers.OnPartRemovedFromSlotEvent(); frame(first,second); expect(.8,.90)
        a.parts[muzzle]=installed(spread)
        frame(first,second,.51); expect(1,.90) -- direct transaction, same entity, no event
        a.slots={}; observers.OnPartRemovedFromSlotEvent(); frame(first,second); expect(1,.90)
        frame(second,first); expect(.90,1)
        frame(nil,second); expect(.90,1) -- left-only uses the primary spring
        a.slots={muzzle,modslot}; a.parts[muzzle]=installed(fixed); a.parts[modslot]=installed(scaled,0,true)
        frame(first,nil); expect(.8*.6,1)
        a.slots={modslot,'AttachmentSlots.TestMod2'}
        a.parts[modslot]=installed(grouped,0,true); a.parts[a.slots[2]]=installed(grouped,0,true)
        frame(first,nil,.51); expect(.8,1) -- one contribution per installed slot
        a.slots={muzzle,modslot}
        a.parts[modslot]=installed(absolute,0,true)
        frame(first,nil,.51); expect((1-.5/2.5)*.8,1)
        aid.data=nil; frame(first,nil,.51); near(scale[1],1); assert(attachments.status().primaryError)
        aid.data=a; frame(first,nil,.51); expect(.64,1)
        frame(nil,nil); expect(1,1)
        frame(first,second); expect(.64,.90)
        setPlayer(false); frame(first,second); expect(1,1)
    ''')
    print('PASS LuaJIT: installed slot equip/swap/remove, quality, same-record hands, full reset and recovery')


def magazine_input_test():
    lua = LuaRuntime(unpack_returned_tuples=True)
    mod = ROOT / 'mods/cet/CyberpunkVRPort_Reload'
    lua.globals().load_module = lambda path: lua.execute((mod / (path+'.lua')).read_text(encoding='utf-8'))
    lua.execute('''
        local catch = load_module('reload/mag_release')
        gamedataItemType = { Wea_Handgun = 1 }
        WeaponObject = { IsOfType = function(id, typ) return id == typ end }
        local gun = { GetItemID = function() return 1 end }
        local other = { GetItemID = function() return 2 end }
        GetVRTwoHandActive = function() return false end
        assert(catch(gun,true,true,true,false))
        assert(not catch(other,true,true,true,false))
        assert(not catch(gun,false,true,true,false))
        assert(not catch(gun,true,false,true,false))
        assert(not catch(gun,true,true,false,false))
        assert(not catch(gun,true,true,true,true))
        GetVRTwoHandActive = function() return true end
        assert(not catch(gun,true,true,true,false))
    ''')
    reload_source = (mod / 'reload/reload.lua').read_text(encoding='utf-8')
    assert "require('reload/mag_release')" in reload_source
    print('PASS LuaJIT pistol B catch and grip ownership')


def audit_records(g, records, manifest):
    import csv
    counts = Counter()
    output = []
    types = {'Wea_Handgun','Wea_Rifle','Wea_PrecisionRifle','Wea_SniperRifle'}
    with records.open(encoding='utf-8-sig') as f:
        for r in map(json.loads, f):
            counts['all'] += 1
            typ=r.get('itemType','').split(' <')[0].split('.')[-1]
            pri, sec = (r.get(k,'').split(' <')[0] for k in ('primaryTriggerMode','secondaryTriggerMode'))
            semi = 'TriggerMode.SemiAuto' in (pri,sec)
            old_scope = typ in types and semi
            in_scope = old_scope or typ in {'Wea_Handgun','Wea_Revolver','Wea_SniperRifle','Wea_PrecisionRifle'} and bool(pri or sec)
            if not in_scope: continue
            counts[typ] += 1
            key=r['id']; audio=r.get('audioName'); kind='ItemType.'+typ
            # Run the production resolver for BOTH rarity paths. A lookup in a parallel
            # test-side family table or counting keys that were never returned is not coverage.
            answers=[]
            for iconic in (False,True):
                w=g.gun(key,audio,kind,iconic,r.get('friendlyName'),pri,sec)
                ans=g.profile(w,key,kind)
                assert isinstance(ans,tuple) and len(ans)==5, (key,typ,audio,pri,sec,iconic,ans)
                solo,supported,ratio,blend,reference=ans
                assert reference in {'M1911A1','G17Gen3','HPoint380','R1022Classic'}, (key,reference)
                assert solo>0 and 0<supported<=solo+1e-8 and 0<ratio<1 and blend>0, (key,ans)
                answers.append(ans)
            assert answers[0]==answers[1], key
            if old_scope: counts['four-type semi with a working profile']+=1
            if typ=='Wea_Revolver' and semi: counts['additional semi revolvers']+=1
            output.append([key,typ,pri.split('.')[-1],sec.split('.')[-1],audio or '',round(solo,5),round(supported,5),round(ratio,7),reference])
    keys=[r[0] for r in output];assert len(keys)==len(set(keys))
    assert counts['all']>0 and counts['four-type semi with a working profile']>0
    if manifest:
        with manifest.open('w',newline='',encoding='utf-8') as f:
            writer=csv.writer(f,lineterminator='\n');writer.writerow(['record','type','primary','secondary','audioFamily','oneHand','twoHand','cycleScale','hotDogsPoseReference']);writer.writerows(sorted(output))
    print('Census: recoil and trigger-input profiles, including iconics:',dict(counts),'total covered',len(output))


def compiled_test(cli, bundle):
    checked_classes = {'WeaponTransition', 'FullAutoEvents', 'FullAutoDecisions', 'SemiAutoDecisions',
                       'ShootEvents', 'CycleRoundDecisions', 'BurstDecisions', 'ChargeDecisions',
                       'ChargeReadyDecisions', 'ChargeMaxDecisions', 'CycleTriggerModeDecisions'}
    with TemporaryDirectory(prefix='cpvr-player-cycle-') as folder:
        out = Path(folder); src = out/'scripts'; src.mkdir()
        for file in (TRIGGER, TRIGGER.with_name('vrport_full_auto_release.reds'),
                     TRIGGER.parents[1]/'CyberpunkVRPort_DualWield/vrport_dual_wield.reds',
                     TRIGGER.parents[1]/'CyberpunkVRPort_LeftHanded/vrport_left_handed.reds'):
            shutil.copyfile(file, src/file.name)
        script = out/'result.redscripts'; code = out/'result.txt'
        subprocess.run([cli,'compile','-s',src,'-b',bundle,'-o',script],cwd=out,check=True)
        assert script.is_file(), 'REDscript compiler produced no bundle'
        # Single output file only. Split-file decompilation follows absolute source paths.
        subprocess.run([cli,'decompile','-i',script,'-o',code,'-m','code'],cwd=out,check=True)
        classes={}
        with code.open(encoding='utf-8') as f:
            current=None
            for line in f:
                if current is None:
                    match=re.match(r'public (?:abstract )?class (\w+) extends ',line)
                    if match and match[1] in checked_classes:
                        current=match[1];classes[current]=[]
                elif line.startswith('}'):
                    current=None
                    if classes.keys()==checked_classes:break
                else:classes[current].append(line)
    def normalized(name): return re.sub(r'\$local\$\d+', '', ''.join(classes[name]))
    def method(name, cls='WeaponTransition'):
        return re.search(r' func '+re.escape(name)+r'\([^\n]* \{\n(.*?)\n  \}',normalized(cls),re.S)[1]
    def called(body, prefix, cls='WeaponTransition'):
        return method(re.search(r'this\.('+prefix+r'\$\d+)\(',body)[1], cls)
    cycle=called(method('CanPerformNextShotInSequence'),'wrapper')
    assert 'fingerSemi: Bool = this.VRFingerSemiAuto(' in cycle and 'ready: Bool = this.VRShotIntervalReady(' in cycle
    assert re.search(r'return ready && \(fingerSemi \|\| this\.proxy\$\d+\(', cycle), cycle
    assert '!ready' in cycle and 'VRDiscardEarlyPress' in cycle
    native=called(cycle,'proxy')
    for required in ('weaponObject.GetEntityID()', 'gamedataStatType.CycleTime','this.GetLastShotTime(stateContext)',
                     'this.CalcCycleTimeDeltaFactor','GameInstance.GetSimTime'):
        assert required in native, required
    assert 'n"LastSecondaryShotTime" : n"LastShotTime"' in method('GetLastShotTimeVarName')
    action=called(method('IsSemiAutoAction'),'wrapper')
    eligible=method('VRFingerSemiAuto')
    for required in ('VRWeaponTriggerEnabled','as PlayerPuppet','ValentinosFrenzySE','QuestForceShoot','MountedToVehicle',
                     'IsInVisionModeActiveState','GetQuestForceShootName','gamedataTriggerMode.SemiAuto'):
        assert required in eligible, required
    for required in ('VRFingerSemiAuto','ShouldShoot','GetActionValue','GetLastChargePressCount',
                     'GetActionPressCount','IsActionJustPressed','GetActionStateTime',
                     'vrAwaitFingerPressCount','lastPress.value + 1 == count','PlayerLastUsedKBM','IsDualWielding'):
        assert required in action, required
    sequence=called(method('StartShootingSequence'),'wrapper')
    assert 'VRFingerSemiAuto' in sequence and 'burstCycleTime, numShotsBurst, isFullChargeFullAuto' in sequence
    assert 'CanPerformNextShotInSequence' in method('CanPerformNextSemiAutoShot')
    assert 'IsMagazineEmpty' in method('CanPerformNextSemiAutoShot')
    assert 'CanPerformNextShotInSequence' in method('ExitCondition', 'CycleRoundDecisions')
    assert 'CanPerformNextFullAutoShot' in method('EnterCondition', 'FullAutoDecisions')
    assert 'CanPerformNextFullAutoShot' in method('ToFullAutoTransitionCondition')
    assert 'CanPerformNextShotInSequence' in method('CanPerformNextFullAutoShot')
    for cls, name, predicate, native_checks in (
        ('BurstDecisions', 'ToShoot', 'VRShotIntervalReady', ('GetBurstTimeRemainingName', 'IsMagazineEmpty')),
        ('ChargeDecisions', 'EnterCondition', 'VRCanBeginCharge', ('IsMagazineEmpty', 'GetWeaponChargeMinValue')),
        ('CycleTriggerModeDecisions', 'ToCharge', 'VRCanBeginCharge', ('CycleTriggerModeStatCycleTime', 'GetActionValue')),
    ):
        gate = called(method(name, cls), 'wrapper', cls)
        assert re.search(r'this\.proxy\$\d+\([^)]+\) && this\.' + predicate + r'\(', gate), (cls, gate)
        original = called(gate, 'proxy', cls)
        assert all(check in original for check in native_checks), (cls, original)
    for cls in ('ChargeDecisions', 'ChargeReadyDecisions', 'ChargeMaxDecisions'):
        assert 'GetActionValue(n"RangedAttack") <= 0.00' in method('ToShoot', cls)
    auto_shoot = called(method('ToShoot', 'FullAutoDecisions'), 'wrapper', 'FullAutoDecisions')
    assert 'VRReleasedFullAuto' in auto_shoot and 'IsMagazineEmpty' in called(auto_shoot, 'proxy', 'FullAutoDecisions')
    release = called(method('VRReleasedFullAuto'), 'wrapper')
    assert 'VRDualHandReleased' in release and 'VRAutoReleaseBlocks' in called(release, 'proxy')
    assert 'CanPerformNextSemiAutoShot' in method('ToSemiAutoTransitionCondition')
    assert 'IsSemiAutoAction' in method('ToSemiAutoTransitionCondition')
    decisions='SemiAutoDecisions'
    entry=called(method('EnterCondition',decisions),'wrapper',decisions)
    for required in ('m_inputValue <= 0.00','VRCurrentFingerAction','VRFingerSemiAuto','ShouldShoot',
                     'VRHasFreshFingerAction','CanPerformNextSemiAutoShot'):
        assert required in entry, required
    for name in ('OnAttach','RefreshEnterCondition','OnQuestForcedShoot'):
        assert 'EnableOnEnterCondition(true)' in called(method(name,decisions),'wrapper',decisions)
    receipt = called(method('OnEnter','ShootEvents'),'wrapper','ShootEvents')
    assert 'VRConsumeFingerAction' in receipt and 'GameInstance.GetEngineTime' in receipt
    accepted = called(receipt, 'proxy', 'ShootEvents')
    assert 'SetLastShotTime' in accepted and 'ShootingSequencePostShoot' in accepted
    for body in (native,method('StartShootingSequence'),normalized('FullAutoEvents')):
        assert 'vrTapCycleScale' not in body
    print('PASS retail REDscript call chains: accepted shot, per-hand cycles, burst rounds, charge entries and full-auto release')


if __name__ == '__main__':
    parser=argparse.ArgumentParser(); parser.add_argument('--records',type=Path)
    parser.add_argument('--cli',type=Path,default=CLI); parser.add_argument('--bundle',type=Path,default=BUNDLE)
    parser.add_argument('--manifest',type=Path,help='write every resolved record and both presentation weights to CSV')
    args=parser.parse_args(); magazine_input_test(); hand_test(args.records,args.manifest); compiled_test(args.cli,args.bundle)
