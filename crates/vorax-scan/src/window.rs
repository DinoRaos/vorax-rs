//! A windowed game's capture holds its title bar and borders (src/solver/live/windowCrop.ts): found in the
//! picture itself, so only the game is read.

use crate::pixels::{js_round, Img, Rect};

pub fn crop_to(img: &Img, r: &Rect) -> Img {
    let x0 = js_round(r.x).max(0.0) as usize;
    let y0 = js_round(r.y).max(0.0) as usize;
    let w = ((img.width - x0) as f64).min(js_round(r.w)) as usize;
    let h = ((img.height - y0) as f64).min(js_round(r.h)) as usize;
    let mut out = Img::new(w, h);
    for y in 0..h {
        let s = ((y0 + y) * img.width + x0) * 4;
        out.data[y * w * 4..(y + 1) * w * 4].copy_from_slice(&img.data[s..s + w * 4]);
    }
    out
}

fn row_colour(img: &Img, y: usize) -> ([f64; 3], f64) {
    let w = img.width;
    let mut mean = [0.0; 3];
    for x in 0..w {
        for (c, m) in mean.iter_mut().enumerate() {
            *m += img.data[(y * w + x) * 4 + c] as f64 / w as f64;
        }
    }
    let mut near = 0usize;
    for x in 0..w {
        let i = (y * w + x) * 4;
        let d = (0..3).map(|c| (img.data[i + c] as f64 - mean[c]).abs()).fold(f64::NEG_INFINITY, f64::max);
        if d <= 24.0 {
            near += 1;
        }
    }
    (mean, near as f64 / w as f64)
}

/// The game's picture inside a capture of its window; None for a borderless or fullscreen game. The title bar is
/// found from a row well inside it, and the window's edge taken as however many rows lie above it: a capture shrunk
/// or stretched to fit (a 4K screen at 150%) blurs that edge over a row or two, and the edge is as thick on the
/// sides and bottom.
pub fn windowed_game(img: &Img) -> Option<Rect> {
    // a row in the title whatever the edge; the most rows the window's edge spans, blurred
    const INSIDE: usize = 5;
    const EDGE: usize = 4;
    if img.height < 200 {
        return None;
    }
    let (inside, even) = row_colour(img, INSIDE);
    // a title bar isn't black (the game's top edge, a masked test frame, is)
    if even < 0.8 || inside.iter().fold(f64::NEG_INFINITY, |m, &v| m.max(v)) < 28.0 {
        return None;
    }
    let is_title = |y: usize| row_colour(img, y).0.iter().zip(&inside).all(|(v, f)| (v - f).abs() <= 12.0);
    // the title's first row: the rows above it are the window's edge
    let mut start = INSIDE;
    while start > 0 && is_title(start - 1) {
        start -= 1;
    }
    // a row of title text is less even, but still, on average, the title's colour
    let mut end = INSIDE;
    while end + 1 < 80 && is_title(end + 1) {
        end += 1;
    }
    if end - start + 1 < 16 {
        return None;
    }
    // an edge above the title and a border under it, each a line of another colour: a game screen that's evenly
    // dark at the top (a loading screen) has neither
    let differs = |y: usize| row_colour(img, y).0.iter().zip(&inside).any(|(v, f)| (v - f).abs() > 8.0);
    if start == 0 || start > EDGE || !differs(end + 1) {
        return None;
    }
    // the game starts under the title's border, as thick as the edge
    let top = end + 1 + start;
    Some(Rect::new(start as f64, top as f64, (img.width - 2 * start) as f64, (img.height - top - start) as f64))
}
