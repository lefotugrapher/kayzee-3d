//! Writing a movie without any system helper: Motion-JPEG in an AVI file,
//! which every platform's players open.

use std::io::Write;
use std::path::{Path, PathBuf};

fn chunk(out: &mut Vec<u8>, tag: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(tag);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
}

fn list(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(b"LIST");
    out.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
}

/// Join picture files (all the same size) into `out` at `fps` frames a second.
pub fn write_avi(out: &Path, fps: u32, frames: &[PathBuf]) -> Result<(), String> {
    let mut jpegs: Vec<Vec<u8>> = Vec::with_capacity(frames.len());
    let (mut w, mut h) = (0u32, 0u32);
    for f in frames {
        let img = image::open(f).map_err(|e| format!("Cannot read {}: {e}", f.display()))?.to_rgb8();
        if w == 0 {
            (w, h) = img.dimensions();
        } else if (w, h) != img.dimensions() {
            return Err("The frames are not all the same size.".to_owned());
        }
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 92).encode_image(&img).map_err(|e| e.to_string())?;
        jpegs.push(bytes);
    }
    if jpegs.is_empty() {
        return Err("There are no frames.".to_owned());
    }
    let fps = fps.max(1);
    let count = jpegs.len() as u32;
    let biggest = jpegs.iter().map(Vec::len).max().unwrap_or(0) as u32;
    let u = |v: u32| v.to_le_bytes();

    let mut avih = Vec::new();
    for v in [1_000_000 / fps, biggest * fps, 0, 0x10, count, 0, 1, biggest, w, h, 0, 0, 0, 0] {
        avih.extend_from_slice(&u(v));
    }
    let mut strh = Vec::new();
    strh.extend_from_slice(b"vids");
    strh.extend_from_slice(b"MJPG");
    for v in [0, 0, 0, 1, fps, 0, count, biggest, 10_000, 0] {
        strh.extend_from_slice(&u(v));
    }
    for v in [0u16, 0, w as u16, h as u16] {
        strh.extend_from_slice(&v.to_le_bytes());
    }
    let mut strf = Vec::new();
    strf.extend_from_slice(&u(40));
    strf.extend_from_slice(&u(w));
    strf.extend_from_slice(&u(h));
    strf.extend_from_slice(&1u16.to_le_bytes());
    strf.extend_from_slice(&24u16.to_le_bytes());
    strf.extend_from_slice(b"MJPG");
    for v in [w * h * 3, 0, 0, 0, 0] {
        strf.extend_from_slice(&u(v));
    }
    let mut strl = Vec::new();
    chunk(&mut strl, b"strh", &strh);
    chunk(&mut strl, b"strf", &strf);
    let mut hdrl = Vec::new();
    chunk(&mut hdrl, b"avih", &avih);
    list(&mut hdrl, b"strl", &strl);

    let mut movi = Vec::new();
    let mut index = Vec::new();
    for j in &jpegs {
        index.extend_from_slice(b"00dc");
        index.extend_from_slice(&u(0x10));
        index.extend_from_slice(&u(movi.len() as u32 + 4));
        index.extend_from_slice(&u(j.len() as u32));
        chunk(&mut movi, b"00dc", j);
    }

    let mut body = Vec::new();
    body.extend_from_slice(b"AVI ");
    list(&mut body, b"hdrl", &hdrl);
    list(&mut body, b"movi", &movi);
    chunk(&mut body, b"idx1", &index);
    let mut file = std::fs::File::create(out).map_err(|e| format!("Cannot make {}: {e}", out.display()))?;
    file.write_all(b"RIFF").and_then(|_| file.write_all(&u(body.len() as u32))).and_then(|_| file.write_all(&body)).map_err(|e| format!("Cannot write {}: {e}", out.display()))
}
