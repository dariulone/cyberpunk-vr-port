#pragma once
#include <cstdint>
#include <memory>

namespace cvr::framegen {
struct DisplayDeadline;
class SteamVrPacer {
public:
    enum class Kind { Real,Synthetic,Repeat };
    SteamVrPacer();
    ~SteamVrPacer();
    // Single XR-loop owner. Begin is inside a successfully begun frame, before
    // image selection/copy. BeforeSubmit holds after GPU work is queued.
    // Stop only after that owner has quiesced.
    void Begin(bool enabled);
    // Probe AFTER the hold. A missing receipt/probe preserves legacy feedback.
    // The callback only polls a completed fence; it must never wait.
    void BeforeSubmit(Kind,uint64_t receipt=0,bool (*ready)(uint64_t)=nullptr,DisplayDeadline* probe=nullptr,void* wake=nullptr,bool (*prepare)(void*)=nullptr,void* context=nullptr);
    void AfterSubmit(bool accepted);
    void Reset();
private:
    struct Impl;
    std::unique_ptr<Impl> impl;
};
}
