//! Reading 3D model files into plain triangle meshes: Wavefront OBJ, AutoCAD
//! DXF (text), 3D Studio 3DS and QuickDraw 3D metafiles (binary 3DMF).
//! Everything comes out y-up, in the file's own units.

use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub name: String,
    pub points: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

impl Mesh {
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let mut it = self.points.iter();
        let first = *it.next()?;
        let (mut lo, mut hi) = (first, first);
        for p in it {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        Some((lo, hi))
    }

    /// Drop triangles that name missing points or are degenerate.
    fn tidy(mut self) -> Self {
        let n = self.points.len() as u32;
        self.triangles.retain(|t| t[0] < n && t[1] < n && t[2] < n && t[0] != t[1] && t[1] != t[2] && t[0] != t[2]);
        self
    }
}

pub fn load(path: &Path) -> Result<Vec<Mesh>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Mesh".into());
    let mut meshes = match ext.as_str() {
        "obj" => obj(&String::from_utf8_lossy(&bytes)),
        "dxf" => dxf(&String::from_utf8_lossy(&bytes)),
        "3ds" => three_ds(&bytes)?,
        "3dmf" | "3mf" | "t3d" | "b3d" => three_dmf(&bytes)?,
        other => return Err(format!("\".{other}\" files are not a 3D model format this app reads (OBJ, DXF, 3DS, 3DMF).")),
    };
    meshes = meshes.into_iter().map(Mesh::tidy).filter(|m| !m.triangles.is_empty()).collect();
    if meshes.is_empty() {
        return Err(format!("{} holds no surfaces that could be read.", path.display()));
    }
    for (i, m) in meshes.iter_mut().enumerate() {
        if m.name.trim().is_empty() {
            m.name = if i == 0 { stem.clone() } else { format!("{stem} {}", i + 1) };
        }
    }
    Ok(meshes)
}

/// All the file's meshes as one.
pub fn merged(meshes: &[Mesh], name: &str) -> Mesh {
    let mut out = Mesh { name: name.to_owned(), ..Default::default() };
    for m in meshes {
        let base = out.points.len() as u32;
        out.points.extend_from_slice(&m.points);
        out.triangles.extend(m.triangles.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
    }
    out
}

fn fan(out: &mut Vec<[u32; 3]>, face: &[u32]) {
    for k in 1..face.len().saturating_sub(1) {
        out.push([face[0], face[k], face[k + 1]]);
    }
}

// ----------------------------------------------------------------------------- OBJ

fn obj(text: &str) -> Vec<Mesh> {
    // points are shared by the whole file; each object or group keeps its own faces
    let mut points: Vec<[f32; 3]> = Vec::new();
    let mut parts: Vec<(String, Vec<[u32; 3]>)> = vec![(String::new(), Vec::new())];
    for line in text.lines() {
        let mut w = line.split_whitespace();
        match w.next() {
            Some("v") => {
                let n: Vec<f32> = w.take(3).filter_map(|x| x.parse().ok()).collect();
                if n.len() == 3 {
                    points.push([n[0], n[1], n[2]]);
                }
            }
            Some("f") => {
                let count = points.len() as i64;
                let face: Vec<u32> = w
                    .filter_map(|v| v.split('/').next()?.parse::<i64>().ok())
                    .filter_map(|i| {
                        let k = if i < 0 { count + i } else { i - 1 };
                        (k >= 0 && k < count).then_some(k as u32)
                    })
                    .collect();
                fan(&mut parts.last_mut().unwrap().1, &face);
            }
            Some("o") | Some("g") => {
                let name = w.collect::<Vec<_>>().join(" ");
                if parts.last().is_some_and(|p| p.1.is_empty()) {
                    parts.last_mut().unwrap().0 = name;
                } else {
                    parts.push((name, Vec::new()));
                }
            }
            _ => {}
        }
    }
    parts
        .into_iter()
        .filter(|(_, t)| !t.is_empty())
        .map(|(name, tris)| {
            // keep only the points this part uses
            let mut map: HashMap<u32, u32> = HashMap::new();
            let mut own = Vec::new();
            let triangles = tris
                .iter()
                .map(|t| {
                    let mut o = [0u32; 3];
                    for k in 0..3 {
                        o[k] = *map.entry(t[k]).or_insert_with(|| {
                            own.push(points[t[k] as usize]);
                            own.len() as u32 - 1
                        });
                    }
                    o
                })
                .collect();
            Mesh { name, points: own, triangles }
        })
        .collect()
}

// ----------------------------------------------------------------------------- DXF

fn z_up(p: [f32; 3]) -> [f32; 3] {
    [p[0], p[2], -p[1]]
}

fn dxf(text: &str) -> Vec<Mesh> {
    // pairs of (group code, value)
    let mut lines = text.lines().map(str::trim);
    let mut pairs: Vec<(i32, &str)> = Vec::new();
    while let (Some(code), Some(value)) = (lines.next(), lines.next()) {
        if let Ok(c) = code.parse::<i32>() {
            pairs.push((c, value));
        }
    }
    let mut by_layer: Vec<(String, Mesh)> = Vec::new();
    fn mesh_for<'a>(by_layer: &'a mut Vec<(String, Mesh)>, layer: &str) -> &'a mut Mesh {
        if let Some(i) = by_layer.iter().position(|(l, _)| l == layer) {
            return &mut by_layer[i].1;
        }
        by_layer.push((layer.to_owned(), Mesh { name: layer.to_owned(), ..Default::default() }));
        &mut by_layer.last_mut().unwrap().1
    }

    let mut i = 0;
    let mut in_entities = false;
    // a polyline being gathered: (layer, flags, positions, faces)
    let mut poly: Option<(String, i32, Vec<[f32; 3]>, Vec<Vec<i32>>, (usize, usize))> = None;
    while i < pairs.len() {
        let (code, value) = pairs[i];
        if code != 0 {
            i += 1;
            continue;
        }
        // the entity's own pairs run to the next code 0
        let mut j = i + 1;
        while j < pairs.len() && pairs[j].0 != 0 {
            j += 1;
        }
        let body = &pairs[i + 1..j];
        let num = |c: i32| body.iter().find(|p| p.0 == c).and_then(|p| p.1.parse::<f32>().ok());
        let int = |c: i32| body.iter().find(|p| p.0 == c).and_then(|p| p.1.trim().parse::<i32>().ok());
        let layer = body.iter().find(|p| p.0 == 8).map(|p| p.1).unwrap_or("0").to_owned();
        if value == "SECTION" {
            let name = body.iter().find(|p| p.0 == 2).map(|p| p.1).unwrap_or("");
            in_entities = name == "ENTITIES" || name == "BLOCKS";
        }
        if in_entities {
            match value {
                "3DFACE" | "SOLID" | "TRACE" => {
                    let corner = |k: i32| Some([num(10 + k)?, num(20 + k)?, num(30 + k).unwrap_or(0.0)]);
                    let c: Vec<[f32; 3]> = (0..4).filter_map(corner).collect();
                    if c.len() >= 3 {
                        let m = mesh_for(&mut by_layer, &layer);
                        let base = m.points.len() as u32;
                        let distinct = c.len() == 4 && c[3] != c[2];
                        m.points.extend(c.iter().map(|p| z_up(*p)));
                        m.triangles.push([base, base + 1, base + 2]);
                        if distinct {
                            // a SOLID's corners go in a Z order, a 3DFACE's around the edge
                            if value == "3DFACE" {
                                m.triangles.push([base, base + 2, base + 3]);
                            } else {
                                m.triangles.push([base + 1, base + 3, base + 2]);
                            }
                        }
                    }
                }
                "POLYLINE" => {
                    let flags = int(70).unwrap_or(0);
                    let (mc, nc) = (int(71).unwrap_or(0).max(0) as usize, int(72).unwrap_or(0).max(0) as usize);
                    poly = Some((layer, flags, Vec::new(), Vec::new(), (mc, nc)));
                }
                "VERTEX" => {
                    if let Some(p) = poly.as_mut() {
                        let vflags = int(70).unwrap_or(0);
                        if vflags & 128 != 0 && vflags & 64 == 0 {
                            // a face record of a polyface mesh
                            let f: Vec<i32> = [71, 72, 73, 74].iter().filter_map(|c| int(*c)).filter(|v| *v != 0).collect();
                            p.3.push(f);
                        } else {
                            p.2.push([num(10).unwrap_or(0.0), num(20).unwrap_or(0.0), num(30).unwrap_or(0.0)]);
                        }
                    }
                }
                "SEQEND" => {
                    if let Some((layer, flags, pts, faces, (mc, nc))) = poly.take() {
                        let m = mesh_for(&mut by_layer, &layer);
                        let base = m.points.len() as u32;
                        if flags & 64 != 0 {
                            m.points.extend(pts.iter().map(|p| z_up(*p)));
                            for f in faces {
                                let idx: Vec<u32> = f.iter().map(|v| base + (v.unsigned_abs() - 1)).collect();
                                fan(&mut m.triangles, &idx);
                            }
                        } else if flags & 16 != 0 && mc >= 2 && nc >= 2 && pts.len() >= mc * nc {
                            // a polygon mesh of M by N points
                            m.points.extend(pts.iter().map(|p| z_up(*p)));
                            let (wrap_m, wrap_n) = (flags & 1 != 0, flags & 32 != 0);
                            let at = |a: usize, b: usize| base + ((a % mc) * nc + (b % nc)) as u32;
                            for a in 0..(if wrap_m { mc } else { mc - 1 }) {
                                for b in 0..(if wrap_n { nc } else { nc - 1 }) {
                                    m.triangles.push([at(a, b), at(a + 1, b), at(a + 1, b + 1)]);
                                    m.triangles.push([at(a, b), at(a + 1, b + 1), at(a, b + 1)]);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        i = j;
    }
    by_layer.into_iter().map(|(_, m)| m).collect()
}

// ----------------------------------------------------------------------------- 3DS

fn three_ds(b: &[u8]) -> Result<Vec<Mesh>, String> {
    fn u16_at(b: &[u8], i: usize) -> Option<usize> {
        Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]) as usize)
    }
    fn u32_at(b: &[u8], i: usize) -> Option<usize> {
        Some(u32::from_le_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]) as usize)
    }
    fn f32_at(b: &[u8], i: usize) -> Option<f32> {
        Some(f32::from_le_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]))
    }
    fn walk(b: &[u8], mut at: usize, end: usize, name: &str, out: &mut Vec<Mesh>) {
        while at + 6 <= end {
            let (Some(id), Some(len)) = (u16_at(b, at), u32_at(b, at + 2)) else { return };
            if len < 6 || at + len > end {
                return;
            }
            let (body, stop) = (at + 6, at + len);
            match id {
                0x4D4D | 0x3D3D => walk(b, body, stop, name, out),
                0x4000 => {
                    let z = b[body..stop].iter().position(|c| *c == 0).unwrap_or(0);
                    let n = String::from_utf8_lossy(&b[body..body + z]).into_owned();
                    walk(b, body + z + 1, stop, &n, out);
                }
                0x4100 => {
                    out.push(Mesh { name: name.to_owned(), ..Default::default() });
                    walk(b, body, stop, name, out);
                }
                0x4110 => {
                    if let (Some(m), Some(n)) = (out.last_mut(), u16_at(b, body)) {
                        for k in 0..n {
                            let p = body + 2 + k * 12;
                            if let (Some(x), Some(y), Some(z)) = (f32_at(b, p), f32_at(b, p + 4), f32_at(b, p + 8)) {
                                m.points.push(z_up([x, y, z]));
                            }
                        }
                    }
                }
                0x4120 => {
                    if let (Some(m), Some(n)) = (out.last_mut(), u16_at(b, body)) {
                        for k in 0..n {
                            let p = body + 2 + k * 8;
                            if let (Some(a), Some(c), Some(d)) = (u16_at(b, p), u16_at(b, p + 2), u16_at(b, p + 4)) {
                                m.triangles.push([a as u32, c as u32, d as u32]);
                            }
                        }
                    }
                }
                _ => {}
            }
            at = stop;
        }
    }
    if u16_at(b, 0) != Some(0x4D4D) {
        return Err("This is not a 3D Studio (3DS) file.".to_owned());
    }
    let mut out = Vec::new();
    walk(b, 0, b.len(), "", &mut out);
    Ok(out)
}

// ----------------------------------------------------------------------------- 3DMF (binary)

fn three_dmf(b: &[u8]) -> Result<Vec<Mesh>, String> {
    if b.len() < 24 || &b[0..4] != b"3DMF" {
        return Err("Only binary 3DMF files are read (this one is text or another format).".to_owned());
    }
    let be32 = |i: usize| -> Option<u32> { Some(u32::from_be_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?])) };
    let bef = |i: usize| -> Option<f32> { be32(i).map(f32::from_bits) };
    let point = |i: usize| -> Option<[f32; 3]> { Some([bef(i)?, bef(i + 4)?, bef(i + 8)?]) };
    let mut out: Vec<Mesh> = Vec::new();
    let mut loose = Mesh::default();

    fn index(b: &[u8], at: usize, width: usize) -> Option<u32> {
        Some(match width {
            1 => *b.get(at)? as u32,
            2 => u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]) as u32,
            _ => u32::from_be_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?]),
        })
    }

    // containers nest; walking every chunk in order reaches the geometry inside them
    let mut stack = vec![(0usize, b.len())];
    while let Some((mut at, end)) = stack.pop() {
        while at + 8 <= end {
            let kind = &b[at..at + 4];
            let Some(size) = be32(at + 4).map(|s| s as usize) else { break };
            let body = at + 8;
            let stop = body.saturating_add(size).min(end);
            match kind {
                b"cntr" | b"bgng" => stack.push((body, stop)),
                b"tmsh" => {
                    if let (Some(nt), Some(nta), Some(ne), Some(nea), Some(np)) =
                        (be32(body), be32(body + 4), be32(body + 8), be32(body + 12), be32(body + 16))
                    {
                        let _ = (nta, nea);
                        let (nt, ne, np) = (nt as usize, ne as usize, np as usize);
                        let w = if np <= 0xFF { 1 } else if np <= 0xFFFF { 2 } else { 4 };
                        let tw = if nt <= 0xFF { 1 } else if nt <= 0xFFFF { 2 } else { 4 };
                        let mut m = Mesh::default();
                        let mut p = body + 24;
                        for _ in 0..nt {
                            if let (Some(a), Some(c), Some(d)) = (index(b, p, w), index(b, p + w, w), index(b, p + 2 * w, w)) {
                                m.triangles.push([a, c, d]);
                            }
                            p += 3 * w;
                        }
                        p += ne * (2 * w + 2 * tw);
                        for k in 0..np {
                            if let Some(q) = point(p + k * 12) {
                                m.points.push(q);
                            }
                        }
                        out.push(m);
                    }
                }
                b"mesh" => {
                    if let Some(nv) = be32(body).map(|n| n as usize) {
                        let mut m = Mesh::default();
                        for k in 0..nv {
                            if let Some(q) = point(body + 4 + k * 12) {
                                m.points.push(q);
                            }
                        }
                        let mut p = body + 4 + nv * 12;
                        if let Some(nf) = be32(p).map(|n| n as usize) {
                            p += 8; // faces, contours
                            for _ in 0..nf.min(1 << 22) {
                                let Some(n) = be32(p).map(|n| (n as i32).unsigned_abs() as usize) else { break };
                                p += 4;
                                let idx: Vec<u32> = (0..n).filter_map(|k| be32(p + k * 4)).collect();
                                p += n * 4;
                                if p > stop {
                                    break;
                                }
                                fan(&mut m.triangles, &idx);
                            }
                        }
                        out.push(m);
                    }
                }
                b"trig" => {
                    let base = loose.points.len() as u32;
                    if let (Some(a), Some(c), Some(d)) = (point(body), point(body + 12), point(body + 24)) {
                        loose.points.extend([a, c, d]);
                        loose.triangles.push([base, base + 1, base + 2]);
                    }
                }
                b"plyg" => {
                    if let Some(n) = be32(body).map(|n| n as usize) {
                        let base = loose.points.len() as u32;
                        let pts: Vec<[f32; 3]> = (0..n).filter_map(|k| point(body + 4 + k * 12)).collect();
                        let idx: Vec<u32> = (0..pts.len() as u32).map(|k| base + k).collect();
                        loose.points.extend(pts);
                        fan(&mut loose.triangles, &idx);
                    }
                }
                _ => {}
            }
            at = body + size;
        }
    }
    if !loose.triangles.is_empty() {
        out.push(loose);
    }
    Ok(out)
}
