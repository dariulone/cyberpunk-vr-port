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

THE GAME'S HAND (default; GRASP_MESH=0 draws the solver's capsules and palm slab instead):
handmesh.py RIG_JSON mesh GLB_R GLB_L
                         unpacks the first-person hands (a0_000_pwa_fpp__full_r/_l.glb as cp77tools uncooks them,
                         e.g. build/hands-only-20261103/trim/source/...) and the anim rig's bone names
                         (player_woman_skeleton.rig as JSON) into mesh/hand_r.bin, hand_l.bin, rig_names.txt
handmesh.hpp             loads them (bind and vertices into the hand's frame; glTF's Y-up turned back), skins them to
                         the solved fingers (muscle joints half way between the bones across their knuckle), rasterizes
                         them over the raymarched thing (thumb red, little finger green; GRASP_TINT=0 skin only), and
                         prints "skin": the real skin's least clearance to the thing per finger (under 0 into it) and
                         "deepest": the joint of each finger's deepest vertex
GRASP_FIT=1              each segment's skin round its bone at the rig's reference (toward the palm the way the hinge
                         bends it, the back, the sides; what kFingers' radius/palm and kThumbRadius/kThumbPalm are), and
                         the real palm's height over the solver's palm plane on a grid
Views: the thumb's side, the little finger's side, the fingertips, 3/4 into the palm, the back of the hand (GRASP_W size).
