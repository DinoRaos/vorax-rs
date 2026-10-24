//! The game data (data/game.json, data/boardValue.json): cards, instruments and the odds as of Oct 22, 2026, and the
//! board value (linear, 656 weights per round: with the Solver's many futures it plays as well as trained trees, and
//! five times faster). Embedded at build time.

use crate::effects;
use crate::types::{EventType, Rarity, Settings};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Targeting {
    Auto,
    Select1,
    Select1To2,
    /// a Potion Kit: opens a hand of this many potions
    Box(u8),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PotionRarity {
    Normal,
    Magic,
    Rare,
    Supreme,
}

#[derive(Clone, Debug)]
pub struct PotionDef {
    pub id: u8,
    pub name: String,
    pub rarity: PotionRarity,
    pub targeting: Targeting,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Pool {
    Basic,
    Advanced,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Timing {
    End,
    /// bit per EventType
    Reactive(u8),
    Periodic(i32),
}

impl Timing {
    pub fn reacts_to(self, e: EventType) -> bool {
        matches!(self, Timing::Reactive(mask) if mask & (1 << e as u8) != 0)
    }
}

#[derive(Clone, Debug)]
pub struct InstrumentDef {
    pub id: u8,
    pub name: String,
    pub pool: Pool,
    pub timing: Timing,
}

/// One decision tree: node 0 is the root; at a branch (f ≥ 0) go to l when x[f] < t, else to r; a leaf gives v.
#[derive(Clone, Debug, Deserialize)]
pub struct Tree {
    pub f: Vec<i32>,
    pub t: Vec<f64>,
    pub l: Vec<usize>,
    pub r: Vec<usize>,
    pub v: Vec<f64>,
}

/// A tree node packed for walking: an inner node's feature (`f`), threshold (`t`) and children, or a leaf
/// (`f` < 0) whose value is `t`.
#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub f: i32,
    pub l: u32,
    pub r: u32,
    pub t: f64,
}

/// A tree model's trees in one array, each from its root, for walking without chasing five arrays per tree.
#[derive(Clone, Debug)]
pub struct Forest {
    pub base: f64,
    pub roots: Vec<u32>,
    pub nodes: Vec<Node>,
}

impl Forest {
    fn pack(base: f64, trees: &[Tree]) -> Forest {
        let mut nodes = Vec::new();
        let mut roots = Vec::with_capacity(trees.len());
        for t in trees {
            let at = nodes.len() as u32;
            roots.push(at);
            for i in 0..t.f.len() {
                let leaf = t.f[i] < 0;
                nodes.push(Node { f: t.f[i], l: at + t.l[i] as u32, r: at + t.r[i] as u32, t: if leaf { t.v[i] } else { t.t[i] } });
            }
        }
        Forest { base, roots, nodes }
    }
}

/// Weights per round (one per feature), or decision trees added to `base`; the prediction is in millions.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum ValueModel {
    Rounds { rounds: Vec<Vec<f64>> },
    Trees { base: f64, trees: Vec<Tree> },
}

pub struct Data {
    /// the upstream commit the data was exported from
    pub commit: Option<String>,
    pub odds: String,
    pub supreme_chance: f64,
    pub default_settings: Settings,
    pub potions: Vec<PotionDef>,
    pub instruments: Vec<InstrumentDef>,
    potion_at: [u16; 256],
    instrument_at: [u16; 256],
    /// deal pools, in upstream's order (ids)
    pub normal_pool: Vec<u8>,
    pub box_pool: Vec<u8>,
    pub supreme_pool: Vec<u8>,
    pub basic_pool: Vec<u8>,
    pub advanced_pool: Vec<u8>,
    /// offer weights by id (1 when the odds don't name the card)
    pub potion_offer: [f64; 256],
    pub box_offer: [f64; 256],
    pub instrument_offer: [f64; 256],
    pub spawn_rarity: [f64; 4],
    pub model: Option<ValueModel>,
    /// a tree model's trees packed for walking
    pub forest: Option<Forest>,
    /// position of each instrument in upstream's list (the board value's feature order)
    pub instrument_index: [u16; 256],
    pub periodic: Vec<u8>,
}

const NONE: u16 = u16::MAX;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawGame {
    #[serde(default)]
    commit: Option<String>,
    odds: String,
    supreme_chance: f64,
    default_settings: RawSettings,
    potions: Vec<RawPotion>,
    instruments: Vec<RawInstrument>,
    potion_offer: HashMap<String, f64>,
    box_offer: HashMap<String, f64>,
    instrument_offer: RawInstrumentOffer,
    spawn_rarity: HashMap<String, f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSettings {
    potion_refreshes: i32,
    instrument_refreshes: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPotion {
    id: u8,
    name: String,
    rarity: PotionRarity,
    targeting: serde_json::Value,
    has_effect: bool,
}

#[derive(Deserialize)]
struct RawInstrument {
    id: u8,
    name: String,
    pool: Pool,
    timing: RawTiming,
}

#[derive(Deserialize)]
struct RawTiming {
    kind: String,
    #[serde(default)]
    on: Vec<String>,
    #[serde(default)]
    every: i32,
}

#[derive(Deserialize)]
struct RawInstrumentOffer {
    basic: HashMap<String, f64>,
    advanced: HashMap<String, f64>,
}

/// The game data this build was compiled with (game.json).
const EMBEDDED_GAME: &str = include_str!("../../../data/game.json");
const EMBEDDED_VALUE: &str = include_str!("../../../data/boardValue.json");

impl Data {
    /// The data this build was compiled with.
    pub fn embedded() -> Result<Data, String> {
        Data::parse(EMBEDDED_GAME, Some(EMBEDDED_VALUE))
    }

    pub fn parse(game: &str, board_value: Option<&str>) -> Result<Data, String> {
        let raw: RawGame = serde_json::from_str(game).map_err(|e| format!("game.json: {e}"))?;
        let model = match board_value.map(str::trim) {
            None | Some("") | Some("null") => None,
            Some(s) => Some(serde_json::from_str::<ValueModel>(s).map_err(|e| format!("boardValue.json: {e}"))?),
        };

        let mut problems = Vec::new();
        let mut potions = Vec::new();
        for p in &raw.potions {
            let targeting = match &p.targeting {
                serde_json::Value::String(s) if s == "auto" => Targeting::Auto,
                serde_json::Value::String(s) if s == "select1" => Targeting::Select1,
                serde_json::Value::String(s) if s == "select1to2" => Targeting::Select1To2,
                serde_json::Value::Object(o) if o.contains_key("box") => Targeting::Box(o["box"].as_u64().unwrap_or(0) as u8),
                other => {
                    problems.push(format!("potion {} ({}): unknown targeting {other}", p.id, p.name));
                    Targeting::Auto
                }
            };
            let is_box = matches!(targeting, Targeting::Box(_));
            match effects::potion_port(p.id) {
                Some((name, t)) if name == p.name && t == targeting => {}
                Some((name, t)) => problems.push(format!("potion {}: upstream has \"{}\" ({targeting:?}), the port has \"{name}\" ({t:?})", p.id, p.name)),
                None if is_box && !p.has_effect => {}
                None => problems.push(format!("potion {} \"{}\" is new upstream: port its effect in crates/vorax/src/effects.rs", p.id, p.name)),
            }
            potions.push(PotionDef { id: p.id, name: p.name.clone(), rarity: p.rarity, targeting });
        }

        let mut instruments = Vec::new();
        for d in &raw.instruments {
            let timing = match d.timing.kind.as_str() {
                "end" => Timing::End,
                "periodic" => Timing::Periodic(d.timing.every),
                "reactive" => {
                    let mut mask = 0u8;
                    for e in &d.timing.on {
                        match EventType::parse(e) {
                            Some(t) => mask |= 1 << t as u8,
                            None => problems.push(format!("instrument {} ({}): unknown event {e}", d.id, d.name)),
                        }
                    }
                    Timing::Reactive(mask)
                }
                k => {
                    problems.push(format!("instrument {} ({}): unknown timing {k}", d.id, d.name));
                    Timing::End
                }
            };
            match effects::instrument_port(d.id) {
                Some(name) if name == d.name => {}
                Some(name) => problems.push(format!("instrument {}: upstream has \"{}\", the port has \"{name}\"", d.id, d.name)),
                None => problems.push(format!("instrument {} \"{}\" is new upstream: port it in crates/vorax/src/effects.rs", d.id, d.name)),
            }
            instruments.push(InstrumentDef { id: d.id, name: d.name.clone(), pool: d.pool, timing });
        }
        if !problems.is_empty() {
            return Err(format!("the Rust port is behind upstream's rules:\n  {}", problems.join("\n  ")));
        }

        let mut potion_at = [NONE; 256];
        for (i, p) in potions.iter().enumerate() {
            potion_at[p.id as usize] = i as u16;
        }
        let mut instrument_at = [NONE; 256];
        for (i, d) in instruments.iter().enumerate() {
            instrument_at[d.id as usize] = i as u16;
        }

        let normal_pool: Vec<u8> = potions.iter().filter(|p| p.rarity != PotionRarity::Supreme).map(|p| p.id).collect();
        let box_pool: Vec<u8> = normal_pool.iter().copied().filter(|&id| !matches!(potions[potion_at[id as usize] as usize].targeting, Targeting::Box(_))).collect();
        let supreme_pool: Vec<u8> = potions.iter().filter(|p| p.rarity == PotionRarity::Supreme).map(|p| p.id).collect();
        let basic_pool: Vec<u8> = instruments.iter().filter(|d| d.pool == Pool::Basic).map(|d| d.id).collect();
        let advanced_pool: Vec<u8> = instruments.iter().filter(|d| d.pool == Pool::Advanced).map(|d| d.id).collect();

        let mut potion_offer = [1.0; 256];
        let mut box_offer = [1.0; 256];
        for p in &potions {
            if let Some(&w) = raw.potion_offer.get(&p.name) {
                potion_offer[p.id as usize] = w;
            }
            if let Some(&w) = raw.box_offer.get(&p.name) {
                box_offer[p.id as usize] = w;
            }
        }
        let mut instrument_offer = [1.0; 256];
        for d in &instruments {
            let pool = if d.pool == Pool::Basic { &raw.instrument_offer.basic } else { &raw.instrument_offer.advanced };
            if let Some(&w) = pool.get(&d.name) {
                instrument_offer[d.id as usize] = w;
            }
        }
        let spawn = |r: &str| raw.spawn_rarity.get(r).copied().unwrap_or(0.0);
        let periodic = instruments.iter().filter(|d| matches!(d.timing, Timing::Periodic(_))).map(|d| d.id).collect();

        Ok(Data {
            commit: raw.commit,
            odds: raw.odds,
            supreme_chance: raw.supreme_chance,
            default_settings: Settings { potion_refreshes: raw.default_settings.potion_refreshes, instrument_refreshes: raw.default_settings.instrument_refreshes },
            potions,
            instruments,
            potion_at,
            instrument_index: instrument_at,
            instrument_at,
            normal_pool,
            box_pool,
            supreme_pool,
            basic_pool,
            advanced_pool,
            potion_offer,
            box_offer,
            instrument_offer,
            spawn_rarity: [spawn("normal"), spawn("magic"), spawn("rare"), spawn("boss")],
            forest: match &model {
                Some(ValueModel::Trees { base, trees }) => Some(Forest::pack(*base, trees)),
                _ => None,
            },
            model,
            periodic,
        })
    }

    #[inline]
    pub fn potion(&self, id: u8) -> &PotionDef {
        let at = self.potion_at[id as usize];
        assert!(at != NONE, "Unknown potion id {id}");
        &self.potions[at as usize]
    }

    #[inline]
    pub fn has_potion(&self, id: u8) -> bool {
        self.potion_at[id as usize] != NONE
    }

    #[inline]
    pub fn instrument(&self, id: u8) -> Option<&InstrumentDef> {
        let at = self.instrument_at[id as usize];
        (at != NONE).then(|| &self.instruments[at as usize])
    }

    pub fn potion_by_name(&self, name: &str) -> Option<u8> {
        let n = norm(name);
        self.potions.iter().find(|p| norm(&p.name) == n).map(|p| p.id)
    }

    pub fn instrument_by_name(&self, name: &str) -> Option<u8> {
        let n = norm(name);
        self.instruments.iter().find(|d| norm(&d.name) == n).map(|d| d.id)
    }

    #[inline]
    pub fn roll_weights_total(&self) -> f64 {
        self.spawn_rarity.iter().fold(0.0, |s, w| s + w)
    }

    pub fn spawn_weight(&self, r: Rarity) -> f64 {
        self.spawn_rarity[r.index()]
    }
}

/// Lower case, letters only: how the advisor matches names read off the screen.
pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphabetic()).map(|c| c.to_ascii_lowercase()).collect()
}

static DATA: OnceLock<Data> = OnceLock::new();

/// The game data this build was compiled with.
pub fn data() -> &'static Data {
    DATA.get_or_init(|| Data::embedded().unwrap_or_else(|e| panic!("{e}")))
}

/// For training: the built-in game data with another board value (a model's JSON, or None for none, when boards
/// are judged by their score). Only before the first `data()` call.
pub fn use_model(board_value: Option<&str>) -> Result<(), String> {
    let d = Data::parse(EMBEDDED_GAME, Some(board_value.unwrap_or("null")))?;
    DATA.set(d).map_err(|_| "game data already loaded".to_string())
}
