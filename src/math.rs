//! Vectors, placements (twelve numbers: the images of an object's own x, y and
//! z axes, then its position) and the camera's moves.

use serde::{Deserialize, Serialize};

pub type V3 = [f64; 3];
/// The images of the own x, y, z axes (three numbers each), then the translation.
pub type M12 = [f64; 12];

pub const IDENTITY: M12 = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
pub fn unit(a: V3) -> V3 {
    let l = len(a);
    if l > 1e-300 { mul(a, 1.0 / l) } else { a }
}
pub fn lerp(a: V3, b: V3, t: f64) -> V3 {
    add(a, mul(sub(b, a), t))
}

pub fn axis(m: &M12, k: usize) -> V3 {
    [m[k * 3], m[k * 3 + 1], m[k * 3 + 2]]
}
pub fn origin(m: &M12) -> V3 {
    [m[9], m[10], m[11]]
}
pub fn from_axes(x: V3, y: V3, z: V3, t: V3) -> M12 {
    [x[0], x[1], x[2], y[0], y[1], y[2], z[0], z[1], z[2], t[0], t[1], t[2]]
}
/// Where the own-space point `p` lands in the world.
pub fn place(m: &M12, p: V3) -> V3 {
    add(origin(m), dir(m, p))
}
/// The own-space direction `p` in the world (no translation).
pub fn dir(m: &M12, p: V3) -> V3 {
    add(add(mul(axis(m, 0), p[0]), mul(axis(m, 1), p[1])), mul(axis(m, 2), p[2]))
}
/// `a` after `b`: the placement that applies `b` first, then `a`.
pub fn compose(a: &M12, b: &M12) -> M12 {
    from_axes(dir(a, axis(b, 0)), dir(a, axis(b, 1)), dir(a, axis(b, 2)), place(a, origin(b)))
}
pub fn inverse(m: &M12) -> Option<M12> {
    let (x, y, z) = (axis(m, 0), axis(m, 1), axis(m, 2));
    let det = dot(x, cross(y, z));
    if det.abs() < 1e-300 {
        return None;
    }
    // rows of the inverse of the matrix whose columns are x, y, z
    let r0 = mul(cross(y, z), 1.0 / det);
    let r1 = mul(cross(z, x), 1.0 / det);
    let r2 = mul(cross(x, y), 1.0 / det);
    // as images of axes: column k of the inverse
    let ix = [r0[0], r1[0], r2[0]];
    let iy = [r0[1], r1[1], r2[1]];
    let iz = [r0[2], r1[2], r2[2]];
    let t = origin(m);
    let it = [-dot(r0, t), -dot(r1, t), -dot(r2, t)];
    Some(from_axes(ix, iy, iz, it))
}
pub fn translation(t: V3) -> M12 {
    from_axes([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], t)
}
pub fn scaling(s: V3) -> M12 {
    from_axes([s[0], 0.0, 0.0], [0.0, s[1], 0.0], [0.0, 0.0, s[2]], [0.0; 3])
}
/// A turn of `angle` radians about the unit `k` through the origin.
pub fn turn(k: V3, angle: f64) -> M12 {
    let k = unit(k);
    let (s, c) = angle.sin_cos();
    let r = |v: V3| add(add(mul(v, c), mul(cross(k, v), s)), mul(k, dot(k, v) * (1.0 - c)));
    from_axes(r([1.0, 0.0, 0.0]), r([0.0, 1.0, 0.0]), r([0.0, 0.0, 1.0]), [0.0; 3])
}
/// The same placement turned/scaled by `op` about the world point `pivot`.
pub fn about(op: &M12, pivot: V3, m: &M12) -> M12 {
    let there = translation(pivot);
    let back = translation(mul(pivot, -1.0));
    compose(&there, &compose(op, &compose(&back, m)))
}

/// Size along each own axis, turn as degrees about x, y, z (applied in that
/// order), and position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trs {
    pub size: V3,
    pub degrees: V3,
    pub position: V3,
}

pub fn decompose(m: &M12) -> Trs {
    let (x, y, z) = (axis(m, 0), axis(m, 1), axis(m, 2));
    let mut size = [len(x), len(y), len(z)];
    let (ux, mut uy, mut uz) = (unit(x), unit(y), unit(z));
    // make the frame square and right-handed; a mirror goes into the z size
    uy = unit(sub(uy, mul(ux, dot(ux, uy))));
    let want = cross(ux, uy);
    if dot(want, uz) < 0.0 {
        size[2] = -size[2];
    }
    uz = want;
    // R = Rz * Ry * Rx with columns ux, uy, uz
    let sy = -ux[2];
    let (rx, ry, rz);
    if sy.abs() < 0.999_999 {
        ry = sy.asin();
        rx = uy[2].atan2(uz[2]);
        rz = ux[1].atan2(ux[0]);
    } else {
        ry = sy.signum() * std::f64::consts::FRAC_PI_2;
        rx = (-uz[1]).atan2(uy[1]);
        rz = 0.0;
    }
    Trs { size, degrees: [rx.to_degrees(), ry.to_degrees(), rz.to_degrees()], position: origin(m) }
}

pub fn recompose(t: &Trs) -> M12 {
    let rx = turn([1.0, 0.0, 0.0], t.degrees[0].to_radians());
    let ry = turn([0.0, 1.0, 0.0], t.degrees[1].to_radians());
    let rz = turn([0.0, 0.0, 1.0], t.degrees[2].to_radians());
    let r = compose(&rz, &compose(&ry, &rx));
    let mut m = compose(&r, &scaling(t.size));
    m[9] = t.position[0];
    m[10] = t.position[1];
    m[11] = t.position[2];
    m
}

// --------------------------------------------------------------------------- camera

/// The camera as the scene files hold it: a frame at a point, an angle and the
/// picture's zoom and pan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cam {
    pub eye: V3,
    pub right: V3,
    pub up: V3,
    pub forward: V3,
    /// The stored field of view, radians (the picture's width spans 0.8 of it).
    pub fov: f64,
    pub zoom: [f64; 2],
    pub pan: [f64; 2],
    pub aspect: f64,
    pub scale: f64,
    /// The point the trackball turns about.
    pub pivot: V3,
}

impl Default for Cam {
    fn default() -> Self {
        let mut c = Cam {
            eye: [10.0, 3.0, -10.0],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            forward: [0.0, 0.0, 1.0],
            fov: 60f64.to_radians(),
            zoom: [1.0, 1.0],
            pan: [0.0, 0.0],
            aspect: 1.0,
            scale: 1.0,
            pivot: [0.0, 0.0, 0.0],
        };
        c.look_at([0.0, 1.0, 0.0]);
        c
    }
}

impl Cam {
    /// Tangents of half the angles the picture's width and height span.
    pub fn half_tans(&self, size: (u32, u32)) -> (f64, f64) {
        let ht = (0.8 * self.fov / 2.0).tan();
        let ratio = size.0.max(1) as f64 / size.1.max(1) as f64;
        (ht * self.zoom[0], ht * self.zoom[1] / (ratio * self.aspect))
    }

    /// Aim at a world point, keeping the horizon level.
    pub fn look_at(&mut self, target: V3) {
        let f = unit(sub(target, self.eye));
        if len(f) < 1e-12 {
            return;
        }
        let world_up = [0.0, 1.0, 0.0];
        let mut r = cross(world_up, f);
        if len(r) < 1e-9 {
            r = self.right;
        }
        self.forward = f;
        self.right = unit(r);
        self.up = unit(cross(self.forward, self.right));
    }

    /// A world point in the camera's frame (right, up, forward).
    pub fn to_camera(&self, p: V3) -> V3 {
        let d = sub(p, self.eye);
        [dot(d, self.right), dot(d, self.up), dot(d, self.forward)]
    }

    /// A world point in the picture, 0..1 across and down; `None` behind the eye.
    pub fn project(&self, p: V3, size: (u32, u32)) -> Option<[f32; 2]> {
        let c = self.to_camera(p);
        if c[2] <= 1e-6 {
            return None;
        }
        let (tx, ty) = self.half_tans(size);
        Some([(0.5 + 0.5 * (c[0] / c[2]) / tx) as f32, (0.5 - 0.5 * (c[1] / c[2]) / ty) as f32])
    }

    /// The ray through the picture point (0..1 across and down): its unit direction.
    pub fn ray(&self, x: f32, y: f32, size: (u32, u32)) -> V3 {
        let (tx, ty) = self.half_tans(size);
        let dx = (x as f64 * 2.0 - 1.0) * tx;
        let dy = (1.0 - y as f64 * 2.0) * ty;
        unit(add(add(self.forward, mul(self.right, dx)), mul(self.up, dy)))
    }

    fn turn_frame(&mut self, op: &M12) {
        self.right = unit(dir(op, self.right));
        self.up = unit(dir(op, self.up));
        self.forward = unit(dir(op, self.forward));
    }

    /// Trackball: swing the camera about the pivot (yaw about the world's up,
    /// pitch about the camera's right).
    pub fn orbit(&mut self, yaw: f64, pitch: f64) {
        let y = turn([0.0, 1.0, 0.0], yaw);
        self.eye = add(self.pivot, dir(&y, sub(self.eye, self.pivot)));
        self.turn_frame(&y);
        // do not go over the top
        let p = turn(self.right, pitch);
        let next_forward = unit(dir(&p, self.forward));
        if next_forward[1].abs() < 0.9995 {
            self.eye = add(self.pivot, dir(&p, sub(self.eye, self.pivot)));
            self.turn_frame(&p);
        }
    }

    /// Turn the camera in place (look around).
    pub fn look(&mut self, yaw: f64, pitch: f64) {
        let y = turn([0.0, 1.0, 0.0], yaw);
        self.turn_frame(&y);
        let p = turn(self.right, pitch);
        let next_forward = unit(dir(&p, self.forward));
        if next_forward[1].abs() < 0.9995 {
            self.turn_frame(&p);
        }
    }

    pub fn slide(&mut self, by: V3) {
        self.eye = add(self.eye, by);
    }

    /// Move across the picture plane.
    pub fn pan_view(&mut self, right: f64, up: f64) {
        self.slide(add(mul(self.right, right), mul(self.up, up)));
    }

    /// Move over the ground: sideways and forward, staying level.
    pub fn pan_ground(&mut self, right: f64, ahead: f64) {
        let flat = unit([self.forward[0], 0.0, self.forward[2]]);
        let side = unit([self.right[0], 0.0, self.right[2]]);
        self.slide(add(mul(side, right), mul(flat, ahead)));
    }

    pub fn dolly(&mut self, ahead: f64) {
        self.slide(mul(self.forward, ahead));
    }

    pub fn bank(&mut self, angle: f64) {
        let r = turn(self.forward, angle);
        self.right = unit(dir(&r, self.right));
        self.up = unit(dir(&r, self.up));
    }

    pub fn distance_to_pivot(&self) -> f64 {
        len(sub(self.pivot, self.eye)).max(1e-6)
    }

    /// A blend of two cameras (for animation and smooth moves).
    pub fn blend(a: &Cam, b: &Cam, t: f64) -> Cam {
        let mut c = a.clone();
        c.eye = lerp(a.eye, b.eye, t);
        c.pivot = lerp(a.pivot, b.pivot, t);
        c.forward = unit(lerp(a.forward, b.forward, t));
        let up = unit(lerp(a.up, b.up, t));
        c.right = unit(cross(up, c.forward));
        c.up = unit(cross(c.forward, c.right));
        c.fov = a.fov + (b.fov - a.fov) * t;
        for k in 0..2 {
            c.zoom[k] = a.zoom[k] + (b.zoom[k] - a.zoom[k]) * t;
            c.pan[k] = a.pan[k] + (b.pan[k] - a.pan[k]) * t;
        }
        c
    }
}

/// A ray against the box `lo..hi` in the placement's own space: the distance
/// along the (world) ray to the entry, or `None`.
pub fn ray_box(m: &M12, lo: V3, hi: V3, from: V3, toward: V3) -> Option<f64> {
    let inv = inverse(m)?;
    let o = place(&inv, from);
    let d = dir(&inv, toward);
    let (mut t0, mut t1) = (0.0f64, f64::INFINITY);
    for k in 0..3 {
        if d[k].abs() < 1e-300 {
            if o[k] < lo[k] || o[k] > hi[k] {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo[k] - o[k]) / d[k], (hi[k] - o[k]) / d[k]);
        let (a, b) = if a < b { (a, b) } else { (b, a) };
        t0 = t0.max(a);
        t1 = t1.min(b);
        if t0 > t1 {
            return None;
        }
    }
    Some(t0)
}

/// Where the ray meets the plane through `point` with unit `normal`.
pub fn ray_plane(from: V3, toward: V3, point: V3, normal: V3) -> Option<V3> {
    let denom = dot(toward, normal);
    if denom.abs() < 1e-12 {
        return None;
    }
    let t = dot(sub(point, from), normal) / denom;
    (t > 0.0).then(|| add(from, mul(toward, t)))
}
