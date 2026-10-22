//! Text on the screen as glyph boxes (src/solver/scan/text.ts) and reading them as characters (digits.ts).

use crate::pixels::{glyph_vector, hsv, js_round, Img, Rect};
use indexmap::IndexMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextKind {
    Label,
    Round,
    Refresh,
}

impl TextKind {
    pub fn key(self) -> &'static str {
        match self {
            TextKind::Label => "label",
            TextKind::Round => "round",
            TextKind::Refresh => "refresh",
        }
    }
}

#[inline]
fn lum(r: u8, g: u8, b: u8) -> f64 {
    0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64
}
#[inline]
fn is_red(r: u8, g: u8, b: u8) -> bool {
    r as i32 > g as i32 + 60 && r as i32 > b as i32 + 50
}

/// The pixel bounds a rect covers, as the TypeScript loops them: floor/ceil, inside the image.
#[inline]
pub fn span(img: &Img, r: &Rect) -> (usize, usize, usize, usize) {
    let x0 = r.x.floor().max(0.0) as usize;
    let y0 = r.y.floor().max(0.0) as usize;
    let x1 = (img.width as f64).min((r.x + r.w).ceil()).max(0.0) as usize;
    let y1 = (img.height as f64).min((r.y + r.h).ceil()).max(0.0) as usize;
    (x0, y0, x1, y1)
}

/// Otsu's threshold over the luminance of a rect's pixels that pass `use_px`.
fn otsu(img: &Img, r: &Rect, use_px: impl Fn(u8, u8, u8) -> bool) -> f64 {
    let mut hist = [0f64; 256];
    let mut n = 0f64;
    // the TypeScript bounds these loops by r.y + r.h (not its ceiling)
    let x0 = r.x.floor().max(0.0) as usize;
    let y0 = r.y.floor().max(0.0) as usize;
    let mut y = y0;
    while (y as f64) < (img.height as f64).min(r.y + r.h) {
        let mut x = x0;
        while (x as f64) < (img.width as f64).min(r.x + r.w) {
            let (rr, gg, bb) = img.px(x, y);
            if use_px(rr, gg, bb) {
                hist[js_round(lum(rr, gg, bb)) as usize] += 1.0;
                n += 1.0;
            }
            x += 1;
        }
        y += 1;
    }
    let mut sum = 0.0;
    for (t, h) in hist.iter().enumerate() {
        sum += t as f64 * h;
    }
    let (mut sum_b, mut w_b, mut best, mut at) = (0.0, 0.0, 0.0, 0usize);
    for t in 0..256 {
        w_b += hist[t];
        if w_b == 0.0 {
            continue;
        }
        let w_f = n - w_b;
        if w_f == 0.0 {
            break;
        }
        sum_b += t as f64 * hist[t];
        let d = sum_b / w_b - (sum - sum_b) / w_f;
        let between = w_b * w_f * (d * d);
        if between > best {
            best = between;
            at = t;
        }
    }
    at as f64
}

fn test_for(img: &Img, r: &Rect, kind: TextKind) -> Box<dyn Fn(u8, u8, u8) -> bool> {
    match kind {
        TextKind::Round => Box::new(|r, g, b| {
            let (_, s, v) = hsv(r, g, b);
            s > 0.2 && v > 0.45 && r >= g && g >= b
        }),
        TextKind::Refresh => Box::new(|r, g, b| {
            let (_, s, v) = hsv(r, g, b);
            s < 0.35 && v > 0.6
        }),
        TextKind::Label => {
            let t = 60f64.max(0.7 * otsu(img, r, |r, g, b| !is_red(r, g, b)));
            Box::new(move |r, g, b| !is_red(r, g, b) && lum(r, g, b) > t)
        }
    }
}

fn median(v: &[f64]) -> f64 {
    let mut a = v.to_vec();
    a.sort_by(|x, y| x.partial_cmp(y).unwrap());
    a[a.len() / 2]
}

/// Glyph boxes in a rect, left to right: runs of columns holding text pixels within the text's own row.
pub fn glyphs(img: &Img, r: &Rect, kind: TextKind) -> Vec<Rect> {
    let test = test_for(img, r, kind);
    let (x0, y0, x1, y1) = span(img, r);
    if x1 <= x0 || y1 <= y0 {
        return vec![];
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let mut on = vec![0u8; w * h];
    let mut rows = vec![0usize; h];
    let mut red = vec![0usize; w];
    for y in 0..h {
        for x in 0..w {
            let (rr, gg, bb) = img.px(x0 + x, y0 + y);
            if test(rr, gg, bb) {
                on[y * w + x] = 1;
                rows[y] += 1;
            }
            if rr as i32 > gg as i32 + 40 {
                red[x] += 1;
            }
        }
    }
    let peak_v = *rows.iter().max().unwrap();
    let peak = rows.iter().position(|&v| v == peak_v).unwrap();
    if rows[peak] == 0 {
        return vec![];
    }
    let (mut top, mut bottom) = (peak, peak);
    while top > 0 && rows[top - 1] > 0 {
        top -= 1;
    }
    while bottom < h - 1 && rows[bottom + 1] > 0 {
        bottom += 1;
    }
    let mut out: Vec<Rect> = Vec::new();
    let mut start: isize = -1;
    let flush = |start: usize, end: usize, out: &mut Vec<Rect>| {
        let (mut n, mut ry0, mut ry1, mut reddish) = (0usize, h as isize, -1isize, 0usize);
        for x in start..end {
            reddish += red[x];
            for y in top..=bottom {
                if on[y * w + x] == 1 {
                    n += 1;
                    ry0 = ry0.min(y as isize);
                    ry1 = ry1.max(y as isize);
                }
            }
        }
        if n >= 3 && (reddish as f64) < ((end - start) * (bottom - top + 1)) as f64 * 0.3 {
            out.push(Rect::new((x0 + start) as f64, (y0 as isize + ry0) as f64, (end - start) as f64, (ry1 - ry0 + 1) as f64));
        }
    };
    for x in 0..w {
        let any = (top..=bottom).any(|y| on[y * w + x] == 1);
        if any && start < 0 {
            start = x as isize;
        }
        if !any && start >= 0 {
            flush(start as usize, x, &mut out);
            start = -1;
        }
    }
    if start >= 0 {
        flush(start as usize, w, &mut out);
    }
    if kind == TextKind::Refresh {
        let hf = h as f64;
        return out.into_iter().filter(|g| g.h >= 0.25 * hf && g.h <= 0.5 * hf).last().into_iter().collect();
    }
    let tall = out.iter().fold(0.0f64, |m, g| m.max(g.h));
    let mut widths: Vec<f64> = out.iter().map(|g| g.w).collect();
    widths.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let typical_w = widths.get(widths.len() / 2).copied().unwrap_or(0.0);
    let kept: Vec<Rect> = out.into_iter().filter(|g| !(g.h < 0.6 * tall && g.w < 0.55 * typical_w)).collect();
    if kept.is_empty() {
        return kept;
    }
    let typical_h = median(&kept.iter().map(|g| g.h).collect::<Vec<_>>());
    let clean: Vec<&Rect> = kept.iter().filter(|g| g.h >= 0.8 * typical_h && g.h <= 1.15 * typical_h).collect();
    let (row_top, row_bottom) = if clean.len() >= 2 {
        (median(&clean.iter().map(|g| g.y).collect::<Vec<_>>()), median(&clean.iter().map(|g| g.y + g.h - 1.0).collect::<Vec<_>>()))
    } else {
        (-1.0, -1.0)
    };
    let clipped: Vec<Rect> = kept
        .iter()
        .map(|g| {
            if row_top < 0.0 || g.h <= 1.15 * typical_h {
                return *g;
            }
            let gy0 = g.y.max(row_top);
            let gy1 = (g.y + g.h - 1.0).min(row_bottom);
            if gy1 > gy0 {
                Rect::new(g.x, gy0, g.w, gy1 - gy0 + 1.0)
            } else {
                *g
            }
        })
        .collect();
    // as the TypeScript indexes it: a column just outside the box reads the row before or after, and one past
    // the ends of the array makes the count NaN (never the thinnest)
    let split = split_wide(clipped, |x| {
        let mut n = 0.0;
        for y in top..=bottom {
            let i = (y * w) as isize + x - x0 as isize;
            n += if i >= 0 && (i as usize) < on.len() { on[i as usize] as f64 } else { f64::NAN };
        }
        n
    });
    if kind == TextKind::Label {
        on_the_line(split, typical_h, row_bottom)
    } else {
        split
    }
}

/// A tank label's glyphs without bright bits around it: a candle under the tank flickers into the band as a
/// glyph or two, lower than the text and a gap away (1x36 read as 1x3621, 66x319 as 66x319661). The line
/// is the longest run of glyphs on the text's bottom row with less than a glyph's height between them.
fn on_the_line(gs: Vec<Rect>, typical_h: f64, row_bottom: f64) -> Vec<Rect> {
    let mut runs: Vec<Vec<Rect>> = Vec::new();
    for g in gs {
        if row_bottom >= 0.0 && (g.y + g.h - 1.0 - row_bottom).abs() > 1f64.max(0.3 * typical_h) {
            continue;
        }
        match runs.last_mut() {
            Some(run) if g.x - (run[run.len() - 1].x + run[run.len() - 1].w) < typical_h => run.push(g),
            _ => runs.push(vec![g]),
        }
    }
    runs.into_iter().fold(Vec::new(), |best, run| if run.len() > best.len() { run } else { best })
}

/// Glyphs much wider than the typical one are touching digits: split them at their thinnest columns.
fn split_wide(gs: Vec<Rect>, density: impl Fn(isize) -> f64) -> Vec<Rect> {
    if gs.is_empty() {
        return gs;
    }
    let typical = 0.62 * median(&gs.iter().map(|g| g.h).collect::<Vec<_>>());
    let mut out = Vec::new();
    for g in gs {
        let parts = js_round(g.w / (typical * 1.15));
        if g.w < 1.6 * typical || parts < 2.0 {
            out.push(g);
            continue;
        }
        let mut cuts: Vec<f64> = Vec::new();
        for p in 1..parts as usize {
            let aim = g.x + (g.w * p as f64) / parts;
            let (mut best, mut low) = (js_round(aim), f64::INFINITY);
            let mut x = js_round(aim - typical / 3.0);
            while x <= js_round(aim + typical / 3.0) {
                let d = density(x as isize);
                if d < low {
                    low = d;
                    best = x;
                }
                x += 1.0;
            }
            cuts.push(best);
        }
        let mut starts = vec![g.x];
        starts.extend(cuts.iter().map(|c| c + 1.0));
        let mut ends = cuts.clone();
        ends.push(g.x + g.w);
        for (i, &x) in starts.iter().enumerate() {
            let w = ends[i] - x;
            if w > 0.25 * typical {
                out.push(Rect::new(x, g.y, w, g.h));
            }
        }
    }
    out
}

/// Mean colour of the brightest 5% of a rect's pixels (by their strongest channel): the colour of its text.
pub fn brightest(img: &Img, r: &Rect) -> [f64; 3] {
    let x0 = r.x.floor().max(0.0) as usize;
    let y0 = r.y.floor().max(0.0) as usize;
    let mut px: Vec<(u8, u8, u8)> = Vec::new();
    let mut y = y0;
    while (y as f64) < (img.height as f64).min(r.y + r.h) {
        let mut x = x0;
        while (x as f64) < (img.width as f64).min(r.x + r.w) {
            px.push(img.px(x, y));
            x += 1;
        }
        y += 1;
    }
    px.sort_by(|a, b| b.0.max(b.1).max(b.2).cmp(&a.0.max(a.1).max(a.2)));
    let n = (px.len() / 20).max(1).min(px.len());
    let top = &px[..n];
    let mean = |c: usize| {
        let s: f64 = top.iter().map(|p| [p.0, p.1, p.2][c] as f64).sum();
        js_round(s / top.len() as f64)
    };
    if top.is_empty() {
        return [f64::NAN; 3];
    }
    [mean(0), mean(1), mean(2)]
}

/// alts: each glyph's three best characters, best first.
#[derive(Clone, Debug, PartialEq)]
pub struct TextRead {
    pub text: String,
    pub score: f64,
    pub second: f64,
    pub alts: Vec<Vec<char>>,
}

/// The characters in a rect, with the mean best and runner-up match scores (1 = identical).
pub fn read_text(img: &Img, g: &[f32], r: &Rect, kind: TextKind, refs: &IndexMap<String, Vec<Vec<f64>>>) -> Option<TextRead> {
    let gs = glyphs(img, r, kind);
    if gs.is_empty() {
        return None;
    }
    let line_h = gs.iter().fold(f64::NEG_INFINITY, |m, b| m.max(b.h));
    let (mut text, mut score, mut second) = (String::new(), 0.0, 0.0);
    let mut alts = Vec::new();
    for b in &gs {
        let v = glyph_vector(g, img.width, img.height, b, line_h);
        let mut best: Vec<(char, f64)> = refs
            .iter()
            .map(|(ch, vs)| {
                let m = vs.iter().map(|rf| rf.iter().zip(&v).fold(0.0, |s, (x, y)| s + x * *y as f64)).fold(f64::NEG_INFINITY, f64::max);
                (ch.chars().next().unwrap(), m)
            })
            .collect();
        best.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        text.push(best[0].0);
        alts.push(best.iter().take(3).map(|x| x.0).collect());
        score += best[0].1;
        second += best.get(1).map_or(-1.0, |x| x.1);
    }
    let n = gs.len() as f64;
    Some(TextRead { text, score: score / n, second: second / n, alts })
}
