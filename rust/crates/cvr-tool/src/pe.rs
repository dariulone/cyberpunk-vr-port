//! The export table of a PE32+ image, read off the file: what RED4ext, the scripts' loaders and the tools find in the
//! plugin (Main, Query, Supports, the CyberpunkVR_* variables, ...). Stage 0 records it; every later DLL is compared.
use crate::Result;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    pub name: Option<String>,
    pub ordinal: u32,
    pub rva: u32,
    /// "DLL.Function" when the export forwards elsewhere.
    pub forward: Option<String>,
}

fn u16_at(d: &[u8], o: usize) -> Result<u16> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or_else(|| format!("truncated at {o:#x}"))
}
fn u32_at(d: &[u8], o: usize) -> Result<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| format!("truncated at {o:#x}"))
}
fn cstr_at(d: &[u8], o: usize) -> Result<String> {
    let tail = d.get(o..).ok_or_else(|| format!("string outside the file at {o:#x}"))?;
    let end = tail.iter().position(|&b| b == 0).ok_or("unterminated string")?;
    Ok(String::from_utf8_lossy(&tail[..end]).into_owned())
}

pub fn exports(data: &[u8]) -> Result<Vec<Export>> {
    if data.get(0..2) != Some(b"MZ") {
        return Err("not a PE image (no MZ)".into());
    }
    let pe = u32_at(data, 0x3c)? as usize;
    if data.get(pe..pe + 4) != Some(b"PE\0\0") {
        return Err("not a PE image (no PE signature)".into());
    }
    let sections = u16_at(data, pe + 6)? as usize;
    let optional_size = u16_at(data, pe + 20)? as usize;
    let optional = pe + 24;
    if u16_at(data, optional)? != 0x20b {
        return Err("not PE32+".into());
    }
    let export_rva = u32_at(data, optional + 112)? as usize;
    let export_size = u32_at(data, optional + 116)? as usize;
    if export_rva == 0 {
        return Ok(Vec::new());
    }
    let table = optional + optional_size;
    let mut map = Vec::with_capacity(sections);
    for i in 0..sections {
        let s = table + i * 40;
        let vsize = u32_at(data, s + 8)? as usize;
        let va = u32_at(data, s + 12)? as usize;
        let raw_size = u32_at(data, s + 16)? as usize;
        let raw = u32_at(data, s + 20)? as usize;
        map.push((va, vsize.max(raw_size), raw));
    }
    let off = |rva: usize| -> Result<usize> {
        map.iter()
            .find(|(va, size, _)| *va <= rva && rva < va + size)
            .map(|(va, _, raw)| raw + rva - va)
            .ok_or_else(|| format!("rva {rva:#x} is in no section"))
    };
    let dir = off(export_rva)?;
    let base = u32_at(data, dir + 16)?;
    let functions = u32_at(data, dir + 20)? as usize;
    let names = u32_at(data, dir + 24)? as usize;
    let address_table = off(u32_at(data, dir + 28)? as usize)?;
    let name_table = off(u32_at(data, dir + 32)? as usize)?;
    let ordinal_table = off(u32_at(data, dir + 36)? as usize)?;
    let mut named = vec![None; functions];
    for i in 0..names {
        let name_rva = u32_at(data, name_table + i * 4)? as usize;
        let index = u16_at(data, ordinal_table + i * 2)? as usize;
        if index < functions {
            named[index] = Some(cstr_at(data, off(name_rva)?)?);
        }
    }
    let mut out = Vec::new();
    for (i, name) in named.into_iter().enumerate() {
        let rva = u32_at(data, address_table + i * 4)?;
        if rva == 0 {
            continue;
        }
        let forward = if (rva as usize) >= export_rva && (rva as usize) < export_rva + export_size {
            Some(cstr_at(data, off(rva as usize)?)?)
        } else {
            None
        };
        out.push(Export { name, ordinal: base + i as u32, rva, forward });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.ordinal.cmp(&b.ordinal)));
    Ok(out)
}

pub fn print_exports(path: &Path) -> Result {
    let data = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    for e in exports(&data)? {
        let name = e.name.clone().unwrap_or_else(|| format!("#{}", e.ordinal));
        match e.forward {
            Some(f) => println!("{name} -> {f}"),
            None => println!("{name} 0x{:08X}", e.rva),
        }
    }
    Ok(())
}
