#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! Kayzee 3D: a native Mac landscape modeller and renderer. It builds scenes
//! from primitives, terrains, lights and imported models, opens .br3/.br5
//! scene files, and renders with the stecyk3d renderer. The fixed data the
//! renderer needs is read from the user's own disc; none ships with the app.

mod anim;
mod anim_ui;
mod app;
mod create;
mod dialogs;
mod emit;
mod fileio;
mod import;
mod iso;
mod job;
mod kit;
mod library;
mod math;
mod matlab;
mod mesh_io;
mod model;
mod scenes;
mod movie;
mod setup;
mod sky_ui;
mod state;
mod terrain_ops;
mod update;
mod terrain_ui;
mod viewport;
mod wire;

use std::path::{Path, PathBuf};

use eframe::egui;

/// Command-line checks used while building the app (no window).
fn tool(args: &[String]) -> Option<Result<(), String>> {
    match args.first().map(String::as_str)? {
        "--setup" if args.len() == 2 => Some(setup::set_up_from_any(Path::new(&args[1])).map(|l| l.iter().for_each(|x| println!("{x}")))),
        "--mesh-info" => {
            for f in &args[1..] {
                match mesh_io::load(Path::new(f)) {
                    Ok(m) => {
                        let all = mesh_io::merged(&m, "all");
                        println!("{f}: {} meshes, {} points, {} triangles", m.len(), all.points.len(), all.triangles.len());
                    }
                    Err(e) => println!("{f}: REFUSED {e}"),
                }
            }
            Some(Ok(()))
        }
        // --render-file SCENE OUT.png : open any scene the app reads and render it
        "--render-file" if args.len() == 3 => Some((|| {
            let p = fileio::open(Path::new(&args[1]))?;
            let text = emit::description(&p, (p.size[0], p.size[1]), p.render.antialias);
            let pic = job::render_now(&text, &setup::substrate_path(), 0)?;
            std::fs::write(&args[2], pic.png()).map_err(|e| e.to_string())?;
            println!("{} objects, levels {}", p.nodes.len(), pic.levels_sha256_hex());
            Ok(())
        })()),
        // --render-at SCENE SECONDS OUT.png : pose an animated scene at a time and render it
        "--render-at" if args.len() == 4 => Some((|| {
            let mut p = fileio::open(Path::new(&args[1]))?;
            let t: f64 = args[2].parse().map_err(|_| "time?".to_owned())?;
            let moving = p.nodes.iter().filter(|n| n.keys.len() > 1).count();
            anim::pose(&mut p, t);
            let text = emit::description(&p, (p.size[0], p.size[1]), p.render.antialias);
            let pic = job::render_now(&text, &setup::substrate_path(), 0)?;
            std::fs::write(&args[3], pic.png()).map_err(|e| e.to_string())?;
            println!("{} moving objects, camera keys {}, {} s at {} fps, t={t}: levels {}", moving, p.anim.camera_keys.len(), p.anim.duration, p.anim.fps, &pic.levels_sha256_hex()[..16]);
            Ok(())
        })()),
        // --describe SCENE OUT.json : write the description the app would render
        "--describe" if args.len() == 3 => Some((|| {
            let p = fileio::open(Path::new(&args[1]))?;
            std::fs::write(&args[2], emit::description(&p, (p.size[0], p.size[1]), p.render.antialias)).map_err(|e| e.to_string())
        })()),
        "--checks" if args.len() == 2 => Some(checks(Path::new(&args[1]))),
        // --scene dream OUT.ridge [SKY_SCENE] : write an example scene (cloud layers borrowed from SKY_SCENE)
        "--scene" if args.len() >= 3 && args[1] == "dream" => Some(fileio::write(&scenes::dream(args.get(3).map(Path::new)), Path::new(&args[2]))),
        // --demo OUT.ridge : write the small scene used for the project's pictures
        "--demo" if args.len() == 2 => Some(fileio::write(&demo(), Path::new(&args[1]))),
        // --update-check : say what the release page answers
        "--update-check" => {
            println!("this build: {}", update::current());
            match update::latest() {
                Ok(r) => println!("latest release: {} ({}) newer: {} -> {}", r.version, r.title, update::newer(&r.version, &update::current()), r.url),
                Err(e) => println!("no answer: {e}"),
            }
            if args.iter().any(|a| a == "--notify") {
                update::notify("Kayzee 3D", "This is what an update notice looks like.");
            }
            Some(Ok(()))
        }
        "--snap-check" => {
            viewport::snap_check().iter().for_each(|l| println!("{l}"));
            Some(Ok(()))
        }
        // --selftest OUT_FOLDER : build a scene with every kind of object, save, reopen, render
        "--selftest" if args.len() == 2 => Some(selftest(Path::new(&args[1]))),
        _ => None,
    }
}

fn selftest(out: &Path) -> Result<(), String> {
    use create::Make::*;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut doc = state::Doc::new(create::new_project());
    let mut seed = 42u64;
    let all = [Sphere, Ellipsoid, Egg, Torus, Cylinder, Puck, Rod, Cube, Brick, Tower, Pyramid, FlatPyramid, Obelisk, Cone, FlatCone, Spike, Disc, Square, Stone, Terrain, Lattice, Radial, Spot, SquareSpot, Parallel, Water];
    for (k, make) in all.iter().enumerate() {
        create::add(&mut doc, *make, &mut seed);
        let (x, z) = ((k % 7) as f64 * 3.2 - 9.6, (k / 7) as f64 * 3.6);
        doc.move_selection([x, 0.0, z]);
    }
    // a cut: a cube with a ball taken out
    let a = create::add(&mut doc, Cube, &mut seed);
    doc.move_selection([-13.0, 0.0, 0.0]);
    let b = create::add(&mut doc, Sphere, &mut seed);
    doc.move_selection([-12.4, 0.7, -0.6]);
    doc.selection = vec![a, b];
    doc.group_selection();
    doc.project.node_mut(a).unwrap().role = model::Role::Positive;
    doc.project.node_mut(b).unwrap().role = model::Role::Negative;
    doc.project.cam.eye = [4.0, 13.0, -22.0];
    doc.project.cam.look_at([0.0, 0.5, 5.0]);
    doc.project.size = [800, 500];
    // keys: the first sphere rises over one second
    doc.selection = vec![doc.project.nodes[1].id];
    doc.project.anim.time = 1.0;
    let before = doc.project.clone();
    doc.move_selection([0.0, 3.0, 0.0]);
    anim::record(&mut doc, &before);

    let file = out.join(format!("selftest.{}", fileio::EXT));
    fileio::write(&doc.project, &file)?;
    let back = fileio::open(&file)?;
    println!("saved and reopened: {} objects, {} materials", back.nodes.len(), back.materials.len());
    for (name, t) in [("t0", 0.0), ("t1", 1.0)] {
        let mut posed = back.clone();
        anim::pose(&mut posed, t);
        let text = emit::description(&posed, (800, 500), 1);
        std::fs::write(out.join(format!("selftest-{name}.json")), &text).map_err(|e| e.to_string())?;
        let pic = job::render_now(&text, &setup::substrate_path(), 0)?;
        std::fs::write(out.join(format!("selftest-{name}.png")), pic.png()).map_err(|e| e.to_string())?;
        println!("rendered {name}: {}", pic.levels_sha256_hex());
    }
    Ok(())
}

/// A small original scene: an island in still water under a low sun.
fn demo() -> model::Project {
    use create::Make::*;
    let mut doc = state::Doc::new(create::new_project());
    let mut seed = 20261121u64;
    let land = create::add(&mut doc, Terrain, &mut seed);
    if let Some(n) = doc.project.node_mut(land) {
        if let model::Shape::Terrain(t) = &mut n.shape {
            let mut g = terrain_ops::fractal(192, terrain_ops::Fractal::Ridges, 77, 8, 2.0);
            terrain_ops::edges(&mut g, 0.55, 0.0, true);
            terrain_ops::erode(&mut g, 1.2, 9);
            terrain_ops::smooth_grid(&mut g, 0.25);
            t.n = 192;
            t.heights = std::sync::Arc::new(g.to_u16());
        }
        let (w, h) = (22.0, 6.0);
        n.m = math::from_axes([w / 192.0, 0.0, 0.0], [0.0, h / 65535.0, 0.0], [0.0, 0.0, w / 192.0], [0.0, 32767.5 * h / 65535.0 - 0.02, 2.0]);
        n.name = "Island".into();
    }
    let rock = doc.project.nodes.iter().find(|n| n.id == land).and_then(|n| n.material).unwrap();
    doc.project.materials[rock].record = create::plain([0.56, 0.5, 0.43]);
    create::add(&mut doc, Water, &mut seed);
    for (x, z, s) in [(-4.2, -5.6, 0.7), (-2.6, -6.4, 0.45), (3.4, -6.0, 0.9), (5.0, -4.8, 0.5)] {
        create::add(&mut doc, Stone, &mut seed);
        doc.scale_own([s, s, s]);
        let c = doc.selection_centre().unwrap();
        doc.move_selection([x - c[0], 0.3 - c[1], z - c[2]]);
    }
    let ball = create::add(&mut doc, Sphere, &mut seed);
    doc.scale_own([0.8, 0.8, 0.8]);
    let c = doc.selection_centre().unwrap();
    doc.move_selection([0.6 - c[0], 1.05 - c[1], -7.2 - c[2]]);
    let m = doc.project.node(ball).and_then(|n| n.material).unwrap();
    doc.project.materials[m].record = serde_json::json!({"flags": 65536, "channels": {
        "diffuse_color": [0.9, 0.9, 0.95], "ambient_color": [1.0, 1.0, 1.0], "specular_color": [1.0, 1.0, 1.0], "specular_halo": [0.9, 0.9, 0.9],
        "diffusion": 0.25, "ambience": 0.1, "specularity": 1.0, "reflection": 0.8 }});
    doc.project.sky = sky_ui::presets().into_iter().find(|p| p.0 == "Golden Hour").map(|p| p.1).unwrap();
    doc.project.sky["sun"]["direction"] = serde_json::json!([-0.55, 0.3, 0.78]);
    doc.project.size = [960, 540];
    doc.project.cam.eye = [1.5, 1.7, -15.0];
    doc.project.cam.pivot = [0.0, 1.6, 0.0];
    doc.project.cam.look_at([0.0, 1.9, 2.0]);
    doc.project.cam.fov = 1.0;
    doc.project.name = "Island".into();
    doc.selection = vec![ball];
    doc.project
}

/// A ball on a floor wearing one material, as a description.
fn ball_scene(record: serde_json::Value, textures: Vec<serde_json::Value>) -> String {
    let mut p = create::new_project();
    p.size = [160, 120];
    p.textures = textures;
    let m = create::own_material(&mut p, "subject", record);
    p.add("ball", model::Shape::Sphere, math::translation([0.0, 1.0, 0.0]), Some(m));
    p.cam.eye = [2.5, 2.2, -4.0];
    p.cam.look_at([0.0, 0.9, 0.0]);
    emit::description(&p, (160, 120), 0)
}

/// Automated checks of the parts that are otherwise reached by clicking.
fn checks(out: &Path) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let sub = setup::substrate_path();
    // 1. every material preset on the disc
    let (mut ok, mut bad, mut reasons) = (0, 0, std::collections::BTreeMap::<String, usize>::new());
    let mut sample = 0;
    for f in library::files_with(&["mat"]) {
        let Ok((presets, textures)) = library::read_presets(&f) else {
            println!("unreadable: {}", f.display());
            continue;
        };
        for (name, record) in presets {
            match job::render_now(&ball_scene(record, textures.clone()), &sub, 0) {
                Ok(pic) => {
                    ok += 1;
                    if ok % 40 == 1 && sample < 6 {
                        sample += 1;
                        let _ = std::fs::write(out.join(format!("preset-{sample}-{}.png", name.replace(['/', ' '], "_"))), pic.png());
                    }
                }
                Err(e) => {
                    bad += 1;
                    *reasons.entry(e.chars().take(70).collect()).or_default() += 1;
                }
            }
        }
    }
    println!("material presets: {ok} render, {bad} refused");
    for (r, n) in &reasons {
        println!("   {n} x {r}");
    }
    // 2. every texture preset, feeding the diffuse colour
    let (mut ok, mut bad) = (0, 0);
    for f in library::files_with(&["brt"]) {
        let Ok((presets, _)) = library::read_presets(&f) else { continue };
        for (_, t) in presets {
            let name = t["name"].as_str().unwrap_or("").to_owned();
            let record = serde_json::json!({"flags": 65536, "channels": {
                "diffuse_color": {"a": [1.0, 1.0, 1.0], "b": [0.0, 0.0, 0.0], "alpha": 1.0, "switches": 0x10000000u32, "a_tex": name},
                "ambient_color": [1.0, 1.0, 1.0], "diffusion": 1.0, "ambience": 0.2}});
            if job::render_now(&ball_scene(record, vec![t]), &sub, 0).is_ok() { ok += 1 } else { bad += 1 }
        }
    }
    println!("texture presets: {ok} render, {bad} refused");
    // 3. terrain tools each change the grid
    {
        use terrain_ops as t;
        let base = t::fractal(64, t::Fractal::Standard, 3, 6, 2.0);
        let tools: Vec<(&str, Box<dyn Fn(&mut t::Grid)>)> = vec![
            ("erode", Box::new(|g| t::erode(g, 1.0, 5))), ("raise", Box::new(|g| t::raise(g, 0.05))), ("smooth", Box::new(|g| t::smooth_grid(g, 0.8))),
            ("sharpen", Box::new(|g| t::sharpen(g, 0.5))), ("spikes", Box::new(|g| t::spikes(g, 0.1, 5))), ("mounds", Box::new(|g| t::mounds(g, 0.2, 5))),
            ("pits", Box::new(|g| t::pits(g, 0.2, 5))), ("dampen", Box::new(|g| t::dampen(g, 0.3))), ("round edges", Box::new(|g| t::edges(g, 0.3, 0.0, true))),
            ("square edges", Box::new(|g| t::edges(g, 0.3, 0.0, false))), ("terraces", Box::new(|g| t::terrace(g, 6, 0.3))), ("equalize", Box::new(|g| t::equalize(g))),
            ("invert", Box::new(|g| t::invert(g))), ("zero edges", Box::new(|g| t::zero_edges(g))), ("flip", Box::new(|g| t::mirror(g, true))),
            ("turn", Box::new(|g| t::rotate90(g))), ("curve", Box::new(|g| t::curve(g, &[(0.0, 0.0), (0.5, 0.2), (1.0, 1.0)]))),
            ("brush", Box::new(|g| t::dab(g, t::Brush::Raise, 32.0, 32.0, 10.0, 1.0, 0.5, 0.5, 1))),
        ];
        let mut changed = 0;
        for (name, f) in &tools {
            let mut g = base.clone();
            f(&mut g);
            if g.h != base.h { changed += 1 } else { println!("   terrain tool did nothing: {name}") }
        }
        let fr = t::Fractal::ALL.iter().filter(|k| t::fractal(64, **k, 9, 6, 2.0).range().1 > 0.5).count();
        let big = t::resample(&base, 256);
        t::to_picture(&base, &out.join("heights.png"))?;
        let back = t::from_picture(&out.join("heights.png"), 64)?;
        let worst = back.h.iter().zip(&base.h).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        println!("terrain: {changed} of {} tools change the grid, {fr} of 6 fractal kinds, resample to {}, picture round trip error {worst:.4}", tools.len(), big.n);
    }
    // 4. movie: eight frames of an object rising
    {
        let mut doc = state::Doc::new(create::new_project());
        let mut seed = 5;
        create::add(&mut doc, create::Make::Sphere, &mut seed);
        doc.project.size = [320, 240];
        doc.project.anim.time = 0.5;
        let before = doc.project.clone();
        doc.move_selection([0.0, 2.0, 0.0]);
        anim::record(&mut doc, &before);
        doc.project.anim.range = [0.0, 0.5];
        let mut files = Vec::new();
        for (k, t) in anim::frame_times(&doc.project).iter().enumerate() {
            let mut posed = doc.project.clone();
            anim::pose(&mut posed, *t);
            let pic = job::render_now(&emit::description(&posed, (320, 240), 0), &sub, 0)?;
            let f = out.join(format!("frame-{k:03}.png"));
            std::fs::write(&f, pic.png()).map_err(|e| e.to_string())?;
            files.push(f);
        }
        let helper = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("kayzee3d-movie"))).filter(|h| h.exists());
        match helper {
            Some(h) => {
                let mov = out.join("check.mov");
                let done = std::process::Command::new(h).arg(&mov).arg("15").args(&files).output().map_err(|e| e.to_string())?;
                println!("movie: {} frames -> {} ({} bytes) {}", files.len(), mov.display(), std::fs::metadata(&mov).map(|m| m.len()).unwrap_or(0), String::from_utf8_lossy(&done.stderr).trim());
            }
            None => {
                let avi = out.join("check.avi");
                movie::write_avi(&avi, 15, &files)?;
                println!("movie: {} frames -> {} ({} bytes)", files.len(), avi.display(), std::fs::metadata(&avi).map(|m| m.len()).unwrap_or(0));
            }
        }
    }
    // 5. models and picture formats
    {
        let mut doc = state::Doc::new(create::new_project());
        let home = std::env::var("HOME").unwrap_or_default();
        // KAYZEE3D_TEST_MODELS names a folder of model files to try (none are part of this project)
        let folder = std::env::var("KAYZEE3D_TEST_MODELS").unwrap_or_default();
        let _ = &home;
        let found: Vec<PathBuf> = std::fs::read_dir(&folder).map(|d| d.flatten().map(|e| e.path()).filter(|p| fileio::is_model(p)).take(2).collect()).unwrap_or_default();
        for (k, path) in found.iter().enumerate() {
            let rel = if k == 0 { "a.obj" } else { "b" };
            match mesh_io::load(&path) {
                Ok(m) => {
                    let all = mesh_io::merged(&m, "model");
                    create::add_mesh(&mut doc, "model", &all);
                    doc.move_selection([if rel.ends_with("obj") { -2.0 } else { 2.0 }, 0.0, 0.0]);
                }
                Err(e) => println!("   model not read: {e}"),
            }
        }
        doc.project.size = [480, 360];
        doc.project.cam.eye = [0.0, 3.0, -8.0];
        doc.project.cam.look_at([0.0, 1.2, 0.0]);
        let pic = job::render_now(&emit::description(&doc.project, (480, 360), 1), &sub, 0)?;
        let mut kinds = Vec::new();
        for ext in ["png", "bmp", "tif", "jpg"] {
            let f = out.join(format!("models.{ext}"));
            fileio::write_picture(&f, pic.width, pic.height, &pic.levels)?;
            kinds.push(format!("{ext} {}", std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0)));
        }
        println!("models: {} objects rendered; pictures written: {}", doc.project.nodes.len() - 1, kinds.join(", "));
    }
    Ok(())
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(result) = tool(&args) {
        if let Err(e) = result {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let auto_render = args.iter().any(|a| a == "--render");
    let first = args.iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).find(|p| p.is_file());
    // How the window is drawn: Metal (through wgpu) on a Mac, plain OpenGL elsewhere.
    // --gl / --wgpu (or KAYZEE3D_RENDERER=gl / wgpu) choose the other way where the usual one fails.
    let asked = std::env::var("KAYZEE3D_RENDERER").unwrap_or_default().to_lowercase();
    let plain_gl = if args.iter().any(|a| a == "--wgpu") || asked == "wgpu" {
        false
    } else if args.iter().any(|a| a == "--gl") || asked == "gl" {
        true
    } else {
        !cfg!(target_os = "macos")
    };
    let options = eframe::NativeOptions {
        renderer: if plain_gl { eframe::Renderer::Glow } else { eframe::Renderer::Wgpu },
        viewport: egui::ViewportBuilder::default().with_title("Kayzee 3D").with_inner_size([1320.0, 860.0]).with_min_inner_size([980.0, 640.0]).with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("Kayzee 3D", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, first, auto_render)))))
}
