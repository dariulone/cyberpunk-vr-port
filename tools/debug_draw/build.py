"""Builds Debug Draw (externals/cp2077_debug_draw, Jack Humbert, MIT): a RED4ext plugin that turns the game's own
debug renderer back on and gives scripts lines, boxes, spheres and text drawn in the world.

    py -3.13 -B tools/debug_draw/build.py            # configure once, build RelWithDebInfo (its copy step wants the PDB), run its unit tests

Its own build, not part of the port's: it brings its own RED4ext.SDK (the author's fork, branch new-types) and its
cmake helpers as submodules, which take `git -c core.longpaths=true -c submodule.active=. submodule update --init`
inside externals/cp2077_debug_draw. The plugin, its scripts, shaders and archive land in
externals/cp2077_debug_draw/game_dir as the game lays them out; tools/patchday/test_install.py deploy-port copies them.
"""
import os
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SOURCE = os.path.join(REPO, 'externals', 'cp2077_debug_draw')
BUILD = os.path.join(REPO, 'build', 'debug_draw')
GENERATOR = 'Visual Studio 18 2026'


def run(args):
    print('>', ' '.join(args), flush=True)
    r = subprocess.run(args, cwd=REPO)
    if r.returncode:
        sys.exit(r.returncode)


def main():
    if not os.path.isfile(os.path.join(SOURCE, 'deps', 'red4ext.sdk', 'CMakeLists.txt')):
        sys.exit('externals/cp2077_debug_draw has no deps: run the submodule update named in this file\'s doc')
    if not os.path.isfile(os.path.join(BUILD, 'CMakeCache.txt')):
        run(['cmake', '-S', SOURCE, '-B', BUILD, '-G', GENERATOR, '-A', 'x64', '-DDEBUG_DRAW_TESTS=ON'])
    run(['cmake', '--build', BUILD, '--config', 'RelWithDebInfo', '--parallel', '8', '--', '/nodeReuse:false'])
    run(['ctest', '--test-dir', BUILD, '-C', 'RelWithDebInfo', '--output-on-failure'])
    game_dir = os.path.join(SOURCE, 'game_dir')
    # THE SCRIPTS. The plugin loads red4ext/plugins/debug_draw/packed.reds (sdk->scripts->Add). Its cmake makes that
    # file with a custom language the Visual Studio generator does not build, so it is packed here with the author's
    # own pure-CMake script (the one their CI build runs): a header, then every .reds under src/redscript.
    helpers = os.path.join(SOURCE, 'deps', 'cyberpunk_cmake')
    run(['cmake', '-DCOMMENT_SLUG=//', '-DGLOB_EXT=reds',
         '-DHEADER_FILE=' + os.path.join(helpers, 'files', 'Header.txt'),
         '-DPACKED_FILE=' + os.path.join(game_dir, 'red4ext', 'plugins', 'debug_draw', 'packed.reds'),
         '-DSEARCH_FOLDER=' + os.path.join(SOURCE, 'src', 'redscript'),
         '-P', os.path.join(helpers, 'scripts', 'PackFiles.cmake')])
    for d, _, ns in os.walk(game_dir):
        for n in sorted(ns):
            print(os.path.relpath(os.path.join(d, n), game_dir).replace('\\', '/'))


if __name__ == '__main__':
    main()
