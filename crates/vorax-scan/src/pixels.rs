//! Pixel helpers for the screenshot reader (src/solver/scan/pixels.ts). JavaScript's number rules are kept where
//! they change results: image bytes round half to even (Uint8ClampedArray), grey values are 32-bit floats
//! (Float32Array), Math.round rounds halves up, Math.hypot sums with Kahan's compensation.

#[derive(Clone, Debug)]
pub struct Img {
    pub width: usize,
    pub height: usize,
    /// RGBA, row by row
    pub data: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct ArtFeature {
    pub hist: Vec<f64>,
    pub grid: Vec<f64>,
}

/// JavaScript's Math.round: halves up.
#[inline]
pub fn js_round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 {
        f + 1.0
    } else {
        f
    }
}

/// A number stored into a Uint8ClampedArray: clamped, rounded half to even.
#[inline]
pub fn clamp_u8(x: f64) -> u8 {
    if x.is_nan() || x <= 0.0 {
        return 0;
    }
    if x >= 255.0 {
        return 255;
    }
    let f = x.floor();
    let d = x - f;
    let r = if d < 0.5 {
        f
    } else if d > 0.5 {
        f + 1.0
    } else if f % 2.0 == 0.0 {
        f
    } else {
        f + 1.0
    };
    r as u8
}

/// V8's Math.hypot: scaled by the largest value, squares summed with Kahan's compensation.
pub fn js_hypot(v: &[f64]) -> f64 {
    let mut max = 0.0f64;
    for &x in v {
        let a = x.abs();
        if a.is_infinite() {
            return f64::INFINITY;
        }
        if a > max {
            max = a;
        }
    }
    if max == 0.0 {
        return 0.0;
    }
    let (mut sum, mut comp) = (0.0f64, 0.0f64);
    for &x in v {
        let n = x.abs() / max;
        let summand = n * n - comp;
        let pre = sum + summand;
        comp = (pre - sum) - summand;
        sum = pre;
    }
    sum.sqrt() * max
}

impl Img {
    pub fn new(width: usize, height: usize) -> Img {
        Img { width, height, data: vec![0; width * height * 4] }
    }

    #[inline]
    pub fn px(&self, x: usize, y: usize) -> (u8, u8, u8) {
        let i = (y * self.width + x) * 4;
        (self.data[i], self.data[i + 1], self.data[i + 2])
    }
}

pub fn gray(img: &Img) -> Vec<f32> {
    let d = &img.data;
    (0..img.width * img.height).map(|i| (0.299 * d[i * 4] as f64 + 0.587 * d[i * 4 + 1] as f64 + 0.114 * d[i * 4 + 2] as f64) as f32).collect()
}

/// Hue 0-360, saturation and value 0-1.
#[inline]
pub fn hsv(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let (r, g, b) = (r as f64, g as f64, b as f64);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let mut h = 0.0;
    if d != 0.0 {
        h = if max == r {
            ((g - b) / d) % 6.0
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        h *= 60.0;
        if h < 0.0 {
            h += 360.0;
        }
    }
    (h, if max != 0.0 { d / max } else { 0.0 }, max / 255.0)
}

pub fn crop(img: &Img, r: Rect) -> Img {
    let x0 = js_round(r.x).max(0.0) as usize;
    let y0 = js_round(r.y).max(0.0) as usize;
    let w = ((img.width as f64 - x0 as f64).min(js_round(r.w))).max(1.0) as usize;
    let h = ((img.height as f64 - y0 as f64).min(js_round(r.h))).max(1.0) as usize;
    let mut out = Img::new(w, h);
    for y in 0..h {
        let src = ((y0 + y) * img.width + x0) * 4;
        let n = (w * 4).min(img.data.len().saturating_sub(src));
        out.data[y * w * 4..y * w * 4 + n].copy_from_slice(&img.data[src..src + n]);
    }
    out
}

/// Bilinear resize.
pub fn resize(img: &Img, w: usize, h: usize) -> Img {
    let mut out = Img::new(w, h);
    let sx = img.width as f64 / w as f64;
    let sy = img.height as f64 / h as f64;
    let iw = img.width;
    for y in 0..h {
        let fy = ((y as f64 + 0.5) * sy - 0.5).max(0.0).min(img.height as f64 - 1.0);
        let y0 = fy.floor() as usize;
        let y1 = (img.height - 1).min(y0 + 1);
        let ty = fy - y0 as f64;
        for x in 0..w {
            let fx = ((x as f64 + 0.5) * sx - 0.5).max(0.0).min(img.width as f64 - 1.0);
            let x0 = fx.floor() as usize;
            let x1 = (img.width - 1).min(x0 + 1);
            let tx = fx - x0 as f64;
            for c in 0..4 {
                let p = |xx: usize, yy: usize| img.data[(yy * iw + xx) * 4 + c] as f64;
                let a = p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx;
                let b = p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx;
                out.data[(y * w + x) * 4 + c] = clamp_u8(a * (1.0 - ty) + b * ty);
            }
        }
    }
    out
}

#[inline]
fn sample(g: &[f32], w: usize, h: usize, x: f64, y: f64) -> f64 {
    let x = (w as f64 - 1.001).min(x.max(0.0));
    let y = (h as f64 - 1.001).min(y.max(0.0));
    let xi = x as usize;
    let yi = y as usize;
    let fx = x - xi as f64;
    let fy = y - yi as f64;
    let i = yi * w + xi;
    let g = |k: usize| g[k] as f64;
    (g(i) * (1.0 - fx) + g(i + 1) * fx) * (1.0 - fy) + (g(i + w) * (1.0 - fx) + g(i + w + 1) * fx) * fy
}

pub const GLYPH_W: usize = 12;
pub const GLYPH_H: usize = 16;

/// A glyph as a small zero-mean, unit-length grid, scaled by the line height so a narrow 1 stays narrow.
pub fn glyph_vector(g: &[f32], w: usize, h: usize, b: &Rect, line_h: f64) -> Vec<f32> {
    let mut v = vec![0f32; GLYPH_W * GLYPH_H];
    let scale = b.h.max(line_h * 0.7) / GLYPH_H as f64;
    let cx = b.x + b.w / 2.0;
    let cy = b.y + b.h / 2.0;
    let sub = js_round(scale).min(4.0).max(1.0) as usize;
    let mut mean = 0.0f64;
    for j in 0..GLYPH_H {
        for i in 0..GLYPH_W {
            let mut s = 0.0;
            for bb in 0..sub {
                for a in 0..sub {
                    s += sample(
                        g,
                        w,
                        h,
                        cx + (i as f64 + (a as f64 + 0.5) / sub as f64 - GLYPH_W as f64 / 2.0) * scale - 0.5,
                        cy + (j as f64 + (bb as f64 + 0.5) / sub as f64 - GLYPH_H as f64 / 2.0) * scale - 0.5,
                    );
                }
            }
            v[j * GLYPH_W + i] = (s / (sub * sub) as f64) as f32;
            mean += v[j * GLYPH_W + i] as f64;
        }
    }
    mean /= v.len() as f64;
    let mut norm = 0.0f64;
    for x in v.iter_mut() {
        *x = (*x as f64 - mean) as f32;
        norm += *x as f64 * *x as f64;
    }
    let norm = if norm.sqrt() != 0.0 { norm.sqrt() } else { 1.0 };
    for x in v.iter_mut() {
        *x = (*x as f64 / norm) as f32;
    }
    v
}

/// A picture's colours (4x4x4 histogram) and layout (8x8 grey grid), for matching card art and icons.
pub fn art_feature(img: &Img, r: Rect) -> ArtFeature {
    let c = crop(img, r);
    let mut hist = vec![0.0; 64];
    let mut grid = vec![0.0; 64];
    let mut n = [0usize; 64];
    for y in 0..c.height {
        for x in 0..c.width {
            let (rr, gg, bb) = c.px(x, y);
            hist[((rr >> 6) as usize) * 16 + ((gg >> 6) as usize) * 4 + (bb >> 6) as usize] += 1.0;
            let cell = ((y * 8) / c.height).min(7) * 8 + ((x * 8) / c.width).min(7);
            grid[cell] += 0.299 * rr as f64 + 0.587 * gg as f64 + 0.114 * bb as f64;
            n[cell] += 1;
        }
    }
    let total = (c.width * c.height) as f64;
    ArtFeature {
        hist: hist.iter().map(|v| v / total).collect(),
        grid: grid.iter().enumerate().map(|(i, v)| v / if n[i] == 0 { 1.0 } else { n[i] as f64 }).collect(),
    }
}

fn cosine(a: &[f64], b: &[f64]) -> f64 {
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for i in 0..a.len() {
        ab += a[i] * b[i];
        aa += a[i] * a[i];
        bb += b[i] * b[i];
    }
    if aa != 0.0 && bb != 0.0 {
        ab / (aa * bb).sqrt()
    } else {
        0.0
    }
}

fn centered(a: &[f64]) -> Vec<f64> {
    let m = a.iter().fold(0.0, |s, v| s + v) / a.len() as f64;
    a.iter().map(|v| v - m).collect()
}

/// Precomputed centred grid, so a reference compares without allocating.
pub struct Art {
    pub hist: Vec<f64>,
    pub grid: Vec<f64>,
}

impl Art {
    pub fn from(f: &ArtFeature) -> Art {
        Art { hist: f.hist.clone(), grid: centered(&f.grid) }
    }
}

pub fn similarity(a: &Art, b: &Art) -> f64 {
    0.5 * cosine(&a.hist, &b.hist) + 0.5 * (0.5 + 0.5 * cosine(&a.grid, &b.grid))
}

/// A rect's grey layout as a zero-mean, unit-length w×h grid: tells apart text lines such as headers.
pub fn signature(img: &Img, r: Rect, w: usize, h: usize) -> Vec<f64> {
    let g = gray(&resize(&crop(img, r), w, h));
    let mean = g.iter().fold(0.0f64, |s, &v| s + v as f64) / g.len() as f64;
    let v: Vec<f64> = g.iter().map(|&x| x as f64 - mean).collect();
    let n = js_hypot(&v);
    let norm = if n != 0.0 { n } else { 1.0 };
    v.iter().map(|x| x / norm).collect()
}

/// A rect in a tilted object's own frame (x right, y down, origin at its centre), sampled out and straightened.
pub fn patch(img: &Img, cx: f64, cy: f64, angle: f64, r: Rect, scale: f64) -> Img {
    let w = js_round(r.w).max(1.0) as usize;
    let h = js_round(r.h).max(1.0) as usize;
    let mut out = Img::new(w, h);
    let (c, s) = (libm::cos(angle), libm::sin(angle));
    let iw = img.width;
    for j in 0..h {
        for i in 0..w {
            let u = (r.x + i as f64 + 0.5) * scale;
            let v = (r.y + j as f64 + 0.5) * scale;
            let x = (img.width as f64 - 1.001).min((cx + u * c - v * s - 0.5).max(0.0));
            let y = (img.height as f64 - 1.001).min((cy + u * s + v * c - 0.5).max(0.0));
            let (x0, y0) = (x as usize, y as usize);
            let (fx, fy) = (x - x0 as f64, y - y0 as f64);
            for k in 0..4 {
                let at = |xx: usize, yy: usize| img.data[(yy * iw + xx) * 4 + k] as f64;
                out.data[(j * w + i) * 4 + k] = clamp_u8((at(x0, y0) * (1.0 - fx) + at(x0 + 1, y0) * fx) * (1.0 - fy) + (at(x0, y0 + 1) * (1.0 - fx) + at(x0 + 1, y0 + 1) * fx) * fy);
            }
        }
    }
    out
}

#[inline]
pub fn dot(a: &[f64], b: &[f64]) -> f64 {
    let mut s = 0.0;
    for i in 0..a.len() {
        s += a[i] * b.get(i).copied().unwrap_or(f64::NAN);
    }
    s
}
