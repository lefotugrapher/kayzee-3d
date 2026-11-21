//! Everything the open document's window holds: the project, what is
//! selected, undo history, and the operations the palettes and menus call.

use std::path::PathBuf;

use crate::math::{self, M12, V3, add, mul, sub};
use crate::model::{Node, Project, Role, Shape};

const UNDO_DEPTH: usize = 80;

pub struct Doc {
    pub project: Project,
    pub selection: Vec<u64>,
    pub file: Option<PathBuf>,
    pub dirty: bool,
    /// Goes up on every change; previews and caches watch it.
    pub revision: u64,
    undo: Vec<(Project, Vec<u64>)>,
    redo: Vec<(Project, Vec<u64>)>,
    /// Objects copied with Copy.
    pub clipboard: Vec<Node>,
    pub material_clipboard: Option<crate::model::Material>,
}

impl Doc {
    pub fn new(project: Project) -> Self {
        Doc { project, selection: Vec::new(), file: None, dirty: false, revision: 1, undo: Vec::new(), redo: Vec::new(), clipboard: Vec::new(), material_clipboard: None }
    }

    // ----------------------------------------------------------------- history

    /// Call before changing the project: remembers the state to return to.
    pub fn checkpoint(&mut self) {
        self.undo.push((self.project.clone(), self.selection.clone()));
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Call after changing the project.
    pub fn touch(&mut self) {
        self.dirty = true;
        self.revision += 1;
    }

    /// Forget the last checkpoint (nothing changed after it).
    pub fn undo_discard(&mut self) {
        self.undo.pop();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some((p, s)) = self.undo.pop() {
            self.redo.push((std::mem::replace(&mut self.project, p), std::mem::replace(&mut self.selection, s)));
            self.touch();
        }
    }

    pub fn redo(&mut self) {
        if let Some((p, s)) = self.redo.pop() {
            self.undo.push((std::mem::replace(&mut self.project, p), std::mem::replace(&mut self.selection, s)));
            self.touch();
        }
    }

    // ----------------------------------------------------------------- selection

    pub fn select_only(&mut self, id: u64) {
        self.selection = vec![id];
    }
    pub fn toggle(&mut self, id: u64) {
        if let Some(i) = self.selection.iter().position(|s| *s == id) {
            self.selection.remove(i);
        } else {
            self.selection.push(id);
        }
    }
    pub fn is_selected(&self, id: u64) -> bool {
        self.selection.contains(&id)
    }
    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }
    pub fn select_all(&mut self) {
        self.selection = self.project.nodes.iter().filter(|n| n.group.is_none() && !n.hidden && !n.locked).map(|n| n.id).collect();
    }
    pub fn select_where(&mut self, pick: impl Fn(&Node) -> bool) {
        let p = &self.project;
        let mut ids: Vec<u64> = p.nodes.iter().filter(|n| !n.hidden && pick(n)).map(|n| p.top(n.id)).collect();
        ids.dedup();
        let mut seen = Vec::new();
        ids.retain(|i| if seen.contains(i) { false } else { seen.push(*i); true });
        self.selection = ids;
    }
    /// Step the selection to the next/previous top-level object.
    pub fn select_step(&mut self, forward: bool) {
        let tops: Vec<u64> = self.project.nodes.iter().filter(|n| n.group.is_none() && !n.hidden).map(|n| n.id).collect();
        if tops.is_empty() {
            return;
        }
        let here = self.selection.first().and_then(|s| tops.iter().position(|t| t == s));
        let next = match here {
            Some(i) if forward => (i + 1) % tops.len(),
            Some(i) => (i + tops.len() - 1) % tops.len(),
            None => 0,
        };
        self.selection = vec![tops[next]];
    }

    /// The single selected object, when exactly one is selected.
    pub fn one(&self) -> Option<&Node> {
        if self.selection.len() == 1 { self.project.node(self.selection[0]) } else { None }
    }

    pub fn selection_centre(&self) -> Option<V3> {
        let (lo, hi) = self.project.world_bounds(&self.selection)?;
        Some(mul(add(lo, hi), 0.5))
    }

    // ----------------------------------------------------------------- transforms on the selection

    /// Apply a world-space operation to every selected object (and group members).
    pub fn transform_selection(&mut self, op: &M12) {
        let ids: Vec<u64> = self.selection.iter().flat_map(|s| self.project.with_descendants(*s)).collect();
        for id in ids {
            if let Some(n) = self.project.node_mut(id) {
                if !n.locked {
                    n.m = math::compose(op, &n.m);
                }
            }
        }
    }

    pub fn move_selection(&mut self, by: V3) {
        self.transform_selection(&math::translation(by));
    }

    /// Turn the selection about its own centre.
    pub fn rotate_selection(&mut self, axis: V3, radians: f64, pivot: V3) {
        let op = math::about(&math::turn(axis, radians), pivot, &math::IDENTITY);
        self.transform_selection(&op);
    }

    /// Resize along world axes about a pivot.
    pub fn scale_selection(&mut self, factors: V3, pivot: V3) {
        let op = math::about(&math::scaling(factors), pivot, &math::IDENTITY);
        self.transform_selection(&op);
    }

    /// Resize each selected object along its OWN axes about its own centre.
    pub fn scale_own(&mut self, factors: V3) {
        let ids = self.selection.clone();
        for id in ids {
            let members = self.project.with_descendants(id);
            if members.len() > 1 {
                // a group resizes as a whole about its centre, along world axes
                if let Some((lo, hi)) = self.project.world_bounds(&[id]) {
                    let pivot = mul(add(lo, hi), 0.5);
                    let op = math::about(&math::scaling(factors), pivot, &math::IDENTITY);
                    for d in members {
                        if let Some(n) = self.project.node_mut(d) {
                            n.m = math::compose(&op, &n.m);
                        }
                    }
                }
            } else if let Some(n) = self.project.node_mut(id) {
                if !n.locked {
                    for k in 0..3 {
                        for c in 0..3 {
                            n.m[k * 3 + c] *= factors[k];
                        }
                    }
                }
            }
        }
    }

    /// Turn each selected object about one of its OWN axes through its own origin.
    pub fn rotate_own(&mut self, axis_index: usize, radians: f64) {
        let ids = self.selection.clone();
        for id in ids {
            let Some(n) = self.project.node(id) else { continue };
            let pivot = if self.project.members(id).is_empty() { math::origin(&n.m) } else { self.project.world_bounds(&[id]).map(|(a, b)| mul(add(a, b), 0.5)).unwrap_or(math::origin(&n.m)) };
            let axis = if self.project.members(id).is_empty() { math::unit(math::axis(&n.m, axis_index)) } else { let mut a = [0.0; 3]; a[axis_index] = 1.0; a };
            let op = math::about(&math::turn(axis, radians), pivot, &math::IDENTITY);
            for d in self.project.with_descendants(id) {
                if let Some(n) = self.project.node_mut(d) {
                    if !n.locked {
                        n.m = math::compose(&op, &n.m);
                    }
                }
            }
        }
    }

    // ----------------------------------------------------------------- object commands

    pub fn delete_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        let ids = std::mem::take(&mut self.selection);
        self.project.remove(&ids);
        self.touch();
    }

    /// Copies of the given objects (with their group members), given fresh ids.
    fn clone_nodes(&mut self, sources: &[Node]) -> Vec<u64> {
        let mut map: std::collections::HashMap<u64, u64> = Default::default();
        for s in sources {
            map.insert(s.id, self.project.fresh_id());
        }
        let mut tops = Vec::new();
        for s in sources {
            let mut n = s.clone();
            n.id = map[&s.id];
            n.group = s.group.and_then(|g| map.get(&g).copied());
            if s.group.is_none() || n.group.is_none() {
                n.group = None;
                tops.push(n.id);
            }
            n.link = s.link.map(|l| map.get(&l).copied().unwrap_or(l));
            n.track = s.track.map(|l| map.get(&l).copied().unwrap_or(l));
            n.name = self.project.name_for(s.name.trim_end_matches(|c: char| c.is_ascii_digit() || c == ' '));
            self.project.nodes.push(n);
        }
        tops
    }

    fn gather(&self, ids: &[u64]) -> Vec<Node> {
        let mut out = Vec::new();
        for id in ids {
            for d in self.project.with_descendants(*id) {
                if let Some(n) = self.project.node(d) {
                    if !out.iter().any(|o: &Node| o.id == n.id) {
                        out.push(n.clone());
                    }
                }
            }
        }
        out
    }

    pub fn duplicate_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        let sources = self.gather(&self.selection.clone());
        self.selection = self.clone_nodes(&sources);
        self.touch();
    }

    /// Duplicate and apply the same offset/turn/resize again and again.
    pub fn replicate(&mut self, copies: usize, offset: V3, degrees: V3, size: V3) {
        if self.selection.is_empty() || copies == 0 {
            return;
        }
        self.checkpoint();
        for _ in 0..copies {
            let sources = self.gather(&self.selection.clone());
            self.selection = self.clone_nodes(&sources);
            let Some(pivot) = self.selection_centre() else { break };
            let t = math::Trs { size, degrees, position: [0.0; 3] };
            let op = math::about(&math::recompose(&t), pivot, &math::IDENTITY);
            self.transform_selection(&op);
            self.move_selection(offset);
        }
        self.touch();
    }

    pub fn copy(&mut self) {
        self.clipboard = self.gather(&self.selection.clone());
    }
    pub fn cut(&mut self) {
        self.copy();
        self.delete_selection();
    }
    pub fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        self.checkpoint();
        let sources = self.clipboard.clone();
        self.selection = self.clone_nodes(&sources);
        self.touch();
    }

    pub fn group_selection(&mut self) -> Option<u64> {
        if self.selection.len() < 2 {
            return None;
        }
        self.checkpoint();
        let (lo, hi) = self.project.world_bounds(&self.selection)?;
        let centre = mul(add(lo, hi), 0.5);
        let size = sub(hi, lo);
        let m = math::from_axes([size[0].max(1e-6), 0.0, 0.0], [0.0, size[1].max(1e-6), 0.0], [0.0, 0.0, size[2].max(1e-6)], centre);
        let id = self.project.add("Group", Shape::Group, m, None);
        for s in self.selection.clone() {
            if let Some(n) = self.project.node_mut(s) {
                n.group = Some(id);
            }
        }
        self.selection = vec![id];
        self.touch();
        Some(id)
    }

    pub fn ungroup_selection(&mut self) {
        let groups: Vec<u64> = self.selection.iter().copied().filter(|s| self.project.node(*s).is_some_and(|n| matches!(n.shape, Shape::Group))).collect();
        if groups.is_empty() {
            return;
        }
        self.checkpoint();
        let mut freed = Vec::new();
        for g in &groups {
            let outer = self.project.node(*g).and_then(|n| n.group);
            for m in self.project.members(*g) {
                if let Some(n) = self.project.node_mut(m) {
                    n.group = outer;
                    n.role = Role::Neutral;
                }
                freed.push(m);
            }
        }
        self.project.nodes.retain(|n| !groups.contains(&n.id));
        self.selection = freed;
        self.touch();
    }

    /// Keep a group's box around its members after they move.
    pub fn refit_groups(&mut self) {
        let groups: Vec<u64> = self.project.nodes.iter().filter(|n| matches!(n.shape, Shape::Group)).map(|n| n.id).collect();
        for g in groups {
            let members = self.project.members(g);
            if let Some((lo, hi)) = self.project.world_bounds(&members) {
                let centre = mul(add(lo, hi), 0.5);
                let size = sub(hi, lo);
                if let Some(n) = self.project.node_mut(g) {
                    n.m = math::from_axes([size[0].max(1e-6), 0.0, 0.0], [0.0, size[1].max(1e-6), 0.0], [0.0, 0.0, size[2].max(1e-6)], centre);
                }
            }
        }
    }

    /// Drop each selected object straight down onto whatever lies below it
    /// (another object's top, or the ground level).
    pub fn land_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        for id in self.selection.clone() {
            let Some((lo, hi)) = self.project.world_bounds(&[id]) else { continue };
            let mine = self.project.with_descendants(id);
            let mut floor: Option<f64> = None;
            for n in &self.project.nodes {
                if mine.contains(&n.id) || n.hidden || matches!(n.shape, Shape::Group | Shape::Light(_)) {
                    continue;
                }
                if let Shape::Plane = n.shape {
                    let y = math::origin(&n.m)[1];
                    if y <= lo[1] + 1e-9 {
                        floor = Some(floor.map_or(y, |f: f64| f.max(y)));
                    }
                    continue;
                }
                let Some((a, b)) = self.project.world_bounds(&[n.id]) else { continue };
                let overlap = a[0] < hi[0] && b[0] > lo[0] && a[2] < hi[2] && b[2] > lo[2];
                if overlap && b[1] <= lo[1] + 1e-9 {
                    floor = Some(floor.map_or(b[1], |f: f64| f.max(b[1])));
                }
            }
            let target = floor.unwrap_or(0.0);
            let by = [0.0, target - lo[1], 0.0];
            let op = math::translation(by);
            for d in mine {
                if let Some(n) = self.project.node_mut(d) {
                    n.m = math::compose(&op, &n.m);
                }
            }
        }
        self.touch();
    }

    /// Line the selection up along one world axis: mode 0 = low sides,
    /// 1 = centres, 2 = high sides.
    pub fn align_selection(&mut self, axis: usize, mode: u8) {
        let Some((lo, hi)) = self.project.world_bounds(&self.selection) else { return };
        if self.selection.len() < 2 {
            return;
        }
        self.checkpoint();
        for id in self.selection.clone() {
            let Some((a, b)) = self.project.world_bounds(&[id]) else { continue };
            let delta = match mode {
                0 => lo[axis] - a[axis],
                2 => hi[axis] - b[axis],
                _ => 0.5 * (lo[axis] + hi[axis]) - 0.5 * (a[axis] + b[axis]),
            };
            let mut by = [0.0; 3];
            by[axis] = delta;
            let op = math::translation(by);
            for d in self.project.with_descendants(id) {
                if let Some(n) = self.project.node_mut(d) {
                    n.m = math::compose(&op, &n.m);
                }
            }
        }
        self.touch();
    }

    /// Scatter the selection: random offsets, turns and sizes up to `amount`
    /// (0..1). `what`: bit 0 position, bit 1 turn, bit 2 size.
    pub fn randomize_selection(&mut self, amount: f64, what: u8, seed: &mut u64) {
        if self.selection.is_empty() {
            return;
        }
        let mut next = || {
            *seed ^= *seed << 13;
            *seed ^= *seed >> 7;
            *seed ^= *seed << 17;
            (*seed >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
        };
        let Some((lo, hi)) = self.project.world_bounds(&self.selection) else { return };
        let span = math::len(sub(hi, lo)).max(1.0);
        for id in self.selection.clone() {
            let Some((a, b)) = self.project.world_bounds(&[id]) else { continue };
            let pivot = mul(add(a, b), 0.5);
            let mut op = math::IDENTITY;
            if what & 4 != 0 {
                let f = (1.0 + amount * next() * 0.9).max(0.05);
                op = math::about(&math::scaling([f, f, f]), pivot, &op);
            }
            if what & 2 != 0 {
                for k in 0..3 {
                    let mut axis = [0.0; 3];
                    axis[k] = 1.0;
                    op = math::compose(&math::about(&math::turn(axis, amount * next() * std::f64::consts::PI), pivot, &math::IDENTITY), &op);
                }
            }
            if what & 1 != 0 {
                let by = [amount * next() * span * 0.5, amount * next() * span * 0.15, amount * next() * span * 0.5];
                op = math::compose(&math::translation(by), &op);
            }
            for d in self.project.with_descendants(id) {
                if let Some(n) = self.project.node_mut(d) {
                    if !n.locked {
                        n.m = math::compose(&op, &n.m);
                    }
                }
            }
        }
    }
}

/// Turn every object that tracks another so it faces its target: lights aim
/// their beam, other objects their own front (+z).
pub fn aim_trackers(p: &mut Project) {
    let targets: Vec<(u64, u64)> = p.nodes.iter().filter_map(|n| n.track.map(|t| (n.id, t))).collect();
    for (id, target) in targets {
        let Some((lo, hi)) = p.world_bounds(&[target]) else { continue };
        let aim_at = mul(add(lo, hi), 0.5);
        let Some(n) = p.node_mut(id) else { continue };
        let here = math::origin(&n.m);
        let to = sub(aim_at, here);
        if math::len(to) < 1e-9 {
            continue;
        }
        let dir = math::unit(to);
        let sizes = [math::len(math::axis(&n.m, 0)), math::len(math::axis(&n.m, 1)), math::len(math::axis(&n.m, 2))];
        let up = if dir[1].abs() > 0.999 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let (x, y, z) = if matches!(n.shape, Shape::Light(_)) {
            // a beam travels along its own -y
            let y = mul(dir, -1.0);
            let x = math::unit(math::cross(up, y));
            (x, y, math::cross(x, y))
        } else {
            let z = dir;
            let x = math::unit(math::cross(up, z));
            (x, math::cross(z, x), z)
        };
        n.m = math::from_axes(mul(x, sizes[0]), mul(y, sizes[1]), mul(z, sizes[2]), here);
    }
}
