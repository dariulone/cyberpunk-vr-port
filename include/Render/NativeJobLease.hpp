#pragma once
#include <array>

namespace cvr::stereo {
struct JobLeaseApi {
    void(*retain)(void*);
    void(*release)(void**);
    bool(*complete)(void*);
};
#if CVR_RUST_VRIK
// The lease: rust/crates/cvr-render/src/native/job_lease.rs through job_lease_abi.rs
// (tools/single_pass_tests/cpu_collector_test runs its lease cases against it in its Rust variant).
struct RustJobLease;
extern "C" {
RustJobLease* cvr_job_lease_new(const JobLeaseApi*);
void cvr_job_lease_free(RustJobLease*);
bool cvr_job_lease_capture(RustJobLease*,void*,void*);
bool cvr_job_lease_complete(const RustJobLease*);
}
#endif
// Retain while the source native job scope is live, then seal after all work was
// queued. Hold both begin/end groups: Advance moves the pending end group to
// begin and creates a new empty end, which alone would falsely appear complete.
#if CVR_RUST_VRIK
class NativeJobLease {
public:
    explicit NativeJobLease(JobLeaseApi api):rust_(cvr_job_lease_new(&api)){}
    ~NativeJobLease(){cvr_job_lease_free(rust_);}
    NativeJobLease(const NativeJobLease&)=delete;
    NativeJobLease& operator=(const NativeJobLease&)=delete;
    bool Capture(void* begin,void* end){return cvr_job_lease_capture(rust_,begin,end);}
    bool Complete()const{return cvr_job_lease_complete(rust_);}
private:
    RustJobLease* rust_;
};
#else
class NativeJobLease {
public:
    explicit NativeJobLease(JobLeaseApi api):api_(api){}
    ~NativeJobLease(){for(auto& group:groups_)if(group)api_.release(&group);}
    NativeJobLease(const NativeJobLease&)=delete;
    NativeJobLease& operator=(const NativeJobLease&)=delete;
    bool Capture(void* begin,void* end) {
        if(sealed_ || !begin || !end || !api_.retain || !api_.release || !api_.complete)return false;
        groups_={begin,begin==end?nullptr:end};for(auto* group:groups_)if(group)api_.retain(group);sealed_=true;return true;
    }
    bool Complete()const {
        if(!sealed_)return false;for(auto* group:groups_)if(group && !api_.complete(group))return false;return true;
    }
private:
    JobLeaseApi api_;std::array<void*,2> groups_{};bool sealed_{};
};
#endif
}
