//! The board value (src/strategy/value.ts): the final score (counted up to the advice's target) games from a board tend to
//! reach, learned by upstream from simulated games; and the build progress the steered player aims for
//! (src/strategy/steer.ts). Sums are taken in the TypeScript's order so the values match bit for bit.

use crate::data::{data, Data, Timing, ValueModel};
use crate::game::ultimate_activity;
use crate::types::*;

/// The advice's target (bot.ts TARGET): the board value is counted up to it.
pub const TARGET: i64 = 840_000;
/// Where the features cap the score: 950,000, the target the shipped model was trained with, kept so it reads
/// boards as it learned them.
const FEATURE_CAP: i64 = 950_000;

/// Predicted final score counted up to 840,000; once the game is over, the score itself (up to 840,000).
pub fn board_value(s: &GameState) -> f64 {
    let d = data();
    let Some(model) = &d.model else { return ultimate_activity(s) as f64 };
    if s.phase == Phase::GameOver {
        return ultimate_activity(s).min(TARGET) as f64;
    }
    let v = match model {
        ValueModel::Rounds { rounds } => {
            let at = (s.round.max(1) as usize).min(rounds.len()) - 1;
            weighted(d, s, &rounds[at])
        }
        ValueModel::Trees { .. } => {
            let forest = d.forest.as_ref().expect("a tree model's forest");
            let mut buf = [0.0; TREE_MAX];
            let n = tree_features_into(d, s, &mut buf);
            let x = &buf[..n];
            let mut v = forest.base;
            for &root in &forest.roots {
                let mut i = root as usize;
                loop {
                    let node = forest.nodes[i];
                    if node.f < 0 {
                        v += node.t;
                        break;
                    }
                    i = if x[node.f as usize] < node.t { node.l } else { node.r } as usize;
                }
            }
            v
        }
    };
    (v * 1e6).min(TARGET as f64).max(0.0)
}

const PAIRED: usize = 11;
const HEAD: usize = 31;
const BLOCK: usize = 2 + (PAIRED + 1) - 1;

/// The features that don't depend on which instruments are owned, what each owned one is paired with, the log score.
fn parts(s: &GameState) -> ([f64; HEAD], [f64; PAIRED], f64) {
    let ua = ultimate_activity(s);
    let uaf = ua as f64;
    let mut n = 0usize;
    let (mut arch, mut rar) = ([0usize; 4], [0usize; 4]);
    let (mut big, mut qty275, mut act255) = (0usize, 0usize, 0usize);
    let (mut sum_act, mut sum_qty) = (0i64, 0i64);
    let (mut top_act, mut top_qty, mut top_group) = (0i64, 0i64, 0i64);
    for m in s.monsters() {
        n += 1;
        arch[m.archetype as usize] += 1;
        rar[m.rarity as usize] += 1;
        big += (m.activity > 150 && m.quantity > 150) as usize;
        qty275 += (m.quantity > 275) as usize;
        act255 += (m.activity > 255) as usize;
        sum_act += m.activity;
        sum_qty += m.quantity;
        top_act = top_act.max(m.activity);
        top_qty = top_qty.max(m.quantity);
        top_group = top_group.max(m.activity * m.quantity);
    }
    let share = |k: usize| k as f64 / 6.0;
    let log_score = libm::log1p(uaf) / 14.0;
    let paired = [
        share(arch[0]), share(arch[1]), share(arch[2]), share(arch[3]),
        share(rar[0]), share(rar[1]), share(rar[2]), share(rar[3]),
        share(big), share(qty275), share(act255),
    ];
    let b = |x: bool| if x { 1.0 } else { 0.0 };
    let mut head = [0.0; HEAD];
    let first = [
        1.0,
        log_score,
        uaf / 1e6,
        ua.min(FEATURE_CAP) as f64 / 1e6,
        uaf.sqrt() / 1000.0,
        b(ua >= FEATURE_CAP),
        sum_act as f64 / 1000.0,
        sum_qty as f64 / 3000.0,
        top_act as f64 / 1000.0,
        top_qty as f64 / 3000.0,
        top_group as f64 / 3e5,
        n as f64 / 6.0,
    ];
    head[..12].copy_from_slice(&first);
    head[12..23].copy_from_slice(&paired);
    let last = [
        s.potion_refreshes as f64 / 3.0,
        s.instrument_refreshes as f64 / 2.0,
        s.instruments.len() as f64 / 3.0,
        s.pending_picks as f64,
        b(s.t8000),
        b(s.t28000),
        b(s.phase == Phase::InstrumentSelect),
        b(s.phase == Phase::RoundEnd),
    ];
    head[23..].copy_from_slice(&last);
    (head, paired, log_score)
}

/// The weighted sum of a board's features, reading only those that aren't 0 (most instruments aren't owned).
fn weighted(d: &Data, s: &GameState, w: &[f64]) -> f64 {
    let (head, paired, log_score) = parts(s);
    let at = |i: usize| w.get(i).copied().unwrap_or(f64::NAN);
    let mut v = 0.0;
    for i in 0..HEAD.min(w.len()) {
        v += w[i] * head[i];
    }
    let count = d.instruments.len();
    let mut owned: crate::stack::StackVec<u16, MAX_EQUIPPED> = crate::stack::StackVec::new();
    for e in &s.instruments {
        let i = d.instrument_index[e.id as usize];
        if i != u16::MAX && !owned.contains(&i) {
            owned.push(i);
        }
    }
    owned.as_mut_slice().sort();
    for &i in &owned {
        let o = HEAD + i as usize * BLOCK;
        if o >= w.len() {
            continue;
        }
        v += w[o];
        for (k, p) in paired.iter().enumerate() {
            v += at(o + 1 + k) * p;
        }
        v += at(o + 1 + PAIRED) * log_score;
    }
    for a in 0..owned.len() {
        for b in a + 1..owned.len() {
            let (i, j) = (owned[a] as usize, owned[b] as usize);
            let pair = HEAD + count * BLOCK + i * count - (i * (i + 1)) / 2 + (j - i - 1);
            if pair < w.len() {
                v += w[pair];
            }
        }
    }
    v
}

/// How many features a board has for the linear board value: the head, a block per instrument, a pair for every two.
pub fn feature_count(d: &Data) -> usize {
    let n = d.instruments.len();
    HEAD + n * BLOCK + n * (n - 1) / 2
}

/// A board's features for the linear board value, as (index, value) for those that aren't 0, by index: the numbers
/// `weighted` sums against a round's weights, for fitting them.
pub fn features(d: &Data, s: &GameState, out: &mut Vec<(u32, f64)>) {
    out.clear();
    let (head, paired, log_score) = parts(s);
    for (i, &v) in head.iter().enumerate() {
        if v != 0.0 {
            out.push((i as u32, v));
        }
    }
    let count = d.instruments.len();
    let mut owned: crate::stack::StackVec<u16, MAX_EQUIPPED> = crate::stack::StackVec::new();
    for e in &s.instruments {
        let i = d.instrument_index[e.id as usize];
        if i != u16::MAX && !owned.contains(&i) {
            owned.push(i);
        }
    }
    owned.as_mut_slice().sort();
    for &i in &owned {
        let o = HEAD + i as usize * BLOCK;
        out.push((o as u32, 1.0));
        for (k, &p) in paired.iter().enumerate() {
            if p != 0.0 {
                out.push(((o + 1 + k) as u32, p));
            }
        }
        if log_score != 0.0 {
            out.push(((o + 1 + PAIRED) as u32, log_score));
        }
    }
    for a in 0..owned.len() {
        for b in a + 1..owned.len() {
            let (i, j) = (owned[a] as usize, owned[b] as usize);
            out.push(((HEAD + count * BLOCK + i * count - (i * (i + 1)) / 2 + (j - i - 1)) as u32, 1.0));
        }
    }
}

/// The most raw numbers a board has for decision trees.
const TREE_MAX: usize = 160;

/// A board in raw numbers for decision trees (TREE_FEATURES order).
pub fn tree_features(d: &Data, s: &GameState) -> Vec<f64> {
    let mut buf = [0.0; TREE_MAX];
    let n = tree_features_into(d, s, &mut buf);
    buf[..n].to_vec()
}

/// `tree_features` into a buffer, without allocating: how many it wrote. The board value calls it for every board
/// the futures rate, so the monsters are counted in one pass.
fn tree_features_into(d: &Data, s: &GameState, x: &mut [f64; TREE_MAX]) -> usize {
    let ua = ultimate_activity(s);
    let (mut arch, mut rar, mut both) = ([0usize; 4], [0usize; 4], [[0usize; 4]; 4]);
    let (mut big, mut qty275, mut act255, mut n) = (0usize, 0usize, 0usize, 0usize);
    let (mut sum_act, mut sum_qty, mut top_act, mut top_qty, mut top_group) = (0i64, 0i64, 0i64, 0i64, 0i64);
    for m in s.monsters() {
        n += 1;
        arch[m.archetype as usize] += 1;
        rar[m.rarity as usize] += 1;
        both[m.rarity as usize][m.archetype as usize] += 1;
        big += (m.activity > 150 && m.quantity > 150) as usize;
        qty275 += (m.quantity > 275) as usize;
        act255 += (m.activity > 255) as usize;
        sum_act += m.activity;
        sum_qty += m.quantity;
        top_act = top_act.max(m.activity);
        top_qty = top_qty.max(m.quantity);
        top_group = top_group.max(m.total());
    }
    let b = |v: bool| if v { 1.0 } else { 0.0 };
    let head = [
        s.round as f64,
        b(s.phase == Phase::InstrumentSelect),
        b(s.phase == Phase::RoundEnd),
        s.potion_refreshes as f64,
        s.instrument_refreshes as f64,
        s.instruments.len() as f64,
        s.pending_picks as f64,
        b(s.t8000),
        b(s.t28000),
        libm::log1p(ua as f64),
        ua.min(FEATURE_CAP) as f64,
        sum_act as f64,
        sum_qty as f64,
        top_act as f64,
        top_qty as f64,
        top_group as f64,
        n as f64,
    ];
    let mut k = 0;
    let mut push = |v: f64| {
        x[k] = v;
        k += 1;
    };
    head.into_iter().for_each(&mut push);
    arch.iter().chain(&rar).chain(both.iter().flatten()).chain(&[big, qty275, act255]).for_each(|&c| push(c as f64));
    // a Map from the instruments list: the last of a duplicated id wins
    let acquired = |id: u8| s.instruments.iter().rev().find(|e| e.id == id).map(|e| e.acquired_round);
    for def in &d.instruments {
        push(b(acquired(def.id).is_some()));
    }
    for &id in &d.periodic {
        let every = match d.instrument(id).unwrap().timing {
            Timing::Periodic(e) => e,
            _ => 0,
        };
        push(match acquired(id) {
            None => -1.0,
            Some(at) => {
                let mut k = 0;
                loop {
                    let e = s.round + k - at;
                    if e > 0 && every > 0 && e % every == 0 {
                        break k as f64;
                    }
                    k += 1;
                }
            }
        });
    }
    k
}

/// How far the owned instruments' conditions are met, 0 to 1 averaged; 1 when none needs anything.
pub fn build_progress(s: &GameState) -> f64 {
    let d = data();
    let ms: crate::stack::StackVec<Monster, TANK_COUNT> = {
        let mut v = crate::stack::StackVec::new();
        for m in s.monsters() {
            v.push(*m);
        }
        v
    };
    let count = |ok: &dyn Fn(&Monster) -> bool| ms.iter().filter(|m| ok(m)).count() as f64;
    let at_least = |n: f64, ok: &dyn Fn(&Monster) -> bool| (count(ok) / n).min(1.0);
    let rare = |r: Rarity| r.is_rare_or_boss();
    let mut sum = 0.0;
    let mut needs = 0usize;
    for e in &s.instruments {
        let Some(def) = d.instrument(e.id) else { continue };
        let p = match def.name.as_str() {
            "Chrysalis Specimen" => at_least(6.0, &|m| m.archetype == Archetype::Pest),
            "Incubation Sac" | "Egg-Burdened Butterfly" => at_least(2.0, &|m| m.archetype == Archetype::Pest),
            "Clustered Eggs" => at_least(3.0, &|m| m.archetype == Archetype::Pest),
            "Hyperplastic Frontal Lobe" => at_least(2.0, &|m| m.archetype == Archetype::Agito),
            "Hypertrophic Pituitary" => {
                if ms.iter().any(|m| m.archetype == Archetype::Agito && rare(m.rarity)) {
                    1.0
                } else if ms.iter().any(|m| m.archetype == Archetype::Agito) {
                    0.5
                } else {
                    0.0
                }
            }
            "Adherent Metatarsals" | "Sinew Garrote" => at_least(2.0, &|m| m.archetype == Archetype::BoneSentry),
            "Prismatic Liver" => at_least(2.0, &|m| m.archetype == Archetype::Aemberon),
            "Rawhide Restraint" => at_least(3.0, &|m| m.rarity == Rarity::Magic),
            "Nettle Toggle" => at_least(3.0, &|m| m.rarity == Rarity::Normal),
            "Tanned Leather Restraint" => {
                let has = [
                    ms.iter().any(|m| m.rarity == Rarity::Normal),
                    ms.iter().any(|m| m.rarity == Rarity::Magic),
                    ms.iter().any(|m| rare(m.rarity)),
                ];
                has.iter().filter(|&&x| x).count() as f64 / 3.0
            }
            _ => continue,
        };
        sum += p;
        needs += 1;
    }
    if needs == 0 {
        1.0
    } else {
        sum / needs as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::{options, play_opt, pick_and_confirm};
    use crate::game::{create_game, dispatch};

    /// The features summed against a round's weights give what `weighted` gives, on boards from whole games.
    #[test]
    fn features_sum_to_the_board_value() {
        let _ = crate::data::use_model(Some(include_str!("../../../fixtures/boardValue-linear.json")));
        let d = data();
        let Some(ValueModel::Rounds { rounds }) = &d.model else { panic!("no linear model") };
        assert_eq!(rounds[0].len(), feature_count(d));
        let mut x = Vec::new();
        let mut boards = 0;
        for seed in 1..40u32 {
            let mut s = create_game(d.default_settings, None, Some(seed));
            for g in 0..300usize {
                if s.phase == Phase::GameOver {
                    break;
                }
                let w = &rounds[(s.round.max(1) as usize).min(rounds.len()) - 1];
                features(d, &s, &mut x);
                let sum: f64 = x.iter().map(|&(i, v)| w[i as usize] * v).sum();
                let want = weighted(d, &s, w);
                assert!((sum - want).abs() <= 1e-9 * want.abs().max(1.0), "seed {seed} step {g}: {sum} vs {want}");
                boards += 1;
                s = match s.phase {
                    Phase::RoundEnd => dispatch(&s, Action::Confirm),
                    Phase::InstrumentSelect => pick_and_confirm(&s, g % s.hand.len()),
                    _ => {
                        let o = options(&s);
                        play_opt(&s, &o[g % o.len()])
                    }
                };
            }
        }
        assert!(boards > 500);
    }
}
