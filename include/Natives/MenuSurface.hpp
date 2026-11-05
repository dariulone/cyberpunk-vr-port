#pragma once
#include <d3d12.h>

namespace RED4ext { struct DynamicTexture; struct WorldWidgetComponent; }
namespace cvr::menu_surface {
// The dedicated texture of one of the port's own screens (the VR menu's, the lobby's), null before it has one.
// Main thread, with the component alive.
RED4ext::DynamicTexture* SurfaceTexture(RED4ext::WorldWidgetComponent* component);
bool Waiting();
// Native ink targets owned by the menu, outside the shared world-widget atlas.
// These callbacks observe engine work; they never composite into an XR image.
void Barriers(ID3D12GraphicsCommandList*,UINT,const D3D12_RESOURCE_BARRIER*);
void Written(ID3D12GraphicsCommandList*,ID3D12Resource*);
void Submitted(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
void ResetList(ID3D12GraphicsCommandList*);
}
