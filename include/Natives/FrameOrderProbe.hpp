#pragma once
// FrameOrderProbe -- when things happen within a frame, for a few frames, on request (VRFrameOrderProbe(frames)).
//
// Asked for carrying a loose body the lobby's way: its PhysX actor kinematic, driven to the hand. Within a
// millimetre only if its target goes in after the hand is final for the frame and before PhysX steps. Which of
// those comes first is what this records: PhysX's own simulate and fetchResults (hooked in PhysX3_x64.dll on the
// first request), the hand bones published, the instances' pose records sent to the renderer, the hand collision
// script's tick, and the frame submit. The log gets one line per frame, each event's first and last time in ms
// after the previous submit, and its thread.
namespace cvr::frameorder {
// 'S'/'s' simulate in/out, 'F'/'f' fetchResults in/out, 'K' hand bones published, 'R' an instance's pose records
// sent, 'L' the hand collision script's tick. Cheap when no recording is running.
void Note(char what);
// The frame submit, on the game thread: closes a frame.
void NoteSubmit();
// Records the next `frames` frames (at most 120) and logs them. False if PhysX could not be hooked.
bool Start(int frames);
}
