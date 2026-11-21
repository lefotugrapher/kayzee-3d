//! The working window: the wireframe scene, selection box and handles, and
//! the mouse work that selects, moves, resizes and turns objects and steers
//! the camera.

use eframe::egui::{self, Color32, CursorIcon, Key, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, Vec2};
#[allow(unused_imports)]
use eframe::egui::StrokeKind as _SK;

use crate::kit;
use crate::math::{self, M12, V3, add, dot, mul, sub, unit};
use crate::model::{Shape, node_box};
use crate::state::Doc;
use crate::wire::{self, Proj, Side};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewKind {
    Camera,
    Flat(Side),
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Handle {
    /// A face of the selection box: (axis, +1 or -1).
    Face(usize, f64),
    /// A corner: signs along the three axes.
    Corner([f64; 3]),
}

enum Drag {
    None,
    /// Moving the selection. `grab` is where the mouse took hold on the level
    /// plane, `lo`/`hi` the selection's box when the drag began, `applied` the
    /// move made so far and `screen` the mouse travel so far.
    Move { moved: bool, grab: Option<V3>, lo: V3, hi: V3, applied: V3, screen: Vec2 },
    Marquee { from: Pos2 },
    Resize { handle: Handle, frame: M12 },
    Turn { handle: Handle, frame: M12, centre: V3 },
    Orbit,
    Pan,
}

/// What a moved object catches on.
#[derive(Clone, Copy, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Snap {
    pub ground: bool,
    pub objects: bool,
    pub grid: bool,
    /// Grid step, world units.
    pub step: f64,
}

impl Default for Snap {
    fn default() -> Self {
        Snap { ground: true, objects: true, grid: false, step: 0.5 }
    }
}

/// The move `t` of a selection whose box was `lo..hi`, pulled onto whatever
/// lies within `tol`: the grid, other objects' sides and centres, the ground.
fn snapped(doc: &Doc, snap: Snap, lo: V3, hi: V3, mut t: V3, tol: f64) -> (V3, &'static str) {
    let mut note = "";
    let mine: Vec<u64> = doc.selection.iter().flat_map(|s| doc.project.with_descendants(*s)).collect();
    if snap.grid && snap.step > 1e-9 {
        for k in [0usize, 2] {
            let c = 0.5 * (lo[k] + hi[k]) + t[k];
            t[k] += (c / snap.step).round() * snap.step - c;
        }
        let base = lo[1] + t[1];
        t[1] += (base / snap.step).round() * snap.step - base;
        note = "grid";
    }
    if snap.objects {
        let size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        let mut best: [Option<f64>; 3] = [None; 3];
        for n in &doc.project.nodes {
            if n.hidden || n.group.is_some() || mine.contains(&n.id) || matches!(n.shape, Shape::Plane | Shape::Light(_)) {
                continue;
            }
            let Some((a, b)) = doc.project.world_bounds(&[n.id]) else { continue };
            for k in 0..3 {
                // only neighbours count: the boxes must nearly meet on the other two axes
                let near = (0..3).filter(|j| *j != k).all(|j| {
                    let reach = tol * 4.0 + 0.25 * size[j].max(b[j] - a[j]);
                    lo[j] + t[j] < b[j] + reach && hi[j] + t[j] > a[j] - reach
                });
                if !near {
                    continue;
                }
                let (l, h) = (lo[k] + t[k], hi[k] + t[k]);
                let c = 0.5 * (l + h);
                // faces meeting, faces level, centres level
                for d in [b[k] - l, a[k] - h, a[k] - l, b[k] - h, 0.5 * (a[k] + b[k]) - c] {
                    if d.abs() < tol && best[k].is_none_or(|old| d.abs() < old.abs()) {
                        best[k] = Some(d);
                    }
                }
            }
        }
        for k in 0..3 {
            if let Some(d) = best[k] {
                t[k] += d;
                note = "object";
            }
        }
    }
    if snap.ground {
        let mut levels: Vec<f64> = doc.project.nodes.iter().filter(|n| !n.hidden && !mine.contains(&n.id) && matches!(n.shape, Shape::Plane)).map(|n| math::origin(&n.m)[1]).collect();
        if levels.is_empty() {
            levels.push(0.0);
        }
        let base = lo[1] + t[1];
        if let Some(g) = levels.iter().copied().min_by(|a, b| (a - base).abs().total_cmp(&(b - base).abs())) {
            if (g - base).abs() < tol {
                t[1] += g - base;
                note = "ground";
            }
        }
    }
    (t, note)
}

fn begin_move(doc: &Doc, proj: &Proj, x: f32, y: f32) -> Drag {
    let (lo, hi) = doc.project.world_bounds(&doc.selection).unwrap_or(([0.0; 3], [0.0; 3]));
    let (from, toward) = proj.ray(x, y);
    let level = 0.5 * (lo[1] + hi[1]);
    let grab = math::ray_plane(from, toward, [0.0, level, 0.0], [0.0, 1.0, 0.0]).filter(|g| toward[1].abs() > 0.06 && math::len(sub(*g, from)) < 4000.0);
    Drag::Move { moved: false, grab, lo, hi, applied: [0.0; 3], screen: Vec2::ZERO }
}

pub struct Viewport {
    pub view: ViewKind,
    pub flat_centre: V3,
    pub flat_half: f64,
    pub depth_cue: bool,
    pub detail: usize,
    pub show_ground_grid: bool,
    pub paper: Color32,
    pub cache: wire::Cache,
    /// Set when the user changed the scene through the window this frame.
    pub changed: bool,
    /// What the object being moved has caught on ("ground", "object", "grid").
    pub snap_note: &'static str,
    drag: Drag,
}

impl Default for Viewport {
    fn default() -> Self {
        Viewport {
            view: ViewKind::Camera,
            flat_centre: [0.0, 0.0, 0.0],
            flat_half: 30.0,
            depth_cue: true,
            detail: 32,
            show_ground_grid: true,
            paper: kit::PAPER,
            cache: wire::Cache::default(),
            changed: false,
            snap_note: "",
            drag: Drag::None,
        }
    }
}

/// The placement of the unit box (-0.5..0.5) that encloses the selection, and
/// whether it follows one object's own axes.
pub fn selection_frame(doc: &Doc) -> Option<M12> {
    if doc.selection.is_empty() {
        return None;
    }
    if doc.selection.len() == 1 {
        let n = doc.project.node(doc.selection[0])?;
        if !matches!(n.shape, Shape::Group | Shape::Plane) {
            let (lo, hi) = node_box(n);
            let centre = mul(add(lo, hi), 0.5);
            let size = sub(hi, lo);
            let safe = |v: f64| if v.abs() < 1e-9 { 1e-9 } else { v };
            let own = math::from_axes([safe(size[0]), 0.0, 0.0], [0.0, safe(size[1]), 0.0], [0.0, 0.0, safe(size[2])], centre);
            return Some(math::compose(&n.m, &own));
        }
    }
    let (lo, hi) = doc.project.world_bounds(&doc.selection)?;
    let centre = mul(add(lo, hi), 0.5);
    let size = sub(hi, lo);
    Some(math::from_axes([size[0].max(1e-9), 0.0, 0.0], [0.0, size[1].max(1e-9), 0.0], [0.0, 0.0, size[2].max(1e-9)], centre))
}

impl Viewport {
    pub fn projection(&self, doc: &Doc) -> Proj {
        let size = (doc.project.size[0], doc.project.size[1]);
        match self.view {
            ViewKind::Camera => Proj::Camera { cam: doc.project.cam.clone(), size },
            ViewKind::Flat(side) => Proj::Flat { side, centre: self.flat_centre, half: self.flat_half, size },
        }
    }

    /// Fit a flat view around everything in the scene.
    pub fn frame_all(&mut self, doc: &Doc) {
        let ids: Vec<u64> = doc.project.nodes.iter().filter(|n| !matches!(n.shape, Shape::Plane)).map(|n| n.id).collect();
        if let Some((lo, hi)) = doc.project.world_bounds(&ids) {
            self.flat_centre = mul(add(lo, hi), 0.5);
            self.flat_half = (math::len(sub(hi, lo)) * 0.6).max(2.0);
        } else {
            self.flat_centre = [0.0; 3];
            self.flat_half = 30.0;
        }
    }

    /// The object under a picture point (0..1 across and down): nearest first,
    /// infinite planes last.
    pub fn pick(&self, doc: &Doc, proj: &Proj, x: f32, y: f32) -> Vec<u64> {
        let (from, toward) = proj.ray(x, y);
        let mut hits: Vec<(f64, u64)> = Vec::new();
        let mut planes: Vec<(f64, u64)> = Vec::new();
        for n in &doc.project.nodes {
            if n.hidden || matches!(n.shape, Shape::Group) {
                continue;
            }
            if let Shape::Plane = n.shape {
                let normal = unit(math::axis(&n.m, 1));
                if let Some(p) = math::ray_plane(from, toward, math::origin(&n.m), normal) {
                    planes.push((math::len(sub(p, from)), n.id));
                }
                continue;
            }
            let (lo, hi) = node_box(n);
            // flat shapes get a little thickness so they can be clicked
            let pad = |a: f64, b: f64| if (b - a).abs() < 1e-6 { (a - 0.03, b + 0.03) } else { (a, b) };
            let (x0, x1) = pad(lo[0], hi[0]);
            let (y0, y1) = pad(lo[1], hi[1]);
            let (z0, z1) = pad(lo[2], hi[2]);
            if let Some(t) = math::ray_box(&n.m, [x0, y0, z0], [x1, y1, z1], from, toward) {
                hits.push((t, n.id));
            }
        }
        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
        planes.sort_by(|a, b| a.0.total_cmp(&b.0));
        hits.into_iter().chain(planes).map(|h| h.1).collect()
    }

    fn handles(frame: &M12) -> Vec<(Handle, V3)> {
        let mut out = Vec::new();
        for k in 0..3 {
            for s in [-1.0, 1.0] {
                let mut p = [0.0; 3];
                p[k] = 0.5 * s;
                out.push((Handle::Face(k, s), math::place(frame, p)));
            }
        }
        for c in 0..8 {
            let s = [if c & 1 == 0 { -1.0 } else { 1.0 }, if c & 2 == 0 { -1.0 } else { 1.0 }, if c & 4 == 0 { -1.0 } else { 1.0 }];
            out.push((Handle::Corner(s), math::place(frame, [0.5 * s[0], 0.5 * s[1], 0.5 * s[2]])));
        }
        out
    }

    /// Draw the scene into `rect` and handle the mouse. `picture` (when given)
    /// is drawn under the wireframe.
    /// `wire`: 0 none, 1 everything, 2 only what is selected. `under` is an
    /// older picture shown beneath `picture` while a new one is being drawn.
    pub fn ui(&mut self, ui: &mut egui::Ui, rect: Rect, doc: &mut Doc, under: Option<&TextureHandle>, picture: Option<&TextureHandle>, wire: u8, snap: Snap) {
        self.changed = false;
        let resp = ui.interact(rect, ui.id().with("working-window"), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, self.paper);
        for t in [under, picture].into_iter().flatten() {
            painter.image(t.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        let proj = self.projection(doc);
        let to_screen = |p: [f32; 2]| Pos2::new(rect.left() + p[0] * rect.width(), rect.top() + p[1] * rect.height());
        let to_picture = |p: Pos2| ((p.x - rect.left()) / rect.width(), (p.y - rect.top()) / rect.height());

        // ---------------------------------------------------------------- wireframe
        if wire != 0 {
            doc_wire(self, doc, &proj, &painter, &to_screen, wire == 2);
        }

        // ---------------------------------------------------------------- selection box and handles
        let frame = selection_frame(doc);
        let mut handle_at: Vec<(Handle, Pos2)> = Vec::new();
        if let (Some(f), true) = (&frame, wire != 0) {
            let corners = wire::box_corners(f, [-0.5; 3], [0.5; 3]);
            for i in 0..8usize {
                for bit in [1usize, 2, 4] {
                    if i & bit == 0 {
                        if let Some((a, b, _)) = proj.segment(corners[i], corners[i | bit]) {
                            painter.line_segment([to_screen(a), to_screen(b)], Stroke::new(1.0, Color32::from_gray(110)));
                        }
                    }
                }
            }
            for (h, world) in Self::handles(f) {
                if let Some((p, _)) = proj.point(world) {
                    let at = to_screen(p);
                    if rect.expand(8.0).contains(at) {
                        let big = matches!(h, Handle::Face(..));
                        let r = Rect::from_center_size(at, Vec2::splat(if big { 7.0 } else { 5.0 }));
                        painter.rect_filled(r, 0.0, Color32::BLACK);
                        handle_at.push((h, at));
                    }
                }
            }
        }

        // ---------------------------------------------------------------- mouse
        let pointer = resp.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.hover_pos()));
        let mods = ui.input(|i| i.modifiers);
        let over_handle = pointer.and_then(|p| handle_at.iter().find(|(_, at)| (p - *at).length() < 7.0).map(|(h, _)| *h));
        if resp.hovered() {
            if over_handle.is_some() {
                ui.ctx().set_cursor_icon(if mods.command { CursorIcon::Alias } else { CursorIcon::ResizeNwSe });
            }
            // wheel: move the camera in or out, or zoom a flat view
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.0 {
                match self.view {
                    ViewKind::Camera => {
                        let step = doc.project.cam.distance_to_pivot() * 0.0025 * scroll as f64;
                        doc.project.cam.dolly(step);
                        doc.touch();
                        self.changed = true;
                    }
                    ViewKind::Flat(_) => self.flat_half = (self.flat_half * (1.0 - 0.002 * scroll as f64)).clamp(0.05, 1.0e5),
                }
            }
        }

        let secondary = ui.input(|i| i.pointer.secondary_down());
        let middle = ui.input(|i| i.pointer.middle_down()) || ui.input(|i| i.key_down(Key::Space));

        if resp.drag_started() {
            if let Some(p) = pointer {
                let (px, py) = to_picture(p);
                if secondary {
                    doc.checkpoint();
                    self.drag = Drag::Orbit;
                } else if middle {
                    doc.checkpoint();
                    self.drag = Drag::Pan;
                } else if let (Some(h), Some(f)) = (over_handle, frame) {
                    doc.checkpoint();
                    let centre = math::origin(&f);
                    self.drag = if mods.command { Drag::Turn { handle: h, frame: f, centre } } else { Drag::Resize { handle: h, frame: f } };
                } else {
                    let under = self.pick(doc, &proj, px, py);
                    let hit_selected = under.iter().map(|id| doc.project.top(*id)).find(|t| doc.is_selected(*t));
                    match (hit_selected, under.first()) {
                        (Some(_), _) => {
                            doc.checkpoint();
                            self.drag = begin_move(doc, &proj, px, py);
                        }
                        (None, Some(first)) if !mods.shift => {
                            // pressing on an unselected object selects it and starts moving it
                            let top = doc.project.top(*first);
                            let plane = doc.project.node(top).is_some_and(|n| matches!(n.shape, Shape::Plane));
                            if plane {
                                self.drag = Drag::Marquee { from: p };
                            } else {
                                doc.select_only(top);
                                doc.checkpoint();
                                self.drag = begin_move(doc, &proj, px, py);
                            }
                        }
                        _ => self.drag = Drag::Marquee { from: p },
                    }
                }
            }
        }

        let mut note_now = "";
        let view_kind = self.view;
        if resp.dragged() {
            let delta = resp.drag_delta();
            let frame_w = rect.width().max(1.0) as f64;
            match &mut self.drag {
                Drag::Orbit => match self.view {
                    ViewKind::Camera => {
                        doc.project.cam.orbit(-delta.x as f64 * 0.008, -delta.y as f64 * 0.008);
                        doc.touch();
                        self.changed = true;
                    }
                    ViewKind::Flat(side) => {
                        let (r, u, _) = side.frame();
                        let k = 2.0 * self.flat_half / frame_w;
                        self.flat_centre = sub(self.flat_centre, add(mul(r, delta.x as f64 * k), mul(u, -delta.y as f64 * k)));
                    }
                },
                Drag::Pan => match self.view {
                    ViewKind::Camera => {
                        let k = proj.world_per_width(doc.project.cam.pivot) / frame_w;
                        doc.project.cam.pan_view(-delta.x as f64 * k, delta.y as f64 * k);
                        doc.touch();
                        self.changed = true;
                    }
                    ViewKind::Flat(side) => {
                        let (r, u, _) = side.frame();
                        let k = 2.0 * self.flat_half / frame_w;
                        self.flat_centre = sub(self.flat_centre, add(mul(r, delta.x as f64 * k), mul(u, -delta.y as f64 * k)));
                    }
                },
                Drag::Move { moved, grab, lo, hi, applied, screen } => {
                    if delta != Vec2::ZERO {
                        *moved = true;
                        *screen += delta;
                        let centre = mul(add(*lo, *hi), 0.5);
                        let k = proj.world_per_width(centre) / frame_w;
                        let (r, u, _) = proj.frame();
                        let held = ui.input(|i| [i.key_down(Key::X), i.key_down(Key::Y), i.key_down(Key::Z)]);
                        let camera = matches!(view_kind, ViewKind::Camera);
                        let free = add(mul(r, screen.x as f64 * k), mul(u, -screen.y as f64 * k));
                        let mut target = if let Some(axis) = held.iter().position(|h| *h) {
                            // hold X, Y or Z to keep to that world axis
                            let mut a = [0.0; 3];
                            a[axis] = 1.0;
                            let (sr, su) = (dot(a, r), dot(a, u));
                            let on_screen = (sr * sr + su * su).sqrt();
                            if on_screen > 0.05 { mul(a, (screen.x as f64 * sr - screen.y as f64 * su) / on_screen * k / on_screen) } else { mul(a, -(screen.y as f64) * k) }
                        } else if camera && mods.alt {
                            // option/alt: straight up and down
                            [0.0, -(screen.y as f64) * k, 0.0]
                        } else if let (true, Some(g), Some(p)) = (camera, *grab, pointer) {
                            // slide over the level the object stands on: it stays under the mouse and keeps its height
                            let (px, py) = ((p.x - rect.left()) / rect.width(), (p.y - rect.top()) / rect.height());
                            let (from, toward) = proj.ray(px, py);
                            match math::ray_plane(from, toward, g, [0.0, 1.0, 0.0]).filter(|h| toward[1].abs() > 0.03 && math::len(sub(*h, from)) < 8000.0) {
                                Some(hit) => [hit[0] - g[0], 0.0, hit[2] - g[2]],
                                None => free,
                            }
                        } else {
                            free
                        };
                        // command/ctrl held: no snapping
                        if !mods.command {
                            let (t, note) = snapped(doc, snap, *lo, *hi, target, 9.0 * k);
                            target = t;
                            note_now = note;
                        }
                        let step = sub(target, *applied);
                        if step != [0.0; 3] {
                            *applied = target;
                            doc.move_selection(step);
                            doc.refit_groups();
                            doc.touch();
                            self.changed = true;
                        }
                    } else {
                        note_now = self.snap_note;
                    }
                }
                Drag::Resize { handle, frame } => {
                    let f = *frame;
                    let centre = math::origin(&f);
                    let k = proj.world_per_width(centre) / frame_w;
                    let (r, u, _) = proj.frame();
                    let screen_move = add(mul(r, delta.x as f64 * k), mul(u, -delta.y as f64 * k));
                    let mut factors = [1.0; 3];
                    match *handle {
                        Handle::Face(axis, sign) => {
                            let a = math::axis(&f, axis);
                            let extent = math::len(a).max(1e-9);
                            let grow = dot(screen_move, unit(a)) * sign;
                            factors[axis] = ((extent + grow * if mods.alt { 2.0 } else { 1.0 }) / extent).max(0.01);
                        }
                        Handle::Corner(_) => {
                            let grow = (delta.x - delta.y) as f64 * 0.006;
                            factors = [(1.0 + grow).max(0.01); 3];
                        }
                    }
                    // scale along the box's own axes, holding the far side still (alt: the centre)
                    let anchor_own = match *handle {
                        Handle::Face(axis, sign) if !mods.alt => {
                            let mut p = [0.0; 3];
                            p[axis] = -0.5 * sign;
                            p
                        }
                        _ => [0.0; 3],
                    };
                    let anchor = math::place(&f, anchor_own);
                    let basis = math::from_axes(unit(math::axis(&f, 0)), unit(math::axis(&f, 1)), unit(math::axis(&f, 2)), anchor);
                    if let Some(inv) = math::inverse(&basis) {
                        let op = math::compose(&basis, &math::compose(&math::scaling(factors), &inv));
                        doc.transform_selection(&op);
                        *frame = math::compose(&op, &f);
                        doc.refit_groups();
                        doc.touch();
                        self.changed = true;
                    }
                }
                Drag::Turn { handle, frame, centre } => {
                    let axis = match *handle {
                        Handle::Face(axis, _) => unit(math::axis(frame, axis)),
                        Handle::Corner(_) => proj.frame().2,
                    };
                    let angle = (delta.x - delta.y) as f64 * 0.01;
                    let op = math::about(&math::turn(axis, angle), *centre, &math::IDENTITY);
                    doc.transform_selection(&op);
                    *frame = math::compose(&op, frame);
                    doc.refit_groups();
                    doc.touch();
                    self.changed = true;
                }
                Drag::Marquee { .. } | Drag::None => {}
            }
        }

        self.snap_note = note_now;
        if let (false, Some(p)) = (note_now.is_empty(), pointer) {
            let at = p + Vec2::new(14.0, -14.0);
            let text = format!("snapped to {note_now}");
            let galley = painter.layout_no_wrap(text, egui::FontId::proportional(11.0), Color32::WHITE);
            let back = Rect::from_min_size(at, galley.size()).expand2(Vec2::new(5.0, 2.0));
            painter.rect_filled(back, 3.0, Color32::from_black_alpha(170));
            painter.galley(at, galley, Color32::WHITE);
        }
        if let (Drag::Marquee { from }, Some(p)) = (&self.drag, pointer) {
            let r = Rect::from_two_pos(*from, p);
            painter.rect_filled(r, 0.0, kit::ACCENT.gamma_multiply(0.08));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, kit::ACCENT), StrokeKind::Inside);
        }

        if resp.drag_stopped() {
            match std::mem::replace(&mut self.drag, Drag::None) {
                Drag::Marquee { from } => {
                    if let Some(p) = pointer {
                        let r = Rect::from_two_pos(from, p);
                        if r.width() > 3.0 || r.height() > 3.0 {
                            let mut ids = if mods.shift { doc.selection.clone() } else { Vec::new() };
                            for n in &doc.project.nodes {
                                if n.hidden || n.locked || matches!(n.shape, Shape::Group | Shape::Plane) {
                                    continue;
                                }
                                let centre = {
                                    let (lo, hi) = node_box(n);
                                    math::place(&n.m, mul(add(lo, hi), 0.5))
                                };
                                if let Some((q, _)) = proj.point(centre) {
                                    if r.contains(to_screen(q)) {
                                        let top = doc.project.top(n.id);
                                        if !ids.contains(&top) {
                                            ids.push(top);
                                        }
                                    }
                                }
                            }
                            doc.selection = ids;
                        } else if !mods.shift {
                            doc.clear_selection();
                        }
                    }
                }
                Drag::Move { moved: false, .. } => {
                    // a press without travel changed nothing: drop the saved state
                    doc.undo_discard();
                }
                _ => {}
            }
        }

        if resp.clicked() {
            if let Some(p) = pointer {
                let (px, py) = to_picture(p);
                let under = self.pick(doc, &proj, px, py);
                match under.first() {
                    Some(first) => {
                        // command-click reaches inside groups; repeated clicks walk down the stack
                        let pick = |id: u64, doc: &Doc| if mods.command { id } else { doc.project.top(id) };
                        let stack: Vec<u64> = under.iter().map(|id| pick(*id, doc)).collect();
                        let mut target = pick(*first, doc);
                        if !mods.shift && doc.selection.len() == 1 {
                            if let Some(i) = stack.iter().position(|s| *s == doc.selection[0]) {
                                if let Some(next) = stack.iter().skip(i + 1).find(|s| **s != doc.selection[0]) {
                                    target = *next;
                                }
                            }
                        }
                        if mods.shift { doc.toggle(target) } else { doc.select_only(target) }
                    }
                    None if !mods.shift => doc.clear_selection(),
                    None => {}
                }
            }
        }

        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, kit::EDGE), StrokeKind::Outside);
    }
}

fn doc_wire(view: &mut Viewport, doc: &Doc, proj: &Proj, painter: &egui::Painter, to_screen: &dyn Fn([f32; 2]) -> Pos2, only_selected: bool) {
    let alive: Vec<u64> = doc.project.nodes.iter().map(|n| n.id).collect();
    view.cache.forget_missing(|id| alive.binary_search(&id).is_ok() || alive.contains(&id));
    let reach = match proj {
        Proj::Camera { cam, .. } => cam.distance_to_pivot().max(8.0) * 2.5,
        Proj::Flat { .. } => f64::INFINITY,
    };
    for n in &doc.project.nodes {
        if n.hidden {
            continue;
        }
        let selected = doc.is_selected(doc.project.top(n.id)) || doc.is_selected(n.id);
        if only_selected && !selected {
            continue;
        }
        let base = if selected {
            Color32::from_rgb(0xd8, 0x1c, 0x1c)
        } else if matches!(n.shape, Shape::Plane) {
            Color32::from_rgb(0x90, 0x90, 0x90)
        } else if matches!(n.shape, Shape::Light(_)) {
            Color32::from_rgb(0x9a, 0x7a, 0x00)
        } else {
            kit::FAMILIES[(n.family as usize) % kit::FAMILIES.len()]
        };
        if matches!(n.shape, Shape::Plane) && !view.show_ground_grid && !selected {
            continue;
        }
        let draw = |a: V3, b: V3| {
            if let Some((pa, pb, depth)) = proj.segment(a, b) {
                let fade = if view.depth_cue && reach.is_finite() { (1.0 - (depth / reach) as f32 * 0.75).clamp(0.22, 1.0) } else { 1.0 };
                painter.line_segment([to_screen(pa), to_screen(pb)], Stroke::new(1.0, kit::solid::mix(view.paper, base, fade)));
            }
        };
        if n.as_box || matches!(n.shape, Shape::Group) {
            if matches!(n.shape, Shape::Group) && !selected {
                continue;
            }
            let (lo, hi) = node_box(n);
            let mut l = Vec::new();
            wire::box_lines(&mut l, lo, hi);
            for (a, b) in l {
                draw(math::place(&n.m, a), math::place(&n.m, b));
            }
            continue;
        }
        let lines = view.cache.lines(n, view.detail);
        for (a, b) in lines.iter() {
            draw(math::place(&n.m, *a), math::place(&n.m, *b));
        }
    }
}

/// Headless check of the snapping rules; returns lines saying what happened.
pub fn snap_check() -> Vec<String> {
    use crate::create::{self, Make};
    let mut out = Vec::new();
    let mut doc = Doc::new(create::new_project());
    let mut seed = 3;
    let cube = create::add(&mut doc, Make::Cube, &mut seed); // 2 across, standing on the ground at the origin
    let ball = create::add(&mut doc, Make::Sphere, &mut seed);
    doc.selection = vec![ball];
    doc.move_selection([6.0, 0.13, 0.0]); // floating 0.13 above the ground, away from the cube
    let all = Snap { ground: true, objects: true, grid: false, step: 0.5 };
    let bounds = |doc: &Doc| doc.project.world_bounds(&doc.selection).unwrap();
    let (lo, hi) = bounds(&doc);
    let (t, note) = snapped(&doc, all, lo, hi, [0.0; 3], 0.2);
    out.push(format!("ball 0.13 above ground, tolerance 0.2 -> move {:.3} ({note}); bottom lands at {:.3}", t[1], lo[1] + t[1]));
    let (t, note) = snapped(&doc, all, lo, hi, [0.0, 0.5, 0.0], 0.2);
    out.push(format!("ball lifted 0.63 above ground -> move {:.3} ({}): left alone = {}", t[1], if note.is_empty() { "no snap" } else { note }, t[1] == 0.5));
    // carry the ball over the cube, a little above its top (the cube's top is at y = 2)
    let (t, note) = snapped(&doc, all, lo, hi, [-6.1, 2.0, 0.05], 0.2);
    out.push(format!("ball carried to just above the cube -> bottom at {:.3} (cube top 2.000), x centre {:.3}, z centre {:.3} ({note})", lo[1] + t[1], 0.5 * (lo[0] + hi[0]) + t[0], 0.5 * (lo[2] + hi[2]) + t[2]));
    // beside the cube: faces meet (cube spans x -1..1; ball is 2 across)
    let (t, note) = snapped(&doc, all, lo, hi, [-3.9, -0.13, 0.0], 0.2);
    out.push(format!("ball pushed beside the cube -> ball left side at {:.3} (cube right side 1.000) ({note})", lo[0] + t[0]));
    let grid = Snap { ground: false, objects: false, grid: true, step: 0.5 };
    let (t, _) = snapped(&doc, grid, lo, hi, [0.37, 0.0, 0.21], 0.2);
    out.push(format!("grid 0.5: centre x {:.3}, z {:.3}, bottom {:.3}", 0.5 * (lo[0] + hi[0]) + t[0], 0.5 * (lo[2] + hi[2]) + t[2], lo[1] + t[1]));
    let off = Snap { ground: false, objects: false, grid: false, step: 0.5 };
    let (t, note) = snapped(&doc, off, lo, hi, [0.0; 3], 0.2);
    out.push(format!("all snapping off -> move {:?} ({})", t, if note.is_empty() { "no snap" } else { note }));
    let _ = cube;
    out
}
