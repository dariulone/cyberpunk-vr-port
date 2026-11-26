//! The retained geometry's capture configuration (src/Render/NativeRetainedGeometry.cpp, `FrameBoundary`): a requested
//! capture's switches must form a consistent chain, checked in three phases at the .cpp's points. Phase 0: scene access
//! is off or the observer (1) with both native scene sections (split 2); the infix consumer needs the owned current
//! path, the native tail the infix, full T5 the tail, borrowed tables full T5, fixed assets borrowed tables and VRS
//! refinement the tail. Phase 1 (once the selectors are known): the current path needs request 1 at stage 7 with
//! selectors, IA snapshots, source tables and the source stream. Phase 2 (after the selector catalog): CBV snapshots need
//! the stream, source CBVs and tables; source details and tables the stream and CBVs; indirect and IA snapshots the
//! stream -- each under request 1. The result is the first broken rule (1..13), or 0.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the checks forward to `cvr_nrg_config_failure` (the configuration mirrored);
//! the unit tests below cover each rule.

/// The configuration the checks read.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Config {
    pub request: u32,
    pub stage: u32,
    pub scene_access: u32,
    pub scene_split: u32,
    pub current: bool,
    pub infix: bool,
    pub tail: bool,
    pub full_t5: bool,
    pub borrowed_tables: bool,
    pub fixed_assets: bool,
    pub vrs_refinement: bool,
    pub selectors: bool,
    pub ia_snapshots: bool,
    pub tables: bool,
    pub stream: bool,
    pub cbv_snapshots: bool,
    pub cbvs: bool,
    pub details: bool,
    pub snapshots: bool,
}
const _: () = assert!(core::mem::size_of::<Config>() == 32);

/// The first broken rule of `phase`, or 0.
pub fn config_failure(c: &Config, phase: u32) -> u32 {
    let first = request_one(c);
    let rules: &[(u32, bool)] = match phase {
        0 => &[
            (1, c.scene_access > 1 || (c.scene_access == 1 && c.scene_split != 2)),
            (2, c.infix && !c.current),
            (3, c.tail && !c.infix),
            (4, c.full_t5 && !c.tail),
            (5, c.borrowed_tables && !c.full_t5),
            (6, c.fixed_assets && !c.borrowed_tables),
            (7, c.vrs_refinement && !c.tail),
        ],
        1 => &[(8, c.current && (!first || c.stage != 7 || !c.selectors || !c.ia_snapshots || !c.tables || !c.stream))],
        2 => &[
            (9, c.cbv_snapshots && (!c.stream || !c.cbvs || !c.tables || !first)),
            (10, c.details && (!c.stream || !c.cbvs || !first)),
            (11, c.tables && (!c.stream || !c.cbvs || !first)),
            (12, c.snapshots && (!c.stream || !first)),
            (13, c.ia_snapshots && (!c.stream || !first)),
        ],
        _ => &[],
    };
    rules.iter().find(|(_, broken)| *broken).map_or(0, |(rule, _)| *rule)
}
fn request_one(c: &Config) -> bool {
    c.request == 1
}

// ---- the C ABI

/// # Safety
/// `config` is the mirrored configuration.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_config_failure(config: *const Config, phase: u32) -> u32 {
    config_failure(unsafe { &*config }, phase)
}

#[cfg(test)]
mod tests {
    //! Each rule.
    use super::*;

    fn full() -> Config {
        Config {
            request: 1,
            stage: 7,
            scene_access: 1,
            scene_split: 2,
            current: true,
            infix: true,
            tail: true,
            full_t5: true,
            borrowed_tables: true,
            fixed_assets: true,
            vrs_refinement: true,
            selectors: true,
            ia_snapshots: true,
            tables: true,
            stream: true,
            cbv_snapshots: true,
            cbvs: true,
            details: true,
            snapshots: true,
        }
    }
    fn all(c: &Config) -> [u32; 3] {
        [config_failure(c, 0), config_failure(c, 1), config_failure(c, 2)]
    }
    #[test]
    fn consistent() {
        assert_eq!(all(&full()), [0, 0, 0]);
        assert_eq!(all(&Config::default()), [0, 0, 0]);
        assert_eq!(all(&Config { request: 2, stage: 6, ..Config::default() }), [0, 0, 0]);
        assert_eq!(config_failure(&Config { scene_access: 9, ..full() }, 3), 0);
    }
    #[test]
    fn chain() {
        assert_eq!(all(&Config { scene_access: 2, ..full() })[0], 1);
        assert_eq!(all(&Config { scene_split: 1, ..full() })[0], 1);
        assert_eq!(all(&Config { scene_access: 0, scene_split: 0, ..full() })[0], 0);
        assert_eq!(all(&Config { current: false, ..full() }), [2, 0, 0]);
        assert_eq!(all(&Config { infix: false, ..full() })[0], 3);
        assert_eq!(all(&Config { tail: false, vrs_refinement: false, ..full() })[0], 4);
        assert_eq!(all(&Config { full_t5: false, ..full() })[0], 5);
        assert_eq!(all(&Config { borrowed_tables: false, ..full() })[0], 6);
        assert_eq!(all(&Config { tail: false, full_t5: false, borrowed_tables: false, fixed_assets: false, ..full() })[0], 7);
        // The first broken rule wins.
        assert_eq!(all(&Config { scene_access: 2, current: false, ..full() })[0], 1);
    }
    #[test]
    fn current() {
        assert_eq!(all(&Config { request: 2, ..full() })[1], 8);
        assert_eq!(all(&Config { stage: 6, ..full() })[1], 8);
        assert_eq!(all(&Config { selectors: false, ..full() })[1], 8);
        assert_eq!(all(&Config { ia_snapshots: false, ..full() })[1], 8);
        assert_eq!(all(&Config { tables: false, ..full() })[1], 8);
        assert_eq!(all(&Config { stream: false, ..full() })[1], 8);
        assert_eq!(all(&Config { current: false, infix: false, tail: false, full_t5: false, borrowed_tables: false, fixed_assets: false, vrs_refinement: false, stage: 6, ..full() }), [0, 0, 0]);
    }
    #[test]
    fn sources() {
        let none = Config { current: false, infix: false, tail: false, full_t5: false, borrowed_tables: false, fixed_assets: false, vrs_refinement: false, ..full() };
        assert_eq!(all(&none), [0, 0, 0]);
        assert_eq!(all(&Config { request: 2, ..none })[2], 9);
        assert_eq!(all(&Config { tables: false, ..none })[2], 9);
        assert_eq!(all(&Config { cbv_snapshots: false, cbvs: false, ..none })[2], 10);
        assert_eq!(all(&Config { cbv_snapshots: false, details: false, cbvs: false, ..none })[2], 11);
        assert_eq!(all(&Config { cbv_snapshots: false, details: false, tables: false, stream: false, ..none })[2], 12);
        assert_eq!(all(&Config { cbv_snapshots: false, details: false, tables: false, snapshots: false, stream: false, ..none })[2], 13);
        assert_eq!(
            all(&Config { cbv_snapshots: false, details: false, tables: false, snapshots: false, ia_snapshots: false, stream: false, cbvs: false, request: 2, ..none })[2],
            0
        );
    }
}
