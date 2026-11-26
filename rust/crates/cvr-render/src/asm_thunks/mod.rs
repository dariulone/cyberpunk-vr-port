//! The port's assembly thunks (formerly src/Hooks/*.asm, which the reference build still assembles with ml64): the
//! engine leaf routines' register contracts -- flags, XMM outputs, home slots, XSAVE areas -- kept around the Rust and
//! C++ callbacks, in LLVM's assembler (`global_asm!`). Each module is its .asm macro-expanded (`ml64 /EP`) and
//! translated; the machine code of all 37 functions, their relocations and unwind data match the ml64 objects (a
//! register-to-register MOV/ALU op takes the other, equivalent direction of its encoding; the frame register's reserved
//! OpInfo in UWOP_SET_FPREG is zero). Only in the plugin build: the callbacks and originals they reach are the DLL's.
mod camera_pose_leaf_thunks;
mod native_bounds_thunk;
mod native_draw_origin_thunk;
mod native_instance_reservation_thunk;
mod native_scene_update_thunk;
mod native_source_srv_binding_thunk;
mod native_upload_reservation_thunk;
mod photo_capture_clamp_thunk;
mod render_graph_cache_thunk;
mod story_attention_thunk;
mod surveillance_input_thunk;
mod swimming_action_leaf_thunk;
mod world_marker_text_thunk;
mod world_touch_triangle_thunk;
