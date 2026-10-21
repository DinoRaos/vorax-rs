//! Card and instrument effects, ported by hand from src/data/potions/*.ts and src/data/instruments.ts.
//! Every random draw happens in the same order as there (argument order included), so a seed plays the same.
//! When upstream adds or changes a card, data.rs refuses to load until it is ported here, and the trace tests
//! (tests/parity.rs) show any effect that plays differently.

use crate::context::Ctx;
use crate::data::Targeting::{self, *};
use crate::types::Archetype::*;
use crate::types::Rarity::*;
use crate::types::*;

/// The potions this port implements: id → (name, targeting). Potion Kits have no effect and aren't listed.
pub fn potion_port(id: u8) -> Option<(&'static str, Targeting)> {
    Some(match id {
        0 => ("Probiotic Enzyme Solution", Auto),
        1 => ("Slough Solution", Select1To2),
        2 => ("Leptomeningeal Solution", Select1),
        3 => ("Osteolytic Ointment", Auto),
        4 => ("Cleansing Ointment", Select1),
        5 => ("Fresh Spinal Powder", Auto),
        6 => ("Petrified Spinal Solution", Select1),
        7 => ("Viviparous Hormone", Auto),
        8 => ("Eggshell Powder", Auto),
        9 => ("Brain Fog Tincture", Auto),
        10 => ("Osteogenic Powder", Auto),
        11 => ("Gray Matter Spinal Solution", Auto),
        12 => ("Hollow Spinal Solution", Select1),
        13 => ("Green Bile Solution", Select1To2),
        14 => ("Compound Leech Solution", Select1),
        15 => ("Digestive Enzyme Solution", Select1),
        16 => ("Vivigenic Powder", Auto),
        17 => ("Potent Xenopurge Powder", Select1),
        18 => ("Xenopurge Powder", Select1),
        19 => ("Pure Leech Solution", Select1),
        20 => ("Bewitching Tincture", Select1),
        21 => ("Mutagenic Powder", Select1To2),
        22 => ("Viscous Bile Solution", Select1),
        23 => ("Slender-Limb Powder - Pest", Auto),
        24 => ("Twin Hormone - Pest", Select1),
        25 => ("Spinal Solution - Agito", Auto),
        26 => ("Anesthetic Tincture - Agito", Select1),
        27 => ("Slender-Limb Powder - Bone Sentry", Auto),
        28 => ("Twin Hormone - Bone Sentry", Select1),
        29 => ("Anesthetic Tincture - Aemberon", Select1),
        30 => ("Spinal Solution - Aemberon", Auto),
        31 => ("Xeno-Hormone", Select1To2),
        32 => ("Targeted Xeno-Hormone", Select1),
        35 => ("Compound Revitalizing Pill", Auto),
        36 => ("Fast-Acting Heart Tonic", Auto),
        37 => ("Plague Quarter Peat Poultice", Auto),
        38 => ("Insect Attractant", Auto),
        39 => ("Purest Holy Water", Auto),
        41 => ("Rousing Salts", Auto),
        _ => return None,
    })
}

/// The instruments this port implements: id → name.
pub fn instrument_port(id: u8) -> Option<&'static str> {
    Some(match id {
        1 => "Chrysalis Specimen",
        2 => "Incubation Sac",
        3 => "Hyperplastic Frontal Lobe",
        4 => "Hypertrophic Pituitary",
        5 => "Adherent Metatarsals",
        6 => "Contracted Claws",
        7 => "Aberrant Sprout",
        8 => "Prismatic Liver",
        9 => "Clustered Eggs",
        10 => "Sloughed Cortex",
        11 => "Sinew Garrote",
        12 => "Vermicular Spinal Cord",
        13 => "Rawhide Restraint",
        14 => "Black Goat Gut Suture",
        15 => "Pig Iron Bone Saw",
        16 => "Two-Inch Skull Nail",
        17 => "Fouled Scraper",
        18 => "Canine Teeth File",
        19 => "Lady of the Plague Quarter",
        20 => "Aberrant Stinger",
        21 => "Tanned Leather Restraint",
        22 => "Eyelid Speculum",
        23 => "Beak of the Twice-Born",
        24 => "Nettle Toggle",
        25 => "Egg-Burdened Butterfly",
        _ => return None,
    })
}

/// A potion's effect on `targets` (empty when it needs none).
pub fn apply_potion(c: &mut Ctx, id: u8, targets: &[u8]) {
    let t0 = targets.first().map(|&t| t as usize);
    // `[t]` destructuring in the TypeScript: the first target's monster, if any
    let first = t0.and_then(|t| c.mon(t).map(|m| (t, m)));
    match id {
        // Probiotic Enzyme Solution
        0 => {
            let pests = c.by_archetype(Pest);
            let n = pests.len() as i64;
            for &i in &pests {
                c.boost(i as i32, 11 * n, 0);
            }
        }
        // Slough Solution
        1 => {
            if !targets.is_empty() {
                let r = c.roll_rarity();
                c.fuse(targets, Pest, r);
            }
        }
        // Leptomeningeal Solution
        2 => {
            let Some((t, m)) = first else { return };
            let times = if m.archetype == Agito {
                match m.rarity {
                    Magic => 2,
                    Rare | Boss => 3,
                    Normal => 1,
                }
            } else {
                1
            };
            c.boost(t as i32, 30 * times, 0);
        }
        // Osteolytic Ointment
        3 => {
            if c.by_archetype(BoneSentry).is_empty() {
                return;
            }
            let others = c.filter(|_, m| m.archetype != BoneSentry);
            let victims = c.sample(others, 3);
            for &v in &victims {
                c.remove(v as usize);
                c.all(28, 28);
            }
        }
        // Cleansing Ointment
        4 => {
            let Some((t, m)) = first else { return };
            c.boost(t as i32, 20, 0);
            if m.archetype != BoneSentry {
                return;
            }
            c.boost(t as i32, 0, 41);
            let right = c.neighbor(t, 1);
            if right >= 0 {
                c.remove(right as usize);
            }
        }
        // Fresh Spinal Powder
        5 => {
            let others = c.filter(|_, m| m.archetype != Aemberon);
            let picks = c.sample(others, 3);
            for &p in &picks {
                c.mutate(p as usize, Some(Aemberon), None);
                let aem = c.by_archetype(Aemberon);
                for &i in &c.sample(aem, 3) {
                    c.boost(i as i32, 0, 42);
                }
            }
        }
        // Petrified Spinal Solution
        6 => {
            let Some((t, m)) = first else { return };
            c.boost(t as i32, 20, 0);
            if m.archetype == Aemberon {
                return;
            }
            c.mutate(t, Some(Aemberon), None);
            c.boost(t as i32, 30, 0);
        }
        // Viviparous Hormone
        7 => {
            let r = c.roll_rarity();
            c.add(Pest, r, 0, 0);
            if c.rng.chance(0.5) {
                let r = c.roll_rarity();
                c.add(Pest, r, 0, 0);
                let r = c.roll_rarity();
                c.add(Pest, r, 0, 0);
            }
        }
        // Eggshell Powder
        8 => {
            let ne = c.non_empty();
            let Some(t) = c.pick(&ne) else { return };
            let m = c.mon(t).unwrap();
            c.boost(t as i32, 0, 25);
            if m.archetype == Pest {
                c.add(Pest, m.rarity, 0, 25);
            }
        }
        // Brain Fog Tincture
        9 => {
            let ne = c.non_empty();
            let Some(t) = c.pick(&ne) else { return };
            c.boost(t as i32, 41, 0);
            if c.mon(t).unwrap().archetype == Agito {
                c.awaken(t, Some(Rare), None);
            }
        }
        // Osteogenic Powder
        10 => {
            let ne = c.non_empty();
            let Some(t) = c.pick(&ne) else { return };
            c.boost(t as i32, 0, 40);
            if c.mon(t).unwrap().archetype != BoneSentry {
                return;
            }
            c.boost(t as i32, 0, 127);
            let others = c.filter(|_, m| m.archetype != BoneSentry);
            if let Some(v) = c.pick(&others) {
                c.remove(v);
            }
        }
        // Gray Matter Spinal Solution
        11 => {
            let ne = c.non_empty();
            let Some(t) = c.pick(&ne) else { return };
            c.boost(t as i32, 41, 0);
            c.mutate(t, None, None);
            if c.rng.chance(0.5) {
                c.mutate(t, Some(Aemberon), None);
                c.boost(t as i32, 30, 0);
            }
        }
        // Hollow Spinal Solution
        12 => {
            let Some((t, _)) = first else { return };
            c.boost(t as i32, 41, 0);
            if c.rng.chance(0.5) {
                c.mutate(t, Some(Aemberon), None);
            }
        }
        // Green Bile Solution
        13 => {
            for &t in targets {
                if c.mon(t as usize).is_none() {
                    continue;
                }
                c.mutate(t as usize, None, Some(Magic));
                c.boost(t as i32, 31, 0);
            }
        }
        // Compound Leech Solution
        14 => {
            let Some((_, m)) = first else { return };
            let same = c.by_rarity(m.rarity);
            for &i in &c.sample(same, 2) {
                c.boost(i as i32, 31, 0);
            }
        }
        // Digestive Enzyme Solution
        15 => {
            let Some((t, _)) = first else { return };
            c.boost(t as i32, 61, 0);
            let left = c.neighbor(t, -1);
            if left >= 0 {
                c.remove(left as usize);
            }
        }
        // Vivigenic Powder
        16 => {
            for r in RARITIES {
                let l = c.by_rarity(r);
                if let Some(t) = c.pick(&l) {
                    c.boost(t as i32, 0, 32);
                }
            }
        }
        // Potent Xenopurge Powder
        17 => {
            let Some((t, m)) = first else { return };
            c.boost(t as i32, 0, 154);
            let others = c.filter(|i, o| i != t && o.archetype != m.archetype);
            for &v in &c.sample(others, 2) {
                c.remove(v as usize);
            }
        }
        // Xenopurge Powder
        18 => {
            let Some((t, m)) = first else { return };
            c.boost(t as i32, 0, 127);
            let others = c.filter(|i, o| i != t && o.archetype != m.archetype);
            if let Some(v) = c.pick(&others) {
                c.remove(v);
            }
        }
        // Pure Leech Solution
        19 => {
            let Some((_, m)) = first else { return };
            let same = c.by_archetype(m.archetype);
            for &i in &c.sample(same, 2) {
                c.boost(i as i32, 31, 0);
            }
        }
        // Bewitching Tincture
        20 => {
            if let Some(t) = t0 {
                c.awaken(t, None, None);
            }
        }
        // Mutagenic Powder
        21 => {
            for &t in targets {
                if c.mon(t as usize).is_none() {
                    continue;
                }
                c.boost(t as i32, 0, 52);
                c.mutate(t as usize, None, Some(Rare));
            }
        }
        // Viscous Bile Solution
        22 => {
            let Some((t, _)) = first else { return };
            c.boost(t as i32, 31, 0);
            let right = c.neighbor(t, 1);
            if right < 0 {
                return;
            }
            let m = c.mon(t).unwrap();
            c.mutate(right as usize, Some(m.archetype), Some(m.rarity));
            c.boost(right, 31, 0);
        }
        // Slender-Limb Powder - Pest
        23 => {
            let r = c.roll_rarity();
            c.add(Pest, r, 0, 73);
        }
        // Twin Hormone - Pest
        24 => {
            let Some((t, _)) = first else { return };
            c.mutate(t, Some(Pest), None);
            if let Some(m) = c.mon(t) {
                c.add(Pest, m.rarity, 0, 0);
            }
        }
        // Spinal Solution - Agito
        25 => {
            let r = c.roll_rarity();
            c.add(Agito, r, 31, 0);
        }
        // Anesthetic Tincture - Agito
        26 => {
            let Some((t, _)) = first else { return };
            c.mutate(t, Some(Agito), None);
            c.awaken(t, Some(Magic), None);
        }
        // Slender-Limb Powder - Bone Sentry
        27 => {
            let r = c.roll_rarity();
            c.add(BoneSentry, r, 0, 73);
        }
        // Twin Hormone - Bone Sentry
        28 => {
            let Some((t, _)) = first else { return };
            c.mutate(t, Some(BoneSentry), None);
            c.boost(t as i32, 0, 62);
        }
        // Anesthetic Tincture - Aemberon
        29 => {
            let Some((t, _)) = first else { return };
            c.mutate(t, Some(Aemberon), None);
            if !c.rng.chance(0.5) {
                return;
            }
            let others = c.filter(|i, _| i != t);
            if let Some(o) = c.pick(&others) {
                c.mutate(o, Some(Aemberon), None);
            }
        }
        // Spinal Solution - Aemberon
        30 => {
            let r = c.roll_rarity();
            c.add(Aemberon, r, 31, 0);
        }
        // Xeno-Hormone
        31 => {
            if targets.is_empty() {
                return;
            }
            for &t in targets {
                c.remove(t as usize);
            }
            for _ in 0..4 {
                let a = c.roll_archetype();
                let r = c.roll_rarity();
                c.add(a, r, 5, 0);
            }
        }
        // Targeted Xeno-Hormone
        32 => {
            let Some(t) = t0 else { return };
            c.remove(t);
            for _ in 0..2 {
                let a = c.roll_archetype();
                c.add(a, Magic, 0, 25);
            }
        }
        // Compound Revitalizing Pill
        35 => {
            for &t in &c.non_empty() {
                let t = t as usize;
                c.mutate(t, Some(Aemberon), None);
                let Some(m) = c.mon(t) else { continue };
                c.boost(t as i32, 11, 0);
                if m.rarity == Magic {
                    c.boost(t as i32, 11, 0);
                } else if m.rarity.is_rare_or_boss() {
                    c.boost(t as i32, 0, 11);
                }
            }
        }
        // Fast-Acting Heart Tonic
        36 => {
            let t = c.lowest();
            if t >= 0 {
                c.boost(t, 42, 42);
            }
        }
        // Plague Quarter Peat Poultice
        37 => {
            if c.by_archetype(BoneSentry).is_empty() {
                return;
            }
            for &v in &c.filter(|_, m| m.archetype != BoneSentry) {
                c.remove(v as usize);
                for &b in &c.by_archetype(BoneSentry) {
                    c.boost(b as i32, 20, 39);
                }
            }
        }
        // Insect Attractant
        38 => {
            let mut overflow = 0;
            for _ in 0..4 {
                let r = c.roll_rarity();
                if c.add(Pest, r, 0, 0) < 0 {
                    overflow += 1;
                }
            }
            if overflow == 0 {
                return;
            }
            for &p in &c.by_archetype(Pest) {
                c.boost(p as i32, 10 * overflow, 15 * overflow);
            }
        }
        // Purest Holy Water
        39 => {
            for &t in &c.by_archetype(Agito) {
                let t = t as usize;
                let r = c.mon(t).unwrap().rarity;
                if r == Boss {
                    c.boost(t as i32, 120, 0);
                } else {
                    c.awaken(t, Some(RARITIES[(r.index() + 2).min(3)]), Some(120));
                }
            }
        }
        // Rousing Salts
        41 => {
            let ne = c.non_empty();
            if let Some(t) = c.pick(&ne) {
                c.boost(t as i32, 111, 111);
            }
        }
        _ => {}
    }
}

/// Whether the instrument has an end-of-round (or periodic) effect.
pub fn has_end_of_round(id: u8) -> bool {
    !matches!(id, 2 | 5 | 6 | 7 | 8 | 12 | 16 | 22)
}

/// An end-of-round or periodic instrument's effect.
pub fn end_of_round(c: &mut Ctx, id: u8) {
    match id {
        // Chrysalis Specimen
        1 => {
            let x = c.by_archetype(Pest).len();
            if x == 0 {
                return;
            }
            for _ in 0..x {
                let ne = c.non_empty();
                for &i in &c.sample(ne, x) {
                    c.boost(i as i32, 0, 8);
                }
            }
        }
        // Hyperplastic Frontal Lobe
        3 => {
            if c.by_archetype(Agito).len() < 2 {
                return;
            }
            let l = c.filter(|_, m| m.rarity.is_rare_or_boss());
            if let Some(t) = c.pick(&l) {
                c.boost(t as i32, 80, 0);
            }
        }
        // Hypertrophic Pituitary
        4 => {
            if !c.tanks.iter().flatten().any(|m| m.archetype == Agito && m.rarity.is_rare_or_boss()) {
                return;
            }
            let ne = c.non_empty();
            if let Some(t) = c.pick(&ne) {
                c.boost(t as i32, 80, 0);
            }
        }
        // Clustered Eggs
        9 => {
            if c.by_archetype(Pest).len() < 3 {
                return;
            }
            let r = c.roll_rarity();
            c.add(Pest, r, 0, 100);
        }
        // Sloughed Cortex
        10 => {
            let magic = c.by_rarity(Magic);
            if magic.len() < 2 {
                return;
            }
            let two = c.sample(magic, 2);
            c.fuse(two.as_slice(), Agito, Rare);
        }
        // Sinew Garrote
        11 => {
            let bones = c.by_archetype(BoneSentry);
            if bones.len() < 2 {
                return;
            }
            let others = c.filter(|_, m| m.archetype != BoneSentry);
            if others.is_empty() {
                return;
            }
            let mut victim = others[0] as usize;
            for &i in &others {
                if total(&c.tanks[i as usize]) < total(&c.tanks[victim]) {
                    victim = i as usize;
                }
            }
            let removed = c.remove(victim);
            let target = c.pick(&bones);
            if let (Some(m), Some(t)) = (removed, target) {
                c.boost(t as i32, m.activity, m.quantity);
            }
        }
        // Rawhide Restraint
        13 => {
            if c.by_rarity(Magic).len() < 3 {
                return;
            }
            let t = c.highest();
            c.boost(t, 0, 100);
        }
        // Black Goat Gut Suture
        14 => {
            for r in [Normal, Magic, Rare] {
                let mut same = c.by_rarity(r);
                while same.len() >= 3 {
                    let triple = [same[0], same[1], same[2]];
                    let a = c.mon(triple[0] as usize).unwrap().archetype;
                    c.fuse(&triple, a, r.next());
                    same = c.by_rarity(r);
                }
            }
        }
        // Pig Iron Bone Saw
        15 => {
            for &i in &c.non_empty() {
                let m = c.mon(i as usize).unwrap();
                if m.activity > 150 && m.quantity > 150 {
                    c.boost(i as i32, 15, 15);
                }
            }
        }
        // Fouled Scraper
        17 => {
            for &i in &c.non_empty() {
                if c.mon(i as usize).unwrap().quantity > 275 {
                    c.boost(i as i32, 20, 0);
                }
            }
        }
        // Canine Teeth File
        18 => {
            for &i in &c.non_empty() {
                if c.mon(i as usize).unwrap().activity > 255 {
                    c.boost(i as i32, 0, 30);
                }
            }
        }
        // Lady of the Plague Quarter: every tier snapshotted first, so a monster awakened this trigger isn't picked again
        19 => {
            let tiers = RARITIES.map(|r| c.by_rarity(r));
            for candidates in &tiers {
                if let Some(t) = c.pick(candidates) {
                    c.awaken(t, None, None);
                }
            }
        }
        // Aberrant Stinger
        20 => {
            for &i in &c.non_empty() {
                c.mutate(i as usize, None, None);
            }
        }
        // Tanned Leather Restraint
        21 => {
            let ms = c.tanks.iter().flatten();
            let (mut n, mut m, mut r) = (false, false, false);
            for x in ms {
                n |= x.rarity == Normal;
                m |= x.rarity == Magic;
                r |= x.rarity.is_rare_or_boss();
            }
            if !(n && m && r) {
                return;
            }
            c.all(20, 0);
        }
        // Beak of the Twice-Born
        23 => {
            let count = c.non_empty().len();
            if (2..=5).contains(&count) {
                let a = c.roll_archetype();
                let r = c.roll_rarity();
                c.add(a, r, 0, 0);
            } else if count == 6 {
                let hi = c.highest();
                let lo = c.lowest();
                if hi == lo {
                    return;
                }
                c.devour(hi as usize, lo as usize);
            }
        }
        // Nettle Toggle
        24 => {
            if c.by_rarity(Normal).len() < 3 {
                return;
            }
            c.all(0, 30);
        }
        // Egg-Burdened Butterfly
        25 => {
            let pests = c.by_archetype(Pest);
            if pests.len() < 2 {
                return;
            }
            let first = c.pick(&pests).unwrap();
            let mut rest = TankList::new();
            for &p in &pests {
                if p as usize != first {
                    rest.push(p);
                }
            }
            let second = c.pick(&rest).unwrap();
            let r = c.roll_rarity();
            c.fuse(&[first as u8, second as u8], Pest, r);
        }
        _ => {}
    }
}

/// A reactive instrument answering an event (the data's `on` list decides which events reach it).
pub fn on_event(c: &mut Ctx, id: u8, e: GameEvent) {
    match id {
        // Incubation Sac
        2 => {
            if c.by_archetype(Pest).len() < 2 {
                return;
            }
            c.all(0, 45);
        }
        // Adherent Metatarsals
        5 => {
            if c.by_archetype(BoneSentry).len() < 2 {
                return;
            }
            let ne = c.non_empty();
            if let Some(t) = c.pick(&ne) {
                c.boost(t as i32, 0, 180);
            }
        }
        // Contracted Claws
        6 => {
            let GameEvent::Remove { monster } = e else { return };
            if monster.archetype == BoneSentry {
                return;
            }
            let ne = c.non_empty();
            if let Some(t) = c.pick(&ne) {
                c.boost(t as i32, 0, 150);
            }
        }
        // Aberrant Sprout
        7 => {
            let GameEvent::Mutate { to } = e else { return };
            if to != Aemberon {
                return;
            }
            c.all(35, 0);
        }
        // Prismatic Liver
        8 => {
            if c.by_archetype(Aemberon).len() < 2 {
                return;
            }
            c.all(20, 0);
        }
        // Vermicular Spinal Cord
        12 => {
            let GameEvent::Add { tank } = e else { return };
            if c.mon(tank).is_none() || !c.rng.chance(0.75) {
                return;
            }
            c.mutate(tank, Some(Aemberon), None);
            c.boost(tank as i32, 80, 0);
        }
        // Two-Inch Skull Nail
        16 => {
            let t = c.lowest();
            if t < 0 {
                return;
            }
            c.boost(t, 25, 25);
        }
        // Eyelid Speculum
        22 => {
            let t = c.highest();
            if t < 0 {
                return;
            }
            c.boost(t, 20, 20);
        }
        _ => {}
    }
}
