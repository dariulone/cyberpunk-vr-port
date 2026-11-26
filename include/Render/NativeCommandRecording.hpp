#pragma once
#include "Render/NativeStereoCameraUpload.hpp"
#include <d3d12.h>
#include <wrl/client.h>
#include <array>
#include <climits>
#include <cstdint>
#include <span>

#if CVR_RUST_VRIK
// THE CONTRACT IS RUST'S (rust/crates/cvr-render/src/native_recording.rs): the pure parts below and the module.
namespace cvr::stereo::native_recording::detail { struct SubmissionState; }
extern "C" {
uint64_t cvr_native_recording_fingerprint(const uint8_t* bytes, size_t len);
bool cvr_native_recording_caller(uint64_t base, uint64_t address, uint32_t rva);
bool cvr_native_recording_execute_origin(uint64_t base, uint64_t imageBytes, uint64_t caller, const uint64_t* frames, size_t count);
bool cvr_native_recording_context_matches(cvr::stereo::camera_upload::Context expected, uint64_t list, uint64_t actualList,
                                          uint64_t state, UINT kind, uint64_t submitted);
bool cvr_native_recording_before_submit_state(cvr::stereo::native_recording::detail::SubmissionState* state, UINT occurrences,
                                              bool native);
void cvr_native_recording_submitted_state(cvr::stereo::native_recording::detail::SubmissionState* state);
}
#endif
namespace cvr::stereo::native_recording {
namespace detail {
inline constexpr uint32_t ResetReturn=0x3E626A,ExecuteReturn=0x3E124F;
struct Code {uint32_t rva,bytes;uint64_t hash;};
// Acquire/reset, handle ownership transfer, bulk/single submit, split fallback,
// context return and queue selection. Recursive submits use disjoint halves.
// 2.5 (audited 2026-10-23): context layout unchanged (+0x28 allocator, +0x30 list, +0x60, +0x68 kind,
// +0x638 serial), Reset and ExecuteCommandLists return sites as above, queue at registry+0x14104D0. Changes:
// locks through guard objects, the context allocation and slot-index pool outlined, sub_1422EF630 (2.31
// sub_1401FAD8C) submits the direct list then the copy list on fixed queues instead of picking them by mode.
inline constexpr std::array<Code,11> Contract{{
    {0x3E5D54,1695,0x0FB56D2DE5F3E34Aull},{0x3E79C8,239,0x6B24682AECE56AD1ull},
    {0x22EF830,1493,0x9117D2376320C0B7ull},{0x22EF630,0x1E6,0x8693CD8B6182484Aull},
    {0x3E0B28,1269,0xC8443F2F3CE39882ull},{0x5B3DA8,640,0xE030F844D980B8EEull},
    {0x3E1020,682,0x898F20179715110Full},{0x2469DCE,206,0x2C1F0022E230A82Cull},
    {0x3E1360,403,0x9C8B7DC7F6242F45ull},{0x3E14F4,167,0x13230B2BADF1035Bull},
    {0x5B4110,77,0xFA1D4E49FE87F2EEull}
}};
#if CVR_RUST_VRIK
inline uint64_t Fingerprint(std::span<const uint8_t> bytes){return cvr_native_recording_fingerprint(bytes.data(),bytes.size());}
inline bool Caller(uint64_t base,uint64_t address,uint32_t rva){return cvr_native_recording_caller(base,address,rva);}
inline constexpr size_t MaxUnwoundFrames=64;
// Frames must come from the OS unwinder, in nearest-to-farthest order.
inline bool ExecuteOrigin(uint64_t base,uint64_t imageBytes,uint64_t caller,std::span<const uint64_t> frames){
    return cvr_native_recording_execute_origin(base,imageBytes,caller,frames.data(),frames.size());
}
inline bool ContextMatches(camera_upload::Context expected,uint64_t list,uint64_t actualList,uint64_t state,UINT kind,uint64_t submitted){
    return cvr_native_recording_context_matches(expected,list,actualList,state,kind,submitted);
}
struct SubmissionState {
    UINT pending{};bool submitted{},rejected{};
    bool Fresh()const{return !pending && !submitted && !rejected;}
    bool BeforeSubmit(UINT occurrences,bool native){return cvr_native_recording_before_submit_state(this,occurrences,native);}
    bool Validating()const{return pending==1 && !submitted && !rejected;}
    void Submitted(){cvr_native_recording_submitted_state(this);}
};
static_assert(sizeof(SubmissionState)==8,"mirrored by SubmissionState in rust/crates/cvr-render/src/native_recording.rs");
#else
inline uint64_t Fingerprint(std::span<const uint8_t> bytes){
    uint64_t hash=14695981039346656037ull;for(const auto byte:bytes)hash=(hash^byte)*1099511628211ull;return hash;
}
inline bool Caller(uint64_t base,uint64_t address,uint32_t rva){
    return base && base<=UINT64_MAX-rva && address==base+rva;
}
inline constexpr size_t MaxUnwoundFrames=64;
// Frames must come from the OS unwinder, in nearest-to-farthest order.
inline bool ExecuteOrigin(uint64_t base,uint64_t imageBytes,uint64_t caller,std::span<const uint64_t> frames){
    if(!base || imageBytes<=ExecuteReturn || imageBytes>UINT64_MAX-base || frames.size()>MaxUnwoundFrames)return false;
    const auto inGame=[&](uint64_t address){return address>=base && address-base<imageBytes;};
    if(inGame(caller))return Caller(base,caller,ExecuteReturn);
    for(const auto frame:frames)if(inGame(frame))return Caller(base,frame,ExecuteReturn);
    return false;
}
inline bool ContextMatches(camera_upload::Context expected,uint64_t list,uint64_t actualList,uint64_t state,UINT kind,uint64_t submitted){
    return expected && list && expected.list==list && actualList==list && expected.state==state &&
        (kind==1 || kind==2) && !submitted;
}
struct SubmissionState {
    UINT pending{};bool submitted{},rejected{};
    bool Fresh()const{return !pending && !submitted && !rejected;}
    bool BeforeSubmit(UINT occurrences,bool native){
        if(!occurrences)return false;
        const bool first=Fresh();if(pending!=UINT_MAX)++pending;else rejected=true;
        if(!first || occurrences!=1 || !native)rejected=true;
        return Validating();
    }
    bool Validating()const{return pending==1 && !submitted && !rejected;}
    void Submitted(){if(pending){--pending;submitted=true;}}
};
#endif
}
struct Lease {
    Microsoft::WRL::ComPtr<IUnknown> token;
    uint64_t generation{},context{},list{},registry{};
};
// This is an audited native ownership contract, not GPU-safe support for replay
// through a foreign/manual caller. It must be acquired before recording probes.
bool Compatible();
void Reset(ID3D12GraphicsCommandList*,uintptr_t caller);
bool Capture(ID3D12GraphicsCommandList*,camera_upload::Context,Lease&);
// Read-only validation at the current native recording position. No recapture,
// token mutation, native resource-use registration or submission change.
bool ValidateRecording(const Lease&,ID3D12GraphicsCommandList*,camera_upload::Context);
void BeforeSubmit(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*,uintptr_t caller);
bool ValidateSubmission(const Lease&,ID3D12CommandQueue*);
void Submitted(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
// 1 arguments, 2 fingerprints, 3 missing token, 4 reset origin, 5 generation,
// 6 native context, 7 registry, 8 submit origin/queue, 9 replay, 10 token storage,
// 11 allocation/exception, 12 not pending.
uint32_t LastFailure();
// Execute caller, expected caller, actual/expected queue, occurrences,
// pending count | submitted<<32 | rejected<<33, Reset caller, generation.
std::array<uint64_t,8> LastDetails();
}
