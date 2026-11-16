#include "CyberpunkVrAdapter.h"
// Compile the actual adapter with the same ImGui version used by local CET.
// Instantiate the rendering template as well as the inline input methods.
void compile_adapter(CyberpunkVrAdapter& adapter,ImDrawData* data,ID3D12Device* device,
    ID3D12CommandQueue* queue,ID3D12DescriptorHeap* heap){
    adapter.Prepare();adapter.Scale({3840,2160},ImGui::GetStyle());adapter.Finish(true);
    adapter.Render(data,device,queue,DXGI_FORMAT_R8G8B8A8_UNORM,heap,[](ID3D12GraphicsCommandList*){});
    adapter.Drain();adapter.Hide();
}
