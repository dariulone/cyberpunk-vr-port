#if !CVR_RUST_VRIK
// The C++ build's plan; the Rust build's is rust/crates/cvr-render/src/instance_observer_plan.rs (the header calls it).
#include "Render/NativeInstanceObserver.hpp"

namespace cvr::stereo::instance_observer::detail {
bool ReservationResult(uint64_t old,uint32_t capacity,uint32_t instances,uint32_t first,uint64_t after,ReservationRange& output)noexcept {
    output={};if(!capacity || (capacity&(capacity-1)) || !instances || instances>capacity)return false;
    const auto physical=old&(capacity-1);uint64_t begin=old;
    if(instances>capacity-physical){if(old>UINT64_MAX-(capacity-physical))return false;begin+=capacity-physical;}
    if(begin>UINT64_MAX-instances || first!=begin%capacity || after!=begin+instances)return false;
    output={0,old,begin,after,capacity,first,instances};return true;
}
bool ResolveWriter(uint64_t address,uint64_t ring,uint64_t generation,uint64_t thread,uint32_t capacity,
    Cursor before,Cursor after,uint32_t first,uint32_t instances,const std::optional<ReservationRange>& actual,
    const std::optional<CachedCursor>& previous,WriterRange& output)noexcept {
    output={};if(!address || !ring || !generation || !thread || !instances || first>=capacity || instances>capacity-first)return false;
    if(actual){const auto& range=*actual;
        if(!range.sequence || range.capacity!=capacity || range.absoluteEnd<range.absoluteBegin ||
            range.absoluteEnd-range.absoluteBegin!=range.instances || range.first!=range.absoluteBegin%capacity ||
            first!=range.first || instances>range.instances)return false;
        const bool cached=before.first>=before.end && instances<128 && range.instances==128;
        if(cached){if(after.first!=first+instances || after.end!=first+128 || 128>capacity-first)return false;
            output.cache=CachedCursor{address,ring,generation,thread,range,after};
        }else if(range.instances!=instances || before!=after ||
            (before.first<before.end && instances<=uint64_t(before.end)-before.first))return false;
        output.absoluteBegin=range.absoluteBegin;output.reservation=range;return true;
    }
    if(!previous)return false;const auto& cache=*previous;const auto& range=cache.reservation;
    if(cache.address!=address || cache.ring!=ring || cache.ringGeneration!=generation || cache.thread!=thread ||
        cache.next!=before || !range.sequence || range.capacity!=capacity || range.instances!=128 ||
        range.absoluteBegin>UINT64_MAX-range.instances || range.absoluteEnd!=range.absoluteBegin+range.instances ||
        range.first!=range.absoluteBegin%capacity || before.end!=range.first+128 || first!=before.first ||
        first<range.first || first>=before.end || instances>before.end-first || after.first!=first+instances || after.end!=before.end)return false;
    output.absoluteBegin=range.absoluteBegin+first-range.first;output.reservation=range;output.cache=cache;output.cache->next=after;return true;
}
bool InstanceLayout(std::span<const shader_snapshot::Input> inputs,uint32_t stride)noexcept {
    if(inputs.empty() || inputs.size()>32 || (stride!=48 && stride!=64))return false;uint32_t next{};bool found{};
    for(const auto& input:inputs)if(input.slot==7){uint32_t bytes{};
        switch(DXGI_FORMAT(input.format)){
        case DXGI_FORMAT_R32G32B32A32_FLOAT:case DXGI_FORMAT_R32G32B32A32_UINT:case DXGI_FORMAT_R32G32B32A32_SINT:bytes=16;break;
        case DXGI_FORMAT_R32G32B32_FLOAT:case DXGI_FORMAT_R32G32B32_UINT:case DXGI_FORMAT_R32G32B32_SINT:bytes=12;break;
        case DXGI_FORMAT_R32G32_FLOAT:case DXGI_FORMAT_R32G32_UINT:case DXGI_FORMAT_R32G32_SINT:bytes=8;break;
        case DXGI_FORMAT_R32_FLOAT:case DXGI_FORMAT_R32_UINT:case DXGI_FORMAT_R32_SINT:bytes=4;break;
        default:return false;}
        const auto at=input.offset==D3D12_APPEND_ALIGNED_ELEMENT?next:input.offset;
        if(input.classification!=D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA || input.step!=1 || at>stride || bytes>stride-at)return false;
        next=at+bytes;found=true;
    }return found;
}
}
#endif
