//! The stereo geometry pass's source scopes (src/Render/StereoGeometryPass.cpp, `stereo::Scope` and `ReadContract`): a
//! shared-read proof's scope belongs to an eye's source draw when it is that family's source root and the draw's root,
//! with a list, generation, occurrence and source, on the draw's own eye (0 or 1); and a read contract's eye scope is
//! the family's root with a list, generation, occurrence and source on the other eye's slot (eye 0's proof carries eye 1).
//!
//! In the Rust build (`CVR_RUST_VRIK`) both forward to `cvr_sgp_scope` (source_binding_assembly::Scope mirrored, 48
//! bytes); `tools/single_pass_tests/geometry_pass_test` reaches them in its Rust variant, and the unit test below covers
//! each field.

/// source_binding_assembly::Scope.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    pub root: u64,
    pub list: u64,
    pub generation: u64,
    pub occurrence: u64,
    pub source: u64,
    pub eye: u32,
}
const _: () = assert!(core::mem::size_of::<Scope>() == 48);

fn present(s: &Scope) -> bool {
    s.list != 0 && s.generation != 0 && s.occurrence != 0 && s.source != 0
}
/// Scope: the proof's scope on the source draw's own eye.
pub fn draw_scope(s: &Scope, family_root: u64, draw_root: u64, draw_side: u32) -> bool {
    s.root == family_root && s.root == draw_root && present(s) && s.eye == draw_side && s.eye < 2
}
/// ReadContract's eye scope: the family's root on the other eye's slot.
pub fn contract_scope(s: &Scope, family_root: u64, eye: u32) -> bool {
    s.root != 0 && s.root == family_root && present(s) && s.eye == if eye != 0 { 0 } else { 1 }
}

// ---- the C ABI

/// `which`: 0 the draw scope (`second` the draw's root, `side` its eye), 1 the contract scope (`side` the contract eye).
///
/// # Safety
/// `scope` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgp_scope(which: u32, scope: *const Scope, family_root: u64, second: u64, side: u32) -> bool {
    let s = unsafe { &*scope };
    if which == 0 { draw_scope(s, family_root, second, side) } else { contract_scope(s, family_root, side) }
}

#[cfg(test)]
mod tests {
    //! Each field of both scopes.
    use super::*;

    #[test]
    fn scopes() {
        let s = Scope { root: 7, list: 1, generation: 2, occurrence: 3, source: 4, eye: 1 };
        assert!(draw_scope(&s, 7, 7, 1));
        assert!(!draw_scope(&s, 7, 8, 1));
        assert!(!draw_scope(&s, 7, 7, 0));
        assert!(!draw_scope(&Scope { source: 0, ..s }, 7, 7, 1));
        assert!(!draw_scope(&Scope { eye: 2, ..s }, 7, 7, 2));
        assert!(contract_scope(&s, 7, 0));
        assert!(!contract_scope(&s, 7, 1));
        assert!(contract_scope(&Scope { eye: 0, ..s }, 7, 1));
        assert!(!contract_scope(&Scope { root: 0, ..s }, 0, 0));
        assert!(!contract_scope(&Scope { occurrence: 0, ..s }, 7, 0));
    }
}
