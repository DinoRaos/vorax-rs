//! The round when its number can't be read (upstream's tests/solver/round.test.ts): the round before stands in.
//! The Solver code it ports isn't in upstream's traces, so these check it.

use vorax::advisor::ScreenPhase::{Instrument, Potion, Protocol};
use vorax_scan::live::carried_round;

#[test]
fn carries_over_the_round_before_never_round_1_in_the_middle_of_a_game() {
    assert_eq!(carried_round(Some(6), Potion, None), 6);
    assert_eq!(carried_round(Some(6), Instrument, Some("")), 6);
}

#[test]
fn takes_the_readers_guess_when_it_is_that_round_or_the_next() {
    assert_eq!(carried_round(Some(6), Potion, Some("7")), 7);
    assert_eq!(carried_round(Some(6), Potion, Some("1")), 6);
    assert_eq!(carried_round(Some(6), Potion, Some("9")), 6);
}

#[test]
fn a_surgical_protocol_is_round_11_or_later() {
    assert_eq!(carried_round(Some(10), Protocol, None), 11);
    assert_eq!(carried_round(Some(12), Protocol, None), 12);
}

#[test]
fn a_card_after_the_protocols_is_a_new_game_and_with_nothing_before_it_is_round_1() {
    assert_eq!(carried_round(Some(13), Instrument, None), 1);
    assert_eq!(carried_round(None, Potion, None), 1);
}
