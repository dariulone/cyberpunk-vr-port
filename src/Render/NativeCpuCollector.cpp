#if !CVR_RUST_VRIK
// The C++ build's owner; the Rust build's is rust/crates/cvr-render/src/cpu_collector.rs (the header calls it).
#include "Render/NativeCpuCollector.hpp"

namespace cvr::stereo {
NativeCpuCollector::~NativeCpuCollector(){Reset();}
void NativeCpuCollector::Reset(){if(owner_){void* owner=owner_;owner_=nullptr;api_.destroy(owner,1);}}
void* NativeCpuCollector::Storage()const{return owner_?static_cast<uint8_t*>(owner_)+0xC0:nullptr;}
void* NativeCpuCollector::Metadata()const{return owner_?static_cast<uint8_t*>(owner_)+0xE05C0:nullptr;}
bool NativeCpuCollector::Create(const CollectorInput& input) {
    if(owner_ || !api_.create || !api_.destroy || !api_.initialize || !input.viewData || !input.scene)return false;
    input_=input;void* allocated{};api_.create(&allocated);if(!allocated)return false;
    owner_=allocated;
    api_.initialize(Storage(),input_.viewData,input_.mode,input_.camera.bytes.data(),input_.scene,Metadata(),input_.viewFlags.data());
    return true;
}
}
#endif
