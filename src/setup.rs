//! First-run setup: the renderer needs fixed data that only an original Bryce
//! disc holds. Nothing of it ships with this app; it is read once from the
//! user's own disc (or install folder) and kept in Application Support.

use std::fs::File;
use std::path::{Path, PathBuf};

pub fn data_folder() -> PathBuf {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        var("APPDATA").or_else(|| var("USERPROFILE")).unwrap_or_default()
    } else if cfg!(target_os = "macos") {
        var("HOME").unwrap_or_default().join("Library").join("Application Support")
    } else {
        var("XDG_DATA_HOME").unwrap_or_else(|| var("HOME").unwrap_or_default().join(".local").join("share"))
    };
    let folder = base.join("Kayzee 3D");
    // the app had another name before: its folder is carried over the first time
    static CARRY: std::sync::Once = std::sync::Once::new();
    CARRY.call_once(|| {
        let earlier = base.join(["Ridge", "line"].concat());
        if earlier.is_dir() && !folder.exists() {
            let _ = std::fs::rename(&earlier, &folder);
        }
    });
    folder
}

pub fn substrate_path() -> PathBuf {
    data_folder().join("substrate.bin")
}

/// Where the app keeps the last folder a scene was opened from.
pub fn recent_file() -> PathBuf {
    data_folder().join("last-folder.txt")
}

pub fn ready() -> bool {
    stecyk3d::substrate::load(&substrate_path()).is_ok()
}

fn find_file(folder: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    let entries = std::fs::read_dir(folder).ok()?;
    let mut folders = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name)) {
            return Some(p);
        }
        if p.is_dir() {
            folders.push(p);
        }
    }
    if depth == 0 {
        return None;
    }
    folders.iter().find_map(|f| find_file(f, name, depth - 1))
}

/// Unpack the disc's installer archive (`data.1`) into `to`.
fn unpack_archive(archive: &Path, to: &Path) -> Result<usize, String> {
    let file = File::open(archive).map_err(|e| format!("Cannot open {}: {e}", archive.display()))?;
    let mut ar = unshield::Archive::new(file).map_err(|e| format!("{} is not a readable installer archive: {e}", archive.display()))?;
    let files: Vec<unshield::FileInfo> = ar.list().cloned().collect();
    let mut count = 0;
    for f in files {
        let mut dest = to.to_path_buf();
        for part in f.path.split('\\') {
            // never leave the destination folder
            if part.is_empty() || part == "." || part == ".." || part.contains('/') {
                continue;
            }
            dest.push(part);
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Cannot make {}: {e}", parent.display()))?;
        }
        let data = ar.load(&f.path).map_err(|e| format!("Cannot unpack {}: {e}", f.path))?;
        std::fs::write(&dest, data).map_err(|e| format!("Cannot write {}: {e}", dest.display()))?;
        count += 1;
    }
    Ok(count)
}

/// Set up from a folder the user picked: an installed copy, or the disc itself
/// (mounted, or its files copied out). Returns lines saying what was done.
pub fn set_up_from(folder: &Path) -> Result<Vec<String>, String> {
    let data = data_folder();
    std::fs::create_dir_all(&data).map_err(|e| format!("Cannot make {}: {e}", data.display()))?;
    let mut report = Vec::new();

    // An installed copy has the program file somewhere inside; a disc has the archive.
    let install = if find_file(folder, "Bryce 3D.exe", 3).is_some() || find_file(folder, "Bryce5.exe", 3).is_some() {
        folder.to_path_buf()
    } else if let Some(archive) = find_file(folder, "data.1", 1) {
        let to = data.join("from-disc");
        let _ = std::fs::remove_dir_all(&to);
        let n = unpack_archive(&archive, &to)?;
        report.push(format!("Unpacked {n} files from the disc's installer archive."));
        to
    } else {
        return Err("That folder holds neither an installed copy nor the disc's installer archive (data.1).".to_owned());
    };

    let out = substrate_path();
    match stecyk3d::extract::extract_expected(&[(None, install)], &out) {
        Ok(lines) => {
            report.extend(lines);
            Ok(report)
        }
        Err(refused) => Err(refused.messages.join("\n")),
    }
}
