//! The cards on offer (src/solver/scan/cards.ts): which kind of pick it is (from the header), and each card from
//! its art and the shape of its text block.

use crate::data::ScanData;
use crate::layout::{card_face, CardPos, Layout, CARD_LOCAL_ART, CARD_LOCAL_TEXT};
use crate::pixels::{art_feature, dot, gray, patch, signature, similarity, Art, Img, Rect};
use crate::reading::{FieldRead, Fields, Problem, Ranks};
use std::collections::HashMap;

const HEADER_MIN: f64 = 0.6;
const ART_MIN: f64 = 0.95;
const ART_GAP: f64 = 0.015;
const ART_UNKNOWN: f64 = 0.92;
pub const TEXT_W: usize = 40;
pub const TEXT_H: usize = 38;

fn whole(r: Rect) -> Rect {
    Rect::new(0.0, 0.0, r.w, r.h)
}

pub fn card_art(img: &Img, c: &CardPos, k: f64, dx: f64, dy: f64) -> Art {
    Art::from(&art_feature(&patch(img, c.cx + dx * k, c.cy + dy * k, c.angle, CARD_LOCAL_ART, k), whole(CARD_LOCAL_ART)))
}

pub fn card_text(img: &Img, c: &CardPos, k: f64, dx: f64, dy: f64) -> Vec<f64> {
    signature(&patch(img, c.cx + dx * k, c.cy + dy * k, c.angle, CARD_LOCAL_TEXT, k), whole(CARD_LOCAL_TEXT), TEXT_W, TEXT_H)
}

const FACE_MIDDLE: Rect = Rect::new(-100.0, -45.0, 185.0, 175.0);
const EDGE_STEP: f64 = 25.0;
const CARD_DETAIL: f64 = 0.07;

fn has_card(img: &Img, c: &CardPos, k: f64) -> bool {
    let p = patch(img, c.cx, c.cy, c.angle, Rect::new(FACE_MIDDLE.x * k, FACE_MIDDLE.y * k, FACE_MIDDLE.w * k, FACE_MIDDLE.h * k), 1.0);
    let g = gray(&p);
    let mut edges = 0usize;
    for y in 1..p.height {
        for x in 1..p.width {
            let i = y * p.width + x;
            if (g[i] as f64 - g[i - 1] as f64).abs() > EDGE_STEP || (g[i] as f64 - g[i - p.width] as f64).abs() > EDGE_STEP {
                edges += 1;
            }
        }
    }
    edges as f64 / (p.width * p.height) as f64 > CARD_DETAIL
}

/// How many of the three hand positions hold a card.
pub fn cards_present(img: &Img, l: &Layout) -> usize {
    l.cards.iter().filter(|c| has_card(img, c, l.k)).count()
}

/// 5 when a Potion Kit's fan reaches both far ends.
pub fn fan_size(img: &Img, l: &Layout) -> usize {
    if has_card(img, &l.hand5[0], l.k) && has_card(img, &l.hand5[4], l.k) {
        5
    } else {
        3
    }
}

/// Offsets every card is compared at (reference px).
fn shifts() -> Vec<(f64, f64)> {
    let mut v = Vec::new();
    for dy in [-36.0, -27.0, -18.0, -9.0, 0.0, 9.0, 18.0] {
        for dx in [-48.0, -36.0, -24.0, -12.0, 0.0, 12.0, 24.0, 36.0, 48.0] {
            v.push((dx, dy));
        }
    }
    v
}
fn fine() -> Vec<(f64, f64)> {
    let mut v = Vec::new();
    for dy in [-4.0, -2.0, 0.0, 2.0, 4.0] {
        for dx in [-6.0, -4.0, -2.0, 0.0, 2.0, 4.0, 6.0] {
            v.push((dx, dy));
        }
    }
    v
}
const REFINE: usize = 6;
const KEEP_RANKS: usize = 5;

pub struct HandRead {
    pub phase: Option<String>,
    pub names: Vec<String>,
    pub fields: Fields,
    pub problems: Vec<Problem>,
    pub ranks: Option<Ranks>,
}

pub fn read_phase(img: &Img, l: &Layout, data: &ScanData) -> (Option<String>, f64, f64) {
    let sig = signature(img, l.header, 48, 8);
    let mut ranked: Vec<(&String, f64)> = data.headers.iter().map(|(p, hs)| (p, hs.iter().map(|h| dot(h, &sig)).fold(f64::NEG_INFINITY, f64::max))).collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let phase = (ranked[0].1 >= HEADER_MIN).then(|| ranked[0].0.clone());
    (phase, ranked[0].1, ranked.get(1).map_or(0.0, |r| r.1))
}

/// Every known card ranked for one card position, best first.
fn rank(img: &Img, c: &CardPos, k: f64, prefix: &str, data: &ScanData) -> Vec<(String, f64)> {
    use std::cell::RefCell;
    let arts: RefCell<HashMap<(i32, i32), std::rc::Rc<Art>>> = RefCell::new(HashMap::new());
    let txts: RefCell<HashMap<(i32, i32), std::rc::Rc<Vec<f64>>>> = RefCell::new(HashMap::new());
    let key = |dx: f64, dy: f64| (dx as i32, dy as i32);
    let art = |dx: f64, dy: f64| arts.borrow_mut().entry(key(dx, dy)).or_insert_with(|| std::rc::Rc::new(card_art(img, c, k, dx, dy))).clone();
    let text = |dx: f64, dy: f64| txts.borrow_mut().entry(key(dx, dy)).or_insert_with(|| std::rc::Rc::new(card_text(img, c, k, dx, dy))).clone();
    struct Scored<'a> {
        name: String,
        best: f64,
        at: (f64, f64),
        a0: f64,
        texts: Option<&'a Vec<Vec<f64>>>,
    }
    let both = |texts: Option<&Vec<Vec<f64>>>, a: f64, dx: f64, dy: f64| match texts {
        Some(ts) => {
            let t = text(dx, dy);
            0.5 * a + 0.25 * (1.0 + ts.iter().map(|x| dot(x, &t)).fold(f64::NEG_INFINITY, f64::max))
        }
        None => a,
    };
    let sh = shifts();
    let mut scored: Vec<Scored> = data
        .cards
        .iter()
        .filter(|(key, _)| key.starts_with(prefix))
        .map(|(key, fs)| {
            let texts = data.texts.get(key);
            let (mut best, mut at, mut a0) = (f64::NEG_INFINITY, (0.0, 0.0), 0.0);
            for &(dx, dy) in &sh {
                let ar = art(dx, dy);
                let a = fs.iter().map(|f| similarity(f, &ar)).fold(f64::NEG_INFINITY, f64::max);
                let v = both(texts, a, dx, dy);
                if v > best {
                    best = v;
                    at = (dx, dy);
                    a0 = a;
                }
            }
            Scored { name: key[prefix.len()..].to_string(), best, at, a0, texts }
        })
        .collect();
    scored.sort_by(|a, b| b.best.partial_cmp(&a.best).unwrap_or(std::cmp::Ordering::Equal));
    let fi = fine();
    for s in scored.iter_mut().take(REFINE) {
        if s.texts.is_some() {
            for &(fx, fy) in &fi {
                s.best = s.best.max(both(s.texts, s.a0, s.at.0 + fx, s.at.1 + fy));
            }
        }
    }
    let mut out: Vec<(String, f64)> = scored.into_iter().map(|s| (s.name, s.best)).collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    out
}

pub struct SlotsRead {
    pub names: Vec<String>,
    pub fields: Fields,
    pub problems: Vec<Problem>,
    pub ranks: Ranks,
}

pub fn read_at(img: &Img, l: &Layout, size: usize, phase: &str, data: &ScanData, slots: Option<&[usize]>) -> SlotsRead {
    let mut fields = Fields::new();
    let mut problems = Vec::new();
    let mut names = Vec::new();
    let mut ranks: Ranks = Vec::new();
    let pos = l.hand_at(size);
    // the card positions are independent: ranked side by side on every core
    use rayon::prelude::*;
    let prefix = format!("{phase}:");
    let ranked_all: Vec<Option<Vec<(String, f64)>>> = pos
        .par_iter()
        .enumerate()
        .map(|(j, c)| if slots.is_some_and(|s| !s.contains(&j)) { None } else { Some(rank(img, c, l.k, &prefix, data)) })
        .collect();
    for (j, (c, ranked)) in pos.iter().zip(ranked_all).enumerate() {
        let Some(ranked) = ranked else {
            ranks.push(vec![]);
            continue;
        };
        ranks.push(ranked.iter().take(KEEP_RANKS).cloned().collect());
        let key = format!("hand{j}");
        let top = ranked.first();
        let second = ranked.get(1).map(|r| r.1);
        if top.is_none_or(|t| t.1 < ART_UNKNOWN) {
            problems.push(Problem::UnknownArt { field: key.clone(), crop: card_face(c, l.k), hand: (size == 5).then_some(5), text: None });
            fields.insert(key, FieldRead { sure: false, guess: top.map(|t| t.0.clone()), score: top.map(|t| t.1), second });
            continue;
        }
        let t = top.unwrap();
        names.push(t.0.clone());
        fields.insert(key, FieldRead { sure: t.1 >= ART_MIN && t.1 - second.unwrap_or(0.0) >= ART_GAP, guess: Some(t.0.clone()), score: Some(t.1), second });
    }
    SlotsRead { names, fields, problems, ranks }
}

/// Which kind of pick the header shows, without reading the cards.
pub fn header_only(img: &Img, l: &Layout, data: &ScanData) -> HandRead {
    let (phase, score, second) = read_phase(img, l, data);
    let mut fields = Fields::new();
    fields.insert("phase".into(), FieldRead { sure: phase.is_some() && score - second >= 0.05, guess: phase.clone(), score: Some(score), second: Some(second) });
    HandRead { phase, names: vec![], fields, problems: vec![], ranks: None }
}

pub fn read_hand(img: &Img, l: &Layout, data: &ScanData) -> HandRead {
    let head = header_only(img, l, data);
    let Some(phase) = head.phase.clone() else { return HandRead { ranks: Some(vec![]), ..head } };
    let size = if phase == "potion" { fan_size(img, l) } else { 3 };
    let read = read_at(img, l, size, &phase, data, None);
    let mut fields = head.fields;
    fields.extend(read.fields);
    HandRead { phase: Some(phase), names: read.names, fields, problems: read.problems, ranks: Some(read.ranks) }
}
