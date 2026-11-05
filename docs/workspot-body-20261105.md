# The body goes to the game while a workspot holds the player (2026-11-05)

Sitting, lying and leaning are a workspot (locomotion state 9) in a staged scene. The entry plays above the scene suspend
threshold, and when it settles the tier drops back below it with the player still held. VRIK and the physical body came back,
and drew the body beside the seat.

`cvr::anim::ShouldSuspendVrikForWorkspot` (`Anim/ScenePolicy.hpp`) keeps VRIK off at tier 2 or above while locomotion is 9. It is used in the pose
hook and in `Hooks/RoomscaleMove.cpp`, so view and room go with the body. With the suspend setting at Never, VRIK stays on as before.

Found by setting the suspend tier to Tier 2+ by hand, which fixed the body in a sitting pose. Confirmed in game on Index controllers with SteamVR.
