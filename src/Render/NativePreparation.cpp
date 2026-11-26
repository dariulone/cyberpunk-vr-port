#include "Render/NativePreparation.hpp"
#include "Render/NativeDepthPreparation.hpp"
#include "Render/NativeGraphParent.hpp"
#include "Render/NativeSceneChild.hpp"
#include "Render/PairedPassFrontier.hpp"
#include <windows.h>
#include <mutex>
#include <vector>
#include <algorithm>
#include <cstring>
#include <unordered_map>
#if CVR_RUST_VRIK
// The game-code contract: rust/crates/cvr-render/src/binding_code.rs (0 pair preparation, 1 material read, 2 material
// binding; through ReadProcessMemory).
#endif

#if !CVR_RUST_VRIK
extern "C" {
#endif
#if CVR_RUST_VRIK
// The exported state is Rust's (rust/crates/cvr-render/src/native_state_exports.rs), declared in the header.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationRequest{},CyberpunkVR_PreparationState{},CyberpunkVR_PreparationSeq{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationCount{},CyberpunkVR_PreparationPacketCount{},CyberpunkVR_PreparationFailure{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationRecords{},CyberpunkVR_PreparationPackets{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationPairRequest{},CyberpunkVR_PreparationPairState{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationPairCompatible{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationPairData[8]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationPairEligibility[8]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationPairRoot[8]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationCameraTrace{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationAllStages{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationGraphParents{};
__declspec(dllexport) uint32_t CyberpunkVR_PreparationGraphParentCount{},CyberpunkVR_PreparationGraphParentBytes=sizeof(cvr::stereo::preparation::GraphParentRecord);
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationPairVerified{},CyberpunkVR_PreparationRootSkipped{},CyberpunkVR_PreparationRootCompatible{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationPairMergeLimit{1},CyberpunkVR_PreparationPairMerged{},CyberpunkVR_PreparationPairCompleted{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PreparationPairGeneration{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationRunRequest{},CyberpunkVR_PreparationRunState{},CyberpunkVR_PreparationRunStop{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationRunFailure{},CyberpunkVR_PreparationRunFrames{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationRunDepth[2]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationRunEyeCopies{},CyberpunkVR_PreparationRunEyeSubmissions{};
#endif
#if !CVR_RUST_VRIK
}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The trace and the pair preparation: rust/crates/cvr-render/src/preparation_trace.rs (the exports by their symbols),
// with the graph parent, the thread's graph context and the graph job polls; the DrawScope class and
// InvokeGraphContext's scene-child scope stay here.
extern "C" {
void cvr_prep_attach(const void*);
bool cvr_prep_current_parent(int32_t,uint32_t,cvr::stereo::preparation::GraphParentRecord*);
bool cvr_prep_revalidate_parent(const cvr::stereo::preparation::GraphParentRecord*);
uint64_t cvr_prep_graph_context_swap(uint64_t);
void cvr_prep_poll_graph_jobs();
void cvr_prep_frame_boundary();
void cvr_prep_begin_stage(uint64_t,uint64_t,uint64_t,uint32_t,uint32_t,int32_t,cvr::stereo::preparation::Ticket*);
void cvr_prep_end_stage(const cvr::stereo::preparation::Ticket*);
void cvr_prep_output(uint32_t,uint64_t);
void cvr_prep_consume(uint32_t,int32_t,uint64_t,uint64_t);
void cvr_prep_begin_task(uint32_t,uint64_t,uint64_t,int32_t,cvr::stereo::preparation::MaterializeTicket*);
void cvr_prep_end_task(uint32_t,const cvr::stereo::preparation::MaterializeTicket*);
void* cvr_prep_prepare_pair(cvr::stereo::preparation::PrepareGraph,void*,void*,void*,uint64_t,uint64_t,bool,uint64_t);
bool cvr_prep_suppress_rtt(uint64_t,uint64_t,bool);
void cvr_prep_root_completed(uint64_t,uint64_t);
void cvr_prep_eye_copy(uint64_t,uint64_t,uint64_t);
void cvr_prep_copy_source(const cvr::stereo::preparation::CopySource*,cvr::stereo::preparation::CopySource*);
void cvr_prep_draw_scope(uint32_t,void*,uint32_t,int32_t,uint64_t);
void cvr_prep_draw_tag(cvr::stereo::preparation::DrawTag*);
void cvr_prep_draw_recorded(uint64_t);
void cvr_prep_submit_lists(uint64_t,uint32_t,ID3D12CommandList* const*,bool);
void cvr_prep_reset_list(uint64_t);
}
namespace cvr::stereo::preparation {
static_assert(sizeof(Ticket)==136 && sizeof(MaterializeTicket)==120 && sizeof(CopySource)==16 && sizeof(DrawTag)==24 &&
    sizeof(DrawScope)==904,"mirrored by preparation_trace.rs");
namespace {
// The exports in preparation_trace.rs's Exports order.
const void* const kExports[]{
    &CyberpunkVR_PreparationRequest,&CyberpunkVR_PreparationState,&CyberpunkVR_PreparationSeq,&CyberpunkVR_PreparationCount,
    &CyberpunkVR_PreparationPacketCount,&CyberpunkVR_PreparationFailure,&CyberpunkVR_PreparationRecords,&CyberpunkVR_PreparationPackets,
    &CyberpunkVR_PreparationPairRequest,&CyberpunkVR_PreparationPairState,&CyberpunkVR_PreparationPairCompatible,
    CyberpunkVR_PreparationPairData,CyberpunkVR_PreparationPairEligibility,CyberpunkVR_PreparationPairRoot,
    &CyberpunkVR_PreparationCameraTrace,&CyberpunkVR_PreparationAllStages,&CyberpunkVR_PreparationGraphParents,
    &CyberpunkVR_PreparationGraphParentCount,&CyberpunkVR_PreparationPairVerified,&CyberpunkVR_PreparationRootSkipped,
    &CyberpunkVR_PreparationRootCompatible,&CyberpunkVR_PreparationPairMergeLimit,&CyberpunkVR_PreparationPairMerged,
    &CyberpunkVR_PreparationPairCompleted,&CyberpunkVR_PreparationPairGeneration,&CyberpunkVR_PreparationRunRequest,
    &CyberpunkVR_PreparationRunState,&CyberpunkVR_PreparationRunStop,&CyberpunkVR_PreparationRunFailure,&CyberpunkVR_PreparationRunFrames,
    CyberpunkVR_PreparationRunDepth,&CyberpunkVR_PreparationRunEyeCopies,&CyberpunkVR_PreparationRunEyeSubmissions};
static_assert(sizeof(kExports)==264,"mirrored by preparation_trace.rs");
const bool kAttached=(cvr_prep_attach(kExports),true);
}
bool Active(){return CyberpunkVR_PreparationState.load(std::memory_order_relaxed)==1;}
bool RunActive(){return CyberpunkVR_PreparationRunState.load(std::memory_order_acquire)==1;}
void PollGraphJobs(){cvr_prep_poll_graph_jobs();}
void FrameBoundary(){cvr_prep_frame_boundary();}
Ticket BeginStage(const void* context,const void* buckets,const void* output,UINT flags0,UINT flags1,int side){
    Ticket ticket;cvr_prep_begin_stage(reinterpret_cast<uintptr_t>(context),reinterpret_cast<uintptr_t>(buckets),reinterpret_cast<uintptr_t>(output),
        flags0,flags1,side,&ticket);return ticket;
}
void EndStage(const Ticket& ticket){cvr_prep_end_stage(&ticket);}
void Output(Kind kind,const void* output){cvr_prep_output(uint32_t(kind),reinterpret_cast<uintptr_t>(output));}
void Consume(UINT node,int side,const void* context,const void* span){
    cvr_prep_consume(node,side,reinterpret_cast<uintptr_t>(context),reinterpret_cast<uintptr_t>(span));
}
MaterializeTicket BeginMaterialize(const void* task,int side){
    MaterializeTicket ticket;cvr_prep_begin_task(0,reinterpret_cast<uintptr_t>(task),0,side,&ticket);return ticket;
}
void EndMaterialize(const MaterializeTicket& ticket){cvr_prep_end_task(0,&ticket);}
MaterializeTicket BeginProxyGather(const void* context,const void* range,int side){
    MaterializeTicket ticket;cvr_prep_begin_task(1,reinterpret_cast<uintptr_t>(context),reinterpret_cast<uintptr_t>(range),side,&ticket);return ticket;
}
void EndProxyGather(const MaterializeTicket& ticket){cvr_prep_end_task(1,&ticket);}
void* PreparePair(PrepareGraph original,void* manager,void* arguments,void* requested,uint64_t eyeName,uintptr_t mainView,bool gameplay,uintptr_t vrcamView){
    return cvr_prep_prepare_pair(original,manager,arguments,requested,eyeName,mainView,gameplay,vrcamView);
}
bool SuppressRtt(const void* frameData,uint64_t eyeName,bool gameplay){return cvr_prep_suppress_rtt(reinterpret_cast<uintptr_t>(frameData),eyeName,gameplay);}
void RootCompleted(const void* frameData,uint64_t generationBefore){cvr_prep_root_completed(reinterpret_cast<uintptr_t>(frameData),generationBefore);}
void EyeCopyRecorded(const void* list,const void* source,const void* destination){
    cvr_prep_eye_copy(reinterpret_cast<uintptr_t>(list),reinterpret_cast<uintptr_t>(source),reinterpret_cast<uintptr_t>(destination));
}
CopySource SetCopySource(CopySource value){CopySource previous;cvr_prep_copy_source(&value,&previous);return previous;}
DrawScope::DrawScope(uint32_t node,int side,const void* arguments){cvr_prep_draw_scope(0,this,node,side,reinterpret_cast<uintptr_t>(arguments));}
DrawTag CurrentDrawTag(){DrawTag tag;cvr_prep_draw_tag(&tag);return tag;}
GraphContextScope::GraphContextScope(uint64_t context):previous_(cvr_prep_graph_context_swap(context)){}
GraphContextScope::~GraphContextScope(){cvr_prep_graph_context_swap(previous_);}
bool CurrentGraphParent(int side,uint32_t expectedFrame,GraphParentRecord& row){return cvr_prep_current_parent(side,expectedFrame,&row);}
bool RevalidateGraphParent(const GraphParentRecord& expected){return cvr_prep_revalidate_parent(&expected);}
DrawScope::~DrawScope(){cvr_prep_draw_scope(1,this,0,0,0);}
void DrawRecorded(const void* list){cvr_prep_draw_recorded(reinterpret_cast<uintptr_t>(list));}
void SubmitLists(const void* queue,uint32_t count,ID3D12CommandList* const* lists,bool serialized){
    cvr_prep_submit_lists(reinterpret_cast<uintptr_t>(queue),count,lists,serialized);
}
void ResetList(const void* list){cvr_prep_reset_list(reinterpret_cast<uintptr_t>(list));}
}
#endif
#else
namespace cvr::stereo::preparation {
namespace {
std::mutex mutex;UINT frame{},limit{};uint64_t epoch{},nextScope{},nextSubmission{};
UINT lastSkipFrame{};
UINT runBudget{};uint64_t runDeadline{},runViews[2]{};
std::unordered_map<const void*,bool> runCopies; // false: recorded, true: submitted; reset retires the generation
std::vector<Record> records;std::vector<packets::Packet> data;
std::vector<GraphParentRecord> graphParents;
thread_local Record current;thread_local uint64_t currentEpoch{};
thread_local DrawScope* drawScope{};
thread_local uint64_t graphContext{};
thread_local CopySource copySource;
std::mutex parentMutex;
std::mutex parentCodeMutex;graph_parent::CodeReceipt parentCode;
std::shared_ptr<const graph_snapshot::Snapshot> parentSource;
uint64_t parentBase{},parentFingerprint{};
uint32_t parentFrame{};
uint64_t parentEpoch{};
std::atomic<void(*)()> graphJobPoll{};
std::atomic<void(*)()> sceneUpdatePoll{};
bool Read(uint64_t address,void* out,size_t bytes){SIZE_T copied{};return address && address<=UINT64_MAX-bytes &&
    ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),out,bytes,&copied) && copied==bytes;}
void Publish(){CyberpunkVR_PreparationRecords=reinterpret_cast<uintptr_t>(records.data());CyberpunkVR_PreparationPackets=reinterpret_cast<uintptr_t>(data.data());
    CyberpunkVR_PreparationCount=UINT(records.size());CyberpunkVR_PreparationPacketCount=UINT(data.size());
    CyberpunkVR_PreparationGraphParents=reinterpret_cast<uintptr_t>(graphParents.data());CyberpunkVR_PreparationGraphParentCount=UINT(graphParents.size());}
void View(Record& row) {
    if(!row.context || row.context>UINT64_MAX-0x20 || !Read(row.context+0x18,&row.view,8) || !row.view || row.view>UINT64_MAX-0x2C48 ||
        !Read(row.view+0x2C40,&row.storage,8))row.status=1;
}
bool Arguments(Record& row,const void* source) {
    std::array<uint8_t,32> bytes{};if(!Read(reinterpret_cast<uintptr_t>(source),bytes.data(),bytes.size())){row.status=1;return false;}
    std::memcpy(&row.context,bytes.data(),8);std::memcpy(&row.stage,bytes.data()+0x10,4);std::memcpy(&row.reserved,bytes.data()+0x14,4);
    row.mode=UINT(bytes[0x18])|(UINT(bytes[0x19])<<8)|(UINT(bytes[0x1A])<<16);View(row);return !row.status;
}
void Copy(Record& row,uint64_t begin,UINT count) {
    row.offset=UINT(data.size());
    if(!count)return;
    if(count>32768 || count>PacketCapacity-data.size()){row.status=2;CyberpunkVR_PreparationFailure=2;return;}
    const auto offset=data.size();data.resize(offset+count);
    if(!Read(begin,data.data()+offset,size_t(count)*16)){data.resize(offset);row.status=1;return;}row.count=count;
}
void Array(Record& row) {
    struct ArrayHeader {uint64_t data;uint32_t capacity,count;};ArrayHeader array{};
    if(!Read(row.output,&array,sizeof(array)) || array.count>array.capacity){row.status=1;return;}Copy(row,array.data,array.count);
}
void Append(Record row,Kind kind,bool array=false) {
    if(records.size()>=RecordCapacity){CyberpunkVR_PreparationFailure=3;return;}
    row.kind=UINT(kind);row.sequence=records.size()+1;row.frame=frame;row.thread=GetCurrentThreadId();LARGE_INTEGER now{};QueryPerformanceCounter(&now);row.qpc=now.QuadPart;
    if(array)Array(row);records.push_back(row);
}
template<class F>void Change(F&& f) {
    if(!Active())return;std::lock_guard lock(mutex);if(!Active())return;CyberpunkVR_PreparationSeq.fetch_add(1);
    try{f();}catch(...){CyberpunkVR_PreparationFailure=4;}
    Publish();CyberpunkVR_PreparationSeq.fetch_add(1);
}
template<class F>void PairChange(F&& f) {
    if(!Pairing())return;std::lock_guard lock(mutex);if(!Pairing())return;
    const bool trace=Active();if(trace)CyberpunkVR_PreparationSeq.fetch_add(1);
    try{f(trace);}catch(...){CyberpunkVR_PreparationFailure=4;CyberpunkVR_PreparationRunFailure=4;}
    if(trace){Publish();CyberpunkVR_PreparationSeq.fetch_add(1);}
}
UINT PairBudget(){return RunActive()?runBudget:CyberpunkVR_PreparationPairMergeLimit.load();}
UINT PairFrame(){return RunActive()?CyberpunkVR_PreparationRunFrames.load():frame;}
void RunBoundary(){
    if(RunActive()){
        const auto skipped=CyberpunkVR_PreparationRootSkipped.load(),completed=CyberpunkVR_PreparationPairCompleted.load();
        if(GetTickCount64()>=runDeadline)CyberpunkVR_PreparationRunFailure=2;
        if(CyberpunkVR_PreparationRunFailure || ((completed>=runBudget || CyberpunkVR_PreparationRunStop) && completed==skipped)){
            CyberpunkVR_PreparationPairRequest=0;
            if(std::any_of(runCopies.begin(),runCopies.end(),[](const auto& item){return !item.second;}))CyberpunkVR_PreparationRunFailure=8;
            CyberpunkVR_PreparationRunState.store(CyberpunkVR_PreparationRunFailure?3:2,std::memory_order_release);
            runCopies.clear();eager_depth::EndRun();return;
        }
        ++CyberpunkVR_PreparationRunFrames;return;
    }
    const auto request=CyberpunkVR_PreparationRunRequest.exchange(0);if(!request)return;
    CyberpunkVR_PreparationRunFailure=0;
    if(request<30 || request>1200 || Active() || CyberpunkVR_PreparationRequest || CyberpunkVR_PreparationPairRequest ||
        CyberpunkVR_PreparationPairVerified!=1 || CyberpunkVR_PreparationRootCompatible!=1 || CyberpunkVR_PreparationPairCompatible!=1){
        CyberpunkVR_PreparationRunFailure=1;CyberpunkVR_PreparationRunState=3;return;
    }
    runBudget=request;runDeadline=GetTickCount64()+45000;eager_depth::BeginRun();
    runViews[0]=CyberpunkVR_PreparationPairData[4];runViews[1]=CyberpunkVR_PreparationPairData[3];runCopies.clear();
    CyberpunkVR_PreparationRunFrames=1;CyberpunkVR_PreparationRunStop=0;
    CyberpunkVR_PreparationRunDepth[0]=0;CyberpunkVR_PreparationRunDepth[1]=0;
    CyberpunkVR_PreparationRunEyeCopies=0;CyberpunkVR_PreparationRunEyeSubmissions=0;
    CyberpunkVR_PreparationRootSkipped=0;CyberpunkVR_PreparationPairMerged=0;CyberpunkVR_PreparationPairCompleted=0;lastSkipFrame=0;
    CyberpunkVR_PreparationRunState=1;CyberpunkVR_PreparationPairRequest=3;
}
}
bool Active(){return CyberpunkVR_PreparationState.load(std::memory_order_relaxed)==1;}
bool RunActive(){return CyberpunkVR_PreparationRunState.load(std::memory_order_acquire)==1;}
bool Pairing(){return Active() || RunActive();}
void SetGraphJobPoll(void(*callback)()){graphJobPoll.store(callback,std::memory_order_release);}
void SetSceneUpdatePoll(void(*callback)()){sceneUpdatePoll.store(callback,std::memory_order_release);}
void PollGraphJobs(){if(const auto callback=graphJobPoll.load(std::memory_order_acquire))callback();if(const auto callback=sceneUpdatePoll.load(std::memory_order_acquire))callback();}
void PublishGraphParentSource(const graph_snapshot::Snapshot& source,uint64_t base)try{
    uint32_t capturedFrame{};uint64_t capturedEpoch{};
    {std::lock_guard lock(mutex);if(Active()){capturedFrame=frame;capturedEpoch=epoch;}}
    std::lock_guard lock(parentMutex);parentSource.reset();parentFingerprint=0;parentBase=base;
    parentFrame=capturedFrame;parentEpoch=capturedEpoch;if(!capturedFrame || source.failure || source.nodes.empty())return;
    parentSource=std::make_shared<const graph_snapshot::Snapshot>(source);
    parentFingerprint=paired_frontier::detail::Fingerprint(source,base);
}catch(...){std::lock_guard lock(parentMutex);parentSource.reset();parentBase=parentFingerprint=parentEpoch=0;parentFrame=0;}
void PrepareGraphParentCodeGuards(uint64_t base){
    std::lock_guard lock(parentCodeMutex);if(parentCode.Attempted())return;
    constexpr graph_parent::Code functions[]{
        {0x40B10C,0x82,0x177EF1EE2846E667ull},{0x333374,0x98,0x6B7C1293E9814833ull},
        {0x40AEF8,0xD8,0x4232C2AC8E043AADull},{0xB3FCFC,0xAF,0x91259D624E26FF57ull}};
    parentCode.Capture(base,Read,functions);
}
bool DescribeGraphParent(uint64_t context,int side,uint32_t expectedFrame,uint64_t expectedEpoch,GraphParentRecord& row){
    row.status=1;
    if(side<0 || side>1)return false;
    std::shared_ptr<const graph_snapshot::Snapshot> source;uint64_t fingerprint{},base{},sourceEpoch{};uint32_t sourceFrame{};
    {std::lock_guard lock(parentMutex);source=parentSource;fingerprint=parentFingerprint;base=parentBase;sourceFrame=parentFrame;sourceEpoch=parentEpoch;}
    if(!source || !fingerprint || !base || !expectedFrame || expectedFrame!=sourceFrame || !expectedEpoch || expectedEpoch!=sourceEpoch)return false;
    PrepareGraphParentCodeGuards(base);
    {std::lock_guard lock(parentCodeMutex);row.status=2;if(!parentCode.VerifiedFor(base))return false;}
    graph_parent::Context native;
    row.status=3;if(!graph_parent::ReadContext(Read,context,native) || native.viewIndex!=UINT(side) ||
        native.view!=CyberpunkVR_PreparationPairData[side?3:4])return false;
    graph_parent::Receipt receipt;row.manager=native.manager;
    row.status=4;if(!graph_parent::Resolve(*source,native,receipt))return false;
    row.fingerprint=fingerprint;row.object=receipt.object;row.work=receipt.work;row.name=receipt.name;
    row.handle=receipt.handle;row.ordinal=receipt.ordinal;row.viewIndex=receipt.viewIndex;row.slot=receipt.slot;row.status=0;return true;
}
void FrameBoundary() {
    if(!Active() && !CyberpunkVR_PreparationRequest.load(std::memory_order_relaxed) && !RunActive() && !CyberpunkVR_PreparationRunRequest.load())return;
    std::lock_guard lock(mutex);
    RunBoundary();
    if(Active()){if(++frame>limit)CyberpunkVR_PreparationState=2;return;}
    const auto request=CyberpunkVR_PreparationRequest.exchange(0);if(!request)return;
    CyberpunkVR_PreparationSeq.fetch_add(1);records.clear();data.clear();graphParents.clear();++epoch;frame=1;
    limit=std::min(request,CyberpunkVR_PreparationCameraTrace.load()?8u:4u);nextScope=nextSubmission=0;
    if(CyberpunkVR_PreparationPairRequest.load()==3 && !RunActive()){
        CyberpunkVR_PreparationRootSkipped=0;CyberpunkVR_PreparationPairMerged=0;CyberpunkVR_PreparationPairCompleted=0;lastSkipFrame=0;
    }
    CyberpunkVR_PreparationFailure=0;Publish();CyberpunkVR_PreparationSeq.fetch_add(1);CyberpunkVR_PreparationState=1;
}
Ticket BeginStage(const void* context,const void* buckets,const void* output,UINT flags0,UINT flags1,int side) {
    Ticket ticket;
    Change([&]{Record row;row.context=0;row.input=reinterpret_cast<uintptr_t>(buckets);row.output=reinterpret_cast<uintptr_t>(output);row.flags0=flags0;row.flags1=flags1;row.side=side;
        row.scope=++nextScope;ticket={epoch,row.scope,currentEpoch,current};
        if(!Arguments(row,context)){if(side>=0)CyberpunkVR_PreparationFailure=1;row.scope=0;current={};currentEpoch=0;Append(row,Kind::StageBegin);return;}
        current=row;currentEpoch=epoch;Append(row,Kind::StageBegin);
    });return ticket;
}
void EndStage(const Ticket& ticket) {
    if(!ticket.scope)return;
    Change([&]{if(ticket.epoch==epoch && currentEpoch==epoch && current.scope==ticket.scope)Append(current,Kind::StageEnd,true);});
    current=ticket.previous;currentEpoch=ticket.previousEpoch;
}
void Output(Kind kind,const void* output) {
    Change([&]{if(currentEpoch!=epoch || !current.scope)return;auto row=current;row.output=reinterpret_cast<uintptr_t>(output);Append(row,kind,true);});
}
void Consume(UINT node,int side,const void* context,const void* span) {
    if(node!=0x33DDA4 || side<0 || side>1)return;
    if(RunActive()){
        Record row;row.side=side;
        if(Arguments(row,context) && row.stage==3){
            if(row.view==runViews[side])++CyberpunkVR_PreparationRunDepth[side];
            else CyberpunkVR_PreparationRunFailure=5;
        }
    }
    Change([&]{Record row;row.side=side;Arguments(row,context);
        uint64_t range[2]{};if(!Read(reinterpret_cast<uintptr_t>(span),range,sizeof(range)) || range[1]<range[0] || (range[1]-range[0])%16 || (range[1]-range[0])/16>32768)row.status=1;
        else Copy(row,range[0],UINT((range[1]-range[0])/16));Append(row,Kind::Consume);});
}
void Graph(const void* manager,bool after) {
    Change([&]{Record row;row.context=reinterpret_cast<uintptr_t>(manager);
        uint64_t views{};uint32_t count{};
        if(row.context>UINT64_MAX-0x58 || !Read(row.context+0x48,&views,8) || !Read(row.context+0x54,&count,4) || count>64){row.status=1;Append(row,after?Kind::GraphAfter:Kind::GraphBefore);return;}
        if(!count){Append(row,after?Kind::GraphAfter:Kind::GraphBefore);return;}
        row.flags0=count;
        for(UINT i=0;i<count;++i){row.flags1=i;row.status=0;row.view=row.storage=row.input=0;
            if(!Read(views+UINT64(i)*8,&row.view,8) || row.view>UINT64_MAX-0x2C48 || !Read(row.view+0x2C40,&row.storage,8) || !Read(row.view+0x28,&row.input,8))row.status=1;
            Append(row,after?Kind::GraphAfter:Kind::GraphBefore);}
    });
}
void Cull(const void* job,const void* output,int side,bool after,const void* definition) {
    Change([&]{Record row;row.context=reinterpret_cast<uintptr_t>(job);row.output=reinterpret_cast<uintptr_t>(output);row.side=side;View(row);
        row.input=reinterpret_cast<uintptr_t>(definition);if(definition)Read(row.input+0x18,&row.flags0,4);
        Read(row.context+0x30,&row.flags1,1);Read(row.context+0x34,&row.mode,4);
        Append(row,after?Kind::CullEnd:Kind::CullBegin);});
}
MaterializeTicket BeginMaterialize(const void* task,int side){
    MaterializeTicket ticket;
    Change([&]{Record row;row.side=side;row.context=reinterpret_cast<uintptr_t>(task);View(row);
        Read(row.context+72,&row.input,8);Read(row.context+80,&row.output,8);Read(row.context+352,&row.flags0,4);
        Read(row.context+0x30,&row.flags1,1);Read(row.context+0x34,&row.mode,4);
        row.scope=++nextScope;ticket={epoch,row};Append(row,Kind::MaterializeBegin);});
    return ticket;
}
void EndMaterialize(const MaterializeTicket& ticket){
    if(ticket.epoch)Change([&]{if(ticket.epoch==epoch)Append(ticket.row,Kind::MaterializeEnd);});
}
MaterializeTicket BeginProxyGather(const void* context,const void* range,int side){
    MaterializeTicket ticket;
    if(side<0 || side>1)return ticket;
    Change([&]{
        Record row;row.side=side;row.context=reinterpret_cast<uintptr_t>(context);View(row);
        row.scope=++nextScope;row.input=reinterpret_cast<uintptr_t>(range);row.offset=UINT(data.size());row.reserved=2;
        std::array<uint64_t,2> span{},after{};
        if(!Read(row.context+0x48,&row.output,8) || row.output!=row.storage ||
            !Read(row.input,span.data(),sizeof(span)) || span[1]<span[0] || (span[1]-span[0])%8)row.status=1;
        const auto count=(span[1]-span[0])/8;
        if(!row.status && (count>16384 || count*2>PacketCapacity-data.size()))row.status=2;
        if(!row.status){
            std::vector<uint64_t> ids(size_t(count),uint64_t{}),check(ids.size());
            std::vector<packets::Packet> methods;methods.reserve(ids.size()*2);
            if(count && !Read(span[0],ids.data(),ids.size()*8))row.status=1;
            for(const auto id:ids){
                uint64_t table{};std::array<uint64_t,2> functions{};
                if(row.status || id<16 || !Read(id-16,&table,8) || !table || table>UINT64_MAX-0x100 ||
                    !Read(table+0xF0,functions.data(),sizeof(functions)) || !functions[1]){row.status=1;break;}
                methods.push_back({id,functions[1]});methods.push_back({table,functions[0]});
            }
            if(!Read(row.input,after.data(),sizeof(after)) || after!=span ||
                (count && (!Read(span[0],check.data(),check.size()*8) || check!=ids)))row.status=1;
            if(!row.status){row.flags0=UINT(count);row.count=UINT(methods.size());data.insert(data.end(),methods.begin(),methods.end());}
        }
        ticket={epoch,row};Append(row,Kind::ProxyGatherBegin);
    });
    return ticket;
}
void EndProxyGather(const MaterializeTicket& ticket){
    if(ticket.epoch)Change([&]{if(ticket.epoch==epoch)Append(ticket.row,Kind::ProxyGatherEnd);});
}
void* PreparePair(PrepareGraph original,void* manager,void* arguments,void* requested,uint64_t eyeName,uintptr_t mainView,bool gameplay,uintptr_t vrcamView) {
    if(!original)return nullptr;
    if(!gameplay || !Pairing() || !CyberpunkVR_PreparationPairRequest.load(std::memory_order_acquire) || !eyeName || !mainView)
        return original(manager,arguments,requested);
    struct Requests {uint64_t data;uint32_t capacity,count;};Requests native{};uint64_t name{},first{};
    const bool readable=Read(reinterpret_cast<uintptr_t>(requested),&native,sizeof(native));
    if(native.count)Read(native.data,&first,8);
    const bool mainRead=mainView<=UINT64_MAX-0x30 && Read(mainView+0x28,&name,8);
    ++CyberpunkVR_PreparationPairEligibility[0];CyberpunkVR_PreparationPairEligibility[1]=native.count;CyberpunkVR_PreparationPairEligibility[2]=native.capacity;
    CyberpunkVR_PreparationPairEligibility[3]=first;CyberpunkVR_PreparationPairEligibility[4]=eyeName;CyberpunkVR_PreparationPairEligibility[5]=name;
    CyberpunkVR_PreparationPairEligibility[6]=mainView;CyberpunkVR_PreparationPairEligibility[7]=readable;
    const auto pairRequest=CyberpunkVR_PreparationPairRequest.load();
    if(pairRequest==2 || pairRequest==3){
        if(pairRequest==3 && CyberpunkVR_PreparationRootSkipped.load()!=CyberpunkVR_PreparationPairMerged.load()+1)return original(manager,arguments,requested);
        if(!readable || native.count!=1 || native.capacity<1 || first!=0 || !mainRead || name!=0)return original(manager,arguments,requested);
        uint64_t source{},composition[2]{},provider{},vrcamName{};uint32_t mode{},mainSize[2]{},eyeSize[2]{};
        const bool rootValid=Read(reinterpret_cast<uintptr_t>(arguments)+16,&source,8) && source &&
            Read(source+0x168,composition,16) && composition[0] && composition[1] && Read(source+0xF98,&provider,8) && provider &&
            Read(source+0xFA0,&mode,4) && mode<=1 && vrcamView && Read(vrcamView+0x28,&vrcamName,8) && vrcamName==eyeName &&
            Read(mainView+0x54,mainSize,8) && Read(vrcamView+0x54,eyeSize,8) && mainSize[0] && mainSize[1] &&
            mainSize[0]<=16384 && mainSize[1]<=16384 && !std::memcmp(mainSize,eyeSize,8);
        if(!CyberpunkVR_PreparationPairRequest.exchange(0))return original(manager,arguments,requested);
        const uint64_t root[]{source,composition[0],composition[1],provider,mode,mainSize[0],mainSize[1],rootValid};
        std::copy_n(root,8,CyberpunkVR_PreparationPairRoot);std::fill_n(CyberpunkVR_PreparationPairData,8,0ull);
        if(!rootValid || (RunActive() && (mainView!=runViews[0] || vrcamView!=runViews[1]))){
            CyberpunkVR_PreparationPairState=5;if(RunActive())CyberpunkVR_PreparationRunFailure=3;return original(manager,arguments,requested);}
        // Diagnostic only: keep MAIN's output-owning root and select both
        // registered views. The original request vector is never rewritten.
        CyberpunkVR_PreparationPairState=1;uint64_t names[]{0,eyeName};Requests pair{reinterpret_cast<uintptr_t>(names),2,2};
        void* result=original(manager,arguments,&pair);
        uint64_t active{};uint32_t count{};bool valid=Read(reinterpret_cast<uintptr_t>(manager)+0x48,&active,8) &&
            Read(reinterpret_cast<uintptr_t>(manager)+0x54,&count,4) && count==2;
        CyberpunkVR_PreparationPairData[0]=eyeName;CyberpunkVR_PreparationPairData[1]=0;CyberpunkVR_PreparationPairData[2]=count;
        for(uint32_t i=0;valid && i<count;++i){uint64_t view{},key{},storage{};
            valid=Read(active+i*8ull,&view,8) && Read(view+0x28,&key,8) && Read(view+0x2C40,&storage,8) && storage;
            if(key==eyeName){CyberpunkVR_PreparationPairData[3]=view;CyberpunkVR_PreparationPairData[5]=storage;}
            else if(key==0){CyberpunkVR_PreparationPairData[4]=view;CyberpunkVR_PreparationPairData[6]=storage;}
            else valid=false;
        }
        auto* pairData=CyberpunkVR_PreparationPairData;pairData[7]=valid && pairData[3]==vrcamView && pairData[4]==mainView && pairData[5] && pairData[6] && pairData[5]!=pairData[6];
        if(pairData[7]){CyberpunkVR_PreparationPairVerified=1;++CyberpunkVR_PreparationPairGeneration;}
        if(pairData[7] && pairRequest==3)++CyberpunkVR_PreparationPairMerged;
        if(!pairData[7] && RunActive())CyberpunkVR_PreparationRunFailure=3;
        CyberpunkVR_PreparationPairState.store(pairData[7]?2:3,std::memory_order_release);return result;
    }
    // The existing is_main_view contract identifies MAIN by the zero CName.
    if(!readable || native.count!=1 || native.capacity<1 || first!=eyeName || !mainRead || name!=0)
        return original(manager,arguments,requested);
    if(!CyberpunkVR_PreparationPairRequest.exchange(0))return original(manager,arguments,requested);
    std::fill_n(CyberpunkVR_PreparationPairData,8,0ull);CyberpunkVR_PreparationPairData[0]=eyeName;CyberpunkVR_PreparationPairData[1]=name;
    CyberpunkVR_PreparationPairData[4]=mainView;
    // 2026-10-01 PID19072: a two-name request reached DrawComposition +20A3FA
    // with texture handle 0 (name 0x31CF52F9). View selection is not sufficient:
    // the outer RTT request does not supply MAIN's composition output resource.
    // Do not hide that dependency by skipping the node or lending an old target.
    CyberpunkVR_PreparationPairState.store(4,std::memory_order_release);
    return original(manager,arguments,requested);
}
bool SuppressRtt(const void* frameData,uint64_t eyeName,bool gameplay){
    if(!gameplay || !Pairing() || CyberpunkVR_PreparationPairRequest.load()!=3 || !CyberpunkVR_PreparationPairVerified ||
        (RunActive() && (CyberpunkVR_PreparationRunFailure || CyberpunkVR_PreparationRunStop)) ||
        !eyeName)return false;
    const auto source=reinterpret_cast<uintptr_t>(frameData);uint32_t mode{};uint64_t requests{},name{};uint32_t capacity{},count{};
    // Our RTT camera already uses the full gameplay builder (mode 0). The
    // one-name request, not the builder mode, identifies the standalone eye.
    if(!Read(source+0xFA0,&mode,4) || mode!=0 || !Read(source+0x140,&requests,8) || !Read(source+0x148,&capacity,4) ||
        !Read(source+0x14C,&count,4) || count!=1 || capacity<1 || !Read(requests,&name,8) || name!=eyeName)return false;
    bool suppressed=false;
    PairChange([&](bool trace){const auto count=CyberpunkVR_PreparationRootSkipped.load(),budget=PairBudget();
        if(!budget || budget>(RunActive()?1200u:CyberpunkVR_PreparationCameraTrace.load()?7u:3u) || count>=budget || count!=CyberpunkVR_PreparationPairMerged.load() ||
            count!=CyberpunkVR_PreparationPairCompleted.load() || lastSkipFrame==PairFrame())return;
        ++CyberpunkVR_PreparationRootSkipped;lastSkipFrame=PairFrame();suppressed=true;
        if(trace){Record row;row.side=1;row.input=source;row.flags0=mode;Append(row,Kind::RootSuppressed);}});
    return suppressed;
}
void RootCompleted(const void* frameData,uint64_t generationBefore){
    PairChange([&](bool trace){const auto source=reinterpret_cast<uintptr_t>(frameData);
        if(CyberpunkVR_PreparationPairGeneration.load()==generationBefore)return;
        if(CyberpunkVR_PreparationPairState.load()!=2 || source!=CyberpunkVR_PreparationPairRoot[0])return;
        uint64_t composition[2]{};Record row;row.context=source;
        row.flags0=Read(source+0x168,composition,16) && composition[0]==CyberpunkVR_PreparationPairRoot[1] &&
            composition[1] && composition[1]==CyberpunkVR_PreparationPairRoot[2];
        row.input=composition[0];row.output=composition[1];if(trace)Append(row,Kind::RootCompleted);
        if(!row.flags0){CyberpunkVR_PreparationPairVerified=0;CyberpunkVR_PreparationPairState=6;CyberpunkVR_PreparationPairRequest=0;
            if(RunActive())CyberpunkVR_PreparationRunFailure=3;return;}
        const auto merged=CyberpunkVR_PreparationPairMerged.load();
        if(merged==CyberpunkVR_PreparationPairCompleted.load()+1 && merged==CyberpunkVR_PreparationRootSkipped.load()){
            CyberpunkVR_PreparationPairCompleted=merged;
            if(merged<PairBudget() && !CyberpunkVR_PreparationRunStop)CyberpunkVR_PreparationPairRequest=3;
        }
    });
}
void EyeCopyRecorded(const void* list,const void* source,const void* destination){
    if(RunActive())PairChange([&](bool){
        if(copySource.side!=1 || copySource.view!=runViews[1] || !list || runCopies.size()>=64 || runCopies.contains(list)){
            CyberpunkVR_PreparationRunFailure=6;return;}
        runCopies.emplace(list,false);++CyberpunkVR_PreparationRunEyeCopies;
    });
    Change([&]{Record row;row.side=copySource.side;row.view=copySource.view;row.flags0=copySource.node;
        row.context=reinterpret_cast<uintptr_t>(list);row.input=reinterpret_cast<uintptr_t>(source);
        row.output=reinterpret_cast<uintptr_t>(destination);Append(row,Kind::EyeCopyRecorded);});
}
CopySource SetCopySource(CopySource value){const auto previous=copySource;copySource=value;return previous;}
DrawScope::DrawScope(uint32_t node,int side,const void* arguments):previous(drawScope){
    drawScope=this;
    if(node!=0x33DDA4 || side<0 || side>1)return;
    Change([&]{source.side=side;if(Arguments(source,arguments) && (source.stage==3 || CyberpunkVR_PreparationAllStages.load())){
        captureEpoch=epoch;source.scope=++nextScope;source.frame=frame;valid=true;
        if(CyberpunkVR_PreparationAllStages.load() && graphParents.size()<RecordCapacity){
            GraphParentRecord parent;parent.scope=source.scope;parent.context=source.context;parent.stage=source.stage;
            DescribeGraphParent(source.context,side,source.frame,captureEpoch,parent);graphParents.push_back(parent);
        }}});
}
DrawTag CurrentDrawTag(){
    const auto* value=drawScope;
    if(!value || !value->valid || !Active())return {};
    std::lock_guard lock(mutex);
    return value->captureEpoch==epoch && Active()?DrawTag{value->source.scope,value->source.stage,value->source.frame,value->source.side}:DrawTag{};
}
GraphContextScope::GraphContextScope(uint64_t context):previous_(graphContext){graphContext=context;}
GraphContextScope::~GraphContextScope(){graphContext=previous_;}
uint8_t InvokeGraphContext(GraphWork work,uintptr_t* node,uint8_t* context,void* arguments){
    GraphContextScope scope(reinterpret_cast<uintptr_t>(context));
    scene_child::Scope child(reinterpret_cast<uintptr_t>(node),reinterpret_cast<uintptr_t>(context));
    return work(node,context,arguments);
}
bool CurrentGraphParent(int side,uint32_t expectedFrame,GraphParentRecord& row){
    row={};uint64_t context=graphContext,capturedEpoch{},scope{};UINT stage{};
    {
        std::lock_guard lock(mutex);if(!Active() || frame!=expectedFrame)return false;capturedEpoch=epoch;
        if(drawScope && drawScope->valid && drawScope->captureEpoch==epoch){
            context=drawScope->source.context;scope=drawScope->source.scope;stage=drawScope->source.stage;
        }
    }
    row.context=context;row.scope=scope;row.stage=stage;
    return DescribeGraphParent(context,side,expectedFrame,capturedEpoch,row);
}
bool RevalidateGraphParent(const GraphParentRecord& expected){
    if(expected.status || expected.viewIndex>1 || !expected.context || !expected.manager || !expected.fingerprint)return false;
    UINT expectedFrame{};uint64_t expectedEpoch{};
    {std::lock_guard lock(mutex);if(!Active())return false;expectedFrame=frame;expectedEpoch=epoch;}
    GraphParentRecord freshParent;
    if(!CurrentGraphParent(int(expected.viewIndex),expectedFrame,freshParent) || std::memcmp(&freshParent,&expected,sizeof(freshParent)))return false;
    std::lock_guard lock(mutex);return Active() && frame==expectedFrame && epoch==expectedEpoch;
}
DrawScope::~DrawScope(){
    if(valid)Change([&]{if(captureEpoch!=epoch)return;
        for(UINT i=0;i<count;++i){auto row=source;row.output=reinterpret_cast<uintptr_t>(lists[i]);row.flags0=draws[i];Append(row,Kind::DrawEnd);}});
    drawScope=previous;
}
void DrawRecorded(const void* list){
    auto* scope=drawScope;if(!Active() || !scope || !scope->valid || !list)return;
    for(UINT i=0;i<scope->count;++i)if(scope->lists[i]==list){++scope->draws[i];return;}
    Change([&]{if(scope->captureEpoch!=epoch)return;
        if(scope->count==scope->lists.size()){CyberpunkVR_PreparationFailure=5;return;}
        scope->lists[scope->count]=list;scope->draws[scope->count++]=1;
        auto row=scope->source;row.output=reinterpret_cast<uintptr_t>(list);Append(row,Kind::DrawBegin);});
}
void SubmitLists(const void* queue,uint32_t count,ID3D12CommandList* const* lists,bool serialized){
    if(!queue || !lists || !count)return;
    if(RunActive())PairChange([&](bool){for(UINT i=0;i<count;++i){const auto found=runCopies.find(lists[i]);if(found==runCopies.end())continue;
        if(found->second){CyberpunkVR_PreparationRunFailure=7;continue;}found->second=true;++CyberpunkVR_PreparationRunEyeSubmissions;}});
    Change([&]{if(count>256){CyberpunkVR_PreparationFailure=5;return;}const auto batch=++nextSubmission;
        for(UINT i=0;i<count;++i){Record row;row.scope=batch;row.context=reinterpret_cast<uintptr_t>(lists[i]);
            row.input=reinterpret_cast<uintptr_t>(queue);row.flags0=i;row.flags1=count;row.reserved=serialized;Append(row,Kind::SubmitList);}});
}
void ResetList(const void* list){
    if(RunActive())PairChange([&](bool){const auto found=runCopies.find(list);if(found!=runCopies.end()){
        if(!found->second)CyberpunkVR_PreparationRunFailure=7;runCopies.erase(found);}});
    Change([&]{Record row;row.context=reinterpret_cast<uintptr_t>(list);Append(row,Kind::ResetList);});
}
bool PairPrepareCompatible() {
    if(!CyberpunkVR_PreparationPairRequest.load(std::memory_order_relaxed))return false;
    static const bool compatible=[]{
        const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
#if CVR_RUST_VRIK
        return cvr_binding_code(0,base);
#else
        struct Region {uint32_t rva,bytes;uint64_t hash;};
        // 2.5: GraphContextPrepare (sub_140423180) absorbed its 2.31 cold chunk (0x1EF9E44) and was rewritten
        // (921 -> 1901 bytes); the pairing contract is not re-audited, so its row never matches.
        const Region regions[]{{0x4231A0,0x74D,0x0000000000000000ull},{0x4230A0,0xDF,0x8660E0F8609D6EE7ull},
            {0x516264,0x20,0x0AC550E5218DAFA1ull}};
        std::array<uint8_t,1024> bytes{};
        for(const auto& region:regions){if(!Read(base+region.rva,bytes.data(),region.bytes))return false;
            uint64_t hash=14695981039346656037ull;for(UINT i=0;i<region.bytes;++i)hash=(hash^bytes[i])*1099511628211ull;
            if(hash!=region.hash)return false;}
        return true;
#endif
    }();
    CyberpunkVR_PreparationPairCompatible.store(compatible?1:2,std::memory_order_release);return compatible;
}
}
#endif
