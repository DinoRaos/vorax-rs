//! What the reader remembers through a game (src/solver/track.ts). Instrument icons are small and some look
//! alike, but instruments only change when one is picked, and a new one must be one of the three just offered,
//! whose cards are read reliably.

use crate::reading::{FieldRead, Fields, Reading};
use serde::{Deserialize, Serialize};
use vorax::advisor::ScreenPhase;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Unsure {
    pub offer: Vec<String>,
    pub slot: usize,
    pub looks: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PickQuestion {
    pub offer: Vec<String>,
    pub slot: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InstrumentTracker {
    /// instruments known so far this game, in pick order
    pub owned: Vec<String>,
    /// the last instrument offer's cards, until one of them is picked
    pub offer: Option<Vec<String>>,
    /// picks it wasn't sure of, until a later board shows them clearly
    pub unsure: Vec<Unsure>,
    /// icons in the panel before the first pick (a starting item)
    pub start: usize,
    /// the round each owned instrument was picked in (0: not known)
    pub rounds: Vec<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tracked {
    pub tracker: InstrumentTracker,
    pub instruments: Vec<String>,
    pub rounds: Vec<i32>,
    pub fields: Fields,
    pub settled: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<PickQuestion>,
}

const ASK_AFTER: u32 = 1;
const SURE: f64 = 0.7;
const GAP: f64 = 0.1;

fn score_in(ranks: Option<&Vec<(String, f64)>>, n: &str) -> f64 {
    ranks.and_then(|r| r.iter().find(|(k, _)| k == n)).map_or(0.0, |x| x.1)
}

fn sorted(mut v: Vec<(String, f64)>) -> Vec<(String, f64)> {
    v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    v
}

pub fn track_instruments(t: &InstrumentTracker, r: &Reading, trigger: Option<&str>) -> Tracked {
    let s = &r.state;
    let is_instr = s.phase == ScreenPhase::Instrument;
    let start = trigger == Some("screen") || (is_instr && s.round == Some(1) && s.instruments.is_empty());
    let mut owned: Vec<String> = if start { vec![] } else { t.owned.clone() };
    let mut offer: Option<Vec<String>> = if start { None } else { t.offer.clone() };
    let mut unsure: Vec<Unsure> = if start { vec![] } else { t.unsure.clone() };
    let mut rounds: Vec<i32> = if start { vec![] } else { t.rounds.clone() };
    let known = |unsure: &[Unsure], n: &str, i: usize| FieldRead { sure: !unsure.iter().any(|q| q.slot == i), guess: Some(n.to_string()), ..Default::default() };
    let next_offer = (is_instr && !s.hand.is_empty()).then(|| s.hand.clone());
    if offer.is_none() && start {
        offer = next_offer.clone();
    }
    let before_first_pick = is_instr && s.round == Some(1) && owned.is_empty();
    let mut start_items = if start { 0 } else { t.start };
    if before_first_pick {
        start_items = s.instruments.len();
    } else if owned.is_empty() && start_items > 0 && offer.is_some() && s.instruments.len() == start_items {
        start_items = 0;
    }
    let all_ranks = r.instrument_ranks.clone().unwrap_or_default();
    let mut panel: Vec<String> = s.instruments.iter().skip(start_items).cloned().collect();
    let mut ranks: Vec<Vec<(String, f64)>> = all_ranks.into_iter().skip(start_items).collect();
    if panel.len() > 3 {
        let extra = panel.len() - 3;
        start_items += extra;
        panel.drain(..extra);
        ranks.drain(..extra.min(ranks.len()));
    }
    let count = if before_first_pick { 0 } else { panel.len() };
    let mut fields = Fields::new();
    let mut ask = None;
    let mut settled = Vec::new();
    if count >= owned.len() {
        for q in &unsure {
            let scored = sorted(q.offer.iter().map(|n| (n.clone(), score_in(ranks.get(q.slot), n))).collect());
            if scored[0].1 >= SURE && scored[0].1 - scored.get(1).map_or(0.0, |x| x.1) >= GAP {
                if q.slot < owned.len() {
                    owned[q.slot] = scored[0].0.clone();
                }
                settled.push(q.slot);
            }
        }
        unsure = unsure.into_iter().filter(|q| !settled.contains(&q.slot)).map(|q| Unsure { looks: q.looks + 1, ..q }).collect();
        if let Some(due) = unsure.iter().find(|q| q.looks == ASK_AFTER) {
            ask = Some(PickQuestion { offer: due.offer.clone(), slot: due.slot });
        }
    }
    if count == owned.len() {
        for (i, n) in owned.iter().enumerate() {
            fields.insert(format!("instrument{i}"), known(&unsure, n, i));
        }
    } else if count == owned.len() + 1 && offer.is_some() {
        let off = offer.clone().unwrap();
        let slot_ranks = ranks.get(count - 1);
        let pick = sorted(off.iter().map(|n| (n.clone(), score_in(slot_ranks, n))).collect());
        let (mut name, mut score) = pick[0].clone();
        let unpictured: Vec<&String> = off.iter().filter(|n| !slot_ranks.is_some_and(|r| r.iter().any(|(k, _)| k == *n))).collect();
        if score < SURE && !unpictured.is_empty() {
            name = unpictured[0].clone();
            score = 0.0;
        }
        owned.push(name.clone());
        rounds.truncate(count - 1);
        rounds.push(if count == 1 {
            1
        } else {
            match s.round {
                None => 0,
                Some(r) if is_instr => r + 1,
                Some(r) => r,
            }
        });
        for (i, n) in owned.iter().enumerate() {
            fields.insert(format!("instrument{i}"), known(&unsure, n, i));
        }
        let second = pick.get(1).map(|x| x.1);
        let sure = score >= SURE && score - second.unwrap_or(0.0) >= GAP;
        fields.insert(format!("instrument{}", count - 1), FieldRead { sure, guess: Some(name), score: Some(score), second });
        if !sure {
            unsure.push(Unsure { offer: off, slot: count - 1, looks: 0 });
        }
        offer = None;
    } else {
        // several picks between two boards (in round 1 two strong cards can reach the first reward at once): the
        // ones known are still where they were, so they keep their rounds, and the new ones date from now, as a
        // single pick would. Otherwise (the panel and what's known disagree) the rounds aren't known
        let grew = count > owned.len() && !before_first_pick && panel.starts_with(&owned);
        let now = match s.round {
            None => 0,
            Some(r) if is_instr => r + 1,
            Some(r) => r,
        };
        rounds = if grew { (0..count).map(|i| rounds.get(i).copied().unwrap_or(if i == 0 { 1 } else { now })).collect() } else { vec![0; panel.len()] };
        owned = panel.clone();
        unsure.retain(|q| q.slot < owned.len());
        for (i, n) in owned.iter().enumerate() {
            let f = r.fields.get(&format!("instrument{}", i + start_items)).cloned().unwrap_or(FieldRead { sure: false, guess: Some(n.clone()), ..Default::default() });
            fields.insert(format!("instrument{i}"), f);
        }
    }
    let rounds: Vec<i32> = (0..owned.len()).map(|i| rounds.get(i).copied().unwrap_or(0)).collect();
    Tracked {
        tracker: InstrumentTracker { owned: owned.clone(), offer: next_offer.or(offer), unsure, start: start_items, rounds: rounds.clone() },
        instruments: owned,
        rounds,
        fields,
        settled,
        ask,
    }
}
