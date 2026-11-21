//! The app's own look: colours and hand-drawn controls (icon buttons, drag
//! pads, trackball, memory dots, thumbnail sliders). Nothing here is a bitmap;
//! every control is drawn with lines and fills.

use eframe::egui::{self, Align2, Color32, CursorIcon, FontId, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};

pub const INK: Color32 = Color32::from_rgb(0xec, 0xec, 0xec);
pub const PANEL: Color32 = Color32::from_rgb(0xc4, 0xc4, 0xc4);
pub const PANEL_DEEP: Color32 = Color32::from_rgb(0xce, 0xce, 0xce);
pub const RAISED: Color32 = Color32::from_rgb(0xd8, 0xd8, 0xd8);
pub const HOVER: Color32 = Color32::from_rgb(0xe8, 0xe8, 0xe8);
pub const EDGE: Color32 = Color32::from_rgb(0x8c, 0x8c, 0x8c);
pub const TEXT: Color32 = Color32::from_rgb(0x1c, 0x1c, 0x1c);
pub const DIM: Color32 = Color32::from_rgb(0x50, 0x50, 0x50);
pub const ACCENT: Color32 = Color32::from_rgb(0x24, 0x38, 0xc4);
pub const ACCENT_DEEP: Color32 = Color32::from_rgb(0xb6, 0xc2, 0xf2);
pub const CANVAS: Color32 = Color32::from_rgb(0xb2, 0xb2, 0xb2);
/// The working window's paper.
pub const PAPER: Color32 = Color32::from_rgb(0xd6, 0xd6, 0xd6);
pub const GOOD: Color32 = Color32::from_rgb(0x7c, 0xc4, 0x8a);
pub const BAD: Color32 = Color32::from_rgb(0xe0, 0x6c, 0x5a);

/// The colours of the wireframe families (the first is the default).
pub const FAMILIES: [Color32; 12] = [
    Color32::from_rgb(0x3c, 0x3c, 0x3c),
    Color32::from_rgb(0xc0, 0x30, 0x20),
    Color32::from_rgb(0xe8, 0xa2, 0x4a),
    Color32::from_rgb(0xe6, 0xd4, 0x5c),
    Color32::from_rgb(0x9c, 0xd0, 0x5c),
    Color32::from_rgb(0x5c, 0xc8, 0x8c),
    Color32::from_rgb(0x5c, 0xc8, 0xd0),
    Color32::from_rgb(0x5c, 0x98, 0xe6),
    Color32::from_rgb(0x8c, 0x7c, 0xe6),
    Color32::from_rgb(0xc8, 0x70, 0xd8),
    Color32::from_rgb(0xe0, 0x70, 0xa4),
    Color32::from_rgb(0x9a, 0x9a, 0x9a),
];

pub fn apply_theme(ctx: &egui::Context) {
    let mut v = egui::Visuals::light();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.window_stroke = Stroke::new(1.0, EDGE);
    v.extreme_bg_color = INK;
    v.override_text_color = Some(TEXT);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, EDGE);
    v.widgets.inactive.weak_bg_fill = RAISED;
    v.widgets.inactive.bg_fill = RAISED;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, EDGE);
    v.widgets.hovered.weak_bg_fill = HOVER;
    v.widgets.hovered.bg_fill = HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(0xf4, 0xf4, 0xf4);
    v.widgets.active.bg_fill = Color32::from_rgb(0xf4, 0xf4, 0xf4);
    v.selection.bg_fill = ACCENT_DEEP;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.slider_trailing_fill = true;
    ctx.set_visuals(v);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(6.0, 6.0);
    style.spacing.button_padding = Vec2::new(9.0, 5.0);
    style.interaction.tooltip_delay = 0.35;
    ctx.set_style(style);
}

/// A square button whose face is drawn by `draw` (given the rectangle to draw
/// in and the colour to draw with).
pub fn icon_button(ui: &mut Ui, size: f32, tip: &str, on: bool, draw: impl FnOnce(&Painter, Rect, Color32)) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let p = ui.painter_at(rect);
    let fill = if on { ACCENT_DEEP } else if resp.hovered() { HOVER } else { RAISED };
    p.rect_filled(rect, 5.0, fill);
    p.rect_stroke(rect, 5.0, Stroke::new(1.0, if on || resp.hovered() { ACCENT } else { EDGE }), StrokeKind::Inside);
    let colour = if !ui.is_enabled() { DIM.gamma_multiply(0.5) } else if on || resp.hovered() { ACCENT } else { TEXT };
    draw(&p, rect.shrink(size * 0.16), colour);
    resp.on_hover_text(keys(tip))
}

/// A button with a short caption under a drawn face.
pub fn tool_button(ui: &mut Ui, size: Vec2, caption: &str, tip: &str, on: bool, draw: impl FnOnce(&Painter, Rect, Color32)) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter_at(rect);
    let enabled = ui.is_enabled();
    let fill = if on { ACCENT_DEEP } else if resp.hovered() && enabled { HOVER } else { RAISED };
    p.rect_filled(rect, 6.0, fill);
    p.rect_stroke(rect, 6.0, Stroke::new(1.0, if (on || resp.hovered()) && enabled { ACCENT } else { EDGE }), StrokeKind::Inside);
    let colour = if !enabled { DIM.gamma_multiply(0.5) } else if on || resp.hovered() { ACCENT } else { TEXT };
    let face = Rect::from_min_max(rect.min + Vec2::new(8.0, 5.0), Pos2::new(rect.max.x - 8.0, rect.max.y - 17.0));
    let side = face.width().min(face.height());
    draw(&p, Rect::from_center_size(face.center(), Vec2::splat(side)), colour);
    p.text(Pos2::new(rect.center().x, rect.max.y - 9.0), Align2::CENTER_CENTER, caption, FontId::proportional(10.0), if enabled { DIM } else { DIM.gamma_multiply(0.5) });
    resp.on_hover_text(keys(tip))
}

/// A round pad you drag on; returns the drag since last frame. `draw` paints its face.
pub fn drag_pad(ui: &mut Ui, size: f32, tip: &str, draw: impl FnOnce(&Painter, Rect, Color32)) -> (Vec2, Response) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    let active = resp.dragged();
    let colour = if active { ACCENT } else if resp.hovered() { TEXT } else { DIM };
    p.circle_filled(rect.center(), size * 0.5 - 1.0, if active { ACCENT_DEEP } else { RAISED });
    p.circle_stroke(rect.center(), size * 0.5 - 1.0, Stroke::new(1.0, if active || resp.hovered() { ACCENT } else { EDGE }));
    draw(&p, rect.shrink(size * 0.2), colour);
    let delta = if active { resp.drag_delta() } else { Vec2::ZERO };
    let resp = resp.on_hover_cursor(CursorIcon::Grab).on_hover_text(keys(tip));
    (delta, resp)
}

/// A rectangular drag strip for one axis; returns the horizontal+vertical drag combined (right/up positive).
pub fn axis_strip(ui: &mut Ui, size: Vec2, label: &str, colour: Color32, tip: &str) -> (f32, Response) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let p = ui.painter_at(rect);
    let active = resp.dragged();
    p.rect_filled(rect, 4.0, if active { ACCENT_DEEP } else if resp.hovered() { HOVER } else { RAISED });
    p.rect_stroke(rect, 4.0, Stroke::new(1.0, if active || resp.hovered() { colour } else { EDGE }), StrokeKind::Inside);
    p.text(rect.center(), Align2::CENTER_CENTER, label, FontId::proportional(11.0), colour);
    let d = if active { resp.drag_delta() } else { Vec2::ZERO };
    (d.x - d.y, resp.on_hover_cursor(CursorIcon::ResizeHorizontal).on_hover_text(keys(tip)))
}

pub enum DotAction {
    Recall(usize),
    Store(usize),
    Clear(usize),
}

/// A row of memory dots: click an empty one to store, a full one to recall,
/// alt/option-click a full one to clear.
pub fn memory_dots(ui: &mut Ui, filled: &[bool], current: Option<usize>, tip: &str) -> Option<DotAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (i, full) in filled.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(13.0), Sense::click());
            let p = ui.painter_at(rect);
            let c = rect.center();
            if *full {
                p.circle_filled(c, 5.0, if current == Some(i) { ACCENT } else { GOOD });
            } else {
                p.circle_stroke(c, 4.5, Stroke::new(1.0, if resp.hovered() { TEXT } else { DIM }));
            }
            if resp.clicked() {
                let alt = ui.input(|i| i.modifiers.alt);
                action = Some(if *full && alt { DotAction::Clear(i) } else if *full { DotAction::Recall(i) } else { DotAction::Store(i) });
            }
            resp.on_hover_text(keys(tip));
        }
    });
    action
}

/// A small picture you drag across to change a value (the sky controls work
/// this way). Returns the drag in points (x right, y up).
pub fn thumb(ui: &mut Ui, size: Vec2, caption: &str, value: &str, tip: &str, draw: impl FnOnce(&Painter, Rect)) -> (Vec2, Response) {
    let (rect, resp) = ui.allocate_exact_size(size + Vec2::new(0.0, 26.0), Sense::click_and_drag());
    let face = Rect::from_min_size(rect.min, size);
    let p = ui.painter_at(rect);
    p.rect_filled(face, 4.0, INK);
    draw(&p.with_clip_rect(face), face);
    p.rect_stroke(face, 4.0, Stroke::new(1.0, if resp.dragged() || resp.hovered() { ACCENT } else { EDGE }), StrokeKind::Inside);
    p.text(Pos2::new(face.center().x, face.max.y + 7.0), Align2::CENTER_CENTER, caption, FontId::proportional(10.0), DIM);
    p.text(Pos2::new(face.center().x, face.max.y + 19.0), Align2::CENTER_CENTER, value, FontId::proportional(10.0), TEXT);
    let d = if resp.dragged() { resp.drag_delta() } else { Vec2::ZERO };
    (Vec2::new(d.x, -d.y), resp.on_hover_cursor(CursorIcon::Grab).on_hover_text(keys(tip)))
}

/// A colour well; clicking opens the colour picker. Returns true when changed.
pub fn colour_well(ui: &mut Ui, rgb: &mut [f32; 3], tip: &str) -> bool {
    let mut c = *rgb;
    let changed = ui.color_edit_button_rgb(&mut c).on_hover_text(keys(tip)).changed();
    if changed {
        *rgb = c;
    }
    changed
}

/// A menu that drops below a button when it is clicked. `stay` keeps it
/// open while its contents are clicked (for checkboxes and sliders).
pub fn menu_below(ui: &mut Ui, resp: &Response, stay: bool, add: impl FnOnce(&mut Ui)) {
    let id = resp.id.with("menu");
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    let how = if stay { egui::PopupCloseBehavior::CloseOnClickOutside } else { egui::PopupCloseBehavior::CloseOnClick };
    egui::popup_below_widget(ui, id, resp, how, |ui| {
        ui.set_min_width(190.0);
        add(ui);
    });
}

/// Menu and tip text with the keys of the machine it runs on: ⌘ and option
/// on a Mac, Ctrl and Alt elsewhere.
pub fn keys(text: &str) -> String {
    if cfg!(target_os = "macos") {
        return text.to_owned();
    }
    text.replace("⇧⌘", "Ctrl+Shift+").replace('⌘', "Ctrl+").replace('⌥', "Alt+").replace("option-click", "alt-click").replace("Option-click", "Alt-click").replace("option", "alt")
}

pub fn heading(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(DIM).small().strong());
}

pub fn rgb(c: [f32; 3]) -> Color32 {
    let f = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    Color32::from_rgb(f(c[0]), f(c[1]), f(c[2]))
}

// --------------------------------------------------------------------------- drawn faces

/// Small line drawings used on buttons. Each draws inside `r` with colour `c`.
pub mod face {
    use super::*;

    fn s(c: Color32) -> Stroke {
        Stroke::new(1.4, c)
    }
    fn at(r: Rect, x: f32, y: f32) -> Pos2 {
        Pos2::new(r.left() + x * r.width(), r.top() + y * r.height())
    }
    fn poly(p: &Painter, r: Rect, c: Color32, pts: &[(f32, f32)], close: bool) {
        for w in pts.windows(2) {
            p.line_segment([at(r, w[0].0, w[0].1), at(r, w[1].0, w[1].1)], s(c));
        }
        if close && pts.len() > 2 {
            let (a, b) = (pts[pts.len() - 1], pts[0]);
            p.line_segment([at(r, a.0, a.1), at(r, b.0, b.1)], s(c));
        }
    }
    fn ellipse(p: &Painter, r: Rect, c: Color32, cx: f32, cy: f32, rx: f32, ry: f32) {
        let n = 28;
        let pts: Vec<(f32, f32)> = (0..n).map(|k| {
            let a = std::f32::consts::TAU * k as f32 / n as f32;
            (cx + rx * a.cos(), cy + ry * a.sin())
        }).collect();
        poly(p, r, c, &pts, true);
    }

    pub fn sphere(p: &Painter, r: Rect, c: Color32) {
        ellipse(p, r, c, 0.5, 0.5, 0.46, 0.46);
        ellipse(p, r, c, 0.5, 0.5, 0.46, 0.16);
        ellipse(p, r, c, 0.5, 0.5, 0.18, 0.46);
    }
    pub fn ellipsoid(p: &Painter, r: Rect, c: Color32) {
        ellipse(p, r, c, 0.5, 0.5, 0.48, 0.3);
        ellipse(p, r, c, 0.5, 0.5, 0.48, 0.1);
        ellipse(p, r, c, 0.5, 0.5, 0.18, 0.3);
    }
    pub fn squashed(p: &Painter, r: Rect, c: Color32) {
        ellipse(p, r, c, 0.5, 0.5, 0.3, 0.48);
        ellipse(p, r, c, 0.5, 0.5, 0.3, 0.14);
    }
    pub fn cube(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.08, 0.3), (0.62, 0.3), (0.62, 0.94), (0.08, 0.94)], true);
        poly(p, r, c, &[(0.08, 0.3), (0.38, 0.06), (0.92, 0.06), (0.62, 0.3)], false);
        poly(p, r, c, &[(0.92, 0.06), (0.92, 0.7), (0.62, 0.94)], false);
    }
    pub fn tall_box(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.26, 0.22), (0.6, 0.22), (0.6, 0.96), (0.26, 0.96)], true);
        poly(p, r, c, &[(0.26, 0.22), (0.42, 0.04), (0.76, 0.04), (0.6, 0.22)], false);
        poly(p, r, c, &[(0.76, 0.04), (0.76, 0.78), (0.6, 0.96)], false);
    }
    pub fn flat_box(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.04, 0.5), (0.66, 0.5), (0.66, 0.8), (0.04, 0.8)], true);
        poly(p, r, c, &[(0.04, 0.5), (0.34, 0.26), (0.96, 0.26), (0.66, 0.5)], false);
        poly(p, r, c, &[(0.96, 0.26), (0.96, 0.56), (0.66, 0.8)], false);
    }
    pub fn pyramid(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.5, 0.06), (0.06, 0.8), (0.62, 0.94), (0.5, 0.06)], false);
        poly(p, r, c, &[(0.62, 0.94), (0.94, 0.7), (0.5, 0.06)], false);
    }
    pub fn cone(p: &Painter, r: Rect, c: Color32) {
        ellipse(p, r, c, 0.5, 0.8, 0.42, 0.14);
        poly(p, r, c, &[(0.08, 0.8), (0.5, 0.05), (0.92, 0.8)], false);
    }
    pub fn cylinder(p: &Painter, r: Rect, c: Color32) {
        ellipse(p, r, c, 0.5, 0.2, 0.36, 0.13);
        ellipse(p, r, c, 0.5, 0.8, 0.36, 0.13);
        poly(p, r, c, &[(0.14, 0.2), (0.14, 0.8)], false);
        poly(p, r, c, &[(0.86, 0.2), (0.86, 0.8)], false);
    }
    pub fn torus(p: &Painter, r: Rect, c: Color32) {
        ellipse(p, r, c, 0.5, 0.5, 0.47, 0.3);
        ellipse(p, r, c, 0.5, 0.46, 0.2, 0.1);
    }
    pub fn disc(p: &Painter, r: Rect, c: Color32) {
        ellipse(p, r, c, 0.5, 0.5, 0.44, 0.44);
    }
    pub fn square(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.12, 0.12), (0.88, 0.12), (0.88, 0.88), (0.12, 0.88)], true);
    }
    pub fn picture(p: &Painter, r: Rect, c: Color32) {
        square(p, r, c);
        poly(p, r, c, &[(0.12, 0.74), (0.38, 0.46), (0.56, 0.64), (0.7, 0.52), (0.88, 0.74)], false);
        ellipse(p, r, c, 0.68, 0.3, 0.07, 0.07);
    }
    pub fn terrain(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.02, 0.86), (0.2, 0.5), (0.32, 0.62), (0.52, 0.16), (0.7, 0.56), (0.8, 0.44), (0.98, 0.86)], true);
        poly(p, r, c, &[(0.52, 0.16), (0.48, 0.5), (0.56, 0.86)], false);
    }
    pub fn lattice(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.04, 0.5), (0.28, 0.22), (0.5, 0.34), (0.72, 0.14), (0.96, 0.5), (0.72, 0.86), (0.5, 0.66), (0.28, 0.78)], true);
        poly(p, r, c, &[(0.04, 0.5), (0.96, 0.5)], false);
    }
    pub fn stone(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.1, 0.62), (0.2, 0.3), (0.5, 0.14), (0.82, 0.3), (0.94, 0.62), (0.7, 0.88), (0.3, 0.86)], true);
        poly(p, r, c, &[(0.2, 0.3), (0.46, 0.5), (0.82, 0.3)], false);
        poly(p, r, c, &[(0.46, 0.5), (0.5, 0.86)], false);
    }
    pub fn ground(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.02, 0.8), (0.3, 0.4), (0.98, 0.4), (0.7, 0.8)], true);
        poly(p, r, c, &[(0.16, 0.6), (0.84, 0.6)], false);
        poly(p, r, c, &[(0.36, 0.8), (0.64, 0.4)], false);
    }
    pub fn water(p: &Painter, r: Rect, c: Color32) {
        for y in [0.36f32, 0.56, 0.76] {
            let pts: Vec<(f32, f32)> = (0..=16).map(|k| {
                let x = k as f32 / 16.0;
                (0.04 + 0.92 * x, y + 0.06 * (x * 12.0).sin())
            }).collect();
            poly(p, r, c, &pts, false);
        }
    }
    pub fn cloud(p: &Painter, r: Rect, c: Color32) {
        ellipse(p, r, c, 0.34, 0.56, 0.24, 0.18);
        ellipse(p, r, c, 0.6, 0.46, 0.26, 0.22);
        poly(p, r, c, &[(0.14, 0.74), (0.86, 0.74)], false);
    }
    pub fn light_radial(p: &Painter, r: Rect, c: Color32) {
        ellipse(p, r, c, 0.5, 0.5, 0.2, 0.2);
        for k in 0..8 {
            let a = std::f32::consts::TAU * k as f32 / 8.0;
            poly(p, r, c, &[(0.5 + 0.3 * a.cos(), 0.5 + 0.3 * a.sin()), (0.5 + 0.46 * a.cos(), 0.5 + 0.46 * a.sin())], false);
        }
    }
    pub fn light_spot(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.4, 0.08), (0.6, 0.08), (0.92, 0.9), (0.08, 0.9)], true);
        ellipse(p, r, c, 0.5, 0.9, 0.42, 0.07);
    }
    pub fn light_square(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.4, 0.08), (0.6, 0.08), (0.92, 0.92), (0.08, 0.92)], true);
        poly(p, r, c, &[(0.5, 0.08), (0.5, 0.92)], false);
    }
    pub fn light_parallel(p: &Painter, r: Rect, c: Color32) {
        square(p, r, c);
        for x in [0.3f32, 0.5, 0.7] {
            poly(p, r, c, &[(x, 0.24), (x, 0.76)], false);
            poly(p, r, c, &[(x - 0.07, 0.64), (x, 0.76), (x + 0.07, 0.64)], false);
        }
    }
    pub fn library(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.5, 0.2), (0.08, 0.1), (0.08, 0.8), (0.5, 0.9), (0.92, 0.8), (0.92, 0.1)], true);
        poly(p, r, c, &[(0.5, 0.2), (0.5, 0.9)], false);
    }
    pub fn arrows_out(p: &Painter, r: Rect, c: Color32) {
        cube(p, r.shrink(r.width() * 0.2), c);
        for (a, b) in [((0.5, 0.0), (0.5, 0.14)), ((0.5, 1.0), (0.5, 0.86)), ((0.0, 0.5), (0.14, 0.5)), ((1.0, 0.5), (0.86, 0.5))] {
            poly(p, r, c, &[a, b], false);
        }
    }
    pub fn rotate(p: &Painter, r: Rect, c: Color32) {
        let n = 22;
        let pts: Vec<(f32, f32)> = (0..n).map(|k| {
            let a = -1.2 + 4.9 * k as f32 / (n - 1) as f32;
            (0.5 + 0.4 * a.cos(), 0.5 + 0.4 * a.sin())
        }).collect();
        poly(p, r, c, &pts, false);
        let (x, y) = pts[n - 1];
        poly(p, r, c, &[(x - 0.16, y - 0.02), (x, y), (x + 0.04, y - 0.18)], false);
    }
    pub fn move_cross(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.5, 0.04), (0.5, 0.96)], false);
        poly(p, r, c, &[(0.04, 0.5), (0.96, 0.5)], false);
        for (tip, a, b) in [((0.5, 0.04), (0.4, 0.16), (0.6, 0.16)), ((0.5, 0.96), (0.4, 0.84), (0.6, 0.84)), ((0.04, 0.5), (0.16, 0.4), (0.16, 0.6)), ((0.96, 0.5), (0.84, 0.4), (0.84, 0.6))] {
            poly(p, r, c, &[a, tip, b], false);
        }
    }
    pub fn align(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.1, 0.06), (0.1, 0.94)], false);
        poly(p, r, c, &[(0.1, 0.2), (0.7, 0.2), (0.7, 0.4), (0.1, 0.4)], false);
        poly(p, r, c, &[(0.1, 0.58), (0.94, 0.58), (0.94, 0.8), (0.1, 0.8)], false);
    }
    pub fn scatter(p: &Painter, r: Rect, c: Color32) {
        for (x, y, s) in [(0.2f32, 0.25f32, 0.1f32), (0.7, 0.2, 0.14), (0.45, 0.55, 0.08), (0.8, 0.7, 0.1), (0.2, 0.78, 0.13)] {
            poly(p, r, c, &[(x - s, y - s), (x + s, y - s), (x + s, y + s), (x - s, y + s)], true);
        }
    }
    pub fn material(p: &Painter, r: Rect, c: Color32) {
        ellipse(p, r, c, 0.5, 0.5, 0.44, 0.44);
        let n = 10;
        let pts: Vec<(f32, f32)> = (0..n).map(|k| {
            let a = 3.5 + 1.2 * k as f32 / (n - 1) as f32;
            (0.5 + 0.3 * a.cos(), 0.5 + 0.3 * a.sin())
        }).collect();
        poly(p, r, c, &pts, false);
    }
    pub fn pencil(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.14, 0.86), (0.2, 0.62), (0.7, 0.12), (0.88, 0.3), (0.38, 0.8)], true);
        poly(p, r, c, &[(0.2, 0.62), (0.38, 0.8)], false);
    }
    pub fn down_to_ground(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.5, 0.06), (0.5, 0.7)], false);
        poly(p, r, c, &[(0.34, 0.54), (0.5, 0.7), (0.66, 0.54)], false);
        poly(p, r, c, &[(0.08, 0.86), (0.92, 0.86)], false);
    }
    pub fn group(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.06, 0.06), (0.94, 0.06), (0.94, 0.94), (0.06, 0.94)], true);
        poly(p, r, c, &[(0.2, 0.3), (0.5, 0.3), (0.5, 0.6), (0.2, 0.6)], true);
        ellipse(p, r, c, 0.66, 0.66, 0.16, 0.16);
    }
    pub fn ungroup(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.06, 0.2), (0.44, 0.2), (0.44, 0.58), (0.06, 0.58)], true);
        ellipse(p, r, c, 0.74, 0.68, 0.2, 0.2);
    }
    pub fn magnet(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.16, 0.08), (0.16, 0.56), (0.26, 0.78), (0.5, 0.9), (0.74, 0.78), (0.84, 0.56), (0.84, 0.08), (0.62, 0.08), (0.62, 0.54), (0.5, 0.64), (0.38, 0.54), (0.38, 0.08)], true);
        poly(p, r, c, &[(0.16, 0.26), (0.38, 0.26)], false);
        poly(p, r, c, &[(0.62, 0.26), (0.84, 0.26)], false);
    }
    pub fn play(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.22, 0.1), (0.86, 0.5), (0.22, 0.9)], true);
    }
    pub fn stop(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.18, 0.18), (0.82, 0.18), (0.82, 0.82), (0.18, 0.82)], true);
    }
    pub fn to_start(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.16, 0.12), (0.16, 0.88)], false);
        poly(p, r, c, &[(0.86, 0.12), (0.26, 0.5), (0.86, 0.88)], true);
    }
    pub fn to_end(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.84, 0.12), (0.84, 0.88)], false);
        poly(p, r, c, &[(0.14, 0.12), (0.74, 0.5), (0.14, 0.88)], true);
    }
    pub fn prev_key(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.6, 0.14), (0.18, 0.5), (0.6, 0.86)], false);
        diamond(p, r, c, 0.74, 0.5, 0.16);
    }
    pub fn next_key(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.4, 0.14), (0.82, 0.5), (0.4, 0.86)], false);
        diamond(p, r, c, 0.26, 0.5, 0.16);
    }
    fn diamond(p: &Painter, r: Rect, c: Color32, x: f32, y: f32, s: f32) {
        poly(p, r, c, &[(x, y - s), (x + s, y), (x, y + s), (x - s, y)], true);
    }
    pub fn add_key(p: &Painter, r: Rect, c: Color32) {
        diamond(p, r, c, 0.4, 0.56, 0.3);
        poly(p, r, c, &[(0.8, 0.08), (0.8, 0.36)], false);
        poly(p, r, c, &[(0.66, 0.22), (0.94, 0.22)], false);
    }
    pub fn remove_key(p: &Painter, r: Rect, c: Color32) {
        diamond(p, r, c, 0.4, 0.56, 0.3);
        poly(p, r, c, &[(0.66, 0.22), (0.94, 0.22)], false);
    }
    pub fn plus(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.5, 0.14), (0.5, 0.86)], false);
        poly(p, r, c, &[(0.14, 0.5), (0.86, 0.5)], false);
    }
    pub fn minus(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.14, 0.5), (0.86, 0.5)], false);
    }
    pub fn hand(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.3, 0.9), (0.2, 0.56), (0.32, 0.5), (0.34, 0.18), (0.46, 0.18), (0.48, 0.44), (0.5, 0.12), (0.62, 0.12), (0.62, 0.44), (0.66, 0.2), (0.78, 0.22), (0.76, 0.66), (0.66, 0.9)], true);
    }
    pub fn eye(p: &Painter, r: Rect, c: Color32) {
        let n = 12;
        let top: Vec<(f32, f32)> = (0..=n).map(|k| { let x = k as f32 / n as f32; (0.04 + 0.92 * x, 0.5 - 0.3 * (x * std::f32::consts::PI).sin()) }).collect();
        let bot: Vec<(f32, f32)> = (0..=n).map(|k| { let x = k as f32 / n as f32; (0.04 + 0.92 * x, 0.5 + 0.3 * (x * std::f32::consts::PI).sin()) }).collect();
        poly(p, r, c, &top, false);
        poly(p, r, c, &bot, false);
        ellipse(p, r, c, 0.5, 0.5, 0.14, 0.14);
    }
    pub fn wire(p: &Painter, r: Rect, c: Color32) {
        for t in [0.1f32, 0.36, 0.64, 0.9] {
            poly(p, r, c, &[(t, 0.1), (t, 0.9)], false);
            poly(p, r, c, &[(0.1, t), (0.9, t)], false);
        }
    }
    pub fn shaded(p: &Painter, r: Rect, c: Color32) {
        p.circle_filled(r.center(), r.width() * 0.42, c.gamma_multiply(0.55));
        ellipse(p, r, c, 0.5, 0.5, 0.44, 0.44);
    }
    pub fn trackball(p: &Painter, r: Rect, c: Color32) {
        ellipse(p, r, c, 0.5, 0.5, 0.48, 0.48);
        ellipse(p, r, c, 0.5, 0.5, 0.48, 0.2);
        ellipse(p, r, c, 0.5, 0.5, 0.2, 0.48);
    }
    pub fn axes(a: &'static str, b: &'static str) -> impl FnOnce(&Painter, Rect, Color32) {
        move |p, r, c| {
            move_cross(p, r, c);
            p.text(at(r, 0.86, 0.28), Align2::CENTER_CENTER, a, FontId::proportional(9.0), c);
            p.text(at(r, 0.32, 0.1), Align2::CENTER_CENTER, b, FontId::proportional(9.0), c);
        }
    }
    pub fn text(label: &'static str) -> impl FnOnce(&Painter, Rect, Color32) {
        move |p, r, c| {
            p.text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional((r.height() * 0.62).clamp(9.0, 15.0)), c);
        }
    }
    pub fn sun(p: &Painter, r: Rect, c: Color32) {
        light_radial(p, r, c);
    }
    pub fn banking(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.08, 0.7), (0.92, 0.3)], false);
        ellipse(p, r, c, 0.5, 0.5, 0.1, 0.1);
    }
    pub fn lens(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, c, &[(0.5, 0.92), (0.06, 0.1)], false);
        poly(p, r, c, &[(0.5, 0.92), (0.94, 0.1)], false);
        poly(p, r, c, &[(0.2, 0.36), (0.8, 0.36)], false);
    }
}

// --------------------------------------------------------------------------- shaded controls

/// Shaded solids drawn with fills: the palette's objects and the round controls.
pub mod solid {
    use super::*;
    use eframe::egui::{Shape, epaint::EllipseShape};

    pub const BLUE: Color32 = Color32::from_rgb(0x28, 0x3a, 0xcc);
    pub const GREY: Color32 = Color32::from_rgb(0xba, 0xba, 0xba);
    pub const GOLD: Color32 = Color32::from_rgb(0xea, 0xc6, 0x2c);
    pub const ROCK: Color32 = Color32::from_rgb(0x9a, 0x7a, 0x5c);

    pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
        let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t.clamp(0.0, 1.0)) as u8;
        Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
    }
    fn dark(c: Color32, k: f32) -> Color32 {
        mix(c, Color32::BLACK, k)
    }
    fn light(c: Color32, k: f32) -> Color32 {
        mix(c, Color32::WHITE, k)
    }
    fn at(r: Rect, x: f32, y: f32) -> Pos2 {
        Pos2::new(r.left() + x * r.width(), r.top() + y * r.height())
    }
    fn poly(p: &Painter, r: Rect, pts: &[(f32, f32)], c: Color32) {
        p.add(Shape::convex_polygon(pts.iter().map(|q| at(r, q.0, q.1)).collect(), c, Stroke::NONE));
    }
    fn oval(p: &Painter, r: Rect, cx: f32, cy: f32, rx: f32, ry: f32, c: Color32) {
        p.add(EllipseShape::filled(at(r, cx, cy), Vec2::new(rx * r.width(), ry * r.height()), c));
    }
    fn shadow(p: &Painter, r: Rect) {
        oval(p, r, 0.54, 0.95, 0.42, 0.07, Color32::from_black_alpha(46));
    }

    /// A lit ball.
    pub fn ball(p: &Painter, c: Pos2, r: f32, base: Color32) {
        let n = 10;
        for k in 0..n {
            let t = k as f32 / (n - 1) as f32;
            let col = if t < 0.55 { mix(dark(base, 0.5), base, t / 0.55) } else { mix(base, Color32::WHITE, (t - 0.55) / 0.45 * 0.9) };
            p.circle_filled(c + Vec2::new(-0.33, -0.36) * r * t, r * (1.0 - 0.9 * t), col);
        }
    }

    /// A flat arrow head with its tip at `tip`, pointing along `dir`.
    pub fn arrow(p: &Painter, tip: Pos2, dir: Vec2, len: f32, half: f32, c: Color32) {
        let side = Vec2::new(-dir.y, dir.x);
        let back = tip - dir * len;
        p.add(Shape::convex_polygon(vec![tip, back + side * half, back - side * half], c, Stroke::new(1.0, dark(c, 0.35))));
        let shaft = back - dir * (len * 0.55);
        p.add(Shape::convex_polygon(vec![back + side * half * 0.42, shaft + side * half * 0.42, shaft - side * half * 0.42, back - side * half * 0.42], c, Stroke::NONE));
    }

    pub fn sphere(p: &Painter, r: Rect) {
        shadow(p, r);
        ball(p, r.center(), r.width() * 0.44, BLUE);
    }
    pub fn squashed_sphere(p: &Painter, r: Rect) {
        shadow(p, r);
        for k in 0..8 {
            let t = k as f32 / 7.0;
            oval(p, r, 0.5 - 0.14 * t, 0.6 - 0.1 * t, 0.46 * (1.0 - 0.9 * t), 0.26 * (1.0 - 0.9 * t), mix(dark(BLUE, 0.5), light(BLUE, 0.7), t));
        }
    }
    pub fn stretched_sphere(p: &Painter, r: Rect) {
        shadow(p, r);
        for k in 0..8 {
            let t = k as f32 / 7.0;
            oval(p, r, 0.5 - 0.08 * t, 0.5 - 0.16 * t, 0.26 * (1.0 - 0.9 * t), 0.46 * (1.0 - 0.9 * t), mix(dark(BLUE, 0.5), light(BLUE, 0.7), t));
        }
    }
    pub fn torus(p: &Painter, r: Rect) {
        shadow(p, r);
        let c = at(r, 0.5, 0.56);
        let rad = Vec2::new(0.33 * r.width(), 0.2 * r.height());
        p.add(EllipseShape::stroke(c, rad, Stroke::new(r.width() * 0.26, dark(BLUE, 0.25))));
        p.add(EllipseShape::stroke(c - Vec2::new(0.0, r.height() * 0.03), rad, Stroke::new(r.width() * 0.15, BLUE)));
        p.add(EllipseShape::stroke(c - Vec2::new(0.0, r.height() * 0.07), rad * 0.96, Stroke::new(r.width() * 0.045, light(BLUE, 0.55))));
    }
    fn tube(p: &Painter, r: Rect, x0: f32, x1: f32, y0: f32, y1: f32, base: Color32) {
        let (w, cx) = (x1 - x0, 0.5 * (x0 + x1));
        let ry = w * 0.2;
        oval(p, r, cx, y1, w * 0.5, ry, dark(base, 0.4));
        for k in 0..6 {
            let (a, b) = (x0 + w * k as f32 / 6.0, x0 + w * (k + 1) as f32 / 6.0);
            let t = k as f32 / 5.0;
            let c = if t < 0.3 { mix(base, light(base, 0.5), t / 0.3) } else { mix(light(base, 0.5), dark(base, 0.45), (t - 0.3) / 0.7) };
            poly(p, r, &[(a, y0), (b + 0.005, y0), (b + 0.005, y1), (a, y1)], c);
        }
        oval(p, r, cx, y0, w * 0.5, ry, light(base, 0.35));
    }
    pub fn cylinder(p: &Painter, r: Rect) {
        shadow(p, r);
        tube(p, r, 0.2, 0.8, 0.2, 0.82, BLUE);
    }
    pub fn light_parallel(p: &Painter, r: Rect) {
        shadow(p, r);
        tube(p, r, 0.24, 0.76, 0.22, 0.82, GOLD);
    }
    fn block(p: &Painter, r: Rect, base: Color32, w: f32, h: f32) {
        let (x0, x1) = (0.46 - w * 0.5, 0.46 + w * 0.5);
        let (top, bottom) = (0.86 - h, 0.86);
        let (dx, dy) = (0.16, 0.13);
        poly(p, r, &[(x0, top), (x0 + dx, top - dy), (x1 + dx, top - dy), (x1, top)], light(base, 0.45));
        poly(p, r, &[(x0, top), (x1, top), (x1, bottom), (x0, bottom)], base);
        poly(p, r, &[(x1, top), (x1 + dx, top - dy), (x1 + dx, bottom - dy), (x1, bottom)], dark(base, 0.42));
    }
    pub fn cube(p: &Painter, r: Rect) {
        shadow(p, r);
        block(p, r, BLUE, 0.56, 0.56);
    }
    pub fn flat_box(p: &Painter, r: Rect) {
        shadow(p, r);
        block(p, r, BLUE, 0.66, 0.26);
    }
    pub fn tall_box(p: &Painter, r: Rect) {
        shadow(p, r);
        block(p, r, BLUE, 0.3, 0.7);
    }
    fn spire(p: &Painter, r: Rect, base: Color32, round: bool) {
        if round {
            oval(p, r, 0.5, 0.82, 0.36, 0.1, dark(base, 0.4));
        }
        let edge = if round { 0.82 } else { 0.9 };
        poly(p, r, &[(0.5, 0.08), (0.14, 0.82), (0.5, edge)], light(base, 0.3));
        poly(p, r, &[(0.5, 0.08), (0.5, edge), (0.86, 0.82)], dark(base, 0.38));
    }
    pub fn pyramid(p: &Painter, r: Rect) {
        shadow(p, r);
        spire(p, r, BLUE, false);
    }
    pub fn cone(p: &Painter, r: Rect) {
        shadow(p, r);
        spire(p, r, BLUE, true);
    }
    pub fn light_spot(p: &Painter, r: Rect) {
        shadow(p, r);
        spire(p, r, GOLD, true);
    }
    pub fn light_square(p: &Painter, r: Rect) {
        shadow(p, r);
        spire(p, r, GOLD, false);
    }
    pub fn light_radial(p: &Painter, r: Rect) {
        shadow(p, r);
        ball(p, r.center(), r.width() * 0.4, GOLD);
    }
    pub fn disc(p: &Painter, r: Rect) {
        oval(p, r, 0.52, 0.62, 0.42, 0.24, Color32::from_black_alpha(40));
        oval(p, r, 0.5, 0.56, 0.42, 0.24, BLUE);
        oval(p, r, 0.44, 0.5, 0.2, 0.09, light(BLUE, 0.3));
    }
    fn tile(p: &Painter, r: Rect, c: Color32) {
        poly(p, r, &[(0.2, 0.3), (0.98, 0.3), (0.8, 0.86), (0.02, 0.86)], c);
    }
    pub fn square(p: &Painter, r: Rect) {
        poly(p, r, &[(0.24, 0.34), (1.0, 0.34), (0.82, 0.9), (0.06, 0.9)], Color32::from_black_alpha(40));
        tile(p, r, BLUE);
    }
    pub fn water(p: &Painter, r: Rect) {
        tile(p, r, Color32::from_rgb(0x4a, 0x7e, 0xd8));
        for (x, y, k) in [(0.42f32, 0.52f32, 0.16f32), (0.42, 0.52, 0.09), (0.66, 0.68, 0.1)] {
            p.add(EllipseShape::stroke(at(r, x, y), Vec2::new(k * r.width() * 1.5, k * r.height() * 0.8), Stroke::new(1.2, Color32::from_rgb(0xc8, 0xdc, 0xff))));
        }
    }
    pub fn cloud(p: &Painter, r: Rect) {
        tile(p, r, Color32::from_rgb(0x9c, 0xc0, 0xee));
        for (x, y, k) in [(0.36f32, 0.5f32, 0.13f32), (0.52, 0.56, 0.16), (0.66, 0.5, 0.11), (0.44, 0.7, 0.1)] {
            oval(p, r, x, y, k * 1.3, k * 0.6, Color32::from_white_alpha(225));
        }
    }
    pub fn ground(p: &Painter, r: Rect) {
        tile(p, r, Color32::from_rgb(0xa8, 0x86, 0x62));
        for (x, y) in [(0.3f32, 0.44f32), (0.5, 0.6), (0.72, 0.42), (0.4, 0.76), (0.64, 0.74), (0.22, 0.66)] {
            oval(p, r, x, y, 0.06, 0.03, Color32::from_rgb(0x7c, 0x5e, 0x42));
        }
    }
    pub fn terrain(p: &Painter, r: Rect) {
        shadow(p, r);
        poly(p, r, &[(0.5, 0.08), (0.3, 0.5), (0.06, 0.86), (0.5, 0.92)], light(GREY, 0.25));
        poly(p, r, &[(0.5, 0.08), (0.5, 0.92), (0.94, 0.86), (0.7, 0.46)], dark(GREY, 0.45));
        poly(p, r, &[(0.5, 0.08), (0.42, 0.3), (0.5, 0.36), (0.58, 0.3)], Color32::WHITE);
    }
    pub fn lattice(p: &Painter, r: Rect) {
        poly(p, r, &[(0.5, 0.06), (0.12, 0.5), (0.5, 0.56)], light(GREY, 0.25));
        poly(p, r, &[(0.5, 0.06), (0.5, 0.56), (0.88, 0.5)], dark(GREY, 0.35));
        poly(p, r, &[(0.5, 0.94), (0.12, 0.5), (0.5, 0.56)], dark(GREY, 0.2));
        poly(p, r, &[(0.5, 0.94), (0.5, 0.56), (0.88, 0.5)], dark(GREY, 0.55));
    }
    pub fn stone(p: &Painter, r: Rect) {
        shadow(p, r);
        poly(p, r, &[(0.14, 0.62), (0.26, 0.3), (0.52, 0.18), (0.5, 0.56)], light(ROCK, 0.35));
        poly(p, r, &[(0.52, 0.18), (0.82, 0.32), (0.92, 0.64), (0.5, 0.56)], ROCK);
        poly(p, r, &[(0.14, 0.62), (0.5, 0.56), (0.92, 0.64), (0.7, 0.88), (0.3, 0.86)], dark(ROCK, 0.4));
    }
    pub fn picture(p: &Painter, r: Rect) {
        poly(p, r, &[(0.14, 0.1), (0.86, 0.1), (0.86, 0.9), (0.14, 0.9)], Color32::from_rgb(0x70, 0x50, 0x30));
        poly(p, r, &[(0.2, 0.16), (0.8, 0.16), (0.8, 0.84), (0.2, 0.84)], Color32::from_rgb(0xa8, 0xcc, 0xf0));
        poly(p, r, &[(0.2, 0.84), (0.4, 0.5), (0.56, 0.7), (0.66, 0.58), (0.8, 0.84)], Color32::from_rgb(0x50, 0x80, 0x48));
        oval(p, r, 0.66, 0.32, 0.07, 0.07, GOLD);
    }
    pub fn tree(p: &Painter, r: Rect) {
        shadow(p, r);
        poly(p, r, &[(0.46, 0.92), (0.47, 0.5), (0.53, 0.5), (0.54, 0.92)], Color32::from_rgb(0x6a, 0x48, 0x2a));
        for (x, y, k) in [(0.5f32, 0.34f32, 0.3f32), (0.32, 0.46, 0.2), (0.68, 0.46, 0.2), (0.5, 0.18, 0.18)] {
            for j in 0..5 {
                let t = j as f32 / 4.0;
                oval(p, r, x - 0.05 * t, y - 0.05 * t, k * (1.0 - 0.7 * t), k * 0.85 * (1.0 - 0.7 * t), mix(Color32::from_rgb(0x2a, 0x50, 0x1c), Color32::from_rgb(0x8c, 0xc0, 0x5a), t));
            }
        }
    }
    pub fn figure(p: &Painter, r: Rect) {
        shadow(p, r);
        let c = GOLD;
        for (a, b, w) in [((0.5f32, 0.3f32), (0.5f32, 0.6f32), 0.09f32), ((0.5, 0.36), (0.16, 0.3), 0.04), ((0.5, 0.36), (0.84, 0.3), 0.04), ((0.47, 0.6), (0.36, 0.95), 0.05), ((0.53, 0.6), (0.64, 0.95), 0.05)] {
            p.line_segment([at(r, a.0, a.1), at(r, b.0, b.1)], Stroke::new(w * r.width(), c));
        }
        ball(p, at(r, 0.5, 0.16), r.width() * 0.09, c);
    }
    pub fn model(p: &Painter, r: Rect) {
        shadow(p, r);
        block(p, r, GREY, 0.5, 0.5);
        p.text(at(r, 0.46, 0.6), Align2::CENTER_CENTER, "3D", FontId::proportional(r.height() * 0.3), Color32::from_gray(40));
    }
    pub fn books(p: &Painter, r: Rect) {
        for (k, c) in [Color32::from_rgb(0xa0, 0x38, 0x30), Color32::from_rgb(0x30, 0x70, 0x40), BLUE].iter().enumerate() {
            let x = 0.2 + k as f32 * 0.21;
            poly(p, r, &[(x, 0.18 + k as f32 * 0.04), (x + 0.18, 0.18 + k as f32 * 0.04), (x + 0.18, 0.88), (x, 0.88)], *c);
        }
    }
    /// A small terrain on a plate (the view chooser).
    pub fn plate(p: &Painter, r: Rect) {
        poly(p, r, &[(0.5, 0.42), (0.98, 0.64), (0.5, 0.92), (0.02, 0.64)], light(GREY, 0.3));
        poly(p, r, &[(0.02, 0.64), (0.5, 0.92), (0.5, 0.98), (0.02, 0.7)], dark(GREY, 0.2));
        poly(p, r, &[(0.5, 0.92), (0.98, 0.64), (0.98, 0.7), (0.5, 0.98)], dark(GREY, 0.4));
        poly(p, r, &[(0.5, 0.1), (0.3, 0.62), (0.5, 0.74)], light(GREY, 0.1));
        poly(p, r, &[(0.5, 0.1), (0.5, 0.74), (0.72, 0.62)], dark(GREY, 0.45));
        ball(p, at(r, 0.2, 0.6), r.width() * 0.06, BLUE);
        ball(p, at(r, 0.78, 0.74), r.width() * 0.06, BLUE);
    }
}

/// An object on the palette's shelf: just its picture, lifted a little when the mouse is over it.
pub fn shelf_button(ui: &mut Ui, size: f32, tip: &str, draw: impl FnOnce(&Painter, Rect)) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let p = ui.painter_at(rect.expand(4.0));
    let lift = if resp.hovered() { Vec2::new(0.0, -3.0) } else { Vec2::ZERO };
    if resp.is_pointer_button_down_on() {
        p.circle_filled(rect.center(), size * 0.5, Color32::from_white_alpha(120));
    }
    draw(&p, rect.shrink(3.0).translate(lift));
    resp.on_hover_text(keys(tip))
}

/// A round grey control you drag on, with blue arrows: bits of `arrows` are
/// 1 up, 2 down, 4 left, 8 right. `core` puts a blue ball in the middle.
pub fn ball_pad(ui: &mut Ui, size: f32, tip: &str, arrows: u8, core: bool) -> (Vec2, Response) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
    let p = ui.painter_at(rect.expand(2.0));
    let c = rect.center();
    let r = size * 0.5 - 1.0;
    let active = resp.dragged();
    if core {
        // arrows fan out from a small blue ball
        let tint = if active { solid::mix(solid::GREY, Color32::WHITE, 0.6) } else if resp.hovered() { solid::mix(solid::GREY, Color32::WHITE, 0.35) } else { solid::GREY };
        for (bit, dir) in [(1u8, Vec2::new(0.0, -1.0)), (2, Vec2::new(0.0, 1.0)), (4, Vec2::new(-1.0, 0.0)), (8, Vec2::new(1.0, 0.0))] {
            if arrows & bit != 0 {
                solid::arrow(&p, c + dir * r, dir, r * 0.42, r * 0.36, tint);
            }
        }
        solid::ball(&p, c, r * 0.34, solid::BLUE);
    } else {
        p.circle_filled(c + Vec2::new(1.5, 2.5), r, Color32::from_black_alpha(50));
        solid::ball(&p, c, r, if active { solid::mix(solid::GREY, Color32::WHITE, 0.4) } else { solid::GREY });
        let blue = if resp.hovered() || active { solid::mix(solid::BLUE, Color32::WHITE, 0.25) } else { solid::BLUE };
        for (bit, dir) in [(1u8, Vec2::new(0.0, -1.0)), (2, Vec2::new(0.0, 1.0)), (4, Vec2::new(-1.0, 0.0)), (8, Vec2::new(1.0, 0.0))] {
            if arrows & bit != 0 {
                solid::arrow(&p, c + dir * r * 0.78, dir, r * 0.36, r * 0.3, blue);
            }
        }
    }
    let delta = if active { resp.drag_delta() } else { Vec2::ZERO };
    (delta, resp.on_hover_cursor(CursorIcon::Grab).on_hover_text(keys(tip)))
}

/// A plain grey ball you click (the render row).
pub fn ball_button(ui: &mut Ui, size: f32, tip: &str, lit: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let p = ui.painter_at(rect.expand(2.0));
    p.circle_filled(rect.center() + Vec2::new(1.0, 2.0), size * 0.5 - 1.0, Color32::from_black_alpha(50));
    let base = if lit { solid::mix(solid::BLUE, Color32::WHITE, 0.2) } else if resp.hovered() { solid::mix(solid::GREY, Color32::WHITE, 0.45) } else { solid::GREY };
    solid::ball(&p, rect.center(), size * 0.5 - 1.0, base);
    resp.on_hover_text(keys(tip))
}

/// A small line icon with no box round it (the right-hand column).
pub fn bare_icon(ui: &mut Ui, size: f32, tip: &str, on: bool, draw: impl FnOnce(&Painter, Rect, Color32)) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let p = ui.painter_at(rect);
    if on {
        p.rect_filled(rect, 3.0, Color32::from_white_alpha(110));
    }
    draw(&p, rect.shrink(size * 0.2), if resp.hovered() { ACCENT } else { Color32::from_gray(40) });
    resp.on_hover_text(keys(tip))
}

/// The tray the palettes' tools stand on, `count` tools of `size` wide.
pub fn shelf(ui: &mut Ui, size: f32, width: f32) {
    let origin = ui.cursor().min;
    let (top, bottom) = (origin.y + size * 0.5, origin.y + size + 8.0);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![Pos2::new(origin.x + 10.0, top), Pos2::new(origin.x + width, top), Pos2::new(origin.x + width - 10.0, bottom), Pos2::new(origin.x - 4.0, bottom)],
        Color32::from_gray(236),
        Stroke::new(1.0, Color32::from_gray(150)),
    ));
    ui.painter().line_segment([Pos2::new(origin.x - 4.0, bottom + 1.5), Pos2::new(origin.x + width - 10.0, bottom + 1.5)], Stroke::new(2.0, Color32::from_black_alpha(60)));
}

/// A shelf tool that is dragged sideways (returns the drag).
pub fn shelf_drag(ui: &mut Ui, size: f32, tip: &str, draw: impl FnOnce(&Painter, Rect)) -> (f32, Response) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
    let p = ui.painter_at(rect.expand(4.0));
    let lift = if resp.hovered() || resp.dragged() { Vec2::new(0.0, -3.0) } else { Vec2::ZERO };
    if resp.dragged() {
        p.circle_filled(rect.center(), size * 0.5, Color32::from_white_alpha(120));
    }
    draw(&p, rect.shrink(3.0).translate(lift));
    let d = if resp.dragged() { resp.drag_delta() } else { Vec2::ZERO };
    (d.x - d.y, resp.on_hover_cursor(CursorIcon::ResizeHorizontal).on_hover_text(keys(tip)))
}

pub mod edit_faces {
    use super::solid::{BLUE, GOLD, GREY, arrow, ball, mix};
    use super::*;

    fn cube(p: &Painter, r: Rect) {
        super::solid::cube(p, r.shrink(r.width() * 0.18));
    }
    /// A blue cube with arrows on the axes given (0 x, 1 y, 2 z, 3 all).
    pub fn resize(axis: u8) -> impl FnOnce(&Painter, Rect) {
        move |p, r| {
            cube(p, r);
            let c = r.center();
            let s = r.width() * 0.46;
            let dirs: &[Vec2] = match axis {
                0 => &[Vec2::new(1.0, 0.0), Vec2::new(-1.0, 0.0)],
                1 => &[Vec2::new(0.0, -1.0), Vec2::new(0.0, 1.0)],
                2 => &[Vec2::new(0.7, -0.5), Vec2::new(-0.7, 0.5)],
                _ => &[Vec2::new(0.7, -0.7), Vec2::new(-0.7, 0.7), Vec2::new(0.7, 0.7), Vec2::new(-0.7, -0.7)],
            };
            for d in dirs {
                arrow(p, c + *d * s, *d, s * 0.34, s * 0.24, GOLD);
            }
        }
    }
    /// A blue cube with a curved arrow about an axis.
    pub fn rotate(axis: u8) -> impl FnOnce(&Painter, Rect) {
        move |p, r| {
            cube(p, r);
            let c = r.center();
            let (rx, ry) = match axis {
                0 => (r.width() * 0.16, r.height() * 0.44),
                1 => (r.width() * 0.46, r.height() * 0.16),
                _ => (r.width() * 0.4, r.height() * 0.4),
            };
            let pts: Vec<Pos2> = (0..=20).map(|k| {
                let a = -0.4 + 4.6 * k as f32 / 20.0;
                c + Vec2::new(rx * a.cos(), ry * a.sin())
            }).collect();
            p.add(egui::Shape::line(pts.clone(), Stroke::new(2.5, GOLD)));
            let (a, b) = (pts[19], pts[20]);
            arrow(p, b, (b - a).normalized(), r.width() * 0.16, r.width() * 0.11, GOLD);
        }
    }
    /// A blue cube with a straight arrow along an axis.
    pub fn reposition(axis: u8) -> impl FnOnce(&Painter, Rect) {
        move |p, r| {
            cube(p, r);
            let d = match axis {
                0 => Vec2::new(1.0, 0.0),
                1 => Vec2::new(0.0, -1.0),
                _ => Vec2::new(0.7, -0.5),
            };
            let c = r.center();
            p.line_segment([c - d * r.width() * 0.3, c + d * r.width() * 0.38], Stroke::new(2.5, GOLD));
            arrow(p, c + d * r.width() * 0.5, d, r.width() * 0.18, r.width() * 0.13, GOLD);
        }
    }
    pub fn materials(p: &Painter, r: Rect) {
        let c = r.center();
        let rad = r.width() * 0.42;
        ball(p, c, rad, Color32::from_rgb(0xc8, 0x50, 0x30));
        for k in 0..4 {
            let a = k as f32 * 1.57 + 0.4;
            p.circle_filled(c + Vec2::new(a.cos(), a.sin()) * rad * 0.5, rad * 0.18, Color32::from_rgba_unmultiplied(255, 230, 120, 160));
        }
    }
    pub fn align(p: &Painter, r: Rect) {
        let (x0, x1) = (r.left() + r.width() * 0.12, r.left() + r.width() * 0.88);
        p.line_segment([Pos2::new(x0, r.top()), Pos2::new(x0, r.bottom())], Stroke::new(2.0, Color32::from_gray(60)));
        for (y, w) in [(0.28f32, 0.5f32), (0.62, 0.76)] {
            let rr = Rect::from_min_size(Pos2::new(x0 + 2.0, r.top() + r.height() * y - 6.0), Vec2::new((x1 - x0) * w, 12.0));
            p.rect_filled(rr, 2.0, BLUE);
            p.rect_filled(Rect::from_min_size(rr.min, Vec2::new(rr.width(), 4.0)), 2.0, mix(BLUE, Color32::WHITE, 0.4));
        }
    }
    pub fn scatter(p: &Painter, r: Rect) {
        for (x, y, s) in [(0.25f32, 0.3f32, 0.13f32), (0.68, 0.24, 0.16), (0.48, 0.6, 0.11), (0.8, 0.72, 0.12), (0.22, 0.78, 0.14)] {
            ball(p, Pos2::new(r.left() + x * r.width(), r.top() + y * r.height()), s * r.width(), BLUE);
        }
    }
    pub fn land(p: &Painter, r: Rect) {
        p.rect_filled(Rect::from_min_max(Pos2::new(r.left(), r.bottom() - r.height() * 0.16), r.max), 2.0, Color32::from_rgb(0xa8, 0x86, 0x62));
        ball(p, Pos2::new(r.center().x, r.top() + r.height() * 0.3), r.width() * 0.2, BLUE);
        arrow(p, Pos2::new(r.center().x, r.bottom() - r.height() * 0.2), Vec2::new(0.0, 1.0), r.width() * 0.2, r.width() * 0.16, GOLD);
    }
    pub fn pencil(p: &Painter, r: Rect) {
        let a = Pos2::new(r.left() + r.width() * 0.2, r.bottom() - r.height() * 0.15);
        let b = Pos2::new(r.right() - r.width() * 0.15, r.top() + r.height() * 0.2);
        p.line_segment([a, b], Stroke::new(r.width() * 0.2, GOLD));
        p.line_segment([b - (b - a) * 0.12, b], Stroke::new(r.width() * 0.2, Color32::from_rgb(0xd0, 0x70, 0x80)));
        p.circle_filled(a, r.width() * 0.07, Color32::from_gray(50));
    }
    pub fn group(p: &Painter, r: Rect) {
        p.rect_stroke(r.shrink(r.width() * 0.08), 3.0, Stroke::new(1.5, Color32::from_gray(70)), StrokeKind::Inside);
        ball(p, Pos2::new(r.left() + r.width() * 0.36, r.top() + r.height() * 0.42), r.width() * 0.18, BLUE);
        ball(p, Pos2::new(r.left() + r.width() * 0.64, r.top() + r.height() * 0.6), r.width() * 0.18, GREY);
    }
    pub fn ungroup(p: &Painter, r: Rect) {
        ball(p, Pos2::new(r.left() + r.width() * 0.28, r.top() + r.height() * 0.34), r.width() * 0.18, BLUE);
        ball(p, Pos2::new(r.left() + r.width() * 0.72, r.top() + r.height() * 0.68), r.width() * 0.18, GREY);
    }
    pub fn convert(p: &Painter, r: Rect) {
        let c = r.center();
        cube(p, Rect::from_center_size(c - Vec2::new(r.width() * 0.18, 0.0), r.size() * 0.75));
        ball(p, c + Vec2::new(r.width() * 0.22, r.height() * 0.12), r.width() * 0.2, GREY);
    }
}
