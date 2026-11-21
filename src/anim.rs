//! Animation: keyframes on objects, the camera, the sky and materials; posing
//! the scene at a time; recording keys as things change.

use crate::math::Cam;
use crate::model::{self, Project, bracket, set_key};
use crate::state::Doc;

/// A time curve between two keys: 0 linear, 1 ease in and out.
fn ease(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// Set every animated thing in the project to its value at time `t`.
pub fn pose(p: &mut Project, t: f64) {
    let linear = p.anim.linear;
    let ease = |f: f64| if linear { f } else { ease(f) };
    for n in &mut p.nodes {
        if let Some((a, b, f)) = bracket(&n.keys, t) {
            n.m = if f == 0.0 { a.v } else { model::blend_placement(&a.v, &b.v, ease(f)) };
        }
    }
    if let Some((a, b, f)) = bracket(&p.anim.camera_keys, t) {
        p.cam = if f == 0.0 { a.v.clone() } else { Cam::blend(&a.v, &b.v, ease(f)) };
    }
    if let Some((a, b, f)) = bracket(&p.anim.sky_keys, t) {
        p.sky = if f == 0.0 { a.v.clone() } else { model::blend_value(&a.v, &b.v, ease(f)) };
    }
    for m in &mut p.materials {
        if let Some((a, b, f)) = bracket(&m.keys, t) {
            m.record = if f == 0.0 { a.v.clone() } else { model::blend_value(&a.v, &b.v, ease(f)) };
        }
    }
    follow_links(p);
    p.anim.time = t;
}

/// After a change at the current time: record keys for whatever differs from
/// `before` (the project as it was when the change began).
pub fn record(doc: &mut Doc, before: &Project) {
    let p = &mut doc.project;
    if !p.anim.auto_key {
        return;
    }
    let t = p.anim.time;
    let started = t > 1e-6;
    for n in &mut p.nodes {
        let Some(old) = before.node(n.id) else { continue };
        if old.m == n.m {
            continue;
        }
        if n.keys.is_empty() {
            if !started {
                continue;
            }
            set_key(&mut n.keys, 0.0, old.m);
        }
        set_key(&mut n.keys, t, n.m);
    }
    if before.cam != p.cam {
        if p.anim.camera_keys.is_empty() {
            if started {
                set_key(&mut p.anim.camera_keys, 0.0, before.cam.clone());
                set_key(&mut p.anim.camera_keys, t, p.cam.clone());
            }
        } else {
            set_key(&mut p.anim.camera_keys, t, p.cam.clone());
        }
    }
    if before.sky != p.sky {
        if p.anim.sky_keys.is_empty() {
            if started {
                set_key(&mut p.anim.sky_keys, 0.0, before.sky.clone());
                set_key(&mut p.anim.sky_keys, t, p.sky.clone());
            }
        } else {
            set_key(&mut p.anim.sky_keys, t, p.sky.clone());
        }
    }
    for (i, m) in p.materials.iter_mut().enumerate() {
        let Some(old) = before.materials.get(i) else { continue };
        if old.record == m.record {
            continue;
        }
        if m.keys.is_empty() {
            if started {
                set_key(&mut m.keys, 0.0, old.record.clone());
                set_key(&mut m.keys, t, m.record.clone());
            }
        } else {
            set_key(&mut m.keys, t, m.record.clone());
        }
    }
}

/// Put a key at the current time on the selection (or on the camera when
/// nothing is selected).
pub fn add_key(doc: &mut Doc) {
    let t = doc.project.anim.time;
    let ids: Vec<u64> = doc.selection.iter().flat_map(|s| doc.project.with_descendants(*s)).collect();
    if ids.is_empty() {
        let cam = doc.project.cam.clone();
        set_key(&mut doc.project.anim.camera_keys, t, cam);
    }
    for id in ids {
        if let Some(n) = doc.project.node_mut(id) {
            let m = n.m;
            set_key(&mut n.keys, t, m);
        }
    }
}

pub fn remove_key(doc: &mut Doc) {
    let t = doc.project.anim.time;
    let ids: Vec<u64> = doc.selection.iter().flat_map(|s| doc.project.with_descendants(*s)).collect();
    if ids.is_empty() {
        model::remove_key(&mut doc.project.anim.camera_keys, t);
    }
    for id in ids {
        if let Some(n) = doc.project.node_mut(id) {
            model::remove_key(&mut n.keys, t);
        }
    }
}

/// Every key time in the scene (for the selection when there is one), sorted.
pub fn key_times(doc: &Doc) -> Vec<f64> {
    let p = &doc.project;
    let mut out: Vec<f64> = Vec::new();
    let ids: Vec<u64> = doc.selection.iter().flat_map(|s| p.with_descendants(*s)).collect();
    for n in &p.nodes {
        if ids.is_empty() || ids.contains(&n.id) {
            out.extend(n.keys.iter().map(|k| k.t));
        }
    }
    if ids.is_empty() {
        out.extend(p.anim.camera_keys.iter().map(|k| k.t));
        out.extend(p.anim.sky_keys.iter().map(|k| k.t));
        for m in &p.materials {
            out.extend(m.keys.iter().map(|k| k.t));
        }
    }
    out.sort_by(|a, b| a.total_cmp(b));
    out.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    out
}

pub fn has_animation(p: &Project) -> bool {
    p.nodes.iter().any(|n| n.keys.len() > 1) || p.anim.camera_keys.len() > 1 || p.anim.sky_keys.len() > 1 || p.materials.iter().any(|m| m.keys.len() > 1)
}

/// The frame times of the working range.
pub fn frame_times(p: &Project) -> Vec<f64> {
    let fps = p.anim.fps.max(1.0);
    let (a, b) = (p.anim.range[0].max(0.0), p.anim.range[1].max(p.anim.range[0]));
    let count = ((b - a) * fps).round() as usize + 1;
    (0..count).map(|k| a + k as f64 / fps).collect()
}

/// Remove every key (the scene keeps its current pose).
pub fn clear(p: &mut Project) {
    for n in &mut p.nodes {
        n.keys.clear();
    }
    p.anim.camera_keys.clear();
    p.anim.sky_keys.clear();
    for m in &mut p.materials {
        m.keys.clear();
    }
}

/// Objects linked to another (and without keys of their own) take their place
/// from their parent: parents first, so chains of links work.
pub fn follow_links(p: &mut Project) {
    for _ in 0..8 {
        let mut moved = false;
        let placed: Vec<(usize, crate::math::M12)> = p
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.keys.len() < 2)
            .filter_map(|(i, n)| {
                let rest = n.link_rest?;
                let parent = p.node(n.link?)?;
                let m = crate::math::compose(&parent.m, &rest);
                (m != n.m).then_some((i, m))
            })
            .collect();
        for (i, m) in placed {
            p.nodes[i].m = m;
            moved = true;
        }
        if !moved {
            break;
        }
    }
}

/// Remember where `child` sits relative to its link parent now.
pub fn set_link_rest(p: &mut Project, child: u64) {
    let rest = p.node(child).and_then(|n| {
        let parent = p.node(n.link?)?;
        Some(crate::math::compose(&crate::math::inverse(&parent.m)?, &n.m))
    });
    if let Some(n) = p.node_mut(child) {
        n.link_rest = rest;
    }
}
