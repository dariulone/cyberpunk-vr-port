# Hand interaction tests

Run outside the game; none of them proves anything about the engine. See `docs/hand-interaction-20261105.md`.

    python tools/hand_tests/grip_test.py
    luajit tools/hand_tests/world_stop_test.lua mods/cet/CyberpunkVRPort_HandCollision/init.lua

`grip_test.py` cuts the solver out of `src/Anim/NpcArmGrip.cpp` as it stands and runs it on a skeleton of man_base's shape:
limbs, head and chest, and how far a head turns however often a pass comes. It needs MSVC; set `VCVARS64` to your
`vcvars64.bat` if it is not in the default place.

The Lua test cuts the world stop out of `init.lua` and runs it against a wall made of arithmetic. It looks
for marker text in that file, so it fails loudly when the code they cut out has moved.
