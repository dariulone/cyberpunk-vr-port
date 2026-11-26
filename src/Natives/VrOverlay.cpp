#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Natives/NativeFunctions.hpp"
#include "Overlay/VrOverlay.hpp"
#include <RED4ext/Scripting/Utils.hpp>
#if CVR_RUST_VRIK
// The natives are rust/crates/cvr-ffi/src/body_setup.rs (the overlay they bridge: rust/crates/cvr-body/src/vr_overlay.rs).
#else
void VROverlayCommand(RED4ext::IScriptable*,RED4ext::CStackFrame* f,int32_t* out,int64_t){++f->code;if(out)*out=cvr::vrui::PollCommand();}
void VROverlayMenu(RED4ext::IScriptable*,RED4ext::CStackFrame* f,bool* out,int64_t){
    bool ready=false,inGame=false;RED4ext::CString labels;
    RED4ext::GetParameter(f,&ready);RED4ext::GetParameter(f,&inGame);RED4ext::GetParameter(f,&labels);++f->code;
    cvr::vrui::BridgeUpdate(ready,inGame,labels.c_str());if(out)*out=cvr::vrui::Visible();
}
void VROverlayToggle(RED4ext::IScriptable*,RED4ext::CStackFrame* f,void*,int64_t){++f->code;cvr::vrui::Toggle();}
#endif
#endif
