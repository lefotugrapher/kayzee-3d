//! Writing the scene as the renderer's description (JSON text).

use serde_json::{Map, Value, json};

use crate::math::{self, V3};
use crate::model::{LightKind, Project, Role, Shape};

fn obj_name(id: u64) -> String {
    format!("o{id}")
}

fn mat_name(i: usize) -> String {
    format!("m{i}")
}

/// A material record as the renderer takes it: the flag bit the renderer
/// refuses on textured surfaces (and never reads) is dropped.
fn material_for_render(record: &Value) -> Value {
    let mut r = record.clone();
    if let Some(flags) = r.get("flags").and_then(Value::as_u64) {
        r["flags"] = json!(flags & !0x40);
    }
    if r.get("channels").is_none() {
        r["channels"] = json!({});
    }
    r
}

fn camera(p: &Project, size: (u32, u32)) -> Value {
    let c = &p.cam;
    let half = (0.8 * c.fov / 2.0).tan();
    let angle = if c.zoom[0] == 1.0 { 0.8 * c.fov } else { 2.0 * (half * c.zoom[0]).atan() };
    let mut out = json!({
        "position": c.eye, "forward": c.forward, "up": c.up,
        "frame_angle": angle.clamp(1e-4, 3.1),
        "pan": c.pan,
    });
    let pixel_aspect = c.aspect * c.zoom[0] / c.zoom[1];
    // the picture may be rendered at another shape than the document's; keep the document's framing
    let doc_ratio = p.size[0] as f64 / p.size[1].max(1) as f64;
    let out_ratio = size.0 as f64 / size.1.max(1) as f64;
    let shape = pixel_aspect * doc_ratio / out_ratio;
    if (shape - 1.0).abs() > 1e-9 {
        out["pixel_aspect"] = json!(shape);
    }
    if p.program == "3.0" && (c.scale * c.zoom[1] - 1.0).abs() > 1e-12 {
        out["plane_height"] = json!(c.scale * 1e-5 * c.zoom[1]);
    }
    if p.render.panorama {
        out["projection"] = json!("panorama");
        out.as_object_mut().unwrap().remove("frame_angle");
    }
    out
}

fn smooth_normals(points: &[V3], tris: &[[usize; 3]]) -> Vec<V3> {
    let mut n = vec![[0.0; 3]; points.len()];
    for t in tris {
        let face = math::cross(math::sub(points[t[1]], points[t[0]]), math::sub(points[t[2]], points[t[0]]));
        for k in t {
            n[*k] = math::add(n[*k], face);
        }
    }
    n.into_iter().map(|v| if math::len(v) > 0.0 { math::unit(v) } else { [0.0, 1.0, 0.0] }).collect()
}

/// Which outputs (1 value, 2 colour, 4 bump) the materials ask of each texture.
fn asked_outputs(p: &Project) -> std::collections::HashMap<String, u64> {
    let mut asked = std::collections::HashMap::new();
    let mut note = |key: &str, ch: &Value| {
        if ch["switches"].as_u64().unwrap_or(0) == 0 {
            return;
        }
        let want = match key {
            "diffuse_color" | "ambient_color" | "specular_color" | "specular_halo" | "transparent_color" => 2,
            "bump" => 4,
            _ => 1,
        };
        for (slot, bits) in [("a_tex", want), ("b_tex", want), ("alpha_tex", 1)] {
            if let Some(name) = ch[slot].as_str() {
                *asked.entry(name.to_owned()).or_insert(0) |= bits;
            }
        }
    };
    const COLOURS: [&str; 5] = ["diffuse_color", "ambient_color", "specular_color", "specular_halo", "transparent_color"];
    const VALUES: [&str; 8] = ["diffusion", "ambience", "specularity", "transparency", "reflection", "bump", "metallicity", "volume_density"];
    for m in &p.materials {
        if let Some(channels) = m.record["channels"].as_object() {
            for (key, ch) in channels {
                note(key, ch);
            }
        }
        for (list, names) in [("colors", &COLOURS[..]), ("values", &VALUES[..])] {
            if let Some(side) = m.record["mix"][list].as_array() {
                for (ch, key) in side.iter().zip(names) {
                    note(key, ch);
                }
            }
        }
    }
    asked
}

/// Textures with every asked output switched on (in the texture and its components).
fn textures_filled(p: &Project) -> Vec<Value> {
    let asked = asked_outputs(p);
    p.textures
        .iter()
        .map(|t| {
            let Some(want) = t["name"].as_str().and_then(|n| asked.get(n)).copied() else { return t.clone() };
            if !t["texture"].is_object() {
                return t.clone();
            }
            let mut t = t.clone();
            let have = t["texture"]["outputs"].as_u64().unwrap_or(0);
            if have & want == want {
                return t;
            }
            t["texture"]["outputs"] = json!(have | want);
            if let Some(components) = t["texture"]["components"].as_array_mut() {
                for c in components {
                    let own = c["outputs"].as_u64().unwrap_or(0);
                    c["outputs"] = json!(own | want);
                }
            }
            t
        })
        .collect()
}

/// The whole scene as a description, to be rendered `size` pixels large.
/// `antialias`: 0 none, 1 normal (edges redone), 2 fine (16 rays a pixel).
pub fn description(p: &Project, size: (u32, u32), antialias: u8) -> String {
    description_marked(p, size, antialias, &[])
}

/// As `description`; the objects in `marked` are flagged as selected (what a mask render shows).
pub fn description_marked(p: &Project, size: (u32, u32), antialias: u8, marked: &[u64]) -> String {
    let mut switches = p.extra.get("switches").and_then(Value::as_u64).unwrap_or(959);
    switches &= !0x2000;
    let (aa, spatial) = match antialias {
        0 => (0, 0),
        1 => {
            switches |= 0x2000;
            (0, 0)
        }
        _ => (2, 0),
    };
    let spatial = if p.render.spatial { 2 } else { spatial };

    let mut materials = Map::new();
    for (i, m) in p.materials.iter().enumerate() {
        materials.insert(mat_name(i), material_for_render(&m.record));
    }

    let visible = |id: u64| p.node(id).is_some_and(|n| !n.hidden);
    let mut objects: Vec<Value> = Vec::new();
    let mut lights: Vec<Value> = Vec::new();
    for n in &p.nodes {
        if n.hidden {
            continue;
        }
        let name = obj_name(n.id);
        let mut entry = json!({ "name": name, "world": n.m });
        if marked.contains(&n.id) {
            entry["placement_flags"] = json!([n.flags[0] | 1, n.flags[1], n.flags[2]]);
        }
        if let Some(i) = n.material.filter(|i| *i < p.materials.len()) {
            entry["material"] = json!(mat_name(i));
        }
        match &n.shape {
            Shape::Sphere => entry["shape"] = json!("sphere"),
            Shape::Cube => entry["shape"] = json!("cube"),
            Shape::Cylinder => entry["shape"] = json!("cylinder"),
            Shape::Cone => entry["shape"] = json!("cone"),
            Shape::Pyramid => entry["shape"] = json!("pyramid"),
            Shape::Plane => entry["shape"] = json!("plane"),
            Shape::Square => entry["shape"] = json!("square"),
            Shape::Disc => entry["shape"] = json!("disc"),
            Shape::Torus { tube, ring } => {
                entry["shape"] = json!("torus");
                entry["torus"] = json!([tube, ring]);
            }
            Shape::Terrain(t) => {
                if t.n < 2 || t.heights.len() != t.n * t.n {
                    continue;
                }
                entry["shape"] = json!("terrain");
                entry["terrain"] = json!({
                    "size": t.n, "heights": &*t.heights, "lower": t.lower, "upper": t.upper,
                    "mirrored": t.mirrored, "smooth": t.smooth, "faces": t.faces, "solid_faces": t.solid_faces,
                });
            }
            Shape::Mesh(mesh) => {
                let pts: Vec<V3> = mesh.points.iter().map(|q| math::place(&n.m, [q[0] as f64, q[1] as f64, q[2] as f64])).collect();
                let flip = math::dot(math::axis(&n.m, 0), math::cross(math::axis(&n.m, 1), math::axis(&n.m, 2))) < 0.0;
                let tris: Vec<[usize; 3]> = mesh
                    .triangles
                    .iter()
                    .filter(|t| t.iter().all(|k| (*k as usize) < pts.len()))
                    .map(|t| if flip { [t[0] as usize, t[2] as usize, t[1] as usize] } else { [t[0] as usize, t[1] as usize, t[2] as usize] })
                    .collect();
                if tris.is_empty() {
                    continue;
                }
                entry.as_object_mut().unwrap().remove("world");
                entry["shape"] = json!("mesh");
                entry["triangles"] = Value::Array(tris.iter().map(|t| json!([pts[t[0]], pts[t[1]], pts[t[2]]])).collect());
                if mesh.smooth {
                    entry["normals"] = json!(smooth_normals(&pts, &tris));
                    entry["uvs"] = json!([]);
                    entry["corner_normals"] = json!(tris);
                    entry["corner_uvs"] = Value::Array(vec![Value::Null; tris.len()]);
                    entry["flags"] = json!(vec![1; tris.len()]);
                    entry["origin"] = json!(math::origin(&n.m));
                }
            }
            Shape::Group => {
                let mut members = Map::new();
                for m in p.nodes.iter().filter(|m| m.group == Some(n.id) && visible(m.id)) {
                    // an empty inner group would be refused; leave it out
                    if matches!(m.shape, Shape::Group) && !p.nodes.iter().any(|x| x.group == Some(m.id) && !x.hidden) {
                        continue;
                    }
                    let role = match m.role {
                        Role::Neutral => "NEUTRAL",
                        Role::Positive => "POSITIVE",
                        Role::Negative => "NEGATIVE",
                        Role::Intersect => "INTERSECT",
                    };
                    members.insert(obj_name(m.id), json!({ "role": role, "cut_shows_positive": m.cut_positive }));
                }
                if members.is_empty() {
                    continue;
                }
                objects.push(json!({ "name": name, "shape": "group", "members": members }));
                continue;
            }
            Shape::Light(l) => {
                let mut e = json!({
                    "name": name, "kind": l.kind.word(), "colour": l.colour, "intensity": l.intensity,
                    "switches": l.switches | 0x100, "position": math::origin(&n.m), "world": n.m,
                });
                match l.kind {
                    LightKind::Spot | LightKind::Parallel => e["exponents"] = json!([l.exponents[0]]),
                    LightKind::SquareSpot | LightKind::SquareParallel => e["exponents"] = json!(l.exponents),
                    LightKind::Radial => {}
                }
                if let Some(i) = n.material.filter(|i| *i < p.materials.len()) {
                    e["material"] = json!(mat_name(i));
                }
                lights.push(e);
                continue;
            }
        }
        objects.push(entry);
    }
    // a member whose group was left out stands on its own: nothing to do, groups are named by the group

    let sky = p.sky.get("sky").cloned().unwrap_or_else(|| json!({}));
    let sun = p.sky.get("sun").cloned().unwrap_or_else(|| json!({"direction": [0.5, 0.7, -0.5], "colour": [1, 1, 1], "intensity": 1.0, "switches": 4195585}));
    let text = json!({
        "size": [size.0, size.1],
        "render": { "version": p.program, "units": 1.0, "antialias": aa, "spatial": spatial, "switches": switches, "preview": false },
        "camera": camera(p, size),
        "sky": sky,
        "sun": sun,
        "textures": if p.render.fill_outputs { textures_filled(p) } else { p.textures.clone() },
        "materials": materials,
        "objects": objects,
        "lights": lights,
    });
    text.to_string()
}
