//! The Materials Lab: every channel of a surface, which texture feeds each
//! channel, a live preview, and the texture editor for procedural textures.

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Vec2};
use serde_json::{Value, json};

use crate::app::{App, Lab, Picture};
use crate::model::{Material, Project, Shape};
use crate::{create, emit, job, kit, library, math, setup};

const COLOURS: [(&str, &str, &str); 5] = [
    ("diffuse_color", "Diffuse", "The surface's own colour under light"),
    ("ambient_color", "Ambient", "Its colour in shade"),
    ("specular_color", "Specular", "The colour of highlights"),
    ("specular_halo", "Specular Halo", "How wide highlights spread (brighter is wider)"),
    ("transparent_color", "Transparent", "The tint of light passing through"),
];
const VALUES: [(&str, &str, &str, f64); 8] = [
    ("diffusion", "Diffusion", "How much light the surface scatters", 1.0),
    ("ambience", "Ambience", "How bright it is in shade", 1.0),
    ("specularity", "Specularity", "How strong highlights are", 1.0),
    ("metallicity", "Metallicity", "Highlights and reflections take the surface colour", 1.0),
    ("bump", "Bump Height", "Depth of the bumps a texture makes", 1.0),
    ("transparency", "Transparency", "How much light passes through", 1.0),
    ("reflection", "Reflection", "How mirror-like the surface is", 1.0),
    ("volume_density", "Volume Density", "Thickness of a volume material", 1.0),
];
const SLOT: [u32; 4] = [0x1000_0000, 0x2000_0000, 0x4000_0000, 0x8000_0000];

pub struct State {
    /// The material being edited (index in the project).
    pub index: Option<usize>,
    preview: Picture,
    preview_rev: u64,
    rev: u64,
    shape: u8,
    /// Texture open in the texture editor (index in the project's textures).
    texture: Option<usize>,
    entered: bool,
}

impl Default for State {
    fn default() -> Self {
        State { index: None, preview: Picture::default(), preview_rev: 0, rev: 1, shape: 0, texture: None, entered: false }
    }
}

/// A channel's plain value (its A side).
fn chan_a(record: &Value, key: &str) -> Value {
    let c = &record["channels"][key];
    if c.is_object() { c["a"].clone() } else { c.clone() }
}

fn chan_colour(record: &Value, key: &str) -> [f32; 3] {
    match chan_a(record, key).as_array() {
        Some(a) if a.len() >= 3 => [a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32, a[2].as_f64().unwrap_or(0.0) as f32],
        _ => [0.0; 3],
    }
}

fn chan_value(record: &Value, key: &str) -> f64 {
    chan_a(record, key).as_f64().unwrap_or(0.0)
}

fn set_chan_a(record: &mut Value, key: &str, v: Value) {
    if !record["channels"].is_object() {
        record["channels"] = json!({});
    }
    if record["channels"][key].is_object() {
        record["channels"][key]["a"] = v;
    } else {
        record["channels"][key] = v;
    }
}

/// The texture feeding a channel, if one does.
fn chan_texture(record: &Value, key: &str) -> Option<String> {
    let c = &record["channels"][key];
    if c["switches"].as_u64().unwrap_or(0) == 0 {
        return None;
    }
    c["a_tex"].as_str().map(str::to_owned)
}

fn set_chan_texture(record: &mut Value, key: &str, texture: Option<&str>, colour: bool) {
    let a = chan_a(record, key);
    match texture {
        None => record["channels"][key] = if a.is_null() { if colour { json!([0.0, 0.0, 0.0]) } else { json!(0.0) } } else { a },
        Some(name) => {
            let a = if a.is_null() { if colour { json!([1.0, 1.0, 1.0]) } else { json!(1.0) } } else { a };
            let b = if colour { json!([0.0, 0.0, 0.0]) } else { json!(1.0) };
            record["channels"][key] = json!({ "a": a, "b": b, "alpha": 1.0, "switches": SLOT[0], "a_tex": name });
        }
    }
}

fn texture_names(p: &Project) -> Vec<String> {
    p.textures.iter().filter_map(|t| t["name"].as_str().map(str::to_owned)).collect()
}

/// A short name to show for a texture.
pub fn nice(name: &str) -> &str {
    name.split_once(':').map(|x| x.1).filter(|s| !s.is_empty()).unwrap_or(name)
}

/// The material of the first selected object that has one; objects without get a fresh one.
pub fn open(app: &mut App) {
    let Some(id) = app.doc.selection.first().copied() else {
        app.warn("Select an object first.");
        return;
    };
    let ids = app.doc.project.with_descendants(id);
    let mut index = ids.iter().filter_map(|i| app.doc.project.node(*i)).find_map(|n| n.material);
    if index.is_none() {
        app.doc.checkpoint();
        let i = create::own_material(&mut app.doc.project, "Material", create::plain([0.72, 0.72, 0.72]));
        for d in ids {
            if let Some(n) = app.doc.project.node_mut(d) {
                if !matches!(n.shape, Shape::Group | Shape::Light(_)) {
                    n.material = Some(i);
                }
            }
        }
        app.doc.touch();
        index = Some(i);
    }
    app.mat.index = index;
    app.mat.entered = false;
    app.mat.texture = None;
    app.mat.rev += 1;
    app.lab = Lab::Materials;
}

pub fn copy_material(app: &mut App) {
    let m = app.doc.selection.first().and_then(|id| app.doc.project.node(*id)).and_then(|n| n.material).and_then(|i| app.doc.project.materials.get(i)).cloned();
    match m {
        Some(m) => app.doc.material_clipboard = Some(m),
        None => app.warn("The selection has no material to copy."),
    }
}

pub fn paste_material(app: &mut App) {
    let Some(m) = app.doc.material_clipboard.clone() else { return };
    if app.doc.selection.is_empty() {
        return;
    }
    app.doc.checkpoint();
    let i = create::own_material(&mut app.doc.project, &m.name, m.record);
    assign(app, i);
    app.doc.touch();
}

/// Give material `i` to every selected object.
pub fn assign(app: &mut App, i: usize) {
    let ids: Vec<u64> = app.doc.selection.iter().flat_map(|s| app.doc.project.with_descendants(*s)).collect();
    for d in ids {
        if let Some(n) = app.doc.project.node_mut(d) {
            if !matches!(n.shape, Shape::Group | Shape::Light(_)) {
                n.material = Some(i);
            }
        }
    }
}

/// A small scene showing one material on a ball, box or ground under a plain sky.
fn preview_scene(p: &Project, record: &Value, shape: u8) -> String {
    let mut s = Project::default();
    s.program = p.program.clone();
    s.size = [200, 200];
    s.sky = create::default_sky();
    s.textures = p.textures.clone();
    let floor = create::own_material(&mut s, "floor", create::plain([0.55, 0.55, 0.58]));
    s.add("floor", Shape::Plane, math::IDENTITY, Some(floor));
    let tile = create::own_material(&mut s, "tile", create::plain([0.2, 0.22, 0.3]));
    for k in 0..8 {
        let (x, z) = ((k % 4) as f64 * 2.0 - 3.0 + if k >= 4 { 1.0 } else { 0.0 }, if k >= 4 { 1.0 } else { 3.0 });
        s.add("tile", Shape::Cube, math::from_axes([1.0, 0.0, 0.0], [0.0, 0.02, 0.0], [0.0, 0.0, 1.0], [x, 0.01, z]), Some(tile));
    }
    let m = create::own_material(&mut s, "subject", record.clone());
    match shape {
        1 => s.add("subject", Shape::Cube, math::compose(&math::translation([0.0, 1.0, 0.0]), &math::compose(&math::turn([0.0, 1.0, 0.0], 0.6), &math::scaling([1.7, 1.7, 1.7]))), Some(m)),
        2 => {
            s.nodes[0].material = Some(m);
            0
        }
        _ => s.add("subject", Shape::Sphere, math::translation([0.0, 1.0, 0.0]), Some(m)),
    };
    s.cam.eye = [2.2, 2.3, -3.6];
    s.cam.pivot = [0.0, 0.9, 0.0];
    s.cam.look_at([0.0, 0.9, 0.0]);
    s.cam.fov = 0.9;
    emit::description(&s, (200, 200), 1)
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(index) = app.mat.index.filter(|i| *i < app.doc.project.materials.len()) else {
        app.lab = Lab::None;
        return;
    };
    if !app.mat.entered {
        app.doc.checkpoint();
        app.mat.entered = true;
    }
    if let Some(t) = app.mat.texture {
        texture_editor(app, ctx, t);
        return;
    }
    let mut record = app.doc.project.materials[index].record.clone();
    let mut name = app.doc.project.materials[index].name.clone();
    let before = (record.clone(), name.clone());
    let names = texture_names(&app.doc.project);
    let users = app.doc.project.nodes.iter().filter(|n| n.material == Some(index)).count();
    let mut leave = false;
    let mut open_texture: Option<usize> = None;
    let mut library_kind: Option<library::Kind> = None;
    let mut make_unique = false;

    app.mat.preview.refresh(ctx, "material-preview");
    egui::SidePanel::left("mat-left").exact_width(236.0).resizable(false).frame(egui::Frame::new().fill(kit::PANEL).inner_margin(14.0)).show(ctx, |ui| {
        ui.label(RichText::new("MATERIALS LAB").color(kit::ACCENT).strong());
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(208.0), egui::Sense::click());
        ui.painter().rect_filled(rect, 4.0, kit::INK);
        if let Some(t) = &app.mat.preview.texture {
            ui.painter().image(t.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        if resp.on_hover_text("Click to redraw the preview").clicked() {
            app.mat.rev += 1;
        }
        if let Some(reason) = app.mat.preview.job.as_ref().and_then(|j| match &j.lock().unwrap().state {
            job::State::Failed(r) => Some(r.clone()),
            _ => None,
        }) {
            ui.label(RichText::new(format!("Preview refused: {reason}")).color(kit::ACCENT).small());
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Show on").small().color(kit::DIM));
            for (k, label) in ["Ball", "Box", "Ground"].iter().enumerate() {
                if ui.selectable_label(app.mat.shape == k as u8, *label).clicked() {
                    app.mat.shape = k as u8;
                    app.mat.rev += 1;
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut name);
        });
        if users > 1 {
            ui.label(RichText::new(format!("Shared by {users} objects.")).small().color(kit::DIM));
            if ui.small_button("Give the selection its own copy").clicked() {
                make_unique = true;
            }
        }
        ui.add_space(6.0);
        if ui.button("Material presets…").clicked() {
            library_kind = Some(library::Kind::Materials);
        }
        if ui.button("Texture presets…").on_hover_text("Add a texture from your disc's texture libraries to this scene").clicked() {
            library_kind = Some(library::Kind::Textures);
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            if ui.add_sized(Vec2::new(208.0, 32.0), egui::Button::new(RichText::new("Done").color(kit::INK).strong()).fill(kit::ACCENT)).clicked() {
                leave = true;
            }
            if ui.button("Cancel").on_hover_text("Leave and put the material back as it was").clicked() {
                app.doc.undo();
                app.rebase();
                leave = true;
            }
        });
    });

    egui::CentralPanel::default().frame(egui::Frame::new().fill(kit::PANEL_DEEP).inner_margin(18.0)).show(ctx, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let pick_texture = |ui: &mut egui::Ui, record: &mut Value, key: &str, colour: bool, open_texture: &mut Option<usize>| {
                let current = chan_texture(record, key);
                let label = current.as_deref().map(nice).unwrap_or("none").to_owned();
                egui::ComboBox::from_id_salt(("tex", key)).width(150.0).selected_text(label).show_ui(ui, |ui| {
                    if ui.selectable_label(current.is_none(), "none").clicked() {
                        set_chan_texture(record, key, None, colour);
                    }
                    for n in &names {
                        if ui.selectable_label(current.as_deref() == Some(n), nice(n)).clicked() {
                            set_chan_texture(record, key, Some(n), colour);
                        }
                    }
                });
                if let Some(c) = current {
                    if let Some(i) = names.iter().position(|n| *n == c) {
                        if ui.small_button("Edit").on_hover_text("Open this texture in the texture editor").clicked() {
                            *open_texture = Some(i);
                        }
                    }
                }
            };
            kit::heading(ui, "COLOUR");
            egui::Grid::new("colours").num_columns(4).spacing([14.0, 8.0]).show(ui, |ui| {
                for (key, label, tip) in COLOURS {
                    ui.label(label).on_hover_text(tip);
                    let mut c = chan_colour(&record, key);
                    if kit::colour_well(ui, &mut c, tip) {
                        set_chan_a(&mut record, key, json!(c));
                    }
                    ui.label(RichText::new("texture").small().color(kit::DIM));
                    ui.horizontal(|ui| pick_texture(ui, &mut record, key, true, &mut open_texture));
                    ui.end_row();
                }
            });
            ui.add_space(10.0);
            kit::heading(ui, "VALUE");
            egui::Grid::new("values").num_columns(4).spacing([14.0, 8.0]).show(ui, |ui| {
                for (key, label, tip, top) in VALUES {
                    ui.label(label).on_hover_text(tip);
                    let mut v = chan_value(&record, key) * 100.0;
                    if ui.add(egui::Slider::new(&mut v, 0.0..=top * 100.0).fixed_decimals(1)).changed() {
                        set_chan_a(&mut record, key, json!(v / 100.0));
                    }
                    ui.label(RichText::new("texture").small().color(kit::DIM));
                    ui.horizontal(|ui| pick_texture(ui, &mut record, key, false, &mut open_texture));
                    ui.end_row();
                }
            });
            ui.add_space(10.0);
            kit::heading(ui, "OPTICS AND SHADING");
            let mut refraction = record["refraction"].as_f64().unwrap_or(1.0) * 100.0;
            if ui.add(egui::Slider::new(&mut refraction, 100.0..=300.0).text("Refraction (100 = none, 133 water, 152 glass)")).changed() {
                record["refraction"] = json!(refraction / 100.0);
            }
            let mut flags = record["flags"].as_u64().unwrap_or(0x10000);
            ui.horizontal_wrapped(|ui| {
                for (bit, label, tip) in [(0x2u64, "Fuzzy", "Soft, cloud-like edges"), (0x8, "Blend transparency", "Transparent where the texture's alpha says so"), (0x20, "Receives no shadows", "Other objects cast no shadow on it"), (0x10, "Bump fades with distance", "Bumps flatten far from the camera")] {
                    let mut on = flags & bit != 0;
                    if ui.checkbox(&mut on, label).on_hover_text(tip).changed() {
                        flags ^= bit;
                    }
                }
            });
            record["flags"] = json!(flags);
            ui.horizontal(|ui| {
                ui.label("Volume colour");
                let v = record["volume_colour"].as_array().cloned().unwrap_or_default();
                let mut c = [v.first().and_then(Value::as_f64).unwrap_or(0.0) as f32, v.get(1).and_then(Value::as_f64).unwrap_or(0.0) as f32, v.get(2).and_then(Value::as_f64).unwrap_or(0.0) as f32];
                if kit::colour_well(ui, &mut c, "Colour inside a transparent solid") {
                    record["volume_colour"] = json!(c);
                }
            });
            ui.add_space(10.0);
            kit::heading(ui, "TEXTURES IN THIS SCENE");
            if names.is_empty() {
                ui.label(RichText::new("None yet. Add one from Texture presets, or open a scene that uses textures.").color(kit::DIM).small());
            }
            ui.horizontal_wrapped(|ui| {
                for (i, n) in names.iter().enumerate() {
                    if ui.button(nice(n)).on_hover_text("Open in the texture editor").clicked() {
                        open_texture = Some(i);
                    }
                }
            });
        });
    });

    if make_unique {
        let i = create::own_material(&mut app.doc.project, &name, record.clone());
        assign(app, i);
        app.mat.index = Some(i);
        app.doc.touch();
        return;
    }
    if (record.clone(), name.clone()) != before {
        let m = &mut app.doc.project.materials[index];
        m.record = record;
        m.name = name;
        app.doc.touch();
        app.mat.rev += 1;
    }
    if let Some(t) = open_texture {
        app.mat.texture = Some(t);
    }
    if let Some(k) = library_kind {
        app.lib.open = Some(k);
    }
    refresh_preview(app, ctx, index);
    if leave {
        app.mat.preview.clear();
        app.mat.entered = false;
        app.lab = Lab::None;
    }
}

fn refresh_preview(app: &mut App, ctx: &egui::Context, index: usize) {
    if app.mat.preview_rev == app.mat.rev || !app.ready || job::is_running(&app.mat.preview.job) {
        if app.mat.preview_rev != app.mat.rev {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
        return;
    }
    app.mat.preview_rev = app.mat.rev;
    let Some(m) = app.doc.project.materials.get(index) else { return };
    let text = preview_scene(&app.doc.project, &m.record, app.mat.shape);
    app.mat.preview.clear();
    app.mat.preview.job = Some(job::start(text, setup::substrate_path(), (200, 200), ctx.clone()));
}

// --------------------------------------------------------------------------- texture editor

fn hex_to_rgb(v: &Value) -> [f32; 3] {
    match v {
        Value::String(s) => {
            let n = u32::from_str_radix(s, 16).unwrap_or(0);
            [((n >> 16) & 255) as f32 / 255.0, ((n >> 8) & 255) as f32 / 255.0, (n & 255) as f32 / 255.0]
        }
        Value::Array(a) if a.len() >= 3 => [a[0].as_f64().unwrap_or(0.0) as f32 / 255.0, a[1].as_f64().unwrap_or(0.0) as f32 / 255.0, a[2].as_f64().unwrap_or(0.0) as f32 / 255.0],
        _ => [0.5; 3],
    }
}

fn rgb_to_hex(c: [f32; 3]) -> Value {
    let f = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    json!(format!("{:06x}", f(c[0]) << 16 | f(c[1]) << 8 | f(c[2])))
}

fn drag(ui: &mut egui::Ui, v: &mut Value, speed: f64, label: &str) -> bool {
    let mut x = v.as_f64().unwrap_or(0.0);
    let changed = ui.add(egui::DragValue::new(&mut x).speed(speed).prefix(format!("{label} ")).max_decimals(4)).changed();
    if changed {
        *v = if v.is_u64() || v.is_i64() { json!(x.round() as i64) } else { json!(x) };
    }
    changed
}

fn colours_row(ui: &mut egui::Ui, c: &mut Value) -> bool {
    let mut changed = false;
    for key in ["a", "b", "c"] {
        let mut rgb = hex_to_rgb(&c[key]);
        if kit::colour_well(ui, &mut rgb, "One of the texture's three colours") {
            c[key] = rgb_to_hex(rgb);
            changed = true;
        }
    }
    changed |= drag(ui, &mut c["mode"], 0.1, "blend mode");
    changed
}

/// The procedural texture editor: components, their noise, filter, phase and colours.
fn texture_editor(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some(mut tex) = app.doc.project.textures.get(t).cloned() else {
        app.mat.texture = None;
        return;
    };
    let before = tex.clone();
    let index = app.mat.index.unwrap_or(0);
    app.mat.preview.refresh(ctx, "material-preview");
    let mut back = false;
    egui::SidePanel::left("tex-left").exact_width(236.0).resizable(false).frame(egui::Frame::new().fill(kit::PANEL).inner_margin(14.0)).show(ctx, |ui| {
        ui.label(RichText::new("TEXTURE EDITOR").color(kit::ACCENT).strong());
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(208.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 4.0, kit::INK);
        if let Some(p) = &app.mat.preview.texture {
            ui.painter().image(p.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        ui.label(RichText::new("Shown on the material being edited.").small().color(kit::DIM));
        ui.label(RichText::new(nice(tex["name"].as_str().unwrap_or(""))).strong());
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            if ui.add_sized(Vec2::new(208.0, 32.0), egui::Button::new(RichText::new("Back to the material").color(kit::INK).strong()).fill(kit::ACCENT)).clicked() {
                back = true;
            }
        });
    });
    egui::CentralPanel::default().frame(egui::Frame::new().fill(kit::PANEL_DEEP).inner_margin(18.0)).show(ctx, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            kit::heading(ui, "MAPPING");
            let modes = ["Object space", "World space", "Parametric", "Parametric scaled", "World top", "Spherical", "Cylindrical", "Reflection map", "Random", "Object top", "Object front"];
            let mut mode = tex["mapping"]["mode"].as_u64().unwrap_or(0) as usize;
            egui::ComboBox::from_id_salt("mapping").selected_text(modes.get(mode).copied().unwrap_or("Other")).show_ui(ui, |ui| {
                for (k, n) in modes.iter().enumerate() {
                    if ui.selectable_value(&mut mode, k, *n).changed() {
                        tex["mapping"]["mode"] = json!(k);
                    }
                }
            });
            for (key, label, speed) in [("scale", "Scale", 0.01), ("offset", "Offset", 0.01)] {
                ui.horizontal(|ui| {
                    ui.label(label);
                    if !tex["mapping"][key].is_array() {
                        tex["mapping"][key] = if key == "scale" { json!([0.51, 0.51, 0.51]) } else { json!([0.0, 0.0, 0.0]) };
                    }
                    for (k, axis) in ["X", "Y", "Z"].iter().enumerate() {
                        drag(ui, &mut tex["mapping"][key][k], speed, axis);
                    }
                    if key == "scale" && ui.small_button("Even").on_hover_text("Make Y and Z match X").clicked() {
                        let x = tex["mapping"][key][0].clone();
                        tex["mapping"][key][1] = x.clone();
                        tex["mapping"][key][2] = x;
                    }
                });
            }
            if tex["kind"] == "picture" {
                ui.add_space(8.0);
                ui.label(RichText::new("This is a picture texture; only its mapping can be changed here.").color(kit::DIM));
                return;
            }
            ui.add_space(10.0);
            kit::heading(ui, "WHOLE TEXTURE");
            let body = &mut tex["texture"];
            ui.horizontal(|ui| {
                let mut out = body["outputs"].as_u64().unwrap_or(7);
                for (bit, label) in [(1u64, "Value"), (2, "Colour"), (4, "Bump")] {
                    let mut on = out & bit != 0;
                    if ui.checkbox(&mut on, label).changed() {
                        out ^= bit;
                    }
                }
                body["outputs"] = json!(out);
            });
            ui.horizontal(|ui| {
                ui.label("Colours");
                colours_row(ui, &mut body["colours"]);
            });
            ui.horizontal(|ui| {
                ui.label("Filter");
                drag(ui, &mut body["filter"]["number"], 0.1, "kind");
                drag(ui, &mut body["filter"]["a"], 0.01, "a");
                drag(ui, &mut body["filter"]["b"], 0.01, "b");
            });
            ui.horizontal(|ui| {
                ui.label("Phase");
                drag(ui, &mut body["phase"]["amount"], 0.01, "amount");
            });
            if let Some(blend) = body["blend"].as_array_mut() {
                ui.horizontal(|ui| {
                    ui.label("Combine components");
                    for (k, b) in blend.iter_mut().enumerate() {
                        drag(ui, b, 0.1, if k == 0 { "1+2" } else { "+3" });
                    }
                });
            }
            let count = body["components"].as_array().map(Vec::len).unwrap_or(0);
            ui.horizontal(|ui| {
                if ui.add_enabled(count < 3, egui::Button::new("Add component")).clicked() {
                    if let Some(first) = body["components"].get(0).cloned() {
                        body["components"].as_array_mut().unwrap().push(first);
                    }
                }
                if ui.add_enabled(count > 1, egui::Button::new("Remove last")).clicked() {
                    body["components"].as_array_mut().unwrap().pop();
                }
            });
            if let Some(components) = body["components"].as_array_mut() {
                for (k, c) in components.iter_mut().enumerate() {
                    ui.add_space(10.0);
                    kit::heading(ui, &format!("COMPONENT {}", k + 1));
                    ui.horizontal(|ui| {
                        let mut out = c["outputs"].as_u64().unwrap_or(7);
                        for (bit, label) in [(1u64, "Value"), (2, "Colour"), (4, "Bump")] {
                            let mut on = out & bit != 0;
                            if ui.checkbox(&mut on, label).changed() {
                                out ^= bit;
                            }
                        }
                        c["outputs"] = json!(out);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Noise");
                        drag(ui, &mut c["noise"]["type"], 0.1, "type");
                        drag(ui, &mut c["noise"]["method"], 0.1, "mode");
                        drag(ui, &mut c["noise"]["octaves"], 0.1, "octaves");
                        drag(ui, &mut c["noise"]["dimension"], 0.05, "dimensions");
                    });
                    ui.horizontal(|ui| {
                        ui.label("Frequency");
                        if let Some(f) = c["frequencies"].as_array_mut() {
                            for (v, axis) in f.iter_mut().zip(["X", "Y", "Z"]) {
                                drag(ui, v, 0.05, axis);
                            }
                        }
                        ui.label("Turn");
                        if let Some(a) = c["angles"].as_array_mut() {
                            for v in a.iter_mut() {
                                drag(ui, v, 0.5, "");
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Filter");
                        drag(ui, &mut c["filter"]["number"], 0.1, "kind");
                        drag(ui, &mut c["filter"]["a"], 0.01, "a");
                        drag(ui, &mut c["filter"]["b"], 0.01, "b");
                        ui.label("Phase");
                        drag(ui, &mut c["phase"]["amount"], 0.01, "amount");
                    });
                    ui.horizontal(|ui| {
                        ui.label("Colours");
                        colours_row(ui, &mut c["colours"]);
                    });
                }
            }
        });
    });
    if tex != before {
        app.doc.project.textures[t] = tex;
        app.doc.touch();
        app.mat.rev += 1;
    }
    refresh_preview(app, ctx, index);
    if back {
        app.mat.texture = None;
    }
}

/// Add a material record (with the textures it names) to the project and give it to the selection.
pub fn adopt(app: &mut App, name: &str, record: Value, textures: Vec<Value>) -> usize {
    let p = &mut app.doc.project;
    let mut text = record.to_string();
    for mut t in textures {
        let old = t["name"].as_str().unwrap_or("").to_owned();
        if !text.contains(&format!("\"{old}\"")) {
            continue;
        }
        let fresh = format!("L{}:{}", p.textures.len(), nice(&old));
        text = text.replace(&format!("\"{old}\""), &format!("\"{fresh}\""));
        t["name"] = json!(fresh);
        p.textures.push(t);
    }
    let record = serde_json::from_str(&text).unwrap_or(record);
    p.materials.push(Material { name: name.to_owned(), record, keys: Vec::new() });
    p.materials.len() - 1
}
