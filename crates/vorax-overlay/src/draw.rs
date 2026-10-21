//! What the overlay draws, and drawing it: a gold outline on the card (or reroll) to take, a marker above each
//! tank to target, and one small pill with the move and how often it reaches 950k. Pure drawing into an RGBA
//! picture, so it can be previewed on a saved frame without the game.

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};
use vorax::advisor::{Advice, Best, ScreenPhase, ScreenState};
use vorax_scan::layout::{CardPos, Layout};
use vorax_scan::pixels::Rect;
use vorax_scan::reading::{Problem, Reading};

/// A card face in reference pixels (layout.ts CARD_FACE).
const CARD_W: f64 = 230.0;
const CARD_H: f64 = 340.0;

#[derive(Clone, Debug, Default)]
pub struct Pill {
    /// the move: a card's name, with its tanks
    pub main: String,
    /// how it ends: "64% reach 950k"
    pub sub: String,
    /// the board is being read or worked out: the last advice, dimmed
    pub dim: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub pill: Option<Pill>,
    /// the card to take: its centre, tilt and size in screen pixels
    pub card: Option<(CardPos, f64, f64)>,
    /// the reroll button
    pub button: Option<Rect>,
    /// a marker above each tank to target: its centre
    pub tanks: Vec<(f64, f64)>,
}

impl Plan {
    pub fn dimmed(&self) -> Plan {
        let mut p = self.clone();
        if let Some(pill) = &mut p.pill {
            pill.dim = true;
        }
        p.card = None;
        p.button = None;
        p.tanks.clear();
        p
    }
}

fn pct(x: f64) -> String {
    format!("{:.0}% reach 950k", x * 100.0)
}

/// The hand positions the reading's names came from: unknown cards are left out of its hand.
fn slots(reading: &Reading, size: usize) -> Vec<usize> {
    let unknown: Vec<usize> = reading
        .problems
        .iter()
        .filter_map(|p| match p {
            Problem::UnknownArt { field, .. } => field.strip_prefix("hand").and_then(|j| j.parse().ok()),
            _ => None,
        })
        .collect();
    (0..size).filter(|j| !unknown.contains(j)).collect()
}

/// The marks for this advice on this board. `l`: the layout of the screen the overlay covers.
pub fn plan_for(advice: &Advice, screen: &ScreenState, reading: &Reading, l: &Layout) -> Plan {
    let mut plan = Plan::default();
    let size = reading.hand_ranks.as_ref().map(|r| r.len()).filter(|&n| n > 0).or(reading.hand_size).unwrap_or(3);
    let at = slots(reading, size);
    let pos = l.hand_at(size);
    match &advice.best {
        Best::Nothing => {}
        Best::Refresh => {
            let r = l.refresh;
            plan.button = Some(Rect::new(r.x - 0.35 * r.w, r.y - 0.45 * r.h, r.w * 1.7, r.h * 1.9));
            let sub = advice.refresh_end.map(|e| pct(e.reached)).unwrap_or_default();
            plan.pill = Some(Pill { main: "Reroll".into(), sub, dim: false });
        }
        Best::Card(r) => {
            let name = screen.hand.get(r.index).cloned().unwrap_or_else(|| format!("Card {}", r.index + 1));
            if let Some(&slot) = at.get(r.index) {
                if let Some(c) = pos.get(slot) {
                    plan.card = Some((*c, CARD_W * l.k, CARD_H * l.k));
                }
            }
            for &t in &r.targets {
                let t = t as usize;
                if t < 6 {
                    let icon = l.icon[t];
                    plan.tanks.push((l.tank_cx[t], icon.y - icon.h * 0.9));
                }
            }
            let main = match r.targets.as_slice() {
                [] => name,
                [a] => format!("{name}  →  tank {}", a + 1),
                ts => format!("{name}  →  tanks {}", ts.iter().map(|t| (t + 1).to_string()).collect::<Vec<_>>().join(" + ")),
            };
            let sub = match (&advice.note, r.end) {
                (Some(_), _) => "always take it".into(),
                (None, Some(e)) => pct(e.reached),
                (None, None) => String::new(),
            };
            plan.pill = Some(Pill { main, sub, dim: false });
        }
    }
    if screen.phase == ScreenPhase::Protocol {
        plan = Plan::default();
    }
    plan
}

pub struct Fonts {
    pub bold: fontdue::Font,
    pub regular: fontdue::Font,
}

impl Fonts {
    /// Segoe UI from Windows (Arial where it's missing).
    pub fn system() -> Option<Fonts> {
        let dir = std::env::var("WINDIR").map(|w| format!("{w}\\Fonts")).unwrap_or_else(|_| "C:\\Windows\\Fonts".into());
        let load = |names: &[&str]| {
            names.iter().find_map(|n| std::fs::read(format!("{dir}\\{n}")).ok()).and_then(|b| fontdue::Font::from_bytes(b, fontdue::FontSettings::default()).ok())
        };
        Some(Fonts { bold: load(&["segoeuib.ttf", "arialbd.ttf"])?, regular: load(&["segoeui.ttf", "arial.ttf"])? })
    }
}

pub(crate) fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, (a * 255.0) as u8)
}

pub(crate) const GOLD: (u8, u8, u8) = (255, 214, 138);
pub(crate) const GOLD_DIM: (u8, u8, u8) = (227, 178, 90);

pub(crate) fn paint(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

pub(crate) fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

/// The marks' colours to choose from (the tray menu): bright ones that stand out on the game's red and gold.
pub const ACCENTS: [(&str, (u8, u8, u8)); 5] = [
    ("Cyan", (64, 232, 255)),
    ("Green", (90, 255, 140)),
    ("Magenta", (255, 92, 214)),
    ("White", (255, 255, 255)),
    ("Gold", (255, 214, 138)),
];

pub fn accent(i: usize) -> (u8, u8, u8) {
    ACCENTS.get(i).unwrap_or(&ACCENTS[0]).1
}

/// An outline that reads on any background: a dark edge under it, a soft glow and a bright line in the accent.
fn glow(pm: &mut Pixmap, path: &tiny_skia::Path, t: Transform, s: f32, (r, g, b): (u8, u8, u8)) {
    for (w, a) in [(22.0, 0.08), (13.0, 0.16)] {
        pm.stroke_path(path, &paint(rgba(r, g, b, a)), &Stroke { width: w * s, ..Default::default() }, t, None);
    }
    pm.stroke_path(path, &paint(rgba(8, 6, 8, 0.85)), &Stroke { width: 8.0 * s, ..Default::default() }, t, None);
    pm.stroke_path(path, &paint(rgba(r, g, b, 1.0)), &Stroke { width: 4.0 * s, ..Default::default() }, t, None);
}

/// A filled mark with a dark edge: reads on the bright tanks and on the dark glass alike.
fn solid(pm: &mut Pixmap, path: &tiny_skia::Path, t: Transform, s: f32, (r, g, b): (u8, u8, u8)) {
    pm.stroke_path(path, &paint(rgba(r, g, b, 0.22)), &Stroke { width: 14.0 * s, ..Default::default() }, t, None);
    pm.stroke_path(path, &paint(rgba(8, 6, 8, 0.9)), &Stroke { width: 5.0 * s, ..Default::default() }, t, None);
    pm.fill_path(path, &paint(rgba(r, g, b, 1.0)), FillRule::Winding, t, None);
}

fn diamond_path(x: f32, y: f32, r: f32) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(x, y - r);
    pb.line_to(x + r * 0.8, y);
    pb.line_to(x, y + r);
    pb.line_to(x - r * 0.8, y);
    pb.close();
    pb.finish()
}

/// Text width at a size, with kerning.
pub(crate) fn measure(font: &fontdue::Font, text: &str, px: f32) -> f32 {
    let mut w = 0.0;
    let mut prev = None;
    for ch in text.chars() {
        if let Some(p) = prev {
            w += font.horizontal_kern(p, ch, px).unwrap_or(0.0);
        }
        w += font.metrics(ch, px).advance_width;
        prev = Some(ch);
    }
    w
}

/// Draws text with its baseline at y, alpha-blended into the premultiplied picture.
pub(crate) fn text(pm: &mut Pixmap, font: &fontdue::Font, s: &str, px: f32, x: f32, baseline: f32, (r, g, b): (u8, u8, u8), alpha: f32) {
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    let data = pm.data_mut();
    let mut pen = x;
    let mut prev = None;
    for ch in s.chars() {
        if let Some(p) = prev {
            pen += font.horizontal_kern(p, ch, px).unwrap_or(0.0);
        }
        let (m, cov) = font.rasterize(ch, px);
        let gx = (pen + m.xmin as f32).round() as i32;
        let gy = (baseline - m.height as f32 - m.ymin as f32).round() as i32;
        for yy in 0..m.height as i32 {
            for xx in 0..m.width as i32 {
                let (px_x, px_y) = (gx + xx, gy + yy);
                if px_x < 0 || px_y < 0 || px_x >= w || px_y >= h {
                    continue;
                }
                let a = cov[(yy * m.width as i32 + xx) as usize] as f32 / 255.0 * alpha;
                if a <= 0.0 {
                    continue;
                }
                let i = ((px_y * w + px_x) * 4) as usize;
                for (k, c) in [r, g, b].iter().enumerate() {
                    data[i + k] = (*c as f32 * a + data[i + k] as f32 * (1.0 - a)).round() as u8;
                }
                data[i + 3] = (255.0 * a + data[i + 3] as f32 * (1.0 - a)).round() as u8;
            }
        }
        pen += m.advance_width;
        prev = Some(ch);
    }
}

/// The overlay for a screen of `w`×`h` pixels, transparent everywhere it draws nothing (premultiplied RGBA).
/// `accent`: the marks' colour.
pub fn render(plan: &Plan, w: u32, h: u32, fonts: &Fonts, accent: (u8, u8, u8)) -> Pixmap {
    let mut pm = Pixmap::new(w.max(1), h.max(1)).unwrap();
    // sizes follow the screen's height, as the game's UI does (1440p is 1)
    let s = (h as f32 / 1440.0).max(0.6);

    if let Some((c, cw, ch)) = plan.card {
        let t = Transform::from_rotate(c.angle.to_degrees() as f32).post_translate(c.cx as f32, c.cy as f32);
        let pad = 8.0 * s;
        let (hw, hh) = (cw as f32 / 2.0 + pad, ch as f32 / 2.0 + pad);
        if let Some(p) = rounded(-hw, -hh, 2.0 * hw, 2.0 * hh, 16.0 * s) {
            glow(&mut pm, &p, t, s, accent);
        }
    }
    if let Some(b) = plan.button {
        if let Some(p) = rounded(b.x as f32, b.y as f32, b.w as f32, b.h as f32, 12.0 * s) {
            glow(&mut pm, &p, Transform::identity(), s, accent);
        }
    }
    for &(x, y) in &plan.tanks {
        if let Some(p) = diamond_path(x as f32, y as f32, 16.0 * s) {
            solid(&mut pm, &p, Transform::identity(), s, accent);
        }
    }

    if let Some(pill) = &plan.pill {
        let (big, small) = (23.0 * s, 18.0 * s);
        let gap = 16.0 * s;
        let mark = 12.0 * s;
        // "33% reach 950k": the share in the accent, the rest in light grey
        let (odds, rest) = match pill.sub.split_once(' ') {
            Some((a, b)) if a.ends_with('%') => (a.to_string(), format!(" {b}")),
            _ => (String::new(), pill.sub.clone()),
        };
        let tw = measure(&fonts.bold, &pill.main, big);
        let ow = measure(&fonts.bold, &odds, small);
        let rw = measure(&fonts.regular, &rest, small);
        let sw = if pill.sub.is_empty() { 0.0 } else { gap + ow + rw };
        let (ph, padx) = (46.0 * s, 20.0 * s);
        let pw = padx * 2.0 + mark + 12.0 * s + tw + sw;
        let (px, py) = ((w as f32 - pw) / 2.0, 14.0 * s);
        let alpha = if pill.dim { 0.55 } else { 1.0 };
        let (ar, ag, ab) = accent;
        if let Some(p) = rounded(px, py, pw, ph, ph / 2.0) {
            pm.fill_path(&p, &paint(rgba(10, 8, 10, 0.92 * alpha)), FillRule::Winding, Transform::identity(), None);
            pm.stroke_path(&p, &paint(rgba(ar, ag, ab, 0.75 * alpha)), &Stroke { width: 1.8 * s, ..Default::default() }, Transform::identity(), None);
        }
        if let Some(p) = diamond_path(px + padx + mark / 2.0, py + ph / 2.0, mark / 2.0) {
            pm.fill_path(&p, &paint(rgba(ar, ag, ab, alpha)), FillRule::Winding, Transform::identity(), None);
        }
        let base = py + ph / 2.0 + big * 0.35;
        let mut x = px + padx + mark + 12.0 * s;
        text(&mut pm, &fonts.bold, &pill.main, big, x, base, (255, 255, 255), alpha);
        x += tw + gap;
        if !pill.sub.is_empty() {
            text(&mut pm, &fonts.bold, &odds, small, x, base, accent, alpha);
            text(&mut pm, &fonts.regular, &rest, small, x + ow, base, (214, 206, 202), alpha);
        }
    }
    pm
}
