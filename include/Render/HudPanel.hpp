#pragma once
#include <d3d12.h>
#include <wrl.h>
#include <memory>
#include <vector>
#include <cstdint>
#include "Runtimes/HudChannels.hpp"
#include "Runtimes/HudLayout.hpp"

namespace RED4ext { struct DynamicTexture; }
namespace cvr::hud {
constexpr size_t MaxSprites = 64;
struct TextureLease {
    uint32_t handle = 0;
    Microsoft::WRL::ComPtr<ID3D12Resource> resource;
    ~TextureLease();
};
// An engine colour target held for one of the port's own reads (Natives/HudPanel.cpp): its pool handle is
// referenced, so the resource outlives the engine's release while the read is in flight. Empty unless it is a
// single-sample 2D RGBA target resting in the engine's shader-read state -- or, with `idle`, in COMMON: an ink
// target the engine leaves to decay after each write (Natives/MenuSurface.cpp), which a read promotes implicitly.
std::shared_ptr<TextureLease> Lease(RED4ext::DynamicTexture* texture, bool idle = false);
struct Sprite {
    std::shared_ptr<TextureLease> texture;
    float x = 0, y = 0, width = 0, height = 0;
    float tint[4] = {1, 1, 1, 1};
    uint64_t name=0;
};
struct Snapshot {
    uint32_t width = 0, height = 0;
    uint64_t stamp = 0, generation = 0;
    bool masked = false;
    bool lootVisible = false;
    bool windshieldMap = false;
    // Projected map: the share of the canvas's height the map takes; the quest text fills the rest (Natives/HudPanel.cpp).
    float windshieldSplit = 1.0f;
    // Compact minimap tile's normalized rectangle on the ordinary HUD canvas.
    Rect hudRect{};
    // The channel's slots exist and stay masked, but nothing under their windows is visible:
    // the canvas would be all zeros, so no GPU work and no layer (Natives/HudPanel.cpp).
    bool emptyContent = false;
    std::vector<Sprite> sprites;
};
struct Frame {
    Microsoft::WRL::ComPtr<ID3D12Resource> canvas;
    uint64_t stamp = 0, generation = 0, serial = 0;
    // The producing queue's fence: the canvas is finished once it reaches `serial` (its pipeline lives as long as the
    // frame can be read). A reader on another queue copies it only after.
    ID3D12Fence* fence = nullptr;
    bool masked = false;
    bool lootVisible = false;
    bool windshieldMap = false;
    float windshieldSplit = 1.0f;
    // Compact minimap tile's normalized rectangle on the ordinary HUD canvas.
    Rect hudRect{};
    bool empty = false;   // no canvas: the channel shows nothing this frame (Snapshot::emptyContent)
};
void Publish(std::shared_ptr<Snapshot> snapshot,Channel channel=Channel::Main);
void Capture(ID3D12Device* device, ID3D12CommandQueue* queue,Channel channel=Channel::Main);
std::shared_ptr<Frame> Latest(Channel channel=Channel::Main,bool completedOnly=false);
void SetConsumerReady(bool ready,Channel channel=Channel::Main);
bool ConsumerReady(Channel channel=Channel::Main);
// NO FLAT COPY FIRST. A consumer that has shown its channel stays PRIMED while it keeps asking (within a second) and
// has not failed outright: between two images -- after a menu, a scene, a channel with nothing in it yet -- it is not
// ready for a few frames (snapshot -> capture -> layer, 100-200 ms), and a mask that waited for it let the game draw
// the prompt or the HUD flat in the eye first (2026-11-14). The native slots are masked while primed; only a consumer
// that cannot show the channel at all (switched off, no swapchain, another device) drops it, and the game's own slots
// come back.
bool ConsumerPrimed(Channel channel=Channel::Main);
void DropConsumer(Channel channel=Channel::Main);
inline bool MaskNative(Channel channel=Channel::Main) {return ConsumerReady(channel) || ConsumerPrimed(channel);}
void Shutdown(Channel channel=Channel::Main);
}
