//! Live advice for the Solver (src/strategy/advisor.ts): given what is on screen, rank the choices by expected
//! payoff. Read-only: it only says what it would play; nothing here acts on the game.

use crate::bot::*;
use crate::data::{data, Targeting};
use crate::game::{can_refresh, card_changes, create_game, ultimate_activity};
use crate::types::*;
use serde::{Deserialize, Serialize, Serializer};
use std::time::Instant;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenMonster {
    pub archetype: Archetype,
    pub rarity: Rarity,
    pub activity: i64,
    pub quantity: i64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScreenPhase {
    Potion,
    Instrument,
    Protocol,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenState {
    /// 6 entries, left to right; null = empty tank
    pub tanks: Vec<Option<ScreenMonster>>,
    /// card names as printed, left to right
    pub hand: Vec<String>,
    /// names of equipped instruments
    pub instruments: Vec<String>,
    /// the round each instrument was picked in; round 1 when absent or 0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrument_rounds: Option<Vec<i32>>,
    pub phase: ScreenPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub potion_refreshes: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrument_refreshes: Option<i32>,
    /// Intern Shelley level (only changes the instrument rerolls a new game starts with)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shelley: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct End {
    pub activity: f64,
    pub reached: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ranked {
    pub index: usize,
    pub targets: Vec<u8>,
    pub value: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<End>,
    /// the card changes nothing on this board (it only adds a monster, and the tanks are full)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_effect: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Best {
    Card(Ranked),
    Refresh,
    /// the choice doesn't change the score
    Nothing,
}

impl Serialize for Best {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Best::Card(r) => r.serialize(s),
            Best::Refresh => s.serialize_str("refresh"),
            Best::Nothing => s.serialize_none(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Advice {
    pub best: Best,
    pub ranking: Vec<Ranked>,
    pub board: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_end: Option<End>,
    /// true: potions judged by look-ahead to the end of the run; false: expected score after this round
    pub plan: bool,
    #[serde(rename = "final")]
    pub final_: bool,
    pub ms: u64,
    pub unknown: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Potion look-ahead: each card on offer, the next best options up to 4, and a reroll, each played to the end 64 times.
pub const LOOK_AHEAD: LookAhead = LookAhead { top: 4, playouts: 64, steer: Some(0.3), inner_samples: None, value: Some(true), halving: None };
/// Futures per instrument choice, each judged by its final score counted up to 840k.
pub const INSTRUMENT_FUTURES: u32 = 64;
/// How the rounds inside an instrument choice's futures are played: by the smarter player (the board value).
const INSTRUMENT_ROLLOUT: Policy = Policy { samples: 4, horizon: 0, roll_samples: None, steer: None, value: Some(true) };
/// A few rounds of look-ahead, only to order options that score the same this round.
const TIE_BREAK: Policy = Policy::new(32, 4);
/// How close a do-nothing move's value must be for a move that does something to go first instead.
const NO_EFFECT_MARGIN: f64 = 0.01;
const SEED: u32 = 0x5eed;

/// JavaScript's Math.round (halves up).
pub fn js_round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 {
        f + 1.0
    } else {
        f
    }
}

pub fn is_xenopanacea(name: &str) -> bool {
    name.trim().to_lowercase().starts_with("xenopanacea")
}

/// A Supreme potion the simulator can't play out, always the advice when offered: a Xenopanacea (its card prints
/// only flavour text) or a Holy Water rite ("… Holy Water: …", whose effect is another card later; Purest Holy
/// Water, with no colon, is simulated).
pub fn is_always_take(name: &str) -> bool {
    is_xenopanacea(name) || name.to_lowercase().contains("holy water:")
}

fn lookup(phase_potion: bool, name: &str) -> Option<u8> {
    if phase_potion {
        data().potion_by_name(name)
    } else {
        data().instrument_by_name(name)
    }
}

fn is_kit(s: &GameState, index: usize) -> bool {
    s.hand.get(index).is_some_and(|c| c.kind == CardKind::Potion && matches!(data().potion(c.id).targeting, Targeting::Box(_)))
}

/// The engine state rebuilt from the screen; names it doesn't know go to `unknown` and are left out.
pub fn to_state(x: &ScreenState, unknown: &mut Vec<String>) -> GameState {
    let mut equipped = Equips::new();
    for (i, n) in x.instruments.iter().enumerate() {
        match data().instrument_by_name(n) {
            Some(id) if equipped.len() < MAX_EQUIPPED => {
                let r = x.instrument_rounds.as_ref().and_then(|r| r.get(i).copied()).filter(|&r| r != 0).unwrap_or(1);
                equipped.push(Equipped { id, acquired_round: r });
            }
            Some(_) => {}
            None => unknown.push(n.clone()),
        }
    }
    let setup = SandboxSetup { tanks: None, instruments: vec![], start_round: 1, thresholds_used: (false, false), forced_hand: vec![], seed: None };
    let s = create_game(data().default_settings, Some(&setup), Some(1));
    let potion = x.phase == ScreenPhase::Potion;
    let mut hand = Hand::new();
    for n in &x.hand {
        match lookup(potion, n) {
            Some(id) if hand.len() < MAX_HAND => hand.push(Card { kind: if potion { CardKind::Potion } else { CardKind::Instrument }, id }),
            Some(_) => {}
            None => unknown.push(n.clone()),
        }
    }
    let mut tanks: Tanks = [None; TANK_COUNT];
    for (t, m) in tanks.iter_mut().zip(x.tanks.iter()) {
        *t = m.as_ref().map(|m| Monster { archetype: m.archetype, rarity: m.rarity, activity: m.activity, quantity: m.quantity });
    }
    let had = equipped.len() + (x.phase == ScreenPhase::Instrument) as usize;
    GameState {
        tanks,
        instruments: equipped,
        round: x.round.unwrap_or(1),
        phase: if potion { Phase::PotionSelect } else { Phase::InstrumentSelect },
        hand,
        potion_refreshes: x.potion_refreshes.unwrap_or(0),
        instrument_refreshes: x.instrument_refreshes.unwrap_or(0),
        // past thresholds only matter for whether more instruments are coming
        t8000: had >= 2,
        t28000: had >= 3,
        ..s
    }
}

/// The futures a board's advice needs, so slices of them can be played on other cores.
#[derive(Clone, Debug)]
pub enum Work {
    Potion { s: GameState, seed: u32, la: LookAhead, opts: Vec<(Opt, f64)>, futures: u32 },
    Instrument { s: GameState, seed: u32, choices: Vec<Pick>, futures: u32 },
}

impl Work {
    pub fn futures(&self) -> u32 {
        match self {
            Work::Potion { futures, .. } | Work::Instrument { futures, .. } => *futures,
        }
    }
}

pub enum Prepared {
    Done(Advice),
    Work { work: Work, x: ScreenState, unknown: Vec<String>, t0: Instant },
}

/// The advice that needs no futures (a protocol, a Xenopanacea or Holy Water rite), or the futures to play for it.
pub fn prepare(x: &ScreenState, futures: Option<u32>) -> Prepared {
    let t0 = Instant::now();
    let mut unknown = Vec::new();
    // Surgical Protocols change the endgame battle, not Ultimate Activity
    if x.phase == ScreenPhase::Protocol {
        return Prepared::Done(Advice { best: Best::Nothing, ranking: vec![], board: 0, refresh_end: None, plan: false, final_: true, ms: 0, unknown, note: None });
    }
    // a ULT the simulator can't play out (a Xenopanacea, a Holy Water rite) is always taken: it's worth more than
    // anything beside it
    if x.phase == ScreenPhase::Potion {
        if let Some(xeno) = x.hand.iter().position(|n| is_always_take(n)) {
            let pick = Ranked { index: xeno, targets: vec![], value: 0.0, end: None, no_effect: None };
            let board = ultimate_activity(&to_state(&ScreenState { hand: vec![], ..x.clone() }, &mut Vec::new()));
            return Prepared::Done(Advice {
                best: Best::Card(pick.clone()),
                ranking: vec![pick],
                board,
                refresh_end: None,
                plan: false,
                final_: true,
                ms: t0.elapsed().as_millis() as u64,
                unknown,
                note: Some(if is_xenopanacea(&x.hand[xeno]) { "A Xenopanacea: always take it when it’s offered.".into() } else { format!("{}: always take it when it’s offered.", x.hand[xeno]) }),
            });
        }
    }
    let s = to_state(x, &mut unknown);
    let work = if s.phase == Phase::PotionSelect {
        let n = futures.unwrap_or(LOOK_AHEAD.playouts);
        Work::Potion { s, seed: SEED, la: LookAhead { playouts: n, ..LOOK_AHEAD }, opts: shortlist(&s, SEED, &LOOK_AHEAD), futures: n }
    } else {
        let mut choices: Vec<Pick> = (0..s.hand.len()).map(Pick::Card).collect();
        if can_refresh(&s) {
            choices.push(Pick::Refresh);
        }
        Work::Instrument { s, seed: SEED, choices, futures: futures.unwrap_or(INSTRUMENT_FUTURES) }
    };
    Prepared::Work { work, x: x.clone(), unknown, t0 }
}

/// Futures `from` to `to` (excluded) of every option; `par` spreads them over every core.
pub fn play_work(w: &Work, from: u32, to: u32, par: bool) -> Vec<Tally> {
    match w {
        Work::Potion { s, seed, la, opts, .. } => {
            let o: Vec<Opt> = opts.iter().map(|c| c.0).collect();
            potion_futures(s, *seed, la, &o, from, to, par)
        }
        Work::Instrument { s, seed, choices, .. } => instrument_futures(s, *seed, choices, &Chooser::Ev(INSTRUMENT_ROLLOUT), Goal::Target, from, to, par),
    }
}

fn to_ranked(o: &Opt, value: f64) -> Ranked {
    match o {
        Opt::Card { index, targets } => Ranked { index: *index as usize, targets: targets.as_slice().to_vec(), value, end: None, no_effect: None },
        Opt::Refresh => Ranked { index: usize::MAX, targets: vec![], value, end: None, no_effect: None },
    }
}

/// Marks the moves that change nothing, and puts a move that does something first when it's about as good.
pub fn mark_no_effect(s: &GameState, ranking: Vec<Ranked>) -> Vec<Ranked> {
    let marked: Vec<Ranked> = ranking
        .into_iter()
        .map(|r| if card_changes(s, r.index, &r.targets) { r } else { Ranked { no_effect: Some(true), ..r } })
        .collect();
    let Some(top) = marked.first() else { return marked };
    if top.no_effect.is_none() {
        return marked;
    }
    let top_value = top.value;
    match marked.iter().position(|r| r.no_effect.is_none() && r.value >= top_value * (1.0 - NO_EFFECT_MARGIN)) {
        Some(i) => {
            let mut out = vec![marked[i].clone()];
            out.extend(marked.iter().enumerate().filter(|(k, _)| *k != i).map(|(_, r)| r.clone()));
            out
        }
        None => marked,
    }
}

/// Options tied with the best this round are put in order by playing a few more rounds.
fn break_ties(s: &GameState, ranking: Vec<Ranked>, seed: u32) -> Vec<Ranked> {
    let Some(first) = ranking.first() else { return ranking };
    let top = first.value;
    let n = ranking.iter().filter(|r| r.value == top).count();
    if n < 2 {
        return ranking;
    }
    let mut tied: Vec<(f64, Ranked)> = ranking[..n].iter().map(|r| (expected(s, &Opt::card(r.index, &r.targets), &TIE_BREAK, seed, 0), r.clone())).collect();
    tied.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<Ranked> = tied.into_iter().map(|(_, r)| r).collect();
    out.extend(ranking[n..].iter().cloned());
    out
}

/// The engine's hand leaves unknown cards out: each choice gets its place on screen.
fn on_screen(x: &ScreenState, mut a: Advice) -> Advice {
    let potion = x.phase == ScreenPhase::Potion;
    let at: Vec<usize> = x.hand.iter().enumerate().filter(|(_, n)| lookup(potion, n).is_some()).map(|(i, _)| i).collect();
    let place = |r: &mut Ranked| r.index = at.get(r.index).copied().unwrap_or(r.index);
    a.ranking.iter_mut().for_each(place);
    if let Best::Card(r) = &mut a.best {
        place(r);
    }
    a
}

/// The advice from the futures played (every option's totals).
pub fn conclude(work: &Work, x: &ScreenState, unknown: Vec<String>, t0: Instant, tallies: &[Tally]) -> Advice {
    let mean = |t: &Tally| t.goal / t.n as f64;
    let reached = |t: &Tally| t.reached as f64 / t.n as f64;
    let (s, ranking, best, refresh_end, plan) = match work {
        Work::Potion { s, opts, .. } => {
            // options dropped early by halving (fewer futures) rank below the ones kept
            let mut ahead: Vec<(Opt, f64, f64, u32)> = opts.iter().zip(tallies).map(|(c, t)| (c.0, mean(t), reached(t), t.n)).collect();
            ahead.sort_by(|a, b| b.3.cmp(&a.3).then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)));
            let reroll = ahead.iter().position(|a| a.0 == Opt::Refresh);
            let refresh_end = reroll.map(|i| End { activity: ahead[i].1, reached: ahead[i].2 });
            let ranked: Vec<Ranked> = ahead
                .iter()
                .enumerate()
                .filter(|(i, _)| Some(*i) != reroll)
                .map(|(_, a)| Ranked { end: Some(End { activity: a.1, reached: a.2 }), ..to_ranked(&a.0, js_round(a.1)) })
                .collect();
            let ranking = mark_no_effect(s, ranked);
            let mut best = if reroll == Some(0) { Best::Refresh } else { ranking.first().cloned().map_or(Best::Nothing, Best::Card) };
            // a Potion Kit on offer is taken before a refresh
            if best == Best::Refresh {
                if let Some(k) = ranking.iter().find(|r| is_kit(s, r.index)) {
                    best = Best::Card(k.clone());
                }
            }
            (s, ranking, best, refresh_end, true)
        }
        Work::Instrument { s, choices, .. } => {
            let mut ranking: Vec<Ranked> = choices
                .iter()
                .zip(tallies)
                .filter_map(|(c, t)| match c {
                    Pick::Card(i) => Some(Ranked { index: *i, targets: vec![], value: js_round(mean(t)), end: Some(End { activity: mean(t), reached: reached(t) }), no_effect: None }),
                    Pick::Refresh => None,
                })
                .collect();
            ranking.sort_by(|a, b| b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal));
            let refresh_end = choices.iter().position(|c| *c == Pick::Refresh).map(|r| End { activity: mean(&tallies[r]), reached: reached(&tallies[r]) });
            let best = match (refresh_end, ranking.first()) {
                (Some(e), Some(top)) if e.activity > top.value => Best::Refresh,
                (_, Some(top)) => Best::Card(top.clone()),
                (Some(_), None) => Best::Refresh,
                (None, None) => Best::Nothing,
            };
            (s, ranking, best, refresh_end, false)
        }
    };
    on_screen(x, Advice { best, ranking, board: ultimate_activity(s), refresh_end, plan, final_: true, ms: t0.elapsed().as_millis() as u64, unknown, note: None })
}

/// The Solver's advice, with every future played (spread over every core when `par`).
pub fn advise_plan(x: &ScreenState, futures: Option<u32>, par: bool) -> Advice {
    advise_halved(x, futures, false, par)
}

/// `advise_plan`, the potions' futures spread by successive halving when `halving` (`halved_futures`): the same
/// futures on average, most of them on the close calls between the best options.
pub fn advise_halved(x: &ScreenState, futures: Option<u32>, halving: bool, par: bool) -> Advice {
    match prepare(x, futures) {
        Prepared::Done(a) => a,
        Prepared::Work { work, x, unknown, t0 } => {
            let tallies = match &work {
                Work::Potion { s, seed, la, opts, .. } if halving => {
                    let o: Vec<Opt> = opts.iter().map(|c| c.0).collect();
                    halved_futures(s, *seed, la, &o, par)
                }
                _ => play_work(&work, 0, work.futures(), par),
            };
            conclude(&work, &x, unknown, t0, &tallies)
        }
    }
}

/// Rank the current choices. `plan`: potions judged by look-ahead to the end of the run (the Solver's advice);
/// otherwise by this round's expected score over `samples` outcomes. Instruments are always judged by the end.
pub fn advise(x: &ScreenState, samples: u32, plan: bool, par: bool) -> Advice {
    if x.phase != ScreenPhase::Potion || plan {
        return advise_plan(x, None, par);
    }
    let t0 = Instant::now();
    if x.hand.iter().any(|n| is_always_take(n)) {
        if let Prepared::Done(a) = prepare(x, None) {
            return a;
        }
    }
    let mut unknown = Vec::new();
    let s = to_state(x, &mut unknown);
    let p = Policy::new(samples, 0);
    let mut ranked: Vec<Ranked> = options(&s).iter().map(|o| to_ranked(o, js_round(expected(&s, o, &p, SEED, 0)))).collect();
    ranked.sort_by(|a, b| b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal));
    let ranking = mark_no_effect(&s, break_ties(&s, ranked, SEED));
    let ua = ultimate_activity(&s);
    let mut best = match ranking.first() {
        Some(r) if !(r.value <= ua as f64 && s.potion_refreshes > 0) => Best::Card(r.clone()),
        _ => Best::Refresh,
    };
    if best == Best::Refresh {
        if let Some(k) = ranking.iter().find(|r| is_kit(&s, r.index)) {
            best = Best::Card(k.clone());
        }
    }
    on_screen(x, Advice { best, ranking, board: ua, refresh_end: None, plan: false, final_: false, ms: t0.elapsed().as_millis() as u64, unknown, note: None })
}

/// Every card and instrument name the screen reader should know about.
pub fn names() -> (Vec<String>, Vec<String>) {
    let d = data();
    (d.potions.iter().map(|p| p.name.clone()).collect(), d.instruments.iter().map(|i| i.name.clone()).collect())
}
