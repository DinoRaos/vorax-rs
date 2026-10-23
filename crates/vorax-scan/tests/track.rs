//! The instrument tracker when more than one icon appears between two boards: in round 1 a boss drop from the map
//! (an instrument owned from the start) and the first pick, or two strong cards reaching the first reward at once.
//! The instruments already known keep the rounds they were picked in, and the new ones date from now.

use serde_json::json;
use vorax_scan::reading::Reading;
use vorax_scan::track::{track_instruments, InstrumentTracker};

fn board(round: i32, phase: &str, instruments: &[&str], hand: &[&str]) -> Reading {
    serde_json::from_value(json!({
        "state": { "tanks": [null, null, null, null, null, null], "hand": hand, "instruments": instruments, "phase": phase, "round": round },
        "fields": {},
        "problems": [],
    }))
    .unwrap()
}

#[test]
fn two_instruments_appearing_at_once_in_round_1_both_date_from_round_1() {
    let offer = ["Hypertrophic Pituitary", "Chrysalis Specimen", "Adherent Metatarsals"];
    let t = track_instruments(&InstrumentTracker::default(), &board(1, "instrument", &[], &offer), Some("screen"));
    let t = track_instruments(&t.tracker, &board(1, "potion", &["Adherent Metatarsals", "Chrysalis Specimen"], &["Xeno-Hormone"]), Some("board"));
    assert_eq!(t.instruments, ["Adherent Metatarsals", "Chrysalis Specimen"]);
    assert_eq!(t.rounds, [1, 1]);
}

#[test]
fn instruments_known_keep_their_rounds_when_two_more_appear() {
    let offer = ["Aberrant Sprout", "Prismatic Liver"];
    let t = track_instruments(&InstrumentTracker::default(), &board(1, "instrument", &[], &offer), Some("screen"));
    let t = track_instruments(&t.tracker, &board(1, "potion", &["Aberrant Sprout"], &["Xeno-Hormone"]), Some("board"));
    assert_eq!(t.rounds, [1]);
    // two picks between the boards of round 4: both date from round 4
    let t = track_instruments(&t.tracker, &board(4, "potion", &["Aberrant Sprout", "Fouled Scraper", "Clustered Eggs"], &["Xeno-Hormone"]), Some("board"));
    assert_eq!(t.instruments, ["Aberrant Sprout", "Fouled Scraper", "Clustered Eggs"]);
    assert_eq!(t.rounds, [1, 4, 4]);
}
