//! Ready-made example scenes, built in code.

use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::math::{self, M12, V3};
use crate::model::{Project, Role, Shape, Terrain};
use crate::state::Doc;
use crate::terrain_ops::{self as ops, Fractal};
use crate::{create, fileio};

fn at(size: V3, centre: V3) -> M12 {
    math::from_axes([size[0], 0.0, 0.0], [0.0, size[1], 0.0], [0.0, 0.0, size[2]], centre)
}

fn chrome() -> Value {
    json!({"flags": 65536, "channels": {
        "diffuse_color": [0.78, 0.8, 0.9], "ambient_color": [1.0, 1.0, 1.0], "specular_color": [1.0, 1.0, 1.0], "specular_halo": [0.92, 0.92, 0.92],
        "diffusion": 0.12, "ambience": 0.08, "specularity": 1.0, "reflection": 0.92, "metallicity": 0.15 }})
}

fn glass() -> Value {
    json!({"flags": 65536, "refraction": 1.33, "channels": {
        "diffuse_color": [0.8, 0.9, 1.0], "ambient_color": [1.0, 1.0, 1.0], "specular_color": [1.0, 1.0, 1.0], "specular_halo": [0.95, 0.95, 0.95],
        "transparent_color": [0.86, 0.94, 1.0], "diffusion": 0.04, "ambience": 0.05, "specularity": 1.0, "transparency": 0.9, "reflection": 0.28 }})
}

/// A hill with stepped sides, so the light draws bands across it.
fn hill(seed: u64, steps: u32) -> Terrain {
    let mut g = ops::fractal(160, Fractal::Standard, seed, 5, 1.0);
    ops::edges(&mut g, 0.8, 0.0, true);
    ops::smooth_grid(&mut g, 0.9);
    ops::normalise(&mut g);
    ops::terrace(&mut g, steps, 0.35);
    ops::smooth_grid(&mut g, 0.12);
    Terrain { n: 160, heights: Arc::new(g.to_u16()), mirrored: false, smooth: true, lower: -1e-5, upper: 1.00001, faces: 0, solid_faces: 63 }
}

/// Still water under a clouded violet sky: a chrome arch half sunk, glass
/// balls adrift, banded hills, and a chrome skull hanging over the water.
/// `sky_from` may name a scene whose cloud layers are borrowed.
pub fn dream(sky_from: Option<&Path>) -> Project {
    let mut doc = Doc::new(create::new_project());
    let p = &mut doc.project;
    p.name = "Chrome Tide".into();
    p.size = [1000, 750];
    // the ground plane becomes the sea bed
    p.nodes[0].m = at([1.0; 3], [0.0, -3.0, 0.0]);
    p.materials[0].record = create::plain([0.1, 0.12, 0.3]);

    let water = create::own_material(p, "Still Water", json!({"flags": 65536, "channels": {
        "diffuse_color": [0.05, 0.16, 0.8], "ambient_color": [1.0, 1.0, 1.0], "specular_color": [1.0, 1.0, 1.0], "specular_halo": [0.9, 0.9, 0.9],
        "diffusion": 0.7, "ambience": 0.2, "specularity": 0.8, "reflection": 0.5 }}));
    p.add("Water", Shape::Plane, math::IDENTITY, Some(water));

    // hills along the horizon
    let earth = create::own_material(p, "Banded Earth", json!({"flags": 65536, "channels": {
        "diffuse_color": [0.5, 0.29, 0.15], "ambient_color": [1.0, 0.85, 0.9], "specular_color": [1.0, 0.9, 0.8], "specular_halo": [0.6, 0.6, 0.6],
        "diffusion": 1.0, "ambience": 0.2, "specularity": 0.1 }}));
    for (k, (x, z, w, h)) in [(-17.0, 30.0, 22.0, 3.4), (-3.0, 38.0, 16.0, 2.4), (9.0, 34.0, 18.0, 2.9), (21.0, 22.0, 24.0, 4.6), (-28.0, 20.0, 18.0, 2.6)].into_iter().enumerate() {
        let t = hill(40 + k as u64 * 7, 9 + k as u32 % 3);
        let n = t.n as f64;
        let m = math::from_axes([w / n, 0.0, 0.0], [0.0, h / 65535.0, 0.0], [0.0, 0.0, w / n], [x, 32767.5 * h / 65535.0 - 0.05, z]);
        p.add("Hill", Shape::Terrain(t), m, Some(earth));
    }

    // the arch: a ring stood on edge, more than half of it under water
    let metal = create::own_material(p, "Chrome", chrome());
    let ring = math::compose(&math::turn([0.0, 1.0, 0.0], 0.5), &math::turn([1.0, 0.0, 0.0], std::f64::consts::FRAC_PI_2));
    let mut arch = math::compose(&ring, &math::scaling([3.9, 3.9, 3.9]));
    arch[9] = -5.6;
    arch[10] = -0.3;
    arch[11] = 12.0;
    p.add("Arch", Shape::Torus { tube: 0.17, ring: 0.83 }, arch, Some(metal));

    // glass adrift
    let clear = create::own_material(p, "Glass", glass());
    for (x, y, z, r) in [(6.0, 4.3, 9.0, 1.45), (-1.2, 5.6, 11.0, 0.42), (2.6, 6.5, 12.0, 0.36), (-2.4, 4.6, 10.0, 0.2), (8.3, 6.9, 13.0, 0.26)] {
        p.add("Glass Ball", Shape::Sphere, at([r, r, r], [x, y, z]), Some(clear));
    }

    // the skull: rounded forms joined, with the hollows cut out
    let hollow = create::own_material(p, "Hollow", json!({"flags": 65536, "channels": {
        "diffuse_color": [0.05, 0.03, 0.1], "ambient_color": [1.0, 1.0, 1.0], "diffusion": 0.6, "ambience": 0.15, "reflection": 0.25, "specularity": 0.4, "specular_color": [0.6, 0.6, 1.0], "specular_halo": [0.7, 0.7, 0.7] }}));
    let mut parts: Vec<(u64, Role)> = Vec::new();
    let mut part = |p: &mut Project, name: &str, shape: Shape, m: M12, mat: usize, role: Role| {
        let id = p.add(name, shape, m, Some(mat));
        parts.push((id, role));
    };
    use Role::{Negative as Cut, Positive as Solid};
    part(p, "Cranium", Shape::Sphere, at([0.96, 0.96, 1.15], [0.0, 0.3, 0.12]), metal, Solid);
    part(p, "Face", Shape::Sphere, at([0.74, 0.8, 0.62], [0.0, -0.46, -0.5]), metal, Solid);
    part(p, "Cheek", Shape::Sphere, at([0.24, 0.2, 0.3], [0.52, -0.38, -0.7]), metal, Solid);
    part(p, "Cheek", Shape::Sphere, at([0.24, 0.2, 0.3], [-0.52, -0.38, -0.7]), metal, Solid);
    part(p, "Jaw", Shape::Sphere, at([0.52, 0.34, 0.46], [0.0, -1.04, -0.5]), metal, Solid);
    part(p, "Eye Socket", Shape::Sphere, at([0.27, 0.25, 0.45], [0.37, -0.08, -0.95]), hollow, Cut);
    part(p, "Eye Socket", Shape::Sphere, at([0.27, 0.25, 0.45], [-0.37, -0.08, -0.95]), hollow, Cut);
    part(p, "Nose", Shape::Pyramid, at([0.3, 0.42, 0.6], [0.0, -0.56, -1.0]), hollow, Cut);
    part(p, "Mouth", Shape::Cube, at([0.92, 0.035, 0.5], [0.0, -1.0, -0.86]), hollow, Cut);
    for k in -3..=3 {
        part(p, "Tooth Gap", Shape::Cube, at([0.022, 0.3, 0.5], [k as f64 * 0.125, -1.0, -0.88]), hollow, Cut);
    }
    doc.selection = parts.iter().map(|x| x.0).collect();
    let skull = doc.group_selection();
    for (id, role) in &parts {
        if let Some(n) = doc.project.node_mut(*id) {
            n.role = *role;
            n.cut_positive = false;
        }
    }
    if let Some(g) = skull {
        if let Some(n) = doc.project.node_mut(g) {
            n.name = "Skull".into();
        }
        doc.selection = vec![g];
        // bigger, turned a little, hung over the water
        let pose = math::compose(&math::translation([1.7, 3.7, 5.6]), &math::compose(&math::turn([0.0, 1.0, 0.0], -0.3), &math::compose(&math::turn([1.0, 0.0, 0.0], 0.1), &math::scaling([1.75, 1.75, 1.75]))));
        doc.transform_selection(&pose);
        doc.refit_groups();
    }
    let p = &mut doc.project;

    // sky: violet overhead, pale lilac at the horizon, a high sun off to the right
    let mut sky = create::default_sky();
    sky["sky"]["flags"] = json!(0x8B);
    sky["sky"]["sky_colour"] = json!([0.05, 0.08, 0.72]);
    sky["sky"]["horizon_colour"] = json!([0.56, 0.5, 1.0]);
    sky["sky"]["dome_colour"] = json!([0.1, 0.06, 0.2]);
    sky["sky"]["haze"] = json!(0.16);
    sky["sky"]["haze_colour"] = json!([0.86, 0.8, 1.0]);
    sky["sky"]["shadows"] = json!(0.25);
    sky["sun"]["direction"] = json!([0.5, 0.62, -0.6]);
    sky["sun"]["colour"] = json!([1.0, 0.96, 1.0]);
    // cloud layers come from another scene's sky, when one is given
    if let Some(other) = sky_from.and_then(|f| fileio::open(f).ok()) {
        let mut any = false;
        for key in ["stratus_texture", "cumulus_texture"] {
            if let Some(t) = other.sky["sky"][key].as_u64().and_then(|i| other.textures.get(i as usize)) {
                let mut t = t.clone();
                t["name"] = json!(format!("cloud{}:{key}", p.textures.len()));
                p.textures.push(t);
                sky["sky"][key] = json!(p.textures.len() - 1);
                any = true;
            }
        }
        if any {
            sky["sky"]["flags"] = json!(0x8B | 0x10 | 0x20 | 0x40);
            for (key, v) in [("cover", json!(0.38)), ("amplitude", json!(1.0)), ("frequency", json!(0.25)), ("cloud_height", json!(12.0)), ("cloud_colour", json!([1.0, 0.94, 1.0])), ("upper_cloud_colour", json!([0.95, 0.9, 1.0])), ("cloud_offset", json!([0.0, 0.0]))] {
                sky["sky"][key] = v;
            }
        }
    }
    p.sky = sky;
    p.render.antialias = 1;
    p.cam.eye = [0.6, 3.4, -13.0];
    p.cam.pivot = [0.5, 2.6, 6.0];
    p.cam.look_at([0.4, 2.3, 6.0]);
    p.cam.fov = 60f64.to_radians() / 0.8 * 0.72;
    doc.selection.clear();
    doc.project
}

/// Trees and a figure close up (used to check the generated shapes).
pub fn grove() -> Project {
    let mut doc = Doc::new(create::new_project());
    let mut seed = 99u64;
    for (x, z) in [(-2.6, 3.0), (2.8, 4.5), (0.4, 7.0)] {
        create::add_tree(&mut doc, &mut seed);
        doc.move_selection([x, 0.0, z]);
    }
    create::add_figure(&mut doc);
    doc.move_selection([0.0, 0.0, 0.0]);
    let p = &mut doc.project;
    p.size = [800, 520];
    p.cam.eye = [0.5, 1.6, -5.5];
    p.cam.look_at([0.3, 1.3, 2.0]);
    p.render.antialias = 1;
    doc.selection.clear();
    doc.project
}
