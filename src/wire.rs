//! Wireframe geometry: each shape as line segments in its own space, and the
//! projections (the scene camera, or a flat view from one side) that put them
//! on screen.

use std::collections::HashMap;
use std::sync::Arc;

use crate::math::{self, Cam, M12, V3, add, dot, mul, sub};
use crate::model::{Node, Shape};

const TAU: f64 = std::f64::consts::TAU;

fn ring(out: &mut Vec<(V3, V3)>, n: usize, f: impl Fn(f64) -> V3) {
    let mut last = f(0.0);
    for k in 1..=n {
        let p = f(TAU * k as f64 / n as f64);
        out.push((last, p));
        last = p;
    }
}

pub fn box_lines(out: &mut Vec<(V3, V3)>, lo: V3, hi: V3) {
    let c = |i: usize| -> V3 { [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }] };
    for i in 0..8usize {
        for bit in [1usize, 2, 4] {
            if i & bit == 0 {
                out.push((c(i), c(i | bit)));
            }
        }
    }
}

/// The shape's lines in its own space. `detail` is how many lines a terrain
/// shows along each side.
pub fn own_lines(shape: &Shape, detail: usize) -> Vec<(V3, V3)> {
    let mut l = Vec::new();
    match shape {
        Shape::Cube => box_lines(&mut l, [-0.5; 3], [0.5; 3]),
        Shape::Pyramid => {
            let base = [[-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, -0.5, 0.5], [-0.5, -0.5, 0.5]];
            for k in 0..4 {
                l.push((base[k], base[(k + 1) % 4]));
                l.push((base[k], [0.0, 0.5, 0.0]));
            }
        }
        Shape::Sphere => {
            ring(&mut l, 32, |a| [a.cos(), 0.0, a.sin()]);
            ring(&mut l, 32, |a| [a.cos(), a.sin(), 0.0]);
            ring(&mut l, 32, |a| [0.0, a.cos(), a.sin()]);
            for y in [-0.5f64, 0.5] {
                let r = (1.0 - y * y).sqrt();
                ring(&mut l, 24, |a| [r * a.cos(), y, r * a.sin()]);
            }
        }
        Shape::Cylinder => {
            for y in [-1.0, 1.0] {
                ring(&mut l, 28, |a| [a.cos(), y, a.sin()]);
            }
            for k in 0..8 {
                let a = TAU * k as f64 / 8.0;
                l.push(([a.cos(), -1.0, a.sin()], [a.cos(), 1.0, a.sin()]));
            }
        }
        Shape::Cone => {
            ring(&mut l, 28, |a| [a.cos(), -1.0, a.sin()]);
            for k in 0..8 {
                let a = TAU * k as f64 / 8.0;
                l.push(([a.cos(), -1.0, a.sin()], [0.0, 1.0, 0.0]));
            }
        }
        Shape::Torus { tube, ring: big } => {
            let (t, r) = (*tube, *big);
            for (rr, y) in [(r + t, 0.0), (r - t, 0.0), (r, t), (r, -t)] {
                ring(&mut l, 36, |a| [rr * a.cos(), y, rr * a.sin()]);
            }
            for k in 0..8 {
                let a = TAU * k as f64 / 8.0;
                ring(&mut l, 12, |b| {
                    let d = r + t * b.cos();
                    [d * a.cos(), t * b.sin(), d * a.sin()]
                });
            }
        }
        Shape::Square => {
            let c = [[-1.0, 0.0, -1.0], [1.0, 0.0, -1.0], [1.0, 0.0, 1.0], [-1.0, 0.0, 1.0]];
            for k in 0..4 {
                l.push((c[k], c[(k + 1) % 4]));
            }
        }
        Shape::Disc => ring(&mut l, 32, |a| [a.cos(), 0.0, a.sin()]),
        Shape::Plane => {
            let (half, step) = (96.0, 8.0);
            let mut v = -half;
            while v <= half + 1e-9 {
                let mut u = -half;
                while u < half - 1e-9 {
                    l.push(([u, 0.0, v], [u + step, 0.0, v]));
                    l.push(([v, 0.0, u], [v, 0.0, u + step]));
                    u += step;
                }
                v += step;
            }
        }
        Shape::Terrain(t) => {
            let n = t.n;
            if n >= 2 && t.heights.len() == n * n {
                let lines = detail.clamp(4, n - 1);
                let at = |i: usize, j: usize, sign: f64| -> V3 {
                    let h = t.heights[j * n + i] as f64;
                    let y = if t.mirrored { h * sign } else { h - 32767.5 };
                    [i as f64 - n as f64 / 2.0, y, j as f64 - n as f64 / 2.0]
                };
                let pos = |k: usize| k * (n - 1) / lines;
                let sides: &[f64] = if t.mirrored { &[1.0, -1.0] } else { &[1.0] };
                for &sign in sides {
                    for a in 0..=lines {
                        for b in 0..lines {
                            l.push((at(pos(b), pos(a), sign), at(pos(b + 1), pos(a), sign)));
                            l.push((at(pos(a), pos(b), sign), at(pos(a), pos(b + 1), sign)));
                        }
                    }
                }
            }
        }
        Shape::Mesh(m) => {
            // each edge once; very large meshes show a share of their faces
            let step = (m.triangles.len() / 4000).max(1);
            let mut seen: std::collections::HashSet<(u32, u32)> = Default::default();
            for t in m.triangles.iter().step_by(step) {
                for k in 0..3 {
                    let (a, b) = (t[k], t[(k + 1) % 3]);
                    let key = if a < b { (a, b) } else { (b, a) };
                    if seen.insert(key) {
                        if let (Some(p), Some(q)) = (m.points.get(a as usize), m.points.get(b as usize)) {
                            l.push(([p[0] as f64, p[1] as f64, p[2] as f64], [q[0] as f64, q[1] as f64, q[2] as f64]));
                        }
                    }
                }
            }
        }
        Shape::Light(light) => {
            use crate::model::LightKind::*;
            match light.kind {
                Radial => {
                    ring(&mut l, 16, |a| [0.5 * a.cos(), 0.5 * a.sin(), 0.0]);
                    ring(&mut l, 16, |a| [0.5 * a.cos(), 0.0, 0.5 * a.sin()]);
                    for k in 0..6 {
                        let mut a = [0.0; 3];
                        a[k / 2] = if k % 2 == 0 { 0.6 } else { -0.6 };
                        l.push((a, mul(a, 1.6)));
                    }
                }
                Spot => {
                    ring(&mut l, 24, |a| [a.cos(), -1.0, a.sin()]);
                    for k in 0..4 {
                        let a = TAU * k as f64 / 4.0;
                        l.push(([0.0, 1.0, 0.0], [a.cos(), -1.0, a.sin()]));
                    }
                }
                SquareSpot => {
                    let base = [[-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, -0.5, 0.5], [-0.5, -0.5, 0.5]];
                    for k in 0..4 {
                        l.push((base[k], base[(k + 1) % 4]));
                        l.push((base[k], [0.0, 0.5, 0.0]));
                    }
                }
                Parallel => {
                    for y in [-1.0, 1.0] {
                        ring(&mut l, 24, |a| [a.cos(), y, a.sin()]);
                    }
                    for k in 0..4 {
                        let a = TAU * k as f64 / 4.0;
                        l.push(([a.cos(), 1.0, a.sin()], [a.cos(), -1.0, a.sin()]));
                    }
                }
                SquareParallel => {
                    box_lines(&mut l, [-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]);
                    l.push(([0.0, 0.5, 0.0], [0.0, -0.5, 0.0]));
                }
            }
        }
        Shape::Group => {}
    }
    l
}

/// Keeps each shape's own-space lines until the shape changes.
#[derive(Default)]
pub struct Cache {
    lines: HashMap<u64, (u64, Arc<Vec<(V3, V3)>>)>,
}

fn fingerprint(shape: &Shape, detail: usize) -> u64 {
    let d = detail as u64;
    match shape {
        Shape::Terrain(t) => (Arc::as_ptr(&t.heights) as u64).wrapping_mul(31) ^ (t.n as u64) << 20 ^ d ^ (t.mirrored as u64) << 60,
        Shape::Mesh(m) => (Arc::as_ptr(&m.triangles) as u64) ^ 7,
        Shape::Torus { tube, ring } => tube.to_bits().wrapping_mul(13) ^ ring.to_bits(),
        Shape::Light(l) => 1000 + l.kind as u64,
        other => other.label().bytes().fold(0xcbf29ce484222325u64, |h, c| (h ^ c as u64).wrapping_mul(0x100000001b3)),
    }
}

impl Cache {
    pub fn lines(&mut self, node: &Node, detail: usize) -> Arc<Vec<(V3, V3)>> {
        let print = fingerprint(&node.shape, detail);
        match self.lines.get(&node.id) {
            Some((p, l)) if *p == print => l.clone(),
            _ => {
                let l = Arc::new(own_lines(&node.shape, detail));
                self.lines.insert(node.id, (print, l.clone()));
                l
            }
        }
    }
    pub fn forget_missing(&mut self, alive: impl Fn(u64) -> bool) {
        self.lines.retain(|id, _| alive(*id));
    }
}

// --------------------------------------------------------------------------- projections

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
    Front,
    Back,
}

impl Side {
    pub const ALL: [Side; 6] = [Side::Top, Side::Bottom, Side::Left, Side::Right, Side::Front, Side::Back];
    pub fn label(self) -> &'static str {
        match self {
            Side::Top => "From Top",
            Side::Bottom => "From Bottom",
            Side::Left => "From Left",
            Side::Right => "From Right",
            Side::Front => "From Front",
            Side::Back => "From Back",
        }
    }
    /// (right, up, forward) of the flat view.
    pub fn frame(self) -> (V3, V3, V3) {
        match self {
            Side::Top => ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]),
            Side::Bottom => ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
            Side::Front => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            Side::Back => ([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]),
            Side::Right => ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]),
            Side::Left => ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
        }
    }
}

/// How the working window looks at the scene.
#[derive(Clone, Debug)]
pub enum Proj {
    /// Through the scene's camera.
    Camera { cam: Cam, size: (u32, u32) },
    /// Flat, from one side: `centre` is the world point in the middle, `half`
    /// the world length from the middle to the left or right edge.
    Flat { side: Side, centre: V3, half: f64, size: (u32, u32) },
}

impl Proj {
    pub fn frame(&self) -> (V3, V3, V3) {
        match self {
            Proj::Camera { cam, .. } => (cam.right, cam.up, cam.forward),
            Proj::Flat { side, .. } => side.frame(),
        }
    }

    /// (x, y, depth): x and y run 0..1 across and down the picture.
    fn flat(&self, p: V3) -> Option<[f64; 3]> {
        match self {
            Proj::Camera { cam, size } => {
                let c = cam.to_camera(p);
                if c[2] <= 1e-9 {
                    return None;
                }
                let (tx, ty) = cam.half_tans(*size);
                Some([0.5 + 0.5 * (c[0] / c[2]) / tx, 0.5 - 0.5 * (c[1] / c[2]) / ty, c[2]])
            }
            Proj::Flat { side, centre, half, size } => {
                let (r, u, f) = side.frame();
                let d = sub(p, *centre);
                let half_y = half * size.1 as f64 / size.0 as f64;
                Some([0.5 + 0.5 * dot(d, r) / half, 0.5 - 0.5 * dot(d, u) / half_y, dot(d, f) + 1.0e5])
            }
        }
    }

    pub fn point(&self, p: V3) -> Option<([f32; 2], f64)> {
        self.flat(p).map(|v| ([v[0] as f32, v[1] as f32], v[2]))
    }

    /// A segment clipped to what is in front of the eye.
    pub fn segment(&self, a: V3, b: V3) -> Option<([f32; 2], [f32; 2], f64)> {
        let (a, b) = match self {
            Proj::Camera { cam, .. } => {
                let near = 1e-3;
                let (ca, cb) = (cam.to_camera(a), cam.to_camera(b));
                if ca[2] < near && cb[2] < near {
                    return None;
                }
                if ca[2] < near {
                    let t = (near - ca[2]) / (cb[2] - ca[2]);
                    (add(a, mul(sub(b, a), t)), b)
                } else if cb[2] < near {
                    let t = (near - cb[2]) / (ca[2] - cb[2]);
                    (a, add(b, mul(sub(a, b), t)))
                } else {
                    (a, b)
                }
            }
            Proj::Flat { .. } => (a, b),
        };
        let (pa, pb) = (self.flat(a)?, self.flat(b)?);
        let off = |f: fn(&[f64; 3]) -> bool| f(&pa) && f(&pb);
        if off(|p| p[0] < -1.0) || off(|p| p[0] > 2.0) || off(|p| p[1] < -1.0) || off(|p| p[1] > 2.0) {
            return None;
        }
        Some(([pa[0] as f32, pa[1] as f32], [pb[0] as f32, pb[1] as f32], 0.5 * (pa[2] + pb[2])))
    }

    /// The ray through a picture point: where it starts and its unit direction.
    pub fn ray(&self, x: f32, y: f32) -> (V3, V3) {
        match self {
            Proj::Camera { cam, size } => (cam.eye, cam.ray(x, y, *size)),
            Proj::Flat { side, centre, half, size } => {
                let (r, u, f) = side.frame();
                let half_y = half * size.1 as f64 / size.0 as f64;
                let p = add(add(*centre, mul(r, (x as f64 * 2.0 - 1.0) * half)), mul(u, (1.0 - y as f64 * 2.0) * half_y));
                (sub(p, mul(f, 1.0e5)), f)
            }
        }
    }

    /// World length of one picture-width at the given world point (for turning
    /// screen drags into world moves).
    pub fn world_per_width(&self, at: V3) -> f64 {
        match self {
            Proj::Camera { cam, size } => {
                let depth = cam.to_camera(at)[2].max(1e-6);
                2.0 * depth * cam.half_tans(*size).0
            }
            Proj::Flat { half, .. } => 2.0 * half,
        }
    }
}

/// The world corners of a placement's own box.
pub fn box_corners(m: &M12, lo: V3, hi: V3) -> [V3; 8] {
    let mut out = [[0.0; 3]; 8];
    for (i, c) in out.iter_mut().enumerate() {
        *c = math::place(m, [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }]);
    }
    out
}
