//! Preset libraries: materials and textures read from the preset files of the
//! user's own disc, the user's own saved objects, and skies taken from scenes.

use std::path::{Path, PathBuf};

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, TextureOptions, Vec2};
use serde_json::Value;
use stecyk3d::scene_export;
use stecyk3d::scene_library::{Library, LibraryKind};

use crate::app::App;
use crate::{fileio, import, kit, matlab, setup};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Objects,
    Materials,
    Textures,
    Skies,
}

#[derive(Default)]
pub struct State {
    pub open: Option<Kind>,
    files: Vec<PathBuf>,
    listed_for: Option<u8>,
    file: Option<PathBuf>,
    /// The open file's presets: (name, record or texture).
    presets: Vec<(String, Value)>,
    textures: Vec<Value>,
    error: Option<String>,
    filter: String,
    /// Small rendered pictures of the open file's presets, made in the background.
    thumbs: Vec<Option<TextureHandle>>,
    rendered: Arc<Mutex<Vec<Option<Vec<u8>>>>>,
    run: Arc<AtomicU64>,
    picked: Option<usize>,
}

const THUMB: u32 = 72;

/// A ball on a floor wearing one material, as the renderer's description.
pub fn thumb_scene(record: Value, textures: Vec<Value>, size: u32) -> String {
    let mut p = crate::create::new_project();
    p.size = [size, size];
    p.textures = textures;
    let m = crate::create::own_material(&mut p, "subject", record);
    p.add("ball", crate::model::Shape::Sphere, crate::math::translation([0.0, 1.0, 0.0]), Some(m));
    p.cam.eye = [1.9, 1.9, -3.0];
    p.cam.look_at([0.0, 0.95, 0.0]);
    p.cam.fov = 0.95;
    crate::emit::description(&p, (size, size), 0)
}

/// The record that shows a texture: it feeds the surface's colour.
pub fn texture_record(texture: &Value) -> Value {
    serde_json::json!({"flags": 65536, "channels": {
        "diffuse_color": {"a": [1.0, 1.0, 1.0], "b": [0.0, 0.0, 0.0], "alpha": 1.0, "switches": 0x10000000u32, "a_tex": texture["name"]},
        "ambient_color": [1.0, 1.0, 1.0], "diffusion": 1.0, "ambience": 0.25}})
}

/// Folders searched for preset files: the installed copy and extra folders the user added.
fn folders() -> Vec<PathBuf> {
    let mut out = vec![setup::data_folder().join("from-disc").join("prog").join("Presets"), setup::data_folder().join("Library")];
    if let Ok(text) = std::fs::read_to_string(setup::data_folder().join("library-folders.txt")) {
        out.extend(text.lines().filter(|l| !l.trim().is_empty()).map(PathBuf::from));
    }
    out
}

fn add_folder(folder: &Path) {
    let file = setup::data_folder().join("library-folders.txt");
    let mut text = std::fs::read_to_string(&file).unwrap_or_default();
    let line = folder.to_string_lossy();
    if !text.lines().any(|l| l == line) {
        text.push_str(&line);
        text.push('\n');
        let _ = std::fs::create_dir_all(setup::data_folder());
        let _ = std::fs::write(file, text);
    }
}

pub fn files_with(ext: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for f in folders() {
        let mut stack = vec![(f, 0)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() && depth < 2 {
                    stack.push((p, depth + 1));
                } else if p.extension().is_some_and(|x| ext.iter().any(|w| x.eq_ignore_ascii_case(w))) {
                    out.push(p);
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn objects_folder() -> PathBuf {
    setup::data_folder().join("Library").join("Objects")
}

/// Read a .mat or .brt file into named presets in the renderer's form.
pub fn read_presets(path: &Path) -> Result<(Vec<(String, Value)>, Vec<Value>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Cannot read it: {e}"))?;
    let textures_file = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("brt"));
    let kind = if textures_file { LibraryKind::Textures } else { LibraryKind::Materials };
    let lib = Library::read(&bytes, kind).map_err(|e| format!("{e}"))?;
    let json: Value = serde_json::from_str(&scene_export::library_json(&lib).write()).map_err(|e| e.to_string())?;
    let empty = Vec::new();
    let pictures = json["pictures"].as_array().unwrap_or(&empty);
    let shared = json["textures"].as_array().unwrap_or(&empty);
    let (textures, names) = import::textures_of(shared, pictures);
    let mut presets = Vec::new();
    for (k, p) in json["presets"].as_array().unwrap_or(&empty).iter().enumerate() {
        let name = p["name"].as_str().map(|s| s.trim_end_matches('\t').trim().to_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| format!("Preset {}", k + 1));
        if p["material"].is_object() {
            presets.push((name, import::material_of(&p["material"], &names)));
        } else if p["texture"].is_object() {
            let (t, _) = import::textures_of(std::slice::from_ref(&p["texture"]), pictures);
            if let Some(mut t) = t.into_iter().next() {
                t["name"] = serde_json::json!(format!("P:{name}"));
                presets.push((name, t));
            }
        }
    }
    Ok((presets, textures))
}

fn load(s: &mut State, path: &Path, ctx: &egui::Context) {
    s.file = Some(path.to_path_buf());
    match read_presets(path) {
        Ok((p, t)) => {
            s.presets = p;
            s.textures = t;
            s.error = None;
        }
        Err(e) => {
            s.presets.clear();
            s.textures.clear();
            s.error = Some(e);
        }
    }
    s.picked = None;
    s.thumbs = vec![None; s.presets.len()];
    s.rendered = Arc::new(Mutex::new(vec![None; s.presets.len()]));
    // draw the small pictures one after another, off the main thread
    let mine = s.run.fetch_add(1, Ordering::SeqCst) + 1;
    let (run, out, presets, textures, ctx) = (s.run.clone(), s.rendered.clone(), s.presets.clone(), s.textures.clone(), ctx.clone());
    let is_textures = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("brt"));
    let _ = std::thread::Builder::new().name("thumbnails".into()).stack_size(64 << 20).spawn(move || {
        for (k, (_, value)) in presets.into_iter().enumerate() {
            if run.load(Ordering::SeqCst) != mine {
                return;
            }
            let text = if is_textures {
                thumb_scene(texture_record(&value), vec![value], THUMB)
            } else {
                // only the textures this material names go along
                let record = value.to_string();
                let used: Vec<Value> = textures.iter().filter(|t| t["name"].as_str().is_some_and(|n| record.contains(&format!("\"{n}\"")))).cloned().collect();
                thumb_scene(value, used, THUMB)
            };
            // a render stopped by another one starting is simply tried again
            let mut result = crate::job::render_now(&text, &setup::substrate_path(), 0);
            let mut tries = 0;
            while tries < 40 && result.as_ref().err().is_some_and(|e| e.contains("cancelled")) && run.load(Ordering::SeqCst) == mine {
                std::thread::sleep(std::time::Duration::from_millis(60));
                result = crate::job::render_now(&text, &setup::substrate_path(), 0);
                tries += 1;
            }
            let rgb = match result {
                Ok(p) if p.width == THUMB && p.height == THUMB => p.levels,
                // a preset the renderer refuses shows as a plain grey square with a mark
                other => {
                    if std::env::var_os("RIDGELINE_DEBUG").is_some() {
                        eprintln!("thumbnail {k}: {}", other.err().unwrap_or_else(|| "wrong size".into()));
                    }
                    let mut v = vec![150u8; (THUMB * THUMB * 3) as usize];
                    for i in 0..THUMB as usize {
                        for j in [i, THUMB as usize - 1 - i] {
                            let at = (i * THUMB as usize + j) * 3;
                            v[at..at + 3].copy_from_slice(&[110, 110, 110]);
                        }
                    }
                    v
                }
            };
            if let Ok(mut all) = out.lock() {
                if k < all.len() {
                    all[k] = Some(rgb);
                }
            }
            ctx.request_repaint();
        }
    });
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(kind) = app.lib.open else { return };
    let code = kind as u8;
    if app.lib.listed_for != Some(code) {
        app.lib.listed_for = Some(code);
        app.lib.files = match kind {
            Kind::Materials => files_with(&["mat"]),
            Kind::Textures => files_with(&["brt"]),
            Kind::Objects => {
                let mut v: Vec<PathBuf> = std::fs::read_dir(objects_folder()).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(fileio::EXT))).collect()).unwrap_or_default();
                v.sort();
                v
            }
            Kind::Skies => Vec::new(),
        };
        app.lib.file = None;
        app.lib.presets.clear();
        app.lib.error = None;
    }
    let title = match kind {
        Kind::Objects => "Object Library",
        Kind::Materials => "Material Presets",
        Kind::Textures => "Texture Presets",
        Kind::Skies => "Sky Library",
    };
    let mut open = true;
    let mut pick_file: Option<PathBuf> = None;
    let mut use_preset: Option<usize> = None;
    let mut use_object: Option<PathBuf> = None;
    let mut relist = false;
    egui::Window::new(title).open(&mut open).default_size([640.0, 470.0]).collapsible(false).frame(egui::Frame::window(&ctx.style()).fill(Color32::from_gray(246))).show(ctx, |ui| match kind {
        Kind::Skies => {
            ui.label("Ready-made skies:");
            ui.horizontal_wrapped(|ui| {
                for (name, v) in crate::sky_ui::presets() {
                    if ui.button(name).clicked() {
                        app.doc.checkpoint();
                        app.doc.project.sky = v;
                        app.doc.touch();
                    }
                }
            });
            ui.separator();
            if ui.button("Take the sky from a scene file…").clicked() {
                sky_from_scene(app);
            }
            ui.label(RichText::new("The disc's own sky preset file is in a format the renderer's reader does not open; skies can be taken from any scene file instead.").small().color(kit::DIM));
        }
        Kind::Objects => {
            ui.horizontal(|ui| {
                if ui.add_enabled(!app.doc.selection.is_empty(), egui::Button::new("Add the selection to the library…")).clicked() {
                    save_object(app);
                    relist = true;
                }
                if ui.button("Import a 3D model…").clicked() {
                    fileio::import_model(app);
                }
            });
            ui.separator();
            if app.lib.files.is_empty() {
                ui.label("Your object library is empty.");
                ui.label(RichText::new("Select objects and add them here to reuse them in other scenes. The disc's .obp object files are in a format the renderer's reader does not open; its OBJ and DXF models import through “Import a 3D model”.").small().color(kit::DIM));
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                for f in &app.lib.files {
                    let name = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    if ui.button(name).on_hover_text("Add to the scene").clicked() {
                        use_object = Some(f.clone());
                    }
                }
            });
        }
        Kind::Materials | Kind::Textures => {
            let files = app.lib.files.clone();
            let names: Vec<String> = app.lib.presets.iter().map(|p| p.0.clone()).collect();
            let ids: Vec<Option<egui::TextureId>> = app.lib.thumbs.iter().map(|t| t.as_ref().map(|t| t.id())).collect();
            let mut picked = app.lib.picked;
            let needle = app.lib.filter.to_lowercase();
            let whole = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(150.0);
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(138.0), Sense::hover());
                    ui.painter().rect_filled(rect.translate(Vec2::new(2.0, 3.0)), 0.0, Color32::from_black_alpha(50));
                    ui.painter().rect_filled(rect, 0.0, Color32::from_gray(205));
                    if let Some(id) = picked.and_then(|k| ids.get(k).copied().flatten()) {
                        ui.painter().image(id, rect, whole, Color32::WHITE);
                    }
                    ui.add_space(8.0);
                    egui::ScrollArea::vertical().id_salt("files").max_height(250.0).show(ui, |ui| {
                        ui.set_max_width(136.0);
                        ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                            for f in &files {
                                let name = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                                let on = app.lib.file.as_ref() == Some(f);
                                let text = if on { RichText::new(name).strong().color(Color32::BLACK) } else { RichText::new(name).color(Color32::from_gray(110)) };
                                if ui.add(egui::Label::new(text).sense(Sense::click())).clicked() {
                                    pick_file = Some(f.clone());
                                }
                            }
                        });
                    });
                    if files.is_empty() {
                        ui.label(RichText::new("No preset files found. Set up from your disc, or add a folder that holds .mat or .brt files.").small().color(kit::DIM));
                    }
                });
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    let title = picked.and_then(|k| names.get(k).cloned()).unwrap_or_else(|| if app.lib.file.is_some() { "Choose a preset".to_owned() } else { "Choose a category".to_owned() });
                    ui.label(RichText::new(title).size(20.0));
                    if let Some(e) = &app.lib.error {
                        ui.label(RichText::new(format!("This file could not be read: {e}")).color(kit::ACCENT));
                    }
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().id_salt("grid").max_height(330.0).show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);
                            for (k, name) in names.iter().enumerate() {
                                if !needle.is_empty() && !name.to_lowercase().contains(&needle) {
                                    continue;
                                }
                                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(62.0), Sense::click());
                                ui.painter().rect_filled(rect, 0.0, Color32::from_gray(190));
                                if let Some(id) = ids.get(k).copied().flatten() {
                                    ui.painter().image(id, rect.shrink(1.0), whole, Color32::WHITE);
                                }
                                let edge = if picked == Some(k) { Stroke::new(2.0, kit::solid::BLUE) } else { Stroke::new(1.0, Color32::from_gray(70)) };
                                ui.painter().rect_stroke(rect, 0.0, edge, egui::StrokeKind::Inside);
                                if resp.clicked() {
                                    picked = Some(k);
                                }
                                if resp.double_clicked() {
                                    use_preset = Some(k);
                                }
                                resp.on_hover_text(name);
                            }
                        });
                    });
                });
            });
            app.lib.picked = picked;
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Add a folder…").on_hover_text("Add a folder of .mat or .brt preset files").clicked() {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        add_folder(&folder);
                        relist = true;
                    }
                }
                ui.label("Find");
                ui.add(egui::TextEdit::singleline(&mut app.lib.filter).desired_width(120.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if kind == Kind::Materials { "Apply to selection" } else { "Add to scene" };
                    if ui.add_enabled(picked.is_some(), egui::Button::new(RichText::new(label).color(kit::INK).strong()).fill(kit::ACCENT)).clicked() {
                        use_preset = picked;
                    }
                });
            });
        }
    });
    if relist {
        app.lib.listed_for = None;
    }
    if let Some(f) = pick_file {
        load(&mut app.lib, &f, ctx);
    }
    // show the first category straight away
    if matches!(kind, Kind::Materials | Kind::Textures) && app.lib.file.is_none() && app.ready {
        if let Some(f) = app.lib.files.first().cloned() {
            load(&mut app.lib, &f, ctx);
        }
    }
    // pictures that finished rendering become textures
    if let Ok(done) = app.lib.rendered.clone().lock() {
        for (k, rgb) in done.iter().enumerate() {
            if let (Some(rgb), Some(slot)) = (rgb, app.lib.thumbs.get_mut(k)) {
                if slot.is_none() {
                    *slot = Some(ctx.load_texture(format!("thumb{k}"), ColorImage::from_rgb([THUMB as usize, THUMB as usize], rgb), TextureOptions::LINEAR));
                }
            }
        }
    }
    if let Some(k) = use_preset {
        let (name, value) = app.lib.presets[k].clone();
        if kind == Kind::Materials {
            if app.doc.selection.is_empty() {
                app.warn("Select an object to give the material to.");
            } else {
                app.doc.checkpoint();
                let textures = app.lib.textures.clone();
                let i = matlab::adopt(app, &name, value, textures);
                matlab::assign(app, i);
                if app.mat.index.is_some() && app.lab == crate::app::Lab::Materials {
                    app.mat.index = Some(i);
                }
                app.doc.touch();
                app.say(format!("“{name}” applied."));
            }
        } else {
            app.doc.checkpoint();
            let mut t = value;
            t["name"] = serde_json::json!(format!("P{}:{name}", app.doc.project.textures.len()));
            app.doc.project.textures.push(t);
            app.doc.touch();
            app.say(format!("Texture “{name}” added. Pick it for a channel in the Materials Lab."));
        }
    }
    if let Some(f) = use_object {
        match fileio::open(&f) {
            Ok(other) => {
                app.doc.checkpoint();
                let ids = fileio::merge(&mut app.doc.project, other);
                app.doc.selection = ids;
                app.doc.touch();
            }
            Err(e) => app.warn(e),
        }
    }
    if !open {
        app.lib.open = None;
        app.lib.listed_for = None;
        app.lib.run.fetch_add(1, Ordering::SeqCst);
    }
}

/// Save the selection as a small scene in the user's object library.
fn save_object(app: &mut App) {
    let name = app.doc.one().map(|n| n.name.clone()).unwrap_or_else(|| "Objects".into());
    let folder = objects_folder();
    let _ = std::fs::create_dir_all(&folder);
    let Some(path) = rfd::FileDialog::new().set_directory(&folder).set_file_name(format!("{name}.{}", fileio::EXT)).add_filter("Library object", &[fileio::EXT]).save_file() else { return };
    let mut p = app.doc.project.clone();
    let keep: Vec<u64> = app.doc.selection.iter().flat_map(|s| p.with_descendants(*s)).collect();
    p.nodes.retain(|n| keep.contains(&n.id));
    match fileio::write(&p, &path) {
        Ok(()) => app.say("Added to the object library."),
        Err(e) => app.warn(e),
    }
}

/// Replace this scene's sky with the sky of another scene file.
pub fn sky_from_scene(app: &mut App) {
    let Some(path) = rfd::FileDialog::new().add_filter("Scenes", &["br3", "br5", fileio::EXT, "BR3", "BR5"]).pick_file() else { return };
    match fileio::open(&path) {
        Ok(other) => {
            app.doc.checkpoint();
            // cloud layers name textures by place in the list: bring them along
            let base = app.doc.project.textures.len();
            let mut sky = other.sky.clone();
            for key in ["stratus_texture", "cumulus_texture"] {
                if let Some(i) = sky["sky"][key].as_u64() {
                    if let Some(t) = other.textures.get(i as usize) {
                        let mut t = t.clone();
                        t["name"] = serde_json::json!(format!("S{}:{}", app.doc.project.textures.len(), matlab::nice(t["name"].as_str().unwrap_or("cloud"))));
                        app.doc.project.textures.push(t);
                        sky["sky"][key] = serde_json::json!(app.doc.project.textures.len() - 1);
                    }
                }
            }
            let _ = base;
            app.doc.project.sky = sky;
            app.doc.touch();
            app.say("Sky taken from that scene.");
        }
        Err(e) => app.warn(format!("That scene could not be read. {e}")),
    }
}
