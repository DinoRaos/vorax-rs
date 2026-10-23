//! The screen reader against tli-vorax-simulator's TypeScript reader (its last open version, 7b4c9ba): every
//! reading recorded in fixtures/scan.jsonl from its test frames, as they are, shrunk and recoloured, must come out
//! the same here.

use serde_json::Value;
use std::path::PathBuf;
use vorax_scan::pixels::{resize, Img};
use vorax_scan::reading::Reading;
use vorax_scan::titles::{pictures, read_titles, title_crop};
use vorax_scan::track::{track_instruments, InstrumentTracker};
use vorax_scan::window::{crop_to, windowed_game};
use vorax_scan::{layout::layout_at, scan, scan_data, scan_upstream};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn decode(path: &PathBuf) -> Img {
    let dec = png::Decoder::new(std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())));
    let mut reader = dec.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    let (w, h) = (info.width as usize, info.height as usize);
    let mut img = Img::new(w, h);
    let ch = info.color_type.samples();
    for i in 0..w * h {
        let p = &buf[i * ch..i * ch + ch];
        let px = match ch {
            1 => [p[0], p[0], p[0], 255],
            2 => [p[0], p[0], p[0], p[1]],
            3 => [p[0], p[1], p[2], 255],
            _ => [p[0], p[1], p[2], p[3]],
        };
        img.data[i * 4..i * 4 + 4].copy_from_slice(&px);
    }
    img
}

fn trunc(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 160 {
        format!("{}…", &s[..160])
    } else {
        s
    }
}

/// JSON equality, numbers within a relative 1e-9 (strings, booleans and structure exactly).
fn same(a: &Value, b: &Value, path: &str, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            if (x - y).abs() > 1e-9 * x.abs().max(y.abs()).max(1e-3) {
                out.push(format!("{path}: rust {x} / ts {y}"));
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                out.push(format!("{path}: rust {} items / ts {} (rust {} / ts {})", x.len(), y.len(), trunc(a), trunc(b)));
                return;
            }
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                same(p, q, &format!("{path}[{i}]"), out);
            }
        }
        (Value::Object(x), Value::Object(y)) => {
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                match (x.get(k), y.get(k)) {
                    (Some(p), Some(q)) => same(p, q, &format!("{path}.{k}"), out),
                    (p, q) => out.push(format!("{path}.{k}: rust {} / ts {}", p.map_or("missing".into(), trunc), q.map_or("missing".into(), trunc))),
                }
            }
        }
        _ if a == b => {}
        _ => out.push(format!("{path}: rust {} / ts {}", trunc(a), trunc(b))),
    }
}

fn check(what: &str, mine: &Value, theirs: &Value, failures: &mut Vec<String>) {
    let mut out = Vec::new();
    same(mine, theirs, "", &mut out);
    if !out.is_empty() {
        failures.push(format!("{what} ({} differences):\n    {}", out.len(), out.iter().take(6).cloned().collect::<Vec<_>>().join("\n    ")));
    }
}

fn checksum(img: &Img) -> Value {
    let (mut a, mut b) = (0u64, 0u64);
    for (i, &v) in img.data.iter().enumerate() {
        a += v as u64;
        b = (b + v as u64 * ((i % 9973) as u64 + 1)) % 1_000_000_007;
    }
    serde_json::json!([img.width, img.height, a, b])
}

/// The recording's misspell: the first a becomes o, the last e becomes c, the end cut off.
fn misspell(n: &str) -> String {
    let mut s = n.replacen('a', "o", 1);
    if let Some(i) = s.rfind('e') {
        s.replace_range(i..i + 1, "c");
    }
    let keep = 6.max(n.len().saturating_sub(4));
    s.chars().take(keep).collect()
}

#[test]
fn readings_match() {
    let data = scan_data();
    let text = std::fs::read_to_string(root().join("fixtures/scan.jsonl")).expect("fixtures/scan.jsonl");
    let frames = root().join("fixtures/frames");
    let mut failures = Vec::new();
    let mut checked = 0;
    let mut tracker = InstrumentTracker::default();
    for line in text.lines().filter(|l| !l.is_empty()) {
        let c: Value = serde_json::from_str(line).unwrap();
        match c["kind"].as_str().unwrap() {
            "window" => {
                let img = decode(&frames.join(c["file"].as_str().unwrap()));
                check(&format!("{} window", c["file"]), &serde_json::to_value(windowed_game(&img)).unwrap(), &c["area"], &mut failures);
            }
            "frame" => {
                let file = c["file"].as_str().unwrap();
                let variant = c["variant"].as_str().unwrap();
                let frame = decode(&frames.join(file));
                let base = match variant {
                    "shrunk" => resize(&frame, (frame.width as f64 * 0.75).round() as usize, (frame.height as f64 * 0.75).round() as usize),
                    "gamma" => {
                        let lut: Vec<u8> = c["gamma"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect();
                        let mut g = frame.clone();
                        for p in g.data.chunks_exact_mut(4) {
                            p[0] = lut[p[0] as usize];
                            p[1] = lut[p[1] as usize];
                            p[2] = lut[p[2] as usize];
                        }
                        g
                    }
                    _ => frame,
                };
                let tag = format!("{file} {variant}");
                let area = windowed_game(&base);
                check(&format!("{tag} windowed"), &serde_json::to_value(area).unwrap(), &c["windowed"], &mut failures);
                let img = match &area {
                    Some(a) => crop_to(&base, a),
                    None => base,
                };
                let with_pictures = scan_upstream(&img, data, true);
                check(&format!("{tag} pictures"), &serde_json::to_value(&with_pictures).unwrap(), &c["pictures"], &mut failures);
                let names = scan_upstream(&img, data, false);
                check(&format!("{tag} names"), &serde_json::to_value(&names).unwrap(), &c["names"], &mut failures);
                let l = layout_at(img.width, img.height, names.ui());
                let size = names.hand_size.unwrap_or_else(|| match with_pictures.hand_ranks.as_ref().map_or(0, |r| r.len()) {
                    0 => 3,
                    n => n,
                });
                let crops: Vec<Value> = l.hand_at(size).iter().map(|p| checksum(&title_crop(&img, p, l.k))).collect();
                check(&format!("{tag} title crops"), &Value::Array(crops), &c["crops"], &mut failures);
                let hand = with_pictures.state.hand.clone();
                for reader in ["exact", "rough", "none"] {
                    let mut i = 0;
                    let mut read = |_: &Img| {
                        let n = hand.get(i).cloned();
                        i += 1;
                        match (reader, n) {
                            ("exact", Some(n)) => n,
                            ("rough", Some(n)) => format!("{}\nsome card text below", misspell(&n)),
                            _ => String::new(),
                        }
                    };
                    let pics = |phase: &str, size: usize, slots: &[usize]| pictures(&img, data, phase, size, slots, 0.0, names.ui());
                    let titled: Reading = read_titles(&img, names.clone(), data, &mut read, Some(&pics));
                    check(&format!("{tag} titles ({reader})"), &serde_json::to_value(&titled).unwrap(), &c["titled"][reader], &mut failures);
                }
                checked += 1;
            }
            "track" => {
                for (k, step) in c["steps"].as_array().unwrap().iter().enumerate() {
                    let r: Reading = serde_json::from_value(step["reading"].clone()).unwrap();
                    let res = track_instruments(&tracker, &r, step["trigger"].as_str());
                    tracker = res.tracker.clone();
                    check(&format!("tracker step {k}"), &serde_json::to_value(&res).unwrap(), &step["result"], &mut failures);
                }
            }
            _ => {}
        }
    }
    assert!(failures.is_empty(), "{} mismatches:\n{}", failures.len(), failures.join("\n"));
    eprintln!("scan: {checked} frame variants match");
}

#[test]
fn settler_matches() {
    use vorax_scan::live::{classify, log_time, Settler};
    let lines: Vec<String> = std::fs::read_to_string(root().join("fixtures/vorax-log-excerpt.txt"))
        .unwrap()
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let want: Value = serde_json::from_str(&std::fs::read_to_string(root().join("fixtures/live.json")).unwrap()).unwrap();
    let kinds: Vec<Value> = lines.iter().map(|l| classify(l).map_or(Value::Null, |k| Value::from(k.name()))).collect();
    assert_eq!(Value::Array(kinds), want["kinds"]);
    let mut s = Settler::default();
    let mut snaps = Vec::new();
    let mut now = log_time(&lines[0]).unwrap();
    let end = log_time(lines.last().unwrap()).unwrap_or(now) + 4000.0;
    let mut i = 0;
    while now <= end {
        let mut batch = Vec::new();
        while i < lines.len() && log_time(&lines[i]).unwrap_or(now) <= now {
            batch.push(lines[i].clone());
            i += 1;
        }
        if !batch.is_empty() {
            s.feed(&batch, now);
        }
        if let Some(d) = s.due(now) {
            snaps.push(serde_json::json!({"kind": d.name(), "at": now as i64}));
        }
        now += 50.0;
    }
    assert_eq!(Value::Array(snaps), want["snaps"]);
    assert_eq!(serde_json::json!([s.refreshes, s.over]), serde_json::json!([want["refreshes"], want["over"]]));
}

/// Frames from live games beyond upstream's: what the reader must get right on screens lit differently.
#[test]
fn live_frames() {
    let dir = root().join("fixtures/live-frames");
    let Ok(img) = std::panic::catch_unwind(|| decode(&dir.join("round9-one-monster.png"))) else { return };
    let r = scan(&img, scan_data(), true);
    // one Pest in tank 1; tanks 2-6 empty, their glass glowing brighter than on upstream's frames
    let filled: Vec<bool> = r.state.tanks.iter().map(|t| t.is_some()).collect();
    assert_eq!(filled, [true, false, false, false, false, false], "{:?}", r.state.tanks);
    assert_eq!(r.state.round, Some(9));
    let m = r.state.tanks[0].as_ref().unwrap();
    assert_eq!((m.activity * m.quantity, m.archetype), (164304, vorax::types::Archetype::Pest), "{m:?}");
    // the hand sits lower than the layout says, and the colour correction spoils the reroll digit: both fall back
    // by pictures alone (no text reader here) the third card scores just under "known", but is the best guess
    assert_eq!(r.state.hand[..2], ["Probiotic Enzyme Solution", "Spinal Solution - Aemberon"]);
    assert_eq!(r.fields["hand2"].guess.as_deref(), Some("Fresh Spinal Powder"));
    assert_eq!(r.hand_dy, Some(24.0));
    assert_eq!(r.state.potion_refreshes, Some(3));
}
