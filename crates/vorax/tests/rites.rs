//! The Holy Water rites (upstream's tests/strategy/advisor.test.ts, issue #13): Supreme potions whose effect is
//! another card later, always the advice like a Xenopanacea. Upstream's traces don't offer one, so these check it.

use serde_json::json;
use vorax::advisor::{self, Best, ScreenState};

fn board(hand: &[&str]) -> ScreenState {
    let tank = json!({ "archetype": "pest", "rarity": "magic", "activity": 81, "quantity": 340 });
    serde_json::from_value(json!({ "tanks": [tank, tank, tank, tank, tank, tank], "hand": hand, "instruments": [], "phase": "potion", "round": 9 })).unwrap()
}

#[test]
fn a_rite_is_always_the_advice() {
    let a = advisor::advise(&board(&["Mutagenic Powder", "Bewitching Tincture", "First Rite Holy Water: Confession"]), 2, false, false);
    assert!(matches!(&a.best, Best::Card(r) if r.index == 2), "{:?}", a.best);
    assert!(a.note.as_deref().is_some_and(|n| n.starts_with("First Rite Holy Water: Confession: always take it")), "{:?}", a.note);
    assert!(a.unknown.is_empty());
}

#[test]
fn purest_holy_water_is_simulated() {
    assert!(!advisor::is_always_take("Purest Holy Water"));
    assert!(advisor::is_always_take("Ritual Holy Water: Flagellation"));
    assert!(advisor::is_always_take("Xenopanacea: Brood"));
}
