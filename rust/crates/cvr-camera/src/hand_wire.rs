//! The hand locator's wire model (src/Overlay/OverlayDebugDraw.cpp, `DrawHandLocatorOverlay`'s `drawHandWire`): the
//! abstract hand (+X thumb side, +Y fingers, +Z palm normal) mapped onto the runtime's grip basis (thumb up -Z,
//! fingers forward +Y, palm +X for the right hand and -X for the left) -- the wrist and palm outline, four two-bone
//! fingers, a two-bone thumb, the three axes and the forward arrow head, in the .cpp's order, sizes scaled; and the
//! axis end labels R, L, U, D and F with their pixel offsets.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_hl_wire` and `cvr_hl_labels` (the projection and the
//! drawing stay the .cpp's); the unit tests below cover them.

/// One bone of the wire, in hand-local coordinates after the grip mapping.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Segment {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub color: u32,
    pub thickness: f32,
}
const _: () = assert!(core::mem::size_of::<Segment>() == 32);
/// One axis end label (abstract coordinates, as the .cpp projects them unmapped).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Label {
    pub at: [f32; 3],
    pub offset: [f32; 2],
    pub text: u8,
}
const _: () = assert!(core::mem::size_of::<Label>() == 24);

/// IM_COL32 (ImGui's RGBA packing, red lowest).
const fn col32(r: u32, g: u32, b: u32, a: u32) -> u32 {
    (a << 24) | (b << 16) | (g << 8) | r
}
const RIGHT: u32 = col32(255, 80, 80, 255);
const UP: u32 = col32(80, 255, 80, 255);
const FORWARD: u32 = col32(80, 160, 255, 255);

/// mapHandPoint.
pub fn map_point(left: bool, h: [f32; 3]) -> [f32; 3] {
    [if left { -h[2] } else { h[2] }, -h[1], -h[0]]
}
/// The wire's bones.
pub fn wire(s: f32, left: bool, body: u32) -> Vec<Segment> {
    let wrist_half_w = 0.026 * s;
    let palm_half_w = 0.034 * s;
    let wrist_y = -0.055 * s;
    let palm_mid_y = -0.018 * s;
    let knuckle_y = 0.045 * s;
    let fwd_len = 0.180 * s;
    let mut out = Vec::with_capacity(23);
    let mut bone = |a: [f32; 3], b: [f32; 3], color: u32, thickness: f32| out.push(Segment { a: map_point(left, a), b: map_point(left, b), color, thickness });
    // The wrist and palm outline.
    bone([-wrist_half_w, wrist_y, 0.0], [wrist_half_w, wrist_y, 0.0], body, 2.0);
    bone([-wrist_half_w, wrist_y, 0.0], [-palm_half_w, palm_mid_y, 0.0], body, 2.0);
    bone([wrist_half_w, wrist_y, 0.0], [palm_half_w, palm_mid_y, 0.0], body, 2.0);
    bone([-palm_half_w, palm_mid_y, 0.0], [-palm_half_w, knuckle_y, 0.0], body, 2.0);
    bone([palm_half_w, palm_mid_y, 0.0], [palm_half_w, knuckle_y, 0.0], body, 2.0);
    bone([-palm_half_w, knuckle_y, 0.0], [palm_half_w, knuckle_y, 0.0], body, 2.0);
    // The fingers: pinky, ring, middle, index.
    for (base_x, mid_x, mid_y, tip_x, tip_y) in [
        (-0.024 * s, -0.026 * s, 0.074 * s, -0.026 * s, 0.096 * s),
        (-0.010 * s, -0.011 * s, 0.086 * s, -0.011 * s, 0.116 * s),
        (0.006 * s, 0.006 * s, 0.094 * s, 0.006 * s, 0.128 * s),
        (0.022 * s, 0.023 * s, 0.086 * s, 0.023 * s, 0.116 * s),
    ] {
        bone([base_x, knuckle_y, 0.0], [mid_x, mid_y, -0.010 * s], body, 2.0);
        bone([mid_x, mid_y, -0.010 * s], [tip_x, tip_y, -0.020 * s], body, 2.0);
    }
    // The thumb (always +X in abstract hand space).
    bone([0.020 * s, -0.004 * s, 0.0], [0.046 * s, 0.014 * s, -0.006 * s], body, 2.0);
    bone([0.046 * s, 0.014 * s, -0.006 * s], [0.065 * s, 0.035 * s, -0.016 * s], body, 2.0);
    // The axes.
    bone([-0.060 * s, 0.0, 0.0], [0.060 * s, 0.0, 0.0], RIGHT, 2.0);
    bone([0.0, -0.060 * s, 0.0], [0.0, 0.060 * s, 0.0], UP, 2.0);
    bone([0.0, 0.0, 0.0], [0.0, 0.0, -fwd_len], FORWARD, 3.0);
    // The forward arrow head.
    let back = -(fwd_len - 0.028 * s);
    bone([0.0, 0.0, -fwd_len], [0.020 * s, 0.0, back], FORWARD, 2.0);
    bone([0.0, 0.0, -fwd_len], [-0.020 * s, 0.0, back], FORWARD, 2.0);
    bone([0.0, 0.0, -fwd_len], [0.0, 0.020 * s, back], FORWARD, 2.0);
    bone([0.0, 0.0, -fwd_len], [0.0, -0.020 * s, back], FORWARD, 2.0);
    out
}
/// The axis end labels.
pub fn labels(s: f32) -> [Label; 5] {
    let fwd_len = 0.180 * s;
    let label = |at: [f32; 3], offset: [f32; 2], text: u8| Label { at, offset, text };
    [
        label([0.070 * s, 0.0, 0.0], [4.0, -6.0], b'R'),
        label([-0.070 * s, 0.0, 0.0], [-10.0, -6.0], b'L'),
        label([0.0, 0.070 * s, 0.0], [-4.0, -14.0], b'U'),
        label([0.0, -0.070 * s, 0.0], [-4.0, 2.0], b'D'),
        label([0.0, 0.0, -(fwd_len + 0.020 * s)], [4.0, -6.0], b'F'),
    ]
}

// ---- the C ABI

/// # Safety
/// `out` holds `capacity` segments. Returns how many were written (all of them, when they fit; else 0).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_hl_wire(scale: f32, left: bool, body: u32, out: *mut Segment, capacity: usize) -> usize {
    let segments = wire(scale, left, body);
    if out.is_null() || segments.len() > capacity {
        return 0;
    }
    unsafe { core::ptr::copy_nonoverlapping(segments.as_ptr(), out, segments.len()) };
    segments.len()
}
/// # Safety
/// `out` holds five labels.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_hl_labels(scale: f32, out: *mut [Label; 5]) {
    unsafe { *out = labels(scale) };
}

#[cfg(test)]
mod tests {
    //! The model.
    use super::*;

    #[test]
    fn mapping() {
        assert_eq!(map_point(false, [1.0, 2.0, 3.0]), [3.0, -2.0, -1.0]);
        assert_eq!(map_point(true, [1.0, 2.0, 3.0]), [-3.0, -2.0, -1.0]);
    }
    #[test]
    fn wires() {
        let body = col32(0, 220, 255, 255);
        let w = wire(1.0, false, body);
        assert_eq!(w.len(), 23);
        // The first bone: the wrist line, mapped.
        assert_eq!(w[0], Segment { a: map_point(false, [-0.026, -0.055, 0.0]), b: map_point(false, [0.026, -0.055, 0.0]), color: body, thickness: 2.0 });
        // The pinky's second bone, the thumb's tip and the forward axis.
        assert_eq!(w[7].b, map_point(false, [-0.026, 0.096, -0.020]));
        assert_eq!(w[15].b, map_point(false, [0.065, 0.035, -0.016]));
        assert_eq!((w[18].b, w[18].color, w[18].thickness), (map_point(false, [0.0, 0.0, -0.180]), FORWARD, 3.0));
        assert_eq!(w[22].b, map_point(false, [0.0, -0.020, -(0.180 - 0.028)]));
        // The left hand mirrors only the palm axis; the scale scales everything.
        let l = wire(1.0, true, body);
        assert!(w.iter().zip(&l).all(|(r, l)| r.a[0] == -l.a[0] && r.a[1] == l.a[1] && r.a[2] == l.a[2]));
        let big = wire(2.0, false, body);
        assert_eq!(big[0].a, map_point(false, [-0.026 * 2.0, -0.055 * 2.0, 0.0]));
        assert_eq!([w[16].color, w[17].color], [RIGHT, UP]);
    }
    #[test]
    fn label_ends() {
        let l = labels(1.0);
        assert_eq!(l.map(|l| l.text), *b"RLUDF");
        assert_eq!(l[4].at, [0.0, 0.0, -(0.180 + 0.020)]);
        assert_eq!(l[1].offset, [-10.0, -6.0]);
        let mut out = [Segment::default(); 22];
        assert_eq!(unsafe { cvr_hl_wire(1.0, false, 0, out.as_mut_ptr(), out.len()) }, 0);
    }
}
