//! main's src/Anim/BladeMeshBlob.cpp: a private copy of a skinned mesh's render blob cut and painted where the blade
//! met it -- each chunk's quantized vertices decoded in the struck bone's bind frame, the triangles of the bone's skin
//! region split against each mark's volume (new vertices blended from their faces, skin weights included), the faces
//! a mark covers moved to a painted chunk of their own, the vertex and index buffers rebuilt.
//!
//! The blob's bytes and layouts come in plain (the engine's structs are read by the caller); what changed comes back:
//! the new buffer, each chunk's new counts and offsets, its painted twin, and where the contact landed.
use crate::blade_mesh_cut::*;
use std::collections::HashMap;

/// A vertex element (GpuWrapApiVertexPackingPackingElement): its packing type and usage, the usage's index, its
/// stream, the stream's kind.
#[derive(Clone, Copy, Debug)]
pub struct Element {
    pub ty: i8,
    pub usage: i8,
    pub usage_index: u8,
    pub stream_index: u8,
    pub stream_type: i8,
}
/// A render chunk (rendChunk): its elements (to the first invalid), the slots' strides, the streams' byte offsets,
/// the index type and offset, its counts and LOD mask.
#[derive(Clone, Debug, Default)]
pub struct ChunkInfo {
    pub elements: Vec<Element>,
    pub slot_strides: Vec<u8>,
    pub byte_offsets: Vec<u32>,
    pub index_type: i8,
    pub index_offset: u32,
    pub num_vertices: u32,
    pub num_indices: u32,
    pub lod_mask: u8,
}
/// The blob's header numbers.
#[derive(Clone, Copy, Debug, Default)]
pub struct Header {
    pub vertex_buffer_size: u32,
    pub index_buffer_size: u32,
    pub index_buffer_offset: u32,
    pub scale: [f32; 4],
    pub offset: [f32; 4],
}
/// A bone's bind matrix (rows X, Y, Z, W).
pub type Matrix = [[f32; 4]; 4];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    None = 0,
    Slash = 1,
    Puncture = 2,
    Trail = 3,
}
/// One mark to make: its volume, the bone, the blade's sweep (four endpoints), the mark, the chunks visible when it
/// was made, the capsule shell's depth, the palette entries of the bone's skin region.
#[derive(Clone, Debug)]
pub struct Edit {
    pub volume: Volume,
    pub bone: i32,
    pub swept: bool,
    pub rays: [Point; 4],
    pub mark: Mark,
    pub visible_chunks: u64,
    pub shell_depth: f64,
    pub skin_region: [bool; 256],
}
/// What the cut did, and where the contact landed (the surviving painted triangle nearest the mark's centre).
#[derive(Clone, Debug)]
pub struct Counts {
    pub split: u32,
    pub inserted: u32,
    pub indices_before: u32,
    pub indices_after: u32,
    pub ink_triangles: u32,
    pub ink_chunks: Vec<(u32, u32)>,
    pub contact: Point,
    pub normal: Point,
    pub contact_edit: i32,
    pub contact_chunk: u32,
    pub contact_ink_chunk: u32,
    pub contact_lod: u32,
    pub entry_distance_squared: f64,
    pub projected_from_shell: bool,
    pub no_overlap: bool,
}
impl Default for Counts {
    fn default() -> Self {
        Counts {
            split: 0,
            inserted: 0,
            indices_before: 0,
            indices_after: 0,
            ink_triangles: 0,
            ink_chunks: Vec::new(),
            contact: [0.0; 3],
            normal: [0.0; 3],
            contact_edit: -1,
            contact_chunk: 0,
            contact_ink_chunk: 0,
            contact_lod: 256,
            entry_distance_squared: 0.0,
            projected_from_shell: false,
            no_overlap: false,
        }
    }
}
/// A changed chunk: its new vertex count, its index count and offset (unpainted), its painted twin's, the streams'
/// new starts.
#[derive(Clone, Copy, Debug, Default)]
pub struct Update {
    pub index: u32,
    pub count: u32,
    pub indices: u32,
    pub offset: u32,
    pub ink_count: u32,
    pub ink_offset: u32,
    pub starts: [u32; 5],
}
/// The new buffer (vertices, aligned to 1 KiB, then indices) and its layout.
#[derive(Clone, Debug, Default)]
pub struct Output {
    pub buffer: Vec<u8>,
    pub vertex_bytes: u32,
    pub index_start: u32,
    pub index_size: u32,
    pub updates: Vec<Update>,
}

// ePackingType / ePackingUsage / EStreamType
const PT_INVALID: i8 = -1;
const PT_FLOAT16_2: i8 = 4;
const PT_FLOAT16_4: i8 = 5;
const PT_SHORT4N: i8 = 13;
const PT_COLOR: i8 = 22;
const PT_UBYTE4: i8 = 25;
const PT_UBYTE4N: i8 = 26;
const PT_DEC4: i8 = 28;
const PS_POSITION: i8 = 1;
const PS_NORMAL: i8 = 2;
const PS_SKIN_INDICES: i8 = 7;
const PS_SKIN_WEIGHTS: i8 = 8;
const ST_PER_VERTEX: i8 = 0;
const ST_PER_INSTANCE: i8 = 1;
const IBCT_INDEX_USHORT: i8 = 1;

#[derive(Clone, Copy)]
struct Attr {
    usage: i8,
    ty: i8,
    stream: usize,
    offset: usize,
    size: usize,
}
#[derive(Clone, Default)]
struct Layout {
    stride: [usize; 5],
    attrs: Vec<Attr>,
    position: i32,
    shading_normal: i32,
    bones: [i32; 2],
    weights: [i32; 2],
}
fn width(ty: i8) -> usize {
    match ty {
        PT_SHORT4N | PT_FLOAT16_4 => 8,
        PT_UBYTE4 | PT_UBYTE4N | PT_DEC4 | PT_COLOR | PT_FLOAT16_2 => 4,
        _ => 0,
    }
}
fn read_layout(chunk: &ChunkInfo) -> Option<Layout> {
    let mut l = Layout { position: -1, shading_normal: -1, bones: [-1, -1], weights: [-1, -1], ..Layout::default() };
    for e in &chunk.elements {
        if e.ty == PT_INVALID {
            break;
        }
        if e.stream_type == ST_PER_INSTANCE {
            continue;
        }
        let (stream, size) = (e.stream_index as usize, width(e.ty));
        if e.stream_type != ST_PER_VERTEX || stream >= 5 || size == 0 {
            return None;
        }
        let index = l.attrs.len() as i32;
        if e.usage == PS_NORMAL && e.ty == PT_DEC4 {
            l.shading_normal = index;
        }
        if e.usage == PS_POSITION {
            if e.ty != PT_SHORT4N || l.position >= 0 {
                return None;
            }
            l.position = index;
        } else if e.usage == PS_SKIN_INDICES || e.usage == PS_SKIN_WEIGHTS {
            if e.usage_index > 1 {
                return None;
            }
            let ids = e.usage == PS_SKIN_INDICES;
            if e.ty != if ids { PT_UBYTE4 } else { PT_UBYTE4N } {
                return None;
            }
            if ids {
                l.bones[e.usage_index as usize] = index;
            } else {
                l.weights[e.usage_index as usize] = index;
            }
        } else if e.ty != PT_FLOAT16_2 && e.ty != PT_FLOAT16_4 && e.ty != PT_DEC4 && e.ty != PT_COLOR {
            return None;
        }
        l.attrs.push(Attr { usage: e.usage, ty: e.ty, stream, offset: l.stride[stream], size });
        l.stride[stream] += size;
    }
    for stream in 0..5 {
        if l.stride[stream] != 0 && (stream >= chunk.slot_strides.len() || l.stride[stream] != chunk.slot_strides[stream] as usize || stream >= chunk.byte_offsets.len()) {
            return None;
        }
    }
    (l.position >= 0 && l.bones[0] >= 0 && l.weights[0] >= 0 && ((l.bones[1] >= 0) == (l.weights[1] >= 0))).then_some(l)
}
fn within(start: usize, length: usize, size: usize) -> bool {
    start <= size && length <= size - start
}
fn align(bytes: &mut Vec<u8>, a: usize) {
    let n = (bytes.len() + a - 1) & !(a - 1);
    bytes.resize(n, 0);
}
fn rd16(d: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([d[at], d[at + 1]])
}
fn rdi16(d: &[u8], at: usize) -> i16 {
    i16::from_le_bytes([d[at], d[at + 1]])
}
fn rd32(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]])
}
fn apply(p: &Point, m: &Matrix) -> Point {
    let f = |r: usize, c: usize| f64::from(m[r][c]);
    [
        f(3, 0) + p[0] * f(0, 0) + p[1] * f(1, 0) + p[2] * f(2, 0),
        f(3, 1) + p[0] * f(0, 1) + p[1] * f(1, 1) + p[2] * f(2, 1),
        f(3, 2) + p[0] * f(0, 2) + p[1] * f(1, 2) + p[2] * f(2, 2),
    ]
}
fn decode_point(d: &[u8], at: usize, h: &Header, bind: &Matrix) -> Point {
    let (s, o) = (h.scale, h.offset);
    let q = |i: usize| f64::from(rdi16(d, at + 2 * i));
    apply(&[q(0) * f64::from(s[0]) / 32767.0 + f64::from(o[0]), q(1) * f64::from(s[1]) / 32767.0 + f64::from(o[1]), q(2) * f64::from(s[2]) / 32767.0 + f64::from(o[2])], bind)
}
fn decode_normal(d: &[u8], at: usize, bind: &Matrix) -> Point {
    let packed = rd32(d, at);
    let n = [0, 1, 2].map(|k| 2.0 * f64::from((packed >> (10 * k)) & 1023) - 1023.0);
    let f = |r: usize, c: usize| f64::from(bind[r][c]);
    [n[0] * f(0, 0) + n[1] * f(1, 0) + n[2] * f(2, 0), n[0] * f(0, 1) + n[1] * f(1, 1) + n[2] * f(2, 1), n[0] * f(0, 2) + n[1] * f(1, 2) + n[2] * f(2, 2)]
}
/// IEEE 754 binary16 to f32 (XMConvertHalfToFloat).
pub fn half_to_float(h: u16) -> f32 {
    let sign = u32::from(h >> 15) << 31;
    let exp = u32::from((h >> 10) & 0x1F);
    let man = u32::from(h & 0x3FF);
    let bits = if exp == 0 {
        if man == 0 {
            sign
        } else {
            // a subnormal: normalised into f32
            let mut e = 127 - 15 + 1;
            let mut m = man;
            while m & 0x400 == 0 {
                m <<= 1;
                e -= 1;
            }
            sign | ((e as u32) << 23) | ((m & 0x3FF) << 13)
        }
    } else if exp == 31 {
        sign | 0x7F80_0000 | (man << 13)
    } else {
        sign | ((exp + 127 - 15) << 23) | (man << 13)
    };
    f32::from_bits(bits)
}
/// f32 to IEEE 754 binary16, round to nearest even (XMConvertFloatToHalf).
pub fn float_to_half(f: f32) -> u16 {
    let x = f.to_bits();
    let sign = ((x >> 16) & 0x8000) as u16;
    let exp = ((x >> 23) & 0xFF) as i32;
    let man = x & 0x7F_FFFF;
    if exp == 255 {
        return sign | 0x7C00 | if man != 0 { 0x200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 31 {
        return sign | 0x7C00;
    }
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        let m = man | 0x80_0000;
        let shift = (14 - e) as u32;
        let half = m >> shift;
        let rem = m & ((1 << shift) - 1);
        let mid = 1 << (shift - 1);
        let rounded = if rem > mid || (rem == mid && half & 1 != 0) { half + 1 } else { half };
        return sign | rounded as u16;
    }
    let half = ((e as u32) << 10) | (man >> 13);
    let rem = man & 0x1FFF;
    let rounded = if rem > 0x1000 || (rem == 0x1000 && half & 1 != 0) { half + 1 } else { half };
    sign | rounded as u16
}

struct StreamSet {
    layout: Layout,
    streams: [Vec<u8>; 5],
    count: u32,
}
impl StreamSet {
    fn field(&self, field: i32, vertex: u32) -> usize {
        let a = self.layout.attrs[field as usize];
        vertex as usize * self.layout.stride[a.stream] + a.offset
    }
    fn stream_of(&self, field: i32) -> usize {
        self.layout.attrs[field as usize].stream
    }
    /// Blend: the vertex at the barycentric point (an existing corner, else a new one, its every attribute and its
    /// skin weights blended; 65535 at most).
    fn blend(&mut self, tri: &[u32; 3], bary: &Point, output: &mut u32) -> bool {
        for i in 0..3 {
            if bary[i] >= 1.0 - 1e-10 {
                *output = tri[i];
                return true;
            }
        }
        if self.count >= 65535 {
            return false;
        }
        *output = self.count;
        self.count += 1;
        for s in 0..5 {
            let n = self.count as usize * self.layout.stride[s];
            self.streams[s].resize(n, 0);
        }
        for field in 0..self.layout.attrs.len() {
            let a = self.layout.attrs[field];
            if a.usage == PS_SKIN_INDICES || a.usage == PS_SKIN_WEIGHTS {
                continue;
            }
            let dst = *output as usize * self.layout.stride[a.stream] + a.offset;
            let half = a.ty == PT_FLOAT16_2 || a.ty == PT_FLOAT16_4;
            let packed = a.ty == PT_DEC4;
            let position = a.usage == PS_POSITION;
            let mut bits = 0u32;
            let channels = if half { a.size / 2 } else { 4 };
            for channel in 0..channels {
                let mut value = 0.0f64;
                for (i, &v) in tri.iter().enumerate() {
                    let src = v as usize * self.layout.stride[a.stream] + a.offset;
                    let d = &self.streams[a.stream];
                    let shift = 10 * channel as u32;
                    let term = if half {
                        f64::from(half_to_float(rd16(d, src + channel * 2)))
                    } else if packed {
                        f64::from((rd32(d, src) >> shift) & if channel == 3 { 3 } else { 1023 })
                    } else if position {
                        f64::from(rdi16(d, src + channel * 2))
                    } else {
                        f64::from(d[src + channel])
                    };
                    value += term * bary[i];
                }
                if !value.is_finite() {
                    return false;
                }
                let d = &mut self.streams[a.stream];
                if half {
                    d[dst + channel * 2..dst + channel * 2 + 2].copy_from_slice(&float_to_half(value as f32).to_le_bytes());
                } else if position {
                    d[dst + channel * 2..dst + channel * 2 + 2].copy_from_slice(&(value.round() as i64 as i16).to_le_bytes());
                } else if packed {
                    bits |= (value.round() as i64 as u32) << (10 * channel as u32);
                } else {
                    d[dst + channel] = value.round() as i64 as u8;
                }
            }
            if packed {
                self.streams[a.stream][dst..dst + 4].copy_from_slice(&bits.to_le_bytes());
            }
        }
        let mut totals = [0f64; 256];
        let banks = if self.layout.bones[1] >= 0 { 2 } else { 1 };
        for (vi, &v) in tri.iter().enumerate() {
            for bank in 0..banks {
                let ids = self.field(self.layout.bones[bank], v);
                let ws = self.field(self.layout.weights[bank], v);
                let (is, wsr) = (self.stream_of(self.layout.bones[bank]), self.stream_of(self.layout.weights[bank]));
                for ch in 0..4 {
                    totals[self.streams[is][ids + ch] as usize] += bary[vi] * f64::from(self.streams[wsr][ws + ch]);
                }
            }
        }
        let mut bones = [0u32; 8];
        let mut weights = [0f64; 8];
        let mut total = 0.0;
        for i in 0..4 * banks {
            let (best, &w) = totals.iter().enumerate().fold((0, &totals[0]), |acc, (j, x)| if *x > *acc.1 { (j, x) } else { acc });
            bones[i] = best as u32;
            weights[i] = w;
            total += w;
            totals[best] = 0.0;
        }
        if total <= 0.0 {
            return false;
        }
        let (mut allocated, mut sum) = (0i64, 0.0f64);
        for i in 0..4 * banks {
            sum += weights[i];
            let next = (255.0 * sum / total).round() as i64;
            let ids = self.field(self.layout.bones[i / 4], *output);
            let ws = self.field(self.layout.weights[i / 4], *output);
            let (is, wsr) = (self.stream_of(self.layout.bones[i / 4]), self.stream_of(self.layout.weights[i / 4]));
            self.streams[is][ids + i % 4] = bones[i] as u8;
            self.streams[wsr][ws + i % 4] = (next - allocated) as u8;
            allocated = next;
        }
        true
    }
}
#[derive(Clone, Copy)]
struct Projection {
    distance: [f64; 2],
    fraction: [f64; 3],
    normal: Triangle,
    stamp: SurfaceHit,
    blend_hit: SurfaceHit,
    blend_chunk: u32,
}
impl Default for Projection {
    fn default() -> Self {
        Projection { distance: [1.0, 1.0], fraction: [2.0, 2.0, 2.0], normal: [[0.0; 3]; 3], stamp: SurfaceHit::default(), blend_hit: SurfaceHit::default(), blend_chunk: 0 }
    }
}
fn endpoint(v: &Volume, side: u32) -> Point {
    let mut p = v.center;
    for axis in 0..3 {
        p[axis] += v.axes[0][axis] * if side != 0 { 1.0 } else { -1.0 } * v.extent[0] + v.axes[2][axis] * (v.extent[2] - v.outside);
    }
    p
}
fn chunk_ok(chunk: &ChunkInfo, layout: &Layout, h: &Header) -> bool {
    chunk.index_type == IBCT_INDEX_USHORT && chunk.num_indices % 3 == 0 && within(chunk.index_offset as usize, chunk.num_indices as usize * 2, h.index_buffer_size as usize) && {
        (0..5).all(|s| layout.stride[s] == 0 || within(chunk.byte_offsets[s] as usize, chunk.num_vertices as usize * layout.stride[s], h.vertex_buffer_size as usize))
    }
}

/// CutPrivateBlob: the marks cut into the blob (at most 12); None when the blob is not one it can change or no
/// chunk overlapped (counts.no_overlap then).
pub fn cut_private_blob(data: &[u8], h: &Header, chunks: &[ChunkInfo], bones: &[Matrix], edits: &[Edit], counts: &mut Counts) -> Option<Output> {
    const LIMIT: usize = 32 * 1024 * 1024;
    if edits.is_empty() || edits.len() > 12 || data.len() > LIMIT {
        return None;
    }
    let (ibo, ibs, vbs) = (h.index_buffer_offset as usize, h.index_buffer_size as usize, h.vertex_buffer_size as usize);
    if !within(ibo, ibs, data.len()) || vbs > ibo || ibo + ibs != data.len() {
        return None;
    }
    let mut projections = vec![[Projection::default(); 8]; edits.len()];
    for (part, chunk) in chunks.iter().enumerate() {
        let layout = read_layout(chunk)?;
        if !chunk_ok(chunk, &layout, h) {
            return None;
        }
        let field = |attr: i32, vertex: u32| {
            let a = layout.attrs[attr as usize];
            chunk.byte_offsets[a.stream] as usize + vertex as usize * layout.stride[a.stream] + a.offset
        };
        for (ei, edit) in edits.iter().enumerate() {
            if edit.mark != Mark::None && (part >= 64 || edit.visible_chunks & (1u64 << part) == 0) {
                continue;
            }
            if edit.bone < 0 || edit.bone as usize >= bones.len() {
                return None;
            }
            let bind = &bones[edit.bone as usize];
            let n = chunk.num_vertices as usize;
            let mut points = vec![[0.0; 3]; n];
            let mut weights = vec![0u32; n];
            let stamp = edit.mark == Mark::Slash || edit.mark == Mark::Puncture;
            let region_weight = if edit.swept { 16 } else { 90 };
            let mut normals = if stamp && layout.shading_normal >= 0 { vec![[0.0; 3]; n] } else { Vec::new() };
            for v in 0..n as u32 {
                for bank in 0..2 {
                    if layout.bones[bank] < 0 {
                        break;
                    }
                    let (ids, values) = (field(layout.bones[bank], v), field(layout.weights[bank], v));
                    for ch in 0..4 {
                        if edit.skin_region[data[ids + ch] as usize] {
                            weights[v as usize] += u32::from(data[values + ch]);
                        }
                    }
                }
                if stamp || weights[v as usize] >= region_weight {
                    points[v as usize] = decode_point(data, field(layout.position, v), h, bind);
                    if !normals.is_empty() {
                        normals[v as usize] = decode_normal(data, field(layout.shading_normal, v), bind);
                    }
                }
            }
            let ends = [endpoint(&edit.volume, 0), endpoint(&edit.volume, 1)];
            let seed = [0, 1, 2].map(|k| (ends[0][k] + ends[1][k]) * 0.5);
            let rays = sample_sweep(&edit.rays);
            for index in (0..chunk.num_indices as usize).step_by(3) {
                let tri = ibo + chunk.index_offset as usize + 2 * index;
                let ids = [rd16(data, tri) as usize, rd16(data, tri + 2) as usize, rd16(data, tri + 4) as usize];
                if ids.iter().any(|&i| i >= n) {
                    return None;
                }
                let weight = weights[ids[0]].min(weights[ids[1]]).min(weights[ids[2]]);
                let blended = weight < region_weight;
                if blended && (!stamp || (weights[ids[0]] == 0 && weights[ids[1]] == 0 && weights[ids[2]] == 0)) {
                    continue;
                }
                let face = [points[ids[0]], points[ids[1]], points[ids[2]]];
                let mut outward = [0.0; 3];
                if !normals.is_empty() {
                    for k in 0..3 {
                        outward[k] = normals[ids[0]][k] + normals[ids[1]][k] + normals[ids[2]][k];
                    }
                }
                for lod in 0..8 {
                    if chunk.lod_mask & (1 << lod) == 0 {
                        continue;
                    }
                    let m = &mut projections[ei][lod];
                    let shading = (!normals.is_empty()).then_some(&outward);
                    if stamp {
                        if !blended {
                            locate_stamp(&rays, &seed, &edit.volume.axes[2], &face, &mut m.stamp, edit.shell_depth, shading, None);
                        } else if !m.stamp.found {
                            let skin = [f64::from(weights[ids[0]]), f64::from(weights[ids[1]]), f64::from(weights[ids[2]])];
                            let mut candidate = SurfaceHit::default();
                            locate_stamp(&rays, &seed, &edit.volume.axes[2], &face, &mut candidate, edit.shell_depth, shading, Some(&skin));
                            if candidate.exact && (!m.blend_hit.found || candidate.seed_distance_squared < m.blend_hit.seed_distance_squared) {
                                m.blend_hit = candidate;
                                m.blend_chunk = part as u32;
                            }
                        }
                    } else if edit.swept {
                        for sample in 0..3 {
                            segment_hit(&rays[sample][0], &rays[sample][1], &face, &mut m.fraction[sample], &mut m.normal[sample]);
                        }
                    } else {
                        for side in 0..2 {
                            let mut bary = [0.0; 3];
                            project_surface(&ends[side], &edit.volume.axes[2], &face, &mut m.distance[side], &mut bary);
                        }
                    }
                }
            }
        }
    }
    let mut vertices: Vec<u8> = data[..ibo].to_vec();
    let mut indices: Vec<u8> = data[ibo..].to_vec();
    let mut updates = Vec::new();
    let mut stats = Counts::default();
    let mut best_ink_gap = f64::INFINITY;
    for (part, chunk) in chunks.iter().enumerate() {
        let layout = read_layout(chunk)?;
        if !chunk_ok(chunk, &layout, h) {
            return None;
        }
        let mut set = StreamSet { layout, streams: Default::default(), count: chunk.num_vertices };
        for s in 0..5 {
            let stride = set.layout.stride[s];
            if stride != 0 {
                let (start, size) = (chunk.byte_offsets[s] as usize, set.count as usize * stride);
                if !within(start, size, vbs) {
                    return None;
                }
                set.streams[s] = data[start..start + size].to_vec();
            }
        }
        let mut triangles: Vec<u32> = (0..chunk.num_indices as usize).map(|i| u32::from(rd16(data, ibo + chunk.index_offset as usize + 2 * i))).collect();
        if triangles.iter().any(|&t| t >= set.count) {
            return None;
        }
        let mut ink_faces = vec![0u8; triangles.len() / 3];
        let mut changed = false;
        let mut ink_centers = vec![[0.0; 3]; edits.len()];
        let mut ink_normals = vec![[0.0; 3]; edits.len()];
        let mut shell_projections = vec![false; edits.len()];
        for (ei, edit) in edits.iter().enumerate() {
            if edit.mark != Mark::None && (part >= 64 || edit.visible_chunks & (1u64 << part) == 0) {
                continue;
            }
            let Some(nearest) = (0..8).find(|lod| chunk.lod_mask & (1 << lod) != 0).map(|lod| projections[ei][lod]) else { continue };
            let mut projected = Cut {
                from: endpoint(&edit.volume, 0),
                to: endpoint(&edit.volume, 1),
                normal: edit.volume.axes[2],
                half_width: edit.volume.extent[1],
                depth: 2.0 * edit.volume.extent[2] - edit.volume.outside,
                outside: edit.volume.outside,
            };
            let stamp = edit.mark == Mark::Slash || edit.mark == Mark::Puncture;
            let blend_boundary = stamp && !nearest.stamp.found;
            let stamp_hit = if blend_boundary { nearest.blend_hit } else { nearest.stamp };
            if stamp {
                if !stamp_hit.found || (blend_boundary && nearest.blend_chunk != part as u32) {
                    continue;
                }
                projected.normal = stamp_hit.normal;
                let mut tangent = edit.volume.axes[0];
                let d: f64 = (0..3).map(|i| tangent[i] * projected.normal[i]).sum();
                for i in 0..3 {
                    tangent[i] -= d * projected.normal[i];
                }
                let mut length = (tangent[0] * tangent[0] + tangent[1] * tangent[1] + tangent[2] * tangent[2]).sqrt();
                if length < 0.01 {
                    let n = projected.normal;
                    tangent = if n[2].abs() < 0.8 { [-n[1], n[0], 0.0] } else { [0.0, -n[2], n[1]] };
                    length = (tangent[0] * tangent[0] + tangent[1] * tangent[1] + tangent[2] * tangent[2]).sqrt();
                }
                let half_length = if edit.mark == Mark::Puncture { 0.008 } else { 0.028 };
                projected.half_width = if edit.mark == Mark::Puncture { 0.005 } else { 0.0025 };
                projected.depth = 0.004;
                projected.outside = 0.004;
                for i in 0..3 {
                    projected.from[i] = stamp_hit.point[i] - half_length * tangent[i] / length;
                    projected.to[i] = stamp_hit.point[i] + half_length * tangent[i] / length;
                }
            } else if edit.swept {
                if !resolve_sweep(&sample_sweep(&edit.rays), &nearest.fraction, &nearest.normal, &mut projected) {
                    continue;
                }
            } else {
                if nearest.distance[0] == 1.0 || nearest.distance[1] == 1.0 || (nearest.distance[0] - nearest.distance[1]).abs() > 2.0 * edit.volume.extent[0] {
                    continue;
                }
                for axis in 0..3 {
                    projected.from[axis] += projected.normal[axis] * nearest.distance[0];
                    projected.to[axis] += projected.normal[axis] * nearest.distance[1];
                }
            }
            let mut volume = Volume::default();
            if !make_volume(&projected, &mut volume) {
                continue;
            }
            for k in 0..3 {
                ink_centers[ei][k] = (projected.from[k] + projected.to[k]) * 0.5;
            }
            ink_normals[ei] = projected.normal;
            shell_projections[ei] = stamp && !stamp_hit.on_segment;
            if edit.bone < 0 || edit.bone as usize >= bones.len() {
                return None;
            }
            let bind = &bones[edit.bone as usize];
            let mut points = vec![[0.0; 3]; set.count as usize];
            let mut influence = vec![0f64; set.count as usize];
            for v in 0..set.count {
                let ps = set.stream_of(set.layout.position);
                points[v as usize] = decode_point(&set.streams[ps], set.field(set.layout.position, v), h, bind);
                for bank in 0..2 {
                    if set.layout.bones[bank] < 0 {
                        break;
                    }
                    let (ids, ws) = (set.field(set.layout.bones[bank], v), set.field(set.layout.weights[bank], v));
                    let (is, wsr) = (set.stream_of(set.layout.bones[bank]), set.stream_of(set.layout.weights[bank]));
                    for ch in 0..4 {
                        if edit.skin_region[set.streams[is][ids + ch] as usize] {
                            influence[v as usize] += f64::from(set.streams[wsr][ws + ch]) / 255.0;
                        }
                    }
                }
            }
            let mut next: Vec<u32> = Vec::with_capacity(triangles.len());
            let mut next_ink: Vec<u8> = Vec::with_capacity(ink_faces.len());
            let (mut pieces, mut ink) = (Vec::new(), Vec::new());
            let mut edge_vertices: HashMap<u64, u32> = HashMap::new();
            let required = if edit.swept { 16.0 / 255.0 } else { 0.35 };
            for index in (0..triangles.len()).step_by(3) {
                let face = [triangles[index], triangles[index + 1], triangles[index + 2]];
                let position = [points[face[0] as usize], points[face[1] as usize], points[face[2] as usize]];
                let fw = [influence[face[0] as usize], influence[face[1] as usize], influence[face[2] as usize]];
                let eligible = if blend_boundary { fw.iter().cloned().fold(f64::NEG_INFINITY, f64::max) > 0.0 } else { fw.iter().cloned().fold(f64::INFINITY, f64::min) >= required };
                let split = eligible
                    && if edit.mark == Mark::None {
                        subtract(&position, &volume, &mut pieces, None, false)
                    } else {
                        mark_face(&position, &volume, !stamp, &mut pieces, &mut ink, edit.mark == Mark::Puncture)
                    };
                if !split {
                    next.extend_from_slice(&face);
                    next_ink.push(ink_faces[index / 3]);
                    continue;
                }
                changed = true;
                stats.split += 1;
                next_ink.extend(std::iter::repeat_n(ink_faces[index / 3], pieces.len()));
                if edit.mark != Mark::None {
                    next_ink.extend(std::iter::repeat_n((ei + 1) as u8, ink.len()));
                    pieces.extend_from_slice(&ink);
                }
                let mut interior: Vec<(Point, u32)> = Vec::new();
                for piece in &pieces {
                    for bary in piece {
                        let mut vertex = 0u32;
                        let (mi, mx) = (bary.iter().cloned().fold(f64::INFINITY, f64::min), bary.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
                        if mx >= 1.0 - 1e-10 {
                            let at = (0..3).find(|&i| bary[i] == mx).unwrap_or(0);
                            vertex = face[at];
                        } else if mi < 1e-10 {
                            let opposite = (0..3).find(|&i| bary[i] == mi).unwrap_or(0);
                            let (mut low, mut high) = ((opposite + 1) % 3, (opposite + 2) % 3);
                            if face[low] > face[high] {
                                std::mem::swap(&mut low, &mut high);
                            }
                            let ratio = (bary[high].clamp(0.0, 1.0) * 16_777_215.0).round() as u64;
                            let key = (u64::from(face[low]) << 40) | (u64::from(face[high]) << 24) | ratio;
                            if let Some(&v) = edge_vertices.get(&key) {
                                vertex = v;
                            } else {
                                if !set.blend(&face, bary, &mut vertex) {
                                    return None;
                                }
                                edge_vertices.insert(key, vertex);
                            }
                        } else if let Some(e) = interior.iter().find(|e| (e.0[0] - bary[0]).abs() + (e.0[1] - bary[1]).abs() < 1e-9) {
                            vertex = e.1;
                        } else {
                            if !set.blend(&face, bary, &mut vertex) {
                                return None;
                            }
                            interior.push((*bary, vertex));
                        }
                        next.push(vertex);
                    }
                }
            }
            triangles = next;
            ink_faces = next_ink;
        }
        if !changed {
            continue;
        }
        // later edits can split or remove earlier ink: the surviving painted triangle is measured
        for i in 0..ink_faces.len() {
            if ink_faces[i] == 0 {
                continue;
            }
            let tag = i32::from(ink_faces[i]) - 1;
            if tag < stats.contact_edit || (tag == stats.contact_edit && u32::from(chunk.lod_mask) > stats.contact_lod) {
                continue;
            }
            let edit = &edits[tag as usize];
            let bind = &bones[edit.bone as usize];
            let ps = set.stream_of(set.layout.position);
            let face = [0, 1, 2].map(|k| decode_point(&set.streams[ps], set.field(set.layout.position, triangles[3 * i + k]), h, bind));
            let (mut point, mut normal) = ([0.0; 3], [0.0; 3]);
            if !closest_surface_point(&face, &ink_centers[tag as usize], &mut point, &mut normal) {
                continue;
            }
            let (start, end) = (endpoint(&edit.volume, 0), endpoint(&edit.volume, 1));
            let (mut gap, mut orientation, mut entry) = (0.0, 0.0, 0.0);
            for k in 0..3 {
                let d = point[k] - ink_centers[tag as usize][k];
                let from_entry = point[k] - (start[k] + end[k]) * 0.5;
                gap += d * d;
                entry += from_entry * from_entry;
                orientation += normal[k] * ink_normals[tag as usize][k];
            }
            if tag == stats.contact_edit && u32::from(chunk.lod_mask) == stats.contact_lod && gap >= best_ink_gap {
                continue;
            }
            if orientation < 0.0 {
                for n in normal.iter_mut() {
                    *n = -*n;
                }
            }
            stats.contact = point;
            stats.normal = normal;
            stats.contact_edit = tag;
            stats.contact_chunk = part as u32;
            stats.contact_lod = u32::from(chunk.lod_mask);
            stats.entry_distance_squared = entry;
            stats.projected_from_shell = shell_projections[tag as usize];
            best_ink_gap = gap;
        }
        let mut update = Update { index: part as u32, count: set.count, ..Update::default() };
        for s in 0..5 {
            if set.layout.stride[s] != 0 {
                align(&mut vertices, 16);
                update.starts[s] = vertices.len() as u32;
                vertices.extend_from_slice(&set.streams[s]);
            }
        }
        align(&mut indices, 4);
        update.offset = indices.len() as u32;
        let write_faces = |painted: bool, indices: &mut Vec<u8>| {
            let mut count = 0u32;
            for (i, &v) in triangles.iter().enumerate() {
                if (ink_faces[i / 3] != 0) == painted {
                    indices.push(v as u8);
                    indices.push((v >> 8) as u8);
                    count += 1;
                }
            }
            count
        };
        update.indices = write_faces(false, &mut indices);
        align(&mut indices, 4);
        update.ink_offset = indices.len() as u32;
        update.ink_count = write_faces(true, &mut indices);
        stats.ink_triangles += update.ink_count / 3;
        if vertices.len() + indices.len() > LIMIT {
            return None;
        }
        stats.indices_before += chunk.num_indices;
        stats.indices_after += update.indices + update.ink_count;
        stats.inserted += set.count - chunk.num_vertices;
        updates.push(update);
    }
    if updates.is_empty() {
        counts.no_overlap = true;
        return None;
    }
    let vertex_bytes = vertices.len() as u32;
    align(&mut vertices, 1024);
    let index_start = vertices.len() as u32;
    let index_size = indices.len() as u32;
    vertices.extend_from_slice(&indices);
    *counts = stats;
    Some(Output { buffer: vertices, vertex_bytes, index_start, index_size, updates })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn halves() {
        for f in [0.0f32, 1.0, -2.5, 0.333_251_95, 65504.0, 6.103_515_6e-5, 1e-6] {
            let h = float_to_half(f);
            let back = half_to_float(h);
            assert!((back - f).abs() <= f.abs() * 1e-3 + 1e-7, "{f} -> {h:04x} -> {back}");
        }
        assert_eq!(float_to_half(1.0), 0x3C00);
        assert_eq!(half_to_float(0xC000), -2.0);
    }
    /// One chunk of one quad (two triangles) bound to bone 0: a slash across it splits faces and paints a chunk.
    #[test]
    fn slash_paints_a_quad() {
        // stream 0: position Short4N (8), stream 1: indices UByte4 (4) + weights UByte4N (4)
        let elements = vec![
            Element { ty: PT_SHORT4N, usage: PS_POSITION, usage_index: 0, stream_index: 0, stream_type: ST_PER_VERTEX },
            Element { ty: PT_UBYTE4, usage: PS_SKIN_INDICES, usage_index: 0, stream_index: 1, stream_type: ST_PER_VERTEX },
            Element { ty: PT_UBYTE4N, usage: PS_SKIN_WEIGHTS, usage_index: 0, stream_index: 1, stream_type: ST_PER_VERTEX },
        ];
        let quad = [[-1.0f64, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
        let mut v0 = Vec::new();
        let mut v1 = Vec::new();
        for p in quad {
            for c in [p[0], p[1], 0.0, 0.0] {
                v0.extend_from_slice(&((c * 32767.0) as i16).to_le_bytes());
            }
            v1.extend_from_slice(&[0, 0, 0, 0, 255, 0, 0, 0]);
        }
        let mut data = v0.clone();
        let off1 = data.len() as u32;
        data.extend_from_slice(&v1);
        let ibo = data.len() as u32;
        for i in [0u16, 1, 2, 0, 2, 3] {
            data.extend_from_slice(&i.to_le_bytes());
        }
        let chunk = ChunkInfo { elements, slot_strides: vec![8, 8, 0, 0, 0, 0, 0, 0], byte_offsets: vec![0, off1, 0, 0, 0], index_type: IBCT_INDEX_USHORT, index_offset: 0, num_vertices: 4, num_indices: 6, lod_mask: 1 };
        // the quad spans 0.1 m: scale 0.1, so its corners are at +-0.1 in the bone's frame (identity)
        let h = Header { vertex_buffer_size: ibo, index_buffer_size: 12, index_buffer_offset: ibo, scale: [0.1, 0.1, 0.1, 0.0], offset: [0.0; 4] };
        let bind: Matrix = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let cut = Cut { from: [-0.015, 0.0, 0.0], to: [0.015, 0.0, 0.0], normal: [0.0, 0.0, 1.0], half_width: 0.004, depth: 0.015, outside: 0.012 };
        let mut volume = Volume::default();
        assert!(make_volume(&cut, &mut volume));
        let mut region = [false; 256];
        region[0] = true;
        let edit = Edit { volume, bone: 0, swept: true, rays: [[-0.01, 0.0, 0.05], [-0.01, 0.0, -0.05], [0.01, 0.0, 0.05], [0.01, 0.0, -0.05]], mark: Mark::Slash, visible_chunks: u64::MAX, shell_depth: 0.0, skin_region: region };
        let mut counts = Counts::default();
        let out = cut_private_blob(&data, &h, &[chunk], &[bind], &[edit], &mut counts).expect("cut");
        assert_eq!(out.updates.len(), 1);
        let u = out.updates[0];
        assert!(u.ink_count >= 3 && u.count > 4, "{u:?}");
        assert!(counts.contact_edit == 0 && counts.ink_triangles > 0);
        assert!(counts.contact[2].abs() < 1e-3, "{:?}", counts.contact);
        assert_eq!(out.index_start % 1024, 0);
        assert_eq!(out.buffer.len() as u32, out.index_start + out.index_size);
    }
}
