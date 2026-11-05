# -*- coding: utf-8 -*-
"""The lobby's own sounds, out of the game: the files mods/cet/CyberpunkVRPort_Lobby/sounds/*.ogg that the lobby plays
through the port (modules/sound.lua -> VRSoundChannel / VRMenuPlayWav, src/Audio/PortSound.cpp).

Each is one medium of one of the game's Wwise events, picked by ear from what game_sounds.py exports
(build/sounds/candidates): the event and the medium's place in its list. The music is streamed Vorbis in the game
and is shipped as the game has it; the sound effects are Opus in the game, exported as WAV, and are shipped as Ogg
Vorbis (ffmpeg's libvorbis, quality 6) -- the port decodes Ogg Vorbis (externals/stb), not Opus.

Usage:
    python tools/vr_lobby_assets/lobby_sounds.py [--ffmpeg PATH] [--force]
"""
import argparse
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
import game_sounds  # noqa: E402

OUT = ROOT / 'mods' / 'cet' / 'CyberpunkVRPort_Lobby' / 'sounds'
WORK = ROOT / 'build' / 'sounds' / 'lobby'

# file -> (event, medium index): what the intro's stages and the lobby play.
SOUNDS = {
    'intro_loop.ogg': ('ui_e3_demo_cyberspace_01', 0),       # the notice, muffled, looped: the game is loading
    'logo_whoosh.ogg': ('global_cyberspace_teleport_whoosh', 0),  # the animated logo appears
    'logo_glitch.ogg': ('ui_splash_screen_glitch', 0),       # the logo's glitch: the boot splash's own
    'ambient.ogg': ('mus_sq029_vr_game_01_loop_START', 1),   # the lobby: a VR game's music, its second layer
    'menu_appear.ogg': ('ui_hologram_turn_on', 0),           # the lobby's holographic menu comes up
    'menu_hover.ogg': ('ui_menu_hover', 0),                  # a hand comes over a row: the game's menu's own
}

FFMPEG_GUESSES = [r'C:\Program Files\Virtual Desktop Streamer\ffmpeg.exe']


def ffmpeg_path(given):
    for p in [given] + [shutil.which('ffmpeg')] + FFMPEG_GUESSES:
        if p and Path(p).exists():
            return p
    raise SystemExit('ffmpeg not found: pass --ffmpeg PATH')


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--ffmpeg')
    ap.add_argument('--force', action='store_true', help='make every file again')
    args = ap.parse_args()
    ffmpeg = ffmpeg_path(args.ffmpeg)
    events = sorted({e for e, _ in SOUNDS.values()})
    sys.argv = [sys.argv[0], str(WORK), 'lobby=' + ','.join(events)]
    game_sounds.DEFAULT.clear()
    game_sounds.main()
    OUT.mkdir(parents=True, exist_ok=True)
    for name, (event, index) in SOUNDS.items():
        found = sorted((WORK / 'lobby' / event).glob('%02d_*' % index))
        if not found:
            raise SystemExit('%s: medium %d of %s was not exported' % (name, index, event))
        src, dst = found[0], OUT / name
        # An encoder run gives other bytes each time (the stream's serial): a file made before stays unless asked.
        if dst.exists() and not args.force:
            print('%-16s kept' % name)
            continue
        if src.suffix.lower() == '.ogg':
            shutil.copyfile(src, dst)
        else:
            subprocess.run([ffmpeg, '-y', '-hide_banner', '-loglevel', 'error', '-i', str(src), '-c:a', 'libvorbis',
                            '-q:a', '6', str(dst)], check=True)
        print('%-16s <- %s %s (%d KB)' % (name, event, src.name, dst.stat().st_size // 1024))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
