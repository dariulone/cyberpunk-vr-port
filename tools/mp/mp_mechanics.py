"""VR mechanics in a NightCoop session, seen from the other PC: this PC's player acts (CETBridge + the harness
simulator), the second PC's host takes composited stills of what it sees (tools/mp/sim_pose.py shot there).

    py -3.13 -B tools/mp/mp_mechanics.py place [--dist 2.5]    # this PC's player in front of the host, facing it
    py -3.13 -B tools/mp/mp_mechanics.py weapon                # a Lexington equipped and drawn here
    py -3.13 -B tools/mp/mp_mechanics.py reload                # a reload here (left X), host stills during it
    py -3.13 -B tools/mp/mp_mechanics.py car                   # a car spawned, this player mounted, driven and steered
    py -3.13 -B tools/mp/mp_mechanics.py shot NAME             # one host still

Stills land in build/mp/shots/host-<name>_L.jpg. A session must be up (coop.py session, the second PC hosting).
"""
import argparse
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path[:0] = [str(HERE), str(ROOT / 'tools/remote'), str(ROOT / 'tools/sim_harness')]
import cet  # noqa: E402
import home_pc  # noqa: E402

SHOTS = ROOT / 'build/mp/shots'
HOME_SHOTS = r'X:\cvr-tmp\mp'
WEAPON = 'Items.Preset_Lexington_Default'
CAR = 'Vehicle.v_standard2_archer_hella_player'


def lua(code, home=False, timeout=30):
    r = cet.call('exec', code, on_home=home, timeout=timeout)
    if not r.get('ok'):
        raise RuntimeError(r.get('error') or r)
    return r.get('result', '')


def host_shot(name):
    remote = f'{HOME_SHOTS}\\host-{name}'
    out = home_pc.run(f"Set-Location X:\\CyberpunkVRPort; py -3.13 -B tools/mp/sim_pose.py shot {remote}").stdout
    SHOTS.mkdir(parents=True, exist_ok=True)
    local = SHOTS / f'host-{name}_L.jpg'
    home_pc.scp(f'{home_pc.HOST}:{remote}_L.jpg'.replace('\\', '/'), str(local))
    print('host still', local if local.exists() else f'missing ({out.strip()})')
    return local


def place(args):
    """This PC's player `dist` m along the host's forward, turned to face it (the host's position read there)."""
    pos = lua('local p=Game.GetPlayer(); local q=p:GetWorldPosition(); local f=p:GetWorldForward(); '
              'print(string.format("%.3f %.3f %.3f %.3f %.3f", q.x, q.y, q.z, f.x, f.y))', home=True).split()
    x, y, z, fx, fy = map(float, pos)
    tx, ty = x + fx * args.dist, y + fy * args.dist
    print(lua(f'''
local p = Game.GetPlayer()
local yaw = math.deg(math.atan2(({fx}), -({fy}))) + 180
Game.GetTeleportationFacility():Teleport(p, Vector4.new({tx}, {ty}, {z}, 1), EulerAngles.new(0, 0, yaw))
print(string.format("placed at %.2f %.2f %.2f, yaw %.0f (the host at %.2f %.2f)", {tx}, {ty}, {z}, yaw, {x}, {y}))
'''))


def weapon(_args):
    print(lua(f'''
local p = Game.GetPlayer()
local ts = Game.GetTransactionSystem()
local tdb = TweakDBID.new("{WEAPON}")
local id
local ok, items = ts:GetItemList(p)
for _, it in ipairs(items) do if ItemID.GetTDBID(it:GetID()) == tdb then id = it:GetID() break end end
if not id then id = ItemID.FromTDBID(tdb); ts:GiveItem(p, id, 1) end
local r = EquipRequest.new(); r.owner = p; r.slotIndex = 0; r.itemID = id; r.addToInventory = false
Game.GetScriptableSystemsContainer():Get(CName.new("EquipmentSystem")):QueueRequest(r)
CVRMpWeapon = id
print("equipped " .. TDBID.ToStringDEBUG(tdb))
'''))
    time.sleep(1.0)
    print(lua('''
local r = DrawItemRequest.new(); r.owner = Game.GetPlayer(); r.itemID = CVRMpWeapon
Game.GetScriptableSystemsContainer():Get(CName.new("EquipmentSystem")):QueueRequest(r)
print("draw asked")
'''))
    time.sleep(2.0)
    print(lua('local w = Game.GetPlayer():GetActiveWeapon(); print(w and TDBID.ToStringDEBUG(ItemID.GetTDBID(w:GetItemID())) or "no weapon")'))
    host_shot('weapon')


def reload(_args):
    """Two shots (a full magazine is not reloaded), then the reload on the simulator's left X (the port's binding;
    the test pad's X does not reach it), host stills during it."""
    from simxr import Sim
    mag = 'local w = Game.GetPlayer():GetActiveWeapon(); print(w and tostring(WeaponObject.GetMagazineAmmoCount(w)) or "no weapon")'
    print('magazine', lua(mag).strip())
    sim = Sim()
    for _ in range(2):
        sim.trigger('right', 1.0)
        time.sleep(0.12)
        sim.trigger('right', 0.0)
        time.sleep(0.4)
    time.sleep(0.8)
    print('magazine after the shots', lua(mag).strip())
    sim.tap('left', 'x', hold=0.15)
    time.sleep(0.4)
    host_shot('reload-1')
    time.sleep(0.5)
    host_shot('reload-2')
    time.sleep(1.5)
    print(lua('local w = Game.GetPlayer():GetActiveWeapon(); if not w then print("no weapon") return end; '
              'print("magazine after " .. tostring(WeaponObject.GetMagazineAmmoCount(w)))'))


PSM_VEHICLE = ('local bb = Game.GetBlackboardSystem():GetLocalInstanced(Game.GetPlayer():GetEntityID(), '
               'GetAllBlackboardDefs().PlayerStateMachine); print(bb:GetInt(GetAllBlackboardDefs().PlayerStateMachine.Vehicle))')
CAR_AT = ('local v = Game.GetPlayer():GetMountedVehicle(); if v then local q = v:GetWorldPosition(); '
          'print(string.format("car %.2f %.2f %.2f, %.1f m/s", q.x, q.y, q.z, v:GetCurrentSpeed())) else print("not in a car") end')
PARTNER_AT = ('for _, peer in ipairs(NightCoopSdk.Peers()) do local e = Game.FindEntityByID(NightCoopSdk.PartnerBody(peer)); '
              'if e then local q = e:GetWorldPosition(); print(string.format("partner %.2f %.2f %.2f", q.x, q.y, q.z)) end end')


def car(args):
    """A parked car set with its driver's door where this player looks, entered with X as a player does, driven and
    steered with the test pad; host stills and both sides' positions.

    A scripted mount (MountingFacility, instant or the forced entry animation) leaves the player's state machine in
    its vehicle transition (4) for good, and a NightCoop guest's summon brings no car: the door and X it is."""
    from simxr import Sim
    from testpad import Pad
    sim = Sim()
    sim.head(yaw=0.0, pitch=-12.0)
    time.sleep(1.0)
    print(lua(f'''
local p = Game.GetPlayer()
local q = p:GetWorldPosition()
local cf = Game.GetCameraSystem():GetActiveCameraForward()
local n = math.sqrt(cf.x * cf.x + cf.y * cf.y)
local fx, fy = cf.x / n, cf.y / n
-- its left side to the player: the car's forward is the player's left
local spec = DynamicEntitySpec.new()
spec.recordID = TweakDBID.new("{CAR}")
spec.position = Vector4.new(q.x + fx * {args.door} - fy * 0.3, q.y + fy * {args.door} + fx * 0.3, q.z + 0.2, 1)
spec.orientation = EulerAngles.new(0, 0, math.deg(math.atan2(fy, fx))):ToQuat()
spec.alwaysSpawned = true
CVRMpCar = Game.GetDynamicEntitySystem():CreateEntity(spec)
print(string.format("car set {args.door} m ahead, its door to the player"))
'''))
    time.sleep(4.0)
    sim.tap('left', 'x', hold=0.25)
    for _ in range(8):
        time.sleep(1.0)
        state = lua(PSM_VEHICLE).strip()
        if state == '1':
            break
    print('player state machine vehicle', state, '(1 = driving)')
    if state != '1':
        return
    host_shot('car-in')
    print('start:', lua(CAR_AT).strip(), '| the host sees', lua(PARTNER_AT, home=True).strip())
    pad = Pad()
    print('game in front:', pad.front())
    pad.triggers(right=0.7, seconds=5.0)
    time.sleep(2.0)
    print('throttle:', lua(CAR_AT).strip(), '| the host sees', lua(PARTNER_AT, home=True).strip())
    pad.stick(x=-0.9, y=0.0, seconds=3.0)
    time.sleep(1.0)
    host_shot('car-steer-1')
    print('steering:', lua(CAR_AT).strip(), '| the host sees', lua(PARTNER_AT, home=True).strip())
    time.sleep(1.5)
    host_shot('car-steer-2')
    pad.triggers(left=1.0, seconds=2.0)
    time.sleep(2.0)
    pad.triggers(seconds=0.0)
    pad.stick(seconds=0.0)
    print('brake:', lua(CAR_AT).strip(), '| the host sees', lua(PARTNER_AT, home=True).strip())


PER_FRAME = '''
-- CETBridge's module: its Update wrapped once so a test can run a function every frame (CVRMpTick)
if not CVRMpTickHooked then
  local b = require("bridge")
  local up = b.Update
  b.Update = function(self, dt) up(self, dt); if CVRMpTick then pcall(CVRMpTick, dt) end end
  CVRMpTickHooked = true
end
'''


def prop(args):
    """The nearest street prop taken into this player's right hand and kept there (VRHeldBody asked every frame, as
    the carry script asks), host stills while it is held, then let go and one more still."""
    print(lua(PER_FRAME + f'''
local p = Game.GetPlayer():GetWorldPosition()
local b = Game.VRNearBody(Vector4.new(p.x, p.y, p.z + 0.9, 1), {args.radius}, 30.0)
-- VRNearBody: (kind, proxy, index within the proxy, its mass)
CVRMpProp = {{ math.floor(b.y + 0.5), math.floor(b.z + 0.5) }}
if b.y <= 0 then print("no prop near") return end
local ok = Game.VRHeldBody(CVRMpProp[1], CVRMpProp[2], 24, Vector4.new(0, 0, 0, 0), Vector4.new(0, 0, 0, 1))
CVRMpTick = function() Game.VRHeldBody(CVRMpProp[1], CVRMpProp[2], 24, Vector4.new(0, 0, 0, 0), Vector4.new(0, 0, 0, 1)) end
print(string.format("prop kind %d proxy %d index %d (%.2f kg) held: %s", b.x, CVRMpProp[1], CVRMpProp[2], b.w, tostring(ok)))
print(Game.VRBodyGraspShape(CVRMpProp[1], CVRMpProp[2]))
'''))
    time.sleep(3.0)
    host_shot('prop-held')
    # the hand moved while it holds: the partner's mirror has to follow
    subprocess_pose('reach', yaw=args.yaw)
    time.sleep(1.5)
    host_shot('prop-held-reach')
    log = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\cyberpunkvrport\log\cyberpunkvrport.log')
    print('here:', [l for l in log.read_text(encoding='utf-8', errors='replace').splitlines() if '[coop-props]' in l or 'steered into' in l][-4:])
    print('host:', home_pc.run(r"Select-String -Path 'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\cyberpunkvrport\log\cyberpunkvrport.log' -Pattern 'coop-props' | Select-Object -Last 4 | ForEach-Object Line").stdout.strip())
    if not args.keep:
        print(lua('CVRMpTick = nil; if CVRMpProp and CVRMpProp[1] > 0 then print(Game.VRHeldBodyLetGo(CVRMpProp[1], CVRMpProp[2], Vector4.new(0,0,0,0), Vector4.new(0,0,0,0))) end'))
        time.sleep(2.0)
        host_shot('prop-let-go')
        subprocess_pose('rest', yaw=args.yaw)


def subprocess_pose(name, yaw=0.0):
    """This PC's simulator hands into one of sim_pose.py's poses."""
    import subprocess
    r = subprocess.run([sys.executable, '-B', str(HERE / 'sim_pose.py'), 'pose', name, '--yaw', str(yaw)], capture_output=True, text=True)
    print('pose', name, (r.stdout + r.stderr).strip()[-200:])


def main():
    a = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = a.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('place')
    p.add_argument('--dist', type=float, default=2.5)
    sub.add_parser('weapon')
    sub.add_parser('reload')
    c = sub.add_parser('car')
    c.add_argument('--door', type=float, default=1.85, help="m from the player to the car's middle (its door within reach)")
    pr = sub.add_parser('prop')
    pr.add_argument('--radius', type=float, default=3.0)
    pr.add_argument('--keep', action='store_true', help='do not let it go')
    pr.add_argument('--yaw', type=float, default=0.0, help="the simulator's head yaw for the hand poses")
    s = sub.add_parser('shot')
    s.add_argument('name')
    args = a.parse_args()
    if args.cmd == 'shot':
        host_shot(args.name)
    else:
        dict(place=place, weapon=weapon, reload=reload, car=car, prop=prop)[args.cmd](args)


if __name__ == '__main__':
    main()
