//! The Classic look wired to the app: which original palette element does what.

use eframe::egui::{self, Color32, Pos2, Rect, Vec2};

use crate::app::{App, Palette};
use crate::classic_ui::{Act, Event, Skin};
use crate::create::{self, Make};
use crate::math;
use crate::model::Shape;
use crate::viewport::ViewKind;
use crate::wire::Side;
use crate::{dialogs, fileio, job, library, matlab};

pub const SCALE: f32 = 1.5;

fn make_of(name: &str) -> Option<Make> {
    Some(match name {
        "water plane" => Make::Water,
        "cloud plane" => Make::Cloud,
        "ground plane" => Make::Ground,
        "terrain" => Make::Terrain,
        "stone" => Make::Stone,
        "sym lattice" => Make::Lattice,
        "sphere" => Make::Sphere,
        "ellipsoid" => Make::Egg,
        "squashed sphere" => Make::Ellipsoid,
        "torus" => Make::Torus,
        "tuboid" | "squashed cylinder" => Make::Puck,
        "cylinder" => Make::Cylinder,
        "stretched cylinder" => Make::Rod,
        "cube" => Make::Cube,
        "brickoid" => Make::Brick,
        "stretched cube" => Make::Tower,
        "pyramid" => Make::Pyramid,
        "squashed pyramid" => Make::FlatPyramid,
        "stretched pyramid" => Make::Obelisk,
        "cone" => Make::Cone,
        "squashed cone" => Make::FlatCone,
        "stretched cone" => Make::Spike,
        "2d Hdisk" | "2d Vdisk" => Make::Disc,
        "2d Hface" | "2d Vface" => Make::Square,
        "radial light" => Make::Radial,
        "spotlight" => Make::Spot,
        "square spotlight" => Make::SquareSpot,
        "parallel light" => Make::Parallel,
        _ => return None,
    })
}

/// The palette titles along the top switch palettes.
fn titles(app: &mut App, ev: &Event) -> bool {
    if ev.act != Act::Click {
        return false;
    }
    let p = match ev.name.as_str() {
        "create text" | "create plopup" => Palette::Create,
        "edit text" | "edit plopup" => Palette::Edit,
        "sky&fog text" | "sky&fog plopup" => Palette::Sky,
        _ => return false,
    };
    app.palette = p;
    true
}

/// Which axis a press inside a three-axis tool means: X sideways, Y up/down, Z on the diagonal, all near the middle.
fn axis_at(at: Vec2) -> Option<usize> {
    if at.length() < 0.28 {
        return None;
    }
    let a = at.y.atan2(at.x).to_degrees().rem_euclid(180.0);
    Some(if !(25.0..155.0).contains(&a) { 0 } else if (65.0..115.0).contains(&a) { 1 } else { 2 })
}

pub fn top(app: &mut App, skin: &mut Skin, ui: &mut egui::Ui) {
    let file = match app.palette {
        Palette::Create => "createpalette",
        Palette::Edit => "editpalette",
        Palette::Sky => "skyfogpalette",
    };
    ui.horizontal(|ui| {
        ui.add_space(150.0);
        if app.palette == Palette::Sky {
            // the Sky & Fog dials are drawn by the program itself: keep the app's own, under the original titles
            let (_, events) = skin.panel(ui, "createpalette", SCALE, &|n: &str| (n == "create text").then(|| "edit text".to_owned()).filter(|_| false));
            for ev in &events {
                titles(app, ev);
            }
            return;
        }
        let (_, events) = skin.panel(ui, file, SCALE, &|_| None);
        for ev in events {
            if titles(app, &ev) {
                continue;
            }
            match app.palette {
                Palette::Create => create_event(app, &ev),
                Palette::Edit => edit_event(app, &ev),
                Palette::Sky => {}
            }
        }
    });
    if app.palette == Palette::Sky {
        ui.horizontal(|ui| {
            ui.add_space(150.0);
            crate::sky_ui::palette(app, ui);
        });
    }
}

fn create_event(app: &mut App, ev: &Event) {
    if ev.act != Act::Click {
        return;
    }
    match ev.name.as_str() {
        "2d pict" => dialogs::add_picture_object(app),
        name => {
            if let Some(m) = make_of(name) {
                let id = create::add(&mut app.doc, m, &mut app.rng);
                // the horizontal flat faces lie on the ground
                if name == "2d Hdisk" || name == "2d Hface" {
                    if let Some(n) = app.doc.project.node_mut(id) {
                        let o = math::origin(&n.m);
                        n.m = math::from_axes([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [o[0], 0.01, o[2]]);
                    }
                }
            }
        }
    }
}

fn edit_event(app: &mut App, ev: &Event) {
    if app.doc.selection.is_empty() {
        return;
    }
    let drag = |ev: &Event| match ev.act {
        Act::Drag(d) => (d.x - d.y) as f64,
        _ => 0.0,
    };
    if ev.act == Act::DragStart {
        app.doc.checkpoint();
    }
    let changed = |app: &mut App| {
        app.doc.refit_groups();
        app.doc.touch();
    };
    match ev.name.as_str() {
        "edit material" if ev.act == Act::Click => matlab::open(app),
        "resize" => {
            let d = drag(ev);
            if d != 0.0 {
                let k = (1.0 + d * 0.012).max(0.02);
                let mut f = [1.0; 3];
                match axis_at(ev.at) {
                    Some(a) => f[a] = k,
                    None => f = [k; 3],
                }
                app.doc.scale_own(f);
                changed(app);
            }
        }
        "rotate" => {
            let d = drag(ev);
            if d != 0.0 {
                app.doc.rotate_own(axis_at(ev.at).unwrap_or(1), d * 0.015);
                changed(app);
            }
        }
        "reposition" => {
            let d = drag(ev);
            if d != 0.0 {
                let mut by = [0.0; 3];
                by[axis_at(ev.at).unwrap_or(0)] = d * 0.03;
                app.doc.move_selection(by);
                changed(app);
            }
        }
        "randomize" | "randomize control" => {
            let d = drag(ev);
            if d != 0.0 {
                let (what, mut seed) = (app.randomize_what, app.rng);
                app.doc.randomize_selection((d.abs() * 0.005).min(0.5), what, &mut seed);
                app.rng = seed;
                changed(app);
            }
        }
        "edit object" if ev.act == Act::Click => app.edit_object(),
        "convert object" if ev.act == Act::Click => app.dlg.attributes = true,
        name if ev.act == Act::Click && name.starts_with("align ") => {
            let parts: Vec<&str> = name.split(' ').collect();
            let (axis, mode) = match (parts.get(1).copied(), parts.get(2).copied()) {
                (Some("x"), Some("left")) => (0, 0),
                (Some("x"), Some("center")) => (0, 1),
                (Some("x"), Some("right")) => (0, 2),
                (Some("y"), Some("bottom")) => (1, 0),
                (Some("y"), Some("center")) => (1, 1),
                (Some("y"), Some("top")) => (1, 2),
                (Some("z"), Some("front")) => (2, 0),
                (Some("z"), Some("center")) => (2, 1),
                (Some("z"), Some("back")) => (2, 2),
                _ => {
                    for a in 0..3 {
                        app.doc.align_selection(a, 1);
                    }
                    return;
                }
            };
            app.doc.align_selection(axis, mode);
        }
        name if ev.act == Act::Click && name.starts_with("convert to ") => {
            let shape = match &name[11..] {
                "sphere" => Some(Shape::Sphere),
                "torus" => Some(Shape::Torus { tube: 0.25, ring: 0.75 }),
                "cylinder" => Some(Shape::Cylinder),
                "cube" => Some(Shape::Cube),
                "pyramid" => Some(Shape::Pyramid),
                "cone" => Some(Shape::Cone),
                "disk" => Some(Shape::Disc),
                "square" => Some(Shape::Square),
                _ => None,
            };
            if let Some(shape) = shape {
                app.doc.checkpoint();
                for id in app.doc.selection.clone() {
                    if let Some(n) = app.doc.project.node_mut(id) {
                        if !matches!(n.shape, Shape::Group | Shape::Light(_) | Shape::Plane) {
                            n.shape = shape.clone();
                        }
                    }
                }
                app.doc.touch();
            }
        }
        _ => {}
    }
}

/// The left control column: preview, saved views, view chooser, camera pads, trackball, render buttons.
pub fn left(app: &mut App, skin: &mut Skin, ui: &mut egui::Ui, ctx: &egui::Context) {
    let file = "controlpalette";
    let dots: Vec<bool> = app.doc.project.view_dots.iter().map(Option::is_some).collect();
    let view_frame = match app.view.view {
        ViewKind::Camera => 0,
        ViewKind::Flat(Side::Top) => 1,
        ViewKind::Flat(Side::Bottom) => 2,
        ViewKind::Flat(Side::Left) => 3,
        ViewKind::Flat(Side::Right) => 4,
        ViewKind::Flat(Side::Front) => 5,
        ViewKind::Flat(Side::Back) => 6,
    };
    let pick = move |n: &str| -> Option<String> {
        if n == "view control" {
            return Some(format!("view control {view_frame}"));
        }
        let k: usize = n.strip_prefix("view ")?.strip_suffix(" dot")?.parse().ok()?;
        dots.get(k).copied().unwrap_or(false).then(|| "view dot full".to_owned())
    };
    let origin = ui.cursor().min;
    // as large as fits the window's height, leaving a line for the status
    let scale = ((ui.available_height() - 24.0) / 430.0).clamp(0.8, SCALE);
    let (_, events) = skin.panel(ui, file, scale, &pick);
    // the small preview sits in its window in the art
    if let (Some(r), Some(t)) = (skin.element_rect(file, "nano preview", origin, scale), &app.nano.texture) {
        ui.painter().image(t.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
    let reach = app.doc.project.cam.distance_to_pivot();
    for ev in events {
        let d = match ev.act {
            Act::Drag(d) => d,
            _ => Vec2::ZERO,
        };
        if ev.act == Act::DragStart && ev.name.contains("control") || ev.name.ends_with("button") && ev.act == Act::DragStart {
            app.doc.checkpoint();
        }
        let mut cam = |f: &mut dyn FnMut(&mut math::Cam)| {
            f(&mut app.doc.project.cam);
            app.view.view = ViewKind::Camera;
            app.doc.touch();
        };
        match (ev.name.as_str(), ev.act) {
            ("trackball control", Act::Drag(_)) => cam(&mut |c| c.orbit(-d.x as f64 * 0.015, -d.y as f64 * 0.015)),
            ("camera XY control", Act::Drag(_)) => cam(&mut |c| c.pan_view(d.x as f64 * reach * 0.006, -d.y as f64 * reach * 0.006)),
            ("camera XZ control", Act::Drag(_)) => cam(&mut |c| c.pan_ground(d.x as f64 * reach * 0.006, -d.y as f64 * reach * 0.006)),
            ("camera YZ control", Act::Drag(_)) => cam(&mut |c| {
                c.dolly(d.x as f64 * reach * 0.008);
                c.pan_view(0.0, -d.y as f64 * reach * 0.006);
            }),
            ("banking button", Act::Drag(_)) => cam(&mut |c| c.bank(d.x as f64 * 0.01)),
            ("field of view button", Act::Drag(_)) => cam(&mut |c| c.fov = (c.fov + d.x as f64 * 0.006).clamp(0.02, 3.0)),
            ("render all button", Act::Click) => {
                if app.rendering() { app.stop_render() } else { app.start_render(ctx) }
            }
            ("clear & render button", Act::Click) => {
                job::stop();
                app.main.clear();
                app.start_render(ctx);
            }
            ("resume render button", Act::Click) => app.start_render(ctx),
            ("textures on/off button", Act::Click) => {
                let w = app.doc.project.extra.get("switches").and_then(|v| v.as_u64()).unwrap_or(959) ^ 0x200;
                app.doc.checkpoint();
                app.doc.project.extra = serde_json::json!({ "switches": w });
                app.doc.touch();
            }
            ("preview mode button", Act::Click) | ("nano preview options menu", Act::Click) => app.nano_auto = !app.nano_auto,
            ("render options menu", Act::Click) => app.dlg.render_options = true,
            ("trackball options menu", Act::Click) | ("camera options menu", Act::Click) => app.dlg.camera = true,
            ("view control", Act::Click) | ("view options menu", Act::Click) => {
                app.view.view = match app.view.view {
                    ViewKind::Camera => ViewKind::Flat(Side::Top),
                    ViewKind::Flat(Side::Top) => ViewKind::Flat(Side::Front),
                    ViewKind::Flat(Side::Front) => ViewKind::Flat(Side::Right),
                    ViewKind::Flat(Side::Right) => ViewKind::Flat(Side::Left),
                    ViewKind::Flat(Side::Left) => ViewKind::Flat(Side::Back),
                    ViewKind::Flat(Side::Back) => ViewKind::Flat(Side::Bottom),
                    ViewKind::Flat(Side::Bottom) => ViewKind::Camera,
                };
                if let ViewKind::Flat(_) = app.view.view {
                    app.view.frame_all(&app.doc);
                }
            }
            (name, Act::Click) if name.starts_with("view ") && name.ends_with(" dot") => {
                if let Ok(k) = name[5..name.len() - 4].parse::<usize>() {
                    let alt = ui.input(|i| i.modifiers.alt);
                    match app.doc.project.view_dots.get(k).cloned().flatten() {
                        Some(_) if alt => app.doc.project.view_dots[k] = None,
                        Some(c) => {
                            app.doc.checkpoint();
                            app.doc.project.cam = c;
                            app.view.view = ViewKind::Camera;
                            app.doc.touch();
                        }
                        None if k < app.doc.project.view_dots.len() => app.doc.project.view_dots[k] = Some(app.doc.project.cam.clone()),
                        None => {}
                    }
                }
            }
            _ => {}
        }
    }
    // status lines under the art
    ui.add_space(4.0);
    ui.vertical_centered(|ui| {
        let first = match app.doc.selection.len() {
            0 => format!("{} objects", app.doc.project.nodes.len()),
            1 => app.doc.one().map(|n| n.name.clone()).unwrap_or_default(),
            k => format!("{k} selected"),
        };
        ui.label(egui::RichText::new(first).strong());
    });
}

/// The right display column.
pub fn right(app: &mut App, skin: &mut Skin, ui: &mut egui::Ui) {
    let (_, events) = skin.panel(ui, "displaypalette", SCALE, &|_| None);
    for ev in events {
        if ev.act != Act::Click {
            continue;
        }
        match ev.name.as_str() {
            "zoom in" => app.zoom_by(0.8),
            "zoom out" => app.zoom_by(1.25),
            "pan" => app.fit_view(),
            "depth cue control" => app.view.depth_cue = !app.view.depth_cue,
            "wireframe resolution" => app.view.detail = if app.view.detail >= 96 { 12 } else { app.view.detail * 2 },
            "image mode" | "plop render mode" => {
                if app.main.texture.is_some() {
                    app.show_picture = !app.show_picture;
                }
            }
            "background selector" => app.view.show_ground_grid = !app.view.show_ground_grid,
            "nano edit mode" => app.prefs.stay_rendered = !app.prefs.stay_rendered,
            "marker pen" => app.lib.open = Some(library::Kind::Materials),
            "ui min max" => app.prefs.classic_look = false,
            _ => {}
        }
    }
    let _ = fileio::EXT;
}
