//! vorax: the Rust Vorax advisor and simulator from the command line. Read-only: it prints advice, it never
//! touches the game.
//!
//!   vorax advise [screen.json] [--futures N] [--quick] [--single]   advice for a screen (stdin when no file)
//!   vorax bench [fixtures-dir]                                      advice timings against the TypeScript's
//!   vorax sim [--games N] [--seed0 S] [--policy value|steer|solver] whole games, score stats
//!   vorax read <screenshot.png> [--advise]                          read a screenshot of the game (and advise on it)
//!   vorax info                                                      the data loaded (odds date, cards)
//! Data: the build's embedded export, or VORAX_DATA=<dir> (tools/export-data.ts output) for a newer one.

use std::io::Read;
use std::path::PathBuf;
use std::time::Instant;
use vorax::advisor::{self, ScreenState};
use vorax::bot::*;
use vorax::game::ultimate_activity;
use vorax::value::TARGET;

fn arg<T: std::str::FromStr>(args: &[String], name: &str) -> Option<T> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    if let Some(n) = arg::<usize>(&args, "--threads") {
        rayon::ThreadPoolBuilder::new().num_threads(n).build_global().ok();
    }
    match cmd {
        "advise" => advise(&args[1..]),
        "bench" => bench(&args[1..]),
        "sim" => sim(&args[1..]),
        "info" => info(),
        "read" => read(&args[1..]),
        _ => {
            eprintln!("usage: vorax advise [screen.json] [--futures N] [--quick] [--single] | bench [fixtures] | sim [--games N] [--seed0 S] [--policy value|steer|solver] | info   (--threads N on any)");
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
                std::hint::black_box(advisor::advise_plan(&screen, None, par));
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
        "solver" => (
            Chooser::Plan(advisor::LOOK_AHEAD),
            Picker::Board { playouts: advisor::INSTRUMENT_FUTURES, rollout: Policy { samples: 4, horizon: 0, roll_samples: None, steer: None, value: Some(true) }, goal: Goal::Target, refresh_playouts: advisor::INSTRUMENT_FUTURES },
        ),
        p => fail(&format!("unknown policy {p}")),
    };
    let t = Instant::now();
    use rayon::prelude::*;
    let mut scores: Vec<i64> = (seed0..seed0 + games).into_par_iter().map(|seed| ultimate_activity(&run_game(seed, &chooser, &picker, None))).collect();
    let secs = t.elapsed().as_secs_f64();
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
