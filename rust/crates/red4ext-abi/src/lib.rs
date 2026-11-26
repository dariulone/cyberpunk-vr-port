//! The REDengine boundary in Rust (stage 2 of docs/rust-migration-plan.md), written against the SDK the port pins:
//! marklove5102/RED4ext.SDK d02a1f32 plus the game 2.5 commits under build/patchday/framework-src.
//!
//! Every layout below mirrors a struct of that SDK and carries its size/offset assertions; `hashes` is generated
//! from its `Detail/AddressHashes.hpp` (`cvr-tool sdk-hashes`). `unsafe` is concentrated here: callers get typed
//! wrappers whose documentation says on which thread they are valid and what ends the lifetime of what they return.
#![allow(clippy::missing_safety_doc)]

pub mod api;
pub mod callback;
pub mod cname;
pub mod cstring;
pub mod dynarray;
pub mod engine;
pub mod fnv;
pub mod function;
pub mod handle;
pub mod hashes;
pub mod memory;
pub mod reloc;
pub mod resource;
pub mod rtti;
pub mod stack;
pub mod tweakdb;
pub mod tweakdbid;

/// `static_assert(sizeof(T) == size)` and `offsetof` checks, mirroring RED4EXT_ASSERT_SIZE / RED4EXT_ASSERT_OFFSET.
#[macro_export]
macro_rules! assert_layout {
    ($t:ty, size = $size:expr $(, $field:ident = $off:expr)* $(,)?) => {
        const _: () = assert!(::core::mem::size_of::<$t>() == $size);
        $(const _: () = assert!(::core::mem::offset_of!($t, $field) == $off);)*
    };
}
