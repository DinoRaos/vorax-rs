//! The board value (src/strategy/value.ts): the final score (counted up to the advice's target) games from a board tend to
//! reach, learned by upstream from simulated games; and the build progress the steered player aims for
//! (src/strategy/steer.ts). Sums are taken in the TypeScript's order so the values match bit for bit.

use crate::data::{data, Data, Timing, ValueModel};
use crate::game::ultimate_activity;
use crate::types::*;

/// The advice's target (bot.ts TARGET): the board value is counted up to it.
pub const TARGET: i64 = 900_000;
/// Where the features cap the score: 950,000, the target the shipped model was trained with, kept so it reads
/// boards as it learned them.
const FEATURE_CAP: i64 = 950_000;

/// Predicted final score counted up to 900,000; once the game is over, the score itself (up to 900,000).
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
        ValueModel::Trees { base, trees } => {
            let x = tree_features(d, s);
            let mut v = *base;
            for t in trees {
                let mut n = 0usize;
                while t.f[n] >= 0 {
                    n = if x[t.f[n] as usize] < t.t[n] { t.l[n] } else { t.r[n] };
                }
                v += t.v[n];
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

/// A board in raw numbers for decision trees (TREE_FEATURES order).
pub fn tree_features(d: &Data, s: &GameState) -> Vec<f64> {
    let ua = ultimate_activity(s);
    let ms: Vec<&Monster> = s.monsters().collect();
    let count = |ok: &dyn Fn(&Monster) -> bool| ms.iter().filter(|m| ok(m)).count() as f64;
    let b = |x: bool| if x { 1.0 } else { 0.0 };
    let mut x = vec![
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
        ms.iter().map(|m| m.activity).sum::<i64>() as f64,
        ms.iter().map(|m| m.quantity).sum::<i64>() as f64,
        ms.iter().map(|m| m.activity).max().unwrap_or(0).max(0) as f64,
        ms.iter().map(|m| m.quantity).max().unwrap_or(0).max(0) as f64,
        ms.iter().map(|m| m.total()).max().unwrap_or(0).max(0) as f64,
        ms.len() as f64,
    ];
    for a in ARCHETYPES {
        x.push(count(&|m| m.archetype == a));
    }
    for r in RARITIES {
        x.push(count(&|m| m.rarity == r));
    }
    for r in RARITIES {
        for a in ARCHETYPES {
            x.push(count(&|m| m.rarity == r && m.archetype == a));
        }
    }
    x.push(count(&|m| m.activity > 150 && m.quantity > 150));
    x.push(count(&|m| m.quantity > 275));
    x.push(count(&|m| m.activity > 255));
    // a Map from the instruments list: the last of a duplicated id wins
    let acquired = |id: u8| s.instruments.iter().rev().find(|e| e.id == id).map(|e| e.acquired_round);
    for def in &d.instruments {
        x.push(b(acquired(def.id).is_some()));
    }
    for &id in &d.periodic {
        let every = match d.instrument(id).unwrap().timing {
            Timing::Periodic(e) => e,
            _ => 0,
        };
        x.push(match acquired(id) {
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
    x
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
