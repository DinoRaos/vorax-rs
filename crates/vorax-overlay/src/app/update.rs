//! Keeping the data current without a rebuild. Upstream commits new odds hourly, a smarter board value nightly
//! and the icons players named: all data, fetched here from upstream's newest commit, checked as the build
//! checks its own (every card and instrument must be one this build has ported), and kept in
//! %APPDATA%\VoraxRs\data, which the overlay loads at start.
//!
//! Rules are code, and can't be fetched: when upstream changes a card's effect, the reader or the advisor, the
//! port needs updating (scripts/sync.ps1 shows what). The updater only notices it and says so.

use super::sys::{data_dir, note};
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;
use windows::core::HSTRING;
use windows::Foundation::Uri;
use windows::Web::Http::HttpClient;

const REPO: &str = "tidewarden/tli-vorax-simulator";

/// Upstream files that are rules (ported by hand), not data.
fn is_rule_file(f: &str) -> bool {
    let rules = ["src/engine/", "src/strategy/", "src/data/potions/", "src/data/instruments.ts", "src/data/archetypes.ts", "src/data/defs.ts", "src/data/shelley.ts", "src/solver/scan/", "src/solver/track.ts", "src/solver/live/triggers.ts", "src/solver/live/windowCrop.ts"];
    rules.iter().any(|r| f.starts_with(r)) && f.ends_with(".ts")
}

pub fn updated_dir() -> PathBuf {
    data_dir().join("data")
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Meta {
    /// upstream's commit the data came from
    pub commit: String,
    pub odds: String,
    /// rule files upstream changed since this build (the port is behind on these)
    #[serde(default)]
    pub rules_changed: Vec<String>,
    /// the build the rules were compared against
    #[serde(default)]
    pub built: String,
}

impl Meta {
    pub fn load() -> Option<Meta> {
        serde_json::from_str(&std::fs::read_to_string(updated_dir().join("meta.json")).ok()?).ok()
    }
}

fn get(client: &HttpClient, url: &str) -> windows::core::Result<String> {
    Ok(client.GetStringAsync(&Uri::CreateUri(&HSTRING::from(url))?)?.get()?.to_string())
}

/// A TypeScript object literal (liveWeights.ts) as JSON: keys quoted, 'strings' in double quotes, comments and
/// trailing commas dropped.
pub fn ts_object_to_json(src: &str) -> Result<String, String> {
    let start = src.find("LIVE_WEIGHTS").and_then(|i| src[i..].find('=').map(|j| i + j + 1)).ok_or("no LIVE_WEIGHTS in liveWeights.ts")?;
    let s: Vec<char> = src[start..].chars().collect();
    let mut out = String::new();
    let mut depth = 0i32;
    let mut i = 0;
    let trim_comma = |out: &mut String| {
        let t = out.trim_end().len();
        if out[..t].ends_with(',') {
            out.truncate(t - 1);
        }
    };
    while i < s.len() {
        let c = s[i];
        match c {
            '/' if s.get(i + 1) == Some(&'/') => {
                while i < s.len() && s[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '/' if s.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < s.len() && !(s[i] == '*' && s[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
                continue;
            }
            '\'' | '"' => {
                let q = c;
                let mut text = String::new();
                i += 1;
                while i < s.len() && s[i] != q {
                    if s[i] == '\\' && i + 1 < s.len() {
                        i += 1;
                    }
                    text.push(s[i]);
                    i += 1;
                }
                out.push_str(&serde_json::to_string(&text).unwrap());
            }
            '{' | '[' => {
                depth += 1;
                out.push(c);
            }
            '}' | ']' => {
                trim_comma(&mut out);
                out.push(c);
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            c if c.is_ascii_alphabetic() || c == '_' || c == '$' => {
                let mut id = String::new();
                while i < s.len() && (s[i].is_ascii_alphanumeric() || s[i] == '_' || s[i] == '$') {
                    id.push(s[i]);
                    i += 1;
                }
                let mut k = i;
                while k < s.len() && s[k].is_whitespace() {
                    k += 1;
                }
                if s.get(k) == Some(&':') {
                    out.push_str(&format!("\"{id}\""));
                } else {
                    out.push_str(&id);
                }
                continue;
            }
            _ => out.push(c),
        }
        i += 1;
    }
    Ok(out)
}

/// The rule files upstream changed between two commits.
fn rules_changed(client: &HttpClient, base: &str, head: &str) -> Result<Vec<String>, String> {
    if base == head {
        return Ok(vec![]);
    }
    let cmp: Value = serde_json::from_str(&get(client, &format!("https://api.github.com/repos/{REPO}/compare/{base}...{head}")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(cmp["files"].as_array().map(|fs| fs.iter().filter_map(|f| f["filename"].as_str()).filter(|f| is_rule_file(f)).map(String::from).collect()).unwrap_or_default())
}

pub enum Checked {
    /// nothing newer upstream
    Current(Meta),
    /// newer data fetched and stored: in use from the next start
    Fetched(Meta),
}

/// Why a check came to nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Failed {
    /// GitHub didn't answer, or not in time
    Offline,
    /// GitHub answered, but upstream's newest data doesn't load in this build (a file it reads is gone, a card it
    /// hasn't ported): only a sync and rebuild takes it
    Changed,
}

/// HTTP 404 as Windows reports it: a file upstream no longer has.
const NOT_FOUND: i32 = 0x80190194u32 as i32;

/// Fetches upstream's newest data when there's a newer commit than the data in use.
pub fn check() -> Result<Checked, (Failed, String)> {
    let offline = |e: String| (Failed::Offline, e);
    let client = HttpClient::new().map_err(|e| offline(e.to_string()))?;
    if let Ok(h) = client.DefaultRequestHeaders() {
        let _ = h.UserAgent().and_then(|ua| ua.TryParseAdd(&HSTRING::from("vorax-rs-overlay")));
    }
    let head: Value = serde_json::from_str(&get(&client, &format!("https://api.github.com/repos/{REPO}/commits/main")).map_err(|e| offline(format!("GitHub: {e}")))?).map_err(|e| offline(e.to_string()))?;
    let sha = head["sha"].as_str().ok_or_else(|| offline("GitHub: no commit".into()))?.to_string();
    fetch(&client, sha)
}

/// Upstream's data at commit `sha`, unless it's the data in use already.
fn fetch(client: &HttpClient, sha: String) -> Result<Checked, (Failed, String)> {
    let changed = |e: String| (Failed::Changed, e);
    let embedded: Value = serde_json::from_str(vorax::data::EMBEDDED_GAME).map_err(|e| changed(e.to_string()))?;
    // VORAX_BUILT_COMMIT: test the fetch as if this build were from another commit
    let built = std::env::var("VORAX_BUILT_COMMIT").unwrap_or_else(|_| embedded["commit"].as_str().unwrap_or_default().to_string());
    if let Some(m) = Meta::load().filter(|m| m.commit == sha && m.built == built) {
        return Ok(Checked::Current(m));
    }
    if sha == built {
        return Ok(Checked::Current(Meta { commit: sha, odds: embedded["odds"].as_str().unwrap_or_default().into(), rules_changed: vec![], built }));
    }
    let raw = |path: &str| {
        get(client, &format!("https://raw.githubusercontent.com/{REPO}/{sha}/{path}")).map_err(|e| (if e.code().0 == NOT_FOUND { Failed::Changed } else { Failed::Offline }, format!("{path}: {e}")))
    };

    // the odds into this build's game.json (its cards and instruments are the ones it has ported)
    let weights: Value = serde_json::from_str(&ts_object_to_json(&raw("src/data/liveWeights.ts")?).map_err(changed)?).map_err(|e| changed(format!("liveWeights.ts: {e}")))?;
    let mut game = embedded.clone();
    game["odds"] = weights["updated"].clone();
    for k in ["supremeChance", "potionOffer", "boxOffer", "instrumentOffer", "spawnRarity"] {
        if weights[k].is_null() {
            return Err(changed(format!("liveWeights.ts: no {k}")));
        }
        game[k] = weights[k].clone();
    }
    let game = serde_json::to_string_pretty(&game).unwrap();
    let board = raw("src/data/boardValue.json")?;
    let scan = [raw("src/solver/scan/data.json")?, raw("src/solver/scan/learnedIcons.json")?, raw("src/solver/scan/levels.json")?];
    let names = include_str!("../../../../data/scan/names.json");

    // as strict as the build: refused when it doesn't load
    vorax::Data::parse(&game, Some(&board)).map_err(changed)?;
    vorax_scan::ScanData::parse(&scan[0], &scan[1], &scan[2], names).map_err(changed)?;
    let rules = rules_changed(client, &built, &sha).unwrap_or_else(|e| {
        note(&format!("couldn't compare upstream's rules: {e}"));
        vec![]
    });

    let dir = updated_dir();
    let tmp = data_dir().join("data.new");
    let _ = std::fs::remove_dir_all(&tmp);
    // a disk that won't take the files: nothing a sync would change, so reported as no answer
    let disk = |e: String| (Failed::Offline, e);
    std::fs::create_dir_all(tmp.join("scan")).map_err(|e| disk(e.to_string()))?;
    let write = |p: PathBuf, s: &str| std::fs::write(&p, s).map_err(|e| disk(format!("{}: {e}", p.display())));
    write(tmp.join("game.json"), &game)?;
    write(tmp.join("boardValue.json"), &board)?;
    for (f, s) in ["data.json", "learnedIcons.json", "levels.json"].iter().zip(&scan) {
        write(tmp.join("scan").join(f), s)?;
    }
    write(tmp.join("scan").join("names.json"), names)?;
    let meta = Meta { commit: sha, odds: weights["updated"].as_str().unwrap_or_default().into(), rules_changed: rules, built };
    write(tmp.join("meta.json"), &serde_json::to_string_pretty(&meta).unwrap())?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&tmp, &dir).map_err(|e| disk(e.to_string()))?;
    Ok(Checked::Fetched(meta))
}

/// `check` on its own thread, given up on after `wait` (a slow network never holds the overlay up).
pub fn check_within(wait: Duration) -> Result<Checked, (Failed, String)> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
        }
        let _ = tx.send(check());
    });
    rx.recv_timeout(wait).map_err(|_| (Failed::Offline, "no answer from GitHub in time".to_string()))?
}

/// The fetched data, when there is some for this build: loaded in place of the build's own.
pub fn use_fetched() -> Option<Meta> {
    let meta = Meta::load()?;
    let embedded: Value = serde_json::from_str(vorax::data::EMBEDDED_GAME).ok()?;
    if embedded["commit"].as_str() != Some(meta.built.as_str()) {
        // fetched by another build
        return None;
    }
    let dir = updated_dir();
    let game = vorax::Data::load_dir(&dir).map_err(|e| note(&format!("fetched data not used: {e}"))).ok()?;
    let scan = vorax_scan::ScanData::load_dir(&dir.join("scan")).map_err(|e| note(&format!("fetched reader data not used: {e}"))).ok()?;
    vorax::data::init(game).ok()?;
    vorax_scan::data::init(scan).ok()?;
    Some(meta)
}

/// What a check means for the tray menu (and the log).
pub fn status(r: Result<Checked, (Failed, String)>) -> super::window::ToUi {
    let in_use = vorax::data().odds.clone();
    let short = |c: &str| c.chars().take(7).collect::<String>();
    let warn = |m: &Meta| (!m.rules_changed.is_empty()).then(|| format!("Upstream rules changed ({} file{}): sync to rebuild", m.rules_changed.len(), if m.rules_changed.len() == 1 { "" } else { "s" }));
    match r {
        Ok(Checked::Current(m)) => {
            note(&format!("data up to date (upstream {})", short(&m.commit)));
            super::window::ToUi::Status { odds: format!("{in_use} · up to date"), warn: warn(&m), ready: None, failed: None }
        }
        Ok(Checked::Fetched(m)) => {
            note(&format!("fetched upstream {} (odds of {}){}", short(&m.commit), m.odds, if m.rules_changed.is_empty() { String::new() } else { format!("; rules changed: {}", m.rules_changed.join(", ")) }));
            super::window::ToUi::Status { odds: format!("{in_use} · newer ready"), warn: warn(&m), ready: Some(m.odds.clone()), failed: None }
        }
        Err((Failed::Offline, e)) => {
            note(&format!("update check failed: {e}"));
            super::window::ToUi::Status { odds: format!("{in_use} · couldn't check for updates"), warn: None, ready: None, failed: Some(Failed::Offline) }
        }
        Err((Failed::Changed, e)) => {
            note(&format!("upstream's newest data doesn't load in this build: {e}"));
            let warn = Some("Upstream's data changed: sync to rebuild".to_string());
            super::window::ToUi::Status { odds: format!("{in_use} · newer data needs a sync"), warn, ready: None, failed: Some(Failed::Changed) }
        }
    }
}
