#pragma once
// The picture of a carried thing: drawn on the hand, while its body stays in PhysX.
#include <cstdint>

namespace cvr::heldpicture {
// A carried body, as the script knows it: its proxy and body index, the bone of the hand, and where the thing sits in that hand's frame.
// False if this kind of thing cannot be drawn this way (or the setting is off): then it stays as it was, its picture on its body.
// Blocked: the body has run into something, and the picture goes back to it. Reach: how far from its body the picture may be, m, for a wall ahead of it.
bool Hold(uint32_t proxy,uint32_t body,int bone,const float offset[3],const float turn[4],bool blocked,float reach);
// The grip has opened: from now on the picture goes with the body, closing what is between them.
void LetGo(uint32_t proxy,uint32_t body);
// On the game thread at the frame submit, the last moment before this frame's proxies are flushed.
void AtFrameSubmit();
}
