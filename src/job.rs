//! Renders running on their own threads. A render takes the scene as the
//! renderer's description text, so it never touches the document being edited.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use stecyk3d::cases::picture;
use stecyk3d::description;
use stecyk3d::render::stages::Picture;
use stecyk3d::render::{OpenChoices, SharedDraws, render_lit_streamed};

pub enum State {
    Running,
    Done { seconds: f32 },
    Failed(String),
    Stopped,
}

pub struct Shared {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    pub drawn: Vec<bool>,
    pub pixels_done: usize,
    pub stamp: u64,
    pub state: State,
}

pub type Handle = Arc<Mutex<Shared>>;

/// Only one render draws at a time (the renderer's stop flag is shared), so a
/// new one waits here for the one before it to wind down.
static BUSY: Mutex<()> = Mutex::new(());
static STOP_ALL: AtomicBool = AtomicBool::new(false);

/// Ask every render going on to stop.
pub fn stop() {
    STOP_ALL.store(true, Ordering::SeqCst);
    stecyk3d::render::parallel::set_cancel(true);
}

pub fn is_running(h: &Option<Handle>) -> bool {
    h.as_ref().is_some_and(|j| matches!(j.lock().unwrap().state, State::Running))
}

pub fn is_done(h: &Option<Handle>) -> bool {
    h.as_ref().is_some_and(|j| matches!(j.lock().unwrap().state, State::Done { .. }))
}

/// Add what the sky needs beyond the tables: the moon's picture, kept beside
/// the data taken from the disc.
fn with_sky_files(mut c: OpenChoices, substrate: &stecyk3d::substrate::Substrate, d: &description::Described) -> OpenChoices {
    if d.scene.sky.flags & 0x20000 != 0 {
        c.moon_picture = Some(match substrate.read_moon_picture(d.version) {
            Ok(m) => m.image().map(Arc::new).map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        });
    }
    c
}

/// Render a description to a finished picture on the calling thread.
pub fn render_now(description_text: &str, substrate: &std::path::Path, threads: usize) -> Result<Picture, String> {
    // renders take turns; a stop asked of an earlier render does not reach this one
    let _turn = BUSY.lock().unwrap_or_else(|e| e.into_inner());
    STOP_ALL.store(false, Ordering::SeqCst);
    stecyk3d::render::parallel::set_cancel(false);
    let substrate = stecyk3d::substrate::load(substrate).map_err(|e| format!("The data taken from your disc cannot be read: {e}"))?;
    let described = description::describe_with(description_text, &description::Tables::of(&substrate)).map_err(|e| e.to_string())?;
    let choices = described.choices(OpenChoices {
        threads: Some(threads),
        shared_draws: SharedDraws::Auto(100_000_000),
        ..picture::choices_from(&substrate)
    });
    let choices = with_sky_files(choices, &substrate, &described);
    let mut nothing = |_: u32, _: u32, _: &[u8]| {};
    let (p, _) = render_lit_streamed(&described.scene, described.version, &choices, &mut nothing).map_err(|e| e.to_string())?;
    Ok(p)
}

/// Start rendering `description_text` (the renderer's JSON) at `size`.
pub fn start(description_text: String, substrate: PathBuf, size: (u32, u32), ctx: eframe::egui::Context) -> Handle {
    start_mode(description_text, substrate, size, ctx, String::new())
}

/// As `start`; `mode` may be "distance" or "altitude" for a grey picture of
/// depth or height instead of the lit picture.
pub fn start_mode(description_text: String, substrate: PathBuf, size: (u32, u32), ctx: eframe::egui::Context, mode: String) -> Handle {
    let (w, h) = size;
    let shared = Arc::new(Mutex::new(Shared {
        width: w,
        height: h,
        rgb: vec![0; (w * h * 3) as usize],
        drawn: vec![false; (w * h) as usize],
        pixels_done: 0,
        stamp: 0,
        state: State::Running,
    }));
    let out = shared.clone();
    std::thread::Builder::new()
        .name("render".into())
        .stack_size(64 << 20)
        .spawn(move || {
            // wait for the render before this one to finish stopping
            let _turn = BUSY.lock().unwrap_or_else(|e| e.into_inner());
            STOP_ALL.store(false, Ordering::SeqCst);
            stecyk3d::render::parallel::set_cancel(false);
            let began = Instant::now();
            let result = (|| -> Result<Picture, String> {
                let substrate = stecyk3d::substrate::load(&substrate).map_err(|e| format!("The data taken from your disc cannot be read: {e}"))?;
                let described = description::describe_with(&description_text, &description::Tables::of(&substrate)).map_err(|e| e.to_string())?;
                let choices = described.choices(OpenChoices {
                    scattered: None,
                    scattered_rows: true,
                    threads: Some(0),
                    shared_draws: SharedDraws::Auto(100_000_000),
                    ..picture::choices_from(&substrate)
                });
                let choices = with_sky_files(choices, &substrate, &described);
                let mut last_paint = Instant::now();
                let mut emit = |x: u32, y: u32, pixels: &[u8]| {
                    let mut s = out.lock().unwrap();
                    let (w, h) = (s.width as usize, s.height as usize);
                    let n = pixels.len() / 3;
                    let start = y as usize * w + x as usize;
                    if y as usize >= h || start + n > w * h {
                        return;
                    }
                    s.rgb[start * 3..(start + n) * 3].copy_from_slice(&pixels[..n * 3]);
                    let mut fresh = 0;
                    for d in &mut s.drawn[start..start + n] {
                        if !*d {
                            *d = true;
                            fresh += 1;
                        }
                    }
                    s.pixels_done = (s.pixels_done + fresh).min(w * h);
                    s.stamp += 1;
                    drop(s);
                    if last_paint.elapsed().as_millis() > 60 {
                        last_paint = Instant::now();
                        ctx.request_repaint();
                    }
                };
                if let Some(m) = stecyk3d::render::RenderMode::parse(&mode) {
                    let (picture, _) = stecyk3d::render::render_mode(&described.scene, described.version, &choices, m).map_err(|e| e.to_string())?;
                    return Ok(picture);
                }
                let (picture, _) = render_lit_streamed(&described.scene, described.version, &choices, &mut emit).map_err(|e| e.to_string())?;
                Ok(picture)
            })();
            let mut s = out.lock().unwrap();
            match result {
                Ok(p) => {
                    s.width = p.width;
                    s.height = p.height;
                    s.drawn = vec![true; (p.width * p.height) as usize];
                    s.pixels_done = s.drawn.len();
                    s.rgb = p.levels;
                    s.state = State::Done { seconds: began.elapsed().as_secs_f32() };
                }
                Err(_) if STOP_ALL.load(Ordering::SeqCst) || stecyk3d::render::parallel::cancelled() => s.state = State::Stopped,
                Err(e) => s.state = State::Failed(e),
            }
            s.stamp += 1;
            drop(s);
            stecyk3d::render::parallel::set_cancel(false);
            ctx.request_repaint();
        })
        .expect("the render thread starts");
    shared
}
