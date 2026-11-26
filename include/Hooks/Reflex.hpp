#pragma once
#include <string>
#if CVR_RUST_VRIK
#include <cstddef>
// The hook and its state: rust/crates/cvr-rig/src/vrik/reflex_hook.rs.
extern "C" {
int cvr_reflex_get_mode();
void cvr_reflex_set_mode(int);
int cvr_reflex_get_applied_mode();
void cvr_reflex_timing_report(bool request,void(*sink)(void*,const char*,size_t),void* out);
}
namespace cvr::reflex {
inline int GetMode() {return cvr_reflex_get_mode();}
inline void SetMode(int mode) {cvr_reflex_set_mode(mode);}
inline int GetAppliedMode() {return cvr_reflex_get_applied_mode();}
inline std::string TimingReport(bool request) {
    std::string text;
    cvr_reflex_timing_report(request,[](void* out,const char* data,size_t size){static_cast<std::string*>(out)->assign(data,size);},&text);
    return text;
}
}
#else
namespace cvr::reflex {
// -1 follows the game; 0 off; 1 low latency; 2 low latency + boost.
int GetMode();
void SetMode(int mode);
int GetAppliedMode(); // -2 until a recognized options block has been submitted.
// Opt-in diagnostic: request on the script thread, collect on the renderer's
// serialized SetOptions path, read the result later. No sampling thread/waits.
std::string TimingReport(bool request);
}
#endif
