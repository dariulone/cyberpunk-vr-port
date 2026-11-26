//! A game save (sav.dat) read without the game (formerly include/Lobby/SaveFile.hpp and SavePackage.hpp; the C++
//! headers stay for tools/saves/savdump.cpp): its compressed chunks unpacked, its node table, and the RED package a node
//! may hold (the scripted systems' data).
//!
//! sav.dat (2.5, save version 277): "VASC", save and game version, then an "FZLC" chunk table -- u32 count, then (u32
//! file offset, u32 stored size, u32 unpacked size) per chunk; each chunk is "4ZLX" + u32 unpacked size + an LZ4 block.
//! Unpacked back to back they are the nodes' data, which starts where the first chunk starts in the file (node offsets
//! count from the file's start). The file ends with u32 the node table's offset and "ENOD"; the table: "EDON", a VLQ
//! count, then per node a string (VLQ length: bit 7 set = 8-bit text), i32 next sibling, i32 first child, u32 offset,
//! u32 size.
//!
//! A package (in a node, after its own u32 index and u32 size): u16 version (4), u16 section count (6), u32 component
//! count, four u32 offsets (name descriptors, name data, chunk descriptors, chunk data; each from the base), u32 count
//! of root ids and that many u64; the base is where the root ids end. A name descriptor: low 24 bits its offset, high 8
//! its length with the terminating zero. A chunk descriptor: u32 type name, u32 offset. A chunk or struct value: u16
//! property count, then per property u16 name, u16 type, u32 offset from its start; defaults are not written.

#[derive(Default, Clone)]
pub struct Node {
    pub name: String,
    pub next: i32,
    pub child: i32,
    pub offset: u32,
    pub size: u32,
}

#[derive(Default)]
pub struct Save {
    /// The unpacked node data, indexed by file offset (the part before it zero).
    pub data: Vec<u8>,
    pub nodes: Vec<Node>,
    pub error: String,
}
impl Save {
    pub fn find(&self, name: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.name == name)
    }
}

pub fn u32_at(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]])
}
/// The engine's variable-length number: the first byte's bit 7 a flag (a string's 8-bit text), bit 6 = more, its low 6
/// bits; then 7 bits a byte, bit 7 = more.
pub fn vlq(d: &[u8], at: &mut usize, flag: &mut bool) -> usize {
    if *at >= d.len() {
        return 0;
    }
    let mut b = d[*at];
    *at += 1;
    *flag = b & 0x80 != 0;
    let mut v = (b & 0x3F) as usize;
    let mut shift = 6;
    let mut more = b & 0x40 != 0;
    while more && *at < d.len() && shift < 32 {
        b = d[*at];
        *at += 1;
        v |= ((b & 0x7F) as usize) << shift;
        shift += 7;
        more = b & 0x80 != 0;
    }
    v
}
/// One LZ4 block (no frame): literals and matches until the input runs out; exactly `dst.len()` bytes.
pub fn lz4(src: &[u8], dst: &mut [u8]) -> bool {
    let (n, cap) = (src.len(), dst.len());
    let (mut i, mut o) = (0usize, 0usize);
    while i < n {
        let token = src[i];
        i += 1;
        let mut lit = (token >> 4) as usize;
        if lit == 15 {
            loop {
                if i >= n {
                    return false;
                }
                let b = src[i];
                i += 1;
                lit += b as usize;
                if b != 255 {
                    break;
                }
            }
        }
        if i + lit > n || o + lit > cap {
            return false;
        }
        dst[o..o + lit].copy_from_slice(&src[i..i + lit]);
        i += lit;
        o += lit;
        if i >= n {
            break; // the last sequence has no match
        }
        if i + 2 > n {
            return false;
        }
        let back = src[i] as usize | (src[i + 1] as usize) << 8;
        i += 2;
        let mut len = (token & 15) as usize;
        if len == 15 {
            loop {
                if i >= n {
                    return false;
                }
                let b = src[i];
                i += 1;
                len += b as usize;
                if b != 255 {
                    break;
                }
            }
        }
        len += 4;
        if back == 0 || back > o || o + len > cap {
            return false;
        }
        for _ in 0..len {
            dst[o] = dst[o - back];
            o += 1;
        }
    }
    o == cap
}

/// The save at `path`: its node data and table, or `error` set.
pub fn read(path: &std::path::Path) -> Save {
    let mut s = Save::default();
    let Ok(file) = std::fs::read(path) else {
        s.error = "cannot open".into();
        return s;
    };
    if file.len() < 64 || &file[..4] != b"VASC" {
        s.error = "not a save".into();
        return s;
    }
    if &file[file.len() - 4..] != b"ENOD" {
        s.error = "no node table".into();
        return s;
    }
    let Some(table) = (4..60).find(|&k| &file[k..k + 4] == b"FZLC").map(|k| k + 4) else {
        s.error = "no chunk table".into();
        return s;
    };
    let count = u32_at(&file, table);
    if count == 0 || count > 512 || table + 4 + 12 * count as usize > file.len() {
        s.error = "bad chunk table".into();
        return s;
    }
    let first = u32_at(&file, table + 4) as usize;
    let mut total = first;
    for c in 0..count as usize {
        total += u32_at(&file, table + 4 + 12 * c + 8) as usize;
    }
    s.data = vec![0; total];
    let mut at = first;
    for c in 0..count as usize {
        let e = table + 4 + 12 * c;
        let (off, stored, size) = (u32_at(&file, e) as usize, u32_at(&file, e + 4) as usize, u32_at(&file, e + 8) as usize);
        if off + stored > file.len()
            || stored < 8
            || &file[off..off + 4] != b"4ZLX"
            || u32_at(&file, off + 4) as usize != size
            || !lz4(&file[off + 8..off + stored], &mut s.data[at..at + size])
        {
            s.error = format!("chunk {c} does not unpack");
            return s;
        }
        at += size;
    }
    // The node table.
    let mut p = u32_at(&file, file.len() - 8) as usize;
    if p + 8 > file.len() {
        s.error = "bad node table offset".into();
        return s;
    }
    p += 4; // its own tag, "EDON"
    let mut narrow = false;
    let nodes = vlq(&file, &mut p, &mut narrow);
    let mut k = 0;
    while k < nodes && p < file.len() - 8 {
        let mut n = Node::default();
        let chars = vlq(&file, &mut p, &mut narrow);
        if narrow {
            let end = (p + chars).min(file.len());
            n.name = file[p..end].iter().map(|&b| b as char).collect();
            p += chars;
        } else {
            for c in 0..chars {
                n.name.push(file.get(p + 2 * c).copied().unwrap_or(0) as char);
            }
            p += 2 * chars;
        }
        if p + 16 > file.len() {
            break;
        }
        n.next = u32_at(&file, p) as i32;
        n.child = u32_at(&file, p + 4) as i32;
        n.offset = u32_at(&file, p + 8);
        n.size = u32_at(&file, p + 12);
        p += 16;
        s.nodes.push(n);
        k += 1;
    }
    if s.nodes.len() != nodes {
        s.error = "node table cut short".into();
    }
    s
}

/// A property of a chunk or struct value: where its value is, in the package's bytes.
#[derive(Clone)]
pub struct Prop {
    pub name: String,
    pub ty: String,
    pub at: usize,
}

/// A RED package inside a save node.
pub struct Package<'a> {
    /// Whether a type is an enum (its value a u16 name).
    pub is_enum: Box<dyn Fn(&str) -> bool + 'a>,
    d: &'a [u8],
    base: usize,
    names: Vec<String>,
    chunks: Vec<(u32, usize)>,
}
impl<'a> Package<'a> {
    /// Without the game's RTTI, an enum is guessed by its name.
    pub fn new() -> Package<'a> {
        Package {
            is_enum: Box::new(|t: &str| t.starts_with("gamedata") || (t.len() > 1 && t.as_bytes()[0] == b'E' && t.as_bytes()[1].is_ascii_uppercase())),
            d: &[],
            base: 0,
            names: Vec::new(),
            chunks: Vec::new(),
        }
    }
    /// The node's bytes; false when they do not read as a package.
    pub fn open(&mut self, data: &'a [u8]) -> bool {
        if data.len() < 36 {
            return false;
        }
        self.d = &data[8..];
        if self.u16(0) != 4 || self.u16(2) != 6 {
            return false;
        }
        let (name_desc, name_data, chunk_desc, chunk_data) = (self.u32(8), self.u32(12), self.u32(16), self.u32(20));
        let roots = self.u32(24) as usize;
        self.base = 28 + 8 * roots;
        let n = self.d.len();
        if self.base + chunk_data as usize > n || name_desc > name_data || name_data > chunk_desc || chunk_desc > chunk_data {
            return false;
        }
        self.names.clear();
        let mut k = name_desc;
        while k + 4 <= name_data {
            let v = self.u32(self.base + k as usize);
            let (off, len) = ((v & 0xFFFFFF) as usize, (v >> 24) as usize);
            if len == 0 || self.base + off + len > n {
                return false;
            }
            let start = self.base + off;
            self.names.push(self.d[start..start + len - 1].iter().map(|&b| b as char).collect());
            k += 4;
        }
        self.chunks.clear();
        let mut k = chunk_desc;
        while k + 8 <= chunk_data {
            let (ty, off) = (self.u32(self.base + k as usize), self.u32(self.base + k as usize + 4) as usize);
            if ty as usize >= self.names.len() || self.base + off > n {
                return false;
            }
            self.chunks.push((ty, self.base + off));
            k += 8;
        }
        true
    }
    pub fn chunks(&self) -> usize {
        self.chunks.len()
    }
    pub fn chunk_type(&self, k: usize) -> &str {
        &self.names[self.chunks[k].0 as usize]
    }
    pub fn chunk_at(&self, k: usize) -> usize {
        self.chunks[k].1
    }
    pub fn name(&self, k: usize) -> &str {
        self.names.get(k).map_or("", |s| s.as_str())
    }
    /// A chunk's or a struct's properties.
    pub fn props(&self, at: usize) -> Vec<Prop> {
        let n = self.d.len();
        let mut out = Vec::new();
        if at + 2 > n {
            return out;
        }
        let count = self.u16(at) as usize;
        if at + 2 + 8 * count > n {
            return out;
        }
        for k in 0..count {
            let e = at + 2 + 8 * k;
            let (name, ty, off) = (self.u16(e) as usize, self.u16(e + 2) as usize, self.u32(e + 4) as usize);
            if name >= self.names.len() || ty >= self.names.len() || at + off > n {
                return Vec::new();
            }
            out.push(Prop {
                name: self.names[name].clone(),
                ty: self.names[ty].clone(),
                at: at + off,
            });
        }
        out
    }
    /// A property by name (also as a script writes it, m_ in front).
    pub fn find<'p>(&self, props: &'p [Prop], name: &str) -> Option<&'p Prop> {
        let m = format!("m_{name}");
        props.iter().find(|p| p.name == name || p.name == m)
    }
    fn fits(&self, at: usize, len: usize) -> usize {
        if at + len <= self.d.len() {
            at + len
        } else {
            0
        }
    }
    /// Where a value of this type ends; 0 when it does not read.
    pub fn end(&self, t: &str, at: usize, depth: i32) -> usize {
        let n = self.d.len();
        if depth > 32 || at > n {
            return 0;
        }
        match t {
            "Bool" | "Int8" | "Uint8" => return self.fits(at, 1),
            "Int16" | "Uint16" | "CName" => return self.fits(at, 2),
            "Int32" | "Uint32" | "Float" => return self.fits(at, 4),
            "Int64" | "Uint64" | "Double" | "TweakDBID" | "CRUID" => return self.fits(at, 8),
            "String" => return if at + 2 <= n { self.fits(at, 2 + self.u16(at) as usize) } else { 0 },
            _ => {}
        }
        if t.starts_with("handle:") || t.starts_with("whandle:") {
            return self.fits(at, 4);
        }
        if let Some(element) = t.strip_prefix("array:") {
            if at + 4 > n {
                return 0;
            }
            let count = self.u32(at);
            let mut p = at + 4;
            let mut k = 0;
            while k < count && p != 0 {
                p = self.end(element, p, depth + 1);
                k += 1;
            }
            return p;
        }
        if (self.is_enum)(t) {
            return self.fits(at, 2);
        }
        // A struct: its property table, then its values.
        let props = self.props(at);
        if at + 2 > n {
            return 0;
        }
        let mut end = at + 2 + 8 * props.len();
        for p in &props {
            let e = self.end(&p.ty, p.at, depth + 1);
            if e == 0 {
                return 0;
            }
            end = end.max(e);
        }
        end
    }
    /// An array's elements: where each starts.
    pub fn elements(&self, t: &str, at: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let Some(element) = t.strip_prefix("array:") else { return out };
        if at + 4 > self.d.len() {
            return out;
        }
        let count = self.u32(at);
        let mut p = at + 4;
        let mut k = 0;
        while k < count && p != 0 {
            out.push(p);
            p = self.end(element, p, 0);
            k += 1;
        }
        if p == 0 {
            out.clear();
        }
        out
    }
    pub fn u8(&self, at: usize) -> u8 {
        self.d.get(at).copied().unwrap_or(0)
    }
    pub fn u16(&self, at: usize) -> u16 {
        if at + 2 <= self.d.len() {
            u16::from_le_bytes([self.d[at], self.d[at + 1]])
        } else {
            0
        }
    }
    pub fn u32(&self, at: usize) -> u32 {
        if at + 4 <= self.d.len() {
            u32_at(self.d, at)
        } else {
            0
        }
    }
    pub fn u64(&self, at: usize) -> u64 {
        if at + 8 <= self.d.len() {
            u64::from_le_bytes(self.d[at..at + 8].try_into().unwrap_or([0; 8]))
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lz4_literals_and_a_match() {
        // "abcabcabc": 3 literals, then a match back 3 for 6 bytes (4 + 2).
        let src = [0x32, b'a', b'b', b'c', 3, 0];
        let mut dst = [0u8; 9];
        assert!(lz4(&src, &mut dst));
        assert_eq!(&dst, b"abcabcabc");
    }
    #[test]
    fn vlq_flag_and_continuation() {
        let d = [0xC1, 0x02];
        let (mut at, mut flag) = (0, false);
        assert_eq!(vlq(&d, &mut at, &mut flag), 1 | (2 << 6));
        assert!(flag);
        assert_eq!(at, 2);
    }
}
