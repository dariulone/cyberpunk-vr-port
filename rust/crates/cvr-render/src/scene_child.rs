//! A SCENE CHILD'S UNIQUE SLOT (the pure part of include/Render/NativeSceneChild.hpp, detail::UniqueChild): of the
//! eleven scene children (each with its slot, object, vtable and work), the observed child must match exactly one --
//! only the peer view's tail (section 2: slots 6..10) or the main view's prefix (section 1: slots 0..5), the section
//! IDs of SceneGraphExpansion's mappings, never every node of either eye. The header forwards to it in the Rust build;
//! tools/single_pass_tests/scene_child_test runs its cases against it.

/// detail::ChildKey.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct ChildKey {
    pub object: u64,
    pub vtable: u64,
    pub work: u64,
    pub slot: u32,
}
const _: () = assert!(core::mem::size_of::<ChildKey>() == 32);

/// UniqueChild: the matching slot, or None.
pub fn unique_child(
    children: &[ChildKey],
    view: u32,
    section: u32,
    observed: &ChildKey,
) -> Option<u32> {
    if children.len() != 11
        || observed.object == 0
        || observed.vtable == 0
        || observed.work == 0
        || !((view == 1 && section == 2) || (view == 0 && section == 1))
    {
        return None;
    }
    for (i, child) in children.iter().enumerate() {
        if child.slot as usize != i || child.object == 0 || child.vtable == 0 || child.work == 0 {
            return None;
        }
    }
    let (begin, end) = if section == 2 { (6u32, 11u32) } else { (0, 6) };
    let mut found = None;
    let mut matches = 0;
    for i in begin..end {
        let child = &children[i as usize];
        if child.object == observed.object
            && child.vtable == observed.vtable
            && child.work == observed.work
        {
            found = Some(i);
            matches += 1;
        }
    }
    if matches == 1 {
        found
    } else {
        None
    }
}

/// detail::UniqueChild (`slot` u32::MAX on a refusal).
///
/// # Safety
/// `children` holds `count` keys; `observed` is valid; `slot` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sc_unique_child(
    children: *const ChildKey,
    count: usize,
    view: u32,
    section: u32,
    observed: *const ChildKey,
    slot: *mut u32,
) -> bool {
    unsafe {
        let children = if count == 0 || children.is_null() {
            &[][..]
        } else {
            core::slice::from_raw_parts(children, count)
        };
        let found = unique_child(children, view, section, &*observed);
        *slot = found.unwrap_or(u32::MAX);
        found.is_some()
    }
}
