//! Animation controls: the time strip along the bottom, playback, and the
//! motion lab (every animated thing with its keys on a timeline).

use std::time::Instant;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use crate::app::{App, Lab};
use crate::kit::{self, face};
use crate::{anim, model};

#[derive(Default)]
pub struct State {
    /// Direction of pendulum playback.
    backwards: bool,
    /// A key being dragged in the motion lab: (row, key index).
    grabbed: Option<(usize, usize)>,
}

/// Move the scene to time `t`.
pub fn go_to(app: &mut App, t: f64) {
    let t = t.clamp(0.0, app.doc.project.anim.duration);
    anim::pose(&mut app.doc.project, t);
    crate::state::aim_trackers(&mut app.doc.project);
    app.doc.revision += 1;
    app.rebase();
}

/// Advance playback.
pub fn tick(app: &mut App, ctx: &egui::Context) {
    let Some((since, from)) = app.playing else { return };
    let a = &app.doc.project.anim;
    let (lo, hi) = (a.range[0], a.range[1].max(a.range[0] + 1e-3));
    let span = hi - lo;
    let elapsed = since.elapsed().as_secs_f64();
    let mut t = from + elapsed;
    match a.play_mode {
        0 => {
            if t >= hi {
                t = hi;
                app.playing = None;
            }
        }
        1 => t = lo + (t - lo).rem_euclid(span),
        _ => {
            let k = (t - lo).rem_euclid(span * 2.0);
            t = if k < span { lo + k } else { hi - (k - span) };
        }
    }
    // step in whole frames so the wireframe matches what will be rendered
    let fps = a.fps.max(1.0);
    let t = (t * fps).round() / fps;
    if (t - a.time).abs() > 1e-9 {
        go_to(app, t);
    }
    ctx.request_repaint();
}

fn time_label(t: f64, fps: f64) -> String {
    let frame = (t * fps).round() as i64;
    format!("{:02}:{:02}.{:02}  frame {}", (t as i64) / 60, (t as i64) % 60, frame.rem_euclid(fps.round().max(1.0) as i64), frame)
}

/// The animation strip (bottom of the workspace).
pub fn strip(app: &mut App, ui: &mut egui::Ui, _ctx: &egui::Context) {
    ui.spacing_mut().item_spacing.x = 3.0;
    let playing = app.playing.is_some();
    let a = app.doc.project.anim.clone();
    if kit::icon_button(ui, 30.0, "Go to the start", false, face::to_start).clicked() {
        app.playing = None;
        go_to(app, a.range[0]);
    }
    let keys = anim::key_times(&app.doc);
    if kit::icon_button(ui, 30.0, "Previous keyframe", false, face::prev_key).clicked() {
        if let Some(t) = keys.iter().rev().find(|k| **k < a.time - 1e-6) {
            go_to(app, *t);
        }
    }
    if playing {
        if kit::icon_button(ui, 30.0, "Stop", true, face::stop).clicked() {
            app.playing = None;
        }
    } else if kit::icon_button(ui, 30.0, "Play", false, face::play).clicked() {
        let from = if a.time >= a.range[1] - 1e-6 { a.range[0] } else { a.time };
        app.playing = Some((Instant::now(), from));
    }
    if kit::icon_button(ui, 30.0, "Next keyframe", false, face::next_key).clicked() {
        if let Some(t) = keys.iter().find(|k| **k > a.time + 1e-6) {
            go_to(app, *t);
        }
    }
    if kit::icon_button(ui, 30.0, "Go to the end", false, face::to_end).clicked() {
        app.playing = None;
        go_to(app, a.range[1]);
    }
    ui.add_space(6.0);
    if kit::icon_button(ui, 30.0, "Add a keyframe here for the selection (or the camera when nothing is selected)", false, face::add_key).clicked() {
        app.doc.checkpoint();
        anim::add_key(&mut app.doc);
        app.doc.touch();
        app.rebase();
    }
    if kit::icon_button(ui, 30.0, "Remove the keyframe here", false, face::remove_key).clicked() {
        app.doc.checkpoint();
        anim::remove_key(&mut app.doc);
        app.doc.touch();
        app.rebase();
    }
    let mut auto = a.auto_key;
    if ui.checkbox(&mut auto, "Auto-key").on_hover_text("Record a keyframe whenever something changes at the current time").changed() {
        app.doc.project.anim.auto_key = auto;
    }
    ui.add_space(6.0);

    // the time scrubber
    let width = (ui.available_width() - 330.0).max(160.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 30.0), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, kit::INK);
    let x_of = |t: f64| rect.left() + 6.0 + (t / a.duration.max(1e-6)) as f32 * (rect.width() - 12.0);
    // working range
    let band = Rect::from_min_max(Pos2::new(x_of(a.range[0]), rect.top() + 2.0), Pos2::new(x_of(a.range[1]), rect.bottom() - 2.0));
    p.rect_filled(band, 2.0, kit::RAISED);
    // ticks each second
    let step = if a.duration > 60.0 { 10.0 } else if a.duration > 20.0 { 5.0 } else { 1.0 };
    let mut t = 0.0;
    while t <= a.duration + 1e-6 {
        p.line_segment([Pos2::new(x_of(t), rect.bottom() - 8.0), Pos2::new(x_of(t), rect.bottom() - 2.0)], Stroke::new(1.0, kit::DIM));
        t += step;
    }
    for k in &keys {
        let c = Pos2::new(x_of(*k), rect.center().y - 3.0);
        p.add(egui::Shape::convex_polygon(vec![c + Vec2::new(0.0, -5.0), c + Vec2::new(5.0, 0.0), c + Vec2::new(0.0, 5.0), c + Vec2::new(-5.0, 0.0)], kit::ACCENT, Stroke::NONE));
    }
    let x = x_of(a.time);
    p.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(2.0, Color32::WHITE));
    if resp.dragged() || resp.clicked() {
        if let Some(m) = resp.interact_pointer_pos() {
            app.playing = None;
            let t = ((m.x - rect.left() - 6.0) / (rect.width() - 12.0)).clamp(0.0, 1.0) as f64 * a.duration;
            let fps = a.fps.max(1.0);
            go_to(app, (t * fps).round() / fps);
        }
    }
    resp.on_hover_text("Time: drag to move through the animation. Diamonds are keyframes.");
    ui.label(RichText::new(time_label(a.time, a.fps)).monospace().small());
    if ui.small_button("Lab").on_hover_text("Advanced Motion Lab").clicked() {
        app.lab = Lab::Motion;
    }
    if ui.small_button("Setup").clicked() {
        app.dlg.animation = true;
    }
}

enum Row {
    Node(u64),
    Camera,
    Sky,
    Material(usize),
}

/// The motion lab: one row per animated thing, keys you can drag in time.
pub fn motion_lab(app: &mut App, ctx: &egui::Context) {
    let mut leave = false;
    egui::TopBottomPanel::bottom("motion-bottom").frame(egui::Frame::new().fill(kit::PANEL).inner_margin(egui::Margin::symmetric(12, 7))).show(ctx, |ui| {
        ui.horizontal(|ui| {
            strip(app, ui, ctx);
        });
    });
    egui::CentralPanel::default().frame(egui::Frame::new().fill(kit::PANEL_DEEP).inner_margin(16.0)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("ADVANCED MOTION LAB").color(kit::ACCENT).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new("Done").color(kit::INK).strong()).fill(kit::ACCENT)).clicked() {
                    leave = true;
                }
                if ui.button("Show only the selection").clicked() {}
            });
        });
        ui.label(RichText::new("Each row is something that changes over time. Drag a diamond to move a keyframe; option-click one to delete it.").small().color(kit::DIM));
        ui.add_space(6.0);
        let p = &app.doc.project;
        let mut rows: Vec<(String, Row, Vec<f64>)> = Vec::new();
        if !p.anim.camera_keys.is_empty() {
            rows.push(("Camera".into(), Row::Camera, p.anim.camera_keys.iter().map(|k| k.t).collect()));
        }
        if !p.anim.sky_keys.is_empty() {
            rows.push(("Sky & Fog".into(), Row::Sky, p.anim.sky_keys.iter().map(|k| k.t).collect()));
        }
        for n in &p.nodes {
            if !n.keys.is_empty() {
                rows.push((n.name.clone(), Row::Node(n.id), n.keys.iter().map(|k| k.t).collect()));
            }
        }
        for (i, m) in p.materials.iter().enumerate() {
            if !m.keys.is_empty() {
                rows.push((format!("Material: {}", m.name), Row::Material(i), m.keys.iter().map(|k| k.t).collect()));
            }
        }
        if rows.is_empty() {
            ui.add_space(30.0);
            ui.label("Nothing is animated yet.");
            ui.label(RichText::new("Move the time marker forward, then move an object, the camera or the sky: with Auto-key on, a keyframe is recorded.").color(kit::DIM));
            return;
        }
        let duration = p.anim.duration.max(1e-6);
        let now = p.anim.time;
        let fps = p.anim.fps.max(1.0);
        let mut moved: Option<(usize, usize, f64)> = None;
        let mut removed: Option<(usize, usize)> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (r, (name, _, times)) in rows.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_sized(Vec2::new(190.0, 24.0), egui::Label::new(RichText::new(name).small()).truncate());
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width() - 8.0, 24.0), Sense::hover());
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 3.0, kit::INK);
                    let x_of = |t: f64| rect.left() + 8.0 + (t / duration) as f32 * (rect.width() - 16.0);
                    if times.len() > 1 {
                        painter.line_segment([Pos2::new(x_of(times[0]), rect.center().y), Pos2::new(x_of(times[times.len() - 1]), rect.center().y)], Stroke::new(2.0, kit::EDGE));
                    }
                    painter.line_segment([Pos2::new(x_of(now), rect.top()), Pos2::new(x_of(now), rect.bottom())], Stroke::new(1.0, Color32::from_white_alpha(120)));
                    for (k, t) in times.iter().enumerate() {
                        let c = Pos2::new(x_of(*t), rect.center().y);
                        let hit = Rect::from_center_size(c, Vec2::splat(14.0));
                        let resp = ui.interact(hit, ui.id().with(("key", r, k)), Sense::click_and_drag());
                        let hot = resp.hovered() || resp.dragged();
                        painter.add(egui::Shape::convex_polygon(vec![c + Vec2::new(0.0, -6.0), c + Vec2::new(6.0, 0.0), c + Vec2::new(0.0, 6.0), c + Vec2::new(-6.0, 0.0)], if hot { Color32::WHITE } else { kit::ACCENT }, Stroke::NONE));
                        if resp.dragged() {
                            if let Some(m) = resp.interact_pointer_pos() {
                                let t = ((m.x - rect.left() - 8.0) / (rect.width() - 16.0)).clamp(0.0, 1.0) as f64 * duration;
                                moved = Some((r, k, (t * fps).round() / fps));
                            }
                        }
                        if resp.clicked() && ui.input(|i| i.modifiers.alt) {
                            removed = Some((r, k));
                        }
                        if hot {
                            painter.text(c + Vec2::new(0.0, -13.0), Align2::CENTER_CENTER, format!("{t:.2} s"), FontId::proportional(9.0), kit::TEXT);
                        }
                    }
                });
            }
        });
        let fix = |keys: &mut Vec<f64>| keys.sort_by(|a, b| a.total_cmp(b));
        let _ = fix;
        if let Some((r, k, t)) = moved {
            if app.ani.grabbed != Some((r, k)) {
                app.doc.checkpoint();
                app.ani.grabbed = Some((r, k));
            }
            match rows[r].1 {
                Row::Camera => retime(&mut app.doc.project.anim.camera_keys, k, t),
                Row::Sky => retime(&mut app.doc.project.anim.sky_keys, k, t),
                Row::Node(id) => {
                    if let Some(n) = app.doc.project.node_mut(id) {
                        retime(&mut n.keys, k, t);
                    }
                }
                Row::Material(i) => retime(&mut app.doc.project.materials[i].keys, k, t),
            }
            app.doc.dirty = true;
        } else if !ctx.input(|i| i.pointer.any_down()) {
            app.ani.grabbed = None;
        }
        if let Some((r, k)) = removed {
            app.doc.checkpoint();
            match rows[r].1 {
                Row::Camera => drop(app.doc.project.anim.camera_keys.remove(k)),
                Row::Sky => drop(app.doc.project.anim.sky_keys.remove(k)),
                Row::Node(id) => {
                    if let Some(n) = app.doc.project.node_mut(id) {
                        n.keys.remove(k);
                    }
                }
                Row::Material(i) => drop(app.doc.project.materials[i].keys.remove(k)),
            }
            app.doc.dirty = true;
        }
    });
    if leave {
        app.lab = Lab::None;
        let t = app.doc.project.anim.time;
        go_to(app, t);
    }
}

/// Move key `k` to time `t`, keeping it between its neighbours.
fn retime<T>(keys: &mut [model::Key<T>], k: usize, t: f64) {
    let lo = if k > 0 { keys[k - 1].t + 1e-3 } else { 0.0 };
    let hi = if k + 1 < keys.len() { keys[k + 1].t - 1e-3 } else { f64::INFINITY };
    if let Some(key) = keys.get_mut(k) {
        key.t = t.clamp(lo, hi.max(lo));
    }
}
