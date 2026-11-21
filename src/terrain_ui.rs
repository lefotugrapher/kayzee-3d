//! The Terrain Editor: a top view you paint on, elevation tools, a filter
//! curve, pictures, resolution and clipping.

use std::sync::Arc;

use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, TextureOptions, Vec2};

use crate::app::{App, Lab};
use crate::kit;
use crate::model::Shape;
use crate::terrain_ops::{self as ops, Brush, Fractal, Grid};

pub struct State {
    id: u64,
    grid: Option<Grid>,
    undo: Vec<Grid>,
    redo: Vec<Grid>,
    texture: Option<TextureHandle>,
    dirty: bool,
    tab: u8,
    brush: Brush,
    brush_size: f32,
    brush_strength: f32,
    brush_soft: f32,
    brush_level: f32,
    gradient: usize,
    fractal: Fractal,
    detail: u32,
    scale: f32,
    seed: u64,
    curve: Vec<(f32, f32)>,
    low: f32,
    high: f32,
    blend_mode: u8,
    blend_amount: f32,
    picture: Option<Grid>,
    painting: bool,
    mirrored: bool,
    smooth: bool,
    solid: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            id: 0,
            grid: None,
            undo: Vec::new(),
            redo: Vec::new(),
            texture: None,
            dirty: true,
            tab: 0,
            brush: Brush::Raise,
            brush_size: 8.0,
            brush_strength: 0.6,
            brush_soft: 0.7,
            brush_level: 0.5,
            gradient: 1,
            fractal: Fractal::Ridges,
            detail: 7,
            scale: 2.0,
            seed: 1,
            curve: vec![(0.0, 0.0), (0.25, 0.25), (0.5, 0.5), (0.75, 0.75), (1.0, 1.0)],
            low: 0.0,
            high: 1.0,
            blend_mode: 1,
            blend_amount: 0.5,
            picture: None,
            painting: false,
            mirrored: false,
            smooth: true,
            solid: true,
        }
    }
}

pub fn open(app: &mut App) {
    let Some(n) = app.doc.one() else { return };
    let Shape::Terrain(t) = &n.shape else { return };
    let s = &mut app.ter;
    s.id = n.id;
    s.grid = Some(Grid::from_u16(t.n, &t.heights));
    s.undo.clear();
    s.redo.clear();
    s.dirty = true;
    s.low = t.lower.max(0.0) as f32;
    s.high = t.upper.min(1.0) as f32;
    s.mirrored = t.mirrored;
    s.smooth = t.smooth;
    s.solid = t.solid_faces != 0;
    s.seed = app.rng | 1;
    app.lab = Lab::Terrain;
}

fn commit(app: &mut App) {
    let s = &app.ter;
    let Some(g) = &s.grid else { return };
    let (id, n, heights) = (s.id, g.n, Arc::new(g.to_u16()));
    let (low, high, mirrored, smooth, solid) = (s.low, s.high, s.mirrored, s.smooth, s.solid);
    app.doc.checkpoint();
    if let Some(node) = app.doc.project.node_mut(id) {
        if let Shape::Terrain(t) = &mut node.shape {
            // a new resolution keeps the terrain the same size in the world
            if t.n != n {
                let k = t.n as f64 / n as f64;
                for c in 0..3 {
                    node.m[c] *= k;
                    node.m[6 + c] *= k;
                }
            }
            t.n = n;
            t.heights = heights;
            t.lower = if low <= 0.0 { -1e-5 } else { low as f64 };
            t.upper = if high >= 1.0 { 1.00001 } else { high as f64 };
            t.mirrored = mirrored;
            t.smooth = smooth;
            t.solid_faces = if solid { 63 } else { 0 };
        }
    }
    app.doc.touch();
}

impl State {
    fn push(&mut self) {
        if let Some(g) = &self.grid {
            self.undo.push(g.clone());
            if self.undo.len() > 40 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }
    fn apply(&mut self, f: impl FnOnce(&mut Grid)) {
        self.push();
        if let Some(g) = &mut self.grid {
            f(g);
        }
        self.dirty = true;
    }
    fn replace(&mut self, f: impl FnOnce(&Grid) -> Grid) {
        self.push();
        if let Some(g) = &self.grid {
            self.grid = Some(f(g));
        }
        self.dirty = true;
    }
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    if app.ter.grid.is_none() {
        app.lab = Lab::None;
        return;
    }
    let mut leave: Option<bool> = None;
    let s = &mut app.ter;
    if s.dirty {
        let g = s.grid.as_ref().unwrap();
        let mut rgba = ops::relief_rgba(g, ops::GRADIENTS[s.gradient].1);
        // clipped heights are shown dimmed
        for (k, v) in g.h.iter().enumerate() {
            if *v < s.low || *v > s.high {
                for c in 0..3 {
                    rgba[k * 4 + c] /= 4;
                }
                rgba[k * 4] = rgba[k * 4].saturating_add(60);
            }
        }
        let image = ColorImage::from_rgba_unmultiplied([g.n, g.n], &rgba);
        match &mut s.texture {
            Some(t) => t.set(image, TextureOptions::LINEAR),
            None => s.texture = Some(ctx.load_texture("terrain", image, TextureOptions::LINEAR)),
        }
        s.dirty = false;
    }

    egui::SidePanel::left("ter-left").exact_width(300.0).resizable(false).frame(egui::Frame::new().fill(kit::PANEL).inner_margin(14.0)).show(ctx, |ui| {
        ui.label(RichText::new("TERRAIN EDITOR").color(kit::ACCENT).strong());
        ui.horizontal(|ui| {
            for (k, label) in ["Elevation", "Filtering", "Pictures"].iter().enumerate() {
                if ui.selectable_label(s.tab == k as u8, *label).clicked() {
                    s.tab = k as u8;
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().max_height(ui.available_height() - 120.0).show(ui, |ui| match s.tab {
            0 => elevation(s, ui),
            1 => filtering(s, ui),
            _ => pictures(s, ui),
        });
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            if ui.add_sized(Vec2::new(272.0, 32.0), egui::Button::new(RichText::new("Done").color(kit::INK).strong()).fill(kit::ACCENT)).clicked() {
                leave = Some(true);
            }
            if ui.button("Cancel").clicked() {
                leave = Some(false);
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(!s.undo.is_empty(), egui::Button::new("Undo")).clicked() {
                    if let (Some(prev), Some(cur)) = (s.undo.pop(), s.grid.take()) {
                        s.redo.push(cur);
                        s.grid = Some(prev);
                        s.dirty = true;
                    }
                }
                if ui.add_enabled(!s.redo.is_empty(), egui::Button::new("Redo")).clicked() {
                    if let (Some(next), Some(cur)) = (s.redo.pop(), s.grid.take()) {
                        s.undo.push(cur);
                        s.grid = Some(next);
                        s.dirty = true;
                    }
                }
            });
        });
    });

    egui::SidePanel::right("ter-right").exact_width(210.0).resizable(false).frame(egui::Frame::new().fill(kit::PANEL).inner_margin(14.0)).show(ctx, |ui| {
        kit::heading(ui, "BRUSH");
        ui.horizontal_wrapped(|ui| {
            for b in Brush::ALL {
                if ui.selectable_label(s.brush == b, b.label()).clicked() {
                    s.brush = b;
                }
            }
        });
        ui.add(egui::Slider::new(&mut s.brush_size, 1.0..=64.0).text("Size"));
        ui.add(egui::Slider::new(&mut s.brush_strength, 0.02..=1.0).text("Flow"));
        ui.add(egui::Slider::new(&mut s.brush_soft, 0.0..=1.0).text("Softness"));
        if s.brush == Brush::Flatten {
            ui.add(egui::Slider::new(&mut s.brush_level, 0.0..=1.0).text("Level"));
        }
        ui.label(RichText::new("Paint on the map. Hold option to pick up a level.").small().color(kit::DIM));
        ui.add_space(8.0);
        kit::heading(ui, "GRID");
        let n = s.grid.as_ref().unwrap().n;
        egui::ComboBox::from_id_salt("grid").selected_text(format!("{n} × {n}")).show_ui(ui, |ui| {
            for size in [16usize, 32, 64, 128, 256, 512, 1024] {
                if ui.selectable_label(n == size, format!("{size} × {size}")).clicked() && n != size {
                    s.replace(|g| ops::resample(g, size));
                }
            }
        });
        ui.add_space(8.0);
        kit::heading(ui, "CLIPPING");
        let (mut lo, mut hi) = (s.low, s.high);
        ui.add(egui::Slider::new(&mut lo, 0.0..=1.0).text("Bottom"));
        ui.add(egui::Slider::new(&mut hi, 0.0..=1.0).text("Top"));
        if (lo, hi) != (s.low, s.high) {
            s.low = lo.min(hi - 0.01).max(0.0);
            s.high = hi.max(lo + 0.01).min(1.0);
            s.dirty = true;
        }
        ui.label(RichText::new("Heights outside this band are cut away.").small().color(kit::DIM));
        ui.add_space(8.0);
        kit::heading(ui, "COLOUR MAP");
        egui::ComboBox::from_id_salt("gradient").selected_text(ops::GRADIENTS[s.gradient].0).show_ui(ui, |ui| {
            for (k, (name, _)) in ops::GRADIENTS.iter().enumerate() {
                if ui.selectable_label(s.gradient == k, *name).clicked() {
                    s.gradient = k;
                    s.dirty = true;
                }
            }
        });
        ui.add_space(8.0);
        kit::heading(ui, "SURFACE");
        ui.checkbox(&mut s.smooth, "Smooth shading");
        ui.checkbox(&mut s.solid, "Solid (for cutting with groups)");
        ui.checkbox(&mut s.mirrored, "Symmetric lattice");
    });

    egui::CentralPanel::default().frame(egui::Frame::new().fill(kit::CANVAS).inner_margin(16.0)).show(ctx, |ui| {
        let area = ui.available_rect_before_wrap();
        let side = area.width().min(area.height());
        let rect = Rect::from_center_size(area.center(), Vec2::splat(side));
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        if let Some(t) = &s.texture {
            ui.painter().image(t.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, kit::EDGE), egui::StrokeKind::Outside);
        let n = s.grid.as_ref().unwrap().n as f32;
        if let Some(p) = resp.hover_pos() {
            ui.painter().circle_stroke(p, s.brush_size / n * side, Stroke::new(1.0, kit::ACCENT));
        }
        if resp.drag_started() || resp.clicked() {
            s.push();
            s.painting = true;
        }
        if (resp.dragged() || resp.clicked()) && s.painting {
            if let Some(p) = resp.interact_pointer_pos() {
                let (gx, gy) = ((p.x - rect.left()) / side * n, (p.y - rect.top()) / side * n);
                if ui.input(|i| i.modifiers.alt) {
                    s.brush_level = s.grid.as_ref().unwrap().sample(gx, gy);
                } else {
                    s.seed = s.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    let (b, size, flow, soft, level, seed) = (s.brush, s.brush_size, s.brush_strength, s.brush_soft, s.brush_level, s.seed);
                    if let Some(g) = &mut s.grid {
                        ops::dab(g, b, gx, gy, size, flow, soft, level, seed);
                    }
                    s.dirty = true;
                    ctx.request_repaint();
                }
            }
        }
        if resp.drag_stopped() {
            s.painting = false;
        }
    });

    if let Some(keep) = leave {
        if keep {
            commit(app);
        }
        app.ter.grid = None;
        app.ter.texture = None;
        app.ter.picture = None;
        app.lab = Lab::None;
    }
}

fn amount_button(ui: &mut egui::Ui, label: &str, tip: &str) -> Option<f32> {
    let r = ui.add(egui::Button::new(label).min_size(Vec2::new(128.0, 26.0)).sense(Sense::click_and_drag())).on_hover_text(format!("{tip} Click, or drag sideways for more or less."));
    if r.clicked() {
        return Some(1.0);
    }
    if r.dragged() && r.drag_delta().x != 0.0 {
        return Some(r.drag_delta().x * 0.05);
    }
    None
}

fn elevation(s: &mut State, ui: &mut egui::Ui) {
    kit::heading(ui, "NEW RELIEF");
    egui::ComboBox::from_id_salt("fractal").selected_text(s.fractal.label()).show_ui(ui, |ui| {
        for f in Fractal::ALL {
            ui.selectable_value(&mut s.fractal, f, f.label());
        }
    });
    ui.add(egui::Slider::new(&mut s.detail, 1..=10).text("Detail"));
    ui.add(egui::Slider::new(&mut s.scale, 1.0..=8.0).text("Feature count"));
    ui.horizontal(|ui| {
        if ui.button("Fractal").on_hover_text("Replace the terrain with new fractal relief").clicked() {
            s.seed = s.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let (kind, seed, detail, scale) = (s.fractal, s.seed, s.detail, s.scale);
            s.replace(|g| ops::fractal(g.n, kind, seed, detail, scale));
        }
        if ui.button("Add to terrain").on_hover_text("Mix new relief into what is there").clicked() {
            s.seed = s.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let (kind, seed, detail, scale) = (s.fractal, s.seed, s.detail, s.scale);
            s.apply(|g| {
                let f = ops::fractal(g.n, kind, seed, detail, scale);
                ops::blend(g, &f, 1, 0.35);
                ops::normalise(g);
            });
        }
        if ui.button("New (flat)").clicked() {
            s.replace(|g| Grid::flat(g.n, 0.0));
        }
    });
    ui.add_space(8.0);
    kit::heading(ui, "SHAPE");
    let seed = s.seed;
    egui::Grid::new("tools").num_columns(2).spacing([6.0, 6.0]).show(ui, |ui| {
        let mut k = 0;
        let mut cell = |ui: &mut egui::Ui, s: &mut State, label: &str, tip: &str, f: &dyn Fn(&mut Grid, f32)| {
            if let Some(a) = amount_button(ui, label, tip) {
                s.apply(|g| f(g, a));
            }
            k += 1;
            if k % 2 == 0 {
                ui.end_row();
            }
        };
        cell(ui, s, "Erode", "Rain wears gullies into the slopes.", &|g, a| ops::erode(g, a.abs().max(0.2), seed));
        cell(ui, s, "Raise / Lower", "Lift or sink the whole terrain.", &|g, a| ops::raise(g, a * 0.04));
        cell(ui, s, "Smooth", "Soften the relief.", &|g, a| ops::smooth_grid(g, a.abs().min(1.0)));
        cell(ui, s, "Sharpen", "Bring out fine detail.", &|g, a| ops::sharpen(g, a.abs() * 0.5));
        cell(ui, s, "Spikes", "Scatter sharp needles.", &|g, a| ops::spikes(g, a.abs() * 0.12, seed));
        cell(ui, s, "Mounds", "Scatter soft hills.", &|g, a| ops::mounds(g, a.abs() * 0.18, seed));
        cell(ui, s, "Pits", "Scatter soft hollows.", &|g, a| ops::pits(g, a.abs() * 0.18, seed));
        cell(ui, s, "Dampen", "Flatten toward the middle height.", &|g, a| ops::dampen(g, a * 0.15));
        cell(ui, s, "Round Edges", "Fade the border down in a circle.", &|g, a| ops::edges(g, (a.abs() * 0.3).clamp(0.05, 1.0), 0.0, true));
        cell(ui, s, "Square Edges", "Fade the border down in a square.", &|g, a| ops::edges(g, (a.abs() * 0.2).clamp(0.05, 1.0), 0.0, false));
        cell(ui, s, "Terraces", "Cut the slopes into steps.", &|g, a| ops::terrace(g, (6.0 + a * 4.0).clamp(2.0, 30.0) as u32, 0.3));
        cell(ui, s, "Equalize", "Use every height evenly.", &|g, _| ops::equalize(g));
        cell(ui, s, "Invert", "Turn hills into valleys.", &|g, _| ops::invert(g));
        cell(ui, s, "Zero Edges", "Set the outermost ring to the lowest height.", &|g, _| ops::zero_edges(g));
        cell(ui, s, "Stretch Range", "Use the full range from lowest to highest.", &|g, _| ops::normalise(g));
        cell(ui, s, "Flip", "Mirror left to right.", &|g, _| ops::mirror(g, true));
        cell(ui, s, "Flip Up/Down", "Mirror top to bottom.", &|g, _| ops::mirror(g, false));
        cell(ui, s, "Turn 90°", "Rotate a quarter turn.", &|g, _| ops::rotate90(g));
    });
}

fn filtering(s: &mut State, ui: &mut egui::Ui) {
    kit::heading(ui, "HEIGHT CURVE");
    ui.label(RichText::new("Drag the points: input height runs left to right, output bottom to top.").small().color(kit::DIM));
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(260.0), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, kit::INK);
    for k in 1..4 {
        let t = k as f32 / 4.0;
        p.line_segment([Pos2::new(rect.left() + t * rect.width(), rect.top()), Pos2::new(rect.left() + t * rect.width(), rect.bottom())], Stroke::new(1.0, kit::EDGE));
        p.line_segment([Pos2::new(rect.left(), rect.top() + t * rect.height()), Pos2::new(rect.right(), rect.top() + t * rect.height())], Stroke::new(1.0, kit::EDGE));
    }
    let at = |q: (f32, f32)| Pos2::new(rect.left() + q.0 * rect.width(), rect.bottom() - q.1 * rect.height());
    let pts: Vec<Pos2> = s.curve.iter().map(|q| at(*q)).collect();
    p.add(egui::Shape::line(pts.clone(), Stroke::new(2.0, kit::ACCENT)));
    for q in &pts {
        p.circle_filled(*q, 4.5, kit::TEXT);
    }
    if resp.dragged() {
        if let Some(m) = resp.interact_pointer_pos() {
            if let Some(i) = (0..s.curve.len()).min_by(|a, b| (pts[*a].x - m.x).abs().total_cmp(&(pts[*b].x - m.x).abs())) {
                s.curve[i].1 = ((rect.bottom() - m.y) / rect.height()).clamp(0.0, 1.0);
            }
        }
    }
    ui.horizontal(|ui| {
        if ui.button("Apply").clicked() {
            let c = s.curve.clone();
            s.apply(|g| ops::curve(g, &c));
        }
        if ui.button("Reset").clicked() {
            for q in &mut s.curve {
                q.1 = q.0;
            }
        }
    });
    kit::heading(ui, "READY CURVES");
    ui.horizontal_wrapped(|ui| {
        let presets: [(&str, fn(f32) -> f32); 8] = [
            ("Plateau", |x| (x * 1.6).min(1.0)),
            ("Valley floor", |x| ((x - 0.3) / 0.7).max(0.0)),
            ("Steep", |x| x * x),
            ("Gentle", |x| x.sqrt()),
            ("S-curve", |x| x * x * (3.0 - 2.0 * x)),
            ("Steps", |x| (x * 4.0).floor() / 4.0),
            ("Peak band", |x| 1.0 - (x * 2.0 - 1.0).abs()),
            ("Negative", |x| 1.0 - x),
        ];
        for (name, f) in presets {
            if ui.button(name).clicked() {
                for q in &mut s.curve {
                    q.1 = f(q.0).clamp(0.0, 1.0);
                }
            }
        }
    });
}

fn pictures(s: &mut State, ui: &mut egui::Ui) {
    kit::heading(ui, "PICTURE AS HEIGHTS");
    ui.label(RichText::new("Bright parts of a picture become high ground.").small().color(kit::DIM));
    if ui.button("Load picture…").clicked() {
        if let Some(path) = rfd::FileDialog::new().add_filter("Pictures", &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "tga", "PNG", "JPG", "BMP", "TIF"]).pick_file() {
            let n = s.grid.as_ref().map(|g| g.n).unwrap_or(128);
            if let Ok(g) = ops::from_picture(&path, n) {
                s.picture = Some(g);
            }
        }
    }
    let has = s.picture.is_some();
    ui.label(if has { "A picture is loaded." } else { "No picture loaded." });
    egui::ComboBox::from_id_salt("blend").selected_text(["Replace", "Add", "Subtract", "Highest", "Lowest", "Multiply"][s.blend_mode as usize]).show_ui(ui, |ui| {
        for (k, name) in ["Replace", "Add", "Subtract", "Highest", "Lowest", "Multiply"].iter().enumerate() {
            ui.selectable_value(&mut s.blend_mode, k as u8, *name);
        }
    });
    ui.add(egui::Slider::new(&mut s.blend_amount, 0.0..=1.0).text("Amount"));
    if ui.add_enabled(has, egui::Button::new("Apply picture")).clicked() {
        let (pic, mode, amount) = (s.picture.clone().unwrap(), s.blend_mode, s.blend_amount);
        s.apply(|g| ops::blend(g, &pic, mode, amount));
    }
    ui.add_space(8.0);
    kit::heading(ui, "SAVE");
    if ui.button("Save heights as a picture…").clicked() {
        if let (Some(path), Some(g)) = (rfd::FileDialog::new().set_file_name("terrain.png").add_filter("PNG (16-bit grey)", &["png"]).save_file(), &s.grid) {
            let _ = ops::to_picture(g, &path);
        }
    }
}
