# -*- coding: utf-8 -*-
"""The game's own sounds for the port's intro and its lobby, out of the Wwise banks, as files to listen to.

WHAT A SOUND IS IN THIS GAME. A Wwise event (base\\sound\\event\\eventsmetadata.json: its name, its 32-bit id, how long it
plays, its tags). The event lives in one of the soundbanks (audio_2_soundbanks.archive, base\\sound\\soundbanks\\*.bnk):
its actions name what they act on -- a Play names a sound, a random or switch container, a piece of interactive
music -- and those hold, down the tree, the media: .wem files embedded in a bank or streamed beside the banks as
base\\sound\\soundbanks\\<media id>.wem.

HOW THE TREE IS READ. Each bank's HIRC section lists objects (type, size, id, body). An event's body lists its
actions; an action's body names its target. Going DOWN from a container would mean parsing each node's base
parameters to reach its children list, which shifts with every Wwise version; every node also names its parent
near its start (its NodeBaseParams' DirectParentID), so the tree is read upwards instead and inverted: a node's
parent is the first id in its body that is a container of this game's banks. A sound's media id is its source's
(the first fields of its body), a music track's are its sources' (likewise).

Usage:
    python tools/vr_lobby_assets/game_sounds.py OUT [category=event,event ...]...

    OUT/<category>/<event>/<n>_<media>.ogg (or .wav), and OUT/index.txt (each event's duration, tags, media).
"""
import json
import re
import shutil
import struct
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\archive\pc')
CLI = Path.home() / '.dotnet/tools/cp77tools.exe'
WORK = ROOT / 'build' / 'sounds'

SOUND, ACTION, EVENT, MUSIC_TRACK = 2, 3, 4, 11
CONTAINERS = {5, 6, 7, 9, 10, 12, 13}     # random/sequence, switch, actor-mixer, layer, music segment/switch/playlist
PLAY = 0x04                               # an action's type, high byte

# What the port's intro and lobby could use: the boot's own music and glitches, the main menu's loop and music,
# the loading screens', the VR tutorial's (the lobby IS its hall), cyberspace's loops and transitions.
DEFAULT = {
    'intro': ['cp_bumpers_temp_sfx_music_long_start', 'cp_bumpers_temp_sfx_music_start', 'cp_intro_temp_sfx_music_start',
              'ui_splash_screen_glitch', 'q110_cyberspace_logo', 'q000_vr_tutorial_logo_fall',
              'q000_vr_tutorial_logo_disappear', 'global_cyberspace_transition_whoosh',
              'global_cyberspace_transition_roarer', 'global_cyberspace_teleport_whoosh', 'q110_cyberspace_enter',
              'q110_cyberspace_warp', 'q116_mikoshi_entering_cyberspace', 'global_cyberspace_time_freeze',
              'ui_e3_demo_cyberspace_01', 'q305_blackwall_horizon_start'],
    'lobby_ambient': ['ui_main_menu_loop_start', 'mus_mainmenu_play', 'mus_mainmenu_character_creation',
                      'q000_vr_tutorial_logo_loop', 'q110_cyberspace_loop_generic_quests',
                      'q110_cyberspace_loop_generic_quests_low', 'q110_cyberspace_warp_loop_start',
                      'q110_cyberspace_blackwall_drone', 'global_cyberspace_sarcophagus_loop_start',
                      'mus_sq029_vr_game_01_loop_START', 'q105_braindance_generator_studio_idle_loop',
                      'q105_braindance_generator_loop', 'ui_hologram_static_loop', 'q305_blackwall_core_vista_loop_start_01',
                      'q110_sc_blackwall_entities_loop', 'mus_q004_bd_edit_mode_start',
                      'mus_q110_maglev_05_cyberspace_01_start', 'lcm_g_cyberspace_start',
                      'global_cyberspace_alt_bridge_mikoshi_start'],
    'loading': ['ui_city_loading_screen', 'ui_city_loading_screen_loop_start', 'ls_mus_ow_atmosphere'],
    'menu_ui': ['ui_main_menu_open_tab', 'ui_main_menu_cc_loading', 'ui_main_menu_cc_confirmation_screen_open',
                'ui_main_menu_cc_confirmation', 'ui_character_customization_navigate'],
    # The lobby's menu panels: how one appears and goes, a press, a hand over a row, a word for the player, and
    # glitches for the intro's logo.
    'menu_open': ['ui_menu_open', 'ui_menu_enter', 'ui_menu_full_screen_open_01_Play', 'ui_gui_open',
                  'ui_gui_tab_character_open', 'ui_gui_tab_gear_open', 'ui_radial_menu_open', 'ui_hologram_turn_on',
                  'ui_terminal_turn_on', 'ui_tv_turn_on', 'ui_focus_mode_scanning_open', 'ui_shard_pane_open',
                  'ui_popup_lore_open', 'ui_menu_tutorial_open'],
    'menu_close': ['ui_menu_close', 'ui_menu_exit', 'ui_menu_full_screen_close_01_Play', 'ui_gui_close',
                   'ui_radial_menu_close', 'ui_hologram_turn_off', 'ui_terminal_turn_off', 'ui_tv_turn_off',
                   'ui_shard_panel_close', 'ui_popup_lore_close', 'ui_menu_tutorial_close',
                   'ui_main_menu_cc_confirmation_screen_close'],
    'menu_click': ['ui_menu_onpress', 'ui_menu_mouse_click', 'ui_terminal_positive', 'ui_terminal_negative',
                   'ui_menu_value_up', 'ui_menu_value_down', 'ui_gui_tab_change', 'ui_messenger_select',
                   'ui_menu_perk_buy', 'ui_menu_map_pin_on', 'ui_menu_map_pin_off', 'ui_tv_select_channel',
                   'ui_menu_full_screen_perk_espionage_select_01_Play', 'ui_menu_item_crafted'],
    'menu_hover': ['ui_menu_hover', 'ui_menu_full_screen_perk_espionage_blue_highlite_01_Play',
                   'ui_focus_mode_scanning_tag_on', 'ui_focus_mode_scanning_tag_off', 'ui_menu_map_pin_created',
                   'ui_menu_attributes_progress_bar_start'],
    'menu_notify': ['ui_messenger_recieved', 'ui_jingle_new_area', 'ui_jingle_press_to_continue',
                    'ui_jingle_quest_update', 'ui_menu_attributes_done'],
    'glitch': ['ui_splash_screen_glitch', 'ui_fx_burnout_glitch_fullscreen', 'ui_fx_hacking_glitch_fullscreen_heavy',
               'ui_fx_hacking_glitch_fullscreen_medium', 'ui_fx_hacking_glitch_fullscreen_low',
               'ui_fx_rezoagwe_glitch_fullscreen', 'vfx_fullscreen_reboot_glitch', 'q005_dex_logo_ui_reboot',
               'q101_sc_01_v_system_reboot_ui'],
}


def run(*args):
    r = subprocess.run([str(a) for a in args], capture_output=True, text=True)
    if r.returncode:
        print(r.stdout[-2000:], r.stderr[-2000:])
        raise SystemExit('failed: %s' % (args[:3],))
    return r.stdout


def events():
    """name -> (wwise id, max duration, tags) from the game's event metadata."""
    meta = WORK / 'meta' / 'base' / 'sound' / 'event' / 'eventsmetadata.json'
    if not meta.exists():
        run(CLI, 'unbundle', GAME / 'content' / 'audio_1_general.archive', '-o', WORK / 'meta', '-r',
            'eventsmetadata|soundbanks\\.json')
    js = WORK / 'json' / 'eventsmetadata.json.json'
    if not js.exists():
        run(CLI, 'convert', 'serialize', meta, '-o', WORK / 'json')
    out = {}
    for e in json.loads(js.read_text(encoding='utf-8'))['Data']['RootChunk']['root']['Data']['events']:
        out[e['redId']['$value']] = (int(e['wwiseId']), float(e.get('maxDuration') or 0.0),
                                     [t['$value'] for t in e.get('tags', [])])
    return out


def banks():
    folder = WORK / 'banks'
    if not any(folder.rglob('*.bnk')):
        run(CLI, 'unbundle', GAME / 'content' / 'audio_2_soundbanks.archive', '-o', folder, '-r', '\\.bnk$')
    return sorted(folder.rglob('*.bnk'))


def chunks(data):
    at = 0
    while at + 8 <= len(data):
        tag, size = data[at:at + 4], struct.unpack_from('<I', data, at + 4)[0]
        yield tag, at + 8, size
        at += 8 + size


def index(paths):
    """Every HIRC object of every bank (id -> (type, body, bank)), and every bank's embedded media
    (media id -> (bank path, offset, size))."""
    objects, embedded = {}, {}
    for path in paths:
        data = path.read_bytes()
        didx, data_at = [], None
        for tag, at, size in chunks(data):
            if tag == b'DIDX':
                for k in range(size // 12):
                    didx.append(struct.unpack_from('<III', data, at + 12 * k))
            elif tag == b'DATA':
                data_at = at
            elif tag == b'HIRC':
                count = struct.unpack_from('<I', data, at)[0]
                p = at + 4
                for _ in range(count):
                    kind, osize = struct.unpack_from('<BI', data, p)
                    body = data[p + 5:p + 5 + osize]
                    oid = struct.unpack_from('<I', body, 0)[0]
                    objects.setdefault(oid, (kind, body, path))
                    p += 5 + osize
        if data_at is not None:
            for mid, off, size in didx:
                embedded.setdefault(mid, (path, data_at + off, size))
    return objects, embedded


def parents(objects):
    """child id -> parent id: the first id in a node's body (past its own id and its sources) that is a container."""
    containers = {oid for oid, (kind, _, _) in objects.items() if kind in CONTAINERS}
    up = {}
    for oid, (kind, body, _) in objects.items():
        if kind not in CONTAINERS and kind not in (SOUND, MUSIC_TRACK):
            continue
        start = 4 + (14 if kind == SOUND else 0)
        if kind == MUSIC_TRACK:
            n = struct.unpack_from('<I', body, 5)[0] if len(body) >= 9 else 0
            start = 9 + 14 * n
        for p in range(start, len(body) - 3):
            v = struct.unpack_from('<I', body, p)[0]
            if v != oid and v in containers:
                up[oid] = v
                break
    down = {}
    for child, parent in up.items():
        down.setdefault(parent, []).append(child)
    return down


def media_of(oid, objects, down, seen=None):
    seen = set() if seen is None else seen
    if oid in seen or oid not in objects:
        return []
    seen.add(oid)
    kind, body, _ = objects[oid]
    out = []
    if kind == SOUND and len(body) >= 13:
        out.append(struct.unpack_from('<I', body, 9)[0])
    elif kind == MUSIC_TRACK and len(body) >= 9:
        n = struct.unpack_from('<I', body, 5)[0]
        for k in range(min(n, 64)):
            out.append(struct.unpack_from('<I', body, 9 + 14 * k + 5)[0])
    for child in down.get(oid, []):
        out += media_of(child, objects, down, seen)
    return out


def event_media(eid, objects, down):
    if eid not in objects or objects[eid][0] != EVENT:
        return None
    body = objects[eid][1]
    # The action count is a byte in this game's banks (a u32 in older ones): whichever fills the body exactly.
    n = body[4]
    ids = struct.unpack_from('<%dI' % n, body, 5) if 5 + 4 * n == len(body) else \
        struct.unpack_from('<%dI' % struct.unpack_from('<I', body, 4)[0], body, 8)
    out = []
    for aid in ids:
        if aid not in objects or objects[aid][0] != ACTION:
            continue
        ab = objects[aid][1]
        kind, target = struct.unpack_from('<HI', ab, 4)
        if kind >> 8 == PLAY:
            out += media_of(target, objects, down)
    return list(dict.fromkeys(out))


def uncooked(ids, out):
    """These media as files to listen to. Music and most interface sounds are Vorbis, streamed beside the banks as
    base/sound/soundbanks/media/<id>.wem: uncooked to Ogg. The sound effects (sfx_container.bnk's sounds,
    PCM in the banks) are Opus in sfx_container_<n>.opuspak, behind sfx_container.opusinfo: exported as .opus."""
    if not ids:
        return
    out.mkdir(parents=True, exist_ok=True)
    # Any character for the separator: the CLI's argument parser turns a doubled backslash into one, which leaves
    # "media\(" -- an escaped parenthesis and an invalid pattern.
    pattern = r'media.(%s)\.wem$' % '|'.join(str(i) for i in sorted(ids))
    for archive in sorted((GAME / 'content').glob('audio_*.archive')) + sorted((GAME / 'ep1').glob('audio_*.archive')):
        subprocess.run([str(CLI), 'uncook', str(archive), '-o', str(out), '-r', pattern], capture_output=True)
    subprocess.run([str(CLI), 'uncook', str(GAME / 'content' / 'audio_2_soundbanks.archive'), '-o', str(out),
                    '-r', r'sfx_container\.opusinfo$', '--opus-hashes', ','.join(str(i) for i in sorted(ids))],
                   capture_output=True)


PCM, EXTENSIBLE = 0x0001, 0xFFFE          # a .wem's format tag (Vorbis is 0xFFFF)


def wem_format(path):
    """(format tag, channels, rate, bits, data offset, data size) of a .wem's RIFF chunks."""
    with open(path, 'rb') as f:
        head = f.read(12)
        if head[:4] != b'RIFF' or head[8:12] != b'WAVE':
            return None
        fmt = None
        while True:
            chunk = f.read(8)
            if len(chunk) < 8:
                return None
            cid, size = chunk[:4], struct.unpack('<I', chunk[4:])[0]
            if cid == b'fmt ':
                tag, channels, rate, _, _, bits = struct.unpack('<HHIIHH', f.read(16))
                fmt = (tag, channels, rate, bits)
                f.seek(size - 16 + (size & 1), 1)
            elif cid == b'data':
                return fmt and fmt + (f.tell(), size)
            else:
                f.seek(size + (size & 1), 1)


def playable(folder):
    """The .wem the uncook leaves as they are, as files a player opens. Vorbis it turns into .Ogg itself; PCM -- the
    loading screens' music, Wwise's 24-byte extensible header round 16-bit samples -- becomes the same samples under
    a plain WAVE header."""
    for wem in sorted(folder.rglob('*.wem')):
        if any(wem.with_suffix(ext).exists() for ext in ('.Ogg', '.ogg', '.wav')):
            continue
        fmt = wem_format(wem)
        if fmt is None:
            print('%s: not a RIFF wave' % wem.name)
        elif fmt[0] in (PCM, EXTENSIBLE) and fmt[3] == 16:
            tag, channels, rate, bits, offset, size = fmt
            block = channels * bits // 8
            with open(wem, 'rb') as src, open(wem.with_suffix('.wav'), 'wb') as dst:
                dst.write(b'RIFF' + struct.pack('<I', 36 + size) + b'WAVE')
                dst.write(b'fmt ' + struct.pack('<IHHIIHH', 16, PCM, channels, rate, rate * block, block, bits))
                dst.write(b'data' + struct.pack('<I', size))
                src.seek(offset)
                shutil.copyfileobj(src, dst, 1 << 20)
        else:
            print('%s: format 0x%04X, %d bits, left as .wem' % (wem.name, fmt[0], fmt[3]))


def found(m, folder):
    for ext in ('.Ogg', '.ogg', '.wav', '.opus', '.wem'):
        hit = next(folder.rglob('%d%s' % (m, ext)), None)
        if hit is not None:
            return hit
    return None


def main():
    out = Path(sys.argv[1])
    wanted = dict(DEFAULT)
    for arg in sys.argv[2:]:
        cat, names = arg.split('=', 1)
        wanted[cat] = names.split(',')
    ev = events()
    objects, embedded = index(banks())
    down = parents(objects)
    print('%d objects, %d embedded media, %d events known' % (len(objects), len(embedded), len(ev)))
    plan, lines = [], []
    for cat, names in wanted.items():
        for name in names:
            if name not in ev:
                lines.append('%-14s %-48s not an event of this game' % (cat, name))
                continue
            eid, dur, tags = ev[name]
            media = event_media(eid, objects, down)
            lines.append('%-14s %-48s %6.1f s  %-40s %s' % (cat, name, dur, ','.join(tags)[:40],
                                                          'not in any bank' if media is None else
                                                          ' '.join(str(m) for m in media) or 'no media (state/switch)'))
            for n, m in enumerate(media or []):
                plan.append((cat, name, n, m))
    listen = WORK / 'listen'
    need = sorted({m for _, _, _, m in plan if found(m, listen) is None})
    # Embedded media (a bank's DATA) have no file of their own: written out as the .wem they are.
    for m in [m for m in need if m in embedded]:
        path, off, size = embedded[m]
        with open(path, 'rb') as f:
            f.seek(off)
            listen.mkdir(parents=True, exist_ok=True)
            (listen / ('%d.wem' % m)).write_bytes(f.read(size))
    uncooked([m for m in need if m not in embedded], listen)
    playable(listen)
    for cat, name, n, m in plan:
        src = found(m, listen)
        if src is None:
            lines.append('%-14s %-48s media %d missing' % (cat, name, m))
            continue
        dst = out / cat / name
        dst.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src, dst / ('%02d_%d%s' % (n, m, src.suffix.lower())))
    out.mkdir(parents=True, exist_ok=True)
    (out / 'index.txt').write_text('\n'.join(lines) + '\n', encoding='utf-8')
    print('\n'.join(lines))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
