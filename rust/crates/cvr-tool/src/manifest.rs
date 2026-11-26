//! File hashes and package manifests: what a package or a test installation holds, path by path.
use crate::Result;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

pub fn sha256(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

pub fn print_hashes(paths: &[String]) -> Result {
    for p in paths {
        println!("{}  {}", sha256(Path::new(p))?, p);
    }
    Ok(())
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        // Links are not followed: a manifest describes this tree only.
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            walk(&entry.path(), out)?;
        } else if kind.is_file() {
            out.push(entry.path());
        }
    }
    Ok(())
}

/// Tab-separated: relative path (with '/'), bytes, SHA-256.
pub fn print_manifest(root: &Path) -> Result {
    let mut files = Vec::new();
    walk(root, &mut files)?;
    for f in files {
        let rel = f.strip_prefix(root).map_err(|e| e.to_string())?;
        let rel = rel.to_string_lossy().replace('\\', "/");
        let bytes = fs::metadata(&f).map_err(|e| e.to_string())?.len();
        println!("{rel}\t{bytes}\t{}", sha256(&f)?);
    }
    Ok(())
}
