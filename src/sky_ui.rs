//! The Sky & Fog palette: small pictures you drag across to change the sky,
//! the sun control, colour wells, sky memory dots and the options menu.

use eframe::egui::{self, Color32, Pos2, RichText, Stroke, Vec2};
use serde_json::{Value, json};

use crate::app::App;
use crate::kit;
use crate::math::{self, V3};

#[derive(Default)]
pub struct State {
    dragging: bool,
}

pub fn num(v: &Value, key: &str, default: f64) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(default)
}

pub fn colour(v: &Value, key: &str, default: [f32; 3]) -> [f32; 3] {
    match v.get(key).and_then(Value::as_array) {
        Some(a) if a.len() >= 3 => [a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32, a[2].as_f64().unwrap_or(0.0) as f32],
        _ => default,
    }
}

fn flags(sky: &Value) -> u64 {
    sky["sky"]["flags"].as_u64().unwrap_or(131)
}

fn sun_dir(sky: &Value) -> V3 {
    let d = sky["sun"]["direction"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect::<Vec<_>>()).unwrap_or_default();
    if d.len() == 3 { math::unit([d[0], d[1], d[2]]) } else { [0.6, 0.6, -0.5] }
}

/// A few ready skies, in the renderer's own terms.
pub fn presets() -> Vec<(&'static str, Value)> {
    let base = crate::create::default_sky();
    let make = |f: &dyn Fn(&mut Value)| {
        let mut v = base.clone();
        f(&mut v);
        v
    };
    vec![
        ("Clear Day", base.clone()),
        ("Hazy Noon", make(&|v| {
            v["sky"]["haze"] = json!(1.6);
            v["sun"]["direction"] = json!([0.2, 0.95, -0.2]);
        })),
        ("Golden Hour", make(&|v| {
            v["sky"]["haze"] = json!(0.9);
            v["sky"]["haze_colour"] = json!([1.0, 0.82, 0.6]);
            v["sun"]["direction"] = json!([0.85, 0.16, -0.5]);
            v["sun"]["colour"] = json!([1.0, 0.86, 0.66]);
        })),
        ("Sunset", make(&|v| {
            v["sky"]["flags"] = json!(131 | 8);
            v["sky"]["sky_colour"] = json!([0.16, 0.14, 0.42]);
            v["sky"]["horizon_colour"] = json!([1.0, 0.5, 0.22]);
            v["sky"]["haze"] = json!(1.2);
            v["sky"]["haze_colour"] = json!([1.0, 0.6, 0.36]);
            v["sun"]["direction"] = json!([0.9, 0.05, -0.42]);
            v["sun"]["colour"] = json!([1.0, 0.66, 0.42]);
        })),
        ("Foggy Morning", make(&|v| {
            v["sky"]["fog"] = json!(22.0);
            v["sky"]["fog_height"] = json!(4.0);
            v["sky"]["fog_colour"] = json!([0.9, 0.92, 0.95]);
            v["sky"]["haze"] = json!(1.0);
            v["sun"]["direction"] = json!([0.7, 0.3, -0.6]);
        })),
        ("Overcast", make(&|v| {
            v["sky"]["flags"] = json!(131 | 8);
            v["sky"]["sky_colour"] = json!([0.56, 0.6, 0.66]);
            v["sky"]["horizon_colour"] = json!([0.82, 0.84, 0.86]);
            v["sky"]["shadows"] = json!(0.6);
            v["sky"]["haze"] = json!(1.4);
            v["sun"]["intensity"] = json!(0.7);
        })),
        ("Deep Blue", make(&|v| {
            v["sky"]["flags"] = json!(131 | 8);
            v["sky"]["sky_colour"] = json!([0.02, 0.08, 0.5]);
            v["sky"]["horizon_colour"] = json!([0.5, 0.74, 1.0]);
            v["sky"]["haze"] = json!(0.2);
        })),
        ("Moonlight", make(&|v| {
            v["sky"]["flags"] = json!(131 | 8 | 0x8000);
            v["sky"]["sky_colour"] = json!([0.0, 0.01, 0.06]);
            v["sky"]["horizon_colour"] = json!([0.05, 0.08, 0.2]);
            v["sky"]["stars"] = json!([7, 1500, 0]);
            v["sky"]["haze"] = json!(0.15);
            v["sun"]["direction"] = json!([0.4, -0.6, -0.5]);
        })),
        ("No Atmosphere", make(&|v| {
            v["sky"]["flags"] = json!(130);
            v["sky"]["plain_colour"] = json!([0.0, 0.0, 0.0]);
            v["sky"]["haze"] = json!(0.0);
        })),
    ]
}

fn edit(app: &mut App, resp: &egui::Response) {
    if resp.drag_started() || resp.clicked() {
        app.doc.checkpoint();
    }
}

pub fn palette(app: &mut App, ui: &mut egui::Ui) {
    let size = Vec2::new(56.0, 40.0);
    let sky = app.doc.project.sky.clone();
    let s = &sky["sky"];
    let word = flags(&sky);
    let mut touched = false;
    let mut set = |app: &mut App, key: &str, v: Value| {
        app.doc.project.sky["sky"][key] = v;
    };

    // sky mode
    ui.vertical(|ui| {
        kit::heading(ui, "SKY MODE");
        let mode = if word & 1 == 0 { 3 } else if word & 8 != 0 { 2 } else if num(s, "shadows", 0.1) < 0.05 { 1 } else { 0 };
        let names = ["Soft Sky", "Darker Sky", "Custom Sky", "Atmosphere Off"];
        let mut pick = mode;
        egui::ComboBox::from_id_salt("skymode").width(110.0).selected_text(names[mode]).show_ui(ui, |ui| {
            for (k, n) in names.iter().enumerate() {
                ui.selectable_value(&mut pick, k, *n);
            }
        });
        if pick != mode {
            app.doc.checkpoint();
            let w = match pick {
                0 => (word | 1) & !8,
                1 => (word | 1) & !8,
                2 => word | 1 | 8,
                _ => word & !1,
            };
            set(app, "flags", json!(w));
            if pick == 1 {
                set(app, "shadows", json!(0.0));
            } else if pick == 0 && num(s, "shadows", 0.1) < 0.05 {
                set(app, "shadows", json!(0.1));
            }
            touched = true;
        }
        // colour wells that matter for the mode
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let wells: &[(&str, &str)] = if word & 1 == 0 { &[("plain_colour", "Sky colour")] } else if word & 8 != 0 { &[("sky_colour", "Sky colour overhead"), ("horizon_colour", "Horizon colour"), ("dome_colour", "Sky-dome light colour")] } else { &[("ambient_colour", "Ambient light colour")] };
            for (key, tip) in wells {
                let mut c = colour(s, key, [0.5, 0.5, 0.5]);
                if kit::colour_well(ui, &mut c, tip) {
                    app.doc.checkpoint();
                    set(app, key, json!(c));
                    touched = true;
                }
            }
        });
    });
    ui.add_space(6.0);

    // dragged thumbnails
    let shadows = num(s, "shadows", 0.1);
    let (d, r) = kit::thumb(ui, size, "Shadows", &format!("{:.0}", (1.0 - shadows) * 100.0), "Shadow strength: drag right for darker shadows", |p, rect| {
        p.rect_filled(rect, 0.0, Color32::from_gray(190));
        p.circle_filled(rect.center() + Vec2::new(-6.0, -4.0), 9.0, Color32::from_gray(235));
        let dark = (shadows * 190.0) as u8;
        p.circle_filled(rect.center() + Vec2::new(7.0, 9.0), 9.0, Color32::from_gray(dark));
    });
    edit(app, &r);
    if d.x != 0.0 {
        set(app, "shadows", json!((shadows - d.x as f64 * 0.01).clamp(0.0, 1.0)));
        touched = true;
    }

    let (fog, fog_h) = (num(s, "fog", 0.0), num(s, "fog_height", 0.0));
    let fog_c = colour(s, "fog_colour", [1.0, 1.0, 1.0]);
    let (d, r) = kit::thumb(ui, size, "Fog", &format!("{fog:.0} / {fog_h:.0}"), "Fog: drag sideways for amount, up and down for height", |p, rect| {
        p.rect_filled(rect, 0.0, Color32::from_rgb(70, 110, 160));
        let h = (fog_h / 40.0).clamp(0.05, 1.0) as f32 * rect.height();
        let band = egui::Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - h), rect.max);
        p.rect_filled(band, 0.0, kit::rgb(fog_c).gamma_multiply((fog / 60.0).clamp(0.0, 1.0) as f32));
    });
    edit(app, &r);
    if d != Vec2::ZERO {
        set(app, "fog", json!((fog + d.x as f64 * 0.4).clamp(0.0, 100.0)));
        set(app, "fog_height", json!((fog_h + d.y as f64 * 0.3).clamp(0.0, 100.0)));
        touched = true;
    }
    let mut c = fog_c;
    if kit::colour_well(ui, &mut c, "Fog colour") {
        app.doc.checkpoint();
        set(app, "fog_colour", json!(c));
        touched = true;
    }

    let haze = num(s, "haze", 0.4);
    let haze_c = colour(s, "haze_colour", [1.0, 1.0, 1.0]);
    let (d, r) = kit::thumb(ui, size, "Haze", &format!("{:.0}", haze * 20.0), "Haze: drag right for a thicker horizon", |p, rect| {
        p.rect_filled(rect, 0.0, Color32::from_rgb(60, 100, 170));
        for k in 0..10 {
            let t = k as f32 / 9.0;
            let y = rect.center().y - 6.0 + t * 12.0;
            let a = (1.0 - (t - 0.5).abs() * 2.0) * (haze as f32 / 3.0).clamp(0.05, 1.0);
            p.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.5, kit::rgb(haze_c).gamma_multiply(a)));
        }
    });
    edit(app, &r);
    if d.x != 0.0 {
        set(app, "haze", json!((haze + d.x as f64 * 0.02).clamp(0.0, 5.0)));
        touched = true;
    }
    let mut c = haze_c;
    if kit::colour_well(ui, &mut c, "Haze colour") {
        app.doc.checkpoint();
        set(app, "haze_colour", json!(c));
        touched = true;
    }

    let height = num(s, "cloud_height", 9.77);
    let (d, r) = kit::thumb(ui, size, "Cloud Height", &format!("{:.0}", height * crate::dialogs::UNITS), "Cloud height: drag up to lift the cloud layer", |p, rect| {
        p.rect_filled(rect, 0.0, Color32::from_rgb(70, 110, 190));
        let y = rect.bottom() - (height / 50.0).clamp(0.08, 0.95) as f32 * rect.height();
        p.line_segment([Pos2::new(rect.left() + 6.0, y), Pos2::new(rect.right() - 6.0, y)], Stroke::new(4.0, Color32::WHITE));
    });
    edit(app, &r);
    if d.y != 0.0 {
        set(app, "cloud_height", json!((height + d.y as f64 * 0.15).clamp(0.5, 200.0)));
        touched = true;
    }

    let has_clouds = s.get("stratus_texture").is_some() || s.get("cumulus_texture").is_some();
    let cover = num(s, "cover", 0.5);
    let (d, r) = kit::thumb(ui, size, "Cloud Cover", &format!("{:.0}", cover * 100.0), "Cloud cover: drag right for more cloud", |p, rect| {
        p.rect_filled(rect, 0.0, Color32::from_rgb(70, 110, 190));
        let n = (cover * 7.0) as usize;
        for k in 0..n {
            let x = rect.left() + 8.0 + (k as f32 * 13.0) % (rect.width() - 14.0);
            let y = rect.top() + 9.0 + (k as f32 * 7.0) % (rect.height() - 16.0);
            p.circle_filled(Pos2::new(x, y), 6.0, Color32::from_white_alpha(210));
        }
    });
    edit(app, &r);
    if d.x != 0.0 {
        set(app, "cover", json!((cover + d.x as f64 * 0.006).clamp(0.0, 1.0)));
        touched = true;
    }
    let (freq, amp) = (num(s, "frequency", 0.25), num(s, "amplitude", 1.0));
    let (d, r) = kit::thumb(ui, size, "Freq / Ampl", &format!("{freq:.2} / {amp:.2}"), "Cloud pattern: drag sideways for frequency, up and down for amplitude", |p, rect| {
        p.rect_filled(rect, 0.0, Color32::from_rgb(40, 60, 100));
        let pts: Vec<Pos2> = (0..=28).map(|k| {
            let t = k as f32 / 28.0;
            Pos2::new(rect.left() + t * rect.width(), rect.center().y - (t * 30.0 * freq as f32 * 3.0).sin() * (amp as f32 * 9.0).clamp(1.0, 17.0))
        }).collect();
        p.add(egui::Shape::line(pts, Stroke::new(1.5, Color32::WHITE)));
    });
    edit(app, &r);
    if d != Vec2::ZERO {
        set(app, "frequency", json!((freq + d.x as f64 * 0.004).clamp(0.01, 4.0)));
        set(app, "amplitude", json!((amp + d.y as f64 * 0.01).clamp(0.0, 4.0)));
        touched = true;
    }
    ui.add_space(6.0);

    // the sun: a dome seen from above; drag the sun around it
    let dir = sun_dir(&sky);
    let linked = word & 0x200 != 0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(66.0), egui::Sense::click_and_drag());
    let p = ui.painter_at(rect);
    let night = dir[1] < 0.0;
    p.circle_filled(rect.center(), 31.0, if night { Color32::from_rgb(10, 14, 40) } else { Color32::from_rgb(70, 120, 200) });
    p.circle_stroke(rect.center(), 31.0, Stroke::new(1.0, if resp.hovered() || resp.dragged() { kit::ACCENT } else { kit::EDGE }));
    p.circle_stroke(rect.center(), 15.5, Stroke::new(1.0, Color32::from_white_alpha(40)));
    let flat = (dir[0] * dir[0] + dir[2] * dir[2]).sqrt();
    let reach = (1.0 - dir[1].abs()).clamp(0.0, 1.0) as f32 * 31.0;
    let at = if flat > 1e-6 { rect.center() + Vec2::new((dir[0] / flat) as f32, -(dir[2] / flat) as f32) * reach } else { rect.center() };
    p.circle_filled(at, 5.0, if night { Color32::from_gray(220) } else { Color32::from_rgb(255, 220, 90) });
    edit(app, &resp);
    if resp.dragged() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let v = (pos - rect.center()) / 31.0;
            let r = v.length().min(1.0) as f64;
            let up = (1.0 - r) * if night && !ui.input(|i| i.modifiers.alt) { -1.0 } else { 1.0 };
            let up = if ui.input(|i| i.modifiers.alt) { -(1.0 - r) } else if night { up } else { 1.0 - r };
            let (x, z) = if r > 1e-6 { (v.x as f64 / v.length() as f64, -(v.y as f64) / v.length() as f64) } else { (0.0, 0.0) };
            let h = (1.0 - up * up).max(0.0).sqrt();
            app.doc.project.sky["sun"]["direction"] = json!([x * h, up, z * h]);
            touched = true;
        }
    }
    resp.on_hover_text("Sun position: the middle is overhead, the rim is the horizon. Hold option to put it below the horizon (night).");
    ui.vertical(|ui| {
        kit::heading(ui, if night { "MOON" } else { "SUN" });
        let mut c = colour(&sky["sun"], "colour", [1.0, 1.0, 1.0]);
        if kit::colour_well(ui, &mut c, "Sun colour") {
            app.doc.checkpoint();
            app.doc.project.sky["sun"]["colour"] = json!(c);
            app.doc.project.sky["sun"]["glow_colour"] = json!(c);
            app.doc.project.sky["sun"]["light_colour"] = json!(c);
            touched = true;
        }
        let mut link = linked;
        if ui.checkbox(&mut link, RichText::new("Link to view").small()).on_hover_text("The sun keeps its place in the picture as the camera turns").changed() {
            app.doc.checkpoint();
            relink_sun(app, link);
            touched = true;
        }
    });
    ui.add_space(6.0);

    // dots, presets, options
    ui.vertical(|ui| {
        kit::heading(ui, "SKY MEMORY");
        let filled: Vec<bool> = app.doc.project.sky_dots.iter().map(Option::is_some).collect();
        if let Some(a) = kit::memory_dots(ui, &filled, None, "Saved skies: click an empty dot to save, a full one to return, option-click to clear") {
            match a {
                kit::DotAction::Store(i) => app.doc.project.sky_dots[i] = Some(app.doc.project.sky.clone()),
                kit::DotAction::Clear(i) => app.doc.project.sky_dots[i] = None,
                kit::DotAction::Recall(i) => {
                    if let Some(v) = app.doc.project.sky_dots[i].clone() {
                        app.doc.checkpoint();
                        app.doc.project.sky = v;
                        touched = true;
                    }
                }
            }
        }
        ui.horizontal(|ui| {
            let b = ui.button("Skies").on_hover_text("Ready-made skies");
            kit::menu_below(ui, &b, false, |ui| {
                for (name, v) in presets() {
                    if ui.button(name).clicked() {
                        app.doc.checkpoint();
                        app.doc.project.sky = v;
                        app.doc.touch();
                    }
                }
                ui.separator();
                if ui.button("Sky from a scene file…").clicked() {
                    crate::library::sky_from_scene(app);
                }
            });
            let o = ui.button("Options");
            kit::menu_below(ui, &o, true, |ui| {
                let word = flags(&app.doc.project.sky);
                let mut next = word;
                for (bit, label, needs_clouds) in [(0x20u64, "Stratus clouds", true), (0x40, "Cumulus clouds", true), (0x1000, "Cloud shadows", true), (0x80, "Haze and fog", false), (0x2000, "Haze takes the sun's colour", false), (0x200000, "Fog takes the sun's colour", false), (0x8000, "Stars", false), (0x4000, "Rainbow", false), (0x10000, "Halo round the sun", false), (0x100, "Sky-dome light", false), (0x100000, "Sun grows near the horizon", false)] {
                    let mut on = word & bit != 0;
                    let ok = !needs_clouds || has_clouds;
                    if ui.add_enabled(ok, egui::Checkbox::new(&mut on, label)).on_disabled_hover_text("This sky has no cloud textures. Use a sky from a scene file that has clouds.").changed() {
                        next ^= bit;
                    }
                }
                if next != word {
                    app.doc.checkpoint();
                    if next & 0x60 != 0 { next |= 0x10 } else { next &= !0x10 }
                    app.doc.project.sky["sky"]["flags"] = json!(next);
                    if next & 0x8000 != 0 && app.doc.project.sky["sky"].get("stars").is_none() {
                        app.doc.project.sky["sky"]["stars"] = json!([7, 1200, 0]);
                    }
                    app.doc.touch();
                }
                ui.separator();
                let mut amb = num(&app.doc.project.sky["sky"], "ambience", 1.0);
                if ui.add(egui::Slider::new(&mut amb, 0.0..=2.0).text("Ambient light")).changed() {
                    app.doc.project.sky["sky"]["ambience"] = json!(amb);
                    app.doc.touch();
                }
                let mut sun = num(&app.doc.project.sky["sun"], "intensity", 1.0);
                if ui.add(egui::Slider::new(&mut sun, 0.0..=3.0).text("Sun strength")).changed() {
                    app.doc.project.sky["sun"]["intensity"] = json!(sun);
                    app.doc.touch();
                }
                if ui.button("Random sky").clicked() {
                    app.doc.checkpoint();
                    randomize(app);
                }
            });
        });
    });

    app.sky.dragging = touched;
    if touched {
        app.doc.touch();
    }
}

/// Switch the sun between world-fixed and view-linked, keeping where it is.
fn relink_sun(app: &mut App, link: bool) {
    let cam = app.doc.project.cam.clone();
    let d = sun_dir(&app.doc.project.sky);
    let word = flags(&app.doc.project.sky);
    let next = if link {
        // world -> camera frame (right, up, forward)
        [math::dot(d, cam.right), math::dot(d, cam.up), math::dot(d, cam.forward)]
    } else {
        math::add(math::add(math::mul(cam.right, d[0]), math::mul(cam.up, d[1])), math::mul(cam.forward, d[2]))
    };
    app.doc.project.sky["sun"]["direction"] = json!(next);
    app.doc.project.sky["sky"]["flags"] = json!(if link { word | 0x200 } else { word & !0x200 });
}

fn randomize(app: &mut App) {
    let mut next = || {
        app.rng ^= app.rng << 13;
        app.rng ^= app.rng >> 7;
        app.rng ^= app.rng << 17;
        (app.rng >> 11) as f64 / (1u64 << 53) as f64
    };
    let az = next() * std::f64::consts::TAU;
    let el = 0.05 + next() * 0.9;
    let h = (1.0 - el * el).sqrt();
    let warm = 1.0 - el;
    let sky = json!([0.05 + next() * 0.3, 0.1 + next() * 0.4, 0.5 + next() * 0.5]);
    let horizon = json!([0.7 + 0.3 * warm, 0.6 + 0.3 * next(), 0.5 + 0.5 * el]);
    let s = &mut app.doc.project.sky;
    s["sun"]["direction"] = json!([az.cos() * h, el, az.sin() * h]);
    s["sun"]["colour"] = json!([1.0, 0.75 + 0.25 * el, 0.55 + 0.45 * el]);
    s["sky"]["haze"] = json!(next() * 1.8);
    s["sky"]["fog"] = json!(if next() < 0.3 { next() * 30.0 } else { 0.0 });
    s["sky"]["fog_height"] = json!(1.0 + next() * 8.0);
    let word = (s["sky"]["flags"].as_u64().unwrap_or(131) | 8 | 1) & !0x200;
    s["sky"]["flags"] = json!(word);
    s["sky"]["sky_colour"] = sky;
    s["sky"]["horizon_colour"] = horizon;
    app.doc.touch();
}
