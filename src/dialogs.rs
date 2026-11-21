//! The small windows: object attributes, light, camera, render options,
//! document setup, replicate, animation setup, movie rendering, help.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};
use serde_json::json;

use crate::app::App;
use crate::math::{self, Trs};
use crate::model::{self, LightKind, Role, Shape};
use crate::{anim, create, emit, job, kit, setup};

/// Display units per world unit (a new sphere is 20.48 across).
pub const UNITS: f64 = 10.24;

pub struct Movie {
    pub done: usize,
    pub total: usize,
    pub result: Option<Result<String, String>>,
    pub stop: bool,
}

#[derive(Default)]
pub struct State {
    pub auto_render: bool,
    pub attributes: bool,
    pub light: bool,
    pub camera: bool,
    pub render_options: bool,
    pub document: bool,
    pub movie: bool,
    pub replicate: bool,
    pub animation: bool,
    pub help: bool,
    pub about: bool,
    rep_copies: usize,
    rep_offset: [f64; 3],
    rep_turn: [f64; 3],
    rep_size: [f64; 3],
    movie_frames_only: bool,
    pub movie_job: Option<Arc<Mutex<Movie>>>,
    /// Attribute edits in progress saved a checkpoint already.
    editing: bool,
}

fn row3(ui: &mut egui::Ui, label: &str, v: &mut [f64; 3], speed: f64, suffix: &str) -> bool {
    let mut changed = false;
    ui.label(label);
    for (k, axis) in ["X", "Y", "Z"].iter().enumerate() {
        changed |= ui.add(egui::DragValue::new(&mut v[k]).speed(speed).prefix(format!("{axis} ")).suffix(suffix).max_decimals(3)).changed();
    }
    ui.end_row();
    changed
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    attributes(app, ctx);
    light(app, ctx);
    camera(app, ctx);
    render_options(app, ctx);
    document(app, ctx);
    replicate(app, ctx);
    animation(app, ctx);
    movie(app, ctx);
    help(app, ctx);
}

fn begin_edit(app: &mut App) {
    if !app.dlg.editing {
        app.doc.checkpoint();
        app.dlg.editing = true;
    }
}

fn attributes(app: &mut App, ctx: &egui::Context) {
    if !app.dlg.attributes {
        app.dlg.editing &= app.dlg.light || app.dlg.camera;
        return;
    }
    let mut open = true;
    egui::Window::new("Object Attributes").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        let Some(id) = app.doc.selection.first().copied() else {
            ui.label("Nothing is selected.");
            return;
        };
        let many = app.doc.selection.len() > 1;
        let Some(node) = app.doc.project.node(id).cloned() else { return };
        if many {
            ui.label(RichText::new(format!("{} objects selected; showing the first.", app.doc.selection.len())).color(kit::DIM));
        }
        let mut name = node.name.clone();
        ui.horizontal(|ui| {
            ui.label("Name");
            if ui.text_edit_singleline(&mut name).changed() {
                begin_edit(app);
                if let Some(n) = app.doc.project.node_mut(id) {
                    n.name = name.clone();
                }
                app.doc.touch();
            }
        });
        ui.label(RichText::new(node.shape.label()).color(kit::DIM).small());
        ui.separator();

        // size shown is the object's full extent along its own axes
        let (lo, hi) = model::node_box(&node);
        let span = [(hi[0] - lo[0]).abs().max(1e-9), (hi[1] - lo[1]).abs().max(1e-9), (hi[2] - lo[2]).abs().max(1e-9)];
        let t = math::decompose(&node.m);
        let mut pos = [t.position[0] * UNITS, t.position[1] * UNITS, t.position[2] * UNITS];
        let mut turn = t.degrees;
        let mut size = [t.size[0] * span[0] * UNITS, t.size[1] * span[1] * UNITS, t.size[2] * span[2] * UNITS];
        let flat = matches!(node.shape, Shape::Plane | Shape::Group);
        let mut changed = false;
        egui::Grid::new("trs").num_columns(4).show(ui, |ui| {
            changed |= row3(ui, "Position", &mut pos, 0.1, "");
            if !flat {
                changed |= row3(ui, "Rotation", &mut turn, 0.5, "°");
                changed |= row3(ui, "Size", &mut size, 0.1, "");
            }
        });
        if changed {
            begin_edit(app);
            let next = Trs { position: [pos[0] / UNITS, pos[1] / UNITS, pos[2] / UNITS], degrees: turn, size: [size[0] / span[0] / UNITS, size[1] / span[1] / UNITS, size[2] / span[2] / UNITS] };
            if matches!(node.shape, Shape::Group) {
                let by = math::sub(next.position, t.position);
                app.doc.move_selection(by);
            } else if let Some(n) = app.doc.project.node_mut(id) {
                n.m = math::recompose(&next);
            }
            app.doc.refit_groups();
            app.doc.touch();
        }
        if let Shape::Torus { tube, ring } = node.shape {
            let (mut tb, mut rg) = (tube, ring);
            ui.horizontal(|ui| {
                ui.label("Torus tube");
                let a = ui.add(egui::DragValue::new(&mut tb).speed(0.005).range(0.01..=5.0)).changed();
                ui.label("ring");
                let b = ui.add(egui::DragValue::new(&mut rg).speed(0.005).range(0.01..=5.0)).changed();
                if a || b {
                    begin_edit(app);
                    if let Some(n) = app.doc.project.node_mut(id) {
                        n.shape = Shape::Torus { tube: tb, ring: rg };
                    }
                    app.doc.touch();
                }
            });
        }
        ui.separator();
        let (mut hidden, mut locked, mut as_box) = (node.hidden, node.locked, node.as_box);
        let mut flags = false;
        ui.horizontal(|ui| {
            flags |= ui.checkbox(&mut hidden, "Hidden").changed();
            flags |= ui.checkbox(&mut locked, "Locked").changed();
            flags |= ui.checkbox(&mut as_box, "Show as box").changed();
        });
        let mut role = node.role;
        let mut cut = node.cut_positive;
        if node.group.is_some() {
            ui.horizontal(|ui| {
                ui.label("In its group:");
                for (r, label) in [(Role::Neutral, "Neutral"), (Role::Positive, "Positive"), (Role::Negative, "Negative"), (Role::Intersect, "Intersect")] {
                    flags |= ui.radio_value(&mut role, r, label).changed();
                }
            });
            if role == Role::Negative {
                flags |= ui.checkbox(&mut cut, "Cut shows the positive's surface").changed();
            }
        } else {
            ui.label(RichText::new("Group objects to cut one from another (positive / negative / intersect).").color(kit::DIM).small());
        }
        let mut family = node.family as usize;
        ui.horizontal(|ui| {
            ui.label("Family");
            egui::ComboBox::from_id_salt("family").selected_text(app.doc.project.family_names.get(family).cloned().unwrap_or_default()).show_ui(ui, |ui| {
                for k in 0..kit::FAMILIES.len() {
                    let name = app.doc.project.family_names.get(k).cloned().unwrap_or_default();
                    flags |= ui.selectable_value(&mut family, k, name).changed();
                }
            });
        });
        // links: follow another object, or keep facing one
        let others: Vec<(u64, String)> = app.doc.project.nodes.iter().filter(|n| n.id != id).map(|n| (n.id, n.name.clone())).collect();
        let (mut link, mut track) = (node.link, node.track);
        for (label, slot) in [("Linked to", &mut link), ("Tracks", &mut track)] {
            ui.horizontal(|ui| {
                ui.label(label);
                let current = slot.and_then(|l| others.iter().find(|o| o.0 == l)).map(|o| o.1.clone()).unwrap_or_else(|| "None".into());
                egui::ComboBox::from_id_salt(label).selected_text(current).show_ui(ui, |ui| {
                    flags |= ui.selectable_value(slot, None, "None").changed();
                    for (oid, name) in &others {
                        flags |= ui.selectable_value(slot, Some(*oid), name).changed();
                    }
                });
            });
        }
        if flags {
            begin_edit(app);
            let ids: Vec<u64> = if many { app.doc.selection.clone() } else { vec![id] };
            for i in ids {
                if let Some(n) = app.doc.project.node_mut(i) {
                    n.hidden = hidden;
                    n.locked = locked;
                    n.as_box = as_box;
                    n.family = family as u8;
                    if i == id {
                        n.role = role;
                        n.cut_positive = cut;
                        n.link = link;
                        n.track = track;
                    }
                }
            }
            crate::state::aim_trackers(&mut app.doc.project);
            app.doc.touch();
        }
    });
    if !open {
        app.dlg.attributes = false;
        app.dlg.editing = false;
    }
}

fn light(app: &mut App, ctx: &egui::Context) {
    if !app.dlg.light {
        return;
    }
    let mut open = true;
    egui::Window::new("Edit Light").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        let Some(id) = app.doc.selection.first().copied() else { return };
        let Some(Shape::Light(mut l)) = app.doc.project.node(id).map(|n| n.shape.clone()) else {
            ui.label("Select a light.");
            return;
        };
        let mut changed = false;
        egui::ComboBox::from_label("Kind").selected_text(l.kind.label()).show_ui(ui, |ui| {
            for k in LightKind::ALL {
                changed |= ui.selectable_value(&mut l.kind, k, k.label()).changed();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Colour");
            changed |= kit::colour_well(ui, &mut l.colour, "The light's colour");
        });
        changed |= ui.add(egui::Slider::new(&mut l.intensity, 0.0..=100.0).text("Intensity")).changed();
        if l.kind != LightKind::Radial {
            changed |= ui.add(egui::Slider::new(&mut l.exponents[0], 0.0..=10.0).text("Edge softness")).changed();
            l.exponents[1] = l.exponents[0];
        }
        let mut falloff = if l.switches & model::LIGHT_NO_FALLOFF != 0 { 0 } else if l.switches & model::LIGHT_SQUARED != 0 { 2 } else { 1 };
        ui.horizontal(|ui| {
            ui.label("Falloff");
            for (v, label) in [(0, "None"), (1, "Linear"), (2, "Squared")] {
                changed |= ui.radio_value(&mut falloff, v, label).changed();
            }
        });
        l.switches &= !(model::LIGHT_NO_FALLOFF | model::LIGHT_SQUARED);
        l.switches |= match falloff {
            0 => model::LIGHT_NO_FALLOFF,
            2 => model::LIGHT_SQUARED,
            _ => 0,
        };
        let mut shadows = l.switches & model::LIGHT_NO_SHADOWS == 0;
        if ui.checkbox(&mut shadows, "Casts shadows").changed() {
            changed = true;
        }
        l.switches = if shadows { l.switches & !model::LIGHT_NO_SHADOWS } else { l.switches | model::LIGHT_NO_SHADOWS };
        let mut on = l.switches & model::LIGHT_ON != 0;
        let _ = &mut on;
        if changed {
            begin_edit(app);
            if let Some(n) = app.doc.project.node_mut(id) {
                n.shape = Shape::Light(l);
            }
            app.doc.touch();
        }
    });
    if !open {
        app.dlg.light = false;
        app.dlg.editing = false;
    }
}

fn camera(app: &mut App, ctx: &egui::Context) {
    if !app.dlg.camera {
        return;
    }
    let mut open = true;
    egui::Window::new("Camera").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        let c = app.doc.project.cam.clone();
        let mut pos = [c.eye[0] * UNITS, c.eye[1] * UNITS, c.eye[2] * UNITS];
        let mut pivot = [c.pivot[0] * UNITS, c.pivot[1] * UNITS, c.pivot[2] * UNITS];
        let mut fov = c.fov.to_degrees();
        let mut pan = c.pan;
        let mut zoom = c.zoom[0];
        let mut changed = false;
        egui::Grid::new("cam").num_columns(4).show(ui, |ui| {
            changed |= row3(ui, "Position", &mut pos, 0.1, "");
            changed |= row3(ui, "Turns about", &mut pivot, 0.1, "");
        });
        changed |= ui.add(egui::Slider::new(&mut fov, 1.0..=170.0).text("Field of view").suffix("°")).changed();
        changed |= ui.add(egui::Slider::new(&mut zoom, 0.1..=4.0).text("Scale")).changed();
        ui.horizontal(|ui| {
            ui.label("Pan");
            changed |= ui.add(egui::DragValue::new(&mut pan[0]).speed(0.005).prefix("X ")).changed();
            changed |= ui.add(egui::DragValue::new(&mut pan[1]).speed(0.005).prefix("Y ")).changed();
        });
        if ui.button("Level the horizon").clicked() {
            begin_edit(app);
            let cam = &mut app.doc.project.cam;
            let ahead = math::add(cam.eye, cam.forward);
            cam.look_at(ahead);
            app.doc.touch();
        }
        if changed {
            begin_edit(app);
            let cam = &mut app.doc.project.cam;
            cam.eye = [pos[0] / UNITS, pos[1] / UNITS, pos[2] / UNITS];
            cam.pivot = [pivot[0] / UNITS, pivot[1] / UNITS, pivot[2] / UNITS];
            cam.fov = fov.to_radians();
            cam.pan = pan;
            cam.zoom = [zoom, zoom];
            app.doc.touch();
        }
    });
    if !open {
        app.dlg.camera = false;
        app.dlg.editing = false;
    }
}

fn render_options(app: &mut App, ctx: &egui::Context) {
    if !app.dlg.render_options {
        return;
    }
    let mut open = true;
    egui::Window::new("Render Options").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        let r = &mut app.doc.project.render;
        let before = (r.antialias, r.spatial, r.panorama, r.scale, r.mode.clone(), r.fill_outputs);
        ui.label("Anti-aliasing");
        ui.horizontal(|ui| {
            ui.radio_value(&mut r.antialias, 0, "Off (fast)");
            ui.radio_value(&mut r.antialias, 1, "Normal");
            ui.radio_value(&mut r.antialias, 2, "Fine (slow)");
        });
        ui.checkbox(&mut r.spatial, "Spatial optimisation (faster on busy scenes)");
        ui.horizontal(|ui| {
            ui.label("Render");
            ui.radio_value(&mut r.mode, String::new(), "Picture");
            ui.radio_value(&mut r.mode, "distance".to_owned(), "Distance mask").on_hover_text("A grey picture: near is dark, far is light");
            ui.radio_value(&mut r.mode, "altitude".to_owned(), "Altitude mask").on_hover_text("A grey picture: low is dark, high is light");
        });
        ui.checkbox(&mut r.panorama, "360° panorama");
        ui.checkbox(&mut r.fill_outputs, "Complete textures that surfaces ask more of").on_hover_text("Some old surfaces ask a texture for colour or bump it was never set to make; the original program used leftover values there. On: the texture makes it, so the scene renders (close, not exact). Off: such scenes are refused.");
        ui.horizontal(|ui| {
            ui.label("Render size");
            for (k, label) in [(0.25, "1:0.25"), (0.5, "1:0.5"), (1.0, "1:1"), (1.5, "1:1.5"), (2.0, "1:2"), (3.0, "1:3"), (4.0, "1:4")] {
                ui.selectable_value(&mut r.scale, k, label);
            }
        });
        let size = app.doc.project.size;
        let k = app.doc.project.render.scale;
        ui.label(RichText::new(format!("Picture: {} × {} pixels", (size[0] as f64 * k).round(), (size[1] as f64 * k).round())).color(kit::DIM).small());
        let r = &app.doc.project.render;
        if before != (r.antialias, r.spatial, r.panorama, r.scale, r.mode.clone(), r.fill_outputs) {
            app.doc.dirty = true;
        }
        // shadows, transparency and textures are switches of the scene
        let mut word = app.doc.project.extra.get("switches").and_then(|v| v.as_u64()).unwrap_or(959);
        let mut flip = false;
        ui.separator();
        for (bit, label) in [(0x8u64, "Shadows"), (0x2, "Transparency"), (0x200, "Textures")] {
            let mut on = word & bit != 0;
            if ui.checkbox(&mut on, label).changed() {
                word ^= bit;
                flip = true;
            }
        }
        if flip {
            app.doc.checkpoint();
            app.doc.project.extra = json!({ "switches": word });
            app.doc.touch();
        }
    });
    app.dlg.render_options = open;
}

fn document(app: &mut App, ctx: &egui::Context) {
    if !app.dlg.document {
        return;
    }
    let mut open = true;
    egui::Window::new("Document Setup").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        let mut name = app.doc.project.name.clone();
        ui.horizontal(|ui| {
            ui.label("Name");
            if ui.text_edit_singleline(&mut name).changed() {
                app.doc.project.name = name.clone();
                app.doc.dirty = true;
            }
        });
        let mut size = app.doc.project.size;
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Size");
            changed |= ui.add(egui::DragValue::new(&mut size[0]).range(16..=8192).suffix(" px")).changed();
            ui.label("×");
            changed |= ui.add(egui::DragValue::new(&mut size[1]).range(16..=8192).suffix(" px")).changed();
        });
        ui.label("Presets");
        ui.horizontal_wrapped(|ui| {
            for (label, w, h) in [("Default 4:3", 640, 480), ("Small", 480, 360), ("Square", 512, 512), ("Legal", 540, 720), ("Letter", 612, 792), ("Photo 3:2", 720, 480), ("Wide 16:9", 960, 540), ("HD", 1920, 1080), ("Panorama", 1440, 360)] {
                if ui.button(label).clicked() {
                    size = [w, h];
                    changed = true;
                }
            }
        });
        if changed {
            begin_edit(app);
            app.doc.project.size = size;
            app.doc.touch();
        }
    });
    if !open {
        app.dlg.document = false;
        app.dlg.editing = false;
    }
}

fn replicate(app: &mut App, ctx: &egui::Context) {
    if !app.dlg.replicate {
        return;
    }
    if app.dlg.rep_copies == 0 {
        app.dlg.rep_copies = 3;
        app.dlg.rep_offset = [20.48, 0.0, 0.0];
        app.dlg.rep_size = [100.0; 3];
    }
    let mut open = true;
    let mut go = false;
    egui::Window::new("Replicate").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.label(RichText::new("Each copy is moved, turned and resized from the one before it.").color(kit::DIM).small());
        ui.add(egui::Slider::new(&mut app.dlg.rep_copies, 1..=200).text("Copies"));
        egui::Grid::new("rep").num_columns(4).show(ui, |ui| {
            row3(ui, "Offset", &mut app.dlg.rep_offset, 0.1, "");
            row3(ui, "Rotate", &mut app.dlg.rep_turn, 0.5, "°");
            row3(ui, "Size", &mut app.dlg.rep_size, 0.5, "%");
        });
        if ui.add_enabled(!app.doc.selection.is_empty(), egui::Button::new("Replicate")).clicked() {
            go = true;
        }
    });
    if go {
        let d = &app.dlg;
        let (copies, off, turn, size) = (d.rep_copies, d.rep_offset, d.rep_turn, d.rep_size);
        app.doc.replicate(copies, [off[0] / UNITS, off[1] / UNITS, off[2] / UNITS], turn, [size[0] / 100.0, size[1] / 100.0, size[2] / 100.0]);
        open = false;
    }
    app.dlg.replicate = open;
}

fn animation(app: &mut App, ctx: &egui::Context) {
    if !app.dlg.animation {
        return;
    }
    let mut open = true;
    egui::Window::new("Animation Setup").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        let a = &mut app.doc.project.anim;
        let mut changed = false;
        changed |= ui.add(egui::Slider::new(&mut a.duration, 0.5..=600.0).text("Length").suffix(" s").logarithmic(true)).changed();
        changed |= ui.add(egui::Slider::new(&mut a.fps, 1.0..=60.0).text("Frames per second").integer()).changed();
        ui.label(RichText::new(format!("{} frames", (a.duration * a.fps).round() as u64 + 1)).color(kit::DIM).small());
        ui.horizontal(|ui| {
            ui.label("Working range");
            changed |= ui.add(egui::DragValue::new(&mut a.range[0]).speed(0.05).range(0.0..=a.duration).suffix(" s")).changed();
            ui.label("to");
            changed |= ui.add(egui::DragValue::new(&mut a.range[1]).speed(0.05).range(0.0..=a.duration).suffix(" s")).changed();
        });
        ui.horizontal(|ui| {
            ui.label("Play");
            changed |= ui.radio_value(&mut a.play_mode, 0, "Once").changed();
            changed |= ui.radio_value(&mut a.play_mode, 1, "Repeat").changed();
            changed |= ui.radio_value(&mut a.play_mode, 2, "Pendulum").changed();
        });
        changed |= ui.checkbox(&mut a.auto_key, "Auto-key: changes are recorded as keyframes").changed();
        if changed {
            a.range[1] = a.range[1].min(a.duration).max(a.range[0]);
            a.time = a.time.min(a.duration);
            app.doc.dirty = true;
        }
    });
    app.dlg.animation = open;
}

/// Render every frame of the working range and, unless only frames are
/// wanted, join them into a QuickTime movie.
fn start_movie(app: &mut App, out: PathBuf, frames_only: bool) {
    let project = app.doc.project.clone();
    let times = anim::frame_times(&project);
    let state = Arc::new(Mutex::new(Movie { done: 0, total: times.len(), result: None, stop: false }));
    app.dlg.movie_job = Some(state.clone());
    job::stop();
    let helper = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("kayzee3d-movie")));
    std::thread::Builder::new()
        .name("movie".into())
        .stack_size(64 << 20)
        .spawn(move || {
            let folder = if frames_only { out.clone() } else { std::env::temp_dir().join(format!("kayzee3d-frames-{}", std::process::id())) };
            let run = || -> Result<String, String> {
                std::fs::create_dir_all(&folder).map_err(|e| format!("Cannot make {}: {e}", folder.display()))?;
                let k = project.render.scale.max(0.05);
                let mut size = (((project.size[0] as f64) * k).round().max(8.0) as u32, ((project.size[1] as f64) * k).round().max(8.0) as u32);
                if !frames_only {
                    // movie frames need even sizes
                    size = (size.0 & !1, size.1 & !1);
                }
                let mut files = Vec::new();
                for (i, t) in times.iter().enumerate() {
                    if state.lock().unwrap().stop {
                        return Err("Stopped.".to_owned());
                    }
                    let mut posed = project.clone();
                    anim::pose(&mut posed, *t);
                    crate::state::aim_trackers(&mut posed);
                    let text = emit::description(&posed, size, posed.render.antialias);
                    let picture = job::render_now(&text, &setup::substrate_path(), 0).map_err(|e| format!("Frame {} could not be rendered: {e}", i + 1))?;
                    let file = folder.join(format!("{}-{:04}.png", project.name, i + 1));
                    std::fs::write(&file, picture.png()).map_err(|e| format!("Cannot write {}: {e}", file.display()))?;
                    files.push(file);
                    state.lock().unwrap().done = i + 1;
                }
                if frames_only {
                    return Ok(format!("{} frames saved in {}", files.len(), folder.display()));
                }
                // without the Mac helper (or when an .avi is asked for) the app writes the movie itself
                let avi = out.extension().is_some_and(|e| e.eq_ignore_ascii_case("avi"));
                let helper = helper.filter(|h| h.exists() && !avi);
                let Some(helper) = helper else {
                    let out = if avi { out.clone() } else { out.with_extension("avi") };
                    crate::movie::write_avi(&out, project.anim.fps.round().max(1.0) as u32, &files)?;
                    let _ = std::fs::remove_dir_all(&folder);
                    return Ok(format!("Movie saved: {}", out.display()));
                };
                let mut cmd = std::process::Command::new(helper);
                cmd.arg(&out).arg(format!("{}", project.anim.fps.round().max(1.0) as u32)).args(&files);
                let done = cmd.output().map_err(|e| format!("The movie writer could not be started: {e}"))?;
                if !done.status.success() {
                    return Err(format!("The movie could not be written: {}", String::from_utf8_lossy(&done.stderr).trim()));
                }
                let _ = std::fs::remove_dir_all(&folder);
                Ok(format!("Movie saved: {}", out.display()))
            };
            let result = run();
            state.lock().unwrap().result = Some(result);
        })
        .expect("the movie thread starts");
}

fn movie(app: &mut App, ctx: &egui::Context) {
    if !app.dlg.movie {
        return;
    }
    let mut open = true;
    let mut start: Option<(PathBuf, bool)> = None;
    let mut finished: Option<Result<String, String>> = None;
    egui::Window::new("Render Animation").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        let a = &app.doc.project.anim;
        let frames = anim::frame_times(&app.doc.project).len();
        ui.label(format!("{frames} frames, {:.2} s to {:.2} s at {} fps", a.range[0], a.range[1], a.fps));
        if !anim::has_animation(&app.doc.project) {
            ui.label(RichText::new("Nothing is animated yet: every frame will be the same.").color(kit::ACCENT).small());
        }
        if let Some(j) = app.dlg.movie_job.clone() {
            let mut m = j.lock().unwrap();
            if let Some(r) = m.result.take() {
                finished = Some(r);
            } else {
                ui.add(egui::ProgressBar::new(m.done as f32 / m.total.max(1) as f32).text(format!("frame {} of {}", m.done, m.total)));
                if ui.button("Stop").clicked() {
                    m.stop = true;
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            return;
        }
        ui.checkbox(&mut app.dlg.movie_frames_only, "Save numbered pictures instead of a movie");
        if ui.add_enabled(app.ready, egui::Button::new("Render…")).clicked() {
            if app.dlg.movie_frames_only {
                if let Some(folder) = rfd::FileDialog::new().set_title("Folder for the frames").pick_folder() {
                    start = Some((folder, true));
                }
            } else if let Some(file) = rfd::FileDialog::new().set_file_name(format!("{}.{}", app.doc.project.name, if cfg!(target_os = "macos") { "mov" } else { "avi" })).add_filter(if cfg!(target_os = "macos") { "QuickTime movie" } else { "AVI movie" }, if cfg!(target_os = "macos") { &["mov", "avi"] } else { &["avi"] }).save_file() {
                start = Some((file, false));
            }
        }
    });
    if let Some((out, frames_only)) = start {
        start_movie(app, out, frames_only);
    }
    if let Some(r) = finished {
        app.dlg.movie_job = None;
        match r {
            Ok(text) => app.say(text),
            Err(e) => app.warn(e),
        }
        open = false;
    }
    app.dlg.movie = open;
}

fn help(app: &mut App, ctx: &egui::Context) {
    if app.dlg.help {
        egui::Window::new("Keys and Mouse").open(&mut app.dlg.help).collapsible(false).show(ctx, |ui| {
            egui::Grid::new("keys").num_columns(2).striped(true).show(ui, |ui| {
                for (a, b) in [
                    ("Click", "Select an object (shift adds, ⌘ reaches inside groups)"),
                    ("Drag an object", "Slide it over the ground; it snaps to the ground, other objects or a grid"),
                    ("option-drag an object", "Move it straight up and down"),
                    ("Hold X, Y or Z while dragging", "Keep to that axis"),
                    ("Hold ⌘ while dragging", "No snapping"),
                    ("Drag a handle", "Resize (⌥ about the centre, ⌘ to turn instead)"),
                    ("Drag empty space", "Select everything inside the box"),
                    ("Right-drag", "Swing the camera around"),
                    ("Space-drag / middle-drag", "Slide the view"),
                    ("Scroll", "Move in and out"),
                    ("Arrows, Page Up/Down", "Nudge the selection (shift: bigger steps)"),
                    ("Tab", "Next object"),
                    ("Delete", "Remove the selection"),
                    ("⌘R / Esc", "Render / stop"),
                    ("⌘Z / ⇧⌘Z", "Undo / redo"),
                    ("⌘D, ⌘G, ⌘U", "Duplicate, group, ungroup"),
                    ("⌘E, ⌘M", "Attributes, Materials Lab"),
                ] {
                    ui.label(RichText::new(kit::keys(a)).strong());
                    ui.label(kit::keys(b));
                    ui.end_row();
                }
            });
        });
    }
    if app.dlg.about {
        egui::Window::new("About").open(&mut app.dlg.about).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(RichText::new("Kayzee 3D").strong().size(18.0));
            ui.label("A native Mac landscape modeller and renderer.");
            ui.label(RichText::new("Opens .br3 and .br5 scene files. Rendering uses the open-source stecyk3d renderer, with data tables read from your own disc.").color(kit::DIM).small());
        });
    }
}

/// A flat face showing a picture chosen from disk.
pub fn add_picture_object(app: &mut App) {
    let Some(path) = rfd::FileDialog::new().add_filter("Pictures", &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "tga", "PNG", "JPG", "BMP", "TIF"]).pick_file() else { return };
    let img = match image::open(&path) {
        Ok(i) => i.to_rgba8(),
        Err(e) => {
            app.warn(format!("That picture could not be read: {e}"));
            return;
        }
    };
    // keep pictures to a size the renderer handles quickly
    let img = if img.width().max(img.height()) > 1024 {
        let k = 1024.0 / img.width().max(img.height()) as f32;
        image::imageops::resize(&img, (img.width() as f32 * k) as u32, (img.height() as f32 * k) as u32, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let (w, h) = img.dimensions();
    let pixels: Vec<String> = img.pixels().map(|p| format!("{:02x}{:02x}{:02x}{:02x}", p.0[3], p.0[0], p.0[1], p.0[2])).collect();
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Picture".into());
    app.doc.checkpoint();
    let p = &mut app.doc.project;
    let tex_name = format!("pic{}:{stem}", p.textures.len());
    p.textures.push(json!({ "name": tex_name, "kind": "picture", "picture": { "width": w, "height": h, "pixels": pixels } }));
    let record = json!({ "flags": 65536, "channels": {
        "diffuse_color": { "a": [1.0, 1.0, 1.0], "b": [0.0, 0.0, 0.0], "alpha": 1.0, "switches": 0x10000000u32, "a_tex": tex_name },
        "ambient_color": { "a": [1.0, 1.0, 1.0], "b": [0.0, 0.0, 0.0], "alpha": 1.0, "switches": 0x10000000u32, "a_tex": tex_name },
        "diffusion": 1.0, "ambience": 0.35 } });
    let mat = create::own_material(p, &stem, record);
    let aspect = w as f64 / h.max(1) as f64;
    let m = math::from_axes([aspect, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [0.0, 1.0, 0.0]);
    let id = p.add("Picture", Shape::Square, m, Some(mat));
    app.doc.selection = vec![id];
    app.doc.touch();
}
