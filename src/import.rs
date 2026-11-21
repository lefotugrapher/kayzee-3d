//! Reading saved scene files (.br3, .br5) into the app's own model. The file
//! is read by the renderer's reader; its export is then re-keyed into the
//! form the renderer's description takes, so what is opened renders the same.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use serde_json::{Map, Value, json};
use stecyk3d::scene::Scene;
use stecyk3d::scene_export;
use stecyk3d::scene_file::Flavour;
use stecyk3d::version::RenderAs;

use crate::math::{self, Cam, M12};
use crate::model::{self, Light, LightKind, Material, MeshData, Node, Project, Role, Shape, Terrain};

const CHANNELS: [&str; 13] = [
    "diffuse_color", "ambient_color", "specular_color", "specular_halo", "transparent_color", "diffusion", "ambience", "specularity", "transparency",
    "reflection", "bump", "metallicity", "volume_density",
];

pub fn version_of(path: &Path) -> (RenderAs, Flavour, usize, &'static str) {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("br5")) {
        (RenderAs::Bryce50, Flavour::Bryce5, 2, "5.0")
    } else {
        (RenderAs::Bryce30, Flavour::Bryce3d, 0, "3.0")
    }
}

fn f(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64)
}

fn nums(v: Option<&Value>) -> Vec<f64> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default()
}

fn has_null(v: &Value) -> bool {
    v.is_null() || v.as_array().is_some_and(|a| a.iter().any(Value::is_null))
}

fn no_nulls(v: &Value) -> Value {
    match v {
        Value::Null => json!(0.0),
        Value::Array(a) => Value::Array(a.iter().map(|x| if x.is_null() { json!(0.0) } else { x.clone() }).collect()),
        other => other.clone(),
    }
}

fn noise(n: &Value) -> Value {
    json!({ "type": n["kind"], "method": n["method"], "octaves": n["count"], "dimension": n["dims"] })
}
fn phase(p: &Value) -> Value {
    json!({ "amount": p["amount"], "noise": noise(&p["noise"]), "angles": p["rotation"], "frequencies": p["frequency"] })
}
fn filter(v: &Value) -> Value {
    json!({ "number": v["kind"], "n": v["n"], "a": v["a"], "b": v["b"] })
}
fn colours(c: &Value) -> Value {
    let hex = |v: &Value| format!("{:06x}", v.as_u64().unwrap_or(0));
    json!({ "mode": c["mode"], "space": c["space"], "a": hex(&c["first"]), "b": hex(&c["second"]), "c": hex(&c["third"]) })
}
fn component(c: &Value) -> Value {
    json!({
        "outputs": c["flags"], "noise": noise(&c["noise"]), "angles": c["rotation"], "frequencies": c["frequency"],
        "phase": phase(&c["phase"]), "filter": filter(&c["filter"]), "colours": colours(&c["colors"]),
    })
}

fn texture(index: usize, t: &Value, pictures: &[Value]) -> Value {
    let name = format!("t{index:04}:{}", t["name"].as_str().unwrap_or(""));
    let mut out = json!({ "name": name, "kind": t["kind"], "mapping": t["mapping"] });
    if t["kind"] == "picture" {
        let p = t["picture"].as_u64().and_then(|i| pictures.get(i as usize)).cloned().unwrap_or(Value::Null);
        let raw = model::b64_decode(p["argb_base64"].as_str().unwrap_or(""));
        let pixels: Vec<String> = raw.chunks_exact(4).map(|c| format!("{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])).collect();
        out["picture"] = json!({ "width": p["width"], "height": p["height"], "pixels": pixels });
        return out;
    }
    let r = &t["record"];
    out["texture"] = json!({
        "outputs": r["outputs"], "blend": r["blend"], "saved": r["saved"], "colours": colours(&r["colors"]),
        "filter": filter(&r["filter"]), "phase": phase(&r["phase"]),
        "components": r["components"].as_array().map(|a| a.iter().map(component).collect::<Vec<_>>()).unwrap_or_default(),
    });
    out
}

fn channel(ch: &Value, tex_names: &[String]) -> Value {
    let mut out = json!({ "a": no_nulls(&ch["a"]), "switches": ch["word"] });
    for key in ["b", "alpha"] {
        if !has_null(&ch[key]) {
            out[key] = ch[key].clone();
        }
    }
    if let Some(refs) = ch["refs"].as_array() {
        for (key, r) in ["a_tex", "b_tex", "alpha_tex"].iter().zip(refs) {
            if let Some(name) = r.as_u64().and_then(|i| tex_names.get(i as usize)) {
                out[*key] = json!(name);
            }
        }
    }
    out
}

fn material(m: &Value, tex_names: &[String]) -> Value {
    let mut channels = Map::new();
    for c in CHANNELS {
        if !m["channels"][c].is_null() {
            channels.insert(c.to_owned(), channel(&m["channels"][c], tex_names));
        }
    }
    let mut out = json!({
        "flags": m["flags"], "channels": channels, "refraction": m["refraction"],
        "volume_colour": no_nulls(&m["volume_color"]), "volume_numbers": no_nulls(&m["volume_numbers"]),
    });
    let mix = &m["mix"];
    if mix.is_object() {
        let side = |list: &Value, zero: Value| -> Vec<Value> {
            list.as_array().map(|a| a.iter().map(|c| if c.is_null() { zero.clone() } else { channel(c, tex_names) }).collect()).unwrap_or_default()
        };
        out["mix"] = json!({ "amounts": mix["amounts"], "colors": side(&mix["colors"], json!([0.0, 0.0, 0.0])), "values": side(&mix["values"], json!(0.0)) });
    }
    out
}

fn is_plain(m: &Value) -> bool {
    m["channels"].as_object().is_none_or(|c| c.values().all(|ch| ch["refs"].as_array().is_none_or(|r| r.iter().all(Value::is_null))))
}

fn sky_and_sun(doc: &Value, program: &str) -> Value {
    let s = &doc["sky"];
    let mut sky = json!({ "flags": s["flags"] });
    let pairs = [
        ("cloud_height", "cloud_height"), ("cover", "cover"), ("cover_amplitude", "amplitude"), ("cloud_frequency", "frequency"),
        ("shadow_lift", "shadows"), ("ambient_amount", "ambience"), ("haze", "haze"), ("fog_height", "fog_height"), ("fog", "fog"),
        ("sky5_lit_air", "lit_air"), ("cumulus_color", "cloud_colour"), ("stratus_color", "upper_cloud_colour"), ("zenith", "sky_colour"),
        ("horizon", "horizon_colour"), ("ambient", "dome_colour"), ("plain", "plain_colour"), ("ambient_color", "ambient_colour"),
        ("haze_color", "haze_colour"), ("fog_color", "fog_colour"), ("cloud_offset", "cloud_offset"), ("moon", "moon"), ("discs", "discs"), ("stars", "stars"),
        ("stratus_texture", "stratus_texture"), ("cumulus_texture", "cumulus_texture"),
    ];
    for (src, dst) in pairs {
        if !s[src].is_null() {
            sky[dst] = s[src].clone();
        }
    }
    if program != "5.0" {
        let d = nums(doc["edge_noise"].get(program));
        sky["detail"] = json!(d.first().copied().unwrap_or(0.0));
        sky["detail_z"] = json!(d.get(1).copied().unwrap_or(0.0));
    }
    if s["sky5"].as_bool() == Some(true) || s["sky5"].is_object() {
        sky["bryce5_part"] = json!(true);
        let five = [
            ("sky5_flags2", "second_flags"), ("sky5_haze_offset", "haze_offset"), ("sky5_haze_thickness", "haze_thickness"), ("sky5_fog_offset", "fog_offset"),
            ("sky5_haze_shift", "haze_shift_colour"), ("sky5_sun_glow_size", "sun_glow_weight"), ("sky5_moon_glow_size", "moon_glow_weight"),
            ("sky5_sun_disc", "sun_disc_weight"), ("sky5_moon_disc", "moon_disc_weight"), ("sky5_star_intensity", "star_intensity"),
            ("sky5_comet_intensity", "comet_intensity"), ("sky5_star_orientation", "star_orientation"), ("sky5_air_steps", "air_steps"),
        ];
        for (src, dst) in five {
            if !s[src].is_null() {
                sky[dst] = s[src].clone();
            }
        }
        let d = nums(Some(&s["sky5_cloud_detail"]));
        if d.len() == 2 {
            sky["cloud_detail"] = json!(d[0]);
            sky["cloud_detail_z"] = json!(d[1]);
        }
    }
    let sun = json!({
        "colour": s["sun_color"], "glow_colour": s["sun_glow_color"], "light_colour": s["sun_light_color"],
        "direction": s["sun_direction_bryce"], "intensity": s["sun_intensity"], "switches": s["sun_flags"],
    });
    json!({ "sky": sky, "sun": sun })
}

fn camera(doc: &Value, vi: usize, size: [u32; 2]) -> (Cam, bool) {
    let c = &doc["camera"];
    let views = c["views"].as_array().cloned().unwrap_or_default();
    let view = views.get(vi).filter(|v| !v.is_null()).or_else(|| views.iter().find(|v| !v.is_null())).cloned().unwrap_or(Value::Null);
    let v3 = |v: &Value, d: [f64; 3]| {
        let n = nums(Some(v));
        if n.len() == 3 { [n[0], n[1], n[2]] } else { d }
    };
    let mut cam = Cam::default();
    let forward = math::unit(v3(&view["forward"], [0.0, 0.0, 1.0]));
    let up = v3(&view["up"], [0.0, 1.0, 0.0]);
    let centre = v3(&view["centre"], [0.0, 3.0, -10.0]);
    cam.forward = forward;
    cam.up = math::unit(math::sub(up, math::mul(forward, math::dot(up, forward))));
    cam.right = math::unit(math::cross(cam.up, cam.forward));
    cam.fov = f(c.get("field_of_view")).unwrap_or(1.0472);
    let positive = |v: f64| if v > 0.0 { v } else { 1.0 };
    let zoom = nums(c.get("zoom"));
    cam.zoom = [positive(*zoom.first().unwrap_or(&1.0)), positive(*zoom.get(1).unwrap_or(&1.0))];
    let pan = nums(c.get("pan"));
    cam.pan = [*pan.first().unwrap_or(&0.0), *pan.get(1).unwrap_or(&0.0)];
    cam.aspect = positive(f(c.get("aspect")).unwrap_or(1.0));
    cam.scale = if vi == 0 { positive(f(c.get("scale")).unwrap_or(1.0)) } else { 1.0 };
    // the eye stands a hair behind the image plane's centre
    let half = (0.8 * cam.fov / 2.0).tan() * cam.zoom[0];
    let plane_h = cam.scale * 1e-5 * cam.zoom[1];
    let plane_w = plane_h * (size[0] as f64 / size[1].max(1) as f64) * (cam.aspect * cam.zoom[0] / cam.zoom[1]);
    let dist = if half > 0.0 { (plane_w / 2.0) / half } else { 0.0 };
    cam.eye = math::sub(centre, math::mul(forward, dist));
    let word = c["projection_word"].as_u64().unwrap_or(0x4000);
    let panorama = word & 0x10000 != 0;
    (cam, panorama)
}

/// Read a scene file into a project, with notes on anything left behind.
pub fn open(path: &Path) -> Result<(Project, Vec<String>), String> {
    let (version, flavour, vi, program) = version_of(path);
    let bytes = std::fs::read(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let scene = Scene::read(&bytes, flavour).map_err(|e| format!("{e}"))?;
    let text = scene_export::document_json_as(&scene, Some(version)).write();
    let doc: Value = serde_json::from_str(&text).map_err(|e| format!("The scene could not be unpacked: {e}"))?;
    let (mut p, mut notes) = from_document(&doc, vi, program)?;
    bake_motion(&scene, version, vi, &doc, &mut p, &mut notes);
    Ok((p, notes))
}

/// The placement of every object (by its place in the file) and the camera, from an export.
fn poses(doc: &Value, vi: usize, size: [u32; 2]) -> (HashMap<u64, M12>, Cam) {
    let mut out = HashMap::new();
    for o in doc["objects"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let worlds = o["worlds"].as_array().cloned().unwrap_or_default();
        let w = nums(worlds.get(vi).filter(|w| !w.is_null()).or_else(|| worlds.iter().find(|w| !w.is_null())));
        if let (12, Some(i)) = (w.len(), o["index"].as_u64()) {
            let mut m = math::IDENTITY;
            m.copy_from_slice(&w);
            out.insert(i, m);
        }
    }
    (out, camera(doc, vi, size).0)
}

/// A saved scene's motion, sampled frame by frame into keys on the objects and the camera.
fn bake_motion(scene: &Scene, version: RenderAs, vi: usize, first: &Value, p: &mut Project, notes: &mut Vec<String>) {
    let (length, fps) = (scene.settings.animation_length as f64, scene.settings.frame_rate as f64);
    if !(length > 0.05 && fps >= 1.0 && length.is_finite()) {
        return;
    }
    p.anim.fps = fps.round();
    p.anim.duration = length;
    p.anim.range = [0.0, length];
    p.anim.time = (scene.settings.current_time as f64).clamp(0.0, length);
    p.anim.linear = true;
    // the node made from each file object, in the order they were made
    let mut node_of: HashMap<u64, u64> = HashMap::new();
    {
        let mut ids = p.nodes.iter().map(|n| n.id);
        for o in first["objects"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let kind = o["kind"].as_str().unwrap_or("");
            let placed = o["worlds"].as_array().is_some_and(|w| w.iter().any(|x| !x.is_null()));
            let kept = match kind {
                "camera" => false,
                "group" => true,
                "sphere" | "cube" | "cylinder" | "cone" | "pyramid" | "plane" | "square" | "slab" | "torus" | "light" => placed,
                "terrain" | "mesh" => placed,
                _ => false,
            };
            if !kept {
                continue;
            }
            if let (Some(i), Some(id)) = (o["index"].as_u64(), ids.next()) {
                node_of.insert(i, id);
            }
        }
    }
    let frames = (length * fps).round() as usize + 1;
    let step = frames.div_ceil(120).max(1);
    let mut tracks: HashMap<u64, Vec<model::Key<M12>>> = HashMap::new();
    let mut cams: Vec<model::Key<Cam>> = Vec::new();
    let mut k = 0;
    while k < frames {
        let t = (k as f64 / fps).min(length);
        let at = scene.at_time(t);
        let text = scene_export::document_json_as(&at, Some(version)).write();
        let Ok(doc) = serde_json::from_str::<Value>(&text) else { break };
        let (worlds, cam) = poses(&doc, vi, p.size);
        for (index, m) in worlds {
            if let Some(id) = node_of.get(&index) {
                tracks.entry(*id).or_default().push(model::Key { t, v: m });
            }
        }
        cams.push(model::Key { t, v: cam });
        k += step;
    }
    let differs = |a: &M12, b: &M12| a.iter().zip(b).any(|(x, y)| (x - y).abs() > 1e-9);
    let mut moving = 0;
    for (id, keys) in tracks {
        if keys.len() > 1 && keys.iter().any(|k| differs(&k.v, &keys[0].v)) {
            if let Some(n) = p.node_mut(id) {
                // a mismatch between the file's order and ours would move the wrong object: check the pose at the saved time
                n.keys = keys;
                moving += 1;
            }
        }
    }
    let cam_moves = cams.len() > 1 && cams.iter().any(|c| c.v.eye != cams[0].v.eye || c.v.forward != cams[0].v.forward || c.v.fov != cams[0].v.fov);
    if cam_moves {
        p.anim.camera_keys = cams;
    }
    if moving > 0 || cam_moves {
        notes.push(format!("Motion read from the file: {moving} moving objects{}.", if cam_moves { " and the camera" } else { "" }));
        let now = p.anim.time;
        crate::anim::pose(p, now);
    }
}

pub fn from_document(doc: &Value, vi: usize, program: &str) -> Result<(Project, Vec<String>), String> {
    let mut notes = Vec::new();
    let mut p = Project::default();
    p.program = program.to_owned();
    let size = nums(doc.get("picture_size"));
    if size.len() == 2 && size[0] >= 1.0 && size[1] >= 1.0 {
        p.size = [size[0] as u32, size[1] as u32];
    }

    let empty = Vec::new();
    let pictures = doc["pictures"].as_array().unwrap_or(&empty);
    let textures = doc["textures"].as_array().unwrap_or(&empty);
    let tex_names: Vec<String> = textures.iter().enumerate().map(|(i, t)| format!("t{i:04}:{}", t["name"].as_str().unwrap_or(""))).collect();
    p.textures = textures.iter().enumerate().map(|(i, t)| texture(i, t, pictures)).collect();
    let mats = doc["materials"].as_array().unwrap_or(&empty);
    p.materials = mats
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let name = m["name"].as_str().filter(|s| !s.is_empty()).map(str::to_owned).unwrap_or_else(|| format!("Material {}", i + 1));
            Material { name, record: material(m, &tex_names), keys: Vec::new() }
        })
        .collect();

    p.sky = sky_and_sun(doc, program);
    let scene_flags = doc["sky"]["scene_flags"].as_u64().unwrap_or(959);
    p.extra = json!({ "switches": scene_flags });
    let aa = doc["settings"]["antialias"].as_u64().unwrap_or(0);
    p.render.antialias = if aa >= 2 { 2 } else if scene_flags & 0x2000 != 0 { 1 } else { 0 };
    p.render.spatial = doc["settings"]["spatial"].as_u64().unwrap_or(0) > 0;
    let (cam, panorama) = camera(doc, vi, p.size);
    p.cam = cam;
    p.render.panorama = panorama;

    // objects: first pass makes nodes, second links members to their groups
    let objects = doc["objects"].as_array().unwrap_or(&empty);
    let mut by_index: HashMap<u64, u64> = HashMap::new();
    let mut latest_material: Option<usize> = None;
    let mut pending: Vec<(u64, u64)> = Vec::new();
    for o in objects {
        let kind = o["kind"].as_str().unwrap_or("");
        let own_material = o["material"].as_u64().map(|m| m as usize).filter(|m| *m < p.materials.len());
        if kind == "camera" {
            continue;
        }
        let worlds = o["worlds"].as_array().cloned().unwrap_or_default();
        let world = worlds.get(vi).filter(|w| !w.is_null()).or_else(|| worlds.iter().find(|w| !w.is_null()));
        let w = nums(world);
        let name = o["name"].as_str().unwrap_or("").to_owned();
        let mut m: M12 = math::IDENTITY;
        if w.len() == 12 {
            m.copy_from_slice(&w);
        } else if kind != "group" {
            notes.push(format!("{kind} \"{name}\" has no placement in the file and was left out."));
            if own_material.is_some() {
                latest_material = own_material;
            }
            continue;
        }
        let mut material = own_material;
        let shape = match kind {
            "sphere" => Shape::Sphere,
            "cube" => Shape::Cube,
            "cylinder" => Shape::Cylinder,
            "cone" => Shape::Cone,
            "pyramid" => Shape::Pyramid,
            "plane" => Shape::Plane,
            "square" => Shape::Square,
            "slab" => {
                notes.push(format!("slab \"{name}\" is shown as an infinite plane."));
                Shape::Plane
            }
            "torus" => {
                let t = nums(o.get("torus"));
                if t.len() == 2 { Shape::Torus { tube: t[0], ring: t[1] } } else { Shape::Torus { tube: 0.25, ring: 0.75 } }
            }
            "terrain" => {
                let t = &o["terrain"];
                let n = t["size"].as_u64().unwrap_or(0) as usize;
                let heights = model::heights_from_b64(t["heights_base64"].as_str().unwrap_or(""));
                if n < 2 || heights.len() != n * n {
                    notes.push(format!("terrain \"{name}\" holds no readable heights and was left out."));
                    continue;
                }
                Shape::Terrain(Terrain {
                    n,
                    heights: Arc::new(heights),
                    mirrored: t["mirrored"].as_bool().unwrap_or(false),
                    smooth: t["smooth"].as_bool().unwrap_or(true),
                    lower: f(t.get("lower")).unwrap_or(-1e-5),
                    upper: f(t.get("upper")).unwrap_or(1.00001),
                    faces: t["faces"].as_u64().unwrap_or(0) as u32,
                    solid_faces: t["solid_faces"].as_u64().unwrap_or(0) as u32,
                })
            }
            "mesh" => {
                let mesh = &o["mesh"];
                let points: Vec<[f32; 3]> = mesh["points"].as_array().map(|a| a.iter().map(|q| { let n = nums(Some(q)); [n.first().copied().unwrap_or(0.0) as f32, n.get(1).copied().unwrap_or(0.0) as f32, n.get(2).copied().unwrap_or(0.0) as f32] }).collect()).unwrap_or_default();
                let triangles: Vec<[u32; 3]> = mesh["triangles"].as_array().map(|a| a.iter().filter_map(|t| { let n = t.as_array()?; Some([n.first()?.as_u64()? as u32, n.get(1)?.as_u64()? as u32, n.get(2)?.as_u64()? as u32]) }).collect()).unwrap_or_default();
                if triangles.is_empty() {
                    notes.push(format!("mesh \"{name}\" holds no triangles and was left out."));
                    continue;
                }
                let smooth = mesh["flags"].as_array().is_some_and(|a| a.iter().any(|v| v.as_u64().unwrap_or(0) & 1 != 0));
                Shape::Mesh(MeshData { points: Arc::new(points), triangles: Arc::new(triangles), smooth })
            }
            "light" => {
                let l = &o["light"];
                let beam = l["beam"].as_str().unwrap_or("radial");
                let mut colour = nums(l.get("color"));
                colour.resize(3, 1.0);
                // a light shines through its material: a plain one tints it, a textured one is a gel
                let mat = own_material.or(if beam != "radial" { latest_material } else { None });
                material = None;
                if let Some(mi) = mat {
                    if is_plain(&mats[mi]) {
                        let d = nums(mats[mi]["channels"]["diffuse_color"].get("a"));
                        for k in 0..3 {
                            colour[k] *= d.get(k).copied().unwrap_or(1.0);
                        }
                    } else {
                        material = Some(mi);
                    }
                }
                let edge = nums(l.get("edge"));
                Shape::Light(Light {
                    kind: LightKind::from_word(beam),
                    colour: [colour[0] as f32, colour[1] as f32, colour[2] as f32],
                    intensity: f(l.get("intensity")).unwrap_or(5.0),
                    switches: l["flags"].as_u64().unwrap_or(0x1100) as u32,
                    exponents: [edge.first().copied().unwrap_or(1.0), edge.get(1).copied().unwrap_or(1.0)],
                })
            }
            "group" => Shape::Group,
            other => {
                notes.push(format!("{other} \"{name}\" is a kind of object this app does not hold yet and was left out."));
                if own_material.is_some() {
                    latest_material = own_material;
                }
                continue;
            }
        };
        let id = p.fresh_id();
        let flags: Vec<u32> = nums(o.get("placement_flags")).iter().map(|v| *v as u32).collect();
        let label = if name.is_empty() { p.name_for(shape.label()) } else { name };
        p.nodes.push(Node {
            id,
            name: label,
            shape,
            m,
            material,
            group: None,
            role: match o["role"].as_str() {
                Some("POSITIVE") => Role::Positive,
                Some("NEGATIVE") => Role::Negative,
                Some("INTERSECT") => Role::Intersect,
                _ => Role::Neutral,
            },
            cut_positive: o["cut_shows_positive"].as_bool().unwrap_or(false),
            hidden: flags.get(1).is_some_and(|w| w & 1 != 0),
            locked: false,
            family: 0,
            as_box: false,
            link: None,
            track: None,
            keys: Vec::new(),
            flags: [flags.first().copied().unwrap_or(0), flags.get(1).copied().unwrap_or(0) & !1, flags.get(2).copied().unwrap_or(0)],
        });
        if let Some(i) = o["index"].as_u64() {
            by_index.insert(i, id);
        }
        if let Some(g) = o["group"].as_u64() {
            pending.push((id, g));
        }
        if own_material.is_some() {
            latest_material = own_material;
        }
    }
    for (id, g) in pending {
        if let (Some(group), Some(n)) = (by_index.get(&g).copied(), p.nodes.iter_mut().find(|n| n.id == id)) {
            n.group = Some(group);
        }
    }
    // give each group a box around its members
    let mut d = crate::state::Doc::new(p);
    d.refit_groups();
    let mut p = d.project;
    p.cam.pivot = {
        let ids: Vec<u64> = p.nodes.iter().filter(|n| !matches!(n.shape, Shape::Plane)).map(|n| n.id).collect();
        p.world_bounds(&ids).map(|(lo, hi)| math::mul(math::add(lo, hi), 0.5)).unwrap_or([0.0, 0.0, 0.0])
    };
    Ok((p, notes))
}

/// Textures of a library or scene export in the renderer's form, with their names in order.
pub fn textures_of(list: &[Value], pictures: &[Value]) -> (Vec<Value>, Vec<String>) {
    let out: Vec<Value> = list.iter().enumerate().map(|(i, t)| texture(i, t, pictures)).collect();
    let names = out.iter().map(|t| t["name"].as_str().unwrap_or("").to_owned()).collect();
    (out, names)
}

/// A material of a library or scene export in the renderer's form.
pub fn material_of(m: &Value, tex_names: &[String]) -> Value {
    material(m, tex_names)
}
