//! What card and instrument effects act through (src/engine/context.ts): every board change, and the reactive
//! instruments it sets off, chained up to MAX_CHAIN_DEPTH deep. Game logs are left out: nothing reads them.

use crate::data::Data;
use crate::effects;
use crate::rng::Rng;
use crate::stack::StackVec;
use crate::types::*;

pub const MAX_CHAIN_DEPTH: u32 = 8;

/// Activity gained when awakening *into* a rarity.
pub const AWAKEN_BONUS: [i64; 4] = [0, 60, 120, 120];

pub fn base_stats(r: Rarity) -> (i64, i64) {
    match r {
        Rarity::Normal => (1, 36),
        Rarity::Magic => (5, 24),
        Rarity::Rare => (15, 12),
        Rarity::Boss => (300, 1),
    }
}

pub fn create_monster(a: Archetype, r: Rarity, act_bonus: i64, qty_bonus: i64) -> Monster {
    let (act, qty) = base_stats(r);
    Monster { archetype: a, rarity: r, activity: act + act_bonus, quantity: qty + qty_bonus }
}

pub fn roll_rarity(d: &Data, rng: &mut Rng) -> Rarity {
    let roll = rng.next() * d.roll_weights_total();
    let mut acc = 0.0;
    for r in RARITIES {
        acc += d.spawn_weight(r);
        if roll < acc {
            return r;
        }
    }
    Rarity::Boss
}

pub fn roll_archetype(rng: &mut Rng) -> Archetype {
    ARCHETYPES[rng.int(4)]
}

pub struct Ctx<'a> {
    pub tanks: &'a mut Tanks,
    pub rng: &'a mut Rng,
    pub d: &'static Data,
    /// reactive instruments owned, by id (duplicates kept)
    reactive: StackVec<u8, MAX_EQUIPPED>,
    depth: u32,
}

impl<'a> Ctx<'a> {
    pub fn new(tanks: &'a mut Tanks, rng: &'a mut Rng, instruments: &Equips, d: &'static Data) -> Self {
        let mut ids: StackVec<u8, MAX_EQUIPPED> = StackVec::new();
        for e in instruments {
            if let Some(def) = d.instrument(e.id) {
                if matches!(def.timing, crate::data::Timing::Reactive(_)) {
                    ids.push(e.id);
                }
            }
        }
        ids.as_mut_slice().sort();
        Ctx { tanks, rng, d, reactive: ids, depth: 0 }
    }

    fn emit(&mut self, e: GameEvent) {
        let event_depth = self.depth;
        if event_depth >= MAX_CHAIN_DEPTH {
            return;
        }
        let kind = e.kind();
        let reactive = self.reactive;
        for &id in &reactive {
            let def = self.d.instrument(id).unwrap();
            if !def.timing.reacts_to(kind) {
                continue;
            }
            self.depth = event_depth + 1;
            effects::on_event(self, id, e);
        }
        self.depth = event_depth;
    }

    #[inline]
    pub fn mon(&self, tank: usize) -> Option<Monster> {
        self.tanks.get(tank).copied().flatten()
    }

    /// Returns the tank index, or -1 when every tank is full (emits addOverflow).
    pub fn add(&mut self, a: Archetype, r: Rarity, act_bonus: i64, qty_bonus: i64) -> i32 {
        let Some(i) = self.tanks.iter().position(|t| t.is_none()) else {
            self.emit(GameEvent::AddOverflow);
            return -1;
        };
        self.tanks[i] = Some(create_monster(a, r, act_bonus, qty_bonus));
        self.emit(GameEvent::Add { tank: i });
        i as i32
    }

    pub fn remove(&mut self, tank: usize) -> Option<Monster> {
        let monster = self.mon(tank)?;
        self.tanks[tank] = None;
        self.emit(GameEvent::Remove { monster });
        Some(monster)
    }

    /// Keeps Activity and Quantity; None rolls archetype (first) / rarity.
    pub fn mutate(&mut self, tank: usize, a: Option<Archetype>, r: Option<Rarity>) {
        let Some(mut m) = self.mon(tank) else { return };
        m.archetype = match a {
            Some(a) => a,
            None => roll_archetype(self.rng),
        };
        m.rarity = match r {
            Some(r) => r,
            None => roll_rarity(self.d, self.rng),
        };
        self.tanks[tank] = Some(m);
        self.emit(GameEvent::Mutate { to: m.archetype });
    }

    /// Defaults: to = next rarity, bonus = AWAKEN_BONUS[to]. Never lowers rarity; a Boss stays Boss and gains nothing.
    pub fn awaken(&mut self, tank: usize, to: Option<Rarity>, bonus: Option<i64>) {
        let Some(mut m) = self.mon(tank) else { return };
        let from = m.rarity;
        let named = to.unwrap_or(from.next());
        let target = if named < from { from } else { named };
        let gain = if from == Rarity::Boss { 0 } else { bonus.unwrap_or(AWAKEN_BONUS[target.index()]) };
        m.rarity = target;
        m.activity += gain;
        self.tanks[tank] = Some(m);
        self.emit(GameEvent::Awaken);
    }

    pub fn fuse(&mut self, sources: &[u8], a: Archetype, r: Rarity) -> i32 {
        let mut activity = 0;
        let mut quantity = 0;
        let mut used: StackVec<u8, 8> = StackVec::new();
        for &s in sources {
            let Some(m) = self.mon(s as usize) else { continue };
            activity += m.activity;
            quantity += m.quantity;
            used.push(s);
        }
        if used.is_empty() {
            return -1;
        }
        for &s in &used {
            self.remove(s as usize);
        }
        let i = self.tanks.iter().position(|t| t.is_none()).expect("a fused source left a tank empty");
        self.tanks[i] = Some(Monster { archetype: a, rarity: r, activity, quantity });
        self.emit(GameEvent::Add { tank: i });
        self.emit(GameEvent::Fuse);
        i as i32
    }

    pub fn devour(&mut self, devourer: usize, prey: usize) {
        let (Some(mut d), Some(p)) = (self.mon(devourer), self.mon(prey)) else { return };
        if devourer == prey {
            return;
        }
        d.activity += p.activity;
        d.quantity += p.quantity;
        self.tanks[devourer] = Some(d);
        self.remove(prey);
        self.emit(GameEvent::Devour);
    }

    /// Plain stat change; emits no event. A tank of -1 or an empty one is left alone.
    #[inline]
    pub fn boost(&mut self, tank: i32, act: i64, qty: i64) {
        if tank < 0 {
            return;
        }
        if let Some(Some(m)) = self.tanks.get_mut(tank as usize) {
            m.activity += act;
            m.quantity += qty;
        }
    }

    #[inline]
    pub fn non_empty(&self) -> TankList {
        let mut out = TankList::new();
        for (i, t) in self.tanks.iter().enumerate() {
            if t.is_some() {
                out.push(i as u8);
            }
        }
        out
    }

    pub fn filter(&self, ok: impl Fn(usize, &Monster) -> bool) -> TankList {
        let mut out = TankList::new();
        for (i, t) in self.tanks.iter().enumerate() {
            if let Some(m) = t {
                if ok(i, m) {
                    out.push(i as u8);
                }
            }
        }
        out
    }

    pub fn by_archetype(&self, a: Archetype) -> TankList {
        self.filter(|_, m| m.archetype == a)
    }

    pub fn by_rarity(&self, r: Rarity) -> TankList {
        self.filter(|_, m| m.rarity == r)
    }

    /// By Total Activity; ties → leftmost; -1 when empty.
    pub fn highest(&self) -> i32 {
        self.pick_by(|a, b| a > b)
    }

    pub fn lowest(&self) -> i32 {
        self.pick_by(|a, b| a < b)
    }

    fn pick_by(&self, better: impl Fn(i64, i64) -> bool) -> i32 {
        let idx = self.non_empty();
        if idx.is_empty() {
            return -1;
        }
        let mut best = idx[0] as usize;
        for &i in &idx {
            if better(total(&self.tanks[i as usize]), total(&self.tanks[best])) {
                best = i as usize;
            }
        }
        best as i32
    }

    /// Nearest non-empty tank in a direction, or -1.
    pub fn neighbor(&self, tank: usize, dir: i32) -> i32 {
        let mut i = tank as i32 + dir;
        while i >= 0 && (i as usize) < TANK_COUNT {
            if self.tanks[i as usize].is_some() {
                return i;
            }
            i += dir;
        }
        -1
    }

    /// `rng.pick` over a tank list.
    #[inline]
    pub fn pick(&mut self, l: &TankList) -> Option<usize> {
        self.rng.pick(l.as_slice()).map(|t| t as usize)
    }

    /// `rng.sample(list, n)`: shuffles the whole list, keeps the first n.
    #[inline]
    pub fn sample(&mut self, mut l: TankList, n: usize) -> TankList {
        let k = self.rng.sample_in_place(l.as_mut_slice(), n);
        l.truncate(k);
        l
    }

    pub fn roll_rarity(&mut self) -> Rarity {
        roll_rarity(self.d, self.rng)
    }

    pub fn roll_archetype(&mut self) -> Archetype {
        roll_archetype(self.rng)
    }

    /// +act/+qty to every monster.
    pub fn all(&mut self, act: i64, qty: i64) {
        for i in self.non_empty().iter() {
            self.boost(*i as i32, act, qty);
        }
    }
}
