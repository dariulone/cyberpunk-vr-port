//! The stereo GPU probe's start and settings (src/Render/StereoGpuProbe.cpp, `Draw`'s request gate and its setting
//! checks): the draw a request starts on (the second eye's, the single 7212-index instance unless a group is probed,
//! three non-contiguous colour targets -- none for a depth prepass -- and every capture flag); and the settings' rules
//! (a group needs the second eye's open group on a plane with the native reference; late visibility needs a group, a
//! preparation gate and full MAIN view and reference masks; scene depth needs late visibility; the scene route needs
//! scene depth and is 0..2; mixed materials need scene depth; a fine-rate override 0..2 needs scene depth without the
//! route; a depth prepass needs scene depth without mixed materials or a rate override; a start index or run needs the
//! depth prepass).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_sgps_start` and `cvr_sgps_settings` (the draw as
//! geometry_commands' DrawRecord; the settings mirrored); the unit tests below cover each rule.

use crate::geometry_commands::DrawRecord;

/// The probe's settings and the open group, as the .cpp reads them.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub group_id: u64,
    pub group_side: i32,
    pub group_plane: u32,
    pub route_mode: u32,
    pub rate_override: u32,
    pub start_indices: u32,
    pub start_run: u32,
    pub prepare_gate_ms: u32,
    pub main_view_mask: u32,
    pub main_reference_mask: u32,
    pub group: bool,
    pub late: bool,
    pub scene: bool,
    pub mixed: bool,
    pub depth_only: bool,
    pub native_reference: bool,
}
const _: () = assert!(core::mem::size_of::<Settings>() == 56);

/// The draw a request starts on.
pub fn start(d: &DrawRecord, group: bool, depth_prepass: bool) -> bool {
    d.side == 1
        && (group || (d.indices == 7212 && d.instances == 1))
        && d.rt_count == if depth_prepass { 0 } else { 3 }
        && !(d.rt_count != 0 && d.rt_contiguous != 0)
        && d.flags & 127 == 63
}
/// The settings' rules.
pub fn settings_valid(s: &Settings) -> bool {
    let route = s.route_mode != 0;
    !(s.group && (s.group_id == 0 || s.group_side != 1 || s.group_plane == u32::MAX || !s.native_reference))
        && !(s.late && (!s.group || s.prepare_gate_ms == 0 || s.main_view_mask != u32::MAX || s.main_reference_mask != u32::MAX))
        && !(s.scene && !s.late)
        && !(route && !s.scene)
        && s.route_mode <= 2
        && !(s.mixed && !s.scene)
        && !(s.rate_override > 2 || (s.rate_override != 0 && (!s.scene || route)))
        && !(s.depth_only && (!s.scene || s.mixed || s.rate_override != 0))
        && !((s.start_indices != 0 || s.start_run != 0) && !s.depth_only)
}

// ---- the C ABI

/// # Safety
/// `draw` is a valid DrawRecord.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgps_start(draw: *const DrawRecord, group: bool, depth_prepass: bool) -> bool {
    start(unsafe { &*draw }, group, depth_prepass)
}
/// # Safety
/// `settings` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgps_settings(settings: *const Settings) -> bool {
    settings_valid(unsafe { &*settings })
}

#[cfg(test)]
mod tests {
    //! Each rule.
    use super::*;

    fn draw() -> DrawRecord {
        let mut d: DrawRecord = unsafe { core::mem::zeroed() };
        (d.side, d.indices, d.instances, d.rt_count, d.rt_contiguous, d.flags) = (1, 7212, 1, 3, 0, 63 | 128);
        d
    }
    #[test]
    fn starts() {
        assert!(start(&draw(), false, false));
        let changes: [fn(&mut DrawRecord); 5] = [|d| d.side = 0, |d| d.indices = 7213, |d| d.instances = 2, |d| d.rt_contiguous = 1, |d| d.flags = 62];
        for change in changes {
            let mut d = draw();
            change(&mut d);
            assert!(!start(&d, false, false));
        }
        // A group admits any draw shape; a depth prepass wants no colour targets, contiguous or not.
        let mut d = draw();
        d.indices = 99;
        assert!(start(&d, true, false));
        assert!(!start(&draw(), false, true));
        let mut d = draw();
        (d.rt_count, d.rt_contiguous) = (0, 1);
        assert!(start(&d, false, true));
    }
    fn plain() -> Settings {
        Settings {
            group_id: 0,
            group_side: -1,
            group_plane: u32::MAX,
            route_mode: 0,
            rate_override: 0,
            start_indices: 0,
            start_run: 0,
            prepare_gate_ms: 0,
            main_view_mask: 0,
            main_reference_mask: 0,
            group: false,
            late: false,
            scene: false,
            mixed: false,
            depth_only: false,
            native_reference: false,
        }
    }
    fn depth() -> Settings {
        Settings {
            group_id: 4,
            group_side: 1,
            group_plane: 2,
            prepare_gate_ms: 5,
            main_view_mask: u32::MAX,
            main_reference_mask: u32::MAX,
            group: true,
            late: true,
            scene: true,
            depth_only: true,
            native_reference: true,
            start_run: 2,
            ..plain()
        }
    }
    #[test]
    fn settings() {
        assert!(settings_valid(&plain()));
        assert!(settings_valid(&depth()));
        let broken: [fn(&mut Settings); 14] = [
            |s| s.group_id = 0,
            |s| s.group_side = 0,
            |s| s.group_plane = u32::MAX,
            |s| s.native_reference = false,
            |s| s.prepare_gate_ms = 0,
            |s| s.main_view_mask = 1,
            |s| s.main_reference_mask = 1,
            |s| s.late = false,
            |s| s.group = false,
            |s| s.mixed = true,
            |s| s.rate_override = 1,
            |s| s.scene = false,
            |s| s.route_mode = 3,
            |s| (s.depth_only, s.start_run) = (false, 2),
        ];
        for change in broken {
            let mut s = depth();
            change(&mut s);
            assert!(!settings_valid(&s), "{s:?}");
        }
        // The route and a rate override need scene depth; the override excludes the route.
        let scene = Settings { depth_only: false, start_run: 0, ..depth() };
        assert!(settings_valid(&Settings { route_mode: 2, ..scene }));
        assert!(settings_valid(&Settings { rate_override: 2, ..scene }));
        assert!(!settings_valid(&Settings { rate_override: 3, ..scene }));
        assert!(!settings_valid(&Settings { rate_override: 1, route_mode: 1, ..scene }));
        assert!(!settings_valid(&Settings { route_mode: 1, ..plain() }));
        assert!(!settings_valid(&Settings { mixed: true, ..plain() }));
        assert!(settings_valid(&Settings { mixed: true, ..scene }));
    }
}
