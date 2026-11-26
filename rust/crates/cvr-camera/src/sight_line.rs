//! The bore-to-sight-line offset (include/Hooks/SightLine.hpp, `cvr::sight::SightLineOffset`): while a sight is looked
//! through (hitscan from the sight line on, a publication under 250 ms old), the mod's offset re-orthogonalised against
//! the shot's forward (which must be finite and at least half a unit long), kept only when finite and within 30 cm.
//!
//! The Rust build's overlay reads it through `sight_line_offset`; the unit tests below cover each condition.

/// The offset for a shot along `forward`, from the published `offset` at `tick` (0 none), `now` the current tick.
pub fn sight_line_offset(enabled: bool, tick: u64, now: u64, offset: [f32; 3], forward: [f32; 3]) -> Option<[f32; 3]> {
    if !enabled || tick == 0 || now.wrapping_sub(tick) > 250 {
        return None;
    }
    let [mut x, mut y, mut z] = offset;
    let fl2 = forward[0] * forward[0] + forward[1] * forward[1] + forward[2] * forward[2];
    if !fl2.is_finite() || fl2 < 0.25 {
        return None;
    }
    let along = (x * forward[0] + y * forward[1] + z * forward[2]) / fl2;
    x -= along * forward[0];
    y -= along * forward[1];
    z -= along * forward[2];
    let l2 = x * x + y * y + z * z;
    if !l2.is_finite() || l2 > 0.3 * 0.3 {
        return None;
    }
    Some([x, y, z])
}

#[cfg(test)]
mod tests {
    //! Each condition.
    use super::*;

    #[test]
    fn offsets() {
        // The component along the shot is removed.
        assert_eq!(sight_line_offset(true, 100, 200, [0.05, 0.3, 0.02], [0.0, 1.0, 0.0]), Some([0.05, 0.0, 0.02]));
        assert_eq!(sight_line_offset(false, 100, 200, [0.05, 0.0, 0.0], [0.0, 1.0, 0.0]), None);
        assert_eq!(sight_line_offset(true, 0, 200, [0.05, 0.0, 0.0], [0.0, 1.0, 0.0]), None);
        assert_eq!(sight_line_offset(true, 100, 351, [0.05, 0.0, 0.0], [0.0, 1.0, 0.0]), None);
        assert_eq!(sight_line_offset(true, 100, 350, [0.05, 0.0, 0.0], [0.0, 1.0, 0.0]), Some([0.05, 0.0, 0.0]));
        assert_eq!(sight_line_offset(true, 100, 200, [0.05, 0.0, 0.0], [0.0, 0.4, 0.0]), None);
        assert_eq!(sight_line_offset(true, 100, 200, [0.05, 0.0, 0.0], [f32::NAN, 1.0, 0.0]), None);
        // Too far off the bore.
        assert_eq!(sight_line_offset(true, 100, 200, [0.31, 0.0, 0.0], [0.0, 1.0, 0.0]), None);
        assert_eq!(sight_line_offset(true, 100, 200, [0.0, 0.0, f32::INFINITY], [0.0, 1.0, 0.0]), None);
    }
}
