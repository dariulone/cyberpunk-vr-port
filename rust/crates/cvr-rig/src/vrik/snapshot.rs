//! The input end of the pose path (formerly src/Anim/HandSnapshot.cpp): the seqlocked read of the shared hand block
//! the present thread publishes, the view packet latched once per solve, the view's position from it, and the guarded
//! readability probe the readers use before dereferencing an engine pointer. Nothing here decides a bone; it decides
//! what the solve is allowed to believe.
use super::env::*;
use super::*;
use crate::math::{dot3, quat_conj, quat_rotate_vec};
use core::ffi::{c_char, c_void};
use core::sync::atomic::{fence, Ordering};
use cvr_body::cmath::{atan2, cos, sin};

/// The latched whole frame of the shared block (the hook's writes, slots 20..22 and 85..88, stay on the raw block).
pub static mut g_handsStable: [f32; 128] = [0.0; 128];

#[repr(C)]
struct MemoryBasicInformation {
    base_address: *mut c_void,
    allocation_base: *mut c_void,
    allocation_protect: u32,
    partition_id: u16,
    region_size: usize,
    state: u32,
    protect: u32,
    kind: u32,
}
#[link(name = "kernel32")]
extern "system" {
    fn VirtualQuery(address: *const c_void, buffer: *mut MemoryBasicInformation, length: usize) -> usize;
}
const MEM_COMMIT: u32 = 0x1000;
const PAGE_NOACCESS: u32 = 0x01;
const PAGE_GUARD: u32 = 0x100;

/// Page-readable guard: true only if [p, p+n) is committed and readable.
#[no_mangle]
pub unsafe extern "C" fn VRIK_IsReadable(p: *const c_void, n: usize) -> bool {
    if p.is_null() {
        return false;
    }
    let mut mbi: MemoryBasicInformation = core::mem::zeroed();
    if VirtualQuery(p, &mut mbi, core::mem::size_of::<MemoryBasicInformation>()) == 0 {
        return false;
    }
    if mbi.state != MEM_COMMIT {
        return false;
    }
    let prot = mbi.protect & 0xFF;
    if prot == PAGE_NOACCESS || prot == 0 {
        return false;
    }
    if mbi.protect & PAGE_GUARD != 0 {
        return false;
    }
    let start = mbi.base_address as usize;
    let end = start.wrapping_add(mbi.region_size);
    let a = p as usize;
    a.wrapping_add(n) <= end
}

#[inline(always)]
unsafe fn seq_at(slot: usize) -> *const u32 {
    g_pSharedHands.add(slot) as *const u32
}

/// SEQLOCK READER: the present thread brackets each write of the block with an odd/even counter in slot 127; a whole
/// consistent frame of [0..126] is latched into g_handsStable, swapped in only when a new complete one arrives.
#[no_mangle]
pub unsafe extern "C" fn RefreshHandsSnapshot() {
    if g_pSharedHands.is_null() {
        return;
    }
    let seq_slot = seq_at(127);
    // PERF fast path: nothing new since the last latch.
    {
        let s_cur = core::ptr::read_volatile(seq_slot);
        if g_handsStableValid && (s_cur & 1) == 0 && s_cur == g_handsStableSeq {
            return;
        }
    }
    for _ in 0..8 {
        let s0 = core::ptr::read_volatile(seq_slot);
        if s0 & 1 != 0 {
            continue; // write in progress -> retry
        }
        fence(Ordering::Acquire);
        let mut tmp = [0.0f32; 127];
        for i in 0..127 {
            tmp[i] = core::ptr::read_volatile(g_pSharedHands.add(i));
        }
        fence(Ordering::Acquire);
        let s1 = core::ptr::read_volatile(seq_slot);
        if s0 == s1 {
            if !g_handsStableValid || s1 != g_handsStableSeq {
                g_handsStable[..127].copy_from_slice(&tmp);
                g_handsStableSeq = s1;
                g_handsStableValid = true;
            }
            return;
        }
    }
    // Contended out: keep the last good snapshot (never expose a torn frame).
}

/// An unfinished first publication is no more readable than any later one.
#[no_mangle]
pub unsafe extern "C" fn SharedPose(i: i32) -> f32 {
    if g_handsStableValid {
        if let Some(v) = g_handsStable.get(i as usize) {
            return *v;
        }
    }
    0.0
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_HeadResidual(fallbackConsumedRight: f32, fallbackConsumedForward: f32, out: *mut f32) {
    let paired = SharedPose(115) == 2.0;
    *out = SharedPose(124) - (if paired { SharedPose(112) } else { fallbackConsumedRight });
    *out.add(1) = SharedPose(125);
    *out.add(2) = SharedPose(126) + (if paired { SharedPose(113) } else { fallbackConsumedForward });
}

/// The snap-window trace (log\cyberpunkvr_snapwin.log), one line.
fn snap_trace(line: &str) {
    let mut bytes = Vec::with_capacity(line.len() + 1);
    bytes.extend(line.bytes().filter(|&b| b != 0));
    bytes.push(0);
    // SAFETY: a NUL-terminated string the call only reads.
    unsafe { cvr_cpp_vrik_snap_trace(bytes.as_ptr() as *const c_char) }
}

static mut s_lastSnapCtr: f32 = -1.0;
static mut s_snapTraceWin: i32 = 0;

/// VIEW PACKET: [104..111] + [141] latched ONCE per solve under the present thread's seqlock [143], so both arms and
/// the view-position resolver consume one render frame; a snap turn's event rotates the just-latched packet once.
#[no_mangle]
pub unsafe extern "C" fn VRIK_LatchViewPacket() {
    g_viewPktValid = false;
    if g_pSharedHands.is_null() {
        return;
    }
    let sh = g_pSharedHands;
    let seq = seq_at(143);
    for _ in 0..8 {
        let s0 = core::ptr::read_volatile(seq);
        if s0 == 0 {
            return; // writer absent (older dxgi)
        }
        if s0 & 1 != 0 {
            continue; // write in progress
        }
        let r = |i: usize| *sh.add(i);
        let tmp = [
            r(104), r(105), r(106), r(107), r(108), r(109), r(110), r(111), r(141),
            r(68),  // publish stamp, for the age census
            r(218), // the position the frame renders from
            r(219), r(220), r(227), r(228), r(229), r(230),
        ];
        let ok142 = r(142);
        if core::ptr::read_volatile(seq) == s0 {
            g_viewPkt = tmp;
            g_viewPktValid = ok142 == 1.0;
            // SNAP EVENT SYNC: [146] the snap yaw delta, [148] the pre-snap heading, [147] the event counter; the
            // (pre-snap) packet is rotated by the delta once, and [149] acknowledges the event to the present thread.
            if g_viewPktValid && !g_pSharedHands.is_null() {
                let ctr = *sh.add(147);
                if s_lastSnapCtr < 0.0 {
                    s_lastSnapCtr = ctr;
                    *sh.add(149) = ctr; // startup: skip history
                }
                if ctr != s_lastSnapCtr {
                    s_lastSnapCtr = ctr;
                    s_snapTraceWin = 14;
                    *sh.add(149) = ctr;
                    let d = *sh.add(146);
                    let mut pre = g_viewPkt[8] - *sh.add(148);
                    while pre > 3.14159265 {
                        pre -= 6.28318531;
                    }
                    while pre < -3.14159265 {
                        pre += 6.28318531;
                    }
                    let pkt_is_pre_snap = pre > -0.035 && pre < 0.035; // ~2deg
                    if (d > 1e-4 || d < -1e-4) && pkt_is_pre_snap {
                        let s = sin(d * 0.5);
                        let c = cos(d * 0.5);
                        let (x, y, z, w) = (g_viewPkt[0], g_viewPkt[1], g_viewPkt[2], g_viewPkt[3]);
                        // Rz(d) * vq, expanded (unit * unit = unit).
                        let nx = c * x - s * y;
                        let ny = c * y + s * x;
                        let nz = c * z + s * w;
                        let nw = c * w - s * z;
                        g_viewPkt[0] = nx;
                        g_viewPkt[1] = ny;
                        g_viewPkt[2] = nz;
                        g_viewPkt[3] = nw;
                        g_viewPkt[8] += d;
                        // The heading-rotated translation delta must turn too.
                        let vs = sin(d);
                        let vc = cos(d);
                        let (dx, dy) = (g_viewPkt[4], g_viewPkt[5]);
                        let rdx = vc * dx - vs * dy;
                        let rdy = vs * dx + vc * dy;
                        g_viewPkt[4] = rdx;
                        g_viewPkt[5] = rdy;
                        if g_handsStableValid {
                            g_handsStable[104] = nx;
                            g_handsStable[105] = ny;
                            g_handsStable[106] = nz;
                            g_handsStable[107] = nw;
                            g_handsStable[108] = rdx;
                            g_handsStable[109] = rdy;
                        }
                    }
                }
                // SPRINT-DOUBLE TRACE: for ~14 solves after each snap event, what this solve consumes.
                if s_snapTraceWin > 0 {
                    s_snapTraceWin -= 1;
                    let (qi, qj, qk, qr) = (g_VREntityQI, g_VREntityQJ, g_VREntityQK, g_VREntityQR);
                    let ent_yaw = atan2(2.0 * (qr * qk + qi * qj), 1.0 - 2.0 * (qj * qj + qk * qk));
                    snap_trace(&format!(
                        "[hk] ms={} ctr={:.0} pktYaw={:.4} entYaw={:.4} plYaw={:.2} pos=({:.2},{:.2})\n",
                        GetTickCount64(),
                        ctr as f64,
                        g_viewPkt[8] as f64,
                        ent_yaw as f64,
                        g_VRPlayerYaw as f64,
                        g_VREntityPosX as f64,
                        g_VREntityPosY as f64
                    ));
                }
            }
            return;
        }
    }
}

/// The render view's position: the coherent delta from the hands' own head sample (or the packet's) on the paired
/// camera (v2), or the legacy absolute position with its fixed-point scale auto-detected.
#[no_mangle]
pub unsafe extern "C" fn VRIK_ResolveViewPos(out: *mut f32) -> bool {
    g_vrikViewScaleUsed = 0.0;
    let flag = if g_viewPktValid { g_viewPkt[7] } else { SharedPose(111) };
    if flag == 0.0 {
        return false;
    }
    if g_VRCamPosValid == 0 {
        return false;
    }
    let coherent = SharedPose(115) == 1.0;
    let pick = |k: usize| {
        if coherent {
            SharedPose(112 + k as i32)
        } else if g_viewPktValid {
            g_viewPkt[4 + k]
        } else {
            SharedPose(108 + k as i32)
        }
    };
    let v = [pick(0), pick(1), pick(2)];
    if flag == 2.0 {
        if v[0] * v[0] + v[1] * v[1] + v[2] * v[2] > 2.25 {
            return false; // sanity: |delta| < 1.5m
        }
        let (mut bx, mut by, mut bz) = (g_VRCamPosX, g_VRCamPosY, g_VRCamPosZ); // fallback: raw cam
        if g_VRCamPairValid != 0 {
            bx = g_VREntityPosX + g_VRCamPairLocalX;
            by = g_VREntityPosY + g_VRCamPairLocalY;
            bz = g_VREntityPosZ + g_VRCamPairLocalZ;
        }
        *out = bx + v[0];
        *out.add(1) = by + v[1];
        *out.add(2) = bz + v[2];
        g_vrikViewScaleUsed = 2.0;
        return true;
    }
    // Legacy (flag==1): absolute position with fixed-point scale auto-detect (old dxgi).
    for s in 0..2 {
        let k = if s == 0 { 1.0f32 } else { 0.5 };
        let dx = v[0] * k - g_VRCamPosX;
        let dy = v[1] * k - g_VRCamPosY;
        let dz = v[2] * k - g_VRCamPosZ;
        if dx * dx + dy * dy + dz * dz < 4.0 {
            *out = v[0] * k;
            *out.add(1) = v[1] * k;
            *out.add(2) = v[2] * k;
            g_vrikViewScaleUsed = k;
            return true;
        }
    }
    false
}

/// The same paired model origin the torso uses, the view delta added exactly once.
#[no_mangle]
pub unsafe extern "C" fn VRIK_ResolveViewModelPos(pairedCamModel: *const f32, pairedEntityQuat: *const f32, out: *mut f32) -> bool {
    let flag = if g_viewPktValid { g_viewPkt[7] } else { SharedPose(111) };
    if flag != 2.0 {
        return false;
    }
    let coherent = SharedPose(115) == 1.0;
    let mut delta = [0.0f32; 3];
    for k in 0..3 {
        delta[k] = if coherent {
            SharedPose(112 + k as i32)
        } else if g_viewPktValid {
            g_viewPkt[4 + k]
        } else {
            SharedPose(108 + k as i32)
        };
    }
    let norm = dot3(&delta, &delta);
    if !norm.is_finite() || norm > 2.25 {
        return false;
    }
    let inv = quat_conj(&a4(pairedEntityQuat));
    let local = quat_rotate_vec(&inv, &delta);
    for k in 0..3 {
        *out.add(k) = *pairedCamModel.add(k) + local[k];
    }
    true
}

/// A VR-space vector in model space per the selected preset (VR is Y-up).
#[no_mangle]
pub unsafe extern "C" fn VRIK_RemapAxis(preset: i32, v: *const f32, o: *mut f32) {
    // Component by component through the pointers, as the C++: an output that is also the input reads what was
    // already written.
    match preset {
        1 => {
            // Y-up -> Z-up (standard OpenXR)
            *o = *v;
            *o.add(1) = -*v.add(2);
            *o.add(2) = *v.add(1);
        }
        2 => {
            *o = *v;
            *o.add(1) = *v.add(2);
            *o.add(2) = *v.add(1);
        }
        3 => {
            *o = -*v;
            *o.add(1) = -*v.add(2);
            *o.add(2) = *v.add(1);
        }
        4 => {
            *o = *v.add(2);
            *o.add(1) = *v;
            *o.add(2) = *v.add(1);
        }
        5 => {
            *o = -*v.add(2);
            *o.add(1) = -*v;
            *o.add(2) = *v.add(1);
        }
        _ => {
            // identity
            *o = *v;
            *o.add(1) = *v.add(1);
            *o.add(2) = *v.add(2);
        }
    }
}
