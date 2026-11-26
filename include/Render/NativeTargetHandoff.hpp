#pragma once
#include "Render/NativeCommandRecording.hpp"
#include "Render/NativeTextureRead.hpp"
#include "Render/StereoSceneState.hpp"
#include <array>
#include <atomic>
#include <cstdint>

namespace cvr::stereo::target_handoff {
inline constexpr UINT SourceLimit=2,TargetsPerSource=4,StatesPerSource=5;
inline constexpr UINT TargetLimit=SourceLimit*TargetsPerSource,StateLimit=SourceLimit*StatesPerSource;
enum class ExitKind:uint32_t {Unknown,Close,Transition,Aliasing,Uav,Copy,Discard,Clear,Resolve,Other};
enum class Failure:uint32_t {
    None,Arguments,Shape,Identity,Recording,Creation,CaptureRace,DuplicateBegin,StateBefore,
    Subresource,SplitBarrier,UnknownBarrier,Aliasing,PostExitWrite,OtherListTouch,AfterClose,
    MissingExit,CloseFailed,ResetBeforeSubmit,Submission,Replay,UnknownCommand,Generation
};
enum SourceFlags:uint64_t {
    Captured=1,Exited=2,CloseSucceeded=4,PendingValidated=8,SubmittedObserved=16,
    Refused=32,ResetBeforeSubmit=64,PrefixUnknown=128
};
enum StateFlags:uint64_t {CurrentKnown=1,FrozenKnown=2,StateRefused=4};
namespace detail {
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/target_handoff.rs (tools/single_pass_tests/target_handoff_test runs its cases
// against them in its Rust variant). The members stay here, mirrored there; the methods run in Rust on them.
struct ObservationWindow;struct TargetState;struct RecordingState;
extern "C" {
bool cvr_th_state_known(uint32_t);
bool cvr_th_window(ObservationWindow*,uint32_t);
bool cvr_th_target(TargetState*,uint32_t,uint32_t,uint32_t,uint32_t,uint32_t);
uint64_t cvr_th_recording(RecordingState*,uint32_t,uint64_t,uint64_t,uint32_t,uint32_t,uint32_t,const TargetState*,HRESULT);
}
struct ObservationWindow {
    uint64_t events{};bool open{},finished{};
    bool Begin(){return cvr_th_window(this,0);}
    bool Event(){return cvr_th_window(this,1);}
    void Finish(){cvr_th_window(this,2);}
    void Clear(){cvr_th_window(this,3);}
};
// Only legacy texture states supported by this observer. The ledger never
// infers implicit promotion, decay, enhanced barriers or split-barrier pairing.
inline bool StateKnown(uint32_t value){return cvr_th_state_known(value);}
struct SubresourceState {
    uint32_t seed{},current{},frozen{},transitions{};
    uint64_t flags{};
};
struct TargetState {
    std::array<SubresourceState,StatesPerSource> states{};
    UINT count{};Failure failure{};
    void Reject(Failure why){cvr_th_target(this,0,uint32_t(why),0,0,0);}
    bool Seed(UINT subresources,uint32_t state){return cvr_th_target(this,1,subresources,state,0,0);}
    bool Transition(UINT subresource,uint32_t before,uint32_t after,D3D12_RESOURCE_BARRIER_FLAGS flags){
        return cvr_th_target(this,2,subresource,before,after,uint32_t(flags));
    }
    void Alias(bool affects){cvr_th_target(this,3,affects,0,0,0);}
    void Freeze(){cvr_th_target(this,4,0,0,0,0);}
};
struct RecordingState {
    uint64_t list{},generation{},queue{},flags{PrefixUnknown};
    Failure failure{};uint32_t exitReason{},exitKind{};HRESULT closeResult{E_PENDING};
    Failure foreignFailure{};uint64_t foreignList{},foreignKind{},foreignEvents{};
    bool pendingObserved{};
    void Reject(Failure why){Call(0,0,0,uint32_t(why));}
    void Foreign(uint64_t commandList,uint32_t kind){Call(1,commandList,0,kind);}
    uint64_t AggregateFlags()const{return Const(2);}
    Failure AggregateFailure()const{return Failure(Const(3));}
    bool Begin(uint64_t commandList,uint64_t recordingGeneration){return Call(4,commandList,recordingGeneration)!=0;}
    bool Match(uint64_t commandList,uint64_t recordingGeneration){return Call(5,commandList,recordingGeneration)!=0;}
    bool Tracking()const{return Const(6)!=0;}
    void Exit(uint32_t reason,uint32_t kind){Call(7,0,0,reason,kind);}
    void Touch(uint64_t commandList,bool write,bool alias=false,uint32_t kind=0){Call(8,commandList,0,write,alias,kind);}
    void ShaderDraw(uint64_t commandList,const TargetState& target,bool retiredRecording=false){
        Call(9,commandList,0,retiredRecording,0,0,&target);
    }
    bool Close(HRESULT result){return Call(10,0,0,0,0,0,nullptr,result)!=0;}
    bool Pending(uint64_t actualQueue,UINT occurrences,bool exactLease){return Call(11,actualQueue,0,occurrences,exactLease)!=0;}
    void Submitted(uint64_t actualQueue){Call(12,actualQueue);}
    void Reset(){Call(13);}
private:
    uint64_t Call(uint32_t which,uint64_t a=0,uint64_t b=0,uint32_t c=0,uint32_t d=0,uint32_t e=0,
        const TargetState* target=nullptr,HRESULT result=S_OK){
        return cvr_th_recording(this,which,a,b,c,d,e,target,result);
    }
    uint64_t Const(uint32_t which)const{return cvr_th_recording(const_cast<RecordingState*>(this),which,0,0,0,0,0,nullptr,S_OK);}
};
static_assert(sizeof(ObservationWindow)==16 && sizeof(SubresourceState)==24 && sizeof(TargetState)==128 &&
    sizeof(RecordingState)==88 && offsetof(RecordingState,foreignList)==56,"mirrored by target_handoff.rs");
#else
struct ObservationWindow {
    uint64_t events{};bool open{},finished{};
    bool Begin(){if(finished)return false;open=true;return true;}
    bool Event(){if(!open)return false;if(events!=UINT64_MAX)++events;return true;}
    void Finish(){open=false;finished=true;}
    void Clear(){*this={};}
};
// Only legacy texture states supported by this observer. The ledger never
// infers implicit promotion, decay, enhanced barriers or split-barrier pairing.
inline bool StateKnown(uint32_t value){
    constexpr uint32_t reads=D3D12_RESOURCE_STATE_DEPTH_READ|D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE|
        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE|D3D12_RESOURCE_STATE_COPY_SOURCE|
        D3D12_RESOURCE_STATE_RESOLVE_SOURCE|D3D12_RESOURCE_STATE_SHADING_RATE_SOURCE;
    constexpr uint32_t writes=D3D12_RESOURCE_STATE_RENDER_TARGET|D3D12_RESOURCE_STATE_UNORDERED_ACCESS|
        D3D12_RESOURCE_STATE_DEPTH_WRITE|D3D12_RESOURCE_STATE_COPY_DEST|D3D12_RESOURCE_STATE_RESOLVE_DEST;
    if(value&~(reads|writes))return false;
    const auto write=value&writes;
    return !write || (!(write&(write-1)) && value==write);
}
struct SubresourceState {
    uint32_t seed{},current{},frozen{},transitions{};
    uint64_t flags{};
};
struct TargetState {
    std::array<SubresourceState,StatesPerSource> states{};
    UINT count{};Failure failure{};
    void Reject(Failure why){if(failure==Failure::None)failure=why;for(UINT i=0;i<count;++i)states[i].flags|=StateRefused;}
    bool Seed(UINT subresources,uint32_t state){
        *this={};
        if(!subresources || subresources>states.size() ||
            (state!=D3D12_RESOURCE_STATE_RENDER_TARGET && state!=D3D12_RESOURCE_STATE_DEPTH_WRITE)){
            Reject(Failure::Shape);return false;
        }
        count=subresources;for(UINT i=0;i<count;++i)states[i]={state,state,0,0,CurrentKnown};return true;
    }
    bool Transition(UINT subresource,uint32_t before,uint32_t after,D3D12_RESOURCE_BARRIER_FLAGS flags){
        if(failure!=Failure::None)return false;
        if(!count){Reject(Failure::Subresource);return false;}
        if(flags!=D3D12_RESOURCE_BARRIER_FLAG_NONE){Reject(Failure::SplitBarrier);return false;}
        if(!StateKnown(before) || !StateKnown(after)){Reject(Failure::UnknownBarrier);return false;}
        if(subresource!=D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES && subresource>=count){Reject(Failure::Subresource);return false;}
        const UINT first=subresource==D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES?0:subresource;
        const UINT end=subresource==D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES?count:subresource+1;
        for(UINT i=first;i<end;++i)if(!(states[i].flags&CurrentKnown) || states[i].current!=before){Reject(Failure::StateBefore);return false;}
        for(UINT i=first;i<end;++i){states[i].current=after;if(states[i].transitions!=UINT_MAX)++states[i].transitions;}
        return true;
    }
    void Alias(bool affects){if(affects)Reject(Failure::Aliasing);}
    void Freeze(){if(failure==Failure::None)for(UINT i=0;i<count;++i){states[i].frozen=states[i].current;states[i].flags|=FrozenKnown;}}
};
struct RecordingState {
    uint64_t list{},generation{},queue{},flags{PrefixUnknown};
    Failure failure{};uint32_t exitReason{},exitKind{};HRESULT closeResult{E_PENDING};
    Failure foreignFailure{};uint64_t foreignList{},foreignKind{},foreignEvents{};
    bool pendingObserved{};
    void Reject(Failure why){flags|=Refused;if(failure==Failure::None)failure=why;}
    void Foreign(uint64_t commandList,uint32_t kind){
        if(!(flags&Captured))return;
        if(!foreignEvents){foreignFailure=Failure::OtherListTouch;foreignList=commandList;foreignKind=kind;}
        if(foreignEvents!=UINT64_MAX)++foreignEvents;
    }
    uint64_t AggregateFlags()const{return foreignEvents?(flags|Refused)&~uint64_t(PendingValidated):flags;}
    Failure AggregateFailure()const{return failure!=Failure::None?failure:foreignFailure;}
    bool Begin(uint64_t commandList,uint64_t recordingGeneration){
        *this={};list=commandList;generation=recordingGeneration;
        if(!list || !generation){Reject(Failure::Arguments);return false;}
        flags|=Captured;return true;
    }
    bool Match(uint64_t commandList,uint64_t recordingGeneration){
        if(commandList!=list || recordingGeneration!=generation){Reject(Failure::Generation);return false;}return true;
    }
    bool Tracking()const{return (flags&Captured) && !(flags&(SubmittedObserved|ResetBeforeSubmit));}
    void Exit(uint32_t reason,uint32_t kind){
        if(!Tracking() || (flags&Exited))return;
        if(flags&CloseSucceeded){Reject(Failure::AfterClose);return;}
        flags|=Exited;exitReason=reason;exitKind=kind;
    }
    void Touch(uint64_t commandList,bool write,bool alias=false,uint32_t kind=0){
        if(!(flags&Captured))return;
        if(commandList!=list){Foreign(commandList,kind);return;}
        if(!Tracking())return;
        if(flags&CloseSucceeded){Reject(Failure::AfterClose);return;}
        if(alias){Reject(Failure::Aliasing);return;}
        if(write && (flags&Exited))Reject(Failure::PostExitWrite);
    }
    void ShaderDraw(uint64_t commandList,const TargetState& target,bool retiredRecording=false){
        if(!(flags&Captured))return;
        const bool foreign=retiredRecording || commandList!=list;
        if(!foreign && !Tracking())return;
        // OM bindings do not describe UAV writes by graphics shaders.
        for(UINT i=0;i<target.count;++i){const auto& state=target.states[i];
            if(((state.flags&CurrentKnown) && state.current==D3D12_RESOURCE_STATE_UNORDERED_ACCESS) ||
                ((state.flags&FrozenKnown) && state.frozen==D3D12_RESOURCE_STATE_UNORDERED_ACCESS)){
                if(foreign)Foreign(commandList,uint32_t(ExitKind::Other));else Reject(Failure::UnknownCommand);
                return;
            }
        }
    }
    bool Close(HRESULT result){
        if(!Tracking())return false;
        closeResult=result;
        if(FAILED(result)){Reject(Failure::CloseFailed);return false;}
        if(flags&CloseSucceeded){Reject(Failure::Replay);return false;}
        flags|=CloseSucceeded;
        if(!(flags&Exited))Reject(Failure::MissingExit);
        return !(flags&Refused);
    }
    bool Pending(uint64_t actualQueue,UINT occurrences,bool exactLease){
        if(pendingObserved || (flags&SubmittedObserved)){Reject(Failure::Replay);return false;}
        pendingObserved=true;queue=actualQueue;
        if(!actualQueue || occurrences!=1 || !exactLease || !(flags&CloseSucceeded) || !(flags&Exited))Reject(Failure::Submission);
        if(flags&Refused)return false;
        flags|=PendingValidated;return true;
    }
    void Submitted(uint64_t actualQueue){
        if(actualQueue && actualQueue==queue && pendingObserved && !(flags&ResetBeforeSubmit))flags|=SubmittedObserved;
    }
    void Reset(){if(Tracking()){flags|=ResetBeforeSubmit;Reject(Failure::ResetBeforeSubmit);}}
};
#endif
}
// Actual first stage9 draw only, before proof/checkpoint commands. The targets
// must be exactly three colors and a writable depth/stencil view. Capturing and
// validating hold identities and observe state; they grant no export permission.
bool Begin(ID3D12GraphicsCommandList*,const native_recording::Lease&,uint32_t frame,uint32_t side,const scene_state::GeometryTargets&);
bool Observing()noexcept;
void MarkExit(ID3D12GraphicsCommandList*,uint32_t reason,ExitKind kind=ExitKind::Unknown);
// Call after retained proof commands and before the incoming native barrier.
// Internal commands must call this too, including checkpoint restore barriers.
void Barriers(ID3D12GraphicsCommandList*,UINT,const D3D12_RESOURCE_BARRIER*);
void Written(ID3D12GraphicsCommandList*,ID3D12Resource*,uint32_t kind);
void ClearView(ID3D12GraphicsCommandList*,D3D12_CPU_DESCRIPTOR_HANDLE,bool depth);
void DrawTargets(ID3D12GraphicsCommandList*,const scene_state::GeometryTargets&);
// Independent OM tracking includes internal commands and generic array views.
// A draw also refuses watched UAV states without shader-UAV binding proof.
void Targets(ID3D12GraphicsCommandList*,UINT,const D3D12_CPU_DESCRIPTOR_HANDLE*,BOOL,const D3D12_CPU_DESCRIPTOR_HANDLE*);
void Draw(ID3D12GraphicsCommandList*);
void Unknown(ID3D12GraphicsCommandList*,uint32_t kind);
void Close(ID3D12GraphicsCommandList*,HRESULT);
void Reset(ID3D12GraphicsCommandList*);
// Bracket the real ExecuteCommandLists, after native_recording::BeforeSubmit.
void BeforeSubmit(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
void Submitted(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
struct TargetSnapshot {
    texture_read::WritableTargetIdentity identity;
    scene_state::TargetInfo view;
    UINT slot{},planes{};
    std::array<D3D12_RESOURCE_STATES,2> finalStates{};
};
struct SourceSnapshot {
    native_recording::Lease recording;
    uint32_t frame{},side{};
    std::array<TargetSnapshot,4> targets;
};
// Read local frozen states while the exact native source submissions are pending.
// Foreign uncertainty is not cleared: these are inputs to an independent access
// window check, never an allocation/residency/export permission by themselves.
bool SnapshotSources(ID3D12CommandQueue*,uint32_t frame,std::array<SourceSnapshot,2>&);
void FinishObservation();
void Clear();
}
extern "C" {
// Read an even sequence, then all arrays, then the same even sequence again.
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_TargetHandoffSeq;
// Schema 2. All rows are uint64_t, including HRESULT as its unsigned 32-bit bits.
// Stats: schema, sources, targets, states, begin, beginRejected, barriers,
// transitions, aliases, writes, exits, closeOk, closeFailed, resets,
// pendingValidated, submitRejected.
__declspec(dllexport) extern uint64_t CyberpunkVR_TargetHandoffStats[16];
// Two rows: frame, side, list, generation, context, registry, targetFirst,
// targetCount, stateFirst, stateCount, flags, exitReason, exitKind, closeHRESULT,
// queue, failure. PrefixUnknown remains set: earlier/foreign recordings are not
// certified noninterfering. SubmittedObserved never means GPU completion.
// Foreign uncertainty adds Refused and removes PendingValidated from these
// aggregate flags. The aggregate failure prefers a local failure when present.
__declspec(dllexport) extern uint64_t CyberpunkVR_TargetHandoffSources[32];
// Two local rows: flags, failure, closeHRESULT, foreignFailure, foreignList,
// foreignKind, foreignEvents, submittedObserved. Foreign observations never
// erase the local per-subresource state or successful Close evidence.
__declspec(dllexport) extern uint64_t CyberpunkVR_TargetHandoffSourceLocal[16];
// Eight owner observations: nativeRecord DWORD at+8, numeric owner at+0x10.
// Tagged native ownership is not established by these numeric observations.
__declspec(dllexport) extern uint64_t CyberpunkVR_TargetHandoffOwners[16];
// Eight rows: source, slot, resource, viewIdentity, nativeRegistry, nativeHandle,
// creationKind, heap, heapOffset, allocationBytes, width, height, format,
// viewFormat, resourceFlags, viewFlags, mips, layers, planes, stateFirst,
// stateCount, flags, failure, seedState.
__declspec(dllexport) extern uint64_t CyberpunkVR_TargetHandoffTargets[192];
// Ten rows: source, target, subresource, seed, current, frozen, flags, transitions.
__declspec(dllexport) extern uint64_t CyberpunkVR_TargetHandoffStates[80];
}
