//! One tank from its label (src/solver/scan/tanks.ts): empty or not, rarity from the name's colour, archetype
//! from the icon above it, and Activity × Quantity, which must multiply to the total shown above it.

use crate::data::ScanData;
use crate::layout::Layout;
use crate::pixels::{hsv, Img, Rect};
use crate::reading::{FieldRead, Fields, TankDebug};
use crate::text::{brightest, read_text, TextKind};
use vorax::advisor::ScreenMonster;
use vorax::types::{Archetype, Rarity};

const NAME_MIN: f64 = 105.0;
const DIGIT_MIN: f64 = 0.75;
const MAX_STAT: f64 = 1_000_000.0;

/// JavaScript's Number() of a string of digits: "" is 0, anything else not a number is NaN.
pub(crate) fn js_number(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    if t.bytes().all(|b| b.is_ascii_digit()) {
        return t.parse::<f64>().unwrap_or(f64::NAN);
    }
    f64::NAN
}

/// Activity × Quantity from their line, kept within the game, and whether they multiply to the total.
pub fn stats(total: Option<&str>, aq: Option<&str>) -> (f64, f64, bool) {
    let parts: Vec<&str> = aq.unwrap_or("").split('x').collect();
    let (a, q) = if parts.len() == 2 { (js_number(parts[0]), js_number(parts[1])) } else { (f64::NAN, f64::NAN) };
    let ok = |n: f64| n.fract() == 0.0 && (1.0..=MAX_STAT).contains(&n);
    if !ok(a) || !ok(q) {
        return (1.0, 1.0, false);
    }
    let t = total.map_or(f64::NAN, js_number);
    (a, q, a * q == t)
}

/// Archetype from the little icon above a tank's name, by colour.
pub fn archetype_of(img: &Img, r: &Rect) -> (Archetype, bool) {
    let mut n = [(Archetype::BoneSentry, 0usize), (Archetype::Pest, 0), (Archetype::Aemberon, 0)];
    let x0 = r.x.floor().max(0.0) as usize;
    let y0 = r.y.floor().max(0.0) as usize;
    let mut y = y0;
    while (y as f64) < (img.height as f64).min(r.y + r.h) {
        let mut x = x0;
        while (x as f64) < (img.width as f64).min(r.x + r.w) {
            let (rr, gg, bb) = img.px(x, y);
            let (h, s, v) = hsv(rr, gg, bb);
            if v >= 0.45 {
                if (35.0..=65.0).contains(&h) && s >= 0.5 {
                    n[0].1 += 1;
                } else if (255.0..=305.0).contains(&h) && s >= 0.5 {
                    n[1].1 += 1;
                } else if (190.0..=240.0).contains(&h) && s >= 0.25 {
                    n[2].1 += 1;
                }
            }
            x += 1;
        }
        y += 1;
    }
    let mut sorted = n;
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    let (best, count) = sorted[0];
    if count >= 4 {
        (best, true)
    } else if count >= 2 {
        (best, false)
    } else {
        (Archetype::Agito, true)
    }
}

const MAX_COMBOS: usize = 200_000;

/// Most stray glyphs dropped from one end of a line: a cursor over the label read as "21" (1x36 as 1x3621).
const MAX_STRAY: usize = 2;

fn readings(alts: &[Vec<char>]) -> Vec<(String, usize)> {
    if alts.iter().fold(1usize, |n, a| n.saturating_mul(a.len())) > MAX_COMBOS {
        return vec![];
    }
    let mut out = vec![(String::new(), 0usize)];
    for choices in alts {
        out = out.iter().flat_map(|(t, r)| choices.iter().enumerate().map(move |(i, c)| (format!("{t}{c}"), r + i))).collect();
    }
    out
}

/// When Activity × Quantity doesn't make the total, one line has a misread digit: the lines that fit, or None.
pub fn reconcile(total: &[Vec<char>], aq: &[Vec<char>]) -> Option<(String, String)> {
    let first = |a: &[Vec<char>]| a.iter().map(|c| c[0]).collect::<String>();
    let best = |c: &[(String, usize)]| {
        let lowest = c.iter().map(|x| x.1).min().unwrap();
        let top: Vec<&(String, usize)> = c.iter().filter(|x| x.1 == lowest).collect();
        (top.len() == 1).then(|| top[0].0.clone())
    };
    let total_text = first(total);
    let aq_fits: Vec<(String, usize)> = readings(aq).into_iter().filter(|r| stats(Some(&total_text), Some(&r.0)).2).collect();
    if !aq_fits.is_empty() {
        return best(&aq_fits).map(|t| (total_text, t));
    }
    let aq_text = first(aq);
    let total_fits: Vec<(String, usize)> = readings(total).into_iter().filter(|r| stats(Some(&r.0), Some(&aq_text)).2).collect();
    if !total_fits.is_empty() {
        return best(&total_fits).map(|t| (t, aq_text));
    }
    // a sliver of the heart, or a cursor over the label, can pass as a glyph or two more at either end of a
    // line: dropped if the rest then fits, the fewest first
    for n in 1..=MAX_STRAY {
        let ends = |s: &str| -> Vec<String> {
            let c: Vec<char> = s.chars().collect();
            if c.len() > n {
                vec![c[n..].iter().collect(), c[..c.len() - n].iter().collect()]
            } else {
                vec![]
            }
        };
        let mut trimmed: Vec<(String, String)> = ends(&aq_text).into_iter().filter(|t| stats(Some(&total_text), Some(t)).2).map(|t| (total_text.clone(), t)).collect();
        trimmed.extend(ends(&total_text).into_iter().filter(|t| stats(Some(t), Some(&aq_text)).2).map(|t| (t, aq_text.clone())));
        if trimmed.len() == 1 {
            return trimmed.pop();
        }
        if trimmed.len() > 1 {
            return None;
        }
    }
    let parts: Vec<&str> = aq_text.split('x').collect();
    let (a, q) = (parts.first().copied().unwrap_or(""), parts.get(1).copied().unwrap_or(""));
    let t = js_number(&total_text);
    if a.is_empty() || q.is_empty() || t.fract() != 0.0 || t.is_nan() {
        return None;
    }
    let (an, qn) = (js_number(a), js_number(q));
    let one_off = |read: &str, want: f64| {
        let w = format!("{}", want as i64);
        w.len() == read.len() && read.chars().zip(w.chars()).filter(|(x, y)| x != y).count() == 1
    };
    let from_q = (qn > 0.0 && t % qn == 0.0 && one_off(a, t / qn)).then(|| format!("{}x{q}", (t / qn) as i64));
    let from_a = (an > 0.0 && t % an == 0.0 && one_off(q, t / an)).then(|| format!("{a}x{}", (t / an) as i64));
    let fixed = match (from_q, from_a) {
        (Some(_), Some(_)) => None,
        (x, y) => x.or(y),
    };
    fixed.filter(|f| stats(Some(&total_text), Some(f)).2).map(|f| (total_text, f))
}

/// The red glow of an empty tank's glass, not a name: deep red but dim. Not in upstream: on its frames empty
/// tanks measure 88 or less (under NAME_MIN), but some screens light the glass brighter (measured live at about
/// 115, 50, 55), which upstream reads as a Boss with unreadable numbers. Boss names measure about 213, 93, 89.
const GLOW_MAX: f64 = 160.0;

pub fn empty_glow(rgb: [f64; 3]) -> bool {
    if rgb.iter().any(|v| v.is_nan()) {
        return false;
    }
    let (h, s, _) = hsv(rgb[0] as u8, rgb[1] as u8, rgb[2] as u8);
    let max = rgb[0].max(rgb[1]).max(rgb[2]);
    s >= 0.35 && (h < 15.0 || h > 330.0) && max < GLOW_MAX
}

/// Rarity from a name's colour.
pub fn rarity_of(rgb: [f64; 3]) -> (Rarity, bool) {
    let (h, s, _) = hsv(rgb[0] as u8, rgb[1] as u8, rgb[2] as u8);
    if s >= 0.35 {
        if (15.0..=70.0).contains(&h) {
            return (Rarity::Rare, true);
        }
        if h < 15.0 || h > 330.0 {
            return (Rarity::Boss, true);
        }
        return (Rarity::Magic, (180.0..=240.0).contains(&h));
    }
    if (180.0..=240.0).contains(&h) {
        return if s >= 0.15 { (Rarity::Magic, true) } else { (if s >= 0.12 { Rarity::Magic } else { Rarity::Normal }, false) };
    }
    (Rarity::Normal, true)
}

fn rarity_name(r: Rarity) -> &'static str {
    match r {
        Rarity::Normal => "normal",
        Rarity::Magic => "magic",
        Rarity::Rare => "rare",
        Rarity::Boss => "boss",
    }
}

pub fn archetype_name(a: Archetype) -> &'static str {
    match a {
        Archetype::Pest => "pest",
        Archetype::Aemberon => "aemberon",
        Archetype::BoneSentry => "boneSentry",
        Archetype::Agito => "agito",
    }
}

pub struct TankRead {
    pub monster: Option<ScreenMonster>,
    pub fields: Fields,
    pub debug: Option<TankDebug>,
}

fn num(x: f64) -> String {
    format!("{}", x as i64)
}

/// `glow`: an empty tank's red glow taken for empty (not in upstream).
pub fn read_tank(img: &Img, g: &[f32], l: &Layout, i: usize, data: &ScanData, glow: bool) -> TankRead {
    let mut fields = Fields::new();
    let rgb = brightest(img, &l.name_band[i]);
    // Math.max(...rgb) < NAME_MIN, where a NaN (a band off the picture) makes it false
    let max = if rgb.iter().any(|v| v.is_nan()) { f64::NAN } else { rgb[0].max(rgb[1]).max(rgb[2]) };
    if max < NAME_MIN || (glow && empty_glow(rgb)) {
        fields.insert(format!("tank{i}.empty"), FieldRead::sure(true));
        return TankRead { monster: None, fields, debug: None };
    }
    let (rarity, sure) = rarity_of(rgb);
    fields.insert(format!("tank{i}.rarity"), FieldRead { sure, guess: Some(rarity_name(rarity).into()), ..Default::default() });
    let (archetype, a_sure) = archetype_of(img, &l.icon[i]);
    fields.insert(format!("tank{i}.archetype"), FieldRead { sure: a_sure, guess: Some(archetype_name(archetype).into()), ..Default::default() });

    let refs = &data.digits["label"];
    let total = read_text(img, g, &l.total_band[i], TextKind::Label, refs);
    let aq = read_text(img, g, &l.aq_band[i], TextKind::Label, refs);
    let mut st = stats(total.as_ref().map(|t| t.text.as_str()), aq.as_ref().map(|t| t.text.as_str()));
    let fix = if !st.2 { if let (Some(t), Some(a)) = (&total, &aq) { reconcile(&t.alts, &a.alts) } else { None } } else { None };
    if let Some((t, a)) = &fix {
        st = stats(Some(t), Some(a));
    }
    let fits = st.2 && total.is_some() && aq.is_some() && (fix.is_some() || (total.as_ref().unwrap().score >= DIGIT_MIN && aq.as_ref().unwrap().score >= DIGIT_MIN));
    let (score, second) = (aq.as_ref().map(|a| a.score), aq.as_ref().map(|a| a.second));
    fields.insert(format!("tank{i}.activity"), FieldRead { sure: fits, guess: Some(num(st.0)), score, second });
    fields.insert(format!("tank{i}.quantity"), FieldRead { sure: fits, guess: Some(num(st.1)), score, second });
    let debug = TankDebug { rgb, total: total.as_ref().map_or(String::new(), |t| t.text.clone()), aq: aq.as_ref().map_or(String::new(), |t| t.text.clone()) };
    TankRead { monster: Some(ScreenMonster { archetype, rarity, activity: st.0 as i64, quantity: st.1 as i64 }), fields, debug: Some(debug) }
}
