#pragma once
#include <array>
#include <cstddef>
#include <cstdint>

namespace cvr::stereo::camera_upload {
using ReadState=bool(*)(uintptr_t,void*,size_t);
struct Context {
    uint64_t context{},state{},list{};
    bool operator==(const Context&)const=default;
    explicit operator bool()const{return context && state && list;}
};
struct CpuSlot {
    uint64_t layout{},table{},address{},descriptor{};
    uint32_t parameter{},index{};
    bool operator==(const CpuSlot&)const=default;
};
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/camera_binding.rs (tools/single_pass_tests/camera_binding_state_test runs its
// cases against them in its Rust variant; the tracker's and scope's records stay here, mirrored there).
struct BindingScope;
using RestoreBinding=bool(*)(void*,Context,uint32_t,uint32_t);
extern "C" {
bool cvr_cb_resolve(ReadState,uint64_t,uint32_t,CpuSlot*,bool);
bool cvr_cb_restore(ReadState,const Context*,BindingScope*,RestoreBinding,void*);
bool cvr_cb_tracker(void*,uint32_t,ReadState,const Context*,uint32_t,uint32_t,BindingScope*);
}
#endif
#if CVR_RUST_VRIK
inline bool ResolveCameraSlotOnce(ReadState read,uint64_t state,uint32_t stage,CpuSlot& out){
    static_assert(sizeof(Context)==24 && sizeof(CpuSlot)==40,"mirrored by camera_binding.rs");
    return cvr_cb_resolve(read,state,stage,&out,false);
}
#else
inline bool ResolveCameraSlotOnce(ReadState read,uint64_t state,uint32_t stage,CpuSlot& out){
    out={};
    if(!read || !state || state>UINT64_MAX-0x70 || stage>1 ||
        !read(state+0x68,&out.layout,8) || !read(state+0x28,&out.table,8) || !out.layout || !out.table ||
        out.layout>UINT64_MAX-0x473)return false;
    uint8_t parameter{};
    if(!read(out.layout+0x455+uint64_t(stage)*0x1c,&parameter,1) || parameter>=64)return false;
    const auto entry=out.layout+0x38+uint64_t(parameter)*16;
    uint32_t offset{};uint16_t first{};
    if(!read(entry+4,&offset,4) || !read(entry+8,&first,2) || first>1 || offset>8190)return false;
    out.parameter=parameter;out.index=offset+1-first;
    if(out.table>UINT64_MAX-uint64_t(out.index)*8-8)return false;
    out.address=out.table+uint64_t(out.index)*8;
    return read(out.address,&out.descriptor,8) && out.descriptor;
}
#endif
// Layout is guarded by the complete native 0x336D3C code fingerprint. Re-read
// the native shadow so a changed table/layout cannot supply a mixed snapshot.
#if CVR_RUST_VRIK
inline bool ResolveCameraSlot(ReadState read,uint64_t state,uint32_t stage,CpuSlot& out){
    return cvr_cb_resolve(read,state,stage,&out,true);
}
#else
inline bool ResolveCameraSlot(ReadState read,uint64_t state,uint32_t stage,CpuSlot& out){
    CpuSlot a,b;
    if(!ResolveCameraSlotOnce(read,state,stage,a) || !ResolveCameraSlotOnce(read,state,stage,b) || a!=b)return false;
    out=a;return true;
}
#endif
struct Binding {
    uint32_t handle{};uint64_t serial{},descriptor{};
};
struct BindingScope {
    Context context;
    std::array<Binding,2> original;
    uint64_t replacement{};
    uint32_t requested{},applied{},restored{},preserved{};
};
using RestoreBinding=bool(*)(void*,Context,uint32_t,uint32_t);
#if CVR_RUST_VRIK
inline bool RestoreScope(ReadState read,Context current,BindingScope& scope,RestoreBinding bind,void* user){
    static_assert(sizeof(Binding)==24 && sizeof(BindingScope)==96,"mirrored by camera_binding.rs");
    return cvr_cb_restore(read,&current,&scope,bind,user);
}
#else
inline bool RestoreScope(ReadState read,Context current,BindingScope& scope,RestoreBinding bind,void* user){
    if(!scope.applied)return true;
    if(!current || current!=scope.context || !bind)return false;
    bool complete=true;
    for(uint32_t stage=0;stage<2;++stage){const uint32_t bit=1u<<stage;if(!(scope.applied&bit))continue;
        CpuSlot slot;
        if(ResolveCameraSlot(read,current.state,stage,slot) && slot.descriptor!=scope.replacement){
            scope.preserved|=bit;scope.applied&=~bit;continue;
        }
        if(!bind(user,current,scope.original[stage].handle,bit) ||
            !ResolveCameraSlot(read,current.state,stage,slot) || slot.descriptor!=scope.original[stage].descriptor){complete=false;continue;}
        scope.restored|=bit;scope.applied&=~bit;
    }
    return complete;
}
#endif
class BindingTracker {
    Context context_;
    std::array<Binding,2> bindings_{};
    uint64_t serial_{};
public:
#if CVR_RUST_VRIK
    // The members' layout is mirrored by camera_binding.rs (Tracker, 80 bytes).
    void Clear(){cvr_cb_tracker(this,0,nullptr,nullptr,0,0,nullptr);}
    void Observe(ReadState read,Context context,uint32_t handle,uint32_t stages){
        static_assert(sizeof(Context)+sizeof(std::array<Binding,2>)+sizeof(uint64_t)==80,"mirrored by camera_binding.rs");
        cvr_cb_tracker(this,1,read,&context,handle,stages,nullptr);
    }
    bool Capture(ReadState read,Context context,uint32_t stages,BindingScope& scope)const{
        return cvr_cb_tracker(const_cast<BindingTracker*>(this),2,read,&context,0,stages,&scope);
    }
    bool Changed(const BindingScope& scope,uint32_t stage)const{
        return cvr_cb_tracker(const_cast<BindingTracker*>(this),3,nullptr,nullptr,0,stage,const_cast<BindingScope*>(&scope));
    }
#else
    void Clear(){context_={};bindings_={};}
    void Observe(ReadState read,Context context,uint32_t handle,uint32_t stages){
        if(!(stages&3))return;
        if(!context){Clear();return;}
        if(context_!=context){Clear();context_=context;}
        for(uint32_t stage=0;stage<2;++stage)if(stages&(1u<<stage)){
            CpuSlot slot;auto& binding=bindings_[stage];binding={};binding.serial=++serial_;
            if(handle && ResolveCameraSlot(read,context.state,stage,slot)){binding.handle=handle;binding.descriptor=slot.descriptor;}
        }
    }
    bool Capture(ReadState read,Context context,uint32_t stages,BindingScope& scope)const{
        scope={};
        if(!context || context!=context_ || !stages || stages>3)return false;
        for(uint32_t stage=0;stage<2;++stage)if(stages&(1u<<stage)){
            CpuSlot slot;const auto& saved=bindings_[stage];
            if(!saved.handle || !saved.serial || !ResolveCameraSlot(read,context.state,stage,slot) || slot.descriptor!=saved.descriptor)return false;
        }
        scope.context=context;scope.original=bindings_;scope.requested=stages;return true;
    }
    bool Changed(const BindingScope& scope,uint32_t stage)const{
        return stage>1 || context_!=scope.context || bindings_[stage].serial!=scope.original[stage].serial;
    }
#endif
};
}
