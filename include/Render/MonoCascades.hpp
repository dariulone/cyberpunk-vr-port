#pragma once
#include <d3d12.h>
#include <atomic>
#include <cstdint>

extern "C" {
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_MonoCascades;
__declspec(dllexport) extern uint32_t CyberpunkVR_MonoCascadeReportSeq;
__declspec(dllexport) extern char CyberpunkVR_MonoCascadeReport[4096];
}
#if CVR_RUST_VRIK
// The cascades: rust/crates/cvr-gpu/src/mono_cascades.rs (the C++ build's declarations below name the calls).
extern "C" {
void cvr_mono_cascades_resource_created(ID3D12Resource*,ID3D12Heap*,uint64_t offset,uint64_t bytes);
void cvr_mono_cascades_dsv_created(ID3D12Resource*,const D3D12_DEPTH_STENCIL_VIEW_DESC*,SIZE_T handle);
void cvr_mono_cascades_copy_descriptors(ID3D12Device*,UINT,const D3D12_CPU_DESCRIPTOR_HANDLE*,const UINT*,UINT,
                                        const D3D12_CPU_DESCRIPTOR_HANDLE*,const UINT*,int type);
void cvr_mono_cascades_source_sampling(const float*,unsigned);
void cvr_mono_cascades_main_sampling(const float*,unsigned);
void cvr_mono_cascades_camera(unsigned,const void*,size_t,bool source);
void cvr_mono_cascades_begin_node(int side,int index);
bool cvr_mono_cascades_skip_node(int side,int index);
void cvr_mono_cascades_end_node();
void cvr_mono_cascades_targets(ID3D12GraphicsCommandList*,UINT,const SIZE_T* dsv);
bool cvr_mono_cascades_skip_draw(ID3D12GraphicsCommandList*);
void cvr_mono_cascades_signature_created(ID3D12CommandSignature*,const D3D12_COMMAND_SIGNATURE_DESC*);
bool cvr_mono_cascades_skip_indirect(ID3D12GraphicsCommandList*,ID3D12CommandSignature*);
bool cvr_mono_cascades_clear(ID3D12GraphicsCommandList*,SIZE_T dsv,int flags,FLOAT depth,UINT8 stencil,UINT rects,int side,bool clearNode);
bool cvr_mono_cascades_discard(ID3D12Resource*);
void cvr_mono_cascades_write(ID3D12Resource*);
void cvr_mono_cascades_barriers(UINT,const D3D12_RESOURCE_BARRIER*);
void cvr_mono_cascades_before_barriers(ID3D12GraphicsCommandList*,UINT,const D3D12_RESOURCE_BARRIER*);
void cvr_mono_cascades_close(ID3D12GraphicsCommandList*);
void cvr_mono_cascades_reset(ID3D12GraphicsCommandList*);
void cvr_mono_cascades_poll();
}
namespace cvr::cascades {
inline void ResourceCreated(ID3D12Resource* r,ID3D12Heap* heap,uint64_t offset,uint64_t bytes){cvr_mono_cascades_resource_created(r,heap,offset,bytes);}
inline void DsvCreated(ID3D12Resource* r,const D3D12_DEPTH_STENCIL_VIEW_DESC* desc,D3D12_CPU_DESCRIPTOR_HANDLE handle){cvr_mono_cascades_dsv_created(r,desc,handle.ptr);}
inline void CopyDescriptors(ID3D12Device* device,UINT destinations,const D3D12_CPU_DESCRIPTOR_HANDLE* dest,const UINT* sizes,UINT sources,
                            const D3D12_CPU_DESCRIPTOR_HANDLE* src,const UINT* counts,D3D12_DESCRIPTOR_HEAP_TYPE type) {
    cvr_mono_cascades_copy_descriptors(device,destinations,dest,sizes,sources,src,counts,int(type));
}
inline void SourceSampling(const float* m,unsigned count){cvr_mono_cascades_source_sampling(m,count);}
inline void MainSampling(const float* m,unsigned count){cvr_mono_cascades_main_sampling(m,count);}
inline void Camera(unsigned index,const void* data,size_t bytes,bool source){cvr_mono_cascades_camera(index,data,bytes,source);}
inline void BeginNode(int side,int index){cvr_mono_cascades_begin_node(side,index);}
inline bool SkipNode(int side,int index){return cvr_mono_cascades_skip_node(side,index);}
inline void EndNode(){cvr_mono_cascades_end_node();}
inline void Targets(ID3D12GraphicsCommandList* list,UINT colors,const D3D12_CPU_DESCRIPTOR_HANDLE* dsv){cvr_mono_cascades_targets(list,colors,dsv?&dsv->ptr:nullptr);}
inline bool SkipDraw(ID3D12GraphicsCommandList* list){return cvr_mono_cascades_skip_draw(list);}
inline void SignatureCreated(ID3D12CommandSignature* s,const D3D12_COMMAND_SIGNATURE_DESC& desc){cvr_mono_cascades_signature_created(s,&desc);}
inline bool SkipIndirect(ID3D12GraphicsCommandList* list,ID3D12CommandSignature* s){return cvr_mono_cascades_skip_indirect(list,s);}
inline bool Clear(ID3D12GraphicsCommandList* list,D3D12_CPU_DESCRIPTOR_HANDLE dsv,D3D12_CLEAR_FLAGS flags,FLOAT depth,UINT8 stencil,UINT rects,int side,bool clearNode) {
    return cvr_mono_cascades_clear(list,dsv.ptr,int(flags),depth,stencil,rects,side,clearNode);
}
inline bool Discard(ID3D12Resource* r,int,bool){return cvr_mono_cascades_discard(r);}
inline void Write(ID3D12Resource* r){cvr_mono_cascades_write(r);}
inline void Barriers(UINT count,const D3D12_RESOURCE_BARRIER* b){cvr_mono_cascades_barriers(count,b);}
inline void BeforeBarriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RESOURCE_BARRIER* b){cvr_mono_cascades_before_barriers(list,count,b);}
inline void Close(ID3D12GraphicsCommandList* list){cvr_mono_cascades_close(list);}
inline void Reset(ID3D12GraphicsCommandList* list){cvr_mono_cascades_reset(list);}
inline void Poll(){cvr_mono_cascades_poll();}
}
#else
namespace cvr::cascades {
void ResourceCreated(ID3D12Resource*,ID3D12Heap*,uint64_t offset,uint64_t bytes);
void DsvCreated(ID3D12Resource*,const D3D12_DEPTH_STENCIL_VIEW_DESC*,D3D12_CPU_DESCRIPTOR_HANDLE);
void CopyDescriptors(ID3D12Device*,UINT,const D3D12_CPU_DESCRIPTOR_HANDLE*,const UINT*,UINT,const D3D12_CPU_DESCRIPTOR_HANDLE*,const UINT*,D3D12_DESCRIPTOR_HEAP_TYPE);
void SourceSampling(const float*,unsigned);
void MainSampling(const float*,unsigned);
void Camera(unsigned,const void*,size_t,bool source);
void BeginNode(int side,int index);
bool SkipNode(int side,int index);
void EndNode();
void Targets(ID3D12GraphicsCommandList*,UINT,const D3D12_CPU_DESCRIPTOR_HANDLE*);
bool SkipDraw(ID3D12GraphicsCommandList*);
void SignatureCreated(ID3D12CommandSignature*,const D3D12_COMMAND_SIGNATURE_DESC&);
bool SkipIndirect(ID3D12GraphicsCommandList*,ID3D12CommandSignature*);
bool Clear(ID3D12GraphicsCommandList*,D3D12_CPU_DESCRIPTOR_HANDLE,D3D12_CLEAR_FLAGS,FLOAT,UINT8,UINT,int side,bool clearNode);
bool Discard(ID3D12Resource*,int side,bool clearNode);
void Write(ID3D12Resource*);
void Barriers(UINT,const D3D12_RESOURCE_BARRIER*);
void BeforeBarriers(ID3D12GraphicsCommandList*,UINT,const D3D12_RESOURCE_BARRIER*);
void Close(ID3D12GraphicsCommandList*);
void Reset(ID3D12GraphicsCommandList*);
void Poll();
}
#endif
