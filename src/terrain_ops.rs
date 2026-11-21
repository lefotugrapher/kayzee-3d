//! Height-grid work for the Terrain Editor: making new relief, the elevation
//! and filter tools, brushes, pictures in and out, and resolution changes.
//! Grids are n by n heights, 0..1 here (stored as 0..65535 in the scene).

#[derive(Clone)]
pub struct Grid {
    pub n: usize,
    pub h: Vec<f32>,
}

impl Grid {
    pub fn flat(n: usize, v: f32) -> Grid {
        Grid { n, h: vec![v; n * n] }
    }
    pub fn from_u16(n: usize, src: &[u16]) -> Grid {
        Grid { n, h: src.iter().map(|v| *v as f32 / 65535.0).collect() }
    }
    pub fn to_u16(&self) -> Vec<u16> {
        self.h.iter().map(|v| (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16).collect()
    }
    #[inline]
    pub fn at(&self, i: usize, j: usize) -> f32 {
        self.h[j * self.n + i]
    }
    #[inline]
    fn at_clamped(&self, i: isize, j: isize) -> f32 {
        let n = self.n as isize;
        self.h[(j.clamp(0, n - 1) * n + i.clamp(0, n - 1)) as usize]
    }
    /// Height at a fractional grid position (bilinear).
    pub fn sample(&self, x: f32, y: f32) -> f32 {
        let m = (self.n - 1) as f32;
        let (x, y) = (x.clamp(0.0, m), y.clamp(0.0, m));
        let (i, j) = (x.floor() as usize, y.floor() as usize);
        let (i1, j1) = ((i + 1).min(self.n - 1), (j + 1).min(self.n - 1));
        let (fx, fy) = (x - i as f32, y - j as f32);
        let a = self.at(i, j) * (1.0 - fx) + self.at(i1, j) * fx;
        let b = self.at(i, j1) * (1.0 - fx) + self.at(i1, j1) * fx;
        a * (1.0 - fy) + b * fy
    }
    pub fn range(&self) -> (f32, f32) {
        self.h.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| (lo.min(*v), hi.max(*v)))
    }
    pub fn clamp01(&mut self) {
        for v in &mut self.h {
            *v = v.clamp(0.0, 1.0);
        }
    }
}

// --------------------------------------------------------------------------- noise

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }
}

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6b343) ^ (y as u32).wrapping_mul(0xd8163841) ^ seed.wrapping_mul(0xcb1ab31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1e995);
    h ^= h >> 15;
    (h & 0xffffff) as f32 / 0xffffff as f32
}

fn smooth(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Value noise, 0..1, with period `period` cells so it tiles.
fn noise(x: f32, y: f32, seed: u32, period: i32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (smooth(x - xi as f32), smooth(y - yi as f32));
    let w = |a: i32| a.rem_euclid(period.max(1));
    let v00 = hash(w(xi), w(yi), seed);
    let v10 = hash(w(xi + 1), w(yi), seed);
    let v01 = hash(w(xi), w(yi + 1), seed);
    let v11 = hash(w(xi + 1), w(yi + 1), seed);
    let a = v00 + (v10 - v00) * fx;
    let b = v01 + (v11 - v01) * fx;
    a + (b - a) * fy
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fractal {
    /// Rolling hills: smooth sums of noise.
    Standard,
    /// Sharp crests: ridged sums.
    Ridges,
    /// Soft puffy mounds.
    Billows,
    /// Plateaus with cliffs.
    Mesas,
    /// The midpoint-displacement look.
    Rough,
    /// Steep peaks rising from flats.
    Peaks,
}

impl Fractal {
    pub const ALL: [Fractal; 6] = [Fractal::Standard, Fractal::Ridges, Fractal::Billows, Fractal::Mesas, Fractal::Rough, Fractal::Peaks];
    pub fn label(self) -> &'static str {
        match self {
            Fractal::Standard => "Standard",
            Fractal::Ridges => "Ridges",
            Fractal::Billows => "Billows",
            Fractal::Mesas => "Mesas",
            Fractal::Rough => "Rough",
            Fractal::Peaks => "Peaks",
        }
    }
}

/// New relief: `detail` is how many octaves (1..10), `scale` the size of the
/// largest features (1 = one across the grid).
pub fn fractal(n: usize, kind: Fractal, seed: u64, detail: u32, scale: f32) -> Grid {
    if kind == Fractal::Rough {
        return diamond_square(n, seed, 0.55);
    }
    let s = seed as u32;
    let mut g = Grid::flat(n, 0.0);
    let base = (scale.max(0.25)).round().max(1.0) as i32;
    for j in 0..n {
        for i in 0..n {
            let (u, v) = (i as f32 / n as f32, j as f32 / n as f32);
            let (mut sum, mut amp, mut freq, mut total) = (0.0f32, 1.0f32, base, 0.0f32);
            let mut weight = 1.0f32;
            for o in 0..detail.clamp(1, 10) {
                let raw = noise(u * freq as f32, v * freq as f32, s.wrapping_add(o * 977), freq);
                let val = match kind {
                    Fractal::Ridges => {
                        let r = 1.0 - (raw * 2.0 - 1.0).abs();
                        let r = r * r * weight;
                        weight = (r * 2.0).clamp(0.0, 1.0);
                        r
                    }
                    Fractal::Billows => (raw * 2.0 - 1.0).abs(),
                    _ => raw,
                };
                sum += val * amp;
                total += amp;
                amp *= 0.5;
                freq *= 2;
            }
            let mut h = sum / total.max(1e-6);
            h = match kind {
                Fractal::Mesas => {
                    let steps = 4.0;
                    let t = h * steps;
                    let k = t.floor();
                    let f = t - k;
                    (k + smooth((f - 0.75).max(0.0) * 4.0)) / steps
                }
                Fractal::Peaks => h.powf(2.6),
                _ => h,
            };
            g.h[j * n + i] = h;
        }
    }
    normalise(&mut g);
    g
}

fn diamond_square(n: usize, seed: u64, rough: f32) -> Grid {
    let mut size = 1usize;
    while size + 1 < n {
        size *= 2;
    }
    let m = size + 1;
    let mut r = Rng::new(seed);
    let mut h = vec![0.0f32; m * m];
    let mut step = size;
    let mut amp = 0.5f32;
    for c in [0, size, size * m, size * m + size] {
        h[c] = r.unit();
    }
    while step > 1 {
        let half = step / 2;
        let mut y = 0;
        while y < size {
            let mut x = 0;
            while x < size {
                let avg = (h[y * m + x] + h[y * m + x + step] + h[(y + step) * m + x] + h[(y + step) * m + x + step]) / 4.0;
                h[(y + half) * m + x + half] = avg + r.signed() * amp;
                x += step;
            }
            y += step;
        }
        let mut y = 0;
        while y <= size {
            let mut x = (y / half % 2) * half;
            if (y / half) % 2 == 0 {
                x = half;
            }
            while x <= size {
                let mut sum = 0.0;
                let mut cnt = 0.0;
                for (dx, dy) in [(-(half as isize), 0isize), (half as isize, 0), (0, -(half as isize)), (0, half as isize)] {
                    let (xx, yy) = (x as isize + dx, y as isize + dy);
                    if xx >= 0 && yy >= 0 && (xx as usize) <= size && (yy as usize) <= size {
                        sum += h[yy as usize * m + xx as usize];
                        cnt += 1.0;
                    }
                }
                h[y * m + x] = sum / cnt + r.signed() * amp;
                x += step;
            }
            y += half;
        }
        step = half;
        amp *= rough;
    }
    let big = Grid { n: m, h };
    let mut g = resample(&big, n);
    normalise(&mut g);
    g
}

pub fn normalise(g: &mut Grid) {
    let (lo, hi) = g.range();
    let span = (hi - lo).max(1e-6);
    for v in &mut g.h {
        *v = (*v - lo) / span;
    }
}

// --------------------------------------------------------------------------- whole-grid tools

pub fn resample(g: &Grid, n: usize) -> Grid {
    let mut out = Grid::flat(n, 0.0);
    let k = (g.n - 1) as f32 / (n.max(2) - 1) as f32;
    for j in 0..n {
        for i in 0..n {
            out.h[j * n + i] = g.sample(i as f32 * k, j as f32 * k);
        }
    }
    out
}

/// Blur with a box of `radius` cells, `passes` times (three passes is close to Gaussian).
pub fn blur(g: &Grid, radius: usize, passes: usize) -> Grid {
    let n = g.n;
    let mut cur = g.h.clone();
    let mut tmp = vec![0.0f32; n * n];
    let r = radius.max(1) as isize;
    for _ in 0..passes.max(1) {
        for j in 0..n {
            let mut acc = 0.0;
            for k in -r..=r {
                acc += cur[j * n + k.clamp(0, n as isize - 1) as usize];
            }
            for i in 0..n {
                tmp[j * n + i] = acc / (2 * r + 1) as f32;
                let out = (i as isize - r).clamp(0, n as isize - 1) as usize;
                let inn = (i as isize + r + 1).clamp(0, n as isize - 1) as usize;
                acc += cur[j * n + inn] - cur[j * n + out];
            }
        }
        for i in 0..n {
            let mut acc = 0.0;
            for k in -r..=r {
                acc += tmp[k.clamp(0, n as isize - 1) as usize * n + i];
            }
            for j in 0..n {
                cur[j * n + i] = acc / (2 * r + 1) as f32;
                let out = (j as isize - r).clamp(0, n as isize - 1) as usize;
                let inn = (j as isize + r + 1).clamp(0, n as isize - 1) as usize;
                acc += tmp[inn * n + i] - tmp[out * n + i];
            }
        }
    }
    Grid { n, h: cur }
}

pub fn smooth_grid(g: &mut Grid, amount: f32) {
    let b = blur(g, ((g.n as f32 / 128.0).ceil() as usize).max(1), 2);
    for (v, s) in g.h.iter_mut().zip(&b.h) {
        *v += (s - *v) * amount.clamp(0.0, 1.0);
    }
}

pub fn sharpen(g: &mut Grid, amount: f32) {
    let b = blur(g, ((g.n as f32 / 128.0).ceil() as usize).max(1), 2);
    for (v, s) in g.h.iter_mut().zip(&b.h) {
        *v += (*v - s) * amount * 2.0;
    }
    g.clamp01();
}

/// Lift or drop everything.
pub fn raise(g: &mut Grid, by: f32) {
    for v in &mut g.h {
        *v += by;
    }
    g.clamp01();
}

/// Flatten toward the middle height (amount > 0) or stretch away (amount < 0).
pub fn dampen(g: &mut Grid, amount: f32) {
    let (lo, hi) = g.range();
    let mid = 0.5 * (lo + hi);
    for v in &mut g.h {
        *v = mid + (*v - mid) * (1.0 - amount).max(0.0);
    }
    g.clamp01();
}

pub fn invert(g: &mut Grid) {
    for v in &mut g.h {
        *v = 1.0 - *v;
    }
}

/// Spread the heights so every level is used equally.
pub fn equalize(g: &mut Grid) {
    let mut sorted: Vec<(f32, usize)> = g.h.iter().copied().zip(0..).collect();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let count = sorted.len().max(2) - 1;
    for (rank, (_, idx)) in sorted.into_iter().enumerate() {
        g.h[idx] = rank as f32 / count as f32;
    }
}

/// Spikes: random needles.
pub fn spikes(g: &mut Grid, amount: f32, seed: u64) {
    let mut r = Rng::new(seed);
    let count = (g.n * g.n / 60).max(1);
    for _ in 0..count {
        let i = (r.next() as usize) % g.h.len();
        g.h[i] += amount * r.unit();
    }
    g.clamp01();
}

/// Bumps: soft mounds dropped at random.
pub fn mounds(g: &mut Grid, amount: f32, seed: u64) {
    let mut r = Rng::new(seed);
    let n = g.n as f32;
    for _ in 0..24 {
        let (cx, cy) = (r.unit() * n, r.unit() * n);
        let rad = (0.04 + 0.12 * r.unit()) * n;
        let height = amount * (0.3 + 0.7 * r.unit());
        stamp(g, cx, cy, rad, |d| height * smooth((1.0 - d).max(0.0)));
    }
    g.clamp01();
}

/// Holes: the inverse of mounds.
pub fn pits(g: &mut Grid, amount: f32, seed: u64) {
    invert(g);
    mounds(g, amount, seed);
    invert(g);
}

fn stamp(g: &mut Grid, cx: f32, cy: f32, rad: f32, f: impl Fn(f32) -> f32) {
    let n = g.n as isize;
    let r = rad.ceil() as isize + 1;
    for j in (cy as isize - r).max(0)..(cy as isize + r).min(n) {
        for i in (cx as isize - r).max(0)..(cx as isize + r).min(n) {
            let d = ((i as f32 - cx).powi(2) + (j as f32 - cy).powi(2)).sqrt() / rad.max(1e-3);
            if d < 1.0 {
                g.h[(j * n + i) as usize] += f(d);
            }
        }
    }
}

/// Erode: water flowing downhill carries material and drops it lower down.
pub fn erode(g: &mut Grid, amount: f32, seed: u64) {
    let n = g.n;
    let mut r = Rng::new(seed);
    let drops = ((n * n) as f32 * 0.6 * amount.clamp(0.05, 3.0)) as usize;
    let (inertia, capacity, deposit, erosion, evaporate) = (0.05f32, 6.0f32, 0.3f32, 0.3f32, 0.02f32);
    let scale = 1.0 / 255.0 * (n as f32 / 128.0).max(0.5);
    for _ in 0..drops {
        let (mut x, mut y) = (r.unit() * (n - 2) as f32, r.unit() * (n - 2) as f32);
        let (mut dx, mut dy, mut speed, mut water, mut load) = (0.0f32, 0.0f32, 1.0f32, 1.0f32, 0.0f32);
        for _ in 0..64 {
            let (i, j) = (x as usize, y as usize);
            if i + 1 >= n || j + 1 >= n {
                break;
            }
            let (fx, fy) = (x - i as f32, y - j as f32);
            let (a, b, c, d) = (g.at(i, j), g.at(i + 1, j), g.at(i, j + 1), g.at(i + 1, j + 1));
            let gx = (b - a) * (1.0 - fy) + (d - c) * fy;
            let gy = (c - a) * (1.0 - fx) + (d - b) * fx;
            let here = a * (1.0 - fx) * (1.0 - fy) + b * fx * (1.0 - fy) + c * (1.0 - fx) * fy + d * fx * fy;
            dx = dx * inertia - gx * (1.0 - inertia);
            dy = dy * inertia - gy * (1.0 - inertia);
            let l = (dx * dx + dy * dy).sqrt();
            if l < 1e-9 {
                break;
            }
            dx /= l;
            dy /= l;
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0.0 || ny < 0.0 || nx >= (n - 1) as f32 || ny >= (n - 1) as f32 {
                break;
            }
            let there = g.sample(nx, ny);
            let drop = there - here;
            let cap = (-drop).max(0.0005) * speed * water * capacity;
            let put = |g: &mut Grid, v: f32| {
                let w = [(1.0 - fx) * (1.0 - fy), fx * (1.0 - fy), (1.0 - fx) * fy, fx * fy];
                g.h[j * n + i] += v * w[0];
                g.h[j * n + i + 1] += v * w[1];
                g.h[(j + 1) * n + i] += v * w[2];
                g.h[(j + 1) * n + i + 1] += v * w[3];
            };
            if load > cap || drop > 0.0 {
                let amount = if drop > 0.0 { drop.min(load) } else { (load - cap) * deposit };
                load -= amount;
                put(g, amount);
            } else {
                let take = ((cap - load) * erosion).min(-drop).min(scale * 8.0);
                load += take;
                put(g, -take);
            }
            speed = (speed * speed + drop.abs() * 4.0).sqrt().min(8.0);
            water *= 1.0 - evaporate;
            x = nx;
            y = ny;
        }
    }
    g.clamp01();
}

/// Terraces: steps of equal height.
pub fn terrace(g: &mut Grid, steps: u32, sharpness: f32) {
    let s = steps.max(2) as f32;
    for v in &mut g.h {
        let t = *v * s;
        let k = t.floor();
        let f = t - k;
        let edge = smooth(((f - (1.0 - sharpness.clamp(0.02, 1.0))) / sharpness.clamp(0.02, 1.0)).clamp(0.0, 1.0));
        *v = ((k + edge) / s).clamp(0.0, 1.0);
    }
}

/// Fade the outer `width` share of the grid down to `to`, with a smooth (round)
/// or straight (square) edge.
pub fn edges(g: &mut Grid, width: f32, to: f32, round: bool) {
    let n = g.n as f32;
    for j in 0..g.n {
        for i in 0..g.n {
            let (u, v) = ((i as f32 + 0.5) / n * 2.0 - 1.0, (j as f32 + 0.5) / n * 2.0 - 1.0);
            let d = if round { (u * u + v * v).sqrt() } else { u.abs().max(v.abs()) };
            let w = width.clamp(0.01, 1.0);
            let k = smooth(((1.0 - d) / w).clamp(0.0, 1.0));
            let h = &mut g.h[j * g.n + i];
            *h = to + (*h - to) * k;
        }
    }
}

/// Set the outermost ring of cells to zero (a clean base for islands).
pub fn zero_edges(g: &mut Grid) {
    let n = g.n;
    for k in 0..n {
        g.h[k] = 0.0;
        g.h[(n - 1) * n + k] = 0.0;
        g.h[k * n] = 0.0;
        g.h[k * n + n - 1] = 0.0;
    }
}

/// Bend the heights through a curve given as points (x, y) in 0..1.
pub fn curve(g: &mut Grid, points: &[(f32, f32)]) {
    if points.len() < 2 {
        return;
    }
    let map = |x: f32| {
        let i = points.iter().position(|p| p.0 >= x).unwrap_or(points.len() - 1).max(1);
        let (a, b) = (points[i - 1], points[i]);
        let t = ((x - a.0) / (b.0 - a.0).max(1e-6)).clamp(0.0, 1.0);
        a.1 + (b.1 - a.1) * t
    };
    for v in &mut g.h {
        *v = map(*v).clamp(0.0, 1.0);
    }
}

/// Cut everything outside low..high flat at those levels.
pub fn clip(g: &mut Grid, low: f32, high: f32) {
    for v in &mut g.h {
        *v = v.clamp(low, high);
    }
}

pub fn mirror(g: &mut Grid, horizontal: bool) {
    let n = g.n;
    for j in 0..n {
        for i in 0..n / 2 {
            if horizontal {
                g.h.swap(j * n + i, j * n + n - 1 - i);
            } else {
                g.h.swap(i * n + j, (n - 1 - i) * n + j);
            }
        }
    }
}

pub fn rotate90(g: &mut Grid) {
    let n = g.n;
    let old = g.h.clone();
    for j in 0..n {
        for i in 0..n {
            g.h[j * n + i] = old[(n - 1 - i) * n + j];
        }
    }
}

/// Mix `other` (resampled to this grid) in: mode 0 replace, 1 add, 2 subtract,
/// 3 highest, 4 lowest, 5 multiply.
pub fn blend(g: &mut Grid, other: &Grid, mode: u8, amount: f32) {
    let o = if other.n == g.n { other.clone() } else { resample(other, g.n) };
    for (v, w) in g.h.iter_mut().zip(&o.h) {
        let target = match mode {
            1 => *v + *w,
            2 => *v - *w,
            3 => v.max(*w),
            4 => v.min(*w),
            5 => *v * *w,
            _ => *w,
        };
        *v += (target - *v) * amount.clamp(0.0, 1.0);
    }
    g.clamp01();
}

// --------------------------------------------------------------------------- brushes

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Brush {
    Raise,
    Lower,
    Smooth,
    Flatten,
    Noise,
}

impl Brush {
    pub const ALL: [Brush; 5] = [Brush::Raise, Brush::Lower, Brush::Smooth, Brush::Flatten, Brush::Noise];
    pub fn label(self) -> &'static str {
        match self {
            Brush::Raise => "Raise",
            Brush::Lower => "Lower",
            Brush::Smooth => "Smooth",
            Brush::Flatten => "Flatten",
            Brush::Noise => "Roughen",
        }
    }
}

/// One dab of the brush at grid position (cx, cy). `size` is the radius in
/// cells, `strength` 0..1, `softness` 0 (hard) .. 1 (soft), `level` the height
/// a Flatten brush pulls toward.
pub fn dab(g: &mut Grid, brush: Brush, cx: f32, cy: f32, size: f32, strength: f32, softness: f32, level: f32, seed: u64) {
    let n = g.n as isize;
    let r = size.max(0.5);
    let reach = r.ceil() as isize + 1;
    let fall = |d: f32| {
        let hard = (1.0 - d).max(0.0);
        let soft = smooth(hard);
        hard.powf(0.2) * (1.0 - softness) + soft * softness
    };
    let src = if brush == Brush::Smooth { Some(blur(g, (r / 3.0).ceil().max(1.0) as usize, 2)) } else { None };
    let mut rng = Rng::new(seed);
    for j in (cy as isize - reach).max(0)..(cy as isize + reach).min(n) {
        for i in (cx as isize - reach).max(0)..(cx as isize + reach).min(n) {
            let d = ((i as f32 - cx).powi(2) + (j as f32 - cy).powi(2)).sqrt() / r;
            if d >= 1.0 {
                continue;
            }
            let w = fall(d) * strength;
            let k = (j * n + i) as usize;
            let v = g.h[k];
            g.h[k] = match brush {
                Brush::Raise => v + w * 0.02,
                Brush::Lower => v - w * 0.02,
                Brush::Smooth => v + (src.as_ref().unwrap().h[k] - v) * w,
                Brush::Flatten => v + (level - v) * w * 0.5,
                Brush::Noise => v + rng.signed() * w * 0.01,
            };
        }
    }
    g.clamp01();
}

// --------------------------------------------------------------------------- pictures

/// Heights from a picture's brightness.
pub fn from_picture(path: &std::path::Path, n: usize) -> Result<Grid, String> {
    let img = image::open(path).map_err(|e| format!("That picture could not be read: {e}"))?.to_luma16();
    let (w, h) = img.dimensions();
    let src = Grid { n: w.max(h) as usize, h: Vec::new() };
    let _ = src;
    let mut out = Grid::flat(n, 0.0);
    for j in 0..n {
        for i in 0..n {
            let x = (i as f32 + 0.5) / n as f32 * w as f32;
            let y = (j as f32 + 0.5) / n as f32 * h as f32;
            let p = img.get_pixel((x as u32).min(w - 1), (y as u32).min(h - 1));
            out.h[j * n + i] = p.0[0] as f32 / 65535.0;
        }
    }
    Ok(out)
}

/// The grid as a 16-bit grey picture.
pub fn to_picture(g: &Grid, path: &std::path::Path) -> Result<(), String> {
    let n = g.n as u32;
    let mut img = image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::new(n, n);
    for (k, v) in g.to_u16().into_iter().enumerate() {
        img.put_pixel(k as u32 % n, k as u32 / n, image::Luma([v]));
    }
    img.save(path).map_err(|e| format!("The picture could not be saved: {e}"))
}

/// Shaded relief as RGBA for the editor's top view: grey by height, lit from
/// the upper left, coloured by `gradient` when given (pairs of height, colour).
pub fn relief_rgba(g: &Grid, gradient: &[(f32, [u8; 3])]) -> Vec<u8> {
    let n = g.n;
    let mut out = vec![0u8; n * n * 4];
    for j in 0..n {
        for i in 0..n {
            let h = g.at(i, j);
            let dx = g.at_clamped(i as isize + 1, j as isize) - g.at_clamped(i as isize - 1, j as isize);
            let dy = g.at_clamped(i as isize, j as isize + 1) - g.at_clamped(i as isize, j as isize - 1);
            let light = (0.75 - (dx + dy) * n as f32 * 0.35).clamp(0.25, 1.15);
            let base = if gradient.len() >= 2 {
                let k = gradient.iter().position(|p| p.0 >= h).unwrap_or(gradient.len() - 1).max(1);
                let (a, b) = (gradient[k - 1], gradient[k]);
                let t = ((h - a.0) / (b.0 - a.0).max(1e-6)).clamp(0.0, 1.0);
                [0, 1, 2].map(|c| a.1[c] as f32 + (b.1[c] as f32 - a.1[c] as f32) * t)
            } else {
                [h * 255.0; 3]
            };
            let px = &mut out[(j * n + i) * 4..(j * n + i) * 4 + 4];
            for c in 0..3 {
                px[c] = (base[c] * light).clamp(0.0, 255.0) as u8;
            }
            px[3] = 255;
        }
    }
    out
}

pub const GRADIENTS: [(&str, &[(f32, [u8; 3])]); 5] = [
    ("Grey", &[(0.0, [20, 20, 20]), (1.0, [250, 250, 250])]),
    ("Earth", &[(0.0, [40, 70, 120]), (0.18, [70, 120, 170]), (0.2, [190, 180, 130]), (0.35, [80, 130, 60]), (0.65, [120, 100, 70]), (0.85, [170, 160, 150]), (1.0, [255, 255, 255])]),
    ("Desert", &[(0.0, [110, 60, 30]), (0.5, [210, 150, 90]), (1.0, [250, 225, 180])]),
    ("Snow", &[(0.0, [60, 70, 90]), (0.6, [150, 160, 175]), (0.75, [235, 240, 248]), (1.0, [255, 255, 255])]),
    ("Heat", &[(0.0, [10, 10, 60]), (0.33, [120, 20, 140]), (0.66, [240, 120, 30]), (1.0, [255, 250, 200])]),
];
