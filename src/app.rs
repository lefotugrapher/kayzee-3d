//! The workspace: palettes along the top, camera and render controls down the
//! left, display options down the right, selection or animation controls
//! along the bottom, and the working window in the middle.

use std::path::{Path, PathBuf};
use std::time::Instant;

use eframe::egui::{self, Align, Align2, Color32, ColorImage, FontId, Key, Layout, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, TextureOptions, Vec2};

use crate::create::{self, Make};
use crate::kit::{self, face};
use crate::model::{LightKind, Project, Shape};
use crate::state::Doc;
use crate::viewport::{ViewKind, Viewport};
use crate::wire::Side;
use crate::{anim, anim_ui, dialogs, emit, fileio, job, library, matlab, setup, sky_ui, terrain_ui};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Palette {
    Create,
    Edit,
    Sky,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lab {
    None,
    Materials,
    Terrain,
    Motion,
}

/// Settings kept between runs.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Prefs {
    /// Keep the rendered picture up while editing, redrawing it after each change.
    #[serde(default)]
    pub stay_rendered: bool,
    #[serde(default)]
    pub snap: crate::viewport::Snap,
    /// Look for a newer version when the app starts.
    #[serde(default = "on")]
    pub check_updates: bool,
    /// When the app last looked (seconds since 1970).
    #[serde(default)]
    pub last_update_check: u64,
    /// The newest version the user has already been told about.
    #[serde(default)]
    pub told_version: String,
}

fn on() -> bool {
    true
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { stay_rendered: false, snap: Default::default(), check_updates: true, last_update_check: 0, told_version: String::new() }
    }
}

impl Prefs {
    fn file() -> PathBuf {
        setup::data_folder().join("settings.json")
    }
    pub fn load() -> Prefs {
        std::fs::read_to_string(Self::file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
    pub fn save(&self) {
        let _ = std::fs::create_dir_all(setup::data_folder());
        if let Ok(t) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(Self::file(), t);
        }
    }
}

pub struct Picture {
    pub job: Option<job::Handle>,
    pub texture: Option<TextureHandle>,
    /// The picture before this one, shown beneath while the new one fills in.
    pub under: Option<TextureHandle>,
    seen: u64,
    last_upload: Instant,
}

impl Default for Picture {
    fn default() -> Self {
        Picture { job: None, texture: None, under: None, seen: 0, last_upload: Instant::now() }
    }
}

impl Picture {
    /// Bring the texture up to date with the render; unfinished pixels stay clear.
    pub fn refresh(&mut self, ctx: &egui::Context, name: &str) {
        let Some(j) = &self.job else { return };
        let s = j.lock().unwrap();
        let running = matches!(s.state, job::State::Running);
        if !running && self.texture.is_some() && s.stamp == self.seen {
            // the new picture is whole (or the render ended): the old one is no longer needed
            self.under = None;
        }
        if matches!(s.state, job::State::Failed(_)) {
            self.under = None;
        }
        if s.stamp == self.seen || s.pixels_done == 0 {
            return;
        }
        if running && self.last_upload.elapsed().as_millis() < 90 {
            ctx.request_repaint_after(std::time::Duration::from_millis(90));
            return;
        }
        let (w, h) = (s.width as usize, s.height as usize);
        let mut rgba = vec![0u8; w * h * 4];
        for i in 0..w * h {
            if s.drawn[i] {
                rgba[i * 4..i * 4 + 3].copy_from_slice(&s.rgb[i * 3..i * 3 + 3]);
                rgba[i * 4 + 3] = 255;
            }
        }
        self.seen = s.stamp;
        drop(s);
        let image = ColorImage::from_rgba_unmultiplied([w, h], &rgba);
        match &mut self.texture {
            Some(t) => t.set(image, TextureOptions::LINEAR),
            None => self.texture = Some(ctx.load_texture(name, image, TextureOptions::LINEAR)),
        }
        self.last_upload = Instant::now();
    }
    pub fn clear(&mut self) {
        self.job = None;
        self.texture = None;
        self.under = None;
        self.seen = 0;
    }
    /// Start over with a new render, keeping the picture so far on show beneath it.
    pub fn restart(&mut self, job: job::Handle) {
        if self.texture.is_some() {
            self.under = self.texture.take();
        }
        self.job = Some(job);
        self.seen = 0;
    }
    /// (width, height, rgb) of a finished render.
    pub fn finished(&self) -> Option<(u32, u32, Vec<u8>)> {
        let s = self.job.as_ref()?.lock().unwrap();
        matches!(s.state, job::State::Done { .. }).then(|| (s.width, s.height, s.rgb.clone()))
    }
}

pub struct App {
    pub doc: Doc,
    pub view: Viewport,
    pub palette: Palette,
    pub show_animation: bool,
    pub lab: Lab,
    pub main: Picture,
    pub nano: Picture,
    pub nano_auto: bool,
    nano_rev: u64,
    last_change: Instant,
    /// Show the rendered picture in the working window (else the wireframe).
    pub show_picture: bool,
    pub message: Option<(bool, String)>,
    pub ready: bool,
    pub rng: u64,
    /// The project as of the last settled change; keys are recorded against it.
    key_base: (u64, Project),
    pub playing: Option<(Instant, f64)>,
    pub sky: sky_ui::State,
    pub mat: matlab::State,
    pub ter: terrain_ui::State,
    pub ani: anim_ui::State,
    pub lib: library::State,
    pub dlg: dialogs::State,
    pub randomize_what: u8,
    pub prefs: Prefs,
    prefs_saved: Prefs,
    /// The scene revision the picture on show was rendered from.
    rendered_rev: u64,
    test_nudge: Option<Instant>,
    pub update: crate::update::Shared,
    /// The user asked to look (so "you are up to date" is worth saying).
    update_asked: bool,
    update_hidden: bool,
    quit_ok: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, first: Option<PathBuf>, auto_render: bool) -> Self {
        kit::apply_theme(&cc.egui_ctx);
        let project = create::new_project();
        let mut app = App {
            key_base: (1, project.clone()),
            doc: Doc::new(project),
            view: Viewport::default(),
            palette: Palette::Create,
            show_animation: true,
            lab: Lab::None,
            main: Picture::default(),
            nano: Picture::default(),
            nano_auto: true,
            nano_rev: 0,
            last_change: Instant::now(),
            show_picture: false,
            message: None,
            ready: setup::ready(),
            rng: 0x2545F4914F6CDD1D ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7),
            playing: None,
            sky: Default::default(),
            mat: Default::default(),
            ter: Default::default(),
            ani: Default::default(),
            lib: Default::default(),
            dlg: Default::default(),
            randomize_what: 7,
            prefs: Prefs::load(),
            prefs_saved: Prefs::load(),
            rendered_rev: 0,
            test_nudge: None,
            update: std::sync::Arc::new(std::sync::Mutex::new(crate::update::Status::Idle)),
            update_asked: false,
            update_hidden: false,
            quit_ok: false,
        };
        if let Some(p) = first {
            app.open_path(&p);
        }
        let now = crate::update::now_secs();
        if app.prefs.check_updates && now.saturating_sub(app.prefs.last_update_check) > 6 * 3600 {
            app.prefs.last_update_check = now;
            crate::update::check(&app.update, cc.egui_ctx.clone());
        }
        if auto_render {
            app.dlg.auto_render = true;
        }
        // RIDGELINE_START=edit,select,terrain … opens the app in a given state (used to check each screen)
        for word in std::env::var("RIDGELINE_START").unwrap_or_default().split(',') {
            match word {
                "edit" => app.palette = Palette::Edit,
                "sky" => app.palette = Palette::Sky,
                "anim" => app.show_animation = true,
                "select" => {
                    let pick = app.doc.project.nodes.iter().find(|n| matches!(n.shape, Shape::Sphere)).or(app.doc.project.nodes.get(1)).map(|n| n.id);
                    app.doc.selection = pick.into_iter().collect();
                }
                "select-terrain" => app.doc.selection = app.doc.project.nodes.iter().find(|n| matches!(n.shape, Shape::Terrain(_))).map(|n| n.id).into_iter().collect(),
                "materials" => matlab::open(&mut app),
                "terrain" => terrain_ui::open(&mut app),
                "motion" => app.lab = Lab::Motion,
                "attributes" => app.dlg.attributes = true,
                "stay" => app.prefs.stay_rendered = true,
                "nostay" => app.prefs.stay_rendered = false,
                // move the selection a few seconds after starting (checks that the picture follows)
                "nudge" => app.test_nudge = Some(Instant::now() + std::time::Duration::from_secs(5)),
                "matlib" => app.lib.open = Some(library::Kind::Materials),
                "top" => {
                    app.view.view = ViewKind::Flat(Side::Top);
                    app.view.frame_all(&app.doc);
                }
                _ => {}
            }
        }
        app
    }

    pub fn say(&mut self, text: impl Into<String>) {
        self.message = Some((false, text.into()));
    }
    pub fn warn(&mut self, text: impl Into<String>) {
        self.message = Some((true, text.into()));
    }

    /// Forget the key-recording baseline (after undo, opening, scrubbing).
    pub fn rebase(&mut self) {
        self.key_base = (self.doc.revision, self.doc.project.clone());
    }

    pub fn replace_project(&mut self, project: Project, file: Option<PathBuf>) {
        job::stop();
        self.main.clear();
        self.nano.clear();
        self.doc = Doc::new(project);
        self.doc.file = file;
        self.view = Viewport::default();
        self.show_picture = false;
        self.lab = Lab::None;
        self.rebase();
    }

    pub fn open_path(&mut self, path: &Path) {
        match fileio::open(path) {
            Ok(project) => {
                let own = path.extension().is_some_and(|e| e.eq_ignore_ascii_case(fileio::EXT));
                self.replace_project(project, own.then(|| path.to_path_buf()));
                {
                    self.doc.project.name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                }
                self.message = None;
            }
            Err(e) => self.warn(format!("That file could not be opened. {e}")),
        }
    }

    // ----------------------------------------------------------------- rendering

    pub fn rendering(&self) -> bool {
        job::is_running(&self.main.job)
    }

    pub fn start_render(&mut self, ctx: &egui::Context) {
        if !self.ready {
            self.warn("Set up from your disc first (File > Set Up From Disc…).");
            return;
        }
        self.render_again(ctx, false);
    }

    /// Render; `keep` leaves the picture already on show in place until the new one covers it.
    fn render_again(&mut self, ctx: &egui::Context, keep: bool) {
        job::stop();
        self.nano.job = None;
        if !keep {
            self.main.clear();
        }
        self.rendered_rev = self.doc.revision;
        let p = &self.doc.project;
        let k = p.render.scale.max(0.05);
        let size = (((p.size[0] as f64) * k).round().max(8.0) as u32, ((p.size[1] as f64) * k).round().max(8.0) as u32);
        let text = emit::description(p, size, p.render.antialias);
        let job = job::start_mode(text, setup::substrate_path(), size, ctx.clone(), p.render.mode.clone());
        if keep { self.main.restart(job) } else { self.main.job = Some(job) }
        self.show_picture = true;
        self.message = None;
    }

    pub fn stop_render(&mut self) {
        job::stop();
    }

    fn nano_tick(&mut self, ctx: &egui::Context) {
        if !self.ready || !self.nano_auto || self.rendering() || self.lab != Lab::None {
            return;
        }
        if self.doc.revision == self.nano_rev || job::is_running(&self.nano.job) {
            if self.doc.revision != self.nano_rev {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            return;
        }
        if self.last_change.elapsed().as_millis() < 350 {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
            return;
        }
        self.nano_rev = self.doc.revision;
        let p = &self.doc.project;
        let w = 132u32;
        let h = ((w as f64) * p.size[1] as f64 / p.size[0].max(1) as f64).round().clamp(40.0, 132.0) as u32;
        let text = emit::description(p, (w, h), 0);
        self.nano.seen = 0;
        self.nano.job = Some(job::start(text, setup::substrate_path(), (w, h), ctx.clone()));
    }

    // ----------------------------------------------------------------- palettes

    fn top(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(150.0);
            ui.spacing_mut().item_spacing.x = 26.0;
            for (p, label) in [(Palette::Create, "Create"), (Palette::Edit, "Edit"), (Palette::Sky, "Sky & Fog")] {
                let on = self.palette == p;
                let colour = if on { Color32::WHITE } else { Color32::from_gray(234) };
                let text = RichText::new(label).font(FontId::proportional(16.0)).strong().color(colour);
                let resp = ui.add(egui::Label::new(text).sense(Sense::click()));
                let r = resp.rect;
                // a small arrow after the name
                let tip = Pos2::new(r.right() + 13.0, r.center().y + 1.0);
                ui.painter().add(egui::Shape::convex_polygon(vec![tip, tip + Vec2::new(-7.0, -4.5), tip + Vec2::new(-7.0, 4.5)], colour, Stroke::NONE));
                if on {
                    ui.painter().line_segment([Pos2::new(r.left(), r.bottom() + 1.0), Pos2::new(r.right(), r.bottom() + 1.0)], Stroke::new(1.5, Color32::WHITE));
                }
                if resp.clicked() {
                    self.palette = p;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let star = if self.doc.dirty { " •" } else { "" };
                ui.label(RichText::new(format!("{}{star}", self.doc.project.name)).color(kit::DIM));
            });
        });
        ui.add_space(4.0);
        egui::ScrollArea::horizontal().id_salt("palette").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(150.0);
                match self.palette {
                    Palette::Create => self.create_palette(ui),
                    Palette::Edit => self.edit_palette(ui),
                    Palette::Sky => sky_ui::palette(self, ui),
                }
            });
        });
    }

    fn create_palette(&mut self, ui: &mut egui::Ui) {
        use kit::solid;
        type Draw = fn(&egui::Painter, Rect);
        let items: [(Make, Draw, &[Make]); 22] = [
            (Make::Water, solid::water, &[]),
            (Make::Cloud, solid::cloud, &[]),
            (Make::Ground, solid::ground, &[]),
            (Make::Terrain, solid::terrain, &[]),
            (Make::Stone, solid::stone, &[]),
            (Make::Lattice, solid::lattice, &[]),
            (Make::Sphere, solid::sphere, &[Make::Ellipsoid, Make::Egg]),
            (Make::Ellipsoid, solid::squashed_sphere, &[]),
            (Make::Egg, solid::stretched_sphere, &[]),
            (Make::Torus, solid::torus, &[]),
            (Make::Cylinder, solid::cylinder, &[Make::Puck, Make::Rod]),
            (Make::Cube, solid::cube, &[Make::Brick, Make::Tower]),
            (Make::Brick, solid::flat_box, &[]),
            (Make::Tower, solid::tall_box, &[]),
            (Make::Pyramid, solid::pyramid, &[Make::FlatPyramid, Make::Obelisk]),
            (Make::Cone, solid::cone, &[Make::FlatCone, Make::Spike]),
            (Make::Disc, solid::disc, &[]),
            (Make::Square, solid::square, &[]),
            (Make::Radial, solid::light_radial, &[]),
            (Make::Spot, solid::light_spot, &[]),
            (Make::SquareSpot, solid::light_square, &[]),
            (Make::Parallel, solid::light_parallel, &[]),
        ];
        let size = 40.0;
        // the shelf the objects stand on
        let origin = ui.cursor().min;
        let width = (items.len() + 3) as f32 * (size + 4.0) + 26.0;
        let (top, bottom) = (origin.y + size * 0.5, origin.y + size + 8.0);
        ui.painter().add(egui::Shape::convex_polygon(
            vec![Pos2::new(origin.x + 10.0, top), Pos2::new(origin.x + width, top), Pos2::new(origin.x + width - 10.0, bottom), Pos2::new(origin.x - 4.0, bottom)],
            Color32::from_gray(236),
            Stroke::new(1.0, Color32::from_gray(150)),
        ));
        ui.painter().line_segment([Pos2::new(origin.x - 4.0, bottom + 1.5), Pos2::new(origin.x + width - 10.0, bottom + 1.5)], Stroke::new(2.0, Color32::from_black_alpha(60)));
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_space(8.0);
        for (make, draw, more) in items {
            let resp = kit::shelf_button(ui, size, make.label(), draw);
            if resp.clicked() {
                create::add(&mut self.doc, make, &mut self.rng);
            }
            if !more.is_empty() {
                resp.context_menu(|ui| {
                    for m in more {
                        if ui.button(m.label()).clicked() {
                            create::add(&mut self.doc, *m, &mut self.rng);
                            ui.close_menu();
                        }
                    }
                });
            }
        }
        if kit::shelf_button(ui, size, "Picture object: a flat face showing a picture", solid::picture).clicked() {
            dialogs::add_picture_object(self);
        }
        if kit::shelf_button(ui, size, "Import a 3D model (OBJ, DXF, 3DS, 3DMF)", solid::model).clicked() {
            fileio::import_model(self);
        }
        if kit::shelf_button(ui, size, "Object library", solid::books).clicked() {
            self.lib.open = Some(library::Kind::Objects);
        }
    }

    fn edit_palette(&mut self, ui: &mut egui::Ui) {
        let size = Vec2::new(58.0, 58.0);
        let some = !self.doc.selection.is_empty();
        ui.add_enabled_ui(some, |ui| {
            if kit::tool_button(ui, size, "Materials", "Open the Materials Lab for the selection", false, face::material).clicked() {
                matlab::open(self);
            }
            ui.add_space(8.0);
            // each tool: three strips to drag, one per axis, and one for all three
            let strip = Vec2::new(26.0, 26.0);
            let axes = [("X", Color32::from_rgb(0xe0, 0x6c, 0x5a)), ("Y", Color32::from_rgb(0x7c, 0xc4, 0x8a)), ("Z", Color32::from_rgb(0x5c, 0x98, 0xe6))];
            ui.vertical(|ui| {
                kit::heading(ui, "RESIZE");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    for (k, (label, colour)) in axes.iter().enumerate() {
                        let (d, resp) = kit::axis_strip(ui, strip, label, *colour, "Drag to resize along this axis");
                        self.drag_edit(&resp, d, |doc, d| {
                            let mut f = [1.0; 3];
                            f[k] = (1.0 + d * 0.01).max(0.02);
                            doc.scale_own(f);
                        });
                    }
                    let (d, resp) = kit::axis_strip(ui, strip, "All", kit::TEXT, "Drag to resize evenly");
                    self.drag_edit(&resp, d, |doc, d| doc.scale_own([(1.0 + d * 0.01).max(0.02); 3]));
                });
            });
            ui.add_space(8.0);
            ui.vertical(|ui| {
                kit::heading(ui, "ROTATE");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    for (k, (label, colour)) in axes.iter().enumerate() {
                        let (d, resp) = kit::axis_strip(ui, strip, label, *colour, "Drag to turn about this axis (shift: steps of 45°)");
                        self.drag_edit(&resp, d, |doc, d| doc.rotate_own(k, d * 0.012));
                    }
                });
            });
            ui.add_space(8.0);
            ui.vertical(|ui| {
                kit::heading(ui, "REPOSITION");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    for (k, (label, colour)) in axes.iter().enumerate() {
                        let (d, resp) = kit::axis_strip(ui, strip, label, *colour, "Drag to move along this world axis");
                        self.drag_edit(&resp, d, |doc, d| {
                            let mut by = [0.0; 3];
                            by[k] = d * 0.02;
                            doc.move_selection(by);
                        });
                    }
                });
            });
            ui.add_space(8.0);
            let align = kit::tool_button(ui, size, "Align", "Line the selection up (click for choices)", false, face::align);
            kit::menu_below(ui, &align, false, |ui| {
                for (axis, name) in ["X", "Y", "Z"].iter().enumerate() {
                    for (mode, how) in ["low sides", "centres", "high sides"].iter().enumerate() {
                        if ui.button(format!("Align {name}: {how}")).clicked() {
                            self.doc.align_selection(axis, mode as u8);
                        }
                    }
                    if axis < 2 {
                        ui.separator();
                    }
                }
            });
            let (d, resp) = {
                let r = kit::tool_button(ui, size, "Randomize", "Drag sideways to scatter the selection; right-click for what to scatter", false, face::scatter);
                (if r.dragged() { r.drag_delta().x } else { 0.0 }, r)
            };
            let what = self.randomize_what;
            let mut seed = self.rng;
            self.drag_edit(&resp, d, |doc, d| doc.randomize_selection((d.abs() * 0.004).min(0.5), what, &mut seed));
            self.rng = seed;
            resp.context_menu(|ui| {
                for (bit, label) in [(1u8, "Position"), (2, "Rotation"), (4, "Size")] {
                    let mut on = self.randomize_what & bit != 0;
                    if ui.checkbox(&mut on, label).changed() {
                        self.randomize_what ^= bit;
                    }
                }
            });
            if kit::tool_button(ui, size, "Land", "Drop the selection onto whatever is below it", false, face::down_to_ground).clicked() {
                self.doc.land_selection();
            }
            ui.add_space(8.0);
            let editable = self.doc.one().map(|n| match &n.shape {
                Shape::Terrain(_) => "Edit Terrain",
                Shape::Light(_) => "Edit Light",
                Shape::Torus { .. } => "Edit Torus",
                _ => "Attributes",
            });
            if kit::tool_button(ui, size, "Edit", editable.unwrap_or("Object attributes"), false, face::pencil).clicked() {
                self.edit_object();
            }
            if kit::tool_button(ui, size, "Group", "Group the selection (⌘G)", false, face::group).clicked() {
                self.doc.group_selection();
            }
            if kit::tool_button(ui, size, "Ungroup", "Ungroup (⌘U)", false, face::ungroup).clicked() {
                self.doc.ungroup_selection();
            }
            let conv = kit::tool_button(ui, size, "Convert", "Turn the selection into another kind of object", false, face::text("⇄"));
            kit::menu_below(ui, &conv, false, |ui| {
                for (label, shape) in [("Sphere", Shape::Sphere), ("Cube", Shape::Cube), ("Cylinder", Shape::Cylinder), ("Cone", Shape::Cone), ("Pyramid", Shape::Pyramid), ("Torus", Shape::Torus { tube: 0.25, ring: 0.75 }), ("Disc", Shape::Disc), ("Square", Shape::Square)] {
                    if ui.button(label).clicked() {
                        self.doc.checkpoint();
                        for id in self.doc.selection.clone() {
                            if let Some(n) = self.doc.project.node_mut(id) {
                                if !matches!(n.shape, Shape::Group | Shape::Light(_) | Shape::Plane) {
                                    n.shape = shape.clone();
                                }
                            }
                        }
                        self.doc.touch();
                    }
                }
            });
        });
        if !some {
            ui.add_space(12.0);
            ui.label(RichText::new("Select an object to edit it.").color(kit::DIM));
        }
    }

    /// Shared plumbing for drag tools: save state when the drag starts, apply
    /// the change while it moves.
    fn drag_edit(&mut self, resp: &egui::Response, amount: f32, apply: impl FnOnce(&mut Doc, f64)) {
        if resp.drag_started() {
            self.doc.checkpoint();
        }
        if amount != 0.0 {
            apply(&mut self.doc, amount as f64);
            self.doc.refit_groups();
            self.doc.touch();
        }
    }

    pub fn edit_object(&mut self) {
        let Some(n) = self.doc.one() else {
            self.dlg.attributes = !self.doc.selection.is_empty();
            return;
        };
        match &n.shape {
            Shape::Terrain(_) => terrain_ui::open(self),
            Shape::Light(_) => self.dlg.light = true,
            _ => self.dlg.attributes = true,
        }
    }

    // ----------------------------------------------------------------- left column

    fn left(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let w = ui.available_width();
        // saved views down the side of the small preview
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 3.0);
            ui.vertical(|ui| {
                for i in 0..self.doc.project.view_dots.len() {
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(12.0, 6.0), Sense::click());
                    let full = self.doc.project.view_dots[i].is_some();
                    ui.painter().rect_filled(r, 3.0, if full { kit::solid::BLUE } else { Color32::from_gray(if resp.hovered() { 250 } else { 226 }) });
                    ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, Color32::from_gray(120)), egui::StrokeKind::Inside);
                    if resp.clicked() {
                        let alt = ui.input(|i| i.modifiers.alt);
                        if full && alt {
                            self.doc.project.view_dots[i] = None;
                        } else if full {
                            if let Some(c) = self.doc.project.view_dots[i].clone() {
                                self.doc.checkpoint();
                                self.doc.project.cam = c;
                                self.view.view = ViewKind::Camera;
                                self.doc.touch();
                            }
                        } else {
                            self.doc.project.view_dots[i] = Some(self.doc.project.cam.clone());
                        }
                    }
                    resp.on_hover_text("Saved camera position: click an empty one to save, a blue one to return, option-click to clear");
                }
            });
            let pw = w - 20.0;
            let aspect = self.doc.project.size[1] as f32 / self.doc.project.size[0].max(1) as f32;
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(pw, (pw * aspect).clamp(46.0, pw)), Sense::click());
            ui.painter().rect_filled(rect.translate(Vec2::new(2.0, 3.0)), 0.0, Color32::from_black_alpha(60));
            ui.painter().rect_filled(rect, 0.0, Color32::from_gray(226));
            if let Some(t) = &self.nano.texture {
                ui.painter().image(t.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            let resp = resp.on_hover_text("Small preview. Click to redraw; right-click for automatic updates.");
            if resp.clicked() {
                self.nano_rev = 0;
                self.last_change = Instant::now() - std::time::Duration::from_secs(1);
            }
            resp.context_menu(|ui| {
                ui.checkbox(&mut self.nano_auto, "Update automatically");
            });
        });
        ui.add_space(6.0);

        // which view the working window shows
        ui.vertical_centered(|ui| {
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(84.0, 62.0), Sense::click());
            kit::solid::plate(&ui.painter_at(rect), rect);
            let label = match self.view.view {
                ViewKind::Camera => "Camera View",
                ViewKind::Flat(s) => s.label(),
            };
            let resp = resp.on_hover_text(format!("{label}: click to choose the view"));
            kit::menu_below(ui, &resp, false, |ui| {
                if ui.selectable_label(self.view.view == ViewKind::Camera, "Camera View").clicked() {
                    self.view.view = ViewKind::Camera;
                }
                for s in Side::ALL {
                    if ui.selectable_label(self.view.view == ViewKind::Flat(s), s.label()).clicked() {
                        self.view.view = ViewKind::Flat(s);
                        self.view.frame_all(&self.doc);
                    }
                }
            });
        });
        ui.add_space(2.0);

        // camera: two crosses, a slider pair, then the big trackball
        let reach = self.doc.project.cam.distance_to_pivot();
        ui.horizontal(|ui| {
            ui.add_space((w - 112.0) * 0.5);
            let (d, r) = kit::ball_pad(ui, 52.0, "Move the camera over the ground: left/right and forward/back", 15, true);
            self.cam_drag(&r, d, |c, d| c.pan_ground(d.x as f64 * reach * 0.004, -d.y as f64 * reach * 0.004));
            let (d, r) = kit::ball_pad(ui, 52.0, "Move the camera left/right and up/down", 15, true);
            self.cam_drag(&r, d, |c, d| c.pan_view(d.x as f64 * reach * 0.004, -d.y as f64 * reach * 0.004));
        });
        ui.vertical_centered(|ui| {
            let (d, r) = kit::ball_pad(ui, 44.0, "Move the camera forward and back", 3, true);
            self.cam_drag(&r, d, |c, d| c.dolly(-d.y as f64 * reach * 0.006));
            ui.add_space(4.0);
            let (d, r) = kit::ball_pad(ui, 78.0, "Trackball: drag to swing the camera around the scene (option: look around in place)", 15, false);
            let alt = ui.input(|i| i.modifiers.alt);
            self.cam_drag(&r, d, |c, d| if alt { c.look(-d.x as f64 * 0.006, -d.y as f64 * 0.006) } else { c.orbit(-d.x as f64 * 0.01, -d.y as f64 * 0.01) });
        });
        ui.horizontal(|ui| {
            ui.add_space((w - 112.0) * 0.5);
            let (d, r) = kit::ball_pad(ui, 24.0, "Banking: tilt the horizon", 12, false);
            self.cam_drag(&r, d, |c, d| c.bank(d.x as f64 * 0.006));
            let (d, r) = kit::ball_pad(ui, 24.0, "Field of view: drag to widen or narrow the lens", 3, false);
            self.cam_drag(&r, d, |c, d| c.fov = (c.fov - d.y as f64 * 0.004).clamp(0.02, 3.0));
            if kit::ball_button(ui, 24.0, "Aim at the selection (or the scene's centre) and turn about it", false).clicked() {
                let target = self.doc.selection_centre().unwrap_or([0.0, 1.0, 0.0]);
                self.doc.checkpoint();
                self.doc.project.cam.pivot = target;
                self.doc.project.cam.look_at(target);
                self.doc.touch();
            }
            if kit::ball_button(ui, 24.0, "Camera settings", false).clicked() {
                self.dlg.camera = true;
            }
        });
        ui.add_space(8.0);

        // the render row: small, medium, large, medium, small
        let running = self.rendering();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.add_space((w - 122.0) * 0.5);
            ui.vertical(|ui| {
                ui.add_space(20.0);
                if kit::ball_button(ui, 16.0, "Render options", false).clicked() {
                    self.dlg.render_options = true;
                }
            });
            ui.vertical(|ui| {
                ui.add_space(14.0);
                if kit::ball_button(ui, 22.0, "Redraw the small preview", false).clicked() {
                    self.nano_rev = 0;
                    self.last_change = Instant::now() - std::time::Duration::from_secs(1);
                }
            });
            if kit::ball_button(ui, 40.0, if running { "Stop rendering (Esc)" } else { "Render the scene (⌘R)" }, running).clicked() {
                if running { self.stop_render() } else { self.start_render(ctx) }
            }
            ui.vertical(|ui| {
                ui.add_space(14.0);
                if kit::ball_button(ui, 22.0, "Show the rendered picture / the wireframe", self.show_picture).clicked() && self.main.texture.is_some() {
                    self.show_picture = !self.show_picture;
                }
            });
            ui.vertical(|ui| {
                ui.add_space(20.0);
                if kit::ball_button(ui, 16.0, "Clear the rendered picture", false).clicked() {
                    job::stop();
                    self.main.clear();
                    self.show_picture = false;
                }
            });
        });

        // what is selected, how big the scene is, and the time
        ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
            let a = &self.doc.project.anim;
            let frame = (a.time * a.fps).round() as i64;
            ui.label(RichText::new(format!("{:02}:{:02}:{:02}.{:02}", 0, (a.time as i64) / 60, (a.time as i64) % 60, frame.rem_euclid(a.fps.round().max(1.0) as i64))).strong().small());
            let status = match &self.main.job {
                Some(j) => {
                    let s = j.lock().unwrap();
                    match &s.state {
                        job::State::Running => format!("Rendering {:.0}%", 100.0 * s.pixels_done as f32 / (s.width * s.height).max(1) as f32),
                        job::State::Done { seconds } => format!("Rendered in {seconds:.1} s"),
                        job::State::Failed(_) => "Could not render".to_owned(),
                        job::State::Stopped => "Stopped".to_owned(),
                    }
                }
                None => String::new(),
            };
            let count = self.doc.project.nodes.len();
            let second = if status.is_empty() { format!("{count} object{}", if count == 1 { "" } else { "s" }) } else { status };
            ui.label(RichText::new(second).strong());
            let first = match self.doc.selection.len() {
                0 => String::new(),
                1 => self.doc.one().map(|n| n.name.clone()).unwrap_or_default(),
                k => format!("{k} selected"),
            };
            ui.label(RichText::new(first).strong());
        });
    }

    fn cam_drag(&mut self, resp: &egui::Response, d: Vec2, apply: impl FnOnce(&mut crate::math::Cam, Vec2)) {
        if resp.drag_started() {
            self.doc.checkpoint();
        }
        if d != Vec2::ZERO {
            apply(&mut self.doc.project.cam, d);
            self.view.view = ViewKind::Camera;
            self.doc.touch();
        }
    }

    // ----------------------------------------------------------------- right column

    fn right(&mut self, ui: &mut egui::Ui) {
        let s = 24.0;
        let has_picture = self.main.texture.is_some();
        if kit::bare_icon(ui, s, "Show the wireframe", !self.show_picture, face::wire).clicked() {
            self.show_picture = false;
        }
        if ui.add_enabled_ui(has_picture, |ui| kit::bare_icon(ui, s, "Show the rendered picture", self.show_picture, face::shaded)).inner.clicked() {
            self.show_picture = true;
        }
        if kit::bare_icon(ui, s, "Stay rendered: keep the picture up while you edit and redraw it after each change", self.prefs.stay_rendered, face::picture).clicked() {
            self.prefs.stay_rendered = !self.prefs.stay_rendered;
        }
        let snap_on = self.prefs.snap.ground || self.prefs.snap.objects || self.prefs.snap.grid;
        let magnet = kit::bare_icon(ui, s, "Snapping: click for what moved objects catch on (hold ⌘ while dragging to switch it off)", snap_on, face::magnet);
        kit::menu_below(ui, &magnet, true, |ui| {
            let sn = &mut self.prefs.snap;
            ui.checkbox(&mut sn.ground, "Snap to the ground");
            ui.checkbox(&mut sn.objects, "Snap to other objects");
            ui.checkbox(&mut sn.grid, "Snap to a grid");
            let mut step = sn.step * crate::dialogs::UNITS;
            if ui.add_enabled(sn.grid, egui::DragValue::new(&mut step).speed(0.1).range(0.1..=500.0).prefix("grid step ")).changed() {
                sn.step = step / crate::dialogs::UNITS;
            }
            ui.separator();
            ui.label(RichText::new(kit::keys("Drag: slide over the ground.\noption-drag: straight up and down.\nHold X, Y or Z: keep to that axis.\nHold ⌘: no snapping.")).small().color(kit::DIM));
        });
        ui.add_space(6.0);
        if kit::bare_icon(ui, s, "Zoom in", false, face::plus).clicked() {
            self.zoom(0.8);
        }
        if kit::bare_icon(ui, s, "Zoom out", false, face::minus).clicked() {
            self.zoom(1.25);
        }
        if kit::bare_icon(ui, s, "Fit everything in view", false, face::arrows_out).clicked() {
            match self.view.view {
                ViewKind::Flat(_) => self.view.frame_all(&self.doc),
                ViewKind::Camera => {
                    let ids: Vec<u64> = self.doc.project.nodes.iter().filter(|n| !matches!(n.shape, Shape::Plane)).map(|n| n.id).collect();
                    if let Some((lo, hi)) = self.doc.project.world_bounds(&ids) {
                        self.doc.checkpoint();
                        let c = crate::math::mul(crate::math::add(lo, hi), 0.5);
                        let r = crate::math::len(crate::math::sub(hi, lo)).max(2.0);
                        let cam = &mut self.doc.project.cam;
                        cam.pivot = c;
                        cam.eye = crate::math::sub(c, crate::math::mul(cam.forward, r * 1.4));
                        self.doc.touch();
                    }
                }
            }
        }
        ui.add_space(6.0);
        if kit::bare_icon(ui, s, "Depth cue: fade far lines", self.view.depth_cue, face::eye).clicked() {
            self.view.depth_cue = !self.view.depth_cue;
        }
        if kit::bare_icon(ui, s, "Show infinite planes as grids", self.view.show_ground_grid, face::ground).clicked() {
            self.view.show_ground_grid = !self.view.show_ground_grid;
        }
        if kit::bare_icon(ui, s, "More wireframe detail", false, face::text("+w")).clicked() {
            self.view.detail = (self.view.detail * 2).min(128);
        }
        if kit::bare_icon(ui, s, "Less wireframe detail", false, face::text("−w")).clicked() {
            self.view.detail = (self.view.detail / 2).max(8);
        }
        ui.add_space(6.0);
        let mut paper = [self.view.paper.r() as f32 / 255.0, self.view.paper.g() as f32 / 255.0, self.view.paper.b() as f32 / 255.0];
        if kit::colour_well(ui, &mut paper, "Background paper colour") {
            self.view.paper = kit::rgb(paper);
        }
    }

    fn zoom(&mut self, factor: f64) {
        match self.view.view {
            ViewKind::Flat(_) => self.view.flat_half *= factor,
            ViewKind::Camera => {
                self.doc.checkpoint();
                let d = self.doc.project.cam.distance_to_pivot();
                self.doc.project.cam.dolly(d * (1.0 - factor));
                self.doc.touch();
            }
        }
    }

    // ----------------------------------------------------------------- bottom strip

    fn bottom(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            if self.show_animation {
                anim_ui::strip(self, ui, ctx);
            } else {
                self.selection_strip(ui);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = if self.show_animation { "Selection" } else { "Animation" };
                if ui.button(label).on_hover_text("Switch between the selection and animation controls").clicked() {
                    self.show_animation = !self.show_animation;
                }
            });
        });
    }

    fn selection_strip(&mut self, ui: &mut egui::Ui) {
        type Draw = fn(&egui::Painter, Rect, Color32);
        type Pick = fn(&Shape) -> bool;
        let kinds: [(&str, Draw, Pick); 11] = [
            ("Select spheres", face::sphere, |s| matches!(s, Shape::Sphere)),
            ("Select tori", face::torus, |s| matches!(s, Shape::Torus { .. })),
            ("Select cylinders", face::cylinder, |s| matches!(s, Shape::Cylinder)),
            ("Select cubes", face::cube, |s| matches!(s, Shape::Cube)),
            ("Select pyramids", face::pyramid, |s| matches!(s, Shape::Pyramid)),
            ("Select cones", face::cone, |s| matches!(s, Shape::Cone)),
            ("Select flat faces", face::square, |s| matches!(s, Shape::Square | Shape::Disc)),
            ("Select terrains", face::terrain, |s| matches!(s, Shape::Terrain(_))),
            ("Select meshes and stones", face::stone, |s| matches!(s, Shape::Mesh(_))),
            ("Select infinite planes", face::ground, |s| matches!(s, Shape::Plane)),
            ("Select lights", face::light_radial, |s| matches!(s, Shape::Light(_))),
        ];
        ui.spacing_mut().item_spacing.x = 3.0;
        for (tip, draw, pick) in kinds {
            if kit::icon_button(ui, 30.0, tip, false, draw).clicked() {
                self.doc.select_where(|n| pick(&n.shape));
            }
        }
        ui.add_space(8.0);
        if kit::icon_button(ui, 30.0, "Previous object", false, face::to_start).clicked() {
            self.doc.select_step(false);
        }
        if kit::icon_button(ui, 30.0, "Next object", false, face::to_end).clicked() {
            self.doc.select_step(true);
        }
        ui.add_space(8.0);
        let fam = ui.button("Families").on_hover_text("Select by family, or put the selection in a family");
        kit::menu_below(ui, &fam, false, |ui| {
            for k in 0..kit::FAMILIES.len() {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                    ui.painter().rect_filled(r, 2.0, kit::FAMILIES[k]);
                    let name = self.doc.project.family_names.get(k).cloned().unwrap_or_default();
                    if ui.button(format!("Select {name}")).clicked() {
                        self.doc.select_where(|n| n.family as usize == k);
                    }
                    if ui.add_enabled(!self.doc.selection.is_empty(), egui::Button::new("Assign")).clicked() {
                        self.doc.checkpoint();
                        let ids: Vec<u64> = self.doc.selection.iter().flat_map(|s| self.doc.project.with_descendants(*s)).collect();
                        for id in ids {
                            if let Some(n) = self.doc.project.node_mut(id) {
                                n.family = k as u8;
                            }
                        }
                        self.doc.touch();
                    }
                });
            }
        });
        if ui.button("All").clicked() {
            self.doc.select_all();
        }
        if ui.button("None").clicked() {
            self.doc.clear_selection();
        }
        ui.add_space(8.0);
        let text = match self.doc.selection.len() {
            0 => format!("{} objects", self.doc.project.nodes.len()),
            1 => self.doc.one().map(|n| format!("{} ({})", n.name, n.shape.label())).unwrap_or_default(),
            k => format!("{k} objects selected"),
        };
        ui.label(RichText::new(text).color(kit::DIM));
    }

    // ----------------------------------------------------------------- menus and keys

    fn menus(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button(kit::keys("New Scene                ⌘N")).clicked() {
                    self.replace_project(create::new_project(), None);
                    ui.close_menu();
                }
                if ui.button(kit::keys("Open…                    ⌘O")).clicked() {
                    fileio::open_dialog(self);
                    ui.close_menu();
                }
                if ui.button("Merge…").on_hover_text("Add another scene's objects to this one").clicked() {
                    fileio::merge_dialog(self);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(kit::keys("Save                     ⌘S")).clicked() {
                    fileio::save(self, false);
                    ui.close_menu();
                }
                if ui.button(kit::keys("Save As…                ⇧⌘S")).clicked() {
                    fileio::save(self, true);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Import 3D Model…").clicked() {
                    fileio::import_model(self);
                    ui.close_menu();
                }
                if ui.add_enabled(self.main.finished().is_some(), egui::Button::new("Save Picture As…")).clicked() {
                    fileio::save_picture(self);
                    ui.close_menu();
                }
                if ui.button("Render Animation…").clicked() {
                    self.dlg.movie = true;
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Document Setup…").clicked() {
                    self.dlg.document = true;
                    ui.close_menu();
                }
                if ui.button("Set Up From Disc…").clicked() {
                    self.run_setup();
                    ui.close_menu();
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.add_enabled(self.doc.can_undo(), egui::Button::new(kit::keys("Undo                     ⌘Z"))).clicked() {
                    self.doc.undo();
                    self.rebase();
                    ui.close_menu();
                }
                if ui.add_enabled(self.doc.can_redo(), egui::Button::new(kit::keys("Redo                    ⇧⌘Z"))).clicked() {
                    self.doc.redo();
                    self.rebase();
                    ui.close_menu();
                }
                ui.separator();
                for (label, act) in [("Cut                      ⌘X", 0), ("Copy                     ⌘C", 1), ("Paste                    ⌘V", 2), ("Duplicate                ⌘D", 3), ("Delete", 4)] {
                    if ui.button(kit::keys(label)).clicked() {
                        match act {
                            0 => self.doc.cut(),
                            1 => self.doc.copy(),
                            2 => self.doc.paste(),
                            3 => self.doc.duplicate_selection(),
                            _ => self.doc.delete_selection(),
                        }
                        ui.close_menu();
                    }
                }
                if ui.button("Replicate…").clicked() {
                    self.dlg.replicate = true;
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Copy Material").clicked() {
                    matlab::copy_material(self);
                    ui.close_menu();
                }
                if ui.add_enabled(self.doc.material_clipboard.is_some(), egui::Button::new("Paste Material")).clicked() {
                    matlab::paste_material(self);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(kit::keys("Select All                ⌘A")).clicked() {
                    self.doc.select_all();
                    ui.close_menu();
                }
                if ui.button("Select None").clicked() {
                    self.doc.clear_selection();
                    ui.close_menu();
                }
            });
            ui.menu_button("Objects", |ui| {
                if ui.button(kit::keys("Attributes…              ⌘E")).clicked() {
                    self.dlg.attributes = true;
                    ui.close_menu();
                }
                if ui.button("Edit Object…").clicked() {
                    self.edit_object();
                    ui.close_menu();
                }
                if ui.button(kit::keys("Edit Material…           ⌘M")).clicked() {
                    matlab::open(self);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(kit::keys("Group                    ⌘G")).clicked() {
                    self.doc.group_selection();
                    ui.close_menu();
                }
                if ui.button(kit::keys("Ungroup                  ⌘U")).clicked() {
                    self.doc.ungroup_selection();
                    ui.close_menu();
                }
                if ui.button("Land").clicked() {
                    self.doc.land_selection();
                    ui.close_menu();
                }
                ui.separator();
                for (label, hide) in [("Hide Selection", true), ("Show Everything", false)] {
                    if ui.button(label).clicked() {
                        self.doc.checkpoint();
                        if hide {
                            for id in std::mem::take(&mut self.doc.selection) {
                                for d in self.doc.project.with_descendants(id) {
                                    if let Some(n) = self.doc.project.node_mut(d) {
                                        n.hidden = true;
                                    }
                                }
                            }
                        } else {
                            self.doc.project.nodes.iter_mut().for_each(|n| n.hidden = false);
                        }
                        self.doc.touch();
                        ui.close_menu();
                    }
                }
                ui.separator();
                if ui.button("Object Library…").clicked() {
                    self.lib.open = Some(library::Kind::Objects);
                    ui.close_menu();
                }
                if ui.button("Material Library…").clicked() {
                    self.lib.open = Some(library::Kind::Materials);
                    ui.close_menu();
                }
                if ui.button("Sky Library…").clicked() {
                    self.lib.open = Some(library::Kind::Skies);
                    ui.close_menu();
                }
            });
            ui.menu_button("Animation", |ui| {
                if ui.button("Animation Setup…").clicked() {
                    self.dlg.animation = true;
                    ui.close_menu();
                }
                if ui.button("Advanced Motion Lab…").clicked() {
                    self.lab = Lab::Motion;
                    ui.close_menu();
                }
                if ui.button("Add Keyframe").clicked() {
                    anim::add_key(&mut self.doc);
                    self.doc.touch();
                    self.rebase();
                    ui.close_menu();
                }
                if ui.button("Remove Keyframe").clicked() {
                    anim::remove_key(&mut self.doc);
                    self.doc.touch();
                    self.rebase();
                    ui.close_menu();
                }
                if ui.button("Remove All Animation").clicked() {
                    self.doc.checkpoint();
                    anim::clear(&mut self.doc.project);
                    self.doc.touch();
                    self.rebase();
                    ui.close_menu();
                }
                if ui.button("Render Animation…").clicked() {
                    self.dlg.movie = true;
                    ui.close_menu();
                }
            });
            ui.menu_button("Render", |ui| {
                if ui.button(kit::keys("Render                   ⌘R")).clicked() {
                    self.start_render(ctx);
                    ui.close_menu();
                }
                if ui.button("Stop").clicked() {
                    self.stop_render();
                    ui.close_menu();
                }
                if ui.button("Render Options…").clicked() {
                    self.dlg.render_options = true;
                    ui.close_menu();
                }
                ui.separator();
                ui.checkbox(&mut self.prefs.stay_rendered, "Stay Rendered While Editing").on_hover_text("The picture stays up when you move things, and is drawn again after each change");
                ui.checkbox(&mut self.prefs.snap.ground, "Snap to the Ground");
                ui.checkbox(&mut self.prefs.snap.objects, "Snap to Other Objects");
                ui.checkbox(&mut self.prefs.snap.grid, "Snap to a Grid");
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Check for Updates…").clicked() {
                    self.update_asked = true;
                    self.update_hidden = false;
                    self.prefs.last_update_check = crate::update::now_secs();
                    crate::update::check(&self.update, ctx.clone());
                    ui.close_menu();
                }
                ui.checkbox(&mut self.prefs.check_updates, "Look for updates at start").on_hover_text("Asks GitHub for the latest release when the app opens. Nothing about you is sent.");
                if ui.button("Project page").clicked() {
                    crate::update::open_page(&format!("https://github.com/{}", crate::update::REPO));
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Keys and Mouse").clicked() {
                    self.dlg.help = true;
                    ui.close_menu();
                }
                if ui.button("About").clicked() {
                    self.dlg.about = true;
                    ui.close_menu();
                }
            });
        });
    }

    pub fn run_setup(&mut self) {
        let Some(folder) = rfd::FileDialog::new().set_title("Choose your disc or install folder").pick_folder() else { return };
        match setup::set_up_from(&folder) {
            Ok(_) => {
                self.ready = setup::ready();
                self.say("Set up from your disc.");
            }
            Err(e) => self.warn(format!("Setup did not finish. {e}")),
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() || self.lab != Lab::None {
            return;
        }
        let (cmd, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        if cmd {
            if pressed(Key::Z) {
                if shift { self.doc.redo() } else { self.doc.undo() }
                self.rebase();
            }
            if pressed(Key::N) {
                self.replace_project(create::new_project(), None);
            }
            if pressed(Key::O) {
                fileio::open_dialog(self);
            }
            if pressed(Key::S) {
                fileio::save(self, shift);
            }
            if pressed(Key::R) {
                self.start_render(ctx);
            }
            if pressed(Key::A) {
                self.doc.select_all();
            }
            if pressed(Key::D) {
                self.doc.duplicate_selection();
            }
            if pressed(Key::G) {
                self.doc.group_selection();
            }
            if pressed(Key::U) {
                self.doc.ungroup_selection();
            }
            if pressed(Key::E) {
                self.dlg.attributes = true;
            }
            if pressed(Key::M) {
                matlab::open(self);
            }
            if pressed(Key::C) {
                self.doc.copy();
            }
            if pressed(Key::X) {
                self.doc.cut();
            }
            if pressed(Key::V) {
                self.doc.paste();
            }
            return;
        }
        if pressed(Key::Delete) || pressed(Key::Backspace) {
            self.doc.delete_selection();
        }
        if pressed(Key::Escape) {
            if self.rendering() { self.stop_render() } else { self.show_picture = false }
        }
        if pressed(Key::Tab) {
            self.doc.select_step(!shift);
        }
        // arrow keys nudge the selection; page up / down move it up and down
        let step = if shift { 0.5 } else { 0.1 };
        let mut by = [0.0; 3];
        for (k, v) in [(Key::ArrowLeft, [-1.0, 0.0, 0.0]), (Key::ArrowRight, [1.0, 0.0, 0.0]), (Key::ArrowUp, [0.0, 0.0, 1.0]), (Key::ArrowDown, [0.0, 0.0, -1.0]), (Key::PageUp, [0.0, 1.0, 0.0]), (Key::PageDown, [0.0, -1.0, 0.0])] {
            if pressed(k) {
                by = crate::math::add(by, crate::math::mul(v, step));
            }
        }
        if by != [0.0; 3] && !self.doc.selection.is_empty() {
            self.doc.checkpoint();
            self.doc.move_selection(by);
            self.doc.refit_groups();
            self.doc.touch();
        }
    }

    fn working_window(&mut self, ui: &mut egui::Ui) {
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 0.0, kit::PANEL);
        if !self.ready {
            let card = Rect::from_center_size(area.center(), Vec2::new(480.0, 190.0));
            ui.painter().rect_filled(card, 10.0, kit::PANEL);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(card.shrink(20.0)));
            child.label(RichText::new("Set up from your disc").font(FontId::proportional(19.0)).strong());
            child.label("Rendering needs a few data tables that only your original disc holds. They are read once, on this Mac, and never leave it. You can build scenes before doing this, but not render them.");
            child.add_space(8.0);
            if child.add(egui::Button::new(RichText::new("Choose disc or install folder…").color(kit::INK).strong()).fill(kit::ACCENT)).clicked() {
                self.run_setup();
            }
            if child.small_button("Not now").clicked() {
                self.ready = true;
                self.nano_auto = false;
            }
            return;
        }
        let (w, h) = (self.doc.project.size[0] as f32, self.doc.project.size[1] as f32);
        let room = area.shrink(8.0);
        let scale = (room.width() / w).min(room.height() / h).max(0.02);
        let frame = Rect::from_center_size(room.center(), Vec2::new(w * scale, h * scale));
        ui.painter().rect_filled(frame.translate(Vec2::new(3.0, 4.0)), 0.0, Color32::from_black_alpha(70));
        let (under, picture) = if self.show_picture { (self.main.under.clone(), self.main.texture.clone()) } else { (None, None) };
        // with "stay rendered" the picture stays up and only what is selected shows as wires
        let stay = self.prefs.stay_rendered;
        let wire = if !self.show_picture { 1 } else if stay { 2 } else if self.rendering() { 1 } else { 0 };
        let before = self.doc.revision;
        self.view.ui(ui, frame, &mut self.doc, under.as_ref(), picture.as_ref(), wire, self.prefs.snap);
        if self.doc.revision != before && self.show_picture && !self.rendering() && !stay {
            self.show_picture = false;
        }
        // a refused render says why
        let failed = self.main.job.as_ref().and_then(|j| match &j.lock().unwrap().state {
            job::State::Failed(r) => Some(r.clone()),
            _ => None,
        });
        if let (Some(reason), true) = (failed, self.show_picture) {
            let card = Rect::from_center_size(frame.center(), Vec2::new(frame.width().min(540.0) - 24.0, 130.0));
            ui.painter().rect_filled(card, 8.0, Color32::from_rgba_unmultiplied(0xf0, 0xf0, 0xf0, 246));
            ui.painter().rect_stroke(card, 8.0, Stroke::new(1.0, kit::ACCENT), egui::StrokeKind::Outside);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(card.shrink(14.0)));
            child.label(RichText::new("This scene can't be rendered yet").strong().color(kit::ACCENT));
            child.label(RichText::new("The renderer stops rather than guess when it meets something it cannot draw exactly.").small());
            child.label(RichText::new(reason).small().color(kit::DIM));
        }
    }
}

impl eframe::App for App {
    /// The backdrop behind the panels is the same grey as the panels.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let c = kit::PANEL;
        [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, 1.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if let Some(p) = dropped.into_iter().next() {
            fileio::open_any(self, &p);
        }
        if self.dlg.auto_render {
            self.dlg.auto_render = false;
            self.start_render(ctx);
        }
        self.keys(ctx);
        if let Some(at) = self.test_nudge {
            if Instant::now() >= at {
                self.test_nudge = None;
                self.doc.checkpoint();
                self.doc.move_selection([0.0, 2.5, 0.0]);
                self.doc.touch();
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
        anim_ui::tick(self, ctx);
        self.main.refresh(ctx, "picture");
        self.nano.refresh(ctx, "nano");

        let rev = self.doc.revision;
        egui::TopBottomPanel::top("menus").show_separator_line(false).frame(egui::Frame::new().fill(kit::PANEL).inner_margin(egui::Margin::symmetric(8, 2))).show(ctx, |ui| self.menus(ui, ctx));
        if self.lab == Lab::None {
            egui::TopBottomPanel::top("palettes").show_separator_line(false).frame(egui::Frame::new().fill(kit::PANEL).inner_margin(egui::Margin::symmetric(12, 4))).show(ctx, |ui| self.top(ui));
            egui::TopBottomPanel::bottom("strip").show_separator_line(false).frame(egui::Frame::new().fill(kit::PANEL).inner_margin(egui::Margin::symmetric(12, 7))).show(ctx, |ui| self.bottom(ui, ctx));
        }
        // a newer version: tell the user once on the desktop, and keep a line in the window
        let status = self.update.lock().unwrap().clone();
        match status {
            crate::update::Status::Newer(r) => {
                if self.prefs.told_version != r.version {
                    self.prefs.told_version = r.version.clone();
                    crate::update::notify("Ridgeline update", &format!("Version {} is out. Open Ridgeline to get it.", r.version));
                }
                if !self.update_hidden {
                    egui::TopBottomPanel::bottom("update").show_separator_line(false).frame(egui::Frame::new().fill(kit::ACCENT_DEEP).inner_margin(egui::Margin::symmetric(12, 6))).show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("A newer Ridgeline is out: {} (you have {}).", r.version, crate::update::current())).strong());
                            if ui.button("Get it").on_hover_text("Opens the download page in your browser").clicked() {
                                crate::update::open_page(&r.url);
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("Later").clicked() {
                                    self.update_hidden = true;
                                }
                            });
                        });
                    });
                }
            }
            crate::update::Status::UpToDate if self.update_asked => {
                self.update_asked = false;
                self.say(format!("You have the latest version ({}).", crate::update::current()));
            }
            crate::update::Status::Failed(e) if self.update_asked => {
                self.update_asked = false;
                self.warn(format!("Could not look for updates: {e}."));
            }
            _ => {}
        }
        if let Some((bad, text)) = self.message.clone() {
            egui::TopBottomPanel::bottom("message").frame(egui::Frame::new().fill(kit::RAISED).inner_margin(egui::Margin::symmetric(12, 6))).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(text).color(if bad { kit::ACCENT } else { kit::TEXT }));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("Dismiss").clicked() {
                            self.message = None;
                        }
                    });
                });
            });
        }
        match self.lab {
            Lab::None => {
                egui::SidePanel::left("controls").show_separator_line(false).exact_width(144.0).resizable(false).frame(egui::Frame::new().fill(kit::PANEL).inner_margin(10.0)).show(ctx, |ui| self.left(ui, ctx));
                egui::SidePanel::right("display").show_separator_line(false).exact_width(40.0).resizable(false).frame(egui::Frame::new().fill(kit::PANEL).inner_margin(10.0)).show(ctx, |ui| self.right(ui));
                egui::CentralPanel::default().frame(egui::Frame::new().fill(kit::PANEL)).show(ctx, |ui| self.working_window(ui));
            }
            Lab::Materials => matlab::show(self, ctx),
            Lab::Terrain => terrain_ui::show(self, ctx),
            Lab::Motion => anim_ui::motion_lab(self, ctx),
        }
        dialogs::show(self, ctx);
        library::show(self, ctx);

        // record keys for whatever changed, then let the preview catch up
        if self.doc.revision != self.key_base.0 {
            let base = std::mem::take(&mut self.key_base.1);
            anim::record(&mut self.doc, &base);
            self.rebase();
        }
        if self.doc.revision != rev {
            self.last_change = Instant::now();
        }
        // "stay rendered": once a change has settled, draw the picture again
        if self.prefs.stay_rendered && self.show_picture && self.ready && self.lab == Lab::None && self.playing.is_none() && self.doc.revision != self.rendered_rev {
            let busy_hands = ctx.input(|i| i.pointer.any_down());
            if !busy_hands && self.last_change.elapsed().as_millis() >= 180 {
                self.render_again(ctx, true);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(60));
            }
        }
        self.nano_tick(ctx);
        if self.prefs != self.prefs_saved {
            self.prefs.save();
            self.prefs_saved = self.prefs.clone();
        }
        let _ = self.quit_ok;
    }
}
