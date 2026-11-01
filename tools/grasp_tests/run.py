# run.py [names...]: the harness on the lobby's things (right hand), PPMs -> one PNG sheet per case + sheet.png.
import subprocess, sys, os
from PIL import Image
HERE = os.path.dirname(os.path.abspath(__file__))
I = ['0', '0', '0', '1', '0', '0']
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
    'book_turned': ['1', '0', '0.07', '0.10', '0.015', '0.3', '0.2', '0.1', '0.93', '0.03', '-0.04'],
    'left_book': ['0', '0', '0.10', '0.07', '0.015', '0', '0', '0', '1', '0', '0'],
    'left_can': ['0', '2', '0.033', '0.033', '0.061'] + I,
    # The toys' held parts (tools/vr_lobby_assets/lobby_toys.py GRASPS).
    'chibi':    ['1', '0', '0.055', '0.024', '0.0625'] + I,
    'skippy':   ['1', '2', '0.032', '0.032', '0.06'] + I,
}
names = sys.argv[1:] or list(CASES)
args = [os.path.join(HERE, 'harness.exe'), os.path.join(HERE, 'rig.txt'), os.path.join(HERE, 'out')]
for n in names: args += [n] + CASES[n]
os.makedirs(os.path.join(HERE, 'out'), exist_ok=True)
print(subprocess.run(args, capture_output=True, text=True).stdout)
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
