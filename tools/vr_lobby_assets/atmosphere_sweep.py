"""Tries the main menu world's clock and weather on the live lobby and lays the results side by side, to choose
the atmosphere the Lobby mod holds (mods/cet/CyberpunkVRPort_Lobby/modules/lobby.lua S.hour / S.weather).

    py -3.13 tools/vr_lobby_assets/atmosphere_sweep.py OUT.png HOUR:WEATHER [HOUR:WEATHER ...] [--pitch P]

Each cell is the left eye looking ahead and P degrees up (default 25). The Lobby mod's own hold is paused while it
runs (its clock is pushed far ahead) and left paused: GetMod("CyberpunkVRPort_Lobby").hot() restores it.
"""
import sys
import time
from pathlib import Path

from PIL import Image, ImageDraw

sys.path.insert(0, str(Path(__file__).resolve().parent))
import game_views  # noqa: E402

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'build/shotgun-reload-20261019/harness'))
from common import lua  # noqa: E402


def main():
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    pitch = float(sys.argv[sys.argv.index('--pitch') + 1]) if '--pitch' in sys.argv else 25.0
    if '--pitch' in sys.argv:
        args.remove(sys.argv[sys.argv.index('--pitch') + 1])
    out, combos = args[0], args[1:]
    W = 300
    H = round(W * game_views.EYE_ASPECT)
    sheet = Image.new('RGB', (W * min(4, len(combos)), H * ((len(combos) + 3) // 4)), (0, 0, 0))
    game_views.head(0, pitch)
    try:
        for i, combo in enumerate(combos):
            hour, weather = combo.split(':')
            lua('GetMod("CyberpunkVRPort_Lobby").debug().clock = 1e9\n'
                'Game.GetTimeSystem():SetGameTimeByHMS(%d, 0, 0)\n'
                'Game.GetWeatherSystem():SetWeather(CName.new("%s"), 0.0, 0)' % (int(hour), weather))
            time.sleep(4.0)
            im = game_views.shot().resize((W, H))
            ImageDraw.Draw(im).text((6, 4), combo, fill=(255, 255, 0))
            sheet.paste(im, ((i % 4) * W, (i // 4) * H))
            print(combo, flush=True)
    finally:
        game_views.head(0, 0)
    sheet.save(out)
    print('->', out)


if __name__ == '__main__':
    main()
