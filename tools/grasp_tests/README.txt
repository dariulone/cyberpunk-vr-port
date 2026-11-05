Offline harness for src/Anim/GraspPose.cpp (the lobby's procedural grasp).

rigdump.py PID rig.txt   the live plugin's reference hands (both hands' subtrees: parent, local pos/rot,
                         ladder finger slots, hand/slot bones), read through the PDB (tools/swimming_tests/live_symbols.py)
build.bat                cl /O2: harness.cpp #includes the REAL src/Anim/GraspPose.cpp with stub/ headers
run.py [names]           cases (right hand unless left_*): seat + close exactly as in the game, prints kind,
                         thumb cost, joint angles and each segment's gap to the thing (mm); out/<name>.png =
                         the solver's own model (finger capsules, palm slab, thing) raymarched from the thumb's side,
                         the little finger's side, the fingertips, and a 3/4 view into the palm; out/sheet.png.
Case args: name side shape hx hy hz qx qy qz qw offD offT  (shape 0 box 1 sphere 2 cylinder 3 cone;
q = the thing's turn in the slot when taken; off = the grab point off the palm centre along the fingers / thumb).
