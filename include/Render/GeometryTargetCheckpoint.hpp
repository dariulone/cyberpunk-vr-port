#pragma once
#include "Render/StereoSceneState.hpp"

#if CVR_RUST_VRIK
#include <memory>
// The checkpoint: rust/crates/cvr-gpu/src/geometry_checkpoint.rs. Copies share one checkpoint, as the C++ build's
// shared storage does; Targets() mirrors the private targets once captured.
extern "C" {
HRESULT cvr_geometry_checkpoint_create(ID3D12Device*,const cvr::stereo::scene_state::detail::GeometryTargetsRaw*,void**);
bool cvr_geometry_checkpoint_capture(void*,ID3D12GraphicsCommandList*,const cvr::stereo::scene_state::detail::GeometryTargetsRaw*);
bool cvr_geometry_checkpoint_targets(const void*,cvr::stereo::scene_state::detail::GeometryTargetsRaw*);
void cvr_geometry_checkpoint_free(void*);
}
namespace cvr::stereo {
class GeometryTargetCheckpoint {
    struct Storage {
        void* rust{};scene_state::GeometryTargets targets;bool captured{};
        ~Storage(){cvr_geometry_checkpoint_free(rust);}
    };
    std::shared_ptr<Storage> storage_;
    static scene_state::detail::GeometryTargetsRaw Raw(const scene_state::GeometryTargets& value){
        scene_state::detail::GeometryTargetsRaw raw{};
        for(UINT i=0;i<5;++i){raw.resources[i]=value.resources[i].Get();raw.views[i]=value.views[i];raw.handles[i]=value.handles[i];}
        raw.colors=value.colors;return raw;
    }
public:
    HRESULT Create(ID3D12Device* device,const scene_state::GeometryTargets& shape){
        if(storage_)return E_UNEXPECTED;
        const auto raw=Raw(shape);void* rust{};const auto hr=cvr_geometry_checkpoint_create(device,&raw,&rust);
        if(FAILED(hr))return hr;
        storage_=std::make_shared<Storage>();storage_->rust=rust;return S_OK;
    }
    // One recording only. The caller proves native RT/DEPTH_WRITE states and
    // one-shot submission. Captured means recorded, not GPU-complete.
    bool Capture(ID3D12GraphicsCommandList* list,const scene_state::GeometryTargets& native){
        if(!storage_ || !list || storage_->captured)return false;
        const auto raw=Raw(native);if(!cvr_geometry_checkpoint_capture(storage_->rust,list,&raw))return false;
        scene_state::detail::GeometryTargetsRaw targets{};
        if(!cvr_geometry_checkpoint_targets(storage_->rust,&targets))return false;
        for(UINT i=0;i<5;++i){storage_->targets.resources[i].Attach(targets.resources[i]);
            storage_->targets.views[i]=targets.views[i];storage_->targets.handles[i]=targets.handles[i];}
        storage_->targets.colors=targets.colors;storage_->captured=true;return true;
    }
    bool Captured() const{return storage_ && storage_->captured;}
    // Empty until Capture has successfully recorded all copies.
    const scene_state::GeometryTargets& Targets() const{
        static const scene_state::GeometryTargets empty;return Captured()?storage_->targets:empty;
    }
};
}
#else
namespace cvr::stereo {
class GeometryTargetCheckpoint {
    struct Storage;
    Microsoft::WRL::ComPtr<Storage> storage_;
public:
    GeometryTargetCheckpoint();
    ~GeometryTargetCheckpoint();
    HRESULT Create(ID3D12Device*,const scene_state::GeometryTargets& shape);
    // One recording only. The caller proves native RT/DEPTH_WRITE states and
    // one-shot submission. Captured means recorded, not GPU-complete.
    bool Capture(ID3D12GraphicsCommandList*,const scene_state::GeometryTargets&);
    bool Captured() const;
    // Empty until Capture has successfully recorded all copies.
    const scene_state::GeometryTargets& Targets() const;
};
}
#endif
