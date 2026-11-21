//! Making new objects: the Create palette's shapes with their starting size,
//! place and surface.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::math::{self, M12, V3};
use crate::model::{Light, LightKind, Material, MeshData, Project, Shape, Terrain};
use crate::state::Doc;
use crate::terrain_ops::{self, Fractal};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Make {
    Water,
    Cloud,
    Ground,
    Terrain,
    Stone,
    Lattice,
    Sphere,
    Ellipsoid,
    Egg,
    Torus,
    Cylinder,
    Puck,
    Rod,
    Cube,
    Brick,
    Tower,
    Pyramid,
    FlatPyramid,
    Obelisk,
    Cone,
    FlatCone,
    Spike,
    Disc,
    Square,
    Radial,
    Spot,
    SquareSpot,
    Parallel,
}

impl Make {
    pub fn label(self) -> &'static str {
        use Make::*;
        match self {
            Water => "Water Plane",
            Cloud => "Cloud Plane",
            Ground => "Ground Plane",
            Terrain => "Terrain",
            Stone => "Stone",
            Lattice => "Symmetric Lattice",
            Sphere => "Sphere",
            Ellipsoid => "Squashed Sphere",
            Egg => "Stretched Sphere",
            Torus => "Torus",
            Cylinder => "Cylinder",
            Puck => "Squashed Cylinder",
            Rod => "Stretched Cylinder",
            Cube => "Cube",
            Brick => "Squashed Cube",
            Tower => "Stretched Cube",
            Pyramid => "Pyramid",
            FlatPyramid => "Squashed Pyramid",
            Obelisk => "Stretched Pyramid",
            Cone => "Cone",
            FlatCone => "Squashed Cone",
            Spike => "Stretched Cone",
            Disc => "Disc",
            Square => "Square",
            Radial => "Radial Light",
            Spot => "Spotlight",
            SquareSpot => "Square Spotlight",
            Parallel => "Parallel Light",
        }
    }
}

/// A plain surface of one colour, in the renderer's material form.
pub fn plain(rgb: [f64; 3]) -> Value {
    json!({
        "flags": 65536,
        "channels": {
            "diffuse_color": rgb, "ambient_color": [1.0, 1.0, 1.0],
            "specular_color": [1.0, 1.0, 1.0], "specular_halo": [0.8, 0.8, 0.8],
            "diffusion": 1.0, "ambience": 0.2, "specularity": 0.25
        }
    })
}

/// Index of a material with this name, adding it when it is missing.
pub fn material_named(p: &mut Project, name: &str, record: impl FnOnce() -> Value) -> usize {
    if let Some(i) = p.materials.iter().position(|m| m.name == name) {
        return i;
    }
    p.materials.push(Material { name: name.to_owned(), record: record(), keys: Vec::new() });
    p.materials.len() - 1
}

/// A fresh copy of a material for one object to own.
pub fn own_material(p: &mut Project, name: &str, record: Value) -> usize {
    p.materials.push(Material { name: name.to_owned(), record, keys: Vec::new() });
    p.materials.len() - 1
}

fn sized(size: V3, centre: V3) -> M12 {
    math::from_axes([size[0], 0.0, 0.0], [0.0, size[1], 0.0], [0.0, 0.0, size[2]], centre)
}

/// A new terrain of `n` by `n` heights.
pub fn terrain_shape(n: usize, seed: u64, mirrored: bool) -> Terrain {
    let mut g = terrain_ops::fractal(n, Fractal::Ridges, seed, 7, 2.0);
    terrain_ops::edges(&mut g, 0.45, 0.0, true);
    if mirrored {
        terrain_ops::raise(&mut g, 0.0);
    }
    Terrain { n, heights: Arc::new(g.to_u16()), mirrored, smooth: true, lower: -1e-5, upper: 1.00001, faces: 0, solid_faces: 63 }
}

/// A rock: a ball of triangles pushed in and out by noise.
pub fn stone_mesh(seed: u64) -> MeshData {
    // start from an octahedron and split each face a few times
    let mut pts: Vec<V3> = vec![[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]];
    let mut tris: Vec<[u32; 3]> = vec![[0, 2, 4], [2, 1, 4], [1, 3, 4], [3, 0, 4], [2, 0, 5], [1, 2, 5], [3, 1, 5], [0, 3, 5]];
    for _ in 0..3 {
        let mut mid: std::collections::HashMap<(u32, u32), u32> = Default::default();
        let mut next = Vec::with_capacity(tris.len() * 4);
        for t in &tris {
            let mut m = [0u32; 3];
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let key = if a < b { (a, b) } else { (b, a) };
                m[k] = *mid.entry(key).or_insert_with(|| {
                    let p = math::unit(math::add(pts[a as usize], pts[b as usize]));
                    pts.push(p);
                    pts.len() as u32 - 1
                });
            }
            next.extend([[t[0], m[0], m[2]], [t[1], m[1], m[0]], [t[2], m[2], m[1]], [m[0], m[1], m[2]]]);
        }
        tris = next;
    }
    // a few random lobes and a squash give each stone its own form
    let mut s = seed.wrapping_mul(0x9E3779B97F4A7C15) | 1;
    let mut rnd = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f64 / (1u64 << 53) as f64
    };
    let lobes: Vec<(V3, f64, f64)> = (0..9).map(|_| (math::unit([rnd() * 2.0 - 1.0, rnd() * 2.0 - 1.0, rnd() * 2.0 - 1.0]), rnd() * 0.35 - 0.12, 1.5 + rnd() * 3.0)).collect();
    let squash = [0.8 + rnd() * 0.4, 0.55 + rnd() * 0.3, 0.8 + rnd() * 0.4];
    let points: Vec<[f32; 3]> = pts
        .iter()
        .map(|p| {
            let mut r = 1.0;
            for (d, amount, tight) in &lobes {
                r += amount * math::dot(*p, *d).max(0.0).powf(*tight);
            }
            [(p[0] * r * squash[0]) as f32, (p[1] * r * squash[1]) as f32, (p[2] * r * squash[2]) as f32]
        })
        .collect();
    MeshData { points: Arc::new(points), triangles: Arc::new(tris), smooth: true }
}

/// Add a new object to the scene; it becomes the selection.
pub fn add(doc: &mut Doc, what: Make, seed: &mut u64) -> u64 {
    doc.checkpoint();
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let p = &mut doc.project;
    let grey = |p: &mut Project, name: &str| own_material(p, name, plain([0.72, 0.72, 0.72]));
    let name = what.label();
    use Make::*;
    let (shape, m, material) = match what {
        Ground => {
            let mat = material_named(p, "Ground", || plain([0.46, 0.5, 0.36]));
            (Shape::Plane, sized([1.0; 3], [0.0, 0.0, 0.0]), Some(mat))
        }
        Water => {
            let mat = material_named(p, "Water", || {
                json!({"flags": 65536, "channels": {
                    "diffuse_color": [0.12, 0.3, 0.5], "ambient_color": [1.0, 1.0, 1.0], "specular_color": [1.0, 1.0, 1.0],
                    "specular_halo": [0.9, 0.9, 0.9], "diffusion": 0.6, "ambience": 0.15, "specularity": 0.9, "reflection": 0.55 }})
            });
            (Shape::Plane, sized([1.0; 3], [0.0, 0.25, 0.0]), Some(mat))
        }
        Cloud => {
            let mat = material_named(p, "Cloud Layer", || {
                json!({"flags": 65536, "channels": {
                    "diffuse_color": [1.0, 1.0, 1.0], "ambient_color": [1.0, 1.0, 1.0], "diffusion": 1.0, "ambience": 0.6, "transparency": 0.5 }})
            });
            (Shape::Plane, sized([1.0; 3], [0.0, 12.0, 0.0]), Some(mat))
        }
        Terrain | Lattice => {
            let t = terrain_shape(128, *seed, what == Lattice);
            let n = t.n as f64;
            // 8 units square; heights 0..65535 become 0..2 units
            let (sx, sy) = (8.0 / n, 2.0 / 65535.0);
            let y = if what == Lattice { 2.0 } else { 0.0 + 32767.5 * sy };
            let mat = grey(p, name);
            (Shape::Terrain(t), sized([sx, sy, sx], [0.0, y, 0.0]), Some(mat))
        }
        Stone => {
            let mat = own_material(p, "Stone", plain([0.5, 0.47, 0.43]));
            (Shape::Mesh(stone_mesh(*seed)), sized([1.0; 3], [0.0, 0.7, 0.0]), Some(mat))
        }
        Sphere => (Shape::Sphere, sized([1.0; 3], [0.0, 1.0, 0.0]), Some(grey(p, name))),
        Ellipsoid => (Shape::Sphere, sized([1.0, 0.5, 1.0], [0.0, 0.5, 0.0]), Some(grey(p, name))),
        Egg => (Shape::Sphere, sized([0.5, 1.0, 0.5], [0.0, 1.0, 0.0]), Some(grey(p, name))),
        Torus => (Shape::Torus { tube: 0.25, ring: 0.75 }, sized([1.0; 3], [0.0, 0.25, 0.0]), Some(grey(p, name))),
        Cylinder => (Shape::Cylinder, sized([1.0; 3], [0.0, 1.0, 0.0]), Some(grey(p, name))),
        Puck => (Shape::Cylinder, sized([1.0, 0.5, 1.0], [0.0, 0.5, 0.0]), Some(grey(p, name))),
        Rod => (Shape::Cylinder, sized([0.5, 1.0, 0.5], [0.0, 1.0, 0.0]), Some(grey(p, name))),
        Cube => (Shape::Cube, sized([2.0; 3], [0.0, 1.0, 0.0]), Some(grey(p, name))),
        Brick => (Shape::Cube, sized([2.0, 1.0, 2.0], [0.0, 0.5, 0.0]), Some(grey(p, name))),
        Tower => (Shape::Cube, sized([1.0, 2.0, 1.0], [0.0, 1.0, 0.0]), Some(grey(p, name))),
        Pyramid => (Shape::Pyramid, sized([2.0; 3], [0.0, 1.0, 0.0]), Some(grey(p, name))),
        FlatPyramid => (Shape::Pyramid, sized([2.0, 1.0, 2.0], [0.0, 0.5, 0.0]), Some(grey(p, name))),
        Obelisk => (Shape::Pyramid, sized([1.0, 2.0, 1.0], [0.0, 1.0, 0.0]), Some(grey(p, name))),
        Cone => (Shape::Cone, sized([1.0; 3], [0.0, 1.0, 0.0]), Some(grey(p, name))),
        FlatCone => (Shape::Cone, sized([1.0, 0.5, 1.0], [0.0, 0.5, 0.0]), Some(grey(p, name))),
        Spike => (Shape::Cone, sized([0.5, 1.0, 0.5], [0.0, 1.0, 0.0]), Some(grey(p, name))),
        // flat faces stand upright, facing the front
        Disc => (Shape::Disc, math::from_axes([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [0.0, 1.0, 0.0]), Some(grey(p, name))),
        Square => (Shape::Square, math::from_axes([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [0.0, 1.0, 0.0]), Some(grey(p, name))),
        Radial => (Shape::Light(Light { kind: LightKind::Radial, ..Default::default() }), sized([0.5; 3], [0.0, 4.0, 0.0]), None),
        Spot => (Shape::Light(Light { kind: LightKind::Spot, ..Default::default() }), sized([1.0; 3], [0.0, 5.0, 0.0]), None),
        SquareSpot => (Shape::Light(Light { kind: LightKind::SquareSpot, ..Default::default() }), sized([2.0; 3], [0.0, 5.0, 0.0]), None),
        Parallel => (Shape::Light(Light { kind: LightKind::Parallel, switches: 0x1500, ..Default::default() }), sized([1.0; 3], [0.0, 5.0, 0.0]), None),
    };
    let id = p.add(name, shape, m, material);
    doc.selection = vec![id];
    doc.touch();
    id
}

/// Add a mesh read from a model file, sized to about two units and set on the ground.
pub fn add_mesh(doc: &mut Doc, name: &str, mesh: &crate::mesh_io::Mesh) -> u64 {
    doc.checkpoint();
    let (lo, hi) = mesh.bounds().unwrap_or(([0.0; 3], [1.0; 3]));
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(hi[2] - lo[2]).max(1e-6) as f64;
    let k = 2.5 / span;
    let centre = [(lo[0] + hi[0]) as f64 * 0.5, lo[1] as f64, (lo[2] + hi[2]) as f64 * 0.5];
    let m = sized([k; 3], [-centre[0] * k, -centre[1] * k, -centre[2] * k]);
    let mat = own_material(&mut doc.project, name, plain([0.72, 0.72, 0.72]));
    let data = MeshData { points: Arc::new(mesh.points.clone()), triangles: Arc::new(mesh.triangles.clone()), smooth: true };
    let id = doc.project.add(name, Shape::Mesh(data), m, Some(mat));
    doc.selection = vec![id];
    doc.touch();
    id
}

/// A new, empty scene the way the app opens: a ground plane under a clear sky.
pub fn new_project() -> Project {
    let mut p = Project::default();
    p.sky = default_sky();
    let ground = material_named(&mut p, "Ground", || plain([0.46, 0.5, 0.36]));
    p.add("Ground Plane", Shape::Plane, math::IDENTITY, Some(ground));
    p.cam.eye = [9.0, 4.0, -12.0];
    p.cam.pivot = [0.0, 1.0, 0.0];
    p.cam.look_at([0.0, 1.0, 0.0]);
    p
}

/// The sky a new scene starts with: {"sky": …, "sun": …} in the renderer's form.
pub fn default_sky() -> Value {
    json!({
        "sky": {
            "flags": 131,
            "sky_colour": [0.2, 0.3, 0.9], "horizon_colour": [1.0, 1.0, 1.0], "plain_colour": [0.2, 0.3, 0.9],
            "dome_colour": [0.0, 0.0, 0.0], "ambient_colour": [1.0, 1.0, 1.0], "ambience": 1.0, "shadows": 0.1,
            "haze_colour": [1.0, 1.0, 1.0], "haze": 0.4, "cloud_height": 9.765625,
            "discs": [1.0, 4.0, 1.0, 1.0, 1.0, 5.0, 2.5, 100.0, 0.3, 0.1405408347024, 0.1051042352657, 2.0, 0.0, 2.0, 0.0]
        },
        "sun": { "direction": [0.7427813527082074, 0.5570860145311556, -0.3713906763541037], "colour": [1.0, 1.0, 1.0], "intensity": 1.0, "switches": 4195585 }
    })
}

// --------------------------------------------------------------------------- trees and figures

struct MeshBuild {
    points: Vec<[f32; 3]>,
    tris: Vec<[u32; 3]>,
}

impl MeshBuild {
    fn new() -> Self {
        MeshBuild { points: Vec::new(), tris: Vec::new() }
    }
    /// A tapered tube from `a` (radius `ra`) to `b` (radius `rb`).
    fn tube(&mut self, a: V3, b: V3, ra: f64, rb: f64, sides: usize) {
        let axis = math::unit(math::sub(b, a));
        let helper = if axis[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
        let u = math::unit(math::cross(axis, helper));
        let v = math::cross(axis, u);
        let base = self.points.len() as u32;
        for (c, r) in [(a, ra), (b, rb)] {
            for k in 0..sides {
                let t = std::f64::consts::TAU * k as f64 / sides as f64;
                let p = math::add(c, math::add(math::mul(u, r * t.cos()), math::mul(v, r * t.sin())));
                self.points.push([p[0] as f32, p[1] as f32, p[2] as f32]);
            }
        }
        let s = sides as u32;
        for k in 0..s {
            let (i, j) = (base + k, base + (k + 1) % s);
            self.tris.push([i, j, j + s]);
            self.tris.push([i, j + s, i + s]);
        }
    }
    /// A small lumpy ball of leaves.
    fn clump(&mut self, c: V3, r: f64, rng: &mut impl FnMut() -> f64) {
        let base = self.points.len() as u32;
        let (rows, cols) = (5usize, 8usize);
        for i in 0..=rows {
            let lat = std::f64::consts::PI * i as f64 / rows as f64;
            for j in 0..cols {
                let lon = std::f64::consts::TAU * j as f64 / cols as f64;
                let rr = r * (0.78 + 0.4 * rng());
                let p = [c[0] + rr * lat.sin() * lon.cos(), c[1] + rr * lat.cos() * 0.85, c[2] + rr * lat.sin() * lon.sin()];
                self.points.push([p[0] as f32, p[1] as f32, p[2] as f32]);
            }
        }
        for i in 0..rows as u32 {
            for j in 0..cols as u32 {
                let a = base + i * cols as u32 + j;
                let b = base + i * cols as u32 + (j + 1) % cols as u32;
                self.tris.push([a, a + cols as u32, b]);
                self.tris.push([b, a + cols as u32, b + cols as u32]);
            }
        }
    }
    fn done(self) -> MeshData {
        MeshData { points: Arc::new(self.points), triangles: Arc::new(self.tris), smooth: true }
    }
}

/// A tree: branching wood and clumps of leaves, two meshes grouped.
pub fn add_tree(doc: &mut Doc, seed: &mut u64) -> u64 {
    doc.checkpoint();
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let mut s = *seed | 1;
    let mut rng = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f64 / (1u64 << 53) as f64
    };
    let (mut wood, mut leaves) = (MeshBuild::new(), MeshBuild::new());
    // grow: each branch splits into two or three thinner ones
    let mut stack = vec![([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.25f64, 0.13f64, 0usize)];
    while let Some((from, dir, len, r, depth)) = stack.pop() {
        let to = math::add(from, math::mul(dir, len));
        let r_end = r * 0.68;
        wood.tube(from, to, r, r_end, if depth < 2 { 7 } else { 5 });
        if depth >= 4 || r_end < 0.012 {
            leaves.clump(to, 0.32 + 0.18 * rng(), &mut rng);
            continue;
        }
        let kids = if rng() < 0.4 { 3 } else { 2 };
        let spin = rng() * std::f64::consts::TAU;
        for k in 0..kids {
            let around = spin + std::f64::consts::TAU * k as f64 / kids as f64;
            let tilt = 0.45 + 0.35 * rng();
            let side = [around.cos(), 0.0, around.sin()];
            let next = math::unit(math::add(math::mul(dir, tilt.cos()), math::mul(side, tilt.sin())));
            let next = math::unit([next[0], next[1] + 0.25, next[2]]);
            stack.push((to, next, len * (0.66 + 0.12 * rng()), r_end, depth + 1));
        }
    }
    let p = &mut doc.project;
    let bark = material_named(p, "Bark", || plain([0.32, 0.22, 0.14]));
    let green = material_named(p, "Leaves", || plain([0.24, 0.42, 0.16]));
    let a = p.add("Tree Trunk", Shape::Mesh(wood.done()), math::IDENTITY, Some(bark));
    let b = p.add("Tree Leaves", Shape::Mesh(leaves.done()), math::IDENTITY, Some(green));
    doc.selection = vec![a, b];
    let g = doc.group_selection();
    if let Some(g) = g {
        let name = doc_name(&doc.project, "Tree");
        if let Some(n) = doc.project.node_mut(g) {
            n.name = name;
        }
        doc.selection = vec![g];
    }
    doc.undo_discard();
    doc.touch();
    g.unwrap_or(a)
}

fn doc_name(p: &Project, base: &str) -> String {
    p.name_for(base)
}

/// A standing human figure built from rounded parts, in gold.
pub fn add_figure(doc: &mut Doc) -> u64 {
    doc.checkpoint();
    let p = &mut doc.project;
    let gold = material_named(p, "Figure Gold", || {
        json!({"flags": 65536, "channels": {
            "diffuse_color": [0.85, 0.62, 0.2], "ambient_color": [1.0, 0.9, 0.7], "specular_color": [1.0, 0.95, 0.8], "specular_halo": [0.85, 0.85, 0.85],
            "diffusion": 0.7, "ambience": 0.2, "specularity": 0.9, "reflection": 0.25, "metallicity": 0.7 }})
    });
    let ball = |c: V3, r: V3| sized(r, c);
    let limb = |a: V3, b: V3, r: f64| {
        // a cylinder's own axis runs y -1..1: point it from a to b
        let mid = math::mul(math::add(a, b), 0.5);
        let half = math::mul(math::sub(b, a), 0.5);
        let y = half;
        let helper = if math::unit(y)[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
        let x = math::mul(math::unit(math::cross(helper, y)), r);
        let z = math::mul(math::unit(math::cross(x, y)), r);
        math::from_axes(x, y, z, mid)
    };
    let mut parts: Vec<(&str, Shape, M12)> = vec![
        ("Head", Shape::Sphere, ball([0.0, 1.66, 0.0], [0.1, 0.12, 0.11])),
        ("Neck", Shape::Cylinder, limb([0.0, 1.47, 0.0], [0.0, 1.57, 0.0], 0.045)),
        ("Chest", Shape::Sphere, ball([0.0, 1.3, 0.0], [0.19, 0.2, 0.11])),
        ("Waist", Shape::Sphere, ball([0.0, 1.06, 0.0], [0.15, 0.16, 0.09])),
        ("Hips", Shape::Sphere, ball([0.0, 0.92, 0.0], [0.17, 0.11, 0.1])),
    ];
    for side in [-1.0, 1.0] {
        let sh = [side * 0.21, 1.42, 0.0];
        let el = [side * 0.5, 1.5, 0.0];
        let wr = [side * 0.78, 1.56, 0.0];
        let hip = [side * 0.1, 0.9, 0.0];
        let kn = [side * 0.2, 0.5, 0.02];
        let an = [side * 0.3, 0.08, 0.0];
        parts.push(("Shoulder", Shape::Sphere, ball(sh, [0.06, 0.06, 0.06])));
        parts.push(("Upper Arm", Shape::Cylinder, limb(sh, el, 0.045)));
        parts.push(("Elbow", Shape::Sphere, ball(el, [0.045, 0.045, 0.045])));
        parts.push(("Forearm", Shape::Cylinder, limb(el, wr, 0.037)));
        parts.push(("Hand", Shape::Sphere, ball([side * 0.85, 1.57, 0.0], [0.07, 0.035, 0.045])));
        parts.push(("Thigh", Shape::Cylinder, limb(hip, kn, 0.07)));
        parts.push(("Knee", Shape::Sphere, ball(kn, [0.06, 0.06, 0.06])));
        parts.push(("Shin", Shape::Cylinder, limb(kn, an, 0.05)));
        parts.push(("Foot", Shape::Sphere, ball([side * 0.31, 0.04, -0.06], [0.05, 0.035, 0.11])));
    }
    let ids: Vec<u64> = parts.into_iter().map(|(n, s, m)| p.add(n, s, m, Some(gold))).collect();
    doc.selection = ids.clone();
    let g = doc.group_selection();
    if let Some(g) = g {
        let name = doc_name(&doc.project, "Figure");
        if let Some(n) = doc.project.node_mut(g) {
            n.name = name;
        }
        doc.selection = vec![g];
    }
    doc.undo_discard();
    doc.touch();
    g.unwrap_or(ids[0])
}
