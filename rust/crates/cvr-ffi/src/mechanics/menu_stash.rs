//! stash_visit.lua's V.current for the menu (the visit itself is menu_vendor's).
use super::menu_core::Menu;
pub(super) fn current(m: &Menu) -> bool {
    super::menu_vendor::stash_current_m(m)
}
