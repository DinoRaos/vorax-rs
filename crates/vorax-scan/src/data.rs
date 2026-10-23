//! The reader's reference data (data/scan: data.json, learnedIcons.json, levels.json, and the names titles are
//! matched against), as tli-vorax-simulator last published it. Embedded at build time.

use crate::pixels::{Art, ArtFeature};
use base64::Engine;
use indexmap::IndexMap;
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Packed {
    digits: IndexMap<String, IndexMap<String, Vec<Vec<f64>>>>,
    cards: IndexMap<String, Vec<ArtFeature>>,
    texts: IndexMap<String, Vec<String>>,
    headers: IndexMap<String, Vec<Vec<f64>>>,
    instrument_icons: IndexMap<String, Vec<Vec<f64>>>,
}

#[derive(Deserialize)]
pub struct Names {
    pub potion: Vec<String>,
    pub instrument: Vec<String>,
    pub protocol: Vec<String>,
}

pub struct ScanData {
    /// glyph vectors by text kind ("label", "round", "refresh"), then character
    pub digits: IndexMap<String, IndexMap<String, Vec<Vec<f64>>>>,
    /// card art by "phase:name", centred for comparing
    pub cards: IndexMap<String, Vec<Art>>,
    /// card text-block shapes by "phase:name"
    pub texts: IndexMap<String, Vec<Vec<f64>>>,
    /// header text shape for each phase
    pub headers: IndexMap<String, Vec<Vec<f64>>>,
    /// equipped-instrument panel icons (built in, then the ones players taught)
    pub icons: IndexMap<String, Vec<Vec<f64>>>,
    /// a normal capture's levels (levels.json)
    pub levels: Vec<f64>,
    pub names: Names,
}

/// Card text shapes are packed one signed byte per value, in steps of this.
const TEXT_STEP: f64 = 0.002;

impl ScanData {
    pub fn parse(data: &str, learned: &str, levels: &str, names: &str) -> Result<ScanData, String> {
        let p: Packed = serde_json::from_str(data).map_err(|e| format!("scan data.json: {e}"))?;
        let learned: IndexMap<String, Vec<Vec<f64>>> = serde_json::from_str(learned).map_err(|e| format!("learnedIcons.json: {e}"))?;
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut texts = IndexMap::new();
        for (k, vs) in p.texts {
            let mut out = Vec::new();
            for s in vs {
                let bytes = b64.decode(s.as_bytes()).map_err(|e| format!("scan text {k}: {e}"))?;
                out.push(bytes.iter().map(|&b| (b as i8) as f64 * TEXT_STEP).collect());
            }
            texts.insert(k, out);
        }
        let mut icons = p.instrument_icons;
        for (name, vs) in learned {
            icons.entry(name).or_default().extend(vs);
        }
        Ok(ScanData {
            digits: p.digits,
            cards: p.cards.into_iter().map(|(k, fs)| (k, fs.iter().map(Art::from).collect())).collect(),
            texts,
            headers: p.headers,
            icons,
            levels: serde_json::from_str(levels).map_err(|e| format!("levels.json: {e}"))?,
            names: serde_json::from_str(names).map_err(|e| format!("names.json: {e}"))?,
        })
    }

    pub fn embedded() -> Result<ScanData, String> {
        ScanData::parse(
            include_str!("../../../data/scan/data.json"),
            include_str!("../../../data/scan/learnedIcons.json"),
            include_str!("../../../data/scan/levels.json"),
            include_str!("../../../data/scan/names.json"),
        )
    }

}

static DATA: OnceLock<ScanData> = OnceLock::new();

pub fn scan_data() -> &'static ScanData {
    DATA.get_or_init(|| ScanData::embedded().unwrap_or_else(|e| panic!("{e}")))
}
