//! The equipped instruments, from the icons in the top-right panel (src/solver/scan/instruments.ts, iconMatch.ts).

use crate::data::ScanData;
use crate::layout::Layout;
use crate::pixels::{signature, Img, Rect};
use crate::reading::{FieldRead, Fields, Ranks};
use std::collections::HashMap;

const BRIGHT: u8 = 60;
const FILLED_MIN: f64 = 0.008;
const ICON_MIN: f64 = 0.85;
const ICON_GAP: f64 = 0.08;
pub const STARTING_ITEM: &str = "(starting item)";

/// Near black: the square an icon sits on.
const DARK: u8 = 25;
/// An icon's square is mostly near black (49–95% measured); an empty slot over the lit scenery has none, yet enough
/// bright pixels to pass FILLED_MIN (1–7% at 1080p), so it read as an icon: 4 instruments where 3 were held.
const BACKDROP_MIN: f64 = 0.3;

pub fn holds_an_icon(img: &Img, r: &Rect) -> bool {
    let x0 = r.x.floor();
    let y0 = r.y.floor();
    let (mut n, mut dark) = (0usize, 0usize);
    let mut y = y0;
    while y < (img.height as f64).min(y0 + r.h) {
        let mut x = x0;
        while x < (img.width as f64).min(x0 + r.w) {
            let (rr, gg, bb) = img.px(x as usize, y as usize);
            let m = rr.max(gg).max(bb);
            if m >= BRIGHT {
                n += 1;
            }
            if m < DARK {
                dark += 1;
            }
            x += 1.0;
        }
        y += 1.0;
    }
    n as f64 / (r.w * r.h) >= FILLED_MIN && dark as f64 / (r.w * r.h) >= BACKDROP_MIN
}

const N: usize = 24;
const REACH: i32 = 2;

fn centre(p: &[f64]) -> (f64, f64) {
    let lo = p.iter().fold(f64::INFINITY, |m, &v| if v < m { v } else { m });
    let (mut s, mut x, mut y) = (0.0, 0.0, 0.0);
    for (i, &v) in p.iter().enumerate() {
        let w = v - lo;
        s += w;
        x += w * (i % N) as f64;
        y += w * (i / N) as f64;
    }
    if s != 0.0 {
        (x / s, y / s)
    } else {
        ((N - 1) as f64 / 2.0, (N - 1) as f64 / 2.0)
    }
}

fn moved(p: &[f64], dx: i32, dy: i32) -> Vec<f64> {
    let bg = p.iter().fold(f64::INFINITY, |m, &v| if v < m { v } else { m });
    let mut q = vec![0.0; p.len()];
    let mut sum = 0.0;
    for i in 0..p.len() {
        let x = (i % N) as i32 - dx;
        let y = (i / N) as i32 - dy;
        q[i] = if x >= 0 && (x as usize) < N && y >= 0 && (y as usize) < N { p[y as usize * N + x as usize] } else { bg };
        sum += q[i];
    }
    let mean = sum / q.len() as f64;
    let mut norm = 0.0;
    for v in q.iter_mut() {
        *v -= mean;
        norm += *v * *v;
    }
    let norm = if norm.sqrt() != 0.0 { norm.sqrt() } else { 1.0 };
    for v in q.iter_mut() {
        *v /= norm;
    }
    q
}

fn js_round(x: f64) -> f64 {
    crate::pixels::js_round(x)
}

/// How alike a reference icon and a panel pattern are, allowing for the crop being a few pixels off.
pub fn icon_similarity(rf: &[f64], p: &[f64], cache: &mut HashMap<(i32, i32), Vec<f64>>) -> f64 {
    if p.len() != N * N || rf.len() != N * N {
        return rf.iter().enumerate().fold(0.0, |s, (i, x)| s + x * p.get(i).copied().unwrap_or(0.0));
    }
    let (rx, ry) = centre(rf);
    let (px, py) = centre(p);
    let (bx, by) = (js_round(rx - px) as i32, js_round(ry - py) as i32);
    let mut best = -1.0f64;
    for dy in by - REACH..=by + REACH {
        for dx in bx - REACH..=bx + REACH {
            let q = cache.entry((dx, dy)).or_insert_with(|| if dx == 0 && dy == 0 { p.to_vec() } else { moved(p, dx, dy) });
            let mut s = 0.0;
            for i in 0..q.len() {
                s += rf[i] * q[i];
            }
            if s > best {
                best = s;
            }
        }
    }
    best
}

pub struct InstrumentsRead {
    pub names: Vec<String>,
    pub fields: Fields,
    pub ranks: Ranks,
    pub known: Vec<String>,
}

pub fn read_instruments(img: &Img, l: &Layout, data: &ScanData) -> InstrumentsRead {
    let mut from_right: Vec<String> = Vec::new();
    let mut reads: Vec<FieldRead> = Vec::new();
    let mut ranks_from_right: Ranks = Vec::new();
    for r in &l.instrument_icon {
        if !holds_an_icon(img, r) {
            break;
        }
        let p = signature(img, *r, 24, 24);
        let mut lined = HashMap::new();
        let mut ranked: Vec<(String, f64)> = data
            .icons
            .iter()
            .filter(|(n, _)| n.as_str() != "none")
            .map(|(n, vs)| (n.clone(), vs.iter().map(|v| icon_similarity(v, &p, &mut lined)).fold(f64::NEG_INFINITY, f64::max)))
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        if ranked[0].0 == STARTING_ITEM {
            continue;
        }
        let second = ranked.get(1).map(|x| x.1);
        reads.push(FieldRead { sure: ranked[0].1 >= ICON_MIN && ranked[0].1 - second.unwrap_or(0.0) >= ICON_GAP, guess: Some(ranked[0].0.clone()), score: Some(ranked[0].1), second });
        from_right.push(ranked[0].0.clone());
        ranks_from_right.push(ranked.into_iter().filter(|(n, _)| n != STARTING_ITEM).collect());
    }
    from_right.reverse();
    reads.reverse();
    ranks_from_right.reverse();
    let mut fields = Fields::new();
    for (i, f) in reads.into_iter().enumerate() {
        fields.insert(format!("instrument{i}"), f);
    }
    let known = data.icons.keys().filter(|n| n.as_str() != "none" && n.as_str() != STARTING_ITEM).cloned().collect();
    InstrumentsRead { names: from_right, fields, ranks: ranks_from_right, known }
}
