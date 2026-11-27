//! "Classic look": the original program's interface pictures and layout, read at run time from
//! the user's own unpacked install (`<data folder>/from-disc/prog/Data/*.dat`). Nothing read here
//! is ever bundled with the app or written into the repository.
//!
//! Every `.dat` file is the same "CCmF - Universal" composite container as a scene file. The
//! classes that matter:
//! - `ImageIV32ARGB`: a picture, header `imge iv32` in the second entry and rle2 pixels in the
//!   first, exactly as a scene's C3dCImage (decoded by `scene_pictures::decode_picture`). Names
//!   end in a state: "... normal", "... hilited" (mouse over), "... selected" (pressed / on),
//!   or "... enabled".
//! - `elem` (472 bytes): one interface element; its rectangle is four big-endian i32 at 0x180 in
//!   Mac order (top, left, bottom, right), in the palette's own pixels; a 64-byte target name at
//!   0x58 (the window it sends to, e.g. "doc window").
//! - `eset`: the children of the element of the same identity, 80-byte records from 0x4C (count
//!   at 0x48): kind tag (imel / txel / coel / elem) at +0, child name at +0x14.
//! - `ixim`: the pictures an image element shows, 40-byte records from 0x1C: picture name (32
//!   bytes) and a state code.
//! - `canv`: the palette's size (height, width at 0x20) and fill colour (ARGB at 0x3C); `imca`
//!   names its background picture (at 0x1C).
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use stecyk3d::scene_export::mac_roman;
use stecyk3d::scene_file::{Container, Flavour, ResourceIndex};
use stecyk3d::scene_pictures::decode_picture;

/// The class of the interface pictures.
pub const PICTURE_CLASS: &str = "ImageIV32ARGB";

/// The user's unpacked `Data` folder, found case-insensitively; `None` when the disc has not
/// been unpacked.
pub fn data_dir() -> Option<PathBuf> {
    let mut at = crate::setup::data_folder();
    for part in ["from-disc", "prog", "Data"] {
        let exact = at.join(part);
        at = if exact.is_dir() {
            exact
        } else {
            std::fs::read_dir(&at)
                .ok()?
                .flatten()
                .map(|e| e.path())
                .find(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(part)))?
        };
    }
    Some(at)
}

/// One decoded interface picture.
#[derive(Clone, Debug)]
pub struct Sprite {
    /// The file it came from, without `.dat` and lower case (e.g. "createpalette").
    pub file: String,
    pub class: String,
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Straight (not premultiplied) RGBA, row by row from the top.
    pub rgba: Vec<u8>,
}

/// The states a picture name ends in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    Normal,
    Hilited,
    Selected,
    Enabled,
}

impl Sprite {
    /// The name without its state word, and the state ("sphere hilited" -> ("sphere", Hilited)).
    pub fn base_and_state(&self) -> (&str, Option<State>) {
        for (suffix, s) in [(" normal", State::Normal), (" hilited", State::Hilited), (" selected", State::Selected), (" enabled", State::Enabled)] {
            if let Some(b) = self.name.strip_suffix(suffix) {
                return (b, Some(s));
            }
        }
        (&self.name, None)
    }
}

impl Sprite {
    /// Where the picture's shape is, from its alpha. The pictures carry the palette's backdrop
    /// in their own colours and are drawn opaque; alpha only marks the shape, and with either
    /// polarity (an OK tick is opaque on clear, the edit tools and cross pads are clear on
    /// opaque), so the shape is whatever differs from the corners. A picture of one alpha level
    /// is all shape.
    pub fn hit_mask(&self) -> Vec<bool> {
        let (w, h) = (self.width as usize, self.height as usize);
        let a = |x: usize, y: usize| self.rgba.get((y * w + x) * 4 + 3).copied().unwrap_or(255) as i32;
        if w == 0 || h == 0 {
            return Vec::new();
        }
        let corner = (a(0, 0) + a(w - 1, 0) + a(0, h - 1) + a(w - 1, h - 1)) / 4;
        let mask: Vec<bool> = (0..w * h).map(|i| (a(i % w, i / w) - corner).abs() > 127).collect();
        if mask.iter().any(|&m| m) { mask } else { vec![true; w * h] }
    }

    /// The pixels with alpha set to 255 (how the original draws them).
    pub fn opaque_rgba(&self) -> Vec<u8> {
        self.rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2], 255]).collect()
    }
}

/// One interface element (button, picture, text field) and where it sits in its palette.
#[derive(Clone, Debug)]
pub struct Element {
    pub file: String,
    pub id: u32,
    pub name: String,
    /// imel (picture), txel (text), coel (colour), elem (plain container) ...
    pub kind: String,
    /// The element whose `eset` lists this one.
    pub parent: Option<u32>,
    /// The window it acts on ("doc window", "control palette", ...), often empty.
    pub target: String,
    /// As stored: left, top, right, bottom, relative to the parent.
    pub rect: [i32; 4],
    /// The same rectangle in the palette's pixels (parents' origins added).
    pub abs: [i32; 4],
    /// The picture names an image element shows (its `ixim`), with their state codes.
    pub frames: Vec<(String, u32)>,
}

/// A palette (or dialog) canvas.
#[derive(Clone, Debug)]
pub struct Canvas {
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// ARGB fill.
    pub fill: [u8; 4],
}

/// The layout of one `.dat` file.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub file: String,
    pub canvases: Vec<Canvas>,
    /// The background picture names (`imca`).
    pub backgrounds: Vec<String>,
    /// Elements in `eset` order, parents before children.
    pub elements: Vec<Element>,
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|w| u32::from_be_bytes([w[0], w[1], w[2], w[3]]))
}

fn text(b: &[u8], at: usize, len: usize) -> String {
    let f = b.get(at..(at + len).min(b.len())).unwrap_or(&[]);
    mac_roman(&f.iter().take_while(|&&c| c != 0).copied().collect::<Vec<u8>>())
}

struct Dat {
    file: String,
    container: Container,
    index: ResourceIndex,
}

impl Dat {
    fn open(path: &Path) -> Option<Dat> {
        let bytes = std::fs::read(path).ok()?;
        let container = Container::parse(&bytes, Flavour::Bryce3d).ok()?;
        let index = ResourceIndex::read(&container).ok()?;
        let file = path.file_stem()?.to_string_lossy().to_lowercase();
        Some(Dat { file, container, index })
    }

    /// (identity, name, first entry, second entry) of every resource of a class.
    fn each(&self, tag: &str) -> Vec<(u32, String, &[u8], &[u8])> {
        self.index
            .classes
            .iter()
            .filter(|c| c.tag == tag.as_bytes())
            .flat_map(|c| c.resources.iter())
            .map(|r| {
                let e = |k: usize| self.container.entry(r.entries[k] as usize).unwrap_or(&[]);
                (r.identity, mac_roman(&r.name), e(0), e(1))
            })
            .collect()
    }

    fn sprites(&self) -> Vec<Sprite> {
        let mut out = Vec::new();
        for (id, name, pixels, header) in self.each(PICTURE_CLASS) {
            let Ok(p) = decode_picture(header, pixels) else { continue };
            let rgba = p.pixels().iter().flat_map(|px| [px.0[1], px.0[2], px.0[3], px.0[0]]).collect();
            out.push(Sprite { file: self.file.clone(), class: PICTURE_CLASS.into(), id, name, width: p.width(), height: p.height(), rgba });
        }
        out.sort_by_key(|s| s.id);
        out
    }

    fn layout(&self) -> Layout {
        let mut l = Layout { file: self.file.clone(), ..Default::default() };
        for (id, name, b, _) in self.each("canv") {
            let fill = b.get(0x3C..0x40).map(|f| [f[0], f[1], f[2], f[3]]).unwrap_or([255, 0, 0, 0]);
            l.canvases.push(Canvas { id, name, height: be32(b, 0x20).unwrap_or(0), width: be32(b, 0x24).unwrap_or(0), fill });
        }
        for (_, _, b, _) in self.each("imca") {
            l.backgrounds.push(text(b, 0x1C, 32));
        }
        // the elements by identity
        struct Raw {
            name: String,
            target: String,
            rect: [i32; 4],
        }
        let mut raw: HashMap<u32, Raw> = HashMap::new();
        let mut by_name: HashMap<String, Vec<u32>> = HashMap::new();
        for (id, name, b, _) in self.each("elem") {
            let w = |at| be32(b, at).unwrap_or(0) as i32;
            // Mac order top, left, bottom, right -> left, top, right, bottom
            let rect = [w(0x184), w(0x180), w(0x18C), w(0x188)];
            by_name.entry(name.clone()).or_default().push(id);
            raw.insert(id, Raw { name, target: text(b, 0x58, 64), rect });
        }
        let mut frames: HashMap<u32, Vec<(String, u32)>> = HashMap::new();
        for (id, _, b, _) in self.each("ixim") {
            let mut v = Vec::new();
            let mut at = 0x1C;
            while at + 40 <= b.len() {
                v.push((text(b, at, 32), be32(b, at + 32).unwrap_or(0)));
                at += 40;
            }
            frames.insert(id, v);
        }
        // children: eset N lists the children of element N by kind and name
        let mut children: HashMap<u32, Vec<(String, u32)>> = HashMap::new();
        let mut has_parent: HashMap<u32, u32> = HashMap::new();
        for (parent, _, b, _) in self.each("eset") {
            let n = be32(b, 0x48).unwrap_or(0) as usize;
            for k in 0..n.min(4096) {
                let at = 0x4C + 80 * k;
                if at + 80 > b.len() {
                    break;
                }
                let kind = text(b, at, 4);
                let name = text(b, at + 0x14, 32);
                let Some(ids) = by_name.get(&name) else { continue };
                // the first element of that name not yet placed (names repeat rarely)
                let Some(&child) = ids.iter().find(|i| !has_parent.contains_key(i) && **i != parent).or(ids.first()) else { continue };
                has_parent.insert(child, parent);
                children.entry(parent).or_default().push((kind, child));
            }
        }
        // walk from the roots (elements no eset lists), parents before children
        let mut roots: Vec<u32> = raw.keys().copied().filter(|i| !has_parent.contains_key(i)).collect();
        roots.sort();
        let mut seen = std::collections::HashSet::new();
        let mut stack: Vec<(u32, String, Option<u32>, [i32; 2])> = roots.into_iter().rev().map(|r| (r, "elem".to_string(), None, [0, 0])).collect();
        while let Some((id, kind, parent, origin)) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(r) = raw.get(&id) else { continue };
            let abs = [r.rect[0] + origin[0], r.rect[1] + origin[1], r.rect[2] + origin[0], r.rect[3] + origin[1]];
            l.elements.push(Element {
                file: self.file.clone(),
                id,
                name: r.name.clone(),
                kind,
                parent,
                target: r.target.clone(),
                rect: r.rect,
                abs,
                frames: frames.get(&id).cloned().unwrap_or_default(),
            });
            // a child's origin: the parent's top-left, except under a root (the palette itself)
            let child_origin = if parent.is_none() { [0, 0] } else { [abs[0], abs[1]] };
            if let Some(kids) = children.get(&id) {
                for (kind, c) in kids.iter().rev() {
                    stack.push((*c, kind.clone(), Some(id), child_origin));
                }
            }
        }
        l
    }
}

/// The `.dat` files of the user's Data folder, sorted.
fn dat_files() -> Vec<PathBuf> {
    let Some(dir) = data_dir() else { return Vec::new() };
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dat"))).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn guarded<T: Default>(f: impl FnOnce() -> T) -> T {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_default()
}

/// Every picture of every `.dat` file (empty when the disc is not unpacked). Odd data is
/// skipped, never a panic.
pub fn load_all() -> Vec<Sprite> {
    dat_files().iter().flat_map(|p| guarded(|| Dat::open(p).map(|d| d.sprites()).unwrap_or_default())).collect()
}

/// The pictures of one file, by stem ("createpalette"), case-insensitively.
pub fn load_file(stem: &str) -> Vec<Sprite> {
    dat_files()
        .iter()
        .filter(|p| p.file_stem().is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(stem)))
        .flat_map(|p| guarded(|| Dat::open(p).map(|d| d.sprites()).unwrap_or_default()))
        .collect()
}

/// Every file's layout (canvases, backgrounds, elements with rectangles).
pub fn load_layouts() -> Vec<Layout> {
    dat_files().iter().filter_map(|p| guarded(|| Dat::open(p).map(|d| d.layout()))).collect()
}

/// `load_all`, decoded once and kept (a result is kept only when it found something, so an
/// install made later is still picked up).
pub fn cached() -> Arc<Vec<Sprite>> {
    static CACHE: Mutex<Option<Arc<Vec<Sprite>>>> = Mutex::new(None);
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(v) = g.as_ref() {
        return v.clone();
    }
    let v = Arc::new(load_all());
    if !v.is_empty() {
        *g = Some(v.clone());
    }
    v
}

/// The picture of a given name in a given file.
pub fn find<'a>(sprites: &'a [Sprite], file: &str, name: &str) -> Option<&'a Sprite> {
    sprites.iter().find(|s| s.file.eq_ignore_ascii_case(file) && s.name == name)
}

/// The picture an element shows at rest: its first frame, else "<name> normal", else "<name>".
pub fn element_sprite<'a>(sprites: &'a [Sprite], e: &Element) -> Option<&'a Sprite> {
    e.frames
        .first()
        .and_then(|(n, _)| find(sprites, &e.file, n))
        .or_else(|| find(sprites, &e.file, &format!("{} normal", e.name)))
        .or_else(|| find(sprites, &e.file, &format!("{} enabled", e.name)))
        .or_else(|| find(sprites, &e.file, &e.name))
}

fn safe(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '+' { c } else { '_' }).collect();
    t.trim_matches('_').to_string()
}

fn save_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    image::save_buffer(path, rgba, w, h, image::ExtendedColorType::Rgba8).map_err(|e| format!("{}: {e}", path.display()))
}

/// The palette drawn from its layout: fill, background, then every element's resting picture.
fn composite(l: &Layout, sprites: &[Sprite]) -> Option<(u32, u32, Vec<u8>)> {
    let c = l.canvases.iter().max_by_key(|c| c.width * c.height)?;
    let (w, h) = (c.width.clamp(1, 4096), c.height.clamp(1, 4096));
    let mut px: Vec<u8> = (0..w * h).flat_map(|_| [c.fill[1], c.fill[2], c.fill[3], 255]).collect();
    let mut blit = |s: &Sprite, x0: i32, y0: i32| {
        for y in 0..s.height as i32 {
            for x in 0..s.width as i32 {
                let (dx, dy) = (x0 + x, y0 + y);
                if dx < 0 || dy < 0 || dx >= w as i32 || dy >= h as i32 {
                    continue;
                }
                let si = ((y as u32 * s.width + x as u32) * 4) as usize;
                let di = ((dy as u32 * w + dx as u32) * 4) as usize;
                // drawn opaque: every picture carries the palette's backdrop in its own pixels
                px[di..di + 3].copy_from_slice(&s.rgba[si..si + 3]);
            }
        }
    };
    for b in &l.backgrounds {
        if let Some(s) = find(sprites, &l.file, b) {
            blit(s, 0, 0);
        }
    }
    for e in &l.elements {
        if l.backgrounds.iter().any(|b| Some(b.as_str()) == element_sprite(sprites, e).map(|s| s.name.as_str())) {
            continue;
        }
        if let Some(s) = element_sprite(sprites, e) {
            blit(s, e.abs[0], e.abs[1]);
        }
    }
    Some((w, h, px))
}

/// `kayzee3d --classic DIR`: every picture as `file__class__id__name.png`, an `index.txt`, a
/// `layout.txt` of every element's rectangle, and `composite__file.png` per palette.
pub fn cli(out: &Path) -> Result<(), String> {
    let dir = data_dir().ok_or("the disc's program files have not been unpacked (no from-disc/prog/Data folder)")?;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let sprites = load_all();
    let mut index = format!("# {} pictures from {}\n# file\tclass\tid\tname\twidth x height\talpha (% opaque / % clear)\n", sprites.len(), dir.display());
    let mut counts: Vec<(String, usize)> = Vec::new();
    for s in &sprites {
        let name = format!("{}__{}__{}__{}.png", s.file, s.class, s.id, safe(&s.name));
        save_png(&out.join(&name), s.width, s.height, &s.rgba)?;
        let n = (s.rgba.len() / 4).max(1);
        let opaque = s.rgba.chunks_exact(4).filter(|p| p[3] == 255).count() * 100 / n;
        let clear = s.rgba.chunks_exact(4).filter(|p| p[3] == 0).count() * 100 / n;
        index += &format!("{}\t{}\t{}\t{}\t{} x {}\t{opaque}/{clear}\n", s.file, s.class, s.id, s.name, s.width, s.height);
        match counts.last_mut() {
            Some((f, n)) if *f == s.file => *n += 1,
            _ => counts.push((s.file.clone(), 1)),
        }
    }
    std::fs::write(out.join("index.txt"), index).map_err(|e| e.to_string())?;

    let mut lay = String::from("# file\tid\tkind\tname\tparent\tleft,top,right,bottom (palette pixels)\ttarget\tframes\n");
    for l in load_layouts() {
        for c in &l.canvases {
            lay += &format!("{}\tcanvas {}\t{:?}\t{} x {}\tfill {:02x?}\n", l.file, c.id, c.name, c.width, c.height, c.fill);
        }
        for b in &l.backgrounds {
            lay += &format!("{}\tbackground\t{b:?}\n", l.file);
        }
        for e in &l.elements {
            let frames: Vec<String> = e.frames.iter().map(|(n, c)| format!("{n}#{c}")).collect();
            lay += &format!(
                "{}\t{}\t{}\t{:?}\t{}\t{},{},{},{}\t{}\t{}\n",
                l.file,
                e.id,
                e.kind,
                e.name,
                e.parent.map(|p| p.to_string()).unwrap_or("-".into()),
                e.abs[0],
                e.abs[1],
                e.abs[2],
                e.abs[3],
                e.target,
                frames.join(" | ")
            );
        }
        if let Some((w, h, px)) = composite(&l, &sprites) {
            save_png(&out.join(format!("composite__{}.png", l.file)), w, h, &px)?;
        }
    }
    std::fs::write(out.join("layout.txt"), lay).map_err(|e| e.to_string())?;
    for (f, n) in &counts {
        println!("{f}: {n}");
    }
    println!("{} pictures written to {}", sprites.len(), out.display());
    Ok(())
}
