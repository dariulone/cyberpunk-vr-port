#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Render/NativeUploadReservation.hpp"
#include <Windows.h>
#include <cstring>
#include <intrin.h>
#include <mutex>

#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/upload_reservation_runtime.rs (the exports, the thunk's XSAVE choice
// and callback, the installation and the ledger).
#else
extern "C" {
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_UploadReservationEnable{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_UploadReservationCount{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_UploadReservationDropped{0};
#endif
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_UploadReservationState{uint32_t(cvr::stereo::upload_reservation::Status::Disabled)};
__declspec(dllexport) std::array<cvr::stereo::upload_reservation::Row,cvr::stereo::upload_reservation::MaxRows> CyberpunkVR_UploadReservationRows{};
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_UploadReservationPrepareRequest{},CyberpunkVR_UploadReservationAcceptRequest{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_UploadReservationExternalPlanSeq{},CyberpunkVR_UploadReservationExternalReceiptSeq{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_UploadReservationExternalInstallState{};
#endif
__declspec(dllexport) cvr::stereo::upload_reservation::ExternalInstallPlan CyberpunkVR_UploadReservationExternalPlan{};
__declspec(dllexport) cvr::stereo::upload_reservation::ExternalInstallReceipt CyberpunkVR_UploadReservationExternalReceipt{};
uint64_t CvrNativeUploadReservationXstateMask{};
uint32_t CvrNativeUploadReservationXstateBytes{512};
uint32_t CvrNativeUploadReservationXstateMode{};
void CvrNativeUploadReservationThunk();
}
namespace cvr::stereo::upload_reservation {
namespace {
std::mutex codeMutex,ledgerMutex;
Ledger<> ledger;std::atomic<bool> lost{false};uint64_t epoch{1};
uintptr_t guardedBase{};bool guardAttempted{},guardValid{},installed{},installationHealthy{};
uint8_t* relay{};
DWORD siteProtection{};
constexpr auto original=OriginalWitnessBytes;
constexpr int32_t retryDisplacement=int32_t(RetryRva)-int32_t(WitnessRva+7);
static_assert(retryDisplacement==-0x1C && uint8_t(retryDisplacement)==RetryDisplacement);
std::array<uint8_t,7> patch{};
ExternalInstallPlan preparedExternalPlan;
uint8_t* externalRelay{};HMODULE externalModule{};
uint64_t externalNonce{};uint32_t externalPlanSequence{};
bool externalPrepared{},externalAccepted{};
uintptr_t Base()noexcept{return reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));}
void State(Status value)noexcept{CyberpunkVR_UploadReservationState.store(uint32_t(value),std::memory_order_relaxed);}
void ExternalState(ExternalInstallState value)noexcept{CyberpunkVR_UploadReservationExternalInstallState.store(uint32_t(value),std::memory_order_release);}
bool Read(uintptr_t address,void* output,size_t bytes)noexcept {
    SIZE_T copied{};return address && bytes && address<=UINTPTR_MAX-bytes &&
        ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<void*>(address),output,bytes,&copied) && copied==bytes;
}
bool WholeBody(uintptr_t base)noexcept {
    std::array<uint8_t,AllocatorBytes> bytes{};
    return base && base==Base() && base<=UINTPTR_MAX-AllocatorRva && Read(base+AllocatorRva,bytes.data(),bytes.size()) &&
        Fnv(bytes.data(),bytes.size())==AllocatorFnv;
}
bool ConfigureXstate()noexcept {
    int cpu[4]{};__cpuid(cpu,0);const auto maximum=cpu[0];__cpuid(cpu,1);
    CvrNativeUploadReservationXstateMask=0;CvrNativeUploadReservationXstateBytes=512;CvrNativeUploadReservationXstateMode=0;
    if(!(uint32_t(cpu[2])&(1u<<27)))return true;
    if(!(uint32_t(cpu[2])&(1u<<26)) || maximum<0xD)return false;
    const auto enabled=_xgetbv(0);__cpuidex(cpu,0xD,0);
    const auto supported=uint64_t(uint32_t(cpu[0]))|(uint64_t(uint32_t(cpu[3]))<<32);
    const auto bytes=uint32_t(cpu[1]);
    if(!ValidXstate(enabled,supported,bytes))return false;
    CvrNativeUploadReservationXstateMask=enabled;CvrNativeUploadReservationXstateBytes=bytes;CvrNativeUploadReservationXstateMode=1;
    return true;
}
uint8_t* AllocateRelay(uintptr_t site)noexcept {
    SYSTEM_INFO info{};GetSystemInfo(&info);const uintptr_t step=info.dwAllocationGranularity;
    if(!step)return nullptr;
    const uintptr_t aligned=site&~(step-1),limit=0x7FFF0000;
    for(uintptr_t distance=step;distance<=limit;distance+=step){
        const uintptr_t candidates[]{aligned>=distance?aligned-distance:0,aligned<=UINTPTR_MAX-distance?aligned+distance:0};
        for(const auto candidate:candidates){
            if(!candidate || candidate>UINTPTR_MAX-step)continue;
            MEMORY_BASIC_INFORMATION region{};
            if(!VirtualQuery(reinterpret_cast<void*>(candidate),&region,sizeof(region)) || region.State!=MEM_FREE || region.RegionSize<step)continue;
            auto* result=static_cast<uint8_t*>(VirtualAlloc(reinterpret_cast<void*>(candidate),step,MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE));
            if(result)return result;
        }
    }
    return nullptr;
}
bool Replace(uintptr_t site,const std::array<uint8_t,7>& expected,const std::array<uint8_t,7>& replacement,bool& written,DWORD& previousProtection)noexcept {
    written=false;
    std::array<uint8_t,7> current{};if(!Read(site,current.data(),current.size()) || current!=expected)return false;
    DWORD protection{};if(!VirtualProtect(reinterpret_cast<void*>(site),replacement.size(),PAGE_EXECUTE_READWRITE,&protection))return false;
    previousProtection=protection;
    std::memcpy(reinterpret_cast<void*>(site),replacement.data(),replacement.size());
    written=true;
    const bool flushed=FlushInstructionCache(GetCurrentProcess(),reinterpret_cast<void*>(site),replacement.size())!=FALSE;
    DWORD ignored{};const bool restored=VirtualProtect(reinterpret_cast<void*>(site),replacement.size(),protection,&ignored)!=FALSE;
    return flushed && restored;
}
bool ProcessIdentity(uint64_t& creation)noexcept {
    FILETIME created{},exited{},kernel{},user{};creation=0;
    if(!GetProcessTimes(GetCurrentProcess(),&created,&exited,&kernel,&user))return false;
    creation=uint64_t(created.dwLowDateTime)|(uint64_t(created.dwHighDateTime)<<32);return creation!=0;
}
bool ThunkExtent(HMODULE module,uint64_t& bytes)noexcept {
    bytes=0;const auto address=reinterpret_cast<uintptr_t>(&CvrNativeUploadReservationThunk);DWORD64 image{};
    const auto function=RtlLookupFunctionEntry(address,&image,nullptr);
    if(!function || image!=reinterpret_cast<uintptr_t>(module) || image>UINT64_MAX-function->EndAddress ||
        image+function->BeginAddress!=address || function->EndAddress<=function->BeginAddress ||
        function->EndAddress-function->BeginAddress>4096)return false;
    bytes=function->EndAddress-function->BeginAddress;return true;
}
bool CurrentExternalCode(const ExternalInstallPlan& plan)noexcept {
    std::array<uint8_t,AllocatorBytes> body{};std::array<uint8_t,14> currentRelay{};
    MEMORY_BASIC_INFORMATION site{},storage{},thunk{};uint64_t creation{},thunkBytes{};
    return ValidExternalPlan(plan) && plan.processId==GetCurrentProcessId() && ProcessIdentity(creation) && creation==plan.processCreationTime &&
        Base()==plan.gameBase && reinterpret_cast<uintptr_t>(externalModule)==plan.modulePinAddress && ThunkExtent(externalModule,thunkBytes) &&
        thunkBytes==plan.thunkBytes && Read(plan.allocatorAddress,body.data(),body.size()) && MatchesExternalPatchedBody(plan,body) &&
        Read(plan.relayAddress,currentRelay.data(),currentRelay.size()) && currentRelay==plan.relayCode &&
        VirtualQuery(reinterpret_cast<void*>(plan.siteAddress),&site,sizeof(site))==sizeof(site) && site.State==MEM_COMMIT && site.Type==MEM_IMAGE &&
        reinterpret_cast<uintptr_t>(site.AllocationBase)==plan.gameBase && site.Protect==plan.originalProtection &&
        VirtualQuery(reinterpret_cast<void*>(plan.relayAddress),&storage,sizeof(storage))==sizeof(storage) && storage.State==MEM_COMMIT &&
        storage.Type==MEM_PRIVATE && reinterpret_cast<uintptr_t>(storage.AllocationBase)==plan.relayAddress && storage.Protect==PAGE_EXECUTE_READ &&
        VirtualQuery(reinterpret_cast<void*>(plan.thunkAddress),&thunk,sizeof(thunk))==sizeof(thunk) && thunk.State==MEM_COMMIT && thunk.Type==MEM_IMAGE &&
        reinterpret_cast<uintptr_t>(thunk.AllocationBase)==plan.modulePinAddress &&
        (thunk.Protect==PAGE_EXECUTE_READ || thunk.Protect==PAGE_EXECUTE_READWRITE || thunk.Protect==PAGE_EXECUTE_WRITECOPY);
}
bool PrepareExternal(uintptr_t base)noexcept {
    if(externalPrepared){ExternalState(externalAccepted?ExternalInstallState::Accepted:ExternalInstallState::Prepared);return true;}
    if(installed){ExternalState(ExternalInstallState::LegacyOwned);return false;}
    // Prepared storage is never replaced: an external commit may already have
    // made the relay reachable even before its receipt is accepted here.
    if(externalRelay)return false;
    if(!guardAttempted){guardAttempted=true;guardedBase=base;guardValid=WholeBody(base);}
    if(!guardValid || guardedBase!=base || !WholeBody(base) || !ConfigureXstate() || base>UINTPTR_MAX-AllocatorRva-AllocatorBytes ||
        externalNonce==UINT64_MAX)return false;
    ExternalInstallPlan next{};next.version=ExternalPlanVersion;next.nonce=externalNonce+1;next.processId=GetCurrentProcessId();
    next.gameBase=base;next.allocatorAddress=base+AllocatorRva;next.siteAddress=base+WitnessRva;
    next.thunkAddress=reinterpret_cast<uintptr_t>(&CvrNativeUploadReservationThunk);next.guardFnv=AllocatorFnv;
    next.allocatorBytes=AllocatorBytes;next.siteBytes=7;next.relayBytes=14;next.original=original;
    if(!ProcessIdentity(next.processCreationTime) || !Read(next.allocatorAddress,next.guard.data(),next.guard.size()) ||
        Fnv(next.guard.data(),next.guard.size())!=AllocatorFnv)return false;
    MEMORY_BASIC_INFORMATION site{};
    if(VirtualQuery(reinterpret_cast<void*>(next.siteAddress),&site,sizeof(site))!=sizeof(site) || site.State!=MEM_COMMIT || site.Type!=MEM_IMAGE ||
        reinterpret_cast<uintptr_t>(site.AllocationBase)!=base)return false;
    next.originalProtection=site.Protect;
    if(!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_PIN,
        reinterpret_cast<LPCWSTR>(next.thunkAddress),&externalModule) || !ThunkExtent(externalModule,next.thunkBytes))return false;
    next.modulePinAddress=reinterpret_cast<uintptr_t>(externalModule);
    externalRelay=AllocateRelay(next.siteAddress);if(!externalRelay)return false;
    next.relayAddress=reinterpret_cast<uintptr_t>(externalRelay);
    const uint8_t jump[]{0xFF,0x25,0,0,0,0};std::memcpy(next.relayCode.data(),jump,sizeof(jump));
    std::memcpy(next.relayCode.data()+6,&next.thunkAddress,sizeof(next.thunkAddress));
    std::memcpy(externalRelay,next.relayCode.data(),next.relayCode.size());
    const auto displacement=int64_t(next.relayAddress)-int64_t(next.siteAddress+5);DWORD previous{};
    if(displacement<INT32_MIN || displacement>INT32_MAX || !VirtualProtect(externalRelay,next.relayCode.size(),PAGE_EXECUTE_READ,&previous) ||
        !FlushInstructionCache(GetCurrentProcess(),externalRelay,next.relayCode.size()))return false;
    next.patch={0xE8,0,0,0,0,0x75,uint8_t(retryDisplacement)};
    const auto relative=int32_t(displacement);std::memcpy(next.patch.data()+1,&relative,sizeof(relative));
    next.checksum=ExternalPlanChecksum(next);if(!ValidExternalPlan(next))return false;
    preparedExternalPlan=next;externalNonce=next.nonce;externalPrepared=true;
    CyberpunkVR_UploadReservationExternalPlanSeq.fetch_add(1,std::memory_order_acq_rel);
    CyberpunkVR_UploadReservationExternalPlan=next;
    externalPlanSequence=CyberpunkVR_UploadReservationExternalPlanSeq.fetch_add(1,std::memory_order_release)+1;
    ExternalState(ExternalInstallState::Prepared);State(Status::Disabled);return true;
}
bool ReadExternalReceipt(ExternalInstallReceipt& receipt)noexcept {
    for(UINT attempt=0;attempt<3;++attempt){
        const auto sequence=CyberpunkVR_UploadReservationExternalReceiptSeq.load(std::memory_order_acquire);
        if(!sequence || sequence&1)return false;
        receipt=CyberpunkVR_UploadReservationExternalReceipt;
        std::atomic_thread_fence(std::memory_order_acquire);
        if(sequence==CyberpunkVR_UploadReservationExternalReceiptSeq.load(std::memory_order_acquire))return true;
    }return false;
}
bool AcceptExternal()noexcept {
    if(!externalPrepared || !externalPlanSequence || externalPlanSequence&1 ||
        CyberpunkVR_UploadReservationExternalPlanSeq.load(std::memory_order_acquire)!=externalPlanSequence)return false;
    const auto exported=CyberpunkVR_UploadReservationExternalPlan;std::atomic_thread_fence(std::memory_order_acquire);
    if(CyberpunkVR_UploadReservationExternalPlanSeq.load(std::memory_order_acquire)!=externalPlanSequence ||
        std::memcmp(&exported,&preparedExternalPlan,sizeof(exported)))return false;
    ExternalInstallReceipt receipt{};
    if(!ReadExternalReceipt(receipt) || !ValidExternalReceipt(preparedExternalPlan,receipt) || !CurrentExternalCode(preparedExternalPlan))return false;
    relay=externalRelay;patch=preparedExternalPlan.patch;siteProtection=DWORD(preparedExternalPlan.originalProtection);
    installed=true;installationHealthy=true;externalAccepted=true;
    ExternalState(ExternalInstallState::Accepted);State(Status::Disabled);return true;
}
void ApplyLoss()noexcept {if(lost.load(std::memory_order_acquire))ledger.QuarantineAll();}
}
void PrepareCodeGuards(uintptr_t base)noexcept {
    std::lock_guard lock(codeMutex);if(guardAttempted)return;
    guardAttempted=true;guardedBase=base;guardValid=WholeBody(base);if(!guardValid)State(Status::GuardMismatch);
}
bool Compatible()noexcept {std::lock_guard lock(codeMutex);return guardValid && guardedBase==Base();}
bool Installed()noexcept {std::lock_guard lock(codeMutex);return installed;}
void PollInstallation(uintptr_t base)noexcept {
    if(!CyberpunkVR_UploadReservationPrepareRequest.load(std::memory_order_acquire) &&
        !CyberpunkVR_UploadReservationAcceptRequest.load(std::memory_order_acquire))return;
    std::unique_lock lock(codeMutex,std::try_to_lock);if(!lock.owns_lock())return;
    CyberpunkVR_UploadReservationEnable.store(0,std::memory_order_release);
    const auto prepare=CyberpunkVR_UploadReservationPrepareRequest.exchange(0,std::memory_order_acq_rel);
    const auto accept=CyberpunkVR_UploadReservationAcceptRequest.exchange(0,std::memory_order_acq_rel);
    if(prepare>1 || accept>1){ExternalState(ExternalInstallState::UnsupportedRequest);State(Status::InstallFailure);return;}
    if(prepare && !PrepareExternal(base?base:Base())){if(!installed)ExternalState(ExternalInstallState::PrepareRejected);State(Status::InstallFailure);}
    if(accept && !AcceptExternal()){ExternalState(ExternalInstallState::ReceiptRejected);State(Status::InstallFailure);}
}
bool Install(uintptr_t base,bool threadsQuiescent)noexcept {
    std::lock_guard lock(codeMutex);
    if(installed)return installationHealthy && base==guardedBase;
    if(!threadsQuiescent){State(Status::InstallFailure);return false;}
    if(externalRelay){State(Status::InstallFailure);return false;}
    if(!guardAttempted){guardAttempted=true;guardedBase=base;guardValid=WholeBody(base);}
    if(!guardValid || guardedBase!=base || !WholeBody(base)){State(Status::GuardMismatch);return false;}
    if(!ConfigureXstate()){State(Status::InstallFailure);return false;}
    if(base>UINTPTR_MAX-WitnessRva || base>UINTPTR_MAX-RetryRva){State(Status::InstallFailure);return false;}
    const auto site=base+WitnessRva;
    relay=AllocateRelay(site);if(!relay){State(Status::InstallFailure);return false;}
    const auto destination=reinterpret_cast<uintptr_t>(&CvrNativeUploadReservationThunk);
    const uint8_t jump[]{0xFF,0x25,0,0,0,0};std::memcpy(relay,jump,sizeof(jump));std::memcpy(relay+6,&destination,sizeof(destination));
    const auto displacement=int64_t(reinterpret_cast<uintptr_t>(relay))-int64_t(site+5);
    DWORD previous{};
    if(displacement<INT32_MIN || displacement>INT32_MAX || !VirtualProtect(relay,14,PAGE_EXECUTE_READ,&previous) ||
        !FlushInstructionCache(GetCurrentProcess(),relay,14)){
        VirtualFree(relay,0,MEM_RELEASE);relay=nullptr;State(Status::InstallFailure);return false;
    }
    patch={0xE8,0,0,0,0,0x75,uint8_t(retryDisplacement)};
    const auto relative=int32_t(displacement);std::memcpy(patch.data()+1,&relative,sizeof(relative));
    bool written{};
    if(!Replace(site,original,patch,written,siteProtection)){
        // A failure after the store still leaves a reachable executable relay.
        installed=written;
        installationHealthy=false;
        if(!installed){VirtualFree(relay,0,MEM_RELEASE);relay=nullptr;}
        State(Status::InstallFailure);return false;
    }
    installed=true;installationHealthy=true;State(Status::Disabled);return true;
}
bool Remove(bool threadsQuiescent)noexcept {
    std::lock_guard lock(codeMutex);if(!installed)return true;
    if(!threadsQuiescent){State(Status::InstallFailure);return false;}
    if(externalAccepted){State(Status::InstallFailure);return false;}
    const auto site=guardedBase+WitnessRva;
    std::array<uint8_t,7> current{};
    if(!Read(site,current.data(),current.size())){State(Status::InstallFailure);return false;}
    if(current==patch){
        bool written{};DWORD previous{};
        if(!Replace(site,patch,original,written,previous) && !written){State(Status::InstallFailure);return false;}
    }else if(current!=original){State(Status::InstallFailure);return false;}
    DWORD previous{};
    // Retain the relay until both operations succeed, including a retry after
    // an earlier restore wrote original bytes but failed to flush/protect them.
    const bool restored=VirtualProtect(reinterpret_cast<void*>(site),original.size(),siteProtection,&previous)!=FALSE;
    const bool flushed=FlushInstructionCache(GetCurrentProcess(),reinterpret_cast<void*>(site),original.size())!=FALSE;
    if(!restored || !flushed){State(Status::InstallFailure);return false;}
    installed=false;installationHealthy=false;CyberpunkVR_UploadReservationEnable.store(0,std::memory_order_release);
    // Quiescence must include callbacks/relay execution before their storage is freed.
    VirtualFree(relay,0,MEM_RELEASE);relay=nullptr;
    siteProtection=0;State(Status::Disabled);return true;
}
bool BeginGeneration(uint64_t ring,uint64_t generation,uint64_t capacity)noexcept {
    std::lock_guard lock(ledgerMutex);ApplyLoss();return ledger.BeginGeneration(ring,generation,capacity);
}
void EndGeneration(uint64_t ring)noexcept {std::lock_guard lock(ledgerMutex);ledger.EndGeneration(ring);}
Status Find(uint64_t sequence,Reservation& output)noexcept {
    std::lock_guard lock(ledgerMutex);ApplyLoss();return ledger.Find(sequence,output);
}
Status ValidateRange(const Reservation& reservation,uint64_t localOffset,uint64_t physicalOffset,uint64_t paddedBytes,
    uint64_t logicalBytes,uint64_t backingBytes,Range& output)noexcept {
    std::lock_guard lock(ledgerMutex);ApplyLoss();return ledger.ValidateRange(reservation,localOffset,physicalOffset,paddedBytes,logicalBytes,backingBytes,output);
}
void Clear()noexcept {
    std::lock_guard lock(ledgerMutex);
    if(epoch==UINT64_MAX){ledger.QuarantineAll();lost.store(true,std::memory_order_release);State(Status::Capacity);return;}
    ledger.Reset(++epoch);CyberpunkVR_UploadReservationRows.fill({});lost.store(false,std::memory_order_release);
    CyberpunkVR_UploadReservationCount.store(0,std::memory_order_release);CyberpunkVR_UploadReservationDropped.store(0,std::memory_order_relaxed);
}
}
extern "C" void CvrNativeUploadReservationObserve(const cvr::stereo::upload_reservation::ThunkFrame* frame)noexcept {
    using namespace cvr::stereo::upload_reservation;
    if(!frame || !(frame->flags&0x40) || !CyberpunkVR_UploadReservationEnable.load(std::memory_order_relaxed))return;
    if(!ledgerMutex.try_lock()){
        lost.store(true,std::memory_order_release);CyberpunkVR_UploadReservationDropped.fetch_add(1,std::memory_order_relaxed);State(Status::Contention);return;
    }
    std::lock_guard lock(ledgerMutex,std::adopt_lock);ApplyLoss();
    Witness witness{frame->rbx,frame->r9,frame->rcx,frame->rdx,frame->r13,frame->capacity,frame->nativeStack,frame->nativeCaller,frame->flags};
    Row row;const auto result=ledger.Observe(witness,GetCurrentThreadId(),row,frame->r8==frame->capacity);
    if(row.sequence){CyberpunkVR_UploadReservationRows[size_t(row.sequence-1)]=row;
        CyberpunkVR_UploadReservationCount.store(uint32_t(ledger.Count()),std::memory_order_release);}
    else if(result==Status::Capacity){lost.store(true,std::memory_order_release);CyberpunkVR_UploadReservationDropped.fetch_add(1,std::memory_order_relaxed);}
    State(result);
}
#endif
#endif
