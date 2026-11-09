#include "Framegen/SourceTimeline.hpp"
#include "Framegen/FlightRecorder.hpp"
#include "Framegen/RefreshCadence.hpp"
#include "Render/CommandResources.hpp"
#include <mutex>
namespace cvr::framegen {
namespace {
double Qpc(){LARGE_INTEGER n{},hz{};QueryPerformanceCounter(&n);QueryPerformanceFrequency(&hz);return double(n.QuadPart)*1000./double(hz.QuadPart);}
struct Marker {
    CaptureTiming timing;
    ID3D12CommandList* list{};
    bool recorded{},released{},submitted{};
    double cpuRecord{},cpuSubmit{};
};
}
struct SourceFrame {
    Microsoft::WRL::ComPtr<ID3D12Device> device;
    Microsoft::WRL::ComPtr<ID3D12Fence> fence;
    ID3D12CommandQueue* queue{};
    uint64_t value{},origin{},pose[2]{};
    uint32_t frame{};
    bool invalid{},rr{};
    std::array<Marker,6> markers;
};
namespace source_timing {
namespace {
std::mutex mutex;
std::array<std::shared_ptr<SourceFrame>,8> slots;
std::atomic<bool> active{false};
bool Reusable(const SourceFrame& f){
    for(const auto& m:f.markers)if(m.recorded && (!m.released || (m.submitted && !m.timing.Complete())))return false;
    return true;
}
bool Initialize(Marker& m,ID3D12Device* d){
    if(m.timing.heap && m.timing.readback)return true;
    D3D12_QUERY_HEAP_DESC q{};q.Type=D3D12_QUERY_HEAP_TYPE_TIMESTAMP;q.Count=2;
    D3D12_RESOURCE_DESC r{};r.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;r.Width=16;
    r.Height=r.DepthOrArraySize=r.MipLevels=1;r.SampleDesc.Count=1;r.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES h{};h.Type=D3D12_HEAP_TYPE_READBACK;
    return SUCCEEDED(d->CreateQueryHeap(&q,IID_PPV_ARGS(&m.timing.heap))) &&
        SUCCEEDED(d->CreateCommittedResource(&h,D3D12_HEAP_FLAG_NONE,&r,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&m.timing.readback)));
}
std::optional<double> End(const Marker& m){
    if(!m.submitted || !m.timing.Complete())return {};
    const auto& c=m.timing.clock;
    if(!c.gpuHz || !c.cpuHz)return {};
    void* p{};D3D12_RANGE range{0,16};
    if(FAILED(m.timing.readback->Map(0,&range,&p)))return {};
    const auto* ticks=static_cast<const uint64_t*>(p);auto start=ticks[0],end=ticks[1];
    D3D12_RANGE none{};m.timing.readback->Unmap(0,&none);
    const auto ms=double(c.cpu)*1000./double(c.cpuHz)+GpuCorrelation::Delta(end,c.gpu)*1000./double(c.gpuHz);
    // Calibration is taken after Execute; a fast marker can precede CPU submit
    // observation. Preserve that sign rather than rejecting/clamping it.
    if(end<start || !std::isfinite(ms) || ms<m.cpuRecord-.25 || std::abs(ms-m.cpuSubmit)>1000)return {};
    return ms;
}
}
bool Wanted(uint32_t frame){return flight::Enabled() && refresh::Targeted() && frame%16==0;}
void Record(ID3D12GraphicsCommandList* list,uint32_t frame,uint64_t pose,uint64_t origin,unsigned eye,Stage stage,bool rr){
    if(!list || !Wanted(frame) || eye>1 || unsigned(stage)>2 || !pose)return;
    Microsoft::WRL::ComPtr<ID3D12Device> currentDevice;
    if(FAILED(list->GetDevice(IID_PPV_ARGS(&currentDevice))))return;
    std::lock_guard lock(mutex);
    std::shared_ptr<SourceFrame> f;
    for(auto& s:slots)if(s && s->frame==frame && s->origin==origin && s->device.Get()==currentDevice.Get()){f=s;break;}
    if(!f){
        for(auto& s:slots)if(!s || (s.use_count()==1 && Reusable(*s))){
            // Query storage is not recycled until all recorded lists reset and
            // every submitted use completes. Retained frames cannot be replaced.
            Microsoft::WRL::ComPtr<ID3D12Device> device;
            if(FAILED(list->GetDevice(IID_PPV_ARGS(&device))))break;
            if(!s || s->device.Get()!=device.Get()){
                s=std::make_shared<SourceFrame>();s->device=device;
                if(FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&s->fence))))s->invalid=true;
            }else {
                s->invalid=false;s->rr=false;s->queue=nullptr;s->pose[0]=s->pose[1]=0;
                for(auto& m:s->markers){
                    auto heap=m.timing.heap;auto readback=m.timing.readback;
                    m={};m.timing.heap=std::move(heap);m.timing.readback=std::move(readback);
                }
            }
            s->frame=frame;s->origin=origin;f=s;active=true;break;
        }
    }
    if(!f || f->invalid){flight::Count(flight::Event::SourceMarkerSkipped);return;}
    if(f->pose[eye] && f->pose[eye]!=pose){f->invalid=true;return;}
    f->pose[eye]=pose;f->rr|=rr;
    auto& m=f->markers[eye*3+unsigned(stage)];
    if(m.recorded){f->invalid=true;flight::Count(flight::Event::SourceMarkerDuplicate);return;}
    if(!Initialize(m,f->device.Get()) || !cvr::gpu::KeepCommandResources(list,{m.timing.heap.Get(),m.timing.readback.Get()})){
        f->invalid=true;flight::Count(flight::Event::SourceMarkerSkipped);return;
    }
    m.list=list;m.recorded=true;m.cpuRecord=Qpc();
    m.timing.Start(list);m.timing.End(list);
    flight::Count(flight::Event::SourceMarkerRecorded);
}
void Submitted(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(!active.load(std::memory_order_relaxed) || !queue || !lists)return;
    std::lock_guard lock(mutex);
    for(auto& f:slots)if(f){
        std::array<Marker*,6> batch{};unsigned n=0;
        for(auto& m:f->markers)if(m.recorded && !m.released){
            unsigned occurrences=0;for(UINT i=0;i<count;++i)occurrences+=lists[i]==m.list;
            if(!occurrences)continue;
            if(m.submitted || occurrences!=1){f->invalid=true;flight::Count(flight::Event::SourceMarkerReplay);}
            if(f->queue && f->queue!=queue)f->invalid=true;
            f->queue=queue;m.submitted=true;m.cpuSubmit=Qpc();m.timing.Enqueue(queue);batch[n++]=&m;
        }
        if(!n)continue;
        const auto v=++f->value;
        const bool ok=SUCCEEDED(queue->Signal(f->fence.Get(),v));
        if(!ok){f->invalid=true;flight::Count(flight::Event::SourceMarkerSignalFailed);}
        // Failed signals deliberately quarantine the slot, never manufacture completion.
        for(unsigned i=0;i<n;++i)batch[i]->timing.Submitted(f->fence.Get(),v);
    }
}
void ResetList(ID3D12GraphicsCommandList* list){
    if(!active.load(std::memory_order_relaxed))return;
    std::lock_guard lock(mutex);
    for(auto& f:slots)if(f)for(auto& m:f->markers)if(m.recorded && !m.released && m.list==list){
        m.released=true;if(!m.submitted)f->invalid=true;
    }
}
std::shared_ptr<const SourceFrame> Acquire(uint32_t frame,const uint64_t (&pose)[2],uint64_t origin,ID3D12CommandQueue* queue){
    if(!Wanted(frame))return {};
    std::lock_guard lock(mutex);
    for(const auto& f:slots)if(f && !f->invalid && f->frame==frame && f->origin==origin && f->queue==queue &&
        f->pose[0]==pose[0] && f->pose[1]==pose[1])return f;
    return {};
}
std::optional<SourceRecord> Read(const std::shared_ptr<const SourceFrame>& f,const CaptureTiming::Result& cap,double genEnqueue,double genStart){
    if(!f)return {};
    std::lock_guard lock(mutex);
    if(f->invalid)return {};
    SourceRecord r;r.frame=f->frame;r.origin=f->origin;r.rr=f->rr;
    r.pose[0]=f->pose[0];r.pose[1]=f->pose[1];r.captureEnd=cap.endQpcMs;
    r.captureStart=cap.endQpcMs-cap.spanMs;r.captureEnqueue=cap.endQpcMs-cap.totalMs;
    r.generationEnqueue=genEnqueue;r.generationStart=genStart;
    for(unsigned i=0;i<6;++i){const auto& m=f->markers[i];if(!m.recorded)continue;
        const auto end=End(m);if(!end || *end>r.captureStart+.25)return {};
        r.mask|=1u<<i;r.end[i/3][i%3]=*end;r.recorded[i/3][i%3]=m.cpuRecord;r.submitted[i/3][i%3]=m.cpuSubmit;
    }
    return r.mask?std::optional(r):std::nullopt;
}
void Report(const std::shared_ptr<const SourceFrame>& f,const CaptureTiming::Result& cap,double enqueue,double start){
    if(!f)return;
    if(const auto r=Read(f,cap,enqueue,start)){flight::Count(flight::Event::SourcePaired);flight::Source(*r);}
    else flight::Count(flight::Event::SourceInvalid);
}
}
}
