"""The game's boot and loading screens, taken out of the headset: the lobby is where the game starts (its own intro,
modules/intro.lua in the Lobby mod), and a flat screen shown in VR is one picture stretched over both eyes.

    py -3.13 -B tools/vr_lobby_assets/boot_screens.py           extract, edit, pack
    py -3.13 -B tools/vr_lobby_assets/boot_screens.py extract   the vanilla files as WolvenKit JSON (build/boot-screens)
    py -3.13 -B tools/vr_lobby_assets/boot_screens.py edit      the edited JSON -> mods/archive/source/raw/base/...
    py -3.13 -B tools/vr_lobby_assets/boot_screens.py pack      the raw JSON -> the pack folder -> the port's archive

THE SPLASH (base\\gameplay\\gui\\fullscreen\\splash_screen_loading, the loading layer's SplashScreen screen, ui_game.inklayers):
its native controller, inkSplashScreenLoadingScreenLogicController, plays its videos through its animations -- an
inkanimVideoInterpolator runs each bink for its duration (the logo train 12 s, the long one 15 s, the localised
legal message 9 s, the trailer 56 s with its titles faded over it) -- and goes on when an animation ends. Every
video interpolator is given duration 0 and every other interpolator and the music cues are taken out, so each
step ends as it starts; every bink it names is the game's own 26-frame tutorial placeholder
(base\\movies\\tutorials\\720x405\\dummy_720x405.bk2, 3 KB) instead of a full-screen video, so none is streamed. The
legal message the splash shows is the intro's to show, in the lobby.

What is left of it is not drawn: its root widget is hidden (visible 0) -- the placeholder's frames, the loading
spinner it shows while the game boots and the "press to continue" panel it shows once the boot is done. The
controller runs as it does; it shows and hides that panel, never its root, and an animation changes opacities,
never visibility. The CET mods are not up yet while it shows, so this has to be the archive's. The loading screens
that come later (a save, fast travel) hang in the headset's dark before the player (src/Render/LoadingScreen.cpp), and
the start screen that would follow the splash is skipped by the plugin (src/Lobby/StartScreen.cpp).

THE BOOT'S LOADING SCREEN (the loading screen the game shows while it boots into the main menu) loops the video the
game's ink settings name (base\\gameplay\\gui\\cp_2077.inkgamesettings, initLoadingScreenLogoLoopVideo): the game's
logo on red. It loops the intro's first stage instead, the legal notice's bink -- the English one here; the plugin
names the player's own language's as the settings load, and draws the port's word under the screen (src/Lobby/
Notice.cpp).
"""
import json
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import make_lobby_scene as MS  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
WORK = ROOT / 'build/boot-screens'
RAW = ROOT / 'mods/archive/source/raw'
DUMMY_BINK = 'base\\movies\\tutorials\\720x405\\dummy_720x405.bk2'
SPLASH = 'base\\gameplay\\gui\\fullscreen\\splash_screen_loading\\splash_screen_loading.inkwidget'
SPLASH_ANIMS = 'base\\gameplay\\gui\\fullscreen\\splash_screen_loading\\splash_screen_loading_animations.inkanim'
INK_SETTINGS = 'base\\gameplay\\gui\\cp_2077.inkgamesettings'
NOTICE_BINK = 'base\\movies\\fullscreen\\common\\cyberpunk2077_game_intro_message_en.bk2'
FILES = [SPLASH, SPLASH_ANIMS, INK_SETTINGS]
ARCHIVES = [GAME / 'archive/pc/content' / (n + '.archive') for n in
            ('basegame_1_engine', 'basegame_2_mainmenu', 'basegame_4_gamedata')]


def rel(depot):
    return Path(*depot.split('\\'))


def vanilla_json(depot):
    return WORK / 'json' / rel(depot).parent / (rel(depot).name + '.json')


def cmd_extract():
    names = '|'.join(rel(f).name.replace('.', '\\.') for f in FILES)
    for archive in ARCHIVES:
        MS.run(MS.CLI, 'unbundle', archive, '-o', WORK / 'vanilla', '-r', '(%s)$' % names)
    for depot in FILES:
        src = WORK / 'vanilla' / rel(depot)
        if not src.exists():
            raise SystemExit('not in the game: ' + depot)
        out = vanilla_json(depot).parent
        out.mkdir(parents=True, exist_ok=True)
        MS.run(MS.CLI, 'convert', 'serialize', src, '-o', out)
    print('extract: %d files -> %s' % (len(FILES), WORK / 'json'))


def depot_value(ref):
    path = ref.get('DepotPath') if isinstance(ref, dict) else None
    return path.get('$value') if isinstance(path, dict) else None


def binks_to_dummy(node):
    """Every bink reference under node -> DUMMY_BINK; how many."""
    n = 0
    if isinstance(node, dict):
        value = depot_value(node)
        if value and value.lower().endswith('.bk2'):
            node['DepotPath']['$value'] = DUMMY_BINK
            return 1
        for v in node.values():
            n += binks_to_dummy(v)
    elif isinstance(node, list):
        for v in node:
            n += binks_to_dummy(v)
    return n


def edit_splash(doc):
    """Its binks -> the placeholder, its root hidden, its controller straight to the end; (binks, roots, controllers)."""
    return binks_to_dummy(doc['Data']['RootChunk']), hide_roots(doc), skip_to_end(doc)


# The controller's steps (the logo train, the long one, the localised message, the game's intro) each name the
# animation it plays; every one names the animation the skip button plays instead, as "No Intro Videos" does at
# OnInitialize (r6/scripts/No Intro Videos - redscript). Here it is the widget's own data, so it holds without that
# script and agrees with it when it is installed: both set the same names, and no second script means no second
# definition of its method for redscript to refuse.
SPLASH_STEPS = ('logosTrainAnimation', 'longLogosTrainAnimation', 'localizedMessageAnimation', 'gameIntroAnimation')
SKIPPED = 'after_skip_pressed'


def skip_to_end(node):
    """Every splash controller's steps -> SKIPPED; how many controllers."""
    n = 0
    if isinstance(node, dict):
        if node.get('$type') == 'inkSplashScreenLoadingScreenLogicController':
            for name in SPLASH_STEPS:
                node[name]['$value'] = SKIPPED
            n += 1
        for v in node.values():
            n += skip_to_end(v)
    elif isinstance(node, list):
        for v in node:
            n += skip_to_end(v)
    return n


def edit_splash_anims(doc):
    """Video interpolators to duration 0, the rest of each definition's interpolators out, its sound cues silenced."""
    videos = 0
    for seq in doc['Data']['RootChunk']['sequences']:
        for definition in seq['Data']['definitions']:
            d = definition['Data']
            kept = [i for i in d.get('interpolators', []) if i['Data']['$type'] == 'inkanimVideoInterpolator']
            for i in kept:
                i['Data']['duration'] = 0
            videos += len(kept)
            d['interpolators'] = kept
            for e in d.get('events', []):
                if 'soundEventName' in e['Data']:
                    e['Data']['soundEventName']['$value'] = 'None'
    return videos


def edit_ink_settings(doc):
    """The boot's loading screen loops the notice: its logo loop video -> the legal notice's bink."""
    video = doc['Data']['RootChunk']['initLoadingScreenLogoLoopVideo']
    video['DepotPath']['$value'] = NOTICE_BINK
    return NOTICE_BINK


def handles(node, out):
    """{HandleId: the handle's Data} of every handle under node."""
    if isinstance(node, dict):
        if 'HandleId' in node and isinstance(node.get('Data'), dict):
            out[node['HandleId']] = node['Data']
        for v in node.values():
            handles(v, out)
    elif isinstance(node, list):
        for v in node:
            handles(v, out)
    return out


def hide_roots(doc):
    """Each library item's root widget hidden, in both of the item's copies (the buffer is the one the game reads).
    A root is a handle, or a reference to one declared elsewhere in the same copy (HandleRefId)."""
    n = 0
    for item in doc['Data']['RootChunk']['libraryItems']:
        copies = []
        package = (item.get('package') or {}).get('Data') or {}
        chunk = (package.get('File') or {}).get('RootChunk')
        if chunk:
            copies.append((package, chunk))
        buffer = (item.get('packageData') or {}).get('Data') or {}
        copies += [(buffer, c) for c in buffer.get('Chunks') or []]
        for tree, chunk in copies:
            root = chunk.get('rootWidget')
            if not isinstance(root, dict):
                continue
            data = root.get('Data') if 'Data' in root else handles(tree, {}).get(root.get('HandleRefId'))
            if isinstance(data, dict) and '$type' in data:
                data['visible'] = 0
                n += 1
    return n


def cmd_edit():
    for depot in FILES:
        doc = json.loads(vanilla_json(depot).read_text(encoding='utf-8-sig'))
        if depot == SPLASH:
            what = '%d binks -> the placeholder, %d root widgets hidden, %d controllers to the end' % edit_splash(doc)
        elif depot == INK_SETTINGS:
            what = 'the boot loading screen loops ' + edit_ink_settings(doc)
        else:
            what = '%d video interpolators at 0 s, the rest out' % edit_splash_anims(doc)
        doc['Header']['ArchiveFileName'] = ''
        out = RAW / rel(depot).parent / (rel(depot).name + '.json')
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(doc, ensure_ascii=False, indent=1) + '\n', encoding='utf-8', newline='\n')
        print('edit %-40s %s' % (rel(depot).name, what))


def cmd_pack():
    for depot in FILES:
        src = RAW / rel(depot).parent / (rel(depot).name + '.json')
        tmp = WORK / 'deserialize' / rel(depot).parent
        if tmp.exists():
            shutil.rmtree(tmp)
        tmp.mkdir(parents=True)
        shutil.copyfile(src, tmp / src.name)
        target = MS.PACK / rel(depot).parent
        target.mkdir(parents=True, exist_ok=True)
        MS.run(MS.CLI, 'convert', 'deserialize', tmp, '-o', target)
        if not (target / rel(depot).name).exists():
            raise SystemExit('deserialize gave no ' + depot)
    MS.pack_archive()


if __name__ == '__main__':
    steps = sys.argv[1:] or ['extract', 'edit', 'pack']
    for step in steps:
        {'extract': cmd_extract, 'edit': cmd_edit, 'pack': cmd_pack}[step]()
