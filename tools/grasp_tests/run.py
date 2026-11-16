# run.py [names...]: the harness on the lobby's things (right hand), PPMs -> one PNG sheet per case + sheet.png.
import subprocess, sys, os
from PIL import Image
HERE = os.path.dirname(os.path.abspath(__file__))
I = ['0', '0', '0', '1', '0', '0', '0']
CASES = {
    'can':      ['1', '2', '0.033', '0.033', '0.061'] + I,
    'cube':     ['1', '0', '0.085', '0.085', '0.085'] + I,
    'sphere':   ['1', '1', '0.11', '0.11', '0.11'] + I,
    'shard':    ['1', '0', '0.055', '0.03', '0.008'] + I,
    'engram':   ['1', '0', '0.011', '0.0232', '0.002'] + I,
    'ball':     ['1', '1', '0.035', '0.035', '0.035'] + I,
    'cone':     ['1', '3', '0.045', '0.02', '0.07'] + I,
    'book':     ['1', '0', '0.10', '0.07', '0.015'] + I,
    'bottle':   ['1', '2', '0.024', '0.024', '0.10'] + I,
    'book_turned': ['1', '0', '0.07', '0.10', '0.015', '0.3', '0.2', '0.1', '0.93', '0.03', '-0.04', '0'],
    'left_book': ['0', '0', '0.10', '0.07', '0.015', '0', '0', '0', '1', '0', '0', '0'],
    'left_can': ['0', '2', '0.033', '0.033', '0.061'] + I,
    # The toys' held parts (tools/vr_lobby_assets/lobby_toys.py GRASPS).
    'chibi':    ['1', '0', '0.055', '0.024', '0.0625'] + I,
    'left_chibi': ['0', '0', '0.055', '0.024', '0.0625'] + I,
    'skippy':   ['1', '2', '0.032', '0.032', '0.06'] + I,
    # small things in three fingers: a car key's fob, a lipstick
    'fob':      ['1', '0', '0.03', '0.015', '0.0075'] + I,
    'lipstick': ['1', '2', '0.01', '0.01', '0.04'] + I,
    # a can taken by an end (GRASP_AXIS=n puts its axis along the palm's normal): its middle past the palm by more
    # than its half length; GRASP_UP=down from above (TOP), GRASP_UP=up from below (CRADLE)
    'can_end':  ['1', '2', '0.033', '0.033', '0.061', '0', '0', '0', '1', '0', '0', '0.08'],
    'bottle_end': ['1', '2', '0.024', '0.024', '0.10', '0', '0', '0', '1', '0', '0', '0.12'],
    # bottles by their hull (shape 5: body radius, neck radius, half height; GRASP_Z its axis's point under the palm,
    # GRASP_AXIS / GRASP_UP as for can_end): a beer bottle and a wine bottle
    'beer':     ['1', '5', '0.031', '0.013', '0.115'] + I,
    'wine':     ['1', '5', '0.037', '0.015', '0.15'] + I,
    'beer_end': ['1', '5', '0.031', '0.013', '0.115', '0', '0', '0', '1', '0', '0', '0.13'],
    # a person's parts as the physics hands hold them (shape 10 + the style: 1 a can, 2 wide, 3 gently, 4 the palm; half:
    # the capsule's radius, -, its half length), across the palm; GRASP_PAXIS=d along the fingers
    'p_forearm': ['1', '11', '0.036', '0', '0.12'] + I,
    'p_uparm':   ['1', '11', '0.044', '0', '0.13'] + I,
    'p_neck':    ['1', '12', '0.048', '0', '0.07'] + I,
    'p_hand':    ['1', '13', '0.029', '0', '0.045'] + I,
    'p_face':    ['1', '14', '0.048', '0', '0.06'] + I,
    'p_thigh':   ['1', '11', '0.066', '0', '0.20'] + I,
    'p_trunk':   ['1', '14', '0.092', '0', '0.09'] + I,
    'p_shoulder': ['1', '11', '0.05', '0', '0.08'] + I,
    # a dead body's limbs as the carry closes on them (the part's radius + K.CORPSE_WRAP_WIDER): a shin, a forearm
    'p_shin':    ['1', '11', '0.072', '0', '0.06'] + I,
    'p_dforearm': ['1', '11', '0.046', '0', '0.06'] + I,
    'p_live_fixed': ['1', '11', '0.066', '0', '0.06'] + I,
    'left_p_shin': ['0', '11', '0.072', '0', '0.06'] + I,
    'left_p_dforearm': ['0', '11', '0.046', '0', '0.06'] + I,
}
names = sys.argv[1:]
regress = '--regress' in names
if regress:
    names.remove('--regress')
names = names or list(CASES)
args = [os.path.join(HERE, 'harness.exe'), os.path.join(HERE, 'rig.txt'), os.path.join(HERE, 'out')]
for n in names: args += [n] + CASES[n]
os.makedirs(os.path.join(HERE, 'out'), exist_ok=True)
env = os.environ.copy()
if regress:
    env['GRASP_REGRESS'] = '1'
    env['GRASP_NO_RENDER'] = '1'
result = subprocess.run(args, capture_output=True, text=True, env=env)
print(result.stdout, end='')
if result.stderr:
    print(result.stderr, file=sys.stderr, end='')
if result.returncode:
    sys.exit(result.returncode)
if regress and 'REGRESSION PASS:' not in result.stdout:
    raise SystemExit('Harness did not run regression assertions; rebuild harness.exe.')
if env.get('GRASP_NO_RENDER'):
    sys.exit(0)
ims = []
for n in names:
    p = os.path.join(HERE, 'out', n + '.ppm')
    if os.path.exists(p):
        im = Image.open(p); im.save(os.path.join(HERE, 'out', n + '.png')); ims.append(im)
if ims:
    W, H = ims[0].size
    sheet = Image.new('RGB', (W, H * len(ims)))
    for i, im in enumerate(ims): sheet.paste(im, (0, H * i))
    sheet.save(os.path.join(HERE, 'out', 'sheet.png'))
