//! Sky presets (.bsk) and terrain presets (.obp) from the user's own disc,
//! read with the renderer's part readers, each with its own small picture.

use std::path::Path;

use serde_json::{Value, json};
use stecyk3d::geom::Rgb;
use stecyk3d::scene_export::mac_roman;
use stecyk3d::scene_file::{Container, Flavour, Resource, ResourceIndex};
use stecyk3d::scene_height_fields::TerrainPart;
use stecyk3d::scene_parts::{FIRST_PART_LEN, LightPart, SUN_PART_LEN, SunPart, first_part_name, order_of};
use stecyk3d::scene_pictures::decode_picture;
use stecyk3d::scene_records::PlacementPart;
use stecyk3d::scene_sky::Sky;

pub struct Preset {
    pub name: String,
    /// (width, height, RGB levels)
    pub thumb: Option<(u32, u32, Vec<u8>)>,
    /// A sky in the app's form ({"sky": …, "sun": …}) or a terrain: {"n", "heights" (list), "mirrored", "smooth"}.
    pub value: Value,
}

struct File {
    container: Container,
    index: ResourceIndex,
}

impl File {
    fn open(path: &Path) -> Result<File, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let container = Container::parse(&bytes, Flavour::Bryce3d).map_err(|e| format!("{e}"))?;
        let index = ResourceIndex::read(&container).map_err(|e| format!("{e}"))?;
        Ok(File { container, index })
    }
    fn resources(&self, tag: &str) -> Vec<&Resource> {
        self.index.classes.iter().filter(|c| c.tag == tag.as_bytes()).flat_map(|c| c.resources.iter()).collect()
    }
    fn data(&self, r: &Resource) -> Option<&[u8]> {
        self.container.entry(r.entries[0] as usize)
    }
    fn name(&self, class: &str, identity: u32) -> String {
        let Some(b) = self.resources(class).into_iter().find(|r| r.identity == identity).and_then(|r| self.data(r)) else { return String::new() };
        mac_roman(&b[..b.len().min(64)].iter().take_while(|&&c| c != 0).copied().collect::<Vec<u8>>()).trim().to_owned()
    }
    fn thumbnail(&self, class: &str, identity: u32) -> Option<(u32, u32, Vec<u8>)> {
        let r = self.resources(class).into_iter().find(|r| r.identity == identity)?;
        let header = self.container.entry(r.entries[1] as usize)?;
        let p = decode_picture(header, self.data(r)?).ok()?;
        Some((p.width(), p.height(), p.pixels().iter().flat_map(|px| [px.0[1], px.0[2], px.0[3]]).collect()))
    }
}

/// A colour as the app stores it, from either form the reader gives.
trait Colour {
    fn rgb(&self) -> Value;
}
impl Colour for Rgb {
    fn rgb(&self) -> Value {
        json!([self.r, self.g, self.b])
    }
}
impl Colour for [f32; 3] {
    fn rgb(&self) -> Value {
        json!(self)
    }
}
fn rgb(c: &impl Colour) -> Value {
    c.rgb()
}

fn sky_value(s: &Sky, sun: Option<(u32, f64, SunPart)>) -> Value {
    let mut sky = json!({
        "flags": s.flags & !0x200, "cloud_height": s.cloud_height, "cloud_colour": rgb(&s.lower_cloud_colour), "upper_cloud_colour": rgb(&s.upper_cloud_colour),
        "cloud_offset": s.cloud_offset, "cover": s.cloud_cover, "amplitude": s.cloud_amplitude, "frequency": s.cloud_frequency,
        "sky_colour": rgb(&s.sky_colour), "horizon_colour": rgb(&s.horizon_colour), "dome_colour": rgb(&s.dome_colour), "plain_colour": rgb(&s.plain_sky_colour),
        "shadows": s.shadow_intensity, "ambient_colour": rgb(&s.ambient_colour), "ambience": s.ambience, "haze_colour": rgb(&s.haze_colour), "haze": s.haze,
        "fog_colour": rgb(&s.fog_colour), "fog_height": s.fog_height, "fog": s.fog, "moon": s.moon, "discs": s.discs, "lit_air": s.lit_air,
        "stars": [s.star_seed, s.star_count, s.comet_count],
    });
    // the cloud textures of a preset are not carried: clouds keep whatever the scene has
    sky["flags"] = json!(s.flags & !0x200 & !(0x10 | 0x20 | 0x40 | 0x1000));
    let sun = match sun {
        Some((flags, intensity, part)) => {
            let mut d: [f64; 3] = part.direction;
            if s.flags & 0x200 != 0 {
                // stored in the camera's frame in the file: use a pleasant world direction of the same height
                let up: f64 = d[1].clamp(-1.0, 1.0);
                let h = (1.0 - up * up).max(0.0).sqrt();
                d = [0.6 * h, up, -0.8 * h];
            }
            json!({"direction": d, "colour": rgb(&part.colour), "glow_colour": rgb(&part.glow_colour), "light_colour": rgb(&part.colour), "intensity": intensity, "switches": flags | 0x400})
        }
        None => crate::create::default_sky()["sun"].clone(),
    };
    json!({"sky": sky, "sun": sun})
}

pub fn skies(path: &Path) -> Result<Vec<Preset>, String> {
    let f = File::open(path)?;
    let mut out = Vec::new();
    for r in f.resources("C3dBaseSky") {
        let Some(data) = f.data(r) else { continue };
        let Ok(sky) = Sky::parse(&data[FIRST_PART_LEN.min(data.len())..], order_of(data)) else { continue };
        let sun = f.resources("C3dSunLight").into_iter().find(|s| s.identity == r.identity).and_then(|s| {
            let d = f.data(s)?;
            let o = order_of(d);
            let placement = PlacementPart::parse_either(d.get(FIRST_PART_LEN..)?, o).ok()?;
            let at = FIRST_PART_LEN + PlacementPart::len_of_version(placement.version)?;
            let v = u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?);
            let light = LightPart::parse(d.get(at..at + LightPart::len_of_version(v).unwrap_or(192))?, o).ok()?;
            let at = at + LightPart::len_of_version(light.version)?;
            let part = SunPart::parse(d.get(at..at + SUN_PART_LEN)?, o).ok()?;
            Some((placement.second_flags, light.intensity, part))
        });
        let name = f.name("CPresetDescr", r.identity);
        out.push(Preset { name: if name.is_empty() { format!("Sky {}", out.len() + 1) } else { name }, thumb: f.thumbnail("skyprevcimage", r.identity), value: sky_value(&sky, sun) });
    }
    Ok(out)
}

pub fn terrains(path: &Path) -> Result<Vec<Preset>, String> {
    let f = File::open(path)?;
    let descs = f.resources("objpresetdesc");
    let mut out = Vec::new();
    for (k, r) in f.resources("C3dHeightField").into_iter().enumerate() {
        let Some(d) = f.data(r) else { continue };
        let o = order_of(d);
        let Ok(placement) = PlacementPart::parse_either(&d[FIRST_PART_LEN.min(d.len())..], o) else { continue };
        let Some(len) = PlacementPart::len_of_version(placement.version) else { continue };
        let Ok(t) = TerrainPart::parse(d, FIRST_PART_LEN + len, o) else { continue };
        let own = first_part_name(d).map(|n| mac_roman(&n)).unwrap_or_default();
        // the preset's own name and picture, when the file lines them up one to one
        let (name, thumb) = match descs.get(k) {
            Some(p) if descs.len() == f.resources("C3dHeightField").len() => (f.name("objpresetdesc", p.identity), f.thumbnail("objprevcimage", p.identity)),
            _ => (own.clone(), None),
        };
        let name = if name.is_empty() { if own.is_empty() { format!("Terrain {}", k + 1) } else { own } } else { name };
        out.push(Preset { name, thumb, value: json!({"n": t.grid.side(), "heights": t.grid.heights(), "mirrored": t.mirrored, "smooth": t.smooth}) });
    }
    Ok(out)
}
