NPC grip joint checks
=====================

cmake -S tools/npc_grip_tests -B build/npc_grip_tests -A x64
cmake --build build/npc_grip_tests --config Release
ctest --test-dir build/npc_grip_tests -C Release --output-on-failure

The executable includes the production NPC solver. The only stubs are its game
lookup/logging boundary and quaternion primitives. Fixture JSON contains the
stored reference and A-pose transforms, names and parents from the game's
man_base and woman_base rigs; provenance and source JSON hashes are in each file.
No poses are generated in the extraction script.

Checks cover 6720 limb/target/world-rotation combinations on four rig/pose variants,
3216 small-step target samples across limbs, duplicate pose passes, and
a new shoulder animation that leaves the wrist's local quaternion unchanged.
They also move a controller over a cached animation pose, replay copies of the
solved buffer, and change the release blend. Each result must match one fresh
solve from the native pose rather than accumulating another pass on IK output.
Two hands pulling the same actor must also preserve that shared native input,
so their individual torso corrections cannot accumulate over repeated passes.
Two actors with shared reference tracks, reused live tracks, or identical bone
values must stay isolated, including retired-pose restoration. Identity comes
from the held AnimatedObject's pose descriptor (+0x18), not MetaRig reference
tracks (+0x40), which are resource data. The old track-only matching fails the
four real-rig isolation fixtures. The live trace logs rejected shared-track
descriptors on its periodic cleanup path (bounded to eight reports).
Pose bounds use the descriptor's populated bone count (+0x0C), with its capacity
(+0x08) as an upper bound. A partially populated LOD pose cannot pose a full rig.
They verify hinge direction and flexion, local wrist/ankle and shoulder/hip
limits, fixed bone lengths, unchanged translations/scales and unrelated bones.
The pre-change solver fails the limb and pose-cache cases separately.

The constraints are functional biped grip envelopes, not clinical measurements.
Bone-local Z hinge directions agree with the rigs' foot IK setup and their
elbow/knee ragdoll direction. Serialized ragdoll shape angles are not directly
animation joint limits: their shape frames differ, so they are not substituted
for bone-local reference frames. Unknown reference hinge geometry is rejected.

Optional live tools (all numerical generation and checking stays in C++):
  npc_grip_tests --make-live INITIAL.json COMMANDS.json
  npc_grip_tests --check-live SAMPLES.json

VRNpcGripState(side) returns one read-only, locked native snapshot while a limb
grip is active. Lua rows are: 1 part/miss/pass count/valid; 2-4 model positions;
5-7 local rotations; 8-10 reference local rotations; 11 effective target with
reference hinge angle in w; 12 parent-frame hinge axis and flexion sign;
13 outgoing bone axis and maximum flexion; 14 incoming axis and twist limit.
The offline checks do not prove NPC skin contact, collision, corpse dynamics,
all modified rigs, or the timing of controller-to-render attachment.

The NPC solve runs after the complete native AnimatedObject graph chain, not
after each graph's pose copy. CP2077 2.5 uses exe+0x1B3E00 for that update and
optionally converts the completed pose in-place to model space at +0x582B3C.
The final conversion call returns to +0x1B427D; solve before that conversion,
or at object-update return when no conversion was requested. Earlier reset
conversions are excluded. Hook installation validates entries and rolls back
the whole group on failure. Local-space fixtures do not validate that ABI;
live verification and the engine disassembly are required as well.

Exact bone anchors unwrap requested joint twist once per completed native pose,
before iterative CCD trials. Crossing +/-180 degrees keeps the same anatomical
stop instead of jumping to the opposite one. There is no time-based filter.
8648 forward/reverse, multi-turn and quaternion-sign-flip samples check all
cone families and hinge pronation. The stateless negative control jumps 160
degrees for a 0.02-degree shoulder request across that seam.

Bone-local physical contacts
---------------------------
VRNpcGripBind(entity, side, part, segment, owner) binds a real PhysX grip to the
upper segment (0), lower segment (1), or hand/foot (2) of a limb. It waits for
the initial finger seat, then captures a fixed wrist position and orientation
in that bone. The bounded C++ solver drives this point rather than inferring it
from a fixed offset to the limb's distal endpoint. Both hands capture from one
source pose and publish from the final combined pose.

The native driver reads paired controller/shoulder data, limits the independent
goal to arm reach, and publishes the same bone attachment to the visible wrist
and the held PhysX capsule. No controller-facing give or quarter-turn blend is
added to an active bone attachment. Other NPC capsules and head/chest grips
still use their existing path; this is not proof of complete body alignment.

Additional checks cover 432 feasible bone-local contacts, unchanged attachment
coordinates, two contacts on a shared bone, reach bounds, and release after an
actor stops submitting poses. The old distal-offset scheme is exercised as a
negative control and can miss the intended contact by more than 20 cm.
Expired grips free the hand slot without dereferencing old buffers; a matching
reused pose is restored when the actor returns. Weak ownership rejects stale
entities, and inactive skeletons skip the driver lock and input snapshot.
Partial graph updates restore retained IK rotations individually in the same
buffer, including after release, while keeping new translation, scale, and
joint rotation channels. Eight real-rig cases cover this; the original whole
pose equality check fails the partial-update negative control.

Anchored VRNpcGripState rows append: 15 bone/owner/serial/live; 16 actual model
point/error; 17 fixed bone-local wrist point; 18 actual world point; 19 desired
world point; 20 model wrist rotation; 21 world wrist rotation.
  npc_grip_tests --check-anchor SAMPLES.json
The live report checks the fixed attachment and measures drawn-wrist distance
and rotation against the published bone pose. Its 15 mm / 5 degree diagnostic
gate is a regression alarm, not a claim that all latency or skin contact is
solved. Fast motion, other body types, and world obstacles need separate tests.

For continuous motion enable trace=1 in physhands.ini. npc_grip_trace.csv
records final avatar FK and the exact NPC attachment consumed by that solve,
plus the latest publication and the completed PhysX hand/motor pose. Revisions
distinguish animation updates; control_at is the source controller snapshot.
The bounded trace queue reports dropped rows and writes on the physics worker.
  npc_grip_trace PATH_TO_npc_grip_trace.csv
The report excludes acquisition blending (weight below 1). Hand/used error
measures modifications after attachment application. Hand/latest error may
include another NPC update; hand/body error includes the physics step's age.
Neither matched consumed poses nor a low steady-state average proves that the
NPC and avatar rendered in the same frame, or that all other colliders agree.
The trace also carries the exact physics motor's pre-step rotation, requested
and commanded spin, acceleration cap, post-step spin and query-based contact
classification. Touch kind 2 is a person; touch_mask records effective physical
hand eligibility. This is a geometric query classification, not a PhysX contact
callback. The analyzer separates command-following error from deviation during
simulation using the commanded angular velocity and step length.
