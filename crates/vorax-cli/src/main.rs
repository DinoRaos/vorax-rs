//! vorax: the Rust Vorax advisor and simulator from the command line. Read-only: it prints advice, it never
//! touches the game.
//!
//!   vorax advise [screen.json] [--futures N] [--quick] [--single]   advice for a screen (stdin when no file)
//!   vorax bench [fixtures-dir]                                      advice timings against the TypeScript's
//!   vorax sim [--games N] [--seed0 S] [--policy value|steer|solver] whole games, score stats
//!   vorax read <screenshot.png> [--advise]                          read a screenshot of the game (and advise on it)
//!   vorax info                                                      the data loaded (odds date, cards)
//!   vorax train [--games N] [--check M] [--seed0 S] [--eps E] --out model.json   fit a board value on simulated games
//!   vorax train [--games N] [--seed0 S] [--eps E] --rows boards.bin  the games' boards as raw rows, for decision trees
//!   vorax gate champion.txt challenger.txt                          a new board value against the one in use
//!   vorax sim --policy solver [--futures N] [--halving] --against champion.txt [--games MAX] [--p1 0.55]
//!       a sequential test on the champion's seeds: stops as soon as the challenger is clearly better or not
//! Data: what the build embedded (data/). --model <model.json|none> on any command uses another board value
//! (none: boards judged by their score); for train, it's the board value the training games are played by.

use std::io::Read;
use std::path::PathBuf;
use std::time::Instant;
use vorax::advisor::{self, ScreenState};
use vorax::bot::*;
use vorax::value::TARGET;

mod train;

fn arg<T: std::str::FromStr>(args: &[String], name: &str) -> Option<T> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    if let Some(n) = arg::<usize>(&args, "--threads") {
        rayon::ThreadPoolBuilder::new().num_threads(n).build_global().ok();
    }
    if let Some(m) = arg::<String>(&args, "--model") {
        let json = (m != "none").then(|| std::fs::read_to_string(&m).unwrap_or_else(|e| fail(&format!("{m}: {e}"))));
        vorax::data::use_model(json.as_deref()).unwrap_or_else(|e| fail(&e));
    }
    match cmd {
        "advise" => advise(&args[1..]),
        "bench" => bench(&args[1..]),
        "sim" => sim(&args[1..]),
        "info" => info(),
        "read" => read(&args[1..]),
        "train" => {
            let a = &args[1..];
            let games = arg(a, "--games").unwrap_or(100_000);
            if let Some(path) = arg::<String>(a, "--rows") {
                let seed0 = arg(a, "--seed0").unwrap_or(5_000_000);
                return train::rows(seed0, seed0 + games, arg(a, "--eps").unwrap_or(0.1), vorax::data().model.is_some(), &path);
            }
            let out: String = arg(a, "--out").unwrap_or_else(|| fail("train: --out model.json (or --rows boards.bin)"));
            train::train(games, arg(a, "--check").unwrap_or((games / 10).max(1000)), arg(a, "--seed0").unwrap_or(5_000_000), arg(a, "--eps").unwrap_or(0.1), vorax::data().model.is_some(), &out)
        }
        "gate" => match &args[1..] {
            [a, b, ..] => train::gate(a, b),
            _ => fail("usage: vorax gate champion.txt challenger.txt"),
        },
        _ => {
            eprintln!("usage: vorax advise [screen.json] [--futures N] [--quick] [--single] | bench [fixtures] | sim [--games N] [--seed0 S] [--policy value|steer|solver|oracle] [--futures N] [--scores out.txt] | info | train [--games N] --out model.json | gate a.txt b.txt   (--threads N, --model m.json|none on any)");
            std::process::exit(if cmd == "help" || cmd == "--help" { 0 } else { 2 });
        }
    }
}

fn info() {
    let d = vorax::data();
    println!("odds {}  ·  {} potions, {} instruments  ·  board value: {}", d.odds, d.potions.len(), d.instruments.len(), match &d.model {
        Some(vorax::data::ValueModel::Rounds { rounds }) => format!("linear, {} rounds × {} weights", rounds.len(), rounds[0].len()),
        Some(vorax::data::ValueModel::Trees { trees, .. }) => format!("{} trees", trees.len()),
        None => "none (score)".into(),
    });
}

fn advise(args: &[String]) {
    let file = args.iter().find(|a| !a.starts_with("--") && args.iter().position(|b| b == *a).is_none_or(|i| i == 0 || !args[i - 1].starts_with("--")));
    let mut text = String::new();
    match file {
        Some(f) => text = std::fs::read_to_string(f).unwrap_or_else(|e| fail(&format!("{f}: {e}"))),
        None => {
            std::io::stdin().read_to_string(&mut text).unwrap();
        }
    }
    let screen: ScreenState = serde_json::from_str(&text).unwrap_or_else(|e| fail(&format!("screen: {e}")));
    let par = !args.iter().any(|a| a == "--single");
    let advice = if args.iter().any(|a| a == "--quick") {
        advisor::advise(&screen, arg(args, "--samples").unwrap_or(16), false, par)
    } else {
        advisor::advise_plan(&screen, arg(args, "--futures"), par)
    };
    println!("{}", serde_json::to_string_pretty(&advice).unwrap());
}

fn fail(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(1)
}

fn bench(args: &[String]) {
    let dir = args.first().filter(|a| !a.starts_with("--")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("fixtures"));
    let text = std::fs::read_to_string(dir.join("advisor.jsonl")).unwrap_or_else(|e| fail(&format!("{}: {e}", dir.display())));
    let reps: u32 = arg(args, "--reps").unwrap_or(3);
    println!("{:<4} {:<10} {:>10} {:>12} {:>12} {:>8} {:>8}", "#", "phase", "TS 1 core", "Rust 1 core", "Rust all", "x1", "xall");
    let (mut ts_total, mut one_total, mut all_total) = (0.0, 0.0, 0.0);
    for (n, line) in text.lines().filter(|l| !l.is_empty()).enumerate() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let screen: ScreenState = serde_json::from_value(v["screen"].clone()).unwrap();
        let ts = v["jsMs"].as_f64().unwrap_or(f64::NAN);
        let time = |par: bool| {
            let mut best = f64::INFINITY;
            for _ in 0..reps {
                let t = Instant::now();
                std::hint::black_box(advisor::advise_plan(&screen, arg(args, "--futures"), par));
                best = best.min(t.elapsed().as_secs_f64() * 1000.0);
            }
            best
        };
        let (one, all) = (time(false), time(true));
        ts_total += ts;
        one_total += one;
        all_total += all;
        println!("{n:<4} {:<10} {:>8.0}ms {:>10.1}ms {:>10.1}ms {:>7.0}x {:>7.0}x", format!("{:?}", screen.phase), ts, one, all, ts / one, ts / all);
    }
    println!("{:<15} {:>8.0}ms {:>10.1}ms {:>10.1}ms {:>7.0}x {:>7.0}x  ({} threads)", "total", ts_total, one_total, all_total, ts_total / one_total, ts_total / all_total, rayon::current_num_threads());
}

fn sim(args: &[String]) {
    let games: u32 = arg(args, "--games").unwrap_or(100);
    let seed0: u32 = arg(args, "--seed0").unwrap_or(1_000_000);
    let policy: String = arg(args, "--policy").unwrap_or_else(|| "value".into());
    let (chooser, picker) = match policy.as_str() {
        // the player inside the advice's futures
        "value" => (Chooser::Ev(Policy { samples: 2, horizon: 0, roll_samples: None, steer: None, value: Some(true) }), Picker::Value),
        "steer" => (Chooser::Ev(Policy { samples: 4, horizon: 0, roll_samples: None, steer: Some(0.3), value: None }), Picker::Value),
        // the Solver's own advice at every move: look-ahead potions, board-aware instruments (slow: a full advice per move)
        "solver" => {
            // --futures N: futures per option for potions and instruments (the Solver's default: 64)
            let n: u32 = arg(args, "--futures").unwrap_or(advisor::INSTRUMENT_FUTURES);
            // --halving: the potions' futures by successive halving (the same number on average)
            let halving = args.iter().any(|a| a == "--halving").then_some(true);
            (
                Chooser::Plan(LookAhead { playouts: n, halving, ..advisor::LOOK_AHEAD }),
                Picker::Board { playouts: n, rollout: Policy { samples: 4, horizon: 0, roll_samples: None, steer: None, value: Some(true) }, goal: Goal::Target, refresh_playouts: n },
            )
        }
        // a cheater that sees the real upcoming cards: the ceiling no strategy can beat
        "oracle" => (Chooser::Oracle, Picker::Oracle),
        p => fail(&format!("unknown policy {p}")),
    };
    // --against champion.txt: a sequential test on the champion's seeds, stopping once the answer is clear
    if let Some(champion) = arg::<String>(args, "--against") {
        let out: Option<String> = arg(args, "--scores");
        // every seed the champion played unless --games says fewer
        let most = arg::<u32>(args, "--games").unwrap_or(u32::MAX);
        return train::duel(&champion, &chooser, &picker, most, arg(args, "--p1").unwrap_or(0.55), out.as_deref());
    }
    let t = Instant::now();
    let by_seed = train::scores(games, seed0, &chooser, &picker);
    let secs = t.elapsed().as_secs_f64();
    if let Some(path) = arg::<String>(args, "--scores") {
        let text: String = by_seed.iter().map(|(seed, score)| format!("{seed} {score}
")).collect();
        std::fs::write(&path, text).unwrap_or_else(|e| fail(&format!("{path}: {e}")));
    }
    let mut scores: Vec<i64> = by_seed.iter().map(|&(_, s)| s).collect();
    scores.sort();
    let n = scores.len() as f64;
    let mean = scores.iter().sum::<i64>() as f64 / n;
    let reached = scores.iter().filter(|&&s| s >= TARGET).count() as f64 / n * 100.0;
    let rainbow = scores.iter().filter(|&&s| s >= 1_120_000).count() as f64 / n * 100.0;
    println!(
        "{policy}: {games} games in {secs:.1}s ({:.1} games/s)  mean {mean:.0}  median {}  p10 {}  p90 {}  ≥{}k {reached:.1}%  ≥1.12M {rainbow:.1}%",
        n / secs,
        scores[scores.len() / 2],
        scores[(n * 0.1) as usize],
        scores[(n * 0.9) as usize],
        TARGET / 1000
    );
}

pub fn decode_png(path: &str) -> vorax_scan::Img {
    let file = std::fs::File::open(path).unwrap_or_else(|e| fail(&format!("{path}: {e}")));
    let mut dec = png::Decoder::new(file);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().unwrap_or_else(|e| fail(&format!("{path}: {e}")));
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap_or_else(|e| fail(&format!("{path}: {e}")));
    let (w, h) = (info.width as usize, info.height as usize);
    let ch = info.color_type.samples();
    let mut img = vorax_scan::Img::new(w, h);
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

fn read(args: &[String]) {
    let Some(path) = args.first() else { fail("usage: vorax read <screenshot.png> [--advise]") };
    let frame = decode_png(path);
    let data = vorax_scan::scan_data();
    let t = Instant::now();
    let img = match vorax_scan::window::windowed_game(&frame) {
        Some(a) => vorax_scan::window::crop_to(&frame, &a),
        None => frame,
    };
    let reading = vorax_scan::scan(&img, data, true);
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!("{}", serde_json::to_string_pretty(&reading.state).unwrap());
    let problems: Vec<String> = reading.problems.iter().map(|p| format!("{p:?}")).collect();
    eprintln!("read {}x{} in {ms:.0} ms{}", img.width, img.height, if problems.is_empty() { String::new() } else { format!("; problems: {}", problems.join(", ")) });
    if args.iter().any(|a| a == "--debug") {
        eprintln!("debug: {}", serde_json::to_string(&reading.debug).unwrap());
        for (k, f) in &reading.fields {
            eprintln!("  {k}: {}", serde_json::to_string(f).unwrap());
        }
    }
    if args.iter().any(|a| a == "--advise") {
        let a = advisor::advise_plan(&reading.state, None, true);
        println!("{}", serde_json::to_string_pretty(&a).unwrap());
        eprintln!("advised in {} ms", a.ms);
    }
}
