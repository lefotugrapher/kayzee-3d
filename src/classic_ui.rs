//! The "Classic look": palettes drawn from the original program's own artwork,
//! read at run time from the user's own disc (never shipped with the app). Each
//! palette is drawn from its stored layout, and clicks and drags are reported
//! by element name for the app to act on.

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, Sense, TextureHandle, TextureOptions, Vec2};

use crate::classic::{self, Element, Layout, Sprite};

pub struct Skin {
    sprites: Arc<Vec<Sprite>>,
    layouts: Vec<Layout>,
    tex: HashMap<(String, String), TextureHandle>,
    masks: HashMap<(String, String), Arc<Vec<bool>>>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Act {
    Click,
    DragStart,
    /// Mouse travel this frame.
    Drag(Vec2),
    DragEnd,
}

#[derive(Clone, Debug)]
pub struct Event {
    pub name: String,
    pub act: Act,
    /// Where the press landed inside the element, -1..1 from its centre (y up).
    pub at: Vec2,
}

/// What to draw for an element this frame instead of its resting picture.
pub type Override<'a> = &'a dyn Fn(&str) -> Option<String>;

impl Skin {
    /// The art from the user's disc, or None when it has not been added.
    pub fn load() -> Option<Skin> {
        let sprites = classic::cached();
        if sprites.is_empty() {
            return None;
        }
        Some(Skin { sprites, layouts: classic::load_layouts(), tex: HashMap::new(), masks: HashMap::new() })
    }

    pub fn layout(&self, file: &str) -> Option<&Layout> {
        self.layouts.iter().find(|l| l.file.eq_ignore_ascii_case(file))
    }

    pub fn size(&self, file: &str) -> Vec2 {
        self.layout(file).and_then(|l| l.canvases.iter().max_by_key(|c| c.width * c.height)).map(|c| Vec2::new(c.width as f32, c.height as f32)).unwrap_or(Vec2::new(100.0, 100.0))
    }

    fn sprite(&self, file: &str, name: &str) -> Option<&Sprite> {
        classic::find(&self.sprites, file, name)
    }

    fn texture(&mut self, ctx: &egui::Context, file: &str, name: &str) -> Option<(TextureHandle, Vec2)> {
        let key = (file.to_owned(), name.to_owned());
        let s = classic::find(&self.sprites, file, name)?;
        let size = Vec2::new(s.width as f32, s.height as f32);
        if let Some(t) = self.tex.get(&key) {
            return Some((t.clone(), size));
        }
        let img = ColorImage::from_rgba_unmultiplied([s.width as usize, s.height as usize], &s.opaque_rgba());
        let t = ctx.load_texture(format!("classic:{file}:{name}"), img, TextureOptions::LINEAR);
        self.tex.insert(key, t.clone());
        Some((t, size))
    }

    fn inside(&mut self, file: &str, name: &str, x: i32, y: i32) -> bool {
        let key = (file.to_owned(), name.to_owned());
        let Some(s) = classic::find(&self.sprites, file, name) else { return true };
        let (w, h) = (s.width as i32, s.height as i32);
        if x < 0 || y < 0 || x >= w || y >= h {
            return false;
        }
        let mask = self.masks.entry(key).or_insert_with(|| Arc::new(s.hit_mask())).clone();
        mask.get((y * w + x) as usize).copied().unwrap_or(true)
    }

    /// The resting picture of an element, and its hover and pressed pictures when it has them.
    fn faces(&self, e: &Element) -> Option<(String, Option<String>, Option<String>)> {
        let s = classic::element_sprite(&self.sprites, e)?;
        let base = s.base_and_state().0.to_owned();
        let named = |w: &str| self.sprite(&e.file, &format!("{base} {w}")).map(|s| s.name.clone());
        Some((s.name.clone(), named("hilited"), named("selected")))
    }

    /// Draw palette `file` at `scale`, with `pick` able to swap an element's picture by name
    /// (for dials and on/off states), and report what the mouse did to which element.
    pub fn panel(&mut self, ui: &mut egui::Ui, file: &str, scale: f32, pick: Override) -> (Rect, Vec<Event>) {
        let size = self.size(file) * scale;
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let ctx = ui.ctx().clone();
        let Some(layout) = self.layout(file).cloned() else { return (rect, Vec::new()) };
        let painter = ui.painter_at(rect);
        let fill = layout.canvases.iter().max_by_key(|c| c.width * c.height).map(|c| Color32::from_rgb(c.fill[1], c.fill[2], c.fill[3])).unwrap_or(Color32::GRAY);
        painter.rect_filled(rect, 0.0, fill);
        let whole = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        let at = |x: i32, y: i32| rect.min + Vec2::new(x as f32, y as f32) * scale;
        for b in &layout.backgrounds {
            if let Some((t, s)) = self.texture(&ctx, file, b) {
                painter.image(t.id(), Rect::from_min_size(rect.min, s * scale), whole, Color32::WHITE);
            }
        }
        let id = ui.id().with(("classic", file));
        let active: Option<String> = ui.memory(|m| m.data.get_temp(id));
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let to_local = |p: Pos2| ((p.x - rect.left()) / scale, (p.y - rect.top()) / scale);

        // the topmost element under the mouse whose picture covers that point
        let mut under: Option<(Element, String)> = None;
        if let Some(p) = pointer.filter(|p| rect.contains(*p)) {
            let (lx, ly) = to_local(p);
            for e in layout.elements.iter().rev() {
                let [l, t, r, b] = e.abs;
                if (lx as i32) < l || (lx as i32) >= r || (ly as i32) < t || (ly as i32) >= b || e.frames.is_empty() && e.kind != "imel" {
                    continue;
                }
                let face = pick(&e.name).or_else(|| self.faces(e).map(|f| f.0));
                let Some(face) = face else { continue };
                if layout.backgrounds.contains(&face) {
                    continue;
                }
                if self.inside(file, &face, lx as i32 - l, ly as i32 - t) {
                    under = Some((e.clone(), face));
                    break;
                }
            }
        }

        for e in &layout.elements {
            let Some((rest, hover, press)) = self.faces(e) else { continue };
            if layout.backgrounds.contains(&rest) {
                continue;
            }
            let mut face = pick(&e.name).unwrap_or(rest);
            let held = active.as_deref() == Some(&e.name) && resp.is_pointer_button_down_on();
            if held {
                if let Some(p) = press {
                    face = p;
                }
            } else if under.as_ref().is_some_and(|(u, _)| u.name == e.name) {
                if let Some(h) = hover {
                    face = h;
                }
            }
            if let Some((t, s)) = self.texture(&ctx, file, &face) {
                painter.image(t.id(), Rect::from_min_size(at(e.abs[0], e.abs[1]), s * scale), whole, Color32::WHITE);
            }
        }

        let mut out = Vec::new();
        let rel = |e: &Element, p: Pos2| {
            let (lx, ly) = to_local(p);
            let [l, t, r, b] = e.abs;
            let (cx, cy) = ((l + r) as f32 * 0.5, (t + b) as f32 * 0.5);
            Vec2::new((lx - cx) / ((r - l) as f32 * 0.5).max(1.0), -(ly - cy) / ((b - t) as f32 * 0.5).max(1.0))
        };
        if resp.drag_started() || (resp.is_pointer_button_down_on() && active.is_none()) {
            if let (Some((e, _)), Some(p)) = (&under, resp.interact_pointer_pos()) {
                ui.memory_mut(|m| m.data.insert_temp(id, e.name.clone()));
                ui.memory_mut(|m| m.data.insert_temp(id.with("at"), rel(e, p)));
                if resp.drag_started() {
                    out.push(Event { name: e.name.clone(), act: Act::DragStart, at: rel(e, p) });
                }
            }
        }
        let active: Option<String> = ui.memory(|m| m.data.get_temp(id));
        let start: Vec2 = ui.memory(|m| m.data.get_temp(id.with("at"))).unwrap_or_default();
        if let Some(name) = &active {
            if resp.dragged() && resp.drag_delta() != Vec2::ZERO {
                out.push(Event { name: name.clone(), act: Act::Drag(resp.drag_delta() / scale), at: start });
            }
            if resp.drag_stopped() {
                out.push(Event { name: name.clone(), act: Act::DragEnd, at: start });
            }
        }
        if resp.clicked() {
            if let Some((e, _)) = &under {
                out.push(Event { name: e.name.clone(), act: Act::Click, at: start });
            }
        }
        if !resp.is_pointer_button_down_on() && !resp.dragged() && !resp.drag_stopped() {
            ui.memory_mut(|m| m.data.remove::<String>(id));
        }
        if let Some((e, _)) = &under {
            resp.on_hover_text(&e.name);
        }
        (rect, out)
    }

    /// The rectangle (in screen points) of an element of a palette drawn at `origin` and `scale`.
    pub fn element_rect(&self, file: &str, name: &str, origin: Pos2, scale: f32) -> Option<Rect> {
        let e = self.layout(file)?.elements.iter().find(|e| e.name == name)?;
        let [l, t, r, b] = e.abs;
        Some(Rect::from_min_max(origin + Vec2::new(l as f32, t as f32) * scale, origin + Vec2::new(r as f32, b as f32) * scale))
    }

    /// The average colour of a picture (for tinting the working window's paper).
    pub fn average(&self, file: &str, name: &str) -> Option<Color32> {
        let s = self.sprite(file, name)?;
        let n = (s.rgba.len() / 4).max(1) as u64;
        let mut sum = [0u64; 3];
        for p in s.rgba.chunks_exact(4) {
            for k in 0..3 {
                sum[k] += p[k] as u64;
            }
        }
        Some(Color32::from_rgb((sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8))
    }

    /// A picture as a texture (the working window's paper).
    pub fn picture(&mut self, ctx: &egui::Context, file: &str, name: &str) -> Option<(TextureHandle, Vec2)> {
        self.texture(ctx, file, name)
    }
}
