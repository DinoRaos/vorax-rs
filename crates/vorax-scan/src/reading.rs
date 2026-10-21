//! What the reader gives back (src/solver/types.ts): the board, how sure it is of each field, what went wrong.

use crate::pixels::Rect;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use vorax::advisor::{ScreenPhase, ScreenState};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FieldRead {
    pub sure: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guess: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second: Option<f64>,
}

impl FieldRead {
    pub fn sure(sure: bool) -> FieldRead {
        FieldRead { sure, ..Default::default() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Problem {
    NotFound,
    /// a card is being played or picked: there is no hand to read
    Animating,
    Unreadable { field: String },
    /// hand: 5 when the card was in a Potion Kit's five-card hand
    UnknownArt {
        field: String,
        crop: Rect,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hand: Option<u8>,
        /// the card's printed title as read, when the text reader ran
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    UnknownName { name: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TankDebug {
    pub rgb: [f64; 3],
    pub total: String,
    pub aq: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReadDebug {
    pub off: f64,
    pub corrected: bool,
    pub tanks: Vec<Option<TankDebug>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
}

pub type Fields = IndexMap<String, FieldRead>;
pub type Ranks = Vec<Vec<(String, f64)>>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub state: ScreenState,
    pub fields: Fields,
    pub problems: Vec<Problem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<Frame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrument_ranks: Option<Ranks>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hand_ranks: Option<Ranks>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icons_known: Option<Vec<String>>,
    /// cards are showing but weren't read from their pictures: left to their printed names
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hand_pending: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hand_size: Option<usize>,
    /// the game's interface scale it was read at, when not 1 (layout.rs)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug: Option<ReadDebug>,
    /// how far below the layout's place the hand was found (reference pixels); not in upstream
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hand_dy: Option<f64>,
}

impl Reading {
    /// The interface scale it was read at.
    pub fn ui(&self) -> f64 {
        self.ui.unwrap_or(1.0)
    }

    pub fn has(&self, f: impl Fn(&Problem) -> bool) -> bool {
        self.problems.iter().any(f)
    }
}

pub fn empty_state() -> ScreenState {
    ScreenState {
        tanks: vec![None; 6],
        hand: vec![],
        instruments: vec![],
        instrument_rounds: None,
        phase: ScreenPhase::Potion,
        round: Some(1),
        potion_refreshes: Some(3),
        instrument_refreshes: None,
        shelley: None,
    }
}
