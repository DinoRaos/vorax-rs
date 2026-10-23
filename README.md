# Vorax overlay (Rust)

> **Unofficial fan project.** Not affiliated with or endorsed by the developers or publishers of Torchlight:
> Infinite. Game names and images belong to their owners. Use at your own risk.

A small Windows overlay for **Surgical Preparation**, the Vorax minigame of Torchlight: Infinite (SS11). It
follows your game, reads the board when it changes and marks the move the Solver advises: the card (or the
reroll) to take, the tanks to target, and how often that line reaches 840k.

![The overlay during a Vorax round: a cyan outline on the Anesthetic Tincture - Agito card, tank 2's glass glowing cyan, and the pill "Anesthetic Tincture - Agito → tank 2 · 33% reach 840k" at the top](docs/overlay.jpg)

Inspired by CoffeeBns's Tartarus Simulator and tidewarden's Vorax Solver (see [Credits](#credits)). Under the
overlay is a game engine, Solver and screen reader in Rust, about 130× faster on one core than the TypeScript
they started from.

**Read-only.** It reads the game window and the game's log file and draws on top. It never clicks or presses
keys, and it doesn't inject into or change the game: you make every move (see [AGENTS.md](AGENTS.md)). Changes
that add input automation won't be accepted.

## What it shows

- an outline on the card to take, or on the reroll button (cyan by default; five colours in the tray menu);
- a glowing outline on the glass of each tank to target;
- one small pill at the top with the move and how often it reaches 840k.

Nothing else is drawn, and nothing is drawn outside the Surgical Preparation screen. The marks only show while the
game is in front.

## Requirements

- **Windows 10 or 11.** The game borderless or windowed, not exclusive fullscreen.
- **The game's log turned on.** In the game: Esc → **Account Related** → **Enable Log**, then log out and back
  in. The overlay finds `UE_game.log` through Steam (or start it with `--log <path>`). The log only says *when*
  the board changes; everything else is read from the picture.

Card names are read with Windows' built-in text recognition: English when that language is installed, otherwise
your Windows language (tested with German). Without it, the pictures decide.

## Download

Get `vorax-overlay.exe` from the [latest release](https://github.com/DinoRaos/vorax-rs/releases/latest) and run
it; there is nothing to install. Windows may say "Windows protected your PC" because the program isn't signed:
click **More info** → **Run anyway**. The release notes list each file's SHA-256 if you want to check your
download.

The overlay doesn't go online: it advises with the data it was built with (see **Data** below), and a new
release brings any change.

## Build from source

You need [Rust](https://rustup.rs) (stable) and the Visual Studio Build Tools with "Desktop development with C++".

```powershell
git clone https://github.com/DinoRaos/vorax-rs.git
cd vorax-rs
cargo build --release                          # a few minutes the first time
.\target\release\vorax-overlay.exe
```

## Use

Start a Vorax game and the marks appear when the cards settle. Other ways to start it:

```powershell
.\target\release\vorax-overlay.exe --shelley 4   # your Intern Shelley level (default 6; remembered)
.\target\release\vorax-overlay.exe --selftest    # checks the log, the game window, a capture and the text reader
.\target\release\vorax-overlay.exe --demo        # a sample board's marks over the game for 15 s (switch to the game)
.\target\release\vorax-overlay.exe --preview shot.png out.png   # the marks drawn onto a saved screenshot
.\target\release\vorax-overlay.exe --frames D:\frames   # also saves every picture it reads (for reporting a misread)
```

| Key | |
|---|---|
| Ctrl+Alt+V | show or hide the overlay |
| Ctrl+Alt+X | read the board now |
| Ctrl+Alt+Q | quit |

<img src="docs/menu.png" alt="The tray menu: odds in use, game log status, Show overlay, Read the board now, Intern Shelley level 1-6, marker colour, Open the debug log, Quit" width="252" align="right">

The tray icon's menu has the same, plus your **Intern Shelley level** (1–6, remembered), the marks' colour, the
odds in use and the debug log (`%APPDATA%\VoraxRs\overlay.log`, with each board read and the advice given: attach
it when reporting a misread board).

When a card is still flying as the board is read, the overlay looks again a moment later (up to three times),
and hides its marks rather than leave ones for a board that's gone.

**Intern Shelley level.** It sets the instrument rerolls the advice counts on while they aren't on screen: a
potion board doesn't show them, so the overlay carries the count last read on an instrument offer, and before any
offer this game, the level's.

**Data.** The card odds, the board-value model (trained for the 840k target) and the screen reader's references
are built in (`data/`), as tli-vorax-simulator last published them on Oct 22, 2026.

## How it works

1. **Log** (`crates/vorax-scan/src/live.rs`): follows `UE_game.log` every 250 ms; its lines say when a new board
   has settled.
2. **Picture**: Windows Graphics Capture of the game window only, started for that board and stopped after,
   frames grabbed until two in a row look alike; cut to the game's client area; no mouse, and on Windows 11 no
   yellow border.
3. **Reading** (`crates/vorax-scan`): tanks, cards, round, rerolls, instruments and the instrument tracker, with
   card titles read by Windows' OCR and the pictures deciding where a title doesn't settle. About 0.5 s per board.
4. **Advice** (`crates/vorax`): the Solver's look-ahead on every core, about 5–25 ms.
5. **Overlay** (`crates/vorax-overlay`): a transparent, click-through, topmost window over the game's picture,
   shown only while the game is in front, redrawn only when the advice changes.

## Why Rust

tli-vorax-simulator was TypeScript in a web page (and an Electron app), which is right for a site anyone can open.
An overlay that sits next to the game all evening asks for something else:

- **Speed.** The Solver's look-ahead plays out many futures for every option. In TypeScript, advice on a hard
  board took up to 20 s on one core; here it takes 143 ms on one core and 20 ms on all of them, so it is ready
  the moment the cards land. Most of the gap is the game state: the TypeScript deep-copied it (with its log) on
  every action, while the Rust state is a fixed-size value that copies in a single memcpy, and
  [rayon](https://github.com/rayon-rs/rayon) spreads the futures over every core.
- **Reading in 0.5 s instead of about 5 s.** The same reading, compiled.
- **One small program.** A ~5 MB exe with no runtime, browser or Electron, idle while the board doesn't change.
  It uses what Windows already has: Windows Graphics Capture for the game window alone, Windows OCR for card
  titles, and a layered window for the marks.
- **Exact.** Rust leaves the order of float sums and random draws in our hands, so the engine matched the
  TypeScript bit for bit (`log1p` comes from `libm`, the same fdlibm algorithm V8 uses), and the tests keep it so.

## The `vorax` command

```powershell
.\target\release\vorax advise screen.json      # the Solver's advice for a screen (ScreenState JSON)
.\target\release\vorax advise --quick < s.json # this round's expected score only
.\target\release\vorax read shot.png --advise  # read a screenshot of the game (and advise on it)
.\target\release\vorax sim --games 1000 --policy value   # value | steer | solver
.\target\release\vorax bench fixtures          # advice timings against the TypeScript's
.\target\release\vorax info                    # the data in use
```

## Tests

`cargo test --release --workspace` replays traces recorded from tli-vorax-simulator's TypeScript (its last
open version, 7b4c9ba), so a change can't make the engine, Solver or reader play differently by accident:

- 300 random games (normal and sandbox starts, every instrument), 11,627 actions: the full state after every
  action, including the RNG state, board value and build progress;
- 60 boards: options, expected values (with rollouts, steering and the board value), choices, shortlists,
  potion and instrument futures, and the instrument picker;
- 8 screens: the Solver's full advice (64 futures per option) and its quick advice;
- 8 whole games played by the bots;
- the screen reader on 6 test frames, each as it is, shrunk to 3/4 and recoloured (every field, score, problem
  and ranking, the card titles with three stand-in text readers, and the instrument tracker through a game), and
  the log settler on a recorded game log.

Everything matches exactly, floats included. A deliberate change to how the game plays (new odds, a new target)
changes the traces too, so it needs new ones.

## Speed

Solver advice on the 8 recorded screens (12 threads):

| | Total | Per screen |
|---|---|---|
| TypeScript, 1 core (Node 24) | 75.0 s | 24 ms – 20.3 s |
| Rust, 1 core | 0.57 s | 0.1 – 143 ms |
| Rust, all cores | 0.08 s | 0.1 – 20 ms |

Whole games: 19,000 games/s with the board-value bot. A game with the Solver's full advice at every move takes
about 2 s on one core, or 0.2 s per game when 12 cores each play their own.

## Layout

| Path | What |
|---|---|
| `crates/vorax` | engine (`game.rs`, `context.rs`, `effects.rs`), board value (`value.rs`), bots (`bot.rs`), Solver (`advisor.rs`) |
| `crates/vorax-scan` | screen reader, instrument tracker and log settler (`scan.rs`, `titles.rs`, `track.rs`, `live.rs`) |
| `crates/vorax-overlay` | the overlay: `draw.rs` (the marks), `pipeline.rs`, and `app/` (capture, OCR, window, tray menu) |
| `crates/vorax-cli` | the `vorax` command |
| `data/` | cards, odds, board-value model; `data/scan/` the reader's references |
| `fixtures/` | what the tests replay: recorded traces, test screenshots and a game log |

## Credits

Inspired by:

- **CoffeeBns's [Tartarus Simulator](https://github.com/blahdot3h/tartarus-simulator)**
  ([twitch.tv/coffeebns](https://twitch.tv/coffeebns)): following the game's log and reading the game window,
  the approach this overlay follows. No code from it is used here.
- **tidewarden's [Vorax Solver](https://github.com/tidewarden/tli-vorax-simulator)**: the game engine, Solver,
  screen reader and data here began as a Rust port of its last open version (7b4c9ba, Oct 22, 2026, MIT), itself
  based on [Moebing's Vorax simulator](https://github.com/Moebing/Torchlight-Infinite-SS11-Vorax-simulator)
  (MIT). Nothing from it after that version is used.

## License

[MIT](LICENSE): use, copy, change and share it freely, keeping the copyright and license notice. The notice keeps
tidewarden's and Moebing's copyright lines, as their MIT license requires for work built on theirs.
