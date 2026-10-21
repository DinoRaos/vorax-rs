//! Game state, as src/engine/types.ts, but fixed-size so a state copies with a memcpy.

use crate::stack::StackVec;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[repr(u8)]
pub enum Archetype {
    #[default]
    Pest,
    Aemberon,
    BoneSentry,
    Agito,
}
pub const ARCHETYPES: [Archetype; 4] = [Archetype::Pest, Archetype::Aemberon, Archetype::BoneSentry, Archetype::Agito];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[repr(u8)]
pub enum Rarity {
    #[default]
    Normal,
    Magic,
    Rare,
    Boss,
}
pub const RARITIES: [Rarity; 4] = [Rarity::Normal, Rarity::Magic, Rarity::Rare, Rarity::Boss];

impl Rarity {
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn next(self) -> Rarity {
        RARITIES[(self.index() + 1).min(3)]
    }
    /// Boss counts as Rare for condition checks only.
    pub fn is_rare_or_boss(self) -> bool {
        matches!(self, Rarity::Rare | Rarity::Boss)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Monster {
    pub archetype: Archetype,
    pub rarity: Rarity,
    pub activity: i64,
    pub quantity: i64,
}

impl Monster {
    #[inline]
    pub fn total(&self) -> i64 {
        self.activity * self.quantity
    }
}

pub const TANK_COUNT: usize = 6;
pub type Tanks = [Option<Monster>; TANK_COUNT];

#[inline]
pub fn total(m: &Option<Monster>) -> i64 {
    m.map_or(0, |m| m.total())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    #[default]
    InstrumentSelect,
    PotionSelect,
    /// an instrument-only round (11–13) waiting for the player to end it
    RoundEnd,
    GameOver,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CardKind {
    #[default]
    Potion,
    Instrument,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Card {
    pub kind: CardKind,
    pub id: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Equipped {
    pub id: u8,
    pub acquired_round: i32,
}

pub const MAX_HAND: usize = 8;
pub const MAX_EQUIPPED: usize = 8;
pub type Hand = StackVec<Card, MAX_HAND>;
pub type Equips = StackVec<Equipped, MAX_EQUIPPED>;
pub type Targets = StackVec<u8, 2>;
/// Tank indices (at most 6).
pub type TankList = StackVec<u8, TANK_COUNT>;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Sandbox {
    pub start_round: i32,
    pub forced_hand: StackVec<u8, MAX_HAND>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Settings {
    pub potion_refreshes: i32,
    pub instrument_refreshes: i32,
}

/// A sandbox start (SandboxSetup): fixed instruments, round and thresholds, optionally fixed tanks and first hand.
#[derive(Clone, Debug, Default)]
pub struct SandboxSetup {
    pub tanks: Option<Tanks>,
    pub instruments: Vec<Equipped>,
    pub start_round: i32,
    pub thresholds_used: (bool, bool),
    pub forced_hand: Vec<u8>,
    pub seed: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct GameState {
    pub tanks: Tanks,
    pub instruments: Equips,
    pub round: i32,
    pub phase: Phase,
    pub hand: Hand,
    pub open_box: Option<u8>,
    pub selected: Option<u8>,
    pub targets: Targets,
    pub potion_refreshes: i32,
    pub instrument_refreshes: i32,
    pub t8000: bool,
    pub t28000: bool,
    pub pending_picks: i32,
    pub rng_state: u32,
    pub sandbox: Option<Sandbox>,
}

impl GameState {
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

    #[inline]
    pub fn monsters(&self) -> impl Iterator<Item = &Monster> {
        self.tanks.iter().flatten()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Action {
    SelectCard { index: i64 },
    ToggleTarget { tank: i64 },
    Confirm,
    Refresh,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventType {
    Add,
    Remove,
    Mutate,
    Awaken,
    Fuse,
    Devour,
    AddOverflow,
}

impl EventType {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "add" => Self::Add,
            "remove" => Self::Remove,
            "mutate" => Self::Mutate,
            "awaken" => Self::Awaken,
            "fuse" => Self::Fuse,
            "devour" => Self::Devour,
            "addOverflow" => Self::AddOverflow,
            _ => return None,
        })
    }
}

/// The parts of a GameEvent the reactive instruments read.
#[derive(Clone, Copy, Debug)]
pub enum GameEvent {
    Add { tank: usize },
    Remove { monster: Monster },
    Mutate { to: Archetype },
    Awaken,
    Fuse,
    Devour,
    AddOverflow,
}

impl GameEvent {
    pub fn kind(&self) -> EventType {
        match self {
            Self::Add { .. } => EventType::Add,
            Self::Remove { .. } => EventType::Remove,
            Self::Mutate { .. } => EventType::Mutate,
            Self::Awaken => EventType::Awaken,
            Self::Fuse => EventType::Fuse,
            Self::Devour => EventType::Devour,
            Self::AddOverflow => EventType::AddOverflow,
        }
    }
}
