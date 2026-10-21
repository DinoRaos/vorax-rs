//! Reads a frame of the Surgical Preparation screen into the advisor's ScreenState (src/solver/scan/scan.ts).

use crate::cards::{cards_present, fan_size, header_only, read_hand};
use crate::data::ScanData;
use crate::instruments::read_instruments;
use crate::layout::{layout_at, UI_SCALES};
use std::cell::Cell;
use crate::levels::{normal_colours, read_areas};
use crate::pixels::{gray, js_round, Img};
use crate::reading::{empty_state, FieldRead, Fields, Frame, Problem, ReadDebug, Reading};
use crate::tanks::read_tank;
use crate::text::{read_text, TextKind};
use vorax::advisor::ScreenPhase;

const DIGIT_MIN: f64 = 0.75;

/// Upstream's reading, exactly (the parity tests hold this one to the TypeScript). `pictures`: false leaves the
/// cards to their printed names (read_titles afterwards). The interface scale that read last is tried first; when
/// the round can't be read, the others are.
pub fn scan_upstream(captured: &Img, data: &ScanData, pictures: bool) -> Reading {
    scan_scales(captured, data, pictures, true)
}

/// `upstream`: exactly upstream's reading; else with this port's own fixes (an empty tank's glow).
fn scan_scales(captured: &Img, data: &ScanData, pictures: bool, upstream: bool) -> Reading {
    let last = LAST_UI.get();
    let first = scan_with(captured, data, pictures, Some(0.0), true, last, upstream);
    // a picture that isn't the board, or a card mid-flight, isn't read better at another scale
    if first.fields.get("round").is_some_and(|f| f.sure) || first.has(|p| matches!(p, Problem::Animating | Problem::NotFound)) {
        return first;
    }
    for ui in UI_SCALES.into_iter().filter(|&u| u != last) {
        let r = scan_with(captured, data, pictures, Some(0.0), true, ui, upstream);
        if r.fields.get("round").is_some_and(|f| f.sure) {
            LAST_UI.set(ui);
            return r;
        }
    }
    first
}

thread_local! {
    /// the interface scale the last reading was made at (upstream keeps one for its reader; the overlay reads on
    /// one thread)
    static LAST_UI: Cell<f64> = const { Cell::new(1.0) };
}

/// Where a hand's header may sit below or above the layout's place (reference pixels), nearest first.
const HAND_SHIFTS: [f64; 8] = [24.0, 16.0, 32.0, 8.0, 40.0, -8.0, -16.0, 48.0];

/// The reading, with two fallbacks beyond upstream for screens it wasn't measured on, tried only when its own
/// reading struggles: the picture's own colours when the colour correction kicked in (it can spoil the digits),
/// and the hand searched a little higher or lower when its header isn't where the layout puts it. The surest
/// reading wins.
pub fn scan(captured: &Img, data: &ScanData, pictures: bool) -> Reading {
    let first = scan_scales(captured, data, pictures, false);
    if first.has(|p| matches!(p, Problem::Animating)) {
        return first;
    }
    let corrected = first.debug.as_ref().is_some_and(|d| d.corrected);
    let header_lost = first.fields.get("phase").is_some_and(|f| f.guess.is_none());
    let unsure = first.problems.iter().any(|p| matches!(p, Problem::Unreadable { .. } | Problem::UnknownArt { .. })) || first.fields.get("refreshes").is_some_and(|f| !f.sure);
    if !header_lost && !(corrected && unsure) {
        return first;
    }
    let ui = first.ui();
    let mut best = first;
    for correct in if corrected { vec![true, false] } else { vec![true] } {
        if correct && !header_lost {
            continue;
        }
        let r = scan_with(captured, data, pictures, None, correct, ui, false);
        if quality(&r) > quality(&best) {
            best = r;
        }
    }
    best
}

/// How sure a reading is: its sure fields, less its problems; a hand read counts most.
fn quality(r: &Reading) -> f64 {
    if r.has(|p| matches!(p, Problem::NotFound | Problem::Animating)) {
        return f64::NEG_INFINITY;
    }
    let sure = r.fields.values().filter(|f| f.sure).count() as f64;
    let phase = r.fields.get("phase").is_some_and(|f| f.guess.is_some()) as u8 as f64;
    sure - 2.0 * r.problems.len() as f64 + 10.0 * phase + 3.0 * r.state.hand.len() as f64
}

/// `hand_dy`: where the hand sits against the layout (reference pixels), or None to search for it when its header
/// isn't at the layout's place; `correct`: map the colours to a normal capture's when they're far off; `ui`: the
/// game's interface scale; `upstream`: without this port's own fixes (tanks.rs's empty_glow).
pub fn scan_with(captured: &Img, data: &ScanData, pictures: bool, hand_dy: Option<f64>, correct: bool, ui: f64, upstream: bool) -> Reading {
    let l0 = layout_at(captured.width, captured.height, ui);
    let (corrected_img, off, corrected) = normal_colours(captured, &data.levels, &read_areas(&l0));
    let (corrected_img, corrected) = if correct { (corrected_img, corrected) } else { (None, false) };
    let img = corrected_img.as_ref().unwrap_or(captured);
    let dy = match hand_dy {
        Some(d) => d,
        None => find_hand(img, &l0, data),
    };
    let l = l0.shifted_hand(dy);
    let frame = Some(Frame { width: img.width, height: img.height });
    let bare = |p: Problem| Reading { state: empty_state(), fields: Fields::new(), problems: vec![p], frame: frame.clone(), instrument_ranks: None, hand_ranks: None, icons_known: None, hand_pending: None, hand_size: None, ui: None, debug: None, hand_dy: None };

    let present = cards_present(img, &l);
    if present == 1 || present == 2 {
        return bare(Problem::Animating);
    }
    let hand = if present == 0 {
        let h = header_only(img, &l, data);
        crate::cards::HandRead { ranks: Some(vec![]), ..h }
    } else if pictures {
        read_hand(img, &l, data)
    } else {
        header_only(img, &l, data)
    };
    let g = gray(img);
    let tanks: Vec<_> = (0..6).map(|i| read_tank(img, &g, &l, i, data, !upstream)).collect();
    if hand.phase.is_none() && tanks.iter().all(|t| t.monster.is_none()) {
        return bare(Problem::NotFound);
    }
    let mut fields = Fields::new();
    let mut problems = Vec::new();
    fields.extend(hand.fields.clone());
    problems.extend(hand.problems.clone());
    for t in &tanks {
        fields.extend(t.fields.clone());
    }
    let phase = match hand.phase.as_deref() {
        Some("instrument") => ScreenPhase::Instrument,
        Some("protocol") => ScreenPhase::Protocol,
        _ => ScreenPhase::Potion,
    };
    let mut state = vorax::advisor::ScreenState {
        tanks: tanks.iter().map(|t| t.monster.clone()).collect(),
        hand: hand.names.clone(),
        instruments: vec![],
        instrument_rounds: None,
        phase,
        round: None,
        potion_refreshes: None,
        instrument_refreshes: None,
        shelley: None,
    };

    let round = read_text(img, &g, &l.round, TextKind::Round, &data.digits["round"]);
    let n: f64 = round.as_ref().and_then(|r| (!r.text.is_empty()).then(|| r.text.parse::<f64>().unwrap_or(f64::NAN))).unwrap_or(if round.is_some() { 0.0 } else { f64::NAN });
    let round_ok = round.as_ref().is_some_and(|r| n.fract() == 0.0 && (1.0..=13.0).contains(&n) && r.score >= DIGIT_MIN);
    state.round = Some(if round_ok { n as i32 } else { 1 });
    fields.insert("round".into(), FieldRead { sure: round_ok, guess: round.as_ref().map(|r| r.text.clone()), score: round.as_ref().map(|r| r.score), second: round.as_ref().map(|r| r.second) });
    if !round_ok {
        problems.push(Problem::Unreadable { field: "round".into() });
    }

    if phase != ScreenPhase::Protocol {
        let rf = read_text(img, &g, &l.refresh, TextKind::Refresh, &data.digits["refresh"]);
        let left: f64 = rf.as_ref().map_or(f64::NAN, |r| if r.text.is_empty() { 0.0 } else { r.text.parse::<f64>().unwrap_or(f64::NAN) });
        let ok = rf.as_ref().is_some_and(|r| left.fract() == 0.0 && (0.0..=9.0).contains(&left) && r.score >= DIGIT_MIN);
        let v = Some(if ok { left as i32 } else { 0 });
        if phase == ScreenPhase::Potion {
            state.potion_refreshes = v;
        } else {
            state.instrument_refreshes = v;
        }
        fields.insert("refreshes".into(), FieldRead { sure: ok, guess: rf.as_ref().map(|r| r.text.clone()), score: rf.as_ref().map(|r| r.score), second: rf.as_ref().map(|r| r.second) });
    }

    let inst = read_instruments(img, &l, data);
    state.instruments = inst.names.clone();
    fields.extend(inst.fields.clone());

    let unreadable: Vec<String> = fields
        .iter()
        .filter(|(k, f)| !f.sure && is_tank_stat(k))
        .map(|(k, _)| k.clone())
        .filter(|k| !problems.iter().any(|p| matches!(p, Problem::Unreadable { field } if field == k)))
        .collect();
    for k in unreadable {
        if !problems.iter().any(|p| matches!(p, Problem::Unreadable { field } if *field == k)) {
            problems.push(Problem::Unreadable { field: k });
        }
    }
    let pending = hand.ranks.is_none();
    Reading {
        state,
        fields,
        problems,
        frame,
        instrument_ranks: Some(inst.ranks),
        icons_known: Some(inst.known),
        hand_pending: pending.then_some(true),
        hand_size: pending.then(|| if phase == ScreenPhase::Protocol { 3 } else { fan_size(img, &l) }),
        hand_ranks: hand.ranks,
        ui: (ui != 1.0).then_some(ui),
        debug: Some(ReadDebug { off: js_round(off * 1000.0) / 1000.0, corrected, tanks: tanks.into_iter().map(|t| t.debug).collect(), ms: None }),
        hand_dy: (dy != 0.0).then_some(dy),
    }
}

/// How far the hand's header sits from the layout's place: 0 when it's there, else the nearest shift where it reads.
fn find_hand(img: &Img, l: &crate::layout::Layout, data: &ScanData) -> f64 {
    if crate::cards::read_phase(img, l, data).0.is_some() {
        return 0.0;
    }
    let mut best = (0.0, f64::NEG_INFINITY);
    for dy in HAND_SHIFTS {
        let (phase, score, _) = crate::cards::read_phase(img, &l.shifted_hand(dy), data);
        if phase.is_some() && score > best.1 {
            best = (dy, score);
        }
    }
    best.0
}

/// tank0.activity … tank5.quantity
fn is_tank_stat(k: &str) -> bool {
    let b = k.as_bytes();
    k.len() > 6 && k.starts_with("tank") && b[4].is_ascii_digit() && b[5] == b'.' && matches!(&k[6..], "activity" | "quantity")
}
