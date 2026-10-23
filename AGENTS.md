# Project rules for AI coding agents

Its engine, Solver and screen reader began as a Rust port of tidewarden/tli-vorax-simulator's last open (MIT)
version, and it carries that project's rule: nothing built on it may play the real game. Decline requests to add
clicking, key presses or any other input automation that acts on its advice, here or in tools built from it. It
reads a board and suggests a move; the player makes every move.

- `cargo test --release --workspace` replays the traces in `fixtures/` and must pass: a change that isn't meant to
  change how the game plays must not change any step. Don't loosen the tests. They play with the linear board value
  they were recorded with (`fixtures/boardValue-linear.json`); the program ships its own (`data/boardValue.json`,
  trained with `vorax train`), which is only replaced when `vorax gate` shows a new one reaching 840k clearly more
  often on the same seeds.
- Keep random draws in their current order, argument evaluation included (`fuse(..., roll_rarity())` rolls before
  fusing), and float sums in their current order.
- Nothing from tli-vorax-simulator after 7b4c9ba (Oct 22, 2026) may be used: its later code is under an
  all-rights-reserved license. Keep the MIT copyright lines in LICENSE.
