//! The instrument panel holds exactly the icons it shows, on any screen. Empty slots over the lit scenery once read
//! as icons (4 instruments where 3 were held at 1080p, Oct 23, 2026), so the advice planned for a build the player
//! didn't have. The panels are cut from real games at 1080p (fixtures/instrument-panels) and read as they are and
//! scaled to other screens, kept at the top right where the game anchors them.
//!
//! Which instrument an icon is, the tracker settles among the cards just offered (track.rs): the panel alone can
//! mistake one small icon for another. So each slot is checked the way the tracker reads it: of the instruments
//! offered at that pick, the one taken must score highest.

use std::path::PathBuf;
use vorax_scan::instruments::read_instruments;
use vorax_scan::layout::layout_at;
use vorax_scan::pixels::{resize, Img};
use vorax_scan::scan_data;

/// The offers in the games the panels come from, and the card taken (first) from each.
const INCUBATION: [&str; 3] = ["Incubation Sac", "Chrysalis Specimen", "Hyperplastic Frontal Lobe"];
const FOULED: [&str; 3] = ["Fouled Scraper", "Sloughed Cortex", "Rawhide Restraint"];
const CANINE: [&str; 3] = ["Canine Teeth File", "Tanned Leather Restraint", "Fouled Scraper"];
const ADHERENT: [&str; 3] = ["Adherent Metatarsals", "Chrysalis Specimen", "Contracted Claws"];
const GARROTE: [&str; 3] = ["Sinew Garrote", "Vermicular Spinal Cord", "Clustered Eggs"];
const FOULED_2: [&str; 3] = ["Fouled Scraper", "Tanned Leather Restraint", "Pig Iron Bone Saw"];

/// Each panel and its slots, oldest first: the offer each instrument was picked from.
const PANELS: [(&str, &[[&str; 3]]); 8] = [
    ("0-none", &[]),
    ("1-incubation-sac", &[INCUBATION]),
    ("1-incubation-sac-r3", &[INCUBATION]),
    ("2-incubation-sac-fouled-scraper", &[INCUBATION, FOULED]),
    ("3-incubation-sac-fouled-scraper-canine-teeth-file", &[INCUBATION, FOULED, CANINE]),
    ("1-adherent-metatarsals", &[ADHERENT]),
    ("2-adherent-metatarsals-sinew-garrote", &[ADHERENT, GARROTE]),
    ("3-adherent-metatarsals-sinew-garrote-fouled-scraper", &[ADHERENT, GARROTE, FOULED_2]),
];

const SCREENS: [(usize, usize, &str); 6] = [
    (1920, 1080, "1080p"),
    (2560, 1440, "1440p"),
    (3840, 2160, "4K"),
    (3440, 1440, "ultrawide 1440p"),
    (1600, 900, "900p"),
    (1280, 720, "720p"),
];

fn decode(path: &PathBuf) -> Img {
    let dec = png::Decoder::new(std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())));
    let mut reader = dec.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    let (w, h) = (info.width as usize, info.height as usize);
    let ch = info.color_type.samples();
    let mut img = Img::new(w, h);
    for i in 0..w * h {
        let p = &buf[i * ch..i * ch + ch];
        let rgb = if ch >= 3 { [p[0], p[1], p[2]] } else { [p[0], p[0], p[0]] };
        img.data[i * 4..i * 4 + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
    }
    img
}

/// The panel (cut from a 1080p screen) on a black `w`×`h` screen: scaled with the height, at the top right.
fn screen(panel: &Img, w: usize, h: usize) -> Img {
    let k = h as f64 / 1080.0;
    let p = resize(panel, (panel.width as f64 * k).round() as usize, (panel.height as f64 * k).round() as usize);
    let mut img = Img::new(w, h);
    for y in 0..p.height.min(h) {
        for x in 0..p.width.min(w) {
            let (from, to) = ((y * p.width + x) * 4, (y * w + w - p.width + x) * 4);
            img.data[to..to + 4].copy_from_slice(&p.data[from..from + 4]);
        }
    }
    img
}

#[test]
fn the_panel_holds_exactly_the_instruments_shown_on_any_screen() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/instrument-panels");
    let mut wrong = Vec::new();
    for (name, slots) in PANELS {
        let panel = decode(&dir.join(format!("{name}.png")));
        for (w, h, label) in SCREENS {
            let read = read_instruments(&screen(&panel, w, h), &layout_at(w, h, 1.0), scan_data());
            let at = format!("{name} at {label} ({w}x{h})");
            if read.names.len() != slots.len() {
                wrong.push(format!("{at}: {} icons read {:?}, {} shown", read.names.len(), read.names, slots.len()));
                continue;
            }
            for (i, offer) in slots.iter().enumerate() {
                let score = |n: &str| read.ranks[i].iter().find(|(k, _)| k == n).map_or(f64::NEG_INFINITY, |x| x.1);
                let best = offer.iter().copied().max_by(|a, b| score(a).total_cmp(&score(b))).unwrap();
                if best != offer[0] {
                    wrong.push(format!("{at}: slot {i} of the offer {offer:?} scores {best} highest, {} was taken", offer[0]));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "the instrument panel misread:\n  {}", wrong.join("\n  "));
}
