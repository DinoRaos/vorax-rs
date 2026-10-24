//! The strategy bots (src/strategy/bot.ts). Every policy judges a move by its expected payoff: the move is
//! replayed under fresh random outcomes (the real RNG state is never peeked at) and the outcomes are averaged.

use crate::data::data;
use crate::game::{can_refresh, create_game, dispatch, needs_targets, ultimate_activity};
use crate::stack::StackVec;
use crate::types::*;
use crate::value::{board_value, TARGET};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

/// One way to play the current hand: a card on up to two targets, or a reroll.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Opt {
    Card { index: u8, targets: Targets },
    #[default]
    Refresh,
}

impl Opt {
    pub fn card(index: usize, targets: &[u8]) -> Opt {
        Opt::Card { index: index as u8, targets: Targets::from_slice(targets) }
    }

    /// The action sequence it stands for: [select, ...targets, confirm] or [refresh].
    pub fn actions(&self) -> Vec<Action> {
        match self {
            Opt::Refresh => vec![Action::Refresh],
            Opt::Card { index, targets } => {
                let mut v = vec![Action::SelectCard { index: *index as i64 }];
                v.extend(targets.iter().map(|&t| Action::ToggleTarget { tank: t as i64 }));
                v.push(Action::Confirm);
                v
            }
        }
    }

    pub fn index(&self) -> Option<usize> {
        match self {
            Opt::Card { index, .. } => Some(*index as usize),
            Opt::Refresh => None,
        }
    }
}

#[inline]
pub fn play_opt(s: &GameState, o: &Opt) -> GameState {
    match o {
        Opt::Refresh => dispatch(s, Action::Refresh),
        Opt::Card { index, targets } => {
            let mut x = dispatch(s, Action::SelectCard { index: *index as i64 });
            for &t in targets {
                x = dispatch(&x, Action::ToggleTarget { tank: t as i64 });
            }
            dispatch(&x, Action::Confirm)
        }
    }
}

#[inline]
pub fn pick_and_confirm(s: &GameState, i: usize) -> GameState {
    let x = dispatch(s, Action::SelectCard { index: i as i64 });
    dispatch(&x, Action::Confirm)
}

pub type Opts = StackVec<Opt, 128>;

/// Every way to play the current hand: [select, ...targets, confirm].
pub fn options(s: &GameState) -> Opts {
    let mut out = Opts::new();
    for i in 0..s.hand.len() {
        let sel = dispatch(s, Action::SelectCard { index: i as i64 });
        let mode = needs_targets(&sel);
        let tanks = sel.non_empty();
        match mode {
            None => out.push(Opt::card(i, &[])),
            Some(m) => {
                for &t in &tanks {
                    out.push(Opt::card(i, &[t]));
                }
                if m == crate::data::Targeting::Select1To2 {
                    for &a in &tanks {
                        for &b in &tanks {
                            if a < b {
                                out.push(Opt::card(i, &[a, b]));
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

#[inline]
fn reseed(s: &GameState, seed: u32) -> GameState {
    GameState { rng_state: seed, ..*s }
}

#[inline]
pub fn mix(a: u32, b: u32) -> u32 {
    (a ^ b.wrapping_add(0x9e37_79b9)).wrapping_mul(0x85eb_ca6b) ^ (a >> 13)
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    /// fresh random outcomes averaged per option
    pub samples: u32,
    /// remaining rounds played by a cheap rollout after the move (0 = judge on this round's result)
    pub horizon: u32,
    /// fresh outcomes averaged per option inside the rollout (default 1)
    #[serde(default)]
    pub roll_samples: Option<u32>,
    /// steer toward what the instruments need (0 = not at all)
    #[serde(default)]
    pub steer: Option<f64>,
    /// judge boards by the board value instead of their score (overrides steer)
    #[serde(default)]
    pub value: Option<bool>,
}

impl Policy {
    pub const fn new(samples: u32, horizon: u32) -> Self {
        Policy { samples, horizon, roll_samples: None, steer: None, value: None }
    }
    fn steer(&self) -> f64 {
        self.steer.unwrap_or(0.0)
    }
    fn value(&self) -> bool {
        self.value.unwrap_or(false)
    }
}

#[inline]
fn worth(s: &GameState, steer: f64, value: bool) -> f64 {
    if value {
        board_value(s)
    } else {
        let ua = ultimate_activity(s) as f64;
        if steer != 0.0 {
            ua * (1.0 + steer * crate::value::build_progress(s))
        } else {
            ua
        }
    }
}

/// Score of a state after resolving any open box the best way (by expectation, recursively).
fn settle(s: &GameState, p: &Policy, seed: u32, depth: u32) -> f64 {
    if s.phase == Phase::PotionSelect && s.open_box.is_some() && depth < 2 {
        let mut best = -1.0f64;
        for o in &options(s) {
            best = best.max(expected(s, o, p, seed, depth + 1));
        }
        return best;
    }
    rollout(s, p.horizon, seed, p.roll_samples.unwrap_or(1), p.steer(), p.value())
}

/// Open a box (if any) and take its first card; only used inside rollouts.
#[inline]
fn resolve_box(n: GameState) -> GameState {
    if n.phase == Phase::PotionSelect && n.open_box.is_some() {
        play_opt(&n, &options(&n)[0])
    } else {
        n
    }
}

/// Lookahead: play `rounds` more rounds, each card the one with the best mean over `samples` fresh outcomes.
fn rollout(s: &GameState, rounds: u32, seed: u32, samples: u32, steer: f64, value: bool) -> f64 {
    let mut s = *s;
    let mut r = 0u32;
    while r < rounds && s.phase != Phase::GameOver {
        if s.phase == Phase::RoundEnd {
            s = dispatch(&s, Action::Confirm);
            r += 1;
            continue;
        }
        if s.phase == Phase::InstrumentSelect {
            s = match value_pick(&s) {
                Pick::Refresh => dispatch(&s, Action::Refresh),
                Pick::Card(i) => pick_and_confirm(&s, i),
            };
            r += 1;
            continue;
        }
        let opts = options(&s);
        let mut best = opts[0];
        let mut bv = -1.0;
        for o in &opts {
            let mut v = 0.0;
            for k in 0..samples {
                v += worth(&resolve_box(play_opt(&reseed(&s, mix(seed, r * 977 + k)), o)), steer, value);
            }
            if v > bv {
                bv = v;
                best = *o;
            }
        }
        s = resolve_box(play_opt(&reseed(&s, mix(seed, r * 977 + 500)), &best));
        r += 1;
    }
    worth(&s, steer, value)
}

/// Mean payoff of playing option `o`, over p.samples fresh outcomes (common seeds across options).
pub fn expected(s: &GameState, o: &Opt, p: &Policy, seed: u32, depth: u32) -> f64 {
    // a box's contents are judged with fewer samples, or nested boxes blow up K × K × options
    let n = if depth == 0 { p.samples } else { 2.max(p.samples >> 2) };
    let mut sum = 0.0;
    for k in 0..n {
        let sk = mix(seed, k + 1 + depth * 101);
        sum += settle(&play_opt(&reseed(s, sk), o), p, sk, depth);
    }
    sum / n as f64
}

/// The chooser's answer: the option, its value, and what it must beat to be played rather than rerolled.
#[derive(Clone, Copy, Debug)]
pub struct Choice {
    pub best: Option<Opt>,
    pub value: f64,
    pub stay: Option<f64>,
}

/// Best option by expected payoff, and its value (steered: as a score, so it still compares with the board's).
pub fn choose(s: &GameState, p: &Policy, seed: u32) -> Choice {
    let mut best = None;
    let mut bv = f64::NEG_INFINITY;
    for o in &options(s) {
        let v = expected(s, o, p, seed, 0);
        if v > bv {
            bv = v;
            best = Some(*o);
        }
    }
    if p.value() {
        return Choice { best, value: bv, stay: Some(board_value(s)) };
    }
    let value = if p.steer() != 0.0 { bv - worth(s, p.steer(), false) + ultimate_activity(s) as f64 } else { bv };
    Choice { best, value, stay: None }
}


/// How an instrument offer is answered: a hand index, or a reroll.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    Card(usize),
    Refresh,
}

#[derive(Clone, Copy, Debug)]
pub enum Picker {
    /// by the board value: the offered instrument whose board is worth most, or a reroll when none beats the board
    Value,
    /// board-aware: every offer (and a reroll) played to the end `playouts` times
    Board { playouts: u32, rollout: Policy, goal: Goal, refresh_playouts: u32 },
    /// a cheater, for the ceiling: see `Chooser::Oracle`
    Oracle,
}

pub fn value_pick(s: &GameState) -> Pick {
    let mut best = 0usize;
    let mut bv = f64::NEG_INFINITY;
    for i in 0..s.hand.len() {
        let v = board_value(&pick_and_confirm(s, i));
        if v > bv {
            bv = v;
            best = i;
        }
    }
    if can_refresh(s) && bv < board_value(s) {
        Pick::Refresh
    } else {
        Pick::Card(best)
    }
}

impl Picker {
    pub fn pick(&self, s: &GameState, seed: u32) -> Pick {
        match self {
            Picker::Value => value_pick(s),
            Picker::Board { playouts, rollout, goal, refresh_playouts } => {
                let r = rank_instruments(s, seed, *playouts, &Chooser::Ev(*rollout), *goal, *refresh_playouts, false);
                let mut best = 0;
                for (i, &v) in r.offers.iter().enumerate() {
                    if v > r.offers[best] {
                        best = i;
                    }
                }
                match r.refresh {
                    Some(rv) if rv > r.offers[best] => Pick::Refresh,
                    _ => Pick::Card(best),
                }
            }
            Picker::Oracle => {
                let mut choices: Vec<Pick> = (0..s.hand.len()).map(Pick::Card).collect();
                if can_refresh(s) {
                    choices.push(Pick::Refresh);
                }
                let end = |c: &Pick| real_end(&match c {
                    Pick::Refresh => dispatch(s, Action::Refresh),
                    Pick::Card(i) => pick_and_confirm(s, *i),
                }, seed);
                let mut best = (choices[0], f64::NEG_INFINITY);
                for c in &choices {
                    let v = end(c);
                    if v > best.1 {
                        best = (*c, v);
                    }
                }
                best.0
            }
        }
    }
}

/// How a game really ends from `s`: played on with its own random draws (the cards and outcomes still to come),
/// by the steered player (judging boards by their score, as tli-vorax-simulator's ORACLE did), counted up to
/// TARGET. Only a cheater can know this.
fn real_end(s: &GameState, seed: u32) -> f64 {
    let inner = Chooser::Ev(Policy { samples: 2, horizon: 0, roll_samples: None, steer: Some(0.3), value: None });
    goal_score(ultimate_activity(&finish(s, seed, &inner, &Picker::Value)))
}

/// The score the advice aims for: 840,000 on the board, which the endgame battle bonus takes to rainbow.
pub fn goal_score(ua: i64) -> f64 {
    ua.min(TARGET) as f64
}

/// What an instrument choice is judged by: the score counted up to TARGET, or the plain average final score.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Goal {
    Target,
    Mean,
}

/// Totals over some of an option's futures; slices of futures add up to the totals over all of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Tally {
    pub goal: f64,
    pub reached: u32,
    pub n: u32,
}

impl Tally {
    fn add(&mut self, o: &Tally) {
        self.goal += o.goal;
        self.reached += o.reached;
        self.n += o.n;
    }
}

/// An instrument offer's answer: the card at a hand index, or a reroll.
pub type InstrumentChoice = Pick;

/// How a potion hand is answered inside a game.
#[derive(Clone, Copy, Debug)]
pub enum Chooser {
    Ev(Policy),
    Plan(LookAhead),
    /// A cheater, for the ceiling no real player can reach: each option (and a reroll) played to the end with the
    /// game's real upcoming cards and outcomes, the best real ending taken (tli-vorax-simulator's ORACLE, MIT)
    Oracle,
}

impl Chooser {
    pub fn choose(&self, s: &GameState, seed: u32) -> Choice {
        match self {
            Chooser::Ev(p) => choose(s, p, seed),
            Chooser::Plan(la) => {
                let best = look_ahead(s, seed, la, false);
                match best.first() {
                    Some(a) => Choice { best: Some(a.o), value: a.now, stay: None },
                    None => Choice { best: None, value: -1.0, stay: None },
                }
            }
            Chooser::Oracle => {
                let mut opts: Vec<Opt> = options(s).iter().copied().collect();
                if s.open_box.is_none() && can_refresh(s) {
                    opts.push(Opt::Refresh);
                }
                let mut best = (None, f64::NEG_INFINITY);
                for o in &opts {
                    let v = real_end(&play_opt(s, o), seed);
                    if v > best.1 {
                        best = (Some(*o), v);
                    }
                }
                Choice { best: best.0, value: f64::INFINITY, stay: None }
            }
        }
    }
}

/// Play a game on from state `s` to the end.
pub fn finish(s: &GameState, seed: u32, chooser: &Chooser, picker: &Picker) -> GameState {
    let mut s = *s;
    for g in 0..300u32 {
        if s.phase == Phase::GameOver {
            break;
        }
        if s.phase == Phase::RoundEnd {
            s = dispatch(&s, Action::Confirm);
            continue;
        }
        if s.phase == Phase::InstrumentSelect {
            s = match picker.pick(&s, mix(seed, 5000 + g)) {
                Pick::Refresh => dispatch(&s, Action::Refresh),
                Pick::Card(i) => pick_and_confirm(&s, i),
            };
            continue;
        }
        let c = chooser.choose(&s, mix(seed, g));
        // refresh a potion hand only when nothing in it is expected to beat the current board
        if s.open_box.is_none() && c.value <= c.stay.unwrap_or(ultimate_activity(&s) as f64) && can_refresh(&s) {
            s = dispatch(&s, Action::Refresh);
            continue;
        }
        if let Some(o) = c.best {
            s = play_opt(&s, &o);
        }
    }
    s
}

/// One future of instrument choice `c`: played to the end, its goal value and whether it reached TARGET.
fn instrument_future(slim: &GameState, seed: u32, c: Pick, rollout: &Chooser, goal: Goal, k: u32, picker: &Picker) -> Tally {
    let sk = mix(seed, 7000 + k);
    let x = reseed(slim, sk);
    let start = match c {
        Pick::Refresh => dispatch(&x, Action::Refresh),
        Pick::Card(i) => pick_and_confirm(&x, i),
    };
    let end = ultimate_activity(&finish(&start, sk, rollout, picker));
    Tally { goal: if goal == Goal::Target { goal_score(end) } else { end as f64 }, reached: (end >= TARGET) as u32, n: 1 }
}

/// Futures `from` to `to` (excluded) of each instrument choice, played to the end of the run.
/// Later instrument offers inside the futures are answered by the board value. `par`: over every core.
#[allow(clippy::too_many_arguments)]
pub fn instrument_futures(s: &GameState, seed: u32, choices: &[Pick], rollout: &Chooser, goal: Goal, from: u32, to: u32, par: bool) -> Vec<Tally> {
    tallies(choices.len(), from, to, par, |c, k| instrument_future(s, seed, choices[c], rollout, goal, k, &Picker::Value))
}

/// Futures from..to for each of `n` options, totalled per option; in parallel when `par`. Goal values are whole
/// numbers, so the totals are exact whatever order the cores add them in.
fn tallies(n: usize, from: u32, to: u32, par: bool, one: impl Fn(usize, u32) -> Tally + Sync) -> Vec<Tally> {
    let per = to.saturating_sub(from) as usize;
    let mut out = vec![Tally::default(); n];
    if par && n * per > 1 {
        let all: Vec<(usize, Tally)> = (0..n * per).into_par_iter().map(|j| (j / per, one(j / per, from + (j % per) as u32))).collect();
        for (c, t) in all {
            out[c].add(&t);
        }
    } else {
        for (c, t) in out.iter_mut().enumerate() {
            for k in from..to {
                t.add(&one(c, k));
            }
        }
    }
    out
}

pub struct InstrumentRanking {
    pub offers: Vec<f64>,
    pub refresh: Option<f64>,
    pub reached: Vec<f64>,
    pub refresh_reached: Option<f64>,
}

#[allow(clippy::too_many_arguments)]
pub fn rank_instruments(s: &GameState, seed: u32, playouts: u32, rollout: &Chooser, goal: Goal, refresh_playouts: u32, par: bool) -> InstrumentRanking {
    let choices: Vec<Pick> = (0..s.hand.len()).map(Pick::Card).collect();
    let each = instrument_futures(s, seed, &choices, rollout, goal, 0, playouts, par);
    let r = can_refresh(s).then(|| instrument_futures(s, seed, &[Pick::Refresh], rollout, goal, 0, refresh_playouts, par)[0]);
    InstrumentRanking {
        offers: each.iter().map(|e| e.goal / e.n as f64).collect(),
        refresh: r.map(|r| r.goal / r.n as f64),
        reached: each.iter().map(|e| e.reached as f64 / e.n as f64).collect(),
        refresh_reached: r.map(|r| r.reached as f64 / r.n as f64),
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LookAhead {
    /// options (best by this round's score) played to the end of the run
    pub top: usize,
    /// futures played per option
    pub playouts: u32,
    #[serde(default)]
    pub steer: Option<f64>,
    /// outcomes averaged per option in each round inside the futures (default 2)
    #[serde(default)]
    pub inner_samples: Option<u32>,
    #[serde(default)]
    pub value: Option<bool>,
    /// successive halving: `playouts` futures per option on average, spread over rounds that each drop the worse
    /// half of the options left (`halved_futures`)
    #[serde(default)]
    pub halving: Option<bool>,
}

/// One option played to the end: this round's expected score, the mean score counted up to TARGET, the share reaching it.
#[derive(Clone, Copy, Debug)]
pub struct Ahead {
    pub o: Opt,
    pub now: f64,
    pub goal: f64,
    pub reached: f64,
}

/// The options the look-ahead plays to the end, with this round's expected score: each card on its best target
/// this round, the next best options up to `top`, and a reroll while rerolls are left.
pub fn shortlist(s: &GameState, seed: u32, la: &LookAhead) -> Vec<(Opt, f64)> {
    let quick = Policy::new(8, 0);
    let mut now: Vec<(Opt, f64)> = options(s).iter().map(|o| (*o, expected(s, o, &quick, seed, 0))).collect();
    now.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut list: Vec<usize> = Vec::new();
    for (i, c) in now.iter().enumerate() {
        if now.iter().position(|d| d.0.index() == c.0.index()) == Some(i) {
            list.push(i);
        }
    }
    for i in 0..now.len() {
        if list.len() < la.top && !list.contains(&i) {
            list.push(i);
        }
    }
    let mut out: Vec<(Opt, f64)> = list.into_iter().map(|i| now[i]).collect();
    if s.open_box.is_none() && can_refresh(s) {
        out.push((Opt::Refresh, ultimate_activity(s) as f64));
    }
    out
}

/// Futures `from` to `to` (excluded) of each potion option, played to the end of the run.
pub fn potion_futures(s: &GameState, seed: u32, la: &LookAhead, opts: &[Opt], from: u32, to: u32, par: bool) -> Vec<Tally> {
    let inner = Chooser::Ev(Policy { samples: la.inner_samples.unwrap_or(2), horizon: 0, roll_samples: None, steer: la.steer, value: la.value });
    tallies(opts.len(), from, to, par, |c, k| {
        let sk = mix(seed, 9000 + k);
        let ua = ultimate_activity(&finish(&play_opt(&reseed(s, sk), &opts[c]), sk, &inner, &Picker::Value));
        Tally { goal: goal_score(ua), reached: (ua >= TARGET) as u32, n: 1 }
    })
}

/// Successive halving (Karnin, Koren & Somekh, 2013): `la.playouts` futures per option on average, spread over
/// ⌈log₂ n⌉ rounds; after each, the worse half of the options left is dropped, so the close calls between the best
/// get the most futures for the same total. An option dropped keeps the tally of its fewer futures. Futures are
/// numbered as in `potion_futures`, so the options left always play the same ones (common random numbers).
/// Tested at 1,024 futures on average against every option getting 1,024 (Oct 24, 2026): not better, 80.1% of 1,440
/// games reached 840k against 81.6%. With about five options, all good ones from the shortlist, little goes to the
/// poor ones anyway, and an option dropped early can't come back; so the overlay plays every option alike.
pub fn halved_futures(s: &GameState, seed: u32, la: &LookAhead, opts: &[Opt], par: bool) -> Vec<Tally> {
    let n = opts.len();
    let mut tallies = vec![Tally::default(); n];
    if n == 0 {
        return tallies;
    }
    let rounds = ((usize::BITS - (n - 1).leading_zeros()) as u64).max(1);
    let total = la.playouts as u64 * n as u64;
    let mean = |t: &Tally| t.goal / t.n.max(1) as f64;
    let mut alive: Vec<usize> = (0..n).collect();
    for round in 0..rounds {
        let each = (total / (rounds * alive.len() as u64)).max(1) as u32;
        let from = tallies[alive[0]].n;
        let these: Vec<Opt> = alive.iter().map(|&i| opts[i]).collect();
        for (&i, t) in alive.iter().zip(potion_futures(s, seed, la, &these, from, from + each, par)) {
            tallies[i].add(&t);
        }
        if round + 1 < rounds {
            alive.sort_by(|&a, &b| mean(&tallies[b]).partial_cmp(&mean(&tallies[a])).unwrap_or(std::cmp::Ordering::Equal));
            alive.truncate(alive.len().div_ceil(2));
        }
    }
    tallies
}

pub fn look_ahead(s: &GameState, seed: u32, la: &LookAhead, par: bool) -> Vec<Ahead> {
    struct Run {
        o: Opt,
        v: f64,
        t: Tally,
    }
    let mut runs: Vec<Run> = shortlist(s, seed, la).into_iter().map(|(o, v)| Run { o, v, t: Tally::default() }).collect();
    let mean = |r: &Run| r.t.goal / r.t.n as f64;
    let play_more = |r: &mut Run, n: u32| {
        let t = potion_futures(s, seed, la, &[r.o], r.t.n, r.t.n + n, par)[0];
        r.t.add(&t);
    };
    if la.halving.unwrap_or(false) {
        let opts: Vec<Opt> = runs.iter().map(|r| r.o).collect();
        for (r, t) in runs.iter_mut().zip(halved_futures(s, seed, la, &opts, par)) {
            r.t = t;
        }
    } else {
        for r in runs.iter_mut() {
            play_more(r, la.playouts);
        }
    }
    // options dropped early keep their estimate from fewer futures, below the ones kept
    let mut out: Vec<(u32, Ahead)> = runs.iter().map(|r| (r.t.n, Ahead { o: r.o, now: r.v, goal: mean(r), reached: r.t.reached as f64 / r.t.n as f64 })).collect();
    out.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.goal.partial_cmp(&a.1.goal).unwrap_or(std::cmp::Ordering::Equal)));
    out.into_iter().map(|(_, a)| a).collect()
}

/// One game: `setup` given = sandbox with fixed instruments from round 1; none = everything dealt.
pub fn run_game(seed: u32, chooser: &Chooser, picker: &Picker, setup: Option<&SandboxSetup>) -> GameState {
    finish(&create_game(data().default_settings, setup, Some(seed)), seed, chooser, picker)
}
