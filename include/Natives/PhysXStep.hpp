#pragma once
// PhysXStep -- the game's PhysX step, hooked in PhysX3_x64.dll: PxScene::simulate and fetchResults.
//
// Measured on the simulator (Natives/FrameOrderProbe.hpp, 2026-11-08), every frame in this order: the hand
// collision script's tick, the player's hands solved and published (~3 ms before the step), PxScene::simulate on
// a job thread, fetchResults ~1.3 ms later, the instances' pose records sent to the renderer, the frame submit.
// So what is set on an actor just before simulate goes into this frame's step with this frame's hands, and
// its result is what this frame draws.
namespace cvr::physxstep {
// Hooks the two once, from a live scene's table. False if there is no scene yet or the table is not PhysX's.
bool Ensure();
}
