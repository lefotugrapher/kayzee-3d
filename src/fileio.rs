//! Opening and saving: the app's own scene files, old scene files, 3D models
//! and pictures.

use std::path::{Path, PathBuf};

use crate::app::App;
use crate::model::Project;
use crate::{create, import, mesh_io};

/// The app's own scene file extension.
pub const EXT: &str = "k3d";
/// The extension earlier versions saved with; still opened.
pub const OLD_EXT: &str = "ridge";

pub fn is_scene(path: &Path) -> bool {
    path.extension().is_some_and(|e| ["br3", "br5", EXT, OLD_EXT].iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// A scene this app saved (now or under its earlier extension).
pub fn is_own(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case(EXT) || e.eq_ignore_ascii_case(OLD_EXT))
}

pub fn is_model(path: &Path) -> bool {
    path.extension().is_some_and(|e| ["obj", "dxf", "3ds", "3dmf"].iter().any(|x| e.eq_ignore_ascii_case(x)))
}

pub fn open(path: &Path) -> Result<Project, String> {
    if is_own(path) {
        let text = std::fs::read_to_string(path).map_err(|e| format!("Cannot read it: {e}"))?;
        let mut p: Project = serde_json::from_str(&text).map_err(|e| format!("It is not a scene this app saved: {e}"))?;
        p.view_dots.resize(crate::model::DOTS, None);
        p.sky_dots.resize(crate::model::DOTS, None);
        return Ok(p);
    }
    import::open(path).map(|(p, _)| p)
}

pub fn write(project: &Project, path: &Path) -> Result<(), String> {
    let text = serde_json::to_string(project).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("The scene could not be saved: {e}"))
}

fn start_folder(app: &App) -> Option<PathBuf> {
    app.doc.file.as_ref().and_then(|f| f.parent().map(Path::to_path_buf)).or_else(|| std::fs::read_to_string(crate::setup::recent_file()).ok().map(PathBuf::from)).filter(|f| f.is_dir())
}

fn remember(path: &Path) {
    if let Some(folder) = path.parent() {
        let _ = std::fs::create_dir_all(crate::setup::data_folder());
        let _ = std::fs::write(crate::setup::recent_file(), folder.to_string_lossy().as_bytes());
    }
}

pub fn open_dialog(app: &mut App) {
    let mut d = rfd::FileDialog::new().add_filter("Scenes", &["br3", "br5", EXT, OLD_EXT, "BR3", "BR5"]);
    if let Some(f) = start_folder(app) {
        d = d.set_directory(f);
    }
    if let Some(p) = d.pick_file() {
        remember(&p);
        app.open_path(&p);
    }
}

/// Open whatever was dropped on the window.
pub fn open_any(app: &mut App, path: &Path) {
    if is_scene(path) {
        remember(path);
        app.open_path(path);
    } else if is_model(path) {
        add_model(app, path);
    } else {
        app.warn("That kind of file is not a scene or a 3D model this app reads.");
    }
}

/// Add another scene's objects (with their materials and textures) to this one.
pub fn merge_dialog(app: &mut App) {
    let mut d = rfd::FileDialog::new().add_filter("Scenes", &["br3", "br5", EXT, OLD_EXT, "BR3", "BR5"]);
    if let Some(f) = start_folder(app) {
        d = d.set_directory(f);
    }
    let Some(path) = d.pick_file() else { return };
    match open(&path) {
        Ok(other) => {
            app.doc.checkpoint();
            let ids = merge(&mut app.doc.project, other);
            app.doc.selection = ids;
            app.doc.touch();
            app.say("Merged. The added objects are selected.");
        }
        Err(e) => app.warn(format!("That scene could not be merged. {e}")),
    }
}

/// Bring every object of `other` into `p`; returns the new top-level ids.
pub fn merge(p: &mut Project, other: Project) -> Vec<u64> {
    let mat_base = p.materials.len();
    // textures are named; a clash gets a suffix, and the materials follow
    let mut renames: Vec<(String, String)> = Vec::new();
    for mut t in other.textures {
        let name = t["name"].as_str().unwrap_or("").to_owned();
        if p.textures.iter().any(|x| x["name"] == t["name"]) {
            let fresh = format!("{name}~{}", p.textures.len());
            t["name"] = serde_json::json!(fresh);
            renames.push((name, fresh));
        }
        p.textures.push(t);
    }
    for mut m in other.materials {
        if !renames.is_empty() {
            let mut text = m.record.to_string();
            for (old, new) in &renames {
                text = text.replace(&format!("\"{old}\""), &format!("\"{new}\""));
            }
            m.record = serde_json::from_str(&text).unwrap_or(m.record);
        }
        p.materials.push(m);
    }
    let mut map = std::collections::HashMap::new();
    for n in &other.nodes {
        map.insert(n.id, p.fresh_id());
    }
    let mut tops = Vec::new();
    for mut n in other.nodes {
        n.id = map[&n.id];
        n.group = n.group.and_then(|g| map.get(&g).copied());
        n.link = n.link.and_then(|g| map.get(&g).copied());
        n.track = n.track.and_then(|g| map.get(&g).copied());
        n.material = n.material.map(|m| m + mat_base);
        if n.group.is_none() {
            tops.push(n.id);
        }
        p.nodes.push(n);
    }
    tops
}

pub fn save(app: &mut App, ask: bool) {
    let path = match (&app.doc.file, ask) {
        (Some(f), false) => f.clone(),
        _ => {
            let mut d = rfd::FileDialog::new().add_filter("Scene", &[EXT]).set_file_name(format!("{}.{EXT}", app.doc.project.name));
            if let Some(f) = start_folder(app) {
                d = d.set_directory(f);
            }
            match d.save_file() {
                Some(p) => p,
                None => return,
            }
        }
    };
    app.doc.project.name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into());
    match write(&app.doc.project, &path) {
        Ok(()) => {
            remember(&path);
            app.doc.file = Some(path);
            app.doc.dirty = false;
            app.say("Saved.");
        }
        Err(e) => app.warn(e),
    }
}

pub fn import_model(app: &mut App) {
    let mut d = rfd::FileDialog::new().add_filter("3D models", &["obj", "dxf", "3ds", "3dmf", "OBJ", "DXF", "3DS", "3DMF"]);
    if let Some(f) = start_folder(app) {
        d = d.set_directory(f);
    }
    if let Some(p) = d.pick_file() {
        add_model(app, &p);
    }
}

pub fn add_model(app: &mut App, path: &Path) {
    match mesh_io::load(path) {
        Ok(meshes) => {
            let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Model".into());
            let all = mesh_io::merged(&meshes, &name);
            create::add_mesh(&mut app.doc, &name, &all);
            app.say(format!("Imported {} ({} triangles).", name, all.triangles.len()));
        }
        Err(e) => app.warn(e),
    }
}

/// Save a finished render: the file type follows the name's extension.
pub fn save_picture(app: &mut App) {
    let Some((w, h, rgb)) = app.main.finished() else { return };
    let name = format!("{}.png", app.doc.project.name);
    let d = rfd::FileDialog::new().set_file_name(name).add_filter("PNG", &["png"]).add_filter("BMP", &["bmp"]).add_filter("TIFF", &["tif", "tiff"]).add_filter("JPEG", &["jpg", "jpeg"]);
    let Some(path) = d.save_file() else { return };
    match write_picture(&path, w, h, &rgb) {
        Ok(()) => app.say(format!("Saved {}", path.display())),
        Err(e) => app.warn(e),
    }
}

pub fn write_picture(path: &Path, w: u32, h: u32, rgb: &[u8]) -> Result<(), String> {
    let img = image::RgbImage::from_raw(w, h, rgb.to_vec()).ok_or("The picture's size is wrong.")?;
    img.save(path).map_err(|e| format!("The picture could not be saved: {e}"))
}
