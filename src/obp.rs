//! Object preset libraries (.obp): every preset with its objects, materials
//! and textures, read with the renderer's own part readers.
//!
//! A preset is one `objpresetdesc` (name), one `objprevcimage` (picture) and
//! one `C3dRootObjList`, all of the same identity. The list's rows name the
//! objects (by class and identity); a row with a linked list (a group, or an
//! object others are linked to) has its members in a `C3dObjList`, deeper
//! groups likewise. Each object's placement names its material by identity
//! (`C3dBaseMaterial`); the materials, their textures and pictures are read
//! with `scene_library::Library` (the material reader resolves every
//! texture). World transforms of linked objects are made with the engine's
//! own link arithmetic, as `Scene::read` does.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use stecyk3d::animation::LinkArithmetic;
use stecyk3d::scene::Material as EngineMaterial;
use stecyk3d::scene_beams::{BeamClass, BeamRecord};
use stecyk3d::scene_export::{self, mac_roman};
use stecyk3d::scene_file::{Container, Flavour, Resource, ResourceIndex};
use stecyk3d::scene_height_fields::{TERRAIN_PART_LEN, TerrainPart};
use stecyk3d::scene_library::{Library, LibraryKind, PresetData};
use stecyk3d::scene_meshes::{CornerUse, MeshResource};
use stecyk3d::scene_parts::{FIRST_PART_LEN, LightPart, first_part_name, order_of};
use stecyk3d::scene_pictures::decode_picture;
use stecyk3d::scene_records::{ByteOrder, FourCc, ObjectKind, PlacementPart, TORUS_PART_LEN, TorusPart, Transform};
use stecyk3d::version::RenderAs;

use crate::math::{self, M12};
use crate::model::{Light, LightKind, Material, MeshData, Node, Project, Role, Shape, Terrain};

/// One object preset, ready to merge into a project.
pub struct ObjPreset {
    pub name: String,
    /// (width, height, RGB levels): the preset's own small picture.
    pub thumb: Option<(u32, u32, Vec<u8>)>,
    /// `material` indexes `materials`; `group` names a node of this list; ids start at 1.
    pub nodes: Vec<Node>,
    pub materials: Vec<Material>,
    /// Texture records in the renderer's form, named "t0000:…" (per preset).
    pub textures: Vec<Value>,
    /// What could not be carried over, one line each.
    pub notes: Vec<String>,
}

impl ObjPreset {
    /// The preset as a project of its own, for `fileio::merge(&mut project, preset.project())`
    /// (which renames clashing texture names and renumbers ids and materials).
    pub fn project(&self) -> Project {
        let mut p = Project::default();
        p.nodes = self.nodes.clone();
        p.materials = self.materials.clone();
        p.textures = self.textures.clone();
        p.next_id = self.nodes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
        p
    }

    /// The world box of what the preset shows: as `Project::world_bounds`, but a terrain counts
    /// only the cells its lower/upper clip keeps (a clipped "blanket" is a small patch of a
    /// large grid). For aiming a camera at the preset.
    pub fn bounds(&self) -> Option<(math::V3, math::V3)> {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for n in self.nodes.iter().filter(|n| !n.hidden && !matches!(n.shape, Shape::Group | Shape::Plane)) {
            let (a, b) = match &n.shape {
                Shape::Terrain(t) if !t.mirrored && t.n >= 2 && t.heights.len() == t.n * t.n => {
                    let (keep_lo, keep_hi) = (t.lower * 65535.0, t.upper * 65535.0);
                    let (mut a, mut b) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
                    let h = t.n as f64 / 2.0;
                    let step = (t.n / 128).max(1);
                    for r in (0..t.n).step_by(step) {
                        for c in (0..t.n).step_by(step) {
                            let v = t.heights[r * t.n + c] as f64;
                            if v < keep_lo || v > keep_hi {
                                continue;
                            }
                            // a cell's own-space place, as `model::node_box` spans the grid
                            let q = [c as f64 / (t.n - 1) as f64 * 2.0 * h - h, v - 32767.5, r as f64 / (t.n - 1) as f64 * 2.0 * h - h];
                            for k in 0..3 {
                                a[k] = a[k].min(q[k]);
                                b[k] = b[k].max(q[k]);
                            }
                        }
                    }
                    if a[0] > b[0] { crate::model::node_box(n) } else { (a, b) }
                }
                _ => crate::model::node_box(n),
            };
            for c in 0..8 {
                let p = [if c & 1 == 0 { a[0] } else { b[0] }, if c & 2 == 0 { a[1] } else { b[1] }, if c & 4 == 0 { a[2] } else { b[2] }];
                let w = math::place(&n.m, p);
                for k in 0..3 {
                    lo[k] = lo[k].min(w[k]);
                    hi[k] = hi[k].max(w[k]);
                }
            }
        }
        (lo[0] <= hi[0]).then_some((lo, hi))
    }
}

/// The kinds this reader tells apart: the engine's, and the disc (class C3dCircle), which the
/// engine's list reader does not know.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Engine(ObjectKind),
    Disc,
    Unknown(FourCc, FourCc),
}

const DISC_CLASS: &str = "C3dCircle";

#[derive(Clone, Copy)]
struct Row {
    kind: Kind,
    identity: u32,
    linked_list: u32,
}

fn word(b: &[u8], at: usize, o: ByteOrder) -> u32 {
    let w = [b[at], b[at + 1], b[at + 2], b[at + 3]];
    match o {
        ByteOrder::Big => u32::from_be_bytes(w),
        ByteOrder::Little => u32::from_le_bytes(w),
    }
}

/// An object list's rows (the layout `ObjectList::parse` reads: 12 bytes, then 16 a row), keeping
/// rows of codes the engine does not know.
fn rows_of(b: &[u8]) -> Result<Vec<Row>, String> {
    if b.len() < 12 {
        return Err("object list too short".into());
    }
    let o = if b[4..8] == [1, 0, 0, 0] { ByteOrder::Little } else { ByteOrder::Big };
    if word(b, 0, o) != 1 {
        return Err("object list of an unknown form".into());
    }
    let count = word(b, 8, o) as usize;
    if b.len() < 12 + count * 16 {
        return Err("object list shorter than its rows".into());
    }
    Ok((0..count)
        .map(|i| {
            let r = &b[12 + 16 * i..12 + 16 * (i + 1)];
            let kind = FourCc::new(&word(r, 0, o).to_be_bytes());
            let class = FourCc::new(&word(r, 4, o).to_be_bytes());
            let k = match ObjectKind::from_codes(kind, class) {
                Some(k) => Kind::Engine(k),
                None if &kind.0 == b"3dob" && matches!(&class.0, b"circ" | b"disc" | b"disk" | b"crcl") => Kind::Disc,
                None => Kind::Unknown(kind, class),
            };
            Row { kind: k, identity: word(r, 8, o), linked_list: word(r, 12, o) }
        })
        .collect())
}

struct File {
    container: Container,
    index: ResourceIndex,
}

impl File {
    fn resources(&self, tag: &str) -> impl Iterator<Item = &Resource> {
        let tag = tag.as_bytes().to_vec();
        self.index.classes.iter().filter(move |c| c.tag == tag).flat_map(|c| c.resources.iter())
    }
    fn find(&self, tag: &str, identity: u32) -> Option<&Resource> {
        self.resources(tag).find(|r| r.identity == identity)
    }
    fn data(&self, r: &Resource) -> Option<&[u8]> {
        self.container.entry(r.entries[0] as usize)
    }
    fn text(&self, r: &Resource, at: usize, len: usize) -> String {
        let Some(b) = self.data(r) else { return String::new() };
        let b = b.get(at..b.len().min(at + len)).unwrap_or(&[]);
        mac_roman(&b.iter().take_while(|&&c| c != 0).copied().collect::<Vec<u8>>()).trim().to_owned()
    }
    fn thumbnail(&self, identity: u32) -> Option<(u32, u32, Vec<u8>)> {
        let r = self.find("objprevcimage", identity)?;
        let header = self.container.entry(r.entries[1] as usize)?;
        let p = decode_picture(header, self.data(r)?).ok()?;
        Some((p.width(), p.height(), p.pixels().iter().flat_map(|px| [px.0[1], px.0[2], px.0[3]]).collect()))
    }
}

fn kind_label(k: Kind) -> String {
    match k {
        Kind::Engine(ObjectKind::HeightField) => "terrain".into(),
        Kind::Engine(ObjectKind::TriMesh) => "mesh".into(),
        Kind::Engine(ObjectKind::SphereLight) => "radial light".into(),
        Kind::Engine(ObjectKind::ConeLight) => "spot light".into(),
        Kind::Engine(ObjectKind::CubeLight) => "parallel light".into(),
        Kind::Engine(ObjectKind::CylinderLight) => "cylinder light".into(),
        Kind::Engine(ObjectKind::PyramidLight) => "square spot light".into(),
        Kind::Engine(k) => format!("{k:?}").to_lowercase(),
        Kind::Disc => "disc".into(),
        Kind::Unknown(a, b) => format!("unknown {a}/{b}"),
    }
}

fn m12(t: &Transform) -> M12 {
    let r = &t.rows;
    [r[0][0], r[0][1], r[0][2], r[1][0], r[1][1], r[1][2], r[2][0], r[2][1], r[2][2], t.translation[0], t.translation[1], t.translation[2]]
}

/// Materials of the file (by identity), and the textures and pictures they name, as the engine
/// reads them.
struct Materials {
    list: Vec<EngineMaterial>,
    textures: Vec<Value>,
    pictures: Vec<Value>,
}

impl Materials {
    fn read(bytes: &[u8]) -> Materials {
        // the material reader of a .mat file, used here for every C3dBaseMaterial of the .obp
        // (with its textures resolved); a preset's object names its material by identity
        let Ok(lib) = Library::read(bytes, LibraryKind::Materials) else { return Materials { list: Vec::new(), textures: Vec::new(), pictures: Vec::new() } };
        let parse = |j: scene_export::J| serde_json::from_str::<Value>(&j.write()).unwrap_or(Value::Null);
        Materials {
            list: lib.presets.iter().filter_map(|p| if let PresetData::Material(m) = &p.data { Some((**m).clone()) } else { None }).collect(),
            textures: lib.textures.iter().map(|t| parse(scene_export::texture_json(t))).collect(),
            pictures: lib.pictures.iter().map(|p| parse(scene_export::picture_json(p))).collect(),
        }
    }
}

/// Renumber every "refs" list in a material export by `map` (file-wide texture index -> preset's).
fn remap_refs(v: &mut Value, map: &HashMap<u64, usize>) {
    match v {
        Value::Object(o) => {
            for (k, x) in o.iter_mut() {
                if k == "refs" {
                    if let Value::Array(a) = x {
                        for r in a.iter_mut() {
                            if let Some(i) = r.as_u64() {
                                *r = map.get(&i).map_or(Value::Null, |&j| json!(j));
                            }
                        }
                    }
                } else {
                    remap_refs(x, map);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| remap_refs(x, map)),
        _ => {}
    }
}

/// Builds one preset's nodes, reading its lists depth first.
struct Builder<'a> {
    file: &'a File,
    mats: &'a Materials,
    nodes: Vec<Node>,
    notes: Vec<String>,
    /// file material index -> preset material index
    used: Vec<usize>,
    seen_lists: Vec<u32>,
    latest_material: Option<usize>,
    counts: &'a mut HashMap<String, usize>,
}

/// A read object's parent link: its node id, its chain (second transform as linked), and whether
/// it is a group.
#[derive(Clone, Copy)]
struct Parent {
    id: u64,
    chain: Transform,
    group: bool,
}

impl Builder<'_> {
    fn material(&mut self, identity: Option<u32>, who: &str) -> Option<usize> {
        let id = identity?;
        let Some(k) = self.mats.list.iter().position(|m| m.identity == id) else {
            self.notes.push(format!("{who}: its material ({id}) is not in the file or cannot be read; shown plain."));
            return None;
        };
        Some(match self.used.iter().position(|&u| u == k) {
            Some(i) => i,
            None => {
                self.used.push(k);
                self.used.len() - 1
            }
        })
    }

    fn list(&mut self, identity: u32, parent: Option<Parent>, depth: usize) {
        if identity == 0 || self.seen_lists.contains(&identity) || depth > 64 {
            return;
        }
        self.seen_lists.push(identity);
        let tag = if depth == 0 { "C3dRootObjList" } else { "C3dObjList" };
        let Some(bytes) = self.file.find(tag, identity).and_then(|r| self.file.data(r)) else {
            self.notes.push(format!("a list of objects ({identity}) is missing from the file."));
            return;
        };
        let rows = match rows_of(bytes) {
            Ok(r) => r,
            Err(e) => {
                self.notes.push(format!("a list of objects cannot be read: {e}."));
                return;
            }
        };
        for row in rows {
            let read = self.object(&row, parent);
            if row.linked_list != 0 {
                match read {
                    Some(p) => self.list(row.linked_list, Some(p), depth + 1),
                    None => self.notes.push("objects linked to an object left out were left out too.".into()),
                }
            }
        }
    }

    /// Reads one row's object into a node; returns it as a parent for its linked list.
    fn object(&mut self, row: &Row, parent: Option<Parent>) -> Option<Parent> {
        let label = kind_label(row.kind);
        *self.counts.entry(label.clone()).or_default() += 1;
        let tag = match row.kind {
            Kind::Engine(k) => k.class_tag(),
            Kind::Disc => DISC_CLASS,
            Kind::Unknown(..) => {
                self.notes.push(format!("{label}: a kind of object this app does not know."));
                return None;
            }
        };
        let Some(d) = self.file.find(tag, row.identity).and_then(|r| self.file.data(r)) else {
            self.notes.push(format!("{label}: its data ({tag} {}) is missing from the file.", row.identity));
            return None;
        };
        if d.len() < FIRST_PART_LEN + 4 {
            self.notes.push(format!("{label}: its data is too short."));
            return None;
        }
        let o = order_of(d);
        let name = first_part_name(d).map(|n| mac_roman(&n).trim().to_owned()).unwrap_or_default();
        let who = if name.is_empty() { label.clone() } else { format!("{label} \"{name}\"") };
        if matches!(row.kind, Kind::Engine(ObjectKind::Camera)) {
            return None;
        }
        let pl = match PlacementPart::parse_either(&d[FIRST_PART_LEN..], o) {
            Ok(p) => p,
            Err(e) => {
                self.notes.push(format!("{who}: its placement cannot be read ({e:?})."));
                return None;
            }
        };
        let at = FIRST_PART_LEN + PlacementPart::len_of_version(pl.version).unwrap_or(0);
        let rest = &d[at.min(d.len())..];
        // the world as the program that made the file places it (Bryce 3D unless a placement is Bryce 5's)
        let version = if pl.version == 2 { RenderAs::Bryce50 } else { RenderAs::Bryce30 };
        let (world, chain) = match parent {
            None => (pl.world(), pl.second),
            Some(p) => LinkArithmetic::of(version).linked(&pl.first, &pl.second, pl.first_flags, &p.chain, version),
        };
        let mut material = self.material(pl.material, &who);
        let shape = match row.kind {
            Kind::Disc => Shape::Disc,
            Kind::Engine(k) => match k {
                ObjectKind::Sphere => Shape::Sphere,
                ObjectKind::Cube => Shape::Cube,
                ObjectKind::Cylinder => Shape::Cylinder,
                ObjectKind::Cone => Shape::Cone,
                ObjectKind::Pyramid => Shape::Pyramid,
                ObjectKind::Plane => Shape::Plane,
                ObjectKind::Square => Shape::Square,
                ObjectKind::Slab => {
                    self.notes.push(format!("{who} is shown as an infinite plane."));
                    Shape::Plane
                }
                ObjectKind::Group => Shape::Group,
                ObjectKind::Torus => match rest.get(..TORUS_PART_LEN).map(|b| TorusPart::parse(b, o)) {
                    Some(Ok(t)) => Shape::Torus { tube: t.tube_radius, ring: t.ring_radius },
                    _ => {
                        self.notes.push(format!("{who}: its torus part cannot be read."));
                        return None;
                    }
                },
                ObjectKind::HeightField => {
                    if d.len() < at + TERRAIN_PART_LEN {
                        self.notes.push(format!("{who}: its heights are missing."));
                        return None;
                    }
                    match TerrainPart::parse(d, at, o) {
                        Ok(t) => Shape::Terrain(Terrain {
                            n: t.grid.side(),
                            heights: Arc::new(t.grid.heights().to_vec()),
                            mirrored: t.mirrored,
                            smooth: t.smooth,
                            lower: t.stored_bounds[0],
                            upper: t.stored_bounds[1],
                            faces: t.closing_faces & 0x3f,
                            solid_faces: t.stored_solid_faces & 0x3f,
                        }),
                        Err(e) => {
                            self.notes.push(format!("{who}: its heights cannot be read ({e:?})."));
                            return None;
                        }
                    }
                }
                ObjectKind::TriMesh => match MeshResource::parse(rest, o, 0) {
                    Ok(m) => mesh_data(&m),
                    Err(e) => {
                        self.notes.push(format!("{who}: its mesh cannot be read ({e:?})."));
                        return None;
                    }
                },
                ObjectKind::SphereLight | ObjectKind::ConeLight | ObjectKind::CubeLight | ObjectKind::CylinderLight | ObjectKind::PyramidLight => {
                    let v = if rest.len() >= 4 { word(rest, 0, o) } else { 0 };
                    let Some(len) = LightPart::len_of_version(v).filter(|l| rest.len() >= *l) else {
                        self.notes.push(format!("{who}: its light part cannot be read."));
                        return None;
                    };
                    let Ok(lp) = LightPart::parse(&rest[..len], o) else {
                        self.notes.push(format!("{who}: its light part cannot be read."));
                        return None;
                    };
                    let lp = lp.with_third_flags(pl.third_flags);
                    let class = match k {
                        ObjectKind::ConeLight => Some(BeamClass::Spot),
                        ObjectKind::CubeLight => Some(BeamClass::Parallel),
                        ObjectKind::CylinderLight => Some(BeamClass::Cylinder),
                        ObjectKind::PyramidLight => Some(BeamClass::Square),
                        _ => None,
                    };
                    let beam = class.filter(|c| rest.len() >= len + c.record_len()).map(|c| BeamRecord::parse(c, &rest[len..], o));
                    let (kind, exponents) = match beam {
                        None => (LightKind::Radial, [1.0, 1.0]),
                        Some(BeamRecord::Spot { exponent, .. }) => (LightKind::Spot, [exponent, 0.0]),
                        Some(BeamRecord::Parallel { exponents: e }) => (LightKind::SquareParallel, [e[0] as f64, e[1] as f64]),
                        Some(BeamRecord::Cylinder { exponent, .. }) => (LightKind::Parallel, [exponent, 0.0]),
                        Some(BeamRecord::Square { exponents: e }) => (LightKind::SquareSpot, [e[0] as f64, e[1] as f64]),
                    };
                    // a light shines through its material: a plain one tints it, a textured one is a gel
                    let mut colour = lp.colour;
                    let mat = material.or(if kind != LightKind::Radial { self.latest_material } else { None });
                    material = None;
                    if let Some(mi) = mat {
                        let m = &self.mats.list[self.used[mi]];
                        if m.textures.any() {
                            material = Some(mi);
                        } else {
                            for c in 0..3 {
                                colour[c] *= m.part.colours[0].a[c];
                            }
                        }
                    }
                    Shape::Light(Light { kind, colour, intensity: lp.intensity, switches: pl.second_flags, exponents })
                }
                ObjectKind::Path | ObjectKind::Blob | ObjectKind::Tree | ObjectKind::Camera => {
                    self.notes.push(format!("{who}: a {label} is not carried over."));
                    if pl.material.is_some() {
                        self.latest_material = material;
                    }
                    return None;
                }
            },
            Kind::Unknown(..) => return None,
        };
        if pl.material.is_some() && !matches!(shape, Shape::Light(_)) {
            self.latest_material = material;
        }
        let id = self.nodes.len() as u64 + 1;
        let in_group = parent.filter(|p| p.group);
        let word2 = pl.second_flags;
        let role = if in_group.is_some() {
            match word2 & 0x38 {
                0x8 => Role::Positive,
                0x10 => Role::Negative,
                0x20 => Role::Intersect,
                _ => Role::Neutral,
            }
        } else {
            Role::Neutral
        };
        let name = if name.is_empty() {
            let base = shape.label();
            let k = self.nodes.iter().filter(|n| n.name.starts_with(base)).count() + 1;
            format!("{base} {k}")
        } else {
            name
        };
        let is_group = matches!(shape, Shape::Group);
        self.nodes.push(Node {
            id,
            name,
            shape,
            m: m12(&world),
            material,
            group: in_group.map(|p| p.id),
            role,
            cut_positive: in_group.is_some() && word2 & 0x10 != 0 && word2 & 0x80 != 0,
            hidden: word2 & 1 != 0,
            locked: false,
            family: 0,
            as_box: false,
            link: None,
            link_rest: None,
            track: None,
            keys: Vec::new(),
            flags: [pl.first_flags, word2 & !1, pl.third_flags],
        });
        Some(Parent { id, chain, group: is_group })
    }
}

/// A mesh part as the app holds it: the corners in the order the stored plane says (as the
/// engine's export does), smooth where the file gives corner normals.
fn mesh_data(m: &MeshResource) -> Shape {
    let points: Vec<[f32; 3]> = m.points.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let triangles: Vec<[u32; 3]> = m
        .triangles
        .iter()
        .filter(|t| t.points.iter().all(|&i| (i as usize) < m.points.len()))
        .map(|t| {
            let p = |i: usize| m.points[t.points[i] as usize];
            let (a, b, c) = (p(0), p(1), p(2));
            let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            if n[0] * t.plane[0] + n[1] * t.plane[1] + n[2] * t.plane[2] < 0.0 { [t.points[0], t.points[2], t.points[1]] } else { t.points }
        })
        .collect();
    let smooth = m.triangles.iter().any(|t| matches!(t.corner_use, CornerUse::NormalsAndCoords | CornerUse::NormalsOnly));
    Shape::Mesh(MeshData { points: Arc::new(points), triangles: Arc::new(triangles), smooth })
}

/// Every preset of an object library, in the order the file lists them.
#[allow(dead_code)] // called by the library panel
pub fn read(path: &Path) -> Result<Vec<ObjPreset>, String> {
    Ok(read_counted(path)?.0)
}

/// As `read`, with the number of objects of each kind met.
fn read_counted(path: &Path) -> Result<(Vec<ObjPreset>, HashMap<String, usize>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let container = Container::parse(&bytes, Flavour::Bryce3d).map_err(|e| format!("Not an object library: {e}"))?;
    let index = ResourceIndex::read(&container).map_err(|e| format!("The library is damaged: {e}"))?;
    let file = File { container, index };
    let mats = Materials::read(&bytes);
    let mut counts = HashMap::new();
    let mut out = Vec::new();
    let descs: Vec<Resource> = file.resources("objpresetdesc").cloned().collect();
    // a file whose names are missing still has its lists
    let ids: Vec<u32> = if descs.is_empty() { file.resources("C3dRootObjList").map(|r| r.identity).collect() } else { descs.iter().map(|r| r.identity).collect() };
    for (k, &identity) in ids.iter().enumerate() {
        let name = file.find("objpresetdesc", identity).map(|r| file.text(r, 0, 64)).filter(|s| !s.is_empty()).unwrap_or_else(|| format!("Object {}", k + 1));
        let mut b = Builder { file: &file, mats: &mats, nodes: Vec::new(), notes: Vec::new(), used: Vec::new(), seen_lists: Vec::new(), latest_material: None, counts: &mut counts };
        if file.find("C3dRootObjList", identity).is_none() {
            b.notes.push("its list of objects is missing from the file.".into());
        } else {
            b.list(identity, None, 0);
        }
        let Builder { mut nodes, notes, used, .. } = b;
        // the preset's own textures (each once, in the order its materials name them) and pictures
        let mut tex_map: HashMap<u64, usize> = HashMap::new();
        let mut pic_map: HashMap<u64, usize> = HashMap::new();
        let mut local_tex: Vec<Value> = Vec::new();
        let mut local_pics: Vec<Value> = Vec::new();
        let mut records: Vec<(String, Value)> = Vec::new();
        for (i, &mi) in used.iter().enumerate() {
            let m = &mats.list[mi];
            let mut j: Value = serde_json::from_str(&scene_export::material_json(m).write()).unwrap_or(Value::Null);
            let mut refs = Vec::new();
            collect_refs(&j, &mut refs);
            for g in refs {
                if tex_map.contains_key(&g) {
                    continue;
                }
                let Some(t) = mats.textures.get(g as usize) else { continue };
                let mut t = t.clone();
                if let Some(pi) = t["picture"].as_u64() {
                    let local = *pic_map.entry(pi).or_insert_with(|| {
                        local_pics.push(mats.pictures.get(pi as usize).cloned().unwrap_or(Value::Null));
                        local_pics.len() - 1
                    });
                    t["picture"] = json!(local);
                }
                tex_map.insert(g, local_tex.len());
                local_tex.push(t);
            }
            remap_refs(&mut j, &tex_map);
            let own = mac_roman(&m.name).trim().to_owned();
            let mname = if own.is_empty() { if used.len() == 1 { name.clone() } else { format!("{name} {}", i + 1) } } else { own };
            records.push((mname, j));
        }
        let (textures, tex_names) = crate::import::textures_of(&local_tex, &local_pics);
        let materials: Vec<Material> = records.into_iter().map(|(n, j)| Material { name: n, record: crate::import::material_of(&j, &tex_names), keys: Vec::new() }).collect();
        // each group gets a box around its members (the editor's own rule)
        if nodes.iter().any(|n| matches!(n.shape, Shape::Group)) {
            let mut p = Project::default();
            p.nodes = std::mem::take(&mut nodes);
            let mut d = crate::state::Doc::new(p);
            d.refit_groups();
            nodes = d.project.nodes;
        }
        out.push(ObjPreset { name, thumb: file.thumbnail(identity), nodes, materials, textures, notes });
    }
    Ok((out, counts))
}

fn collect_refs(v: &Value, out: &mut Vec<u64>) {
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                if k == "refs" {
                    out.extend(x.as_array().into_iter().flatten().filter_map(Value::as_u64));
                } else {
                    collect_refs(x, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_refs(x, out)),
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------
// The check: `kayzee3d --obp OUT_DIR`
// ---------------------------------------------------------------------------------------

fn library_files() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let folders = [
        crate::setup::data_folder().join("from-disc/prog/Presets"),
        PathBuf::from(&home).join("skill/bryce-mac/disc/Bryce 3D Model Libraries"),
        PathBuf::from(&home).join("skill/bryce-mac/disc/Terrain Libraries"),
    ];
    let mut out = Vec::new();
    for f in folders {
        let mut here: Vec<PathBuf> = std::fs::read_dir(&f).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("obp"))).collect()).unwrap_or_default();
        here.sort();
        out.extend(here);
    }
    out
}

/// Which presets of a file the check renders (all with OBP_ALL set).
fn chosen(n: usize) -> Vec<usize> {
    if std::env::var("OBP_ALL").is_ok() {
        return (0..n).collect();
    }
    if let Ok(list) = std::env::var("OBP_PICK") {
        return list.split(',').filter_map(|s| s.trim().parse().ok()).filter(|&k| k < n).collect();
    }
    let mut v = vec![0, n / 2];
    v.dedup();
    v.retain(|&k| k < n);
    v
}

/// A scene showing one preset: a new project with the preset merged in, the ground under it and
/// the camera aimed at its box.
pub fn preview_project(preset: &ObjPreset, size: [u32; 2]) -> Project {
    let mut p = crate::create::new_project();
    p.size = size;
    crate::fileio::merge(&mut p, preset.project());
    if let Some((lo, hi)) = preset.bounds() {
        let centre = math::mul(math::add(lo, hi), 0.5);
        let extent = math::len(math::sub(hi, lo)).max(1e-6);
        if let Some(g) = p.nodes.iter_mut().find(|n| n.name == "Ground Plane") {
            g.m[10] = lo[1];
        }
        let fov = p.cam.fov;
        let dist = 0.62 * extent / (0.8 * fov / 2.0).tan().max(0.05);
        let dir = math::unit([0.62, 0.42, -0.66]);
        p.cam.eye = math::add(centre, math::mul(dir, dist));
        p.cam.look_at(centre);
        p.cam.pivot = centre;
    }
    p
}

pub fn cli(out: &Path) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let substrate = crate::setup::substrate_path();
    let render = std::env::var("OBP_NO_RENDER").is_err();
    for path in library_files() {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let (presets, counts) = match read_counted(&path) {
            Ok(r) => r,
            Err(e) => {
                println!("{}: REFUSED: {e}", path.display());
                continue;
            }
        };
        let objects: usize = presets.iter().map(|p| p.nodes.len()).sum();
        let mut kinds: Vec<_> = counts.iter().collect();
        kinds.sort();
        println!(
            "{}: {} presets, {} nodes made, {} with pictures; met: {}",
            path.display(),
            presets.len(),
            objects,
            presets.iter().filter(|p| p.thumb.is_some()).count(),
            kinds.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", ")
        );
        if let Ok(bytes) = std::fs::read(&path)
            && let Ok(c) = Container::parse(&bytes, Flavour::Bryce3d)
            && let Ok(ix) = ResourceIndex::read(&c)
        {
            let mut seen = std::collections::HashSet::new();
            let dup = ix.classes.iter().flat_map(|cl| cl.resources.iter().map(move |r| (cl.tag.clone(), r.identity))).filter(|k| !seen.insert(k.clone())).count();
            if dup > 0 {
                println!("   identities used twice within a class: {dup}");
            }
        }
        let empty = presets.iter().filter(|p| p.nodes.is_empty()).count();
        if empty > 0 {
            println!("   presets with no object made: {empty}");
        }
        let mut refusals: HashMap<String, usize> = HashMap::new();
        for p in &presets {
            for n in &p.notes {
                *refusals.entry(n.clone()).or_default() += 1;
            }
        }
        let mut refusals: Vec<_> = refusals.into_iter().collect();
        refusals.sort_by(|a, b| b.1.cmp(&a.1));
        for (n, k) in refusals.iter().take(8) {
            println!("   refusal x{k}: {n}");
        }
        for k in chosen(presets.len()) {
            let pr = &presets[k];
            let folder = path.parent().and_then(|f| f.file_name()).map(|f| f.to_string_lossy().chars().take(4).collect::<String>()).unwrap_or_default();
            let slug: String = format!("{folder}-{stem}").chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
            if let Some((w, h, rgb)) = &pr.thumb {
                crate::fileio::write_picture(&out.join(format!("{slug}-{k}-thumb.png")), *w, *h, rgb)?;
            }
            let mut kinds: HashMap<&str, usize> = HashMap::new();
            for n in &pr.nodes {
                *kinds.entry(n.shape.label()).or_default() += 1;
            }
            let size = pr.bounds().map(|(lo, hi)| format!("box {:.2?}..{:.2?}", lo, hi)).unwrap_or_default();
            println!("   preset {k} \"{}\": {} nodes {:?}, {} materials, {} textures, {size}", pr.name, pr.nodes.len(), kinds, pr.materials.len(), pr.textures.len());
            if std::env::var("OBP_NODES").is_ok() {
                for t in &pr.textures {
                    let mut t = t.clone();
                    if t.get("picture").is_some() { t["picture"] = json!("…"); }
                    println!("        texture {t}");
                }
                for n in &pr.nodes {
                    println!("      {} {:?} {} m {:.3?} group {:?} {:?} material {:?}", n.id, n.shape.label(), n.name, n.m, n.group, n.role, n.material);
                    if let Some(m) = n.material.and_then(|i| pr.materials.get(i)) {
                        println!("        material {}: {}", m.name, m.record);
                    }
                    if let Shape::Terrain(t) = &n.shape {
                        println!("        terrain n {} lower {} upper {} faces {} solid {} smooth {} heights {}..{}", t.n, t.lower, t.upper, t.faces, t.solid_faces, t.smooth, t.heights.iter().min().unwrap_or(&0), t.heights.iter().max().unwrap_or(&0));
                    }
                }
            }
            if !render || pr.nodes.is_empty() {
                continue;
            }
            let p = preview_project(pr, [320, 320]);
            let text = crate::emit::description(&p, (320, 320), 0);
            match crate::job::render_now(&text, &substrate, 0) {
                Ok(pic) => crate::fileio::write_picture(&out.join(format!("{slug}-{k}.png")), pic.width, pic.height, &pic.levels)?,
                Err(e) => println!("   preset {k} render failed: {e}"),
            }
        }
    }
    Ok(())
}
