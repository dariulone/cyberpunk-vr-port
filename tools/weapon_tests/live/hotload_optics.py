"""Targeted Optics update replacement under root's exclusive, save-protected lease.

The existing onUpdate callback and path publisher remain registered exactly once.
No game launch/load/save, input, weapon, or global mod reload is performed.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import uuid

ROOT=Path(__file__).resolve().parents[3]
spec=importlib.util.spec_from_file_location('weapon_optics_hot_protocol',Path(__file__).with_name('run.py'))
protocol=importlib.util.module_from_spec(spec);spec.loader.exec_module(protocol)
FILES=('init.lua','optics/frames.lua','optics/integrated.lua')

def patch_source(token,source_root):
    modules={name:(source_root/(name+'.lua')).read_text(encoding='utf-8') for name in
             ('optics/profiles','optics/visibility','optics/frames','optics/integrated')}
    body=(source_root/'init.lua').read_text(encoding='utf-8')
    literal=protocol.literal
    declarations='\n'.join('dependencies['+literal(name)+']=assert(loadstring('+literal(code)+'))()' for name,code in modules.items())
    factory='return function(dependencies) local require=function(name)return assert(dependencies[name],name)end;local registerForEvent=function()end;\n'+body+'\nend'
    return '''
assert(GoalOpticsPatch==nil,'another optics hot patch owns restoration')
local guard=GoalSaveGuard
assert(guard and guard.verified==true and guard.verification=='causal-remove-readd' and not guard.removed and Game.IsSavingLocked())
assert(Game.GetPlayer() and not Game.GetPlayer():GetActiveWeapon(),'holster before replacing optic state')
local live=assert(GetMod('CyberpunkVRPort_Optics'));assert(live.path=='none','old optic state must be inactive')
local dependencies={['optics/policy']=live.policy}
'''+declarations+'''
local fresh=assert(loadstring('''+literal(factory)+'''))()(dependencies)
local settings={'enabled','policy','shaderSplit','sightZoomEnabled','sightMode','approach','shotFromSight'}
local saved,keys={},{}
for k,v in pairs(live)do saved[k]=v;if k~='update' then keys[k]=true end end
local patch={token='''+literal(token)+''',live=live,saved=saved,fresh=fresh,keys=keys,rootName=guard.name}
GoalOpticsPatch=patch
live.update=function(delta)
 for _,k in ipairs(settings)do if live[k]~=nil then fresh[k]=live[k]end end
 local ok,err=pcall(fresh.update,delta)
 for k in pairs(fresh)do if k~='update' then keys[k]=true end end
 for k in pairs(keys)do live[k]=fresh[k]end
 if not ok then error(err)end
end
patch.restore=function()
 assert(Game.GetPlayer() and not Game.GetPlayer():GetActiveWeapon(),'holster before optics rollback')
 fresh.update(.3)
 for k in pairs(keys)do live[k]=nil end
 for k,v in pairs(saved)do live[k]=v end
 GoalOpticsPatch=nil
 return true
end
'''

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--game',type=Path,default=Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077'))
    action=p.add_mutually_exclusive_group(required=True)
    action.add_argument('--out',type=Path)
    action.add_argument('--restore',type=Path,help='Restore only this hot patch using its saved manifest/token')
    p.add_argument('--root-exclusive-lease',action='store_true')
    args=p.parse_args()
    if not args.root_exclusive_lease:p.error('explicit root live lease required')
    bridge=protocol.Bridge(args.game/'red4ext/plugins/cyberpunkvrport/lua/CVRBridge')
    target=args.game/'red4ext/plugins/cyberpunkvrport/lua/CyberpunkVRPort_Optics'
    if args.restore:
        out=args.restore.resolve();manifest=json.loads((out/'manifest.json').read_text(encoding='utf-8'))
        # Earlier hotloads backed up only init and frames.
        names=FILES[:2] if len(manifest['files'])==2 else FILES
        expected=[target/name for name in names]
        for row,path in zip(manifest['files'],expected,strict=True):
            if Path(row['path']).resolve()!=path.resolve() or not path.resolve().is_relative_to(target.resolve()):raise RuntimeError('manifest path is outside this optics module')
        token=protocol.literal(manifest['token'])
        if not bridge.call("(function() local p=GoalOpticsPatch;assert(p and p.token=="+token+");assert(Game.IsSavingLocked() and GoalSaveGuard and GoalSaveGuard.verified);return p.restore() end)()"):
            raise RuntimeError('runtime rollback did not succeed')
        for row,path in zip(manifest['files'],expected,strict=True):
            name=path.relative_to(target).as_posix()
            if row['existed']:path.write_bytes((out/(name.replace('/','-')+'.before')).read_bytes())
            else:path.unlink(missing_ok=True)
        manifest['status']='restored';(out/'manifest.json').write_text(json.dumps(manifest,indent=2),encoding='utf-8')
        print('Restored owned optics hot patch');return
    out=args.out.resolve();out.mkdir(parents=True,exist_ok=False)
    protection=bridge.call("(function() local g=GoalSaveGuard;return g and g.verified==true and g.verification=='causal-remove-readd' and not g.removed and Game.IsSavingLocked() and Game.GetPlayer()~=nil and Game.GetPlayer():GetActiveWeapon()==nil and GoalOpticsPatch==nil end)()")
    if not protection:raise RuntimeError('root save protection/inactive optics precondition unavailable; no files changed')
    source=ROOT/'mods/cet/CyberpunkVRPort_Optics'
    token=uuid.uuid4().hex;manifest={'token':token,'files':[],'status':'prepared'}
    for name in FILES:
        destination=target/name;before=destination.read_bytes() if destination.exists() else None
        if before is not None:(out/(name.replace('/','-')+'.before')).write_bytes(before)
        data=(source/name).read_bytes();manifest['files'].append({'path':str(destination),'existed':before is not None,'sha256':hashlib.sha256(data).hexdigest()})
    code=patch_source(token,source);(out/'patch.lua').write_text(code,encoding='utf-8')
    (out/'manifest.json').write_text(json.dumps(manifest,indent=2),encoding='utf-8')
    for name in FILES:(target/name).write_bytes((source/name).read_bytes())
    try:
        bridge.call(code,'exec')
        result=bridge.call("(function() local p=GoalOpticsPatch;return {token=p and p.token,path=GetMod('CyberpunkVRPort_Optics').path,rootBlocked=Game.IsSavingLocked()}end)()")
        if result.get('token')!=token or not result['rootBlocked']:raise RuntimeError('hot patch verification failed')
        manifest['status']='installed-single-callback';manifest['runtime']=result
    except BaseException as error:
        manifest['status']='needs-inspection';manifest['error']=str(error)
        raise
    finally:(out/'manifest.json').write_text(json.dumps(manifest,indent=2),encoding='utf-8')
    print(json.dumps(manifest,indent=2))

if __name__=='__main__':main()
