#pragma once
#include <d3d12.h>

namespace cvr::framegen::captured_color {
// Only port-owned capture-pool textures carry this immutable allocation tag.
// Writers publish GENERIC_READ before signaling capture. All readers keep it
// unchanged; pool leases prohibit another write until every reader retires.
inline constexpr GUID kSharedReadTag{0x7612f283,0x4e78,0x47e6,{0x8c,0x02,0x41,0x72,0x97,0xa3,0x6b,0xe8}};
inline bool Shared(ID3D12Resource* resource){
    UINT size=sizeof(UINT),value{};
    return resource && SUCCEEDED(resource->GetPrivateData(kSharedReadTag,&size,&value)) && size==sizeof(value) && value==1;
}
inline HRESULT MarkShared(ID3D12Resource* resource){const UINT value=1;return resource->SetPrivateData(kSharedReadTag,sizeof(value),&value);}
inline D3D12_RESOURCE_STATES ReadState(ID3D12Resource* resource){return Shared(resource)?D3D12_RESOURCE_STATE_GENERIC_READ:D3D12_RESOURCE_STATE_COPY_SOURCE;}
}
