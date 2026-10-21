//! Colour correction (src/solver/scan/levels.ts): a capture through HDR, another gamma or brightness is mapped
//! so its levels spread like a normal capture's, with one curve for all three channels.

use crate::layout::Layout;
use crate::pixels::{js_round, Img, Rect};

pub fn read_areas(l: &Layout) -> Vec<Rect> {
    let mut v = Vec::new();
    v.extend(l.icon);
    v.extend(l.name_band);
    v.extend(l.total_band);
    v.extend(l.aq_band);
    v.extend(l.card_face);
    v.push(l.header);
    v.push(l.refresh);
    v.push(l.round);
    v.extend(l.instrument_icon);
    v
}

/// For each level 0-255, the share of the channel values (all three pooled) at or below it, in `areas`.
pub fn levels(img: &Img, areas: &[Rect]) -> Vec<f64> {
    let mut h = [0f64; 256];
    let mut n = 0f64;
    for r in areas {
        let x0 = r.x.floor().max(0.0) as usize;
        let x1 = (img.width as f64).min((r.x + r.w).ceil()).max(0.0) as usize;
        let y0 = r.y.floor().max(0.0) as usize;
        let y1 = (img.height as f64).min((r.y + r.h).ceil()).max(0.0) as usize;
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * img.width + x) * 4;
                h[img.data[i] as usize] += 1.0;
                h[img.data[i + 1] as usize] += 1.0;
                h[img.data[i + 2] as usize] += 1.0;
                n += 3.0;
            }
        }
    }
    let mut below = 0.0;
    h.iter()
        .map(|&c| {
            below += c;
            below / n
        })
        .collect()
}

/// The frame with its levels mapped to spread like `reference` (histogram matching).
pub fn match_levels(img: &Img, reference: &[f64], areas: &[Rect]) -> Img {
    let src = levels(img, areas);
    let mut curve = [0u8; 256];
    let mut w = 0usize;
    for v in 0..256 {
        let p = ((if v > 0 { src[v - 1] } else { 0.0 }) + src[v]) / 2.0;
        while w < 255 && reference[w] < p {
            w += 1;
        }
        let lo = if w > 0 { reference[w - 1] } else { 0.0 };
        let x = js_round(w as f64 - 0.5 + if reference[w] > lo { (p - lo) / (reference[w] - lo) } else { 0.5 });
        curve[v] = x.max(0.0).min(255.0) as u8;
    }
    let mut out = img.clone();
    for px in out.data.chunks_exact_mut(4) {
        px[0] = curve[px[0] as usize];
        px[1] = curve[px[1] as usize];
        px[2] = curve[px[2] as usize];
    }
    out
}

/// Just beyond what any normal capture measured (0.10).
pub const OFF_NORMAL: f64 = 0.12;

/// The frame with a normal capture's colours, how far off it was, and whether it was corrected.
pub fn normal_colours(img: &Img, reference: &[f64], areas: &[Rect]) -> (Option<Img>, f64, bool) {
    let q = levels(img, areas);
    let off = q.iter().zip(reference).fold(f64::NEG_INFINITY, |m, (a, b)| m.max((a - b).abs()));
    if off > OFF_NORMAL {
        (Some(match_levels(img, reference, areas)), off, true)
    } else {
        (None, off, false)
    }
}
