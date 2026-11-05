"""holster_showcase.py OUT_DIR: every holster place with a weapon in it, shot from around the body.

The camera leaves the head: the player's FPP camera component takes a local offset (the port puts the headset's
pose on top of it, so the offset carries the view out of the body), the body stops turning after the head
(xr_hybrid_body_rotation 0 in vrport.ini, restored at the end) and the simulator's head turns to look back at V.
The hands hang at the sides in tracking space, so they stay with the body while the head looks round.

With "Valentino Holsters" installed the shells are on (modules/shells.lua): the close views (arms held out) show
them -- the leg holster, the harness's holster under the arm, the chest knife sheath.

Three loadouts fill the places (modules/placements.lua): pistols + a blade (right hip, under the left arm, left
hip), long guns (both shoulders, the lower back), knives + a blade (chest, right thigh, left hip). Each is shot from
the front, both sides and the back; one contact sheet per loadout and an index of every file.

Needs the game on a save with the player standing in the open (the H10 test save), the port's Lua host bridge
(build/shotgun-reload-20261019/harness) and the simulator harness (simxr.py, this folder).
"""
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, r'C:\Users\dariulone\Desktop\CyberpunkVRPort\build\shotgun-reload-20261019\harness')
from simxr import Sim            # noqa: E402
from common import lua           # noqa: E402
from PIL import Image, ImageDraw  # noqa: E402

INI = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\red4ext\plugins\cyberpunkvrport\config\vrport.ini')

LOADOUTS = [
    ('pistols', ['Items.Preset_Overture_Default', 'Items.Preset_Unity_Default', 'Items.Preset_Katana_Default']),
    ('long', ['Items.Preset_Ajax_Default', 'Items.Preset_Carnage_Default', 'Items.Preset_Saratoga_Default']),
    ('knives', ['Items.Preset_Knife_Default', 'Items.Preset_Tanto_Default', 'Items.Preset_Machete_Default']),
]
# SHOWCASE_LOADOUTS='[["name", ["Items.X", ...]], ...]' replaces them (e.g. the other handgun kinds' leg shells).
if os.environ.get('SHOWCASE_LOADOUTS'):
    LOADOUTS = [(n, list(i)) for n, i in json.loads(os.environ['SHOWCASE_LOADOUTS'])]
# (name, camera local offset x right / y forward / z up from the head, head yaw (+ left), pitch)
# The side views' yaws are measured, not a right angle: the offset camera's view swings about the head as it turns,
# and at +-90 the body sat on the picture's edge (side_try / side_b, 2026-11-03).
VIEWS = [
    ('front', (0.0, 1.55, -0.50), 180, -12),
    ('side_a', (1.30, 0.05, -0.50), -115, -12),
    ('back', (0.0, -1.45, -0.40), 0, -14),
    ('side_b', (-1.30, 0.05, -0.50), 120, -12),
    ('belt_close', (0.0, 0.85, -0.70), 180, 0),
]
# Close views with the arms held out to the sides (they hide the hips and the ribs otherwise), each eye 2400 px: the
# chest, the drawing hip from the front, the drawing side.
CLOSE = [
    ('chest_close', (0.0, 0.55, -0.15), 180, -8),
    ('hip_close', (0.25, 0.55, -0.55), 165, -20),
    ('side_close', (-0.62, 0.10, -0.20), 118, -8),
]

# Whatever the equipping put in a hand goes back on the body (the third weapon slot comes out drawn).
PUT_AWAY = """
local p = Game.GetPlayer()
local w = p:GetActiveWeapon()
if w ~= nil then
  if w:IsMelee() then p:VRHolsterMelee() else p:VRUnequipWeapon() end
  print("put away")
else
  print("hands empty")
end
"""

EQUIP = """
local p = Game.GetPlayer()
local ts = Game.GetTransactionSystem()
local es = Game.GetScriptableSystemsContainer():Get(CName.new("EquipmentSystem"))
local names = %s
for i, n in ipairs(names) do
  local id = ItemID.FromTDBID(TweakDBID.new(n))
  if not ts:HasItem(p, id) then ts:GiveItem(p, id, 1) end
  local r = EquipRequest.new()
  r.owner = p
  r.itemID = id
  r.slotIndex = i - 1
  r.addToInventory = false
  es:QueueRequest(r)
end
print("equipped")
"""

PLACES = """
local st = GetMod("CyberpunkVRPort_Holster").state()
local out = {}
for _, c in ipairs(st.copies) do out[#out + 1] = c.item .. "@" .. c.place .. (c.shown and "" or "(hidden)") end
print(table.concat(out, " "))
"""


def set_ini(key, value):
    """One key of vrport.ini, the file's own line ends kept (it is CRLF)."""
    text = INI.read_bytes().decode('utf-8')
    nl = '\r\n' if '\r\n' in text else '\n'
    lines = [(key + '=' + value) if l.startswith(key + '=') else l for l in text.splitlines()]
    INI.write_bytes((nl.join(lines) + nl).encode('utf-8'))


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else HERE / 'showcase')
    out.mkdir(parents=True, exist_ok=True)
    backup = out / 'vrport.ini.before'
    shutil.copyfile(INI, backup)
    s = Sim()
    index = []
    try:
        set_ini('xr_hybrid_body_rotation', '0')
        time.sleep(1.0)
        s.hand('right', pos=(0.21, 0.88, 0.06), ypr=(0, -88, 0), mode='world')
        s.hand('left', pos=(-0.21, 0.88, 0.06), ypr=(0, -88, 0), mode='world')
        for name, items in LOADOUTS:
            print(lua(EQUIP % ('{' + ','.join(json.dumps(n) for n in items) + '}'), 15))
            time.sleep(3.0)
            for _ in range(3):
                said = lua(PUT_AWAY, 15)
                if 'hands empty' in said:
                    break
                time.sleep(2.0)
            time.sleep(1.0)
            places = lua(PLACES, 15)
            print(name, places)
            shots = []
            for view, cam, yaw, pitch in VIEWS:
                lua('Game.GetPlayer():GetFPPCameraComponent():SetLocalPosition(Vector4.new(%f, %f, %f, 1))' % cam, 10)
                s.head(yaw, pitch)
                time.sleep(1.2)
                f = None
                for _ in range(3):
                    try:
                        f = s.shot(str(out / ('%s_%s' % (name, view))), eyes='left', eye_size=1400)[0]
                        break
                    except TimeoutError:
                        print('shot timeout, retry')
                if f:
                    shots.append((view, f))
                    index.append({'loadout': name, 'view': view, 'file': f, 'places': places})
            s.hand('right', pos=(0.72, 1.38, 0.05), ypr=(0, 0, 0), mode='world')
            s.hand('left', pos=(-0.72, 1.38, 0.05), ypr=(0, 0, 0), mode='world')
            for view, cam, yaw, pitch in CLOSE:
                lua('Game.GetPlayer():GetFPPCameraComponent():SetLocalPosition(Vector4.new(%f, %f, %f, 1))' % cam, 10)
                s.head(yaw, pitch)
                time.sleep(1.5)
                for _ in range(3):
                    try:
                        f = s.shot(str(out / ('%s_%s' % (name, view))), eyes='left', eye_size=2400)[0]
                        index.append({'loadout': name, 'view': view, 'file': f, 'places': places})
                        break
                    except TimeoutError:
                        print('shot timeout, retry')
            s.hand('right', pos=(0.21, 0.88, 0.06), ypr=(0, -88, 0), mode='world')
            s.hand('left', pos=(-0.21, 0.88, 0.06), ypr=(0, -88, 0), mode='world')
            # one sheet per loadout
            ims = [(v, Image.open(f)) for v, f in shots]
            if ims:
                w, h = ims[0][1].size
                tw, th = w * 2 // 5, h * 2 // 5
                sheet = Image.new('RGB', (tw * len(ims), th + 22), (16, 16, 18))
                d = ImageDraw.Draw(sheet)
                for i, (v, im) in enumerate(ims):
                    sheet.paste(im.resize((tw, th)), (i * tw, 22))
                    d.text((i * tw + 6, 4), '%s / %s' % (name, v), fill=(255, 255, 255))
                sheet.save(out / ('%s_sheet.jpg' % name), quality=88)
    finally:
        lua('Game.GetPlayer():GetFPPCameraComponent():SetLocalPosition(Vector4.new(0, 0, 0, 1))', 10)
        shutil.copyfile(backup, INI)
        s.head(0, 0)
        s.restore()
    (out / 'index.json').write_text(json.dumps(index, indent=1), encoding='utf-8')
    print('done', len(index), 'shots in', out)


if __name__ == '__main__':
    main()
