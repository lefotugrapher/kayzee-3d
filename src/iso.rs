//! Reading files straight out of a CD image (ISO 9660), so a disc image can
//! be used without burning or mounting it.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const SECTOR: u64 = 2048;

pub struct Entry {
    pub path: String,
    pub start: u64,
    pub len: u64,
    pub dir: bool,
}

fn le32(b: &[u8]) -> u64 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as u64
}

fn read_at(f: &mut File, at: u64, len: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; len];
    f.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
    f.read_exact(&mut buf).map_err(|_| "the image ends early".to_owned())?;
    Ok(buf)
}

/// Every file and folder in the image (names from the plain ISO 9660 tree).
pub fn list(path: &Path) -> Result<Vec<Entry>, String> {
    let mut f = File::open(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    let pvd = read_at(&mut f, 16 * SECTOR, SECTOR as usize)?;
    if pvd[0] != 1 || &pvd[1..6] != b"CD001" {
        return Err("That file is not a CD image (ISO 9660).".to_owned());
    }
    let root = &pvd[156..190];
    let mut out = Vec::new();
    let mut stack = vec![(String::new(), le32(&root[2..]), le32(&root[10..]), 0usize)];
    while let Some((prefix, start, len, depth)) = stack.pop() {
        if depth > 12 || len > 16 << 20 {
            continue;
        }
        let data = read_at(&mut f, start * SECTOR, len as usize)?;
        let mut at = 0usize;
        while at < data.len() {
            let n = data[at] as usize;
            if n == 0 {
                // records do not cross sectors: skip to the next one
                at = (at / SECTOR as usize + 1) * SECTOR as usize;
                continue;
            }
            if at + n > data.len() || n < 34 {
                break;
            }
            let r = &data[at..at + n];
            let name_len = r[32] as usize;
            let raw = &r[33..33 + name_len.min(n - 33)];
            at += n;
            if raw == [0] || raw == [1] {
                continue;
            }
            let mut name = String::from_utf8_lossy(raw).into_owned();
            if let Some(i) = name.find(';') {
                name.truncate(i);
            }
            let name = name.trim_end_matches('.').to_owned();
            let dir = r[25] & 2 != 0;
            let full = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
            let (s, l) = (le32(&r[2..]), le32(&r[10..]));
            if dir {
                stack.push((full.clone(), s, l, depth + 1));
            }
            out.push(Entry { path: full, start: s, len: l, dir });
        }
    }
    Ok(out)
}

/// The bytes of one file of the image.
pub fn read(path: &Path, e: &Entry) -> Result<Vec<u8>, String> {
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    read_at(&mut f, e.start * SECTOR, e.len as usize)
}
