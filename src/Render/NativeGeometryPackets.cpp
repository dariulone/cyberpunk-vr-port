#include "Render/NativeGeometryPackets.hpp"
#include "Render/NativePacketInputs.hpp"
#include "Render/NativeUploadShadow.hpp"
#include "Render/NativePreparation.hpp"
#include "Render/NativeDrawOrigin.hpp"
#include <windows.h>
#include <algorithm>
#include <cstring>
#include <mutex>
#include <vector>
#include <new>
#if CVR_RUST_VRIK
// The game-code contracts: rust/crates/cvr-render/src/code_tables.rs (through ReadProcessMemory).
#endif

extern "C" {
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's are rust/crates/cvr-render/src/geometry_packets.rs'
__declspec(dllexport) uint32_t CyberpunkVR_GeometryPacketRecordBytes=sizeof(cvr::stereo::packets::Record);
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GeometryPacketHookCompatible{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GeometryPacketRequest{0},CyberpunkVR_GeometryPacketState{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GeometryPacketSeq{0},CyberpunkVR_GeometryPacketCount{0},CyberpunkVR_GeometryPacketDataCount{0};
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // as above
__declspec(dllexport) cvr::stereo::packets::Record CyberpunkVR_GeometryPacketRecords[cvr::stereo::packets::RecordCapacity]{};
__declspec(dllexport) cvr::stereo::packets::Packet CyberpunkVR_GeometryPacketData[cvr::stereo::packets::PacketCapacity]{};
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GeometryPacketResolve{0},CyberpunkVR_GeometryPacketInputCount{0},CyberpunkVR_GeometryPacketInputFailure{0};
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // as above
__declspec(dllexport) uint32_t CyberpunkVR_GeometryPacketInputBytes=sizeof(cvr::stereo::packets::Inputs);
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
__declspec(dllexport) uint64_t CyberpunkVR_GeometryPacketInputs{};
__declspec(dllexport) uint64_t CyberpunkVR_GeometryPacketInputBlob{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GeometryPacketInputBlobBytes{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GeometryOriginMode{1},CyberpunkVR_GeometryOriginCompatible{},CyberpunkVR_GeometryOriginVerified{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_GeometryOriginStats[4]{};
#endif
}
#if CVR_RUST_VRIK
// The capture, the inputs, the stack walks and the draw geometry: rust/crates/cvr-render/src/geometry_packets.rs (the
// engine's memory through ReadProcessMemory, the walks through cvr-engine's unwinder). The exports' definitions above
// and the preparation's pairing stay here, lent through the runtime; ResolveLive's vectors stay its caller's.
struct CvrGeometryPacketRuntime {
    cvr::stereo::packets::Record* records;cvr::stereo::packets::Packet* data;
    std::atomic<uint32_t>* request;std::atomic<uint32_t>* state;std::atomic<uint32_t>* seq;std::atomic<uint32_t>* count;
    std::atomic<uint32_t>* dataCount;std::atomic<uint32_t>* resolve;std::atomic<uint32_t>* inputCount;
    std::atomic<uint32_t>* inputFailure;uint64_t* inputs;uint64_t* inputBlob;std::atomic<uint32_t>* inputBlobBytes;
    std::atomic<uint32_t>* originMode;std::atomic<uint32_t>* originCompatible;std::atomic<uint32_t>* originVerified;
    std::atomic<uint64_t>* originStats;
};
extern "C" {
void cvr_gp_attach(const CvrGeometryPacketRuntime*);
void cvr_gp_frame_boundary(bool);
void cvr_gp_begin(uint32_t,int,const void*,const void*,const void*,cvr::stereo::packets::Ticket*);
void cvr_gp_end(uint64_t,uint32_t);
void cvr_gp_draw_origin(const cvr::stereo::packets::DrawOrigin*);
}
static_assert(sizeof(cvr::stereo::packets::Ticket)==16 && sizeof(cvr::stereo::packets::MappedInstances)==32,"mirrored by geometry_packets.rs");
namespace cvr::stereo::packets {
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's runtime is rust/crates/cvr-render/src/geometry_packets.rs' own
namespace {
const CvrGeometryPacketRuntime kRuntime{CyberpunkVR_GeometryPacketRecords,CyberpunkVR_GeometryPacketData,
    &CyberpunkVR_GeometryPacketRequest,&CyberpunkVR_GeometryPacketState,&CyberpunkVR_GeometryPacketSeq,&CyberpunkVR_GeometryPacketCount,
    &CyberpunkVR_GeometryPacketDataCount,&CyberpunkVR_GeometryPacketResolve,&CyberpunkVR_GeometryPacketInputCount,
    &CyberpunkVR_GeometryPacketInputFailure,&CyberpunkVR_GeometryPacketInputs,&CyberpunkVR_GeometryPacketInputBlob,
    &CyberpunkVR_GeometryPacketInputBlobBytes,&CyberpunkVR_GeometryOriginMode,&CyberpunkVR_GeometryOriginCompatible,
    &CyberpunkVR_GeometryOriginVerified,CyberpunkVR_GeometryOriginStats};
[[maybe_unused]] const bool kAttached=(cvr_gp_attach(&kRuntime),true);
}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's draw origin callbacks are engine_detours.rs'
void EnterDrawOrigin(DrawOrigin origin){cvr_gp_draw_origin(&origin);}
void LeaveDrawOrigin(){cvr_gp_draw_origin(nullptr);}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
void FrameBoundary(bool liveInputs){cvr_gp_frame_boundary(liveInputs);}
Ticket Begin(uint32_t node,int side,const void* renderer,const void* arguments,const void* span){
    Ticket ticket;cvr_gp_begin(node,side,renderer,arguments,span,&ticket);return ticket;
}
void End(Ticket ticket){cvr_gp_end(ticket.epoch,ticket.index);}
#endif
}
#else
namespace cvr::stereo::packets {
namespace {
std::mutex mutex;
uint64_t epoch{};
uint32_t frame{},limit{};
std::vector<Inputs> inputs;
std::vector<uint8_t> inputBlob;
bool resolveInputs{};
thread_local DrawOriginStack drawOrigins;
constexpr uint32_t MaxInputPackets=65536;
bool Read(uint64_t address,void* destination,size_t size) {
    SIZE_T read{};
    return address && address<=UINT64_MAX-size &&
        ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),destination,size,&read) && read==size;
}
uint64_t Now(){LARGE_INTEGER value{};QueryPerformanceCounter(&value);return value.QuadPart;}
bool ReadInput(void* context,uint64_t address,void* output,size_t bytes){
    if(address>=upload_shadow::AddressTag && address-upload_shadow::AddressTag<UploadShadow::Limit) {
        const auto* source=static_cast<const MappedInstances*>(context);
        return source && upload_shadow::Read(source->gpu,source->resource,address-upload_shadow::AddressTag,output,bytes);
    }
    return Read(address,output,bytes);
}
MappedInstances Uploaded(uint64_t renderer) {
    MappedInstances result{};uint64_t registry{};uint32_t handle{};
    const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    if(renderer && renderer<=UINT64_MAX-0x204 && Read(renderer+0x200,&handle,4) && handle && handle<=0x100000 &&
        Read(game+0x3BCDCA8,&registry,8) && registry && registry<=UINT64_MAX-0x5C0B40-uint64_t(handle-1)*0xB0) {
        std::array<uint8_t,72> record{};
        if(Read(registry+uint64_t(handle-1)*0xB0+0x5C0AF8,record.data(),record.size())) {
            uint32_t bytes{};std::memcpy(&bytes,record.data(),4);result.bytes=bytes;
            std::memcpy(&result.gpu,record.data()+8,8);std::memcpy(&result.resource,record.data()+16,8);std::memcpy(&result.cpu,record.data()+64,8);
        }
    }
    return result;
}
void ClearInputs() {
    std::vector<Inputs>().swap(inputs);CyberpunkVR_GeometryPacketInputs=0;
    std::vector<uint8_t>().swap(inputBlob);CyberpunkVR_GeometryPacketInputBlob=0;CyberpunkVR_GeometryPacketInputBlobBytes=0;
    CyberpunkVR_GeometryPacketInputCount.store(0,std::memory_order_release);resolveInputs=false;
    upload_shadow::Watch(0,0,0);
}
void CopyInputs(const Record& row) {
    if(!resolveInputs || !row.count || CyberpunkVR_GeometryPacketInputFailure.load(std::memory_order_relaxed))return;
    if(row.offset+row.count>MaxInputPackets){CyberpunkVR_GeometryPacketInputFailure=1;return;}
    try {
        inputs.resize(row.offset+row.count);
        uint64_t global{},pool{},renderer{};
        const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        // The consumer loads this exact pool from [global+0x4628]. Check that
        // [global+0x80] is also the renderer argument before decoding anything.
        if(Read(game+0x3BBCAE8,&global,8) && global && global<=UINT64_MAX-0x4630 &&
           Read(global+0x80,&renderer,8) && renderer==row.renderer && Read(global+0x4628,&pool,8)) {
            auto uploaded=Uploaded(renderer);
            if(!uploaded.cpu)uploaded.cpu=upload_shadow::AddressTag;
            for(uint32_t i=0;i<row.count;++i)inputs[row.offset+i]=ResolveDetails(CyberpunkVR_GeometryPacketData[row.offset+i],pool,uploaded,ReadInput,&uploaded,inputBlob);
        }else CyberpunkVR_GeometryPacketInputFailure=3;
    }catch(const std::bad_alloc&){CyberpunkVR_GeometryPacketInputFailure=2;}
    // Publish current owners even after allocation failure; an earlier resize
    // may have moved them. Readers reject the failure instead of a stale pointer.
    CyberpunkVR_GeometryPacketInputs=reinterpret_cast<uintptr_t>(inputs.data());
    CyberpunkVR_GeometryPacketInputBlob=reinterpret_cast<uintptr_t>(inputBlob.data());CyberpunkVR_GeometryPacketInputBlobBytes.store(static_cast<uint32_t>(inputBlob.size()),std::memory_order_release);
    CyberpunkVR_GeometryPacketInputCount.store(static_cast<uint32_t>(inputs.size()),std::memory_order_release);
}
void NativeGroup(Record& row) {
    __try {
        const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        CONTEXT context{};RtlCaptureContext(&context);
        ULONG_PTR low{},high{};GetCurrentThreadStackLimits(&low,&high);
        for(unsigned depth=0;depth<24 && context.Rsp>=low && context.Rsp+8<=high;++depth) {
            if(context.Rip>=game+0x33DDA4 && context.Rip<game+0x33E356) {
                // sub_14033DDA4 (2.31: sub_14023A938). At both native packet-consumer call sites RSI is the node
                // definition and RBX points at the section's 48-byte definition itself ([RSI+0x30] + 48*index,
                // index < [RSI+0x3C]). The plane bucket is no longer held: it is [RSP+0x38] + 0x1C0 *
                // (10*entry[+0x10] + entry[+0x14]) (sub_14228E460), what 2.31 kept at [RBP-0x78].
                uint64_t sections{},base{};uint32_t count{};
                if(context.Rsp>high-0x40 || !Read(context.Rsi+0x30,&sections,8) || !Read(context.Rsi+0x3C,&count,4) ||
                    context.Rbx<sections || (context.Rbx-sections)%48 || (context.Rbx-sections)/48>=std::min(count,1024u))return;
                row.nodeDefinition=context.Rsi;row.entry=context.Rbx;
                if(Read(context.Rsp+0x38,&base,8) && Read(row.entry,row.groupBytes.data(),row.groupBytes.size())) {
                    uint32_t pass{};std::memcpy(&pass,row.groupBytes.data()+0x10,4);
                    row.plane=base+0x1C0*(uint64_t(pass)*10+row.groupBytes[0x14]);row.metadataFlags|=4;
                }
                return;
            }
            DWORD64 image{};const auto function=RtlLookupFunctionEntry(context.Rip,&image,nullptr);
            if(function){PVOID handler{};DWORD64 establisher{};RtlVirtualUnwind(UNW_FLAG_NHANDLER,image,context.Rip,function,&context,&handler,&establisher,nullptr);}
            else {if(!Read(context.Rsp,&context.Rip,8))return;context.Rsp+=8;}
        }
    } __except(EXCEPTION_EXECUTE_HANDLER) {}
}
}
bool ResolveLive(uint64_t renderer,std::span<const Packet> packets,std::vector<Inputs>& result,std::vector<uint8_t>& blob,bool geometryOnly){
    uint64_t global{},pool{},actual{};const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    if(packets.size()>MaxSpanPackets || !Read(game+0x3BBCAE8,&global,8) || !global || global>UINT64_MAX-0x4630 ||
        !Read(global+0x80,&actual,8) || actual!=renderer || !Read(global+0x4628,&pool,8))return false;
    auto uploaded=Uploaded(renderer);if(!uploaded.cpu)uploaded.cpu=upload_shadow::AddressTag;
    result.clear();blob.clear();result.reserve(packets.size());
    for(const auto packet:packets)result.push_back(ResolveDetails(packet,pool,uploaded,ReadInput,&uploaded,blob,geometryOnly));
    return true;
}
static bool DrawGeometryCompatible(uint32_t stride){
    static const auto compatible=[]{
#if CVR_RUST_VRIK
        // The ordinary flush (its consumer detour excluded) and the skinned flush and gather.
        const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        return std::array{cvr_code_table(0,game),cvr_code_table(1,game)};
#else
        struct Region {uint32_t rva,bytes;uint64_t hash;};
        // Exclude the existing 15-byte consumer detour, not its body. 2.5: the flush only moved its
        // constant scratch (+0x1C0 -> +0x200, 448 -> 512 bytes); the consumer body is read in NativePacketInputs.hpp.
        const Region ordinary[]{{0x3353D8,0x168,0xB1E1FF97629AB0DAull},{0x334CCF,0x707,0x5A76D7E54F97EEC4ull}};
        // The separate skin flush consumes batch+0x1C and binds stride 64;
        // its gather selects that stream using packet.second bit 50. 2.5 reordered only the
        // gather's local constant block (one float moved first).
        const Region skinned[]{{0x333C80,0xD8,0x087244DA3165FD2Cull},{0x335544,0x3B9,0x9C5CE84210616C2Aull}};
        std::array<uint8_t,2048> data{};const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        const auto verify=[&](const auto& regions){
            for(const auto& region:regions){if(!Read(game+region.rva,data.data(),region.bytes))return false;
                uint64_t hash=14695981039346656037ull;for(UINT i=0;i<region.bytes;++i)hash=(hash^data[i])*1099511628211ull;
                if(hash!=region.hash)return false;}
            return true;
        };
        return std::array{verify(ordinary),verify(skinned)};
#endif
    }();
    return compatible[0] && (stride==48 || (stride==64 && compatible[1]));
}
static bool LegacyDrawGeometry(uint64_t renderer,uint32_t instances,uint32_t stride,std::array<uint8_t,64>& output){
    if(!renderer || !instances || !DrawGeometryCompatible(stride))return false;
    __try {
        const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));CONTEXT context{};RtlCaptureContext(&context);
        ULONG_PTR low{},high{};GetCurrentThreadStackLimits(&low,&high);
        for(UINT depth=0;depth<48 && context.Rsp>=low && context.Rsp<=high-8;++depth){
            if(context.Rip==game+(stride==64?0x335512:0x335496)){
                const DrawOrigin origin{context.Rsi,context.R14,context.Rbx,context.Rdi,context.R15,
                    stride==64?instances:context.R12,true};
                const auto read=[](void*,uint64_t address,void* out,size_t bytes){return Read(address,out,bytes);};
                return ResolveOrigin(origin,renderer,instances,stride,read,nullptr,output);
            }
            DWORD64 image{};const auto entry=RtlLookupFunctionEntry(context.Rip,&image,nullptr);
            if(entry){PVOID handler{};DWORD64 establisher{};RtlVirtualUnwind(UNW_FLAG_NHANDLER,image,context.Rip,entry,&context,&handler,&establisher,nullptr);}
            else {if(!Read(context.Rsp,&context.Rip,8))return false;context.Rsp+=8;}
        }
    }__except(EXCEPTION_EXECUTE_HANDLER){}
    return false;
}
void EnterDrawOrigin(DrawOrigin origin){drawOrigins.Enter(origin);}
void LeaveDrawOrigin(){drawOrigins.Leave();}
bool CurrentDrawGeometry(uint64_t renderer,uint32_t instances,uint32_t stride,std::array<uint8_t,64>& output){
    if(!renderer || !instances || !DrawGeometryCompatible(stride))return false;
    // The scoped thunk covers only 0x335900/0x335496. Skinned batches call
    // 0x333C80/0x335512 and use verified unwinding until a separate thunk exists.
    if(stride==64){++CyberpunkVR_GeometryOriginStats[3];return LegacyDrawGeometry(renderer,instances,stride,output);}
    const auto mode=CyberpunkVR_GeometryOriginMode.load(std::memory_order_relaxed);
    const auto* origin=drawOrigins.Current();std::array<uint8_t,64> scoped{};
    const auto read=[](void*,uint64_t address,void* out,size_t bytes){return Read(address,out,bytes);};
    const bool valid=mode && CyberpunkVR_GeometryOriginCompatible==1 && origin && ResolveOrigin(*origin,renderer,instances,stride,read,nullptr,scoped);
    if(mode==2 && CyberpunkVR_GeometryOriginVerified==1 && !CyberpunkVR_GeometryOriginStats[1] && valid){
        ++CyberpunkVR_GeometryOriginStats[2];output=scoped;return true;
    }
    const bool legacy=LegacyDrawGeometry(renderer,instances,stride,output);
    if(mode==1 && CyberpunkVR_GeometryOriginCompatible==1){
        if(valid!=legacy || (valid && scoped!=output)){
            ++CyberpunkVR_GeometryOriginStats[1];CyberpunkVR_GeometryOriginVerified=0;return false;
        }
        if(valid && ++CyberpunkVR_GeometryOriginStats[0]>=128 && !CyberpunkVR_GeometryOriginStats[1])CyberpunkVR_GeometryOriginVerified=1;
    }else ++CyberpunkVR_GeometryOriginStats[3];
    return legacy;
}
void FrameBoundary(bool liveInputs) {
    static bool live{};
    if(liveInputs){
        uint64_t global{},renderer{};const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        live=true;MappedInstances source{};
        if(Read(game+0x3BBCAE8,&global,8) && global && global<=UINT64_MAX-0x88 && Read(global+0x80,&renderer,8)){
            source=Uploaded(renderer);
        }
        upload_shadow::Watch(source.cpu?0:source.gpu,source.resource,source.bytes);
        return;
    }
    if(live){upload_shadow::Stop();live=false;}
    if(CyberpunkVR_GeometryPacketState.load(std::memory_order_relaxed)!=1 &&
       !CyberpunkVR_GeometryPacketRequest.load(std::memory_order_relaxed)) {
        if(!CyberpunkVR_GeometryPacketResolve.load(std::memory_order_relaxed) && (CyberpunkVR_GeometryPacketInputCount.load(std::memory_order_acquire) || upload_shadow::Active())) {
            std::lock_guard lock(mutex);CyberpunkVR_GeometryPacketSeq.fetch_add(1,std::memory_order_acq_rel);
            ClearInputs();CyberpunkVR_GeometryPacketSeq.fetch_add(1,std::memory_order_release);
        }
        return;
    }
    std::lock_guard lock(mutex);
    if(CyberpunkVR_GeometryPacketState.load(std::memory_order_relaxed)==1) {
        if(++frame>limit){upload_shadow::Stop();CyberpunkVR_GeometryPacketState.store(2,std::memory_order_release);}
        return;
    }
    const auto request=CyberpunkVR_GeometryPacketRequest.exchange(0,std::memory_order_acq_rel);if(!request)return;
    CyberpunkVR_GeometryPacketSeq.fetch_add(1,std::memory_order_acq_rel);
    ++epoch;frame=1;limit=std::min(request,4u);
    CyberpunkVR_GeometryPacketCount.store(0,std::memory_order_relaxed);CyberpunkVR_GeometryPacketDataCount.store(0,std::memory_order_relaxed);
    const auto resolveMode=CyberpunkVR_GeometryPacketResolve.load(std::memory_order_relaxed);
    ClearInputs();resolveInputs=resolveMode==1;CyberpunkVR_GeometryPacketInputFailure=0;
    // Mode 2 observes only the native instance uploads for a bounded GPU probe.
    // It does not resolve/copy every material program and object in the scene.
    if(resolveMode) {
        uint64_t global{},renderer{};const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        if(Read(game+0x3BBCAE8,&global,8) && global && global<=UINT64_MAX-0x88 && Read(global+0x80,&renderer,8)) {
            const auto source=Uploaded(renderer);if(!source.cpu)upload_shadow::Watch(source.gpu,source.resource,source.bytes);
        }
    }
    CyberpunkVR_GeometryPacketSeq.fetch_add(1,std::memory_order_release);
    CyberpunkVR_GeometryPacketState.store(1,std::memory_order_release);
}
Ticket Begin(uint32_t node,int side,const void* renderer,const void* arguments,const void* span) {
    if(preparation::Pairing())preparation::Consume(node,side,arguments,span);
    if(node!=0x33DDA4 || side<0 || side>1 || CyberpunkVR_GeometryPacketState.load(std::memory_order_relaxed)!=1)return {};
    std::lock_guard lock(mutex);
    if(CyberpunkVR_GeometryPacketState.load(std::memory_order_relaxed)!=1)return {};
    const auto index=CyberpunkVR_GeometryPacketCount.load(std::memory_order_relaxed);
    const auto offset=CyberpunkVR_GeometryPacketDataCount.load(std::memory_order_relaxed);
    if(index>=RecordCapacity){CyberpunkVR_GeometryPacketState.store(3,std::memory_order_release);return {};}
    Record row{};row.sequence=index+1;row.beginQpc=Now();row.renderer=reinterpret_cast<uintptr_t>(renderer);
    row.arguments=reinterpret_cast<uintptr_t>(arguments);row.frame=frame;row.node=node;row.side=side;row.thread=GetCurrentThreadId();row.offset=offset;
    auto status=Status::BadArguments;
    bool full=false;
    if(Read(row.arguments,row.argumentBytes.data(),row.argumentBytes.size())) {
        std::memcpy(&row.context,row.argumentBytes.data(),sizeof(row.context));
        if(row.context && row.context<=UINT64_MAX-0x18 && Read(row.context+0x18,&row.view,sizeof(row.view))) {
            row.metadataFlags|=1;
            if(row.view && row.view<=UINT64_MAX-0x2C40 && Read(row.view+0x2C40,&row.storage,sizeof(row.storage)))row.metadataFlags|=2;
        }
        uint64_t range[2]{};
        status=Status::BadSpan;
        if(Read(reinterpret_cast<uintptr_t>(span),range,sizeof(range))) {
            row.begin=range[0];row.end=range[1];
            if(row.end>=row.begin && (row.end-row.begin)%sizeof(Packet)==0 && (row.begin%alignof(Packet))==0) {
                const auto count=(row.end-row.begin)/sizeof(Packet);
                if(!count)status=Status::Empty;
                else if(!row.begin)status=Status::BadSpan;
                else if(count>MaxSpanPackets)status=Status::TooLarge;
                else if(count>PacketCapacity-offset){status=Status::TooLarge;full=true;}
                else {
                    status=Status::Unreadable;
                    // The caller owns this immutable span throughout the native
                    // call. Copy it now, before the shared view scratch is reused.
                    if(Read(row.begin,CyberpunkVR_GeometryPacketData+offset,size_t(count)*sizeof(Packet))) {
                        row.count=static_cast<uint32_t>(count);status=Status::Copied;
                    }
                }
            }
        }
    }
    row.status=static_cast<uint32_t>(status);
    NativeGroup(row);
    CyberpunkVR_GeometryPacketSeq.fetch_add(1,std::memory_order_acq_rel);
    CopyInputs(row);
    CyberpunkVR_GeometryPacketRecords[index]=row;
    CyberpunkVR_GeometryPacketDataCount.store(offset+row.count,std::memory_order_relaxed);
    CyberpunkVR_GeometryPacketCount.store(index+1,std::memory_order_relaxed);
    CyberpunkVR_GeometryPacketSeq.fetch_add(1,std::memory_order_release);
    if(full)CyberpunkVR_GeometryPacketState.store(3,std::memory_order_release);
    return {epoch,index};
}
void End(Ticket ticket) {
    if(ticket.index==UINT32_MAX)return;
    std::lock_guard lock(mutex);
    if(ticket.epoch!=epoch || ticket.index>=CyberpunkVR_GeometryPacketCount.load(std::memory_order_relaxed))return;
    CyberpunkVR_GeometryPacketSeq.fetch_add(1,std::memory_order_acq_rel);
    CyberpunkVR_GeometryPacketRecords[ticket.index].endQpc=Now();
    CyberpunkVR_GeometryPacketSeq.fetch_add(1,std::memory_order_release);
}
}
#endif
