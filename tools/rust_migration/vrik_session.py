"""The player's solve (VRIK) in the game, through the OpenXR Simulator's head-pose file channel: the probes the solve
exports -- the solved toes (CyberpunkVR_DebugToes), the body under the view (CyberpunkVR_DebugClipBody: the camera in
model space, the clip's camera bone and hips, the clip weight), the wrists' shortfall after the arm's caps
(CyberpunkVR_DebugGripShortMm) and the squat stance gate (CyberpunkVR_DebugSquatStance) -- averaged at rest, in a 0.35 m
squat and in a 0.25 m forward lean, then back at rest; and the hands' layers at work: the two-hand holds loaded
(CyberpunkVR_DebugTwoHandLoaded) and the grasp's palms measured (CyberpunkVR_DebugGrasp[2]). Prints one JSON line. The game must be in the world
(tools/quickboot_tests/launch_to_save.py) and is left running."""
import json
import time

from roomscale_session import glide, read, send, start  # the shared simulator/process plumbing


def probes():
    toes = read('CyberpunkVR_DebugToes', '<6f')
    body = read('CyberpunkVR_DebugClipBody', '<12f')
    short = read('CyberpunkVR_DebugGripShortMm', '<2f')
    stance = read('CyberpunkVR_DebugSquatStance', '<2f')
    return list(toes) + list(body[:9]) + [body[9]] + list(short) + [stance[1]]


NAMES = ['toe_r_x', 'toe_r_y', 'toe_r_z', 'toe_l_x', 'toe_l_y', 'toe_l_z', 'cam_x', 'cam_y', 'cam_z',
         'clipcam_x', 'clipcam_y', 'clipcam_z', 'cliphips_x', 'cliphips_y', 'cliphips_z', 'clip_w',
         'short_r_mm', 'short_l_mm', 'stance_w']


def average(seconds=1.0):
    samples = []
    end = time.time() + seconds
    while time.time() < end:
        samples.append(probes())
        time.sleep(.02)
    return {n: round(sum(s[i] for s in samples) / len(samples), 4) for i, n in enumerate(NAMES)}


result = {}
try:
    time.sleep(1.5)
    result['rest'] = average()
    glide(dy=-0.35, seconds=1.0)
    time.sleep(1.5)
    result['squat'] = average()
    glide(dy=0.35, seconds=1.0)
    glide(dz=-0.25, seconds=1.0)
    time.sleep(1.5)
    result['lean'] = average()
    glide(dz=0.25, seconds=1.0)
    time.sleep(1.5)
    result['rest_again'] = average()
    result['layers'] = {'two_hand_loaded': read('CyberpunkVR_DebugTwoHandLoaded', '<i')[0],
                        'grasp_palms': read('CyberpunkVR_DebugGrasp', '<4Q')[2]}
finally:
    send({axis: start[axis] for axis in ('x', 'y', 'z', 'yaw', 'pitch', 'roll')})
print(json.dumps(result))
