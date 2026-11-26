#pragma once
#include <d3d12.h>
#include <wrl/client.h>
#include <span>

#if CVR_RUST_VRIK
#include <utility>
// The arrays: rust/crates/cvr-gpu/src/target_array.rs (copies share the array, as ComPtr copies did).
extern "C" {
void* cvr_target_array_new();
void* cvr_target_array_clone(const void*);
void cvr_target_array_free(void*);
HRESULT cvr_target_array_initialize(void*,ID3D12Device*,const D3D12_RESOURCE_DESC*,D3D12_RESOURCE_STATES,const D3D12_CLEAR_VALUE*);
void* cvr_target_array_resource(const void*);
UINT cvr_target_array_planes(const void*);
HRESULT cvr_target_array_copy_eye(const void*,ID3D12GraphicsCommandList*,UINT eye,D3D12_RESOURCE_STATES,ID3D12Resource*,
    D3D12_RESOURCE_STATES,D3D12_RESOURCE_STATES);
HRESULT cvr_target_array_import_eye(const void*,ID3D12GraphicsCommandList*,UINT eye,D3D12_RESOURCE_STATES,ID3D12Resource*,D3D12_RESOURCE_STATES);
HRESULT cvr_target_array_import_eyes(const void*,ID3D12GraphicsCommandList*,UINT eyeMask,D3D12_RESOURCE_STATES,ID3D12Resource*,D3D12_RESOURCE_STATES);
HRESULT cvr_target_array_transition_eye(const void*,ID3D12GraphicsCommandList*,UINT eye,D3D12_RESOURCE_STATES,D3D12_RESOURCE_STATES);
HRESULT cvr_target_array_import_eye_layer(const void*,ID3D12GraphicsCommandList*,UINT eye,D3D12_RESOURCE_STATES,ID3D12Resource*,D3D12_RESOURCE_STATES);
HRESULT cvr_target_array_copy_eye_layer_uniform(const void*,ID3D12GraphicsCommandList*,UINT eye,D3D12_RESOURCE_STATES,ID3D12Resource*,
    D3D12_RESOURCE_STATES,D3D12_RESOURCE_STATES);
HRESULT cvr_target_array_copy_eye_layer(const void*,ID3D12GraphicsCommandList*,UINT eye,D3D12_RESOURCE_STATES,ID3D12Resource*,
    const D3D12_RESOURCE_STATES*,size_t,const D3D12_RESOURCE_STATES*,size_t);
}
namespace cvr::stereo {
class StereoTargetArray {
public:
    StereoTargetArray():rust_(cvr_target_array_new()){}
    StereoTargetArray(const StereoTargetArray& other):rust_(cvr_target_array_clone(other.rust_)){}
    StereoTargetArray(StereoTargetArray&& other)noexcept:rust_(std::exchange(other.rust_,cvr_target_array_new())){}
    StereoTargetArray& operator=(StereoTargetArray other)noexcept{std::swap(rust_,other.rust_);return *this;}
    ~StereoTargetArray(){cvr_target_array_free(rust_);}
    HRESULT Initialize(ID3D12Device* device,const D3D12_RESOURCE_DESC& nativeTarget,
        D3D12_RESOURCE_STATES initialState,const D3D12_CLEAR_VALUE* clear=nullptr){
        return cvr_target_array_initialize(rust_,device,&nativeTarget,initialState,clear);
    }
    HRESULT CopyEye(ID3D12GraphicsCommandList* list,UINT eye,D3D12_RESOURCE_STATES sourceState,
        ID3D12Resource* nativeTarget,D3D12_RESOURCE_STATES targetBefore,D3D12_RESOURCE_STATES targetAfter) const{
        return cvr_target_array_copy_eye(rust_,list,eye,sourceState,nativeTarget,targetBefore,targetAfter);
    }
    HRESULT ImportEye(ID3D12GraphicsCommandList* list,UINT eye,D3D12_RESOURCE_STATES arrayState,
        ID3D12Resource* nativeSource,D3D12_RESOURCE_STATES sourceState) const{
        return cvr_target_array_import_eye(rust_,list,eye,arrayState,nativeSource,sourceState);
    }
    HRESULT ImportEyes(ID3D12GraphicsCommandList* list,UINT eyeMask,D3D12_RESOURCE_STATES arrayState,
        ID3D12Resource* nativeSource,D3D12_RESOURCE_STATES sourceState) const{
        return cvr_target_array_import_eyes(rust_,list,eyeMask,arrayState,nativeSource,sourceState);
    }
    HRESULT TransitionEye(ID3D12GraphicsCommandList* list,UINT eye,D3D12_RESOURCE_STATES before,D3D12_RESOURCE_STATES after) const{
        return cvr_target_array_transition_eye(rust_,list,eye,before,after);
    }
    HRESULT ImportEyeLayer(ID3D12GraphicsCommandList* list,UINT eye,D3D12_RESOURCE_STATES arrayState,
        ID3D12Resource* nativeSource,D3D12_RESOURCE_STATES sourceState) const{
        return cvr_target_array_import_eye_layer(rust_,list,eye,arrayState,nativeSource,sourceState);
    }
    HRESULT CopyEyeLayer(ID3D12GraphicsCommandList* list,UINT eye,D3D12_RESOURCE_STATES arrayState,
        ID3D12Resource* nativeTarget,D3D12_RESOURCE_STATES targetBefore,D3D12_RESOURCE_STATES targetAfter) const{
        return cvr_target_array_copy_eye_layer_uniform(rust_,list,eye,arrayState,nativeTarget,targetBefore,targetAfter);
    }
    HRESULT CopyEyeLayer(ID3D12GraphicsCommandList* list,UINT eye,D3D12_RESOURCE_STATES arrayState,
        ID3D12Resource* nativeTarget,std::span<const D3D12_RESOURCE_STATES> targetBefore,
        std::span<const D3D12_RESOURCE_STATES> targetAfter) const{
        return cvr_target_array_copy_eye_layer(rust_,list,eye,arrayState,nativeTarget,targetBefore.data(),targetBefore.size(),
            targetAfter.data(),targetAfter.size());
    }
    ID3D12Resource* Resource() const{return static_cast<ID3D12Resource*>(cvr_target_array_resource(rust_));}
    UINT PlaneCount() const{return cvr_target_array_planes(rust_);}
private:
    void* rust_;
};
}
#else
namespace cvr::stereo {
// Array output for a common geometry pass, with an explicit copy back into the
// native single-view targets consumed by unmodified lighting/temporal shaders.
// The owner retains this object and native destinations until their command
// queue fence completes; no CPU waits here.
class StereoTargetArray {
public:
    HRESULT Initialize(ID3D12Device*,const D3D12_RESOURCE_DESC& nativeTarget,
        D3D12_RESOURCE_STATES initialState,const D3D12_CLEAR_VALUE* clear=nullptr);
    HRESULT CopyEye(ID3D12GraphicsCommandList*,UINT eye,D3D12_RESOURCE_STATES sourceState,
        ID3D12Resource* nativeTarget,D3D12_RESOURCE_STATES targetBefore,D3D12_RESOURCE_STATES targetAfter) const;
    HRESULT ImportEye(ID3D12GraphicsCommandList*,UINT eye,D3D12_RESOURCE_STATES arrayState,
        ID3D12Resource* nativeSource,D3D12_RESOURCE_STATES sourceState) const;
    // Initialize either or both layers with one source/array transition pair.
    HRESULT ImportEyes(ID3D12GraphicsCommandList*,UINT eyeMask,D3D12_RESOURCE_STATES arrayState,
        ID3D12Resource* nativeSource,D3D12_RESOURCE_STATES sourceState) const;
    // Per-layer variants leave the other eye's state and contents untouched.
    HRESULT TransitionEye(ID3D12GraphicsCommandList*,UINT eye,D3D12_RESOURCE_STATES before,D3D12_RESOURCE_STATES after) const;
    HRESULT ImportEyeLayer(ID3D12GraphicsCommandList*,UINT eye,D3D12_RESOURCE_STATES arrayState,
        ID3D12Resource* nativeSource,D3D12_RESOURCE_STATES sourceState) const;
    HRESULT CopyEyeLayer(ID3D12GraphicsCommandList*,UINT eye,D3D12_RESOURCE_STATES arrayState,
        ID3D12Resource* nativeTarget,D3D12_RESOURCE_STATES targetBefore,D3D12_RESOURCE_STATES targetAfter) const;
    // Exactly PlaneCount states in native subresource order. Copies honor the
    // caller's predication; barriers remain unconditional. The caller proves
    // native access/state and retains lifetimes; this helper grants no authority.
    HRESULT CopyEyeLayer(ID3D12GraphicsCommandList*,UINT eye,D3D12_RESOURCE_STATES arrayState,
        ID3D12Resource* nativeTarget,std::span<const D3D12_RESOURCE_STATES> targetBefore,
        std::span<const D3D12_RESOURCE_STATES> targetAfter) const;
    ID3D12Resource* Resource() const {return resource_.Get();}
    UINT PlaneCount() const {return planes_;}
private:
    HRESULT ValidateLayer(ID3D12GraphicsCommandList*,UINT eye,ID3D12Resource* native) const;
    Microsoft::WRL::ComPtr<ID3D12Resource> resource_;
    D3D12_RESOURCE_DESC native_{};
    UINT planes_{};
};
}
#endif
