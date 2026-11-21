//! The editable scene: objects, camera, sky, materials, animation. This is the
//! app's own model and file format (JSON); imported scene files are turned
//! into it and the renderer's description is written from it.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::math::{self, Cam, M12, V3};

pub const FORMAT: u32 = 1;
pub const DOTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Neutral,
    Positive,
    Negative,
    Intersect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightKind {
    Radial,
    Spot,
    SquareSpot,
    Parallel,
    SquareParallel,
}

impl LightKind {
    pub const ALL: [LightKind; 5] = [LightKind::Radial, LightKind::Spot, LightKind::SquareSpot, LightKind::Parallel, LightKind::SquareParallel];
    pub fn label(self) -> &'static str {
        match self {
            LightKind::Radial => "Radial Light",
            LightKind::Spot => "Spotlight",
            LightKind::SquareSpot => "Square Spotlight",
            LightKind::Parallel => "Parallel Light",
            LightKind::SquareParallel => "Square Parallel Light",
        }
    }
    /// The renderer's word for the kind.
    pub fn word(self) -> &'static str {
        match self {
            LightKind::Radial => "radial",
            LightKind::Spot => "cone",
            LightKind::SquareSpot => "pyramid",
            LightKind::Parallel => "cylinder",
            LightKind::SquareParallel => "box",
        }
    }
    pub fn from_word(w: &str) -> LightKind {
        match w {
            "cone" => LightKind::Spot,
            "pyramid" => LightKind::SquareSpot,
            "cylinder" => LightKind::Parallel,
            "box" => LightKind::SquareParallel,
            _ => LightKind::Radial,
        }
    }
}

/// Switch bits of a light.
pub const LIGHT_ON: u32 = 0x100;
pub const LIGHT_VISIBLE: u32 = 0x200;
pub const LIGHT_NO_FALLOFF: u32 = 0x400;
pub const LIGHT_NO_SHADOWS: u32 = 0x800;
pub const LIGHT_SQUARED: u32 = 0x8000_0000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Light {
    pub kind: LightKind,
    pub colour: [f32; 3],
    pub intensity: f64,
    /// The renderer's switch word (on, fall-off, shadows).
    pub switches: u32,
    /// Edge softness of a beam: 0 is a hard edge.
    pub exponents: [f64; 2],
}

impl Default for Light {
    fn default() -> Self {
        Light { kind: LightKind::Radial, colour: [1.0, 1.0, 1.0], intensity: 5.0, switches: 0x1100, exponents: [1.0, 1.0] }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Terrain {
    pub n: usize,
    #[serde(with = "heights_b64")]
    pub heights: Arc<Vec<u16>>,
    pub mirrored: bool,
    pub smooth: bool,
    /// Heights outside lower..upper (0..1 of the full range) are cut away.
    pub lower: f64,
    pub upper: f64,
    /// Closing faces drawn (bits: bottom, top, four sides).
    #[serde(default)]
    pub faces: u32,
    /// Faces that make it a solid for boolean work.
    #[serde(default)]
    pub solid_faces: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MeshData {
    #[serde(with = "points_b64")]
    pub points: Arc<Vec<[f32; 3]>>,
    #[serde(with = "tris_b64")]
    pub triangles: Arc<Vec<[u32; 3]>>,
    pub smooth: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Shape {
    Sphere,
    Cube,
    Cylinder,
    Cone,
    Pyramid,
    Torus { tube: f64, ring: f64 },
    /// An infinite plane (ground, water or cloud level).
    Plane,
    /// A flat square face.
    Square,
    /// A flat round face.
    Disc,
    Terrain(Terrain),
    Mesh(MeshData),
    Light(Light),
    Group,
}

impl Shape {
    pub fn label(&self) -> &'static str {
        match self {
            Shape::Sphere => "Sphere",
            Shape::Cube => "Cube",
            Shape::Cylinder => "Cylinder",
            Shape::Cone => "Cone",
            Shape::Pyramid => "Pyramid",
            Shape::Torus { .. } => "Torus",
            Shape::Plane => "Infinite Plane",
            Shape::Square => "Square",
            Shape::Disc => "Disc",
            Shape::Terrain(t) if t.mirrored => "Symmetric Lattice",
            Shape::Terrain(_) => "Terrain",
            Shape::Mesh(_) => "Mesh",
            Shape::Light(_) => "Light",
            Shape::Group => "Group",
        }
    }

    /// The own-space box that encloses the shape.
    pub fn bounds(&self) -> (V3, V3) {
        match self {
            Shape::Cube | Shape::Pyramid => ([-0.5; 3], [0.5; 3]),
            Shape::Sphere | Shape::Cylinder | Shape::Cone | Shape::Light(_) => ([-1.0; 3], [1.0; 3]),
            Shape::Torus { tube, ring } => ([-(ring + tube), -tube.abs(), -(ring + tube)], [ring + tube, tube.abs(), ring + tube]),
            Shape::Plane => ([-1e6, -1e-3, -1e6], [1e6, 1e-3, 1e6]),
            Shape::Square | Shape::Disc => ([-1.0, 0.0, -1.0], [1.0, 0.0, 1.0]),
            Shape::Terrain(t) => {
                let h = t.n as f64 / 2.0;
                if t.mirrored { ([-h, -65535.0, -h], [h, 65535.0, h]) } else { ([-h, -32767.5, -h], [h, 32767.5, h]) }
            }
            Shape::Mesh(m) => {
                let mut lo = [f64::INFINITY; 3];
                let mut hi = [f64::NEG_INFINITY; 3];
                for p in m.points.iter() {
                    for k in 0..3 {
                        lo[k] = lo[k].min(p[k] as f64);
                        hi[k] = hi[k].max(p[k] as f64);
                    }
                }
                if lo[0] > hi[0] { ([-1.0; 3], [1.0; 3]) } else { (lo, hi) }
            }
            Shape::Group => ([-0.5; 3], [0.5; 3]),
        }
    }
}

/// One keyframe of a track: a time (seconds) and the value held then.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Key<T> {
    pub t: f64,
    pub v: T,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: u64,
    pub name: String,
    pub shape: Shape,
    pub m: M12,
    /// Index into the project's materials.
    pub material: Option<usize>,
    /// The group this object is a member of.
    pub group: Option<u64>,
    pub role: Role,
    /// A negative member's cut shows the positive's surface.
    #[serde(default)]
    pub cut_positive: bool,
    pub hidden: bool,
    pub locked: bool,
    pub family: u8,
    /// Shown as its bounding box only.
    #[serde(default)]
    pub as_box: bool,
    /// The object this one follows (its placement is carried along).
    #[serde(default)]
    pub link: Option<u64>,
    /// Where this object sits in its link parent's own space (set when linked).
    #[serde(default)]
    pub link_rest: Option<M12>,
    /// The object this one keeps facing.
    #[serde(default)]
    pub track: Option<u64>,
    #[serde(default)]
    pub keys: Vec<Key<M12>>,
    /// Flag words read from a scene file, passed back unchanged.
    #[serde(default)]
    pub flags: [u32; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// The renderer's own material record (channels, numbers, flags).
    pub record: Value,
    #[serde(default)]
    pub keys: Vec<Key<Value>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderOpts {
    /// 0 none, 1 normal, 2 fine
    pub antialias: u8,
    pub spatial: bool,
    /// "" for the lit picture, or mask / distance / altitude
    pub mode: String,
    pub panorama: bool,
    /// Render size as a multiple of the document size.
    pub scale: f64,
    /// Where a surface asks a texture for something the texture was not set
    /// to make (the original left stale values there), make the texture
    /// produce it, so the scene renders instead of being refused.
    #[serde(default = "yes")]
    pub fill_outputs: bool,
    /// Picture effects applied after rendering.
    #[serde(default)]
    pub fx: Effects,
}

/// Finishing touches on a rendered picture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Effects {
    /// Film grain, 0..1.
    pub grain: f32,
    /// Colour wash over the picture and how strong it is (0..1).
    pub tint: [f32; 3],
    pub tint_amount: f32,
    /// -1 (grey) .. 1 (vivid).
    pub saturation: f32,
    /// -1 (flat) .. 1 (punchy).
    pub contrast: f32,
    /// Darkened corners, 0..1.
    pub vignette: f32,
}

impl Default for Effects {
    fn default() -> Self {
        Effects { grain: 0.0, tint: [0.75, 0.55, 1.0], tint_amount: 0.0, saturation: 0.0, contrast: 0.0, vignette: 0.0 }
    }
}

impl Effects {
    pub fn is_none(&self) -> bool {
        self.grain == 0.0 && self.tint_amount == 0.0 && self.saturation == 0.0 && self.contrast == 0.0 && self.vignette == 0.0
    }

    /// Apply to RGB levels, in place.
    pub fn apply(&self, w: u32, h: u32, rgb: &mut [u8]) {
        if self.is_none() {
            return;
        }
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let (cx, cy) = (w as f32 * 0.5, h as f32 * 0.5);
        let reach = (cx * cx + cy * cy).sqrt().max(1.0);
        for (i, px) in rgb.chunks_exact_mut(3).enumerate() {
            let (x, y) = ((i as u32 % w) as f32, (i as u32 / w) as f32);
            let mut c = [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0];
            let grey = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
            for k in 0..3 {
                c[k] = grey + (c[k] - grey) * (1.0 + self.saturation);
                c[k] = 0.5 + (c[k] - 0.5) * (1.0 + self.contrast);
                // a tint multiplies the light parts and lifts the shadows a little
                let tinted = c[k] * self.tint[k] + 0.08 * self.tint[k] * (1.0 - c[k]);
                c[k] += (tinted - c[k]) * self.tint_amount;
            }
            if self.vignette > 0.0 {
                let d = (((x - cx).powi(2) + (y - cy).powi(2)).sqrt() / reach).powi(2);
                for v in &mut c {
                    *v *= 1.0 - self.vignette * d;
                }
            }
            if self.grain > 0.0 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let n = ((seed >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.35 * self.grain;
                for v in &mut c {
                    *v += n;
                }
            }
            for k in 0..3 {
                px[k] = (c[k].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            }
        }
    }
}

fn yes() -> bool {
    true
}

impl Default for RenderOpts {
    fn default() -> Self {
        RenderOpts { antialias: 1, spatial: true, mode: String::new(), panorama: false, scale: 1.0, fill_outputs: true, fx: Effects::default() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Anim {
    pub fps: f64,
    /// Length in seconds.
    pub duration: f64,
    pub time: f64,
    pub auto_key: bool,
    /// Working area (seconds) that playback and movie rendering cover.
    pub range: [f64; 2],
    pub camera_keys: Vec<Key<Cam>>,
    pub sky_keys: Vec<Key<Value>>,
    /// 0 once, 1 repeat, 2 pendulum
    pub play_mode: u8,
    /// Keys are joined by straight lines (set for motion read from a scene
    /// file, which is sampled frame by frame); otherwise moves ease in and out.
    #[serde(default)]
    pub linear: bool,
}

impl Default for Anim {
    fn default() -> Self {
        Anim { fps: 15.0, duration: 4.0, time: 0.0, auto_key: true, range: [0.0, 4.0], camera_keys: Vec::new(), sky_keys: Vec::new(), play_mode: 1, linear: false }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub format: u32,
    pub name: String,
    pub size: [u32; 2],
    pub cam: Cam,
    pub view_dots: Vec<Option<Cam>>,
    /// The renderer's own sky record.
    pub sky: Value,
    pub sky_dots: Vec<Option<Value>>,
    pub nodes: Vec<Node>,
    pub materials: Vec<Material>,
    /// The renderer's own texture records, named by index from materials and the sky.
    pub textures: Vec<Value>,
    /// Pictures used by materials (the renderer's records).
    pub pictures: Vec<Value>,
    pub render: RenderOpts,
    pub anim: Anim,
    pub next_id: u64,
    pub family_names: Vec<String>,
    /// Members of the scene file this app passes back unchanged (edge noise and the like).
    #[serde(default)]
    pub extra: Value,
    /// "3.0", "3.1" or "5.0": which program's rules the renderer follows.
    #[serde(default = "three")]
    pub program: String,
}

fn three() -> String {
    "3.0".to_owned()
}

impl Default for Project {
    fn default() -> Self {
        Project {
            format: FORMAT,
            name: "Untitled".into(),
            size: [640, 480],
            cam: Cam::default(),
            view_dots: vec![None; DOTS],
            sky: json!({}),
            sky_dots: vec![None; DOTS],
            nodes: Vec::new(),
            materials: Vec::new(),
            textures: Vec::new(),
            pictures: Vec::new(),
            render: RenderOpts::default(),
            anim: Anim::default(),
            next_id: 1,
            family_names: (1..=12).map(|k| format!("Family {k}")).collect(),
            extra: Value::Null,
            program: three(),
        }
    }
}

impl Project {
    pub fn fresh_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn node(&self, id: u64) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }
    pub fn node_mut(&mut self, id: u64) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }
    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    /// The members of a group, direct only.
    pub fn members(&self, group: u64) -> Vec<u64> {
        self.nodes.iter().filter(|n| n.group == Some(group)).map(|n| n.id).collect()
    }

    /// A group and everything inside it, or just the object.
    pub fn with_descendants(&self, id: u64) -> Vec<u64> {
        let mut out = vec![id];
        let mut k = 0;
        while k < out.len() {
            let here = out[k];
            out.extend(self.members(here));
            // objects linked to this one are carried along with it
            for n in &self.nodes {
                if n.link == Some(here) && !out.contains(&n.id) {
                    out.push(n.id);
                }
            }
            k += 1;
            if out.len() > 100_000 {
                break;
            }
        }
        out
    }

    /// The outermost group holding the object (the thing a click selects).
    pub fn top(&self, id: u64) -> u64 {
        let mut here = id;
        let mut guard = 0;
        while let Some(g) = self.node(here).and_then(|n| n.group) {
            here = g;
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        here
    }

    /// A unique name for a new object of the given kind.
    pub fn name_for(&self, base: &str) -> String {
        let mut k = 1;
        loop {
            let name = format!("{base} {k}");
            if !self.nodes.iter().any(|n| n.name == name) {
                return name;
            }
            k += 1;
        }
    }

    pub fn add(&mut self, name: &str, shape: Shape, m: M12, material: Option<usize>) -> u64 {
        let id = self.fresh_id();
        let name = self.name_for(name);
        self.nodes.push(Node {
            id,
            name,
            shape,
            m,
            material,
            group: None,
            role: Role::Neutral,
            cut_positive: false,
            hidden: false,
            locked: false,
            family: 0,
            as_box: false,
            link: None,
            link_rest: None,
            track: None,
            keys: Vec::new(),
            flags: [0; 3],
        });
        id
    }

    /// Remove objects (and the members of removed groups); fix up references.
    pub fn remove(&mut self, ids: &[u64]) {
        let mut all: Vec<u64> = Vec::new();
        for id in ids {
            all.extend(self.with_descendants(*id));
        }
        self.nodes.retain(|n| !all.contains(&n.id));
        for n in &mut self.nodes {
            if n.link.is_some_and(|l| all.contains(&l)) {
                n.link = None;
            }
            if n.track.is_some_and(|l| all.contains(&l)) {
                n.track = None;
            }
        }
    }

    /// The world box around the given objects: (low corner, high corner).
    pub fn world_bounds(&self, ids: &[u64]) -> Option<(V3, V3)> {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let mut any = false;
        for id in ids {
            for d in self.with_descendants(*id) {
                let Some(n) = self.node(d) else { continue };
                if matches!(n.shape, Shape::Group | Shape::Plane) {
                    if matches!(n.shape, Shape::Plane) {
                        // an infinite plane counts as a small patch about its origin
                        let o = math::origin(&n.m);
                        for k in 0..3 {
                            lo[k] = lo[k].min(o[k] - if k == 1 { 0.0 } else { 4.0 });
                            hi[k] = hi[k].max(o[k] + if k == 1 { 0.0 } else { 4.0 });
                        }
                        any = true;
                    }
                    continue;
                }
                let (a, b) = node_box(n);
                for c in 0..8 {
                    let p = [if c & 1 == 0 { a[0] } else { b[0] }, if c & 2 == 0 { a[1] } else { b[1] }, if c & 4 == 0 { a[2] } else { b[2] }];
                    let w = math::place(&n.m, p);
                    for k in 0..3 {
                        lo[k] = lo[k].min(w[k]);
                        hi[k] = hi[k].max(w[k]);
                    }
                    any = true;
                }
            }
        }
        any.then_some((lo, hi))
    }
}

/// The object's own-space box, tightened to a terrain's real heights.
pub fn node_box(n: &Node) -> (V3, V3) {
    match &n.shape {
        Shape::Terrain(t) => {
            let h = t.n as f64 / 2.0;
            let (mut lo, mut hi) = (u16::MAX, u16::MIN);
            for v in t.heights.iter().step_by((t.heights.len() / 4096).max(1)) {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
            if t.mirrored {
                ([-h, -(hi as f64), -h], [h, hi as f64, h])
            } else {
                ([-h, lo as f64 - 32767.5, -h], [h, hi as f64 - 32767.5, h])
            }
        }
        s => s.bounds(),
    }
}

// --------------------------------------------------------------------------- values that change over time

/// Blend two JSON values of the same shape: numbers are blended, everything
/// else is taken from `a` (or from `b` once past the middle).
pub fn blend_value(a: &Value, b: &Value, t: f64) -> Value {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) if x != y => json!(x + (y - x) * t),
            _ => a.clone(),
        },
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => Value::Array(x.iter().zip(y).map(|(p, q)| blend_value(p, q, t)).collect()),
        (Value::Object(x), Value::Object(y)) => {
            let mut out = serde_json::Map::new();
            for (k, v) in x {
                out.insert(k.clone(), match y.get(k) {
                    Some(w) => blend_value(v, w, t),
                    None => v.clone(),
                });
            }
            Value::Object(out)
        }
        _ => if t < 0.5 { a.clone() } else { b.clone() },
    }
}

/// The keys either side of time `t` and how far between them it lies.
pub fn bracket<T>(keys: &[Key<T>], t: f64) -> Option<(&Key<T>, &Key<T>, f64)> {
    if keys.is_empty() {
        return None;
    }
    if t <= keys[0].t {
        return Some((&keys[0], &keys[0], 0.0));
    }
    let last = &keys[keys.len() - 1];
    if t >= last.t {
        return Some((last, last, 0.0));
    }
    let i = keys.iter().position(|k| k.t > t)?;
    let (a, b) = (&keys[i - 1], &keys[i]);
    let span = (b.t - a.t).max(1e-12);
    Some((a, b, (t - a.t) / span))
}

/// Put a key at time `t` (replacing one already there), keeping the order.
pub fn set_key<T>(keys: &mut Vec<Key<T>>, t: f64, v: T) {
    if let Some(k) = keys.iter_mut().find(|k| (k.t - t).abs() < 1e-6) {
        k.v = v;
        return;
    }
    let at = keys.iter().position(|k| k.t > t).unwrap_or(keys.len());
    keys.insert(at, Key { t, v });
}

pub fn remove_key<T>(keys: &mut Vec<Key<T>>, t: f64) -> bool {
    let before = keys.len();
    keys.retain(|k| (k.t - t).abs() >= 1e-6);
    keys.len() != before
}

/// A placement between two keys: position and sizes blended straight, the
/// turn blended along the shortest way.
pub fn blend_placement(a: &M12, b: &M12, t: f64) -> M12 {
    let (ta, tb) = (math::decompose(a), math::decompose(b));
    let mut d = ta;
    for k in 0..3 {
        d.size[k] = ta.size[k] + (tb.size[k] - ta.size[k]) * t;
        d.position[k] = ta.position[k] + (tb.position[k] - ta.position[k]) * t;
        let mut turn = tb.degrees[k] - ta.degrees[k];
        while turn > 180.0 {
            turn -= 360.0;
        }
        while turn < -180.0 {
            turn += 360.0;
        }
        d.degrees[k] = ta.degrees[k] + turn * t;
    }
    math::recompose(&d)
}

// --------------------------------------------------------------------------- compact storage of big arrays

fn b64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn b64_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

pub fn heights_to_b64(h: &[u16]) -> String {
    let bytes: Vec<u8> = h.iter().flat_map(|v| v.to_le_bytes()).collect();
    b64_encode(&bytes)
}

pub fn heights_from_b64(text: &str) -> Vec<u16> {
    b64_decode(text).chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

mod heights_b64 {
    use super::*;
    use serde::{Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &Arc<Vec<u16>>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&heights_to_b64(v))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<Vec<u16>>, D::Error> {
        let text = String::deserialize(d)?;
        Ok(Arc::new(heights_from_b64(&text)))
    }
}

mod points_b64 {
    use super::*;
    use serde::{Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &Arc<Vec<[f32; 3]>>, s: S) -> Result<S::Ok, S::Error> {
        let bytes: Vec<u8> = v.iter().flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes())).collect();
        s.serialize_str(&b64_encode(&bytes))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<Vec<[f32; 3]>>, D::Error> {
        let bytes = b64_decode(&String::deserialize(d)?);
        let f: Vec<f32> = bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        Ok(Arc::new(f.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect()))
    }
}

mod tris_b64 {
    use super::*;
    use serde::{Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &Arc<Vec<[u32; 3]>>, s: S) -> Result<S::Ok, S::Error> {
        let bytes: Vec<u8> = v.iter().flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes())).collect();
        s.serialize_str(&b64_encode(&bytes))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<Vec<[u32; 3]>>, D::Error> {
        let bytes = b64_decode(&String::deserialize(d)?);
        let f: Vec<u32> = bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        Ok(Arc::new(f.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect()))
    }
}
